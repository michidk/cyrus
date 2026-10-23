//! Receiver-owned attack execution through the four defense slots and shared animation owner.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::attack_step_with_animation;
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Apply the d001 response after message intent clears. The order follows the
    /// published combat-message contract: admission, displacement, impact sound,
    /// hurt taunt, weapon cleanup, hit reaction, damage and coordination handoff.
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
        let selected_player = self
            .selected_player_index()
            .map(|index| self.id(index))
            .transpose()?;
        let selected = selected_player == Some(receiver);

        if !selected {
            let required = self.actors[receiver_index].attributes[102];
            if required != 0 && required != self.actors[sender_index].attributes[23] {
                return Ok(());
            }
        }
        if self.actors[receiver_index].attributes[97] != 0 {
            let holding = self.spacing_weapon(receiver, inputs)?.1;
            let stance = self.actors[receiver_index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| combat.flags & 4 != 0);
            if !holding && !stance {
                return Ok(());
            }
        }
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
                start_index: Some(4),
            },
        )?;
        self.request_hurt_combat_taunt(receiver, inputs, random, sounds)?;
        if selected
            && self
                .globals
                .player_death_override_group()
                .context("hit response death-animation override unavailable")?
                != 0
        {
            return Ok(());
        }
        let sender_action = self.actors[sender_index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .map(|combat| combat.action)
            .unwrap_or(-1);
        if sender_action == 8 {
            let selected = selected_player.context("hit response selected player unavailable")?;
            self.cleanup_actor_weapon(
                receiver,
                selected,
                inputs
                    .nominal_ticks
                    .context("hit response nominal ticks unavailable")?,
                sounds,
            )?;
            self.cleanup_actor_weapon(
                sender,
                selected,
                inputs
                    .nominal_ticks
                    .context("hit response nominal ticks unavailable")?,
                sounds,
            )?;
        }

        let ticks = inputs
            .nominal_ticks
            .context("hit response nominal ticks unavailable")?;
        let reaction_group = if !selected {
            117
        } else {
            let holding = self.spacing_weapon(receiver, inputs)?.1;
            let stance = self.actors[receiver_index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| combat.flags & 4 != 0);
            let odd = self.globals.world_loop_frame & 1 != 0;
            if holding || stance {
                if odd { 118 } else { 117 }
            } else if self.actors[receiver_index]
                .animation
                .as_ref()
                .is_some_and(|animation| animation.playback.has_group(119))
            {
                let own = self.spatial(receiver)?.local;
                let target = self.spatial(sender)?.local;
                if super::super::decision::ordinary_defence_forward_arc(
                    own.position,
                    target.position,
                    i32::from(own.angles[1]),
                ) {
                    119
                } else {
                    120
                }
            } else if odd {
                133
            } else {
                132
            }
        };
        if self.actors[receiver_index].animation.is_some() {
            self.request_ordinary_group(receiver, reaction_group, false, ticks, sounds)?;
        }

        let receiver_action = self.actors[receiver_index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .map(|combat| combat.action)
            .unwrap_or(-1);
        if !matches!(receiver_action, -1 | 9) {
            self.actors[sender_index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .context("hit sender combat unavailable")?
                .action = -1;
        }

        let damage_admitted = self.actors[receiver_index].attributes[102] == 0
            || self.actors[sender_index].attributes[23]
                == self.actors[receiver_index].attributes[102];
        if !damage_admitted {
            return Ok(());
        }
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
        if selected {
            self.apply_selected_additive_health(receiver, change, inputs, Some(random), sounds)?;
        } else {
            let environment = super::transition::ObstacleEnvironment {
                selected_player,
                auto_grab: false,
                item_system_ready: inputs
                    .hud
                    .map(|hud| hud.inventory_ready())
                    .transpose()?
                    .unwrap_or(false),
                shell_execution: inputs.shell_execution.unwrap_or(false),
                ticks,
            };
            self.apply_nonplayer_contact_health(receiver, change, environment)?;
        }
        if self.actors[receiver_index].attributes[3] < 1 {
            self.request_combat_taunt(receiver, 6, 0, inputs, random, sounds)?;
            self.request_timed_combat_taunt(sender, 7, 0, inputs, random, sounds)?;
            let motion_kind = self.current_combat_motion(sender)?.kind;
            self.dispatch_combat_death(
                receiver,
                if motion_kind == 0 { 0 } else { 1 },
                inputs,
                random,
                sounds,
            )?;
        }
        // The original restores a charged (item 75) sender through the ordinary
        // chargeWeapon(0) swap after the damage response. Keep this after health
        // and death dispatch so a failed earlier service retains its writes.
        if self.actors[sender_index].attributes[23] == 75 {
            self.restore_charged_sender_weapon(sender, inputs)?;
        }
        let combat = self.actors[receiver_index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("hit response receiver combat unavailable")?;
        if combat.attacker_selection_state == 0
            && !selected
            && !self.query_is_dead(receiver, selected_player)?
        {
            if let Some(coordinator) = self.ordinary_waiting_coordinator(sender)? {
                self.send_combat_message(receiver, coordinator, 0xd003)?;
            } else {
                self.set_combat_coordination(receiver, 1)?;
            }
        }
        Ok(())
    }

    /// Apply the installed `chargeWeapon(0)` inventory swap for a hit sender.
    /// This is the production equivalent of the original post-damage restoration
    /// branch: item 1 is added, the sender's weapon attribute becomes 1, item 75
    /// is emptied, and item 1 is selected.
    fn restore_charged_sender_weapon(
        &mut self,
        sender: ScriptActorId,
        inputs: ScriptInputs<'_>,
    ) -> Result<()> {
        let hud = inputs
            .hud
            .context("combat weapon restoration inventory unavailable")?;
        if !hud.inventory_ready()? {
            return Ok(());
        }
        let sender_index = self.resolve(sender)?;
        let actors = &mut self.actors;
        hud.add_item(1, 1, &mut |weapon| {
            // Inventory acquisition may repair an empty selection. The original
            // restoration service belongs to the hit sender, so that repair must
            // update the sender's weapon attribute even when the sender is not the
            // selected player owning the HUD view.
            actors[sender_index].attributes[23] = weapon;
            Ok(())
        })?;
        actors[sender_index].attributes[23] = 1;
        hud.remove_item(75)?;
        hud.select_item(1)
    }
    /// The direct-damage response (`0xd005`): displace the receiver 3000 units away from the
    /// sender, then (for the selected player) return at once while a death-animation override is
    /// set. A nonplayer starts hit-reaction group 117; the selected player starts 117 or 118 by
    /// world-loop parity while holding a weapon or in combat stance (the drawn-weapon bit of its
    /// combat record), else 132 or 133. The selected player's health response is skipped while the
    /// damage-suppression cheat is on. Otherwise the sender's attribute 96 is subtracted from the
    /// receiver's health (the selected player through its shared additive health), and below one
    /// a nonplayer requests taunt category six and either identity runs contact death with
    /// selector zero.
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
        let selected = self.selected_player_index() == Some(receiver_index);
        let selected_player = self
            .selected_player_index()
            .map(|index| self.id(index))
            .transpose()?;
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
        if selected
            && self
                .globals
                .player_death_override_group()
                .context("direct damage death-animation override unavailable")?
                != 0
        {
            return Ok(());
        }
        let ticks = inputs
            .nominal_ticks
            .context("direct damage nominal ticks unavailable")?;
        let group = if selected {
            let (_, holding) = self.spacing_weapon(receiver, inputs)?;
            let stance = holding
                || self.actors[receiver_index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .is_some_and(|combat| combat.flags & 4 != 0);
            let odd = self.globals.world_loop_frame & 1 != 0;
            Some(match (stance, odd) {
                (true, false) => 117,
                (true, true) => 118,
                (false, false) => 132,
                (false, true) => 133,
            })
        } else {
            Some(117)
        };
        if let Some(group) = group
            && self.actors[receiver_index].animation.is_some()
        {
            self.request_ordinary_group(receiver, group, false, ticks, sounds)?;
        }
        if selected
            && inputs
                .death_suppressed
                .context("direct damage damage-suppression state unavailable")?
        {
            return Ok(());
        }
        let damage = i32::from(self.actors[sender_index].attributes[96]);
        if selected {
            self.apply_selected_additive_health(
                receiver,
                damage.wrapping_neg(),
                inputs,
                Some(random),
                sounds,
            )?;
        } else {
            // d005 uses the generic mode-zero health admission. In particular, this
            // preserves the nonzero attr-120 veto and attr-97 weapon/stance gate before
            // changing the receiver, rather than writing the health byte directly.
            let environment = super::transition::ObstacleEnvironment {
                selected_player,
                auto_grab: false,
                item_system_ready: inputs
                    .hud
                    .map(|hud| hud.inventory_ready())
                    .transpose()?
                    .unwrap_or(false),
                shell_execution: inputs.shell_execution.unwrap_or(false),
                ticks,
            };
            self.apply_nonplayer_contact_health(receiver, damage.wrapping_neg(), environment)?;
        }
        if self.actors[receiver_index].attributes[3] < 1 {
            if !selected {
                self.request_combat_taunt(receiver, 6, 0, inputs, random, sounds)?;
            }
            self.dispatch_combat_death(receiver, 0, inputs, random, sounds)?;
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
    /// Candidates without combat state take the impact response and end the stage.
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
            if self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_none()
            {
                // A candidate without combat state takes the impact response directly.
                if self.query_is_dead(defender, geometry.environment.selected_player)?
                    || self.contact_impact_gated(id, geometry.environment)?
                {
                    return Ok(());
                }
                self.apply_contact_impact(
                    id,
                    defender,
                    geometry.environment,
                    inputs,
                    random,
                    sounds,
                )?;
                self.actors[id.index]
                    .control
                    .as_mut()
                    .and_then(|control| control.combat.as_mut())
                    .context("attacker combat unavailable")?
                    .flags &= !3;
                return Ok(());
            }
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
                if self.contact_impact_gated(id, geometry.environment)? {
                    continue;
                }
                self.apply_contact_impact(
                    id,
                    defender,
                    geometry.environment,
                    inputs,
                    random,
                    sounds,
                )?;
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
                        start_index: None,
                    },
                )?;
            }
        }
        Ok(())
    }

    /// Attribute 97 confines an attacker's impact response to holding a weapon or combat flag four.
    fn contact_impact_gated(
        &self,
        id: ScriptActorId,
        environment: super::transition::ObstacleEnvironment,
    ) -> Result<bool> {
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
            environment.item_system_ready,
            environment.shell_execution,
        )
        .0;
        Ok(attacker.attributes[97] != 0
            && !holding
            && control
                .combat
                .as_ref()
                .is_none_or(|combat| combat.flags & 4 == 0))
    }

    /// The attacker's impact effect, then damage and death dispatch for a target with health.
    fn apply_contact_impact(
        &mut self,
        id: ScriptActorId,
        defender: ScriptActorId,
        environment: super::transition::ObstacleEnvironment,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let effect = i32::from(self.actors[id.index].attributes[86]);
        let rate_offset_hz = random.rnd(128)?.wrapping_mul(16).wrapping_sub(1024);
        sounds(
            id,
            crate::script::ScriptSound::Positional {
                effect,
                volume: 0,
                rate_offset_hz,
                position: self.sight_position(id.index)?,
                start_index: Some(4),
            },
        )?;
        if self.actors[defender.index].attributes[3] == 0 {
            return Ok(());
        }
        let damage = if self.actors[id.index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .is_some_and(|combat| combat.retained_action_indices[1] < 0)
        {
            0
        } else {
            self.current_combat_motion(id)?
                .damage
                .context("contact motion damage unavailable")?
        };
        if environment.selected_player == Some(defender) {
            self.apply_selected_contact_health(
                defender,
                damage.wrapping_neg(),
                inputs,
                random,
                sounds,
            )?;
        } else {
            self.apply_nonplayer_contact_health(defender, damage.wrapping_neg(), environment)?;
        }
        if self.actors[defender.index].attributes[3] == 0 {
            self.dispatch_contact_death(defender, 0, inputs.nominal_ticks, sounds)?;
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
            self.force_death_group(id, index, group, ticks, sounds)?;
            if self.actors[index]
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

    /// Force-start a death group on `index`, dispatching its animation sounds and events.
    fn force_death_group(
        &mut self,
        id: ScriptActorId,
        index: usize,
        group: u16,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
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
                        return event_context.dispatch(flags, event_camera.as_mut(), event_effects);
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
        Ok(())
    }

    /// Combat death service for the damage responses (`0xd005` direct damage and the scaled hit
    /// response); the weapon-contact caller uses [`Self::dispatch_contact_death`] instead.
    /// Selector zero prefers group 122 when present, otherwise group 121. With attribute 69
    /// clear, the selected player stores the group as the death override when none is set (its
    /// death controller starts the animation); a nonplayer force-starts it, stores death default
    /// 124/123 (releasing its walk nodes), sends `0xd002` to the first coordinator sharing its
    /// target and clears its close-distance and coordination state. Every receiver then resets its
    /// action selector to -1 and runs the combat reset (selector eight only releases walk nodes),
    /// takes selector eight and clears combat flag 0x04 and the coordination state. The selected
    /// player finishes with health -256 through the death-owning service, enabling hit
    /// inhibition for the world loop's death exit; it is left alone while death suppression or
    /// hit inhibition is active. Follows preservation's combat death service rules.
    pub(super) fn dispatch_combat_death(
        &mut self,
        id: ScriptActorId,
        selector: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let selected = self.selected_player_index() == Some(index);
        if selected {
            let state = self
                .globals
                .selected_health
                .as_ref()
                .context("combat death health owner unavailable")?;
            if state.hit_inhibited()
                || inputs
                    .death_suppressed
                    .context("combat death suppression state unavailable")?
            {
                return Ok(());
            }
        }
        if self.actors[index].attributes[69] == 0 {
            let animation = self.actors[index].animation.as_ref();
            let group = if selector == 0 && animation.is_some_and(|a| a.playback.has_group(122)) {
                122
            } else {
                121
            };
            if selected {
                if self
                    .globals
                    .player_death_override_group()
                    .context("combat death override unavailable")?
                    == 0
                {
                    self.globals
                        .set_player_death_override_group(i32::from(group))?;
                }
            } else {
                if animation.is_some() {
                    let ticks = inputs
                        .nominal_ticks
                        .context("combat death nominal ticks unavailable")?;
                    self.force_death_group(id, index, group, ticks, sounds)?;
                }
                self.release_held_walk_nodes(id)?;
                let control = self.actors[index]
                    .control
                    .as_mut()
                    .context("combat death control unavailable")?;
                anyhow::ensure!(control.combat.is_some(), "combat death record unavailable");
                control.combat_death_default = Some(if group == 122 { 124 } else { 123 });
                // Without a selected player nothing is excluded; the search skips the receiver
                // itself, so passing it excludes no other candidate.
                let player = self
                    .selected_player_index()
                    .map(|player| self.id(player))
                    .transpose()?
                    .unwrap_or(id);
                if let Some(candidate) = self.ordinary_active_coordinator(id, player, inputs)? {
                    self.send_combat_message(id, candidate, 0xd002)?;
                }
                let combat = self.actors[index]
                    .control
                    .as_mut()
                    .and_then(|control| control.combat.as_mut())
                    .expect("validated combat record");
                combat.close_distance_passes = 0;
                combat.attacker_selection_state = 0;
            }
        }
        let was_dead = {
            let control = self.actors[index]
                .control
                .as_mut()
                .context("combat death control unavailable")?;
            control
                .combat
                .as_mut()
                .context("combat death record unavailable")?
                .action = -1;
            control.death_selector == 8
        };
        if was_dead {
            self.release_held_walk_nodes(id)?;
        }
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated combat death control");
        if !was_dead {
            if control.death_selector != 1 {
                control.death_selector = 1;
                control.death_response = 0;
            }
            if control.killed {
                control.script_control &= !2;
            }
        }
        control.death_selector = 8;
        let combat = control.combat.as_mut().expect("validated combat record");
        combat.flags &= !4;
        combat.close_distance_passes = 0;
        combat.attacker_selection_state = 0;
        if selected {
            self.apply_selected_death_health(id, -256, None, inputs, Some(random), sounds)?;
        }
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
