//! Receiver-owned attack execution through the four defense slots and shared animation owner.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::attack_step_with_animation;
use anyhow::{Context, Result};

impl ScriptWorld {
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
    /// and request ordered clank cues. Noncombat and actor-owned face responses
    /// remain unsupported.
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
            anyhow::ensure!(
                matches!(candidate.owner, CandidateOwner::Static(_)),
                "actor-owned static-face attack response unsupported"
            );
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
            let contacts = ground.scene.static_collision.runtime_segment_contacts(
                &single,
                models,
                &Default::default(),
                endpoints[1],
                endpoints[0],
                true,
                None,
            )?;
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
