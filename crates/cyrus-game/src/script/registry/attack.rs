//! Receiver-owned attack execution through the four defense slots and shared animation owner.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::attack_step_with_animation;
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Apply the verified d001 response core after message intent clears. This slice
    /// owns displacement, positional impact sound, taunt, wrapping strength/armor
    /// damage arithmetic and contact-death selector routing. Animation admission
    /// remains in the playback follow-up.
    pub(super) fn process_hit_response_core(
        &mut self,
        receiver: ScriptActorId,
        sender: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let receiver_index = self.resolve(receiver)?;
        let sender_index = self.resolve(sender)?;
        let from = self.spatial(sender)?.local.position;
        let origin = self.spatial(receiver)?.local.position;
        let heading = super::super::spatial::query_bearing(
            origin[0].wrapping_sub(from[0]),
            origin[2].wrapping_sub(from[2]),
        );
        let moved = cyrus_world::transform::translate_forward(origin, heading, 3000)?;
        self.spatial
            .as_mut()
            .context("hit response spatial state unavailable")?
            .actor_mut(receiver_index)?
            .local
            .position = moved;
        let effect = i32::from(self.actors[receiver_index].attributes[86]);
        let rate_offset_hz = random.rnd(128)?.wrapping_mul(16).wrapping_sub(1024);
        sounds(
            receiver,
            crate::script::ScriptSound::Positional {
                effect,
                volume: 0,
                rate_offset_hz,
                position: self.sight_position(receiver_index)?,
            },
        )?;
        self.request_hurt_combat_taunt(receiver, inputs, random, sounds)?;
        let (strength, damage) = {
            let combat = self.actors[sender_index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("hit sender combat unavailable")?;
            let motion = self.current_combat_motion(sender)?;
            (
                combat.strength,
                motion.damage.context("hit motion damage unavailable")?,
            )
        };
        let armor = self.actors[receiver_index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("hit receiver combat unavailable")?
            .armor;
        let product = (strength.wrapping_add(100)).wrapping_mul(damage);
        let scaled = product.wrapping_div(100);
        let mitigated = scaled.wrapping_mul(armor).wrapping_div(100);
        let change = (scaled.wrapping_sub(mitigated)).wrapping_neg();
        let health = i32::from(self.actors[receiver_index].attributes[3])
            .wrapping_add(change)
            .clamp(0, 255);
        self.actors[receiver_index].attributes[3] = health as u8;
        if health < 1 {
            self.dispatch_contact_death(receiver, 0, inputs.nominal_ticks, sounds)?;
        }
        Ok(())
    }
    /// The direct-damage response (`0xd005`) for a nonplayer receiver: displace it 3000 units away
    /// from the sender, start its hit-reaction group (117) when it has animation, subtract the
    /// sender's attribute 96 from its health (clamped to a byte) and, when that leaves it below
    /// one, request receiver taunt category six and run contact death with selector zero. The
    /// selected player's branch (death-animation override, weapon and stance reaction groups,
    /// damage suppression, shared additive health) is unsupported and an explicit error.
    pub(super) fn process_direct_damage_response(
        &mut self,
        receiver: ScriptActorId,
        sender: ScriptActorId,
        inputs: super::ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let receiver_index = self.resolve(receiver)?;
        let sender_index = self.resolve(sender)?;
        anyhow::ensure!(
            self.selected_player_index() != Some(receiver_index),
            "direct-damage response for the selected player unsupported"
        );
        let from = self.spatial(sender)?.local.position;
        let origin = self.spatial(receiver)?.local.position;
        let heading = super::super::spatial::query_bearing(
            origin[0].wrapping_sub(from[0]),
            origin[2].wrapping_sub(from[2]),
        );
        let moved = cyrus_world::transform::translate_forward(origin, heading, 3000)?;
        self.spatial
            .as_mut()
            .context("direct damage spatial state unavailable")?
            .actor_mut(receiver_index)?
            .local
            .position = moved;
        if self.actors[receiver_index].animation.is_some() {
            let ticks = inputs
                .nominal_ticks
                .context("direct damage nominal ticks unavailable")?;
            self.request_ordinary_group(receiver, 117, false, ticks, sounds)?;
        }
        let damage = i32::from(self.actors[sender_index].attributes[96]);
        let health = i32::from(self.actors[receiver_index].attributes[3])
            .wrapping_sub(damage)
            .clamp(0, 255);
        self.actors[receiver_index].attributes[3] = health as u8;
        if health < 1 {
            self.request_combat_taunt(receiver, 6, 0, inputs, random, sounds)?;
            self.dispatch_contact_death(receiver, 0, inputs.nominal_ticks, sounds)?;
        }
        Ok(())
    }

    /// Read attack-processing admission without advancing playback or marking contact.
    /// Reverse playback rejects processing before intent or motion queries. The signed
    /// command cursor admits the start-inclusive/end-exclusive active
    /// window, or a nonzero-duration extended window with combat flag 0x20 clear.
    /// Current-motion fields remain required; missing numeric metadata has no default.
    /// Follows preservation's SOUP attack-processing admission rules.
    pub fn combat_attack_window(&self, id: ScriptActorId) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let actor = &self.actors[self.resolve(id)?];
            let Some(combat) = actor
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
            else {
                return Ok(false);
            };
            if matches!(combat.action, -1 | 9) {
                return Ok(false);
            }
            let animation = actor
                .animation
                .as_ref()
                .context("receiver animation unavailable")?;
            if animation.playback.is_reversed() || combat.flags & 1 == 0 {
                return Ok(false);
            }
            let cursor = animation
                .playback
                .current_command_cursor()?
                .context("receiver animation command cursor unavailable")?;
            let motion = self.current_combat_motion(id)?;
            let start = motion.first_collide_frame;
            let duration = motion.collide_duration;
            let end = start.wrapping_add(duration);
            if start <= cursor && cursor < end {
                return Ok(true);
            }
            Ok(duration != 0 && cursor >= end && combat.flags & 0x20 == 0)
        })();
        result.with_context(|| format!("world {} instance {} attack window", self.world, id.index))
    }

    /// Query the current motion's defence contact window without changing combat state.
    /// The signed command cursor includes `first_defend_frame` and excludes the
    /// wrapping end. A cursor before the start does not require duration metadata.
    /// This query has no attack-intent, contact-mark or reverse-direction gate.
    /// Follows preservation's SOUP defence contact window contract.
    pub fn combat_defence_window(&self, id: ScriptActorId) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let actor = &self.actors[self.resolve(id)?];
            let cursor = actor
                .animation
                .as_ref()
                .context("receiver animation unavailable")?
                .playback
                .current_command_cursor()?
                .context("receiver animation command cursor unavailable")?;
            let motion = self.current_combat_motion(id)?;
            let start = motion
                .first_defend_frame
                .context("first_defend_frame unavailable")?;
            if cursor < start {
                return Ok(false);
            }
            let duration = motion
                .defend_duration
                .context("defend_duration unavailable")?;
            Ok(cursor < start.wrapping_add(duration))
        })();
        result.with_context(|| format!("world {} instance {} defence window", self.world, id.index))
    }

    /// Traverse the receiver-bound actor candidates in retained collision order.
    /// Defended candidates skip damage contact; the first combat hit consumes intent,
    /// sends the hit message and requests the attacker taunt before returning.
    /// Unowned static models query retained tip/base endpoints with enabled sidedness
    /// and request ordered clank cues. Actor-owned faces use independent actor
    /// contact, then impact, additive health and caller-owned death dispatch.
    /// Selected-player health and noncombat actor-list responses remain unsupported.
    pub(super) fn process_combat_attack_candidates(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        use cyrus_collision::candidates::CandidateOwner;
        let geometry = inputs
            .locomotion_geometry
            .context("attack candidate geometry unavailable")?;
        let ground = geometry.ground;
        self.require_collision_query(id, ground.prepared, ground.scene, ground.animations)?;
        for candidate in &ground.prepared.candidates.actors {
            let CandidateOwner::Actor(index) = candidate.owner else {
                continue;
            };
            let defender = self.id(index)?;
            if self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some()
                && self.try_combat_hit_defence(
                    defender,
                    id,
                    ground.animations,
                    ground.scene.actor_collision.as_ref(),
                    inputs,
                    random,
                    sounds,
                )?
            {
                continue;
            }
            if !self.combat_motion_contact(
                id,
                defender,
                ground.animations,
                ground.scene.actor_collision.as_ref(),
            )? {
                continue;
            }
            anyhow::ensure!(
                self.actors[index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .is_some(),
                "noncombat attack contact response unsupported"
            );
            self.actors[id.index]
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat")
                .flags &= !3;
            self.send_combat_message(id, defender, 0xd001)?;
            let clock = inputs
                .combat_clock_sample
                .context("combat clock sample unavailable")? as i32;
            self.request_timed_combat_taunt(id, 3, clock, inputs, random, sounds)?;
            return Ok(());
        }
        for candidate in &ground.prepared.candidates.faces {
            let endpoints = self
                .equipped_weapon(id)?
                .context("retained weapon endpoints unavailable")?
                .endpoints;
            let single = cyrus_collision::candidates::CollisionCandidates {
                faces: vec![*candidate],
                actors: Vec::new(),
            };
            let models = ground
                .scene
                .actor_collision
                .as_ref()
                .context("attack live geometry unavailable; re-export world")?;
            let mut poses = std::collections::BTreeMap::new();
            if let CandidateOwner::Actor(index) = candidate.owner {
                let record = self.model_presentation(self.id(index)?)?.with_context(|| {
                    format!("face candidate actor {index}: stored model pose unavailable")
                })?;
                poses.insert(index, record.collision_pose()?);
            }
            let contacts = ground.scene.static_collision.runtime_segment_contacts(
                &single,
                models,
                &poses,
                endpoints[1],
                endpoints[0],
                true,
                None,
            )?;
            if let CandidateOwner::Actor(index) = candidate.owner {
                let defender = self.id(index)?;
                if !self.combat_motion_contact(id, defender, ground.animations, Some(models))? {
                    continue;
                }
                let target = &self.actors[index];
                if target
                    .control
                    .as_ref()
                    .context("face target control unavailable")?
                    .death_selector
                    == 8
                {
                    continue;
                }
                if geometry.environment.selected_player == Some(defender)
                    && self
                        .globals
                        .selected_health()
                        .context("selected hit-inhibition owner unavailable")?
                        .hit_inhibited()
                {
                    continue;
                }
                let attacker = &self.actors[id.index];
                let control = attacker
                    .control
                    .as_ref()
                    .context("face attacker control unavailable")?;
                let holding = super::super::activation::weapon_predicates(
                    control.living_state,
                    attacker
                        .animation
                        .as_ref()
                        .map(|a| super::super::ActivationAnimation {
                            group: a.playback.current_group(),
                        }),
                    control.combat.is_some(),
                    control.weapon_drawn,
                    attacker.attributes[110],
                    geometry.environment.item_system_ready,
                    geometry.environment.shell_execution,
                )
                .0;
                if attacker.attributes[97] != 0
                    && !holding
                    && control
                        .combat
                        .as_ref()
                        .is_none_or(|combat| combat.flags & 4 == 0)
                {
                    continue;
                }
                let effect = i32::from(attacker.attributes[86]);
                let rate_offset_hz = random.rnd(128)?.wrapping_mul(16).wrapping_sub(1024);
                sounds(
                    id,
                    crate::script::ScriptSound::Positional {
                        effect,
                        volume: 0,
                        rate_offset_hz,
                        position: self.sight_position(id.index)?,
                    },
                )?;
                if self.actors[index].attributes[3] != 0 {
                    let damage = if control
                        .combat
                        .as_ref()
                        .is_some_and(|combat| combat.retained_action_indices[1] < 0)
                    {
                        0
                    } else {
                        self.current_combat_motion(id)?
                            .damage
                            .context("contact motion damage unavailable")?
                    };
                    if geometry.environment.selected_player == Some(defender) {
                        self.apply_selected_contact_health(
                            defender,
                            damage.wrapping_neg(),
                            inputs,
                            random,
                            sounds,
                        )?;
                    } else {
                        self.apply_nonplayer_contact_health(
                            defender,
                            damage.wrapping_neg(),
                            geometry.environment,
                        )?;
                    }
                    if self.actors[index].attributes[3] == 0 {
                        self.dispatch_contact_death(defender, 0, inputs.nominal_ticks, sounds)?;
                    }
                }
                continue;
            }
            if contacts.is_empty() {
                continue;
            }
            let effect = self.combat_clank_effects[0].context("sword_clank_1 unavailable")?;
            for _ in 0..if candidate.candidate_type == 1 { 2 } else { 1 } {
                let rate_offset_hz = random.rnd(128)?.wrapping_mul(16).wrapping_sub(1024);
                let position = self.sight_position(id.index)?;
                sounds(
                    id,
                    crate::script::ScriptSound::Positional {
                        effect,
                        volume: 0,
                        rate_offset_hz,
                        position,
                    },
                )?;
            }
        }
        Ok(())
    }

    /// Additive contact health follows preservation's contact-health rules.
    /// Death dispatch belongs to the contact caller, including after rejected changes.
    pub fn apply_nonplayer_contact_health(
        &mut self,
        id: ScriptActorId,
        change: i32,
        environment: super::transition::ObstacleEnvironment,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        if actor.attributes[120] != 0 {
            return Ok(());
        }
        let control = actor
            .control
            .as_ref()
            .context("contact health control unavailable")?;
        let holding = super::super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|animation| super::super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
            control.combat.is_some(),
            control.weapon_drawn,
            actor.attributes[110],
            environment.item_system_ready,
            environment.shell_execution,
        )
        .0;
        if actor.attributes[97] != 0
            && !holding
            && control
                .combat
                .as_ref()
                .is_none_or(|combat| combat.flags & 4 == 0)
        {
            return Ok(());
        }
        anyhow::ensure!(
            environment.selected_player != Some(id),
            "selected contact health unavailable"
        );
        self.actors[index].attributes[3] =
            super::falling::ordinary_health_value(actor.attributes[3], change);
        Ok(())
    }

    /// Contact death with a nonzero selector skips group 122 and checks group 121.
    /// Missing groups still assign death selector eight. Group defaults and walk-node
    /// release follow preservation's contact-death animation dispatch contract.
    pub fn dispatch_contact_death(
        &mut self,
        id: ScriptActorId,
        selector: i32,
        ticks: Option<u32>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let group = self.actors[index].animation.as_ref().and_then(|animation| {
            [122, 121]
                .into_iter()
                .filter(|group| selector == 0 || *group != 122)
                .find(|group| animation.playback.has_group(*group))
        });
        if let Some(group) = group {
            let ticks = ticks.context("contact death nominal ticks unavailable")?;
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .expect("validated contact death animation");
            let frame =
                animation
                    .playback
                    .force_request_with_sounds(group, 0, ticks, &mut |sound| {
                        if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                            return event_context.dispatch(
                                flags,
                                event_camera.as_mut(),
                                event_effects,
                            );
                        }
                        sounds(
                            id,
                            crate::script::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    })?;
            animation.frame = frame.or(animation.frame);
            if actor
                .control
                .as_ref()
                .is_some_and(|control| control.combat.is_some())
            {
                self.release_held_walk_nodes(id)?;
                self.actors[index]
                    .control
                    .as_mut()
                    .expect("validated death control")
                    .combat_death_default = Some(if group == 122 { 124 } else { 123 });
            }
        }
        self.actors[index]
            .control
            .as_mut()
            .context("contact death control unavailable")?
            .death_selector = 8;
        Ok(())
    }

    /// Execute a caller-selected attack move synchronously, without allocating a VM task.
    /// The pending move selects one of this receiver's four defense slots. Animation and
    /// sound services precede combat start writes; prior service writes survive errors.
    /// Idle cleanup avoids metadata and timing queries. This does not schedule combat.
    pub fn execute_combat_attack(
        &mut self,
        id: ScriptActorId,
        requested: i32,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let index = self.resolve(id)?;
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let pending = combat.retained_action_indices[0];
            if combat.decision.attack_state == 0 && requested == -1
                || combat.decision.attack_state != 0 && pending == -1
            {
                return Ok(false);
            }
            let slot = usize::try_from(pending).context("negative pending attack move")?;
            let record = combat
                .defense_indices
                .get(slot)
                .copied()
                .flatten()
                .with_context(|| format!("defense slot {slot}: unpopulated"))?;
            let group = self
                .combat_defenses
                .get(usize::from(record))
                .with_context(|| format!("defense slot {slot} record {record}: unavailable"))?
                .animation_group;
            let actor = &self.actors[index];
            let current = actor
                .animation
                .as_ref()
                .context("receiver animation unavailable")?
                .playback
                .current_group()
                .map(i32::from);
            let starting = combat.decision.attack_state == 0 && current != Some(group);
            let timing = if starting {
                Some(inputs.nominal_ticks.context("nominal ticks unavailable")?)
            } else {
                None
            };
            let position = if starting {
                Some(self.sight_position(index)?)
            } else {
                None
            };
            let event_context = super::effects::ActorEventContext::capture(self, id);
            let event_camera = &mut self.camera;
            let event_effects = &mut self.effects;
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor.animation.as_mut().expect("admitted animation");
            let combat = actor
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            let step = attack_step_with_animation(
                &mut combat.decision,
                &mut combat.action,
                &mut combat.retained_action_indices[0],
                requested,
                current,
                || Some(group),
                &mut |group| {
                    let group =
                        u16::try_from(group).context("animation group outside u16 range")?;
                    let frame = animation.playback.force_request_with_sounds(
                        group,
                        0,
                        timing.expect("start timing"),
                        &mut |sound| {
                            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                                return event_context.dispatch(
                                    flags,
                                    event_camera.as_mut(),
                                    event_effects,
                                );
                            }
                            sounds(
                                id,
                                crate::script::ScriptSound::Animation {
                                    sound,
                                    surface,
                                    position,
                                },
                            )
                        },
                    )?;
                    animation.frame = frame.or(animation.frame);
                    animation.playback.set_loop_count(1)?;
                    animation.playback.clear_pending_request();
                    Ok(())
                },
            )?;
            Ok(step.active)
        })();
        result
            .with_context(|| format!("world {} instance {} attack executor", self.world, id.index))
    }
}
