//! Live nonplayer counter timing, synchronous attack start and primitive sidesteps.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::{CounterAttack, CounterAttackInput, Sidestep, counter_attack};
use anyhow::{Context, Result, ensure};

impl ScriptWorld {
    pub(super) fn current_combat_motion(
        &self,
        target: ScriptActorId,
    ) -> Result<&cyrus_assets::script::combat::Motion> {
        let combat = self.actors[self.resolve(target)?]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("target combat unavailable")?;
        let slot = usize::try_from(combat.retained_action_indices[1])
            .context("negative target attack slot")?;
        let motion = usize::try_from(combat.decision.move_selector)
            .context("negative target motion selector")?;
        self.combat_attack(target, slot)?
            .with_context(|| format!("target attack slot {slot}: unpopulated"))?
            .motions
            .get(motion)
            .with_context(|| format!("target attack slot {slot} motion {motion}: unavailable"))
    }

    /// Start an ordinary nonplayer's already selected pending attack without a VM task.
    /// Drawing/sheathing and existing actions return false before animation services.
    /// Action nine precedes the executor; earlier writes survive an executor error.
    /// The selected player's separate pending-move selection remains unsupported.
    pub fn start_ordinary_combat_attack(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<_> {
            let index = self.resolve(id)?;
            if let Some(activation) = inputs.activation {
                self.validate_activation(activation)?;
            }
            let host = super::GameHost {
                native_source_program: None,
                world: self.world,
                animation_system: self.updates.animation_system,
                task_system: self.updates.task_system,
                index,
                generation: self.generation,
                ordinary_actors: None,
                walk_map_count: None,
                registry: self,
                sounds: None,
                random: None,
                inputs,
            };
            let (_, drawing, sheathing) = host.receiver_weapon_predicates("StartAttack")?;
            if drawing || sheathing {
                return Ok(false);
            }
            let selected_player = host.selected_player();
            let actor = &mut self.actors[index];
            let combat = actor
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .context("receiver combat unavailable")?;
            if combat.action != -1 {
                return Ok(false);
            }
            ensure!(
                selected_player != Some(id)
                    && !matches!(
                        actor.source,
                        super::ScriptActorSource::Player
                            | super::ScriptActorSource::AlternatePlayer
                    ),
                "selected-player attack starter unavailable"
            );
            combat.action = 9;
            self.execute_combat_attack(id, 0, inputs, sounds)?;
            Ok(true)
        })();
        result.with_context(|| format!("world {} instance {} attack starter", self.world, id.index))
    }

    /// Select and execute one ordinary counter from the same distance/tick sample as spacing.
    /// Queries are lazy behind weapon, window, facing, action, slot and target-mark gates.
    /// Intent and pending writes precede the starter; the target mark follows its return.
    /// Sidesteps call movement directly. This does not schedule automatic combat traversal.
    pub fn counter_ordinary_combat(
        &mut self,
        id: ScriptActorId,
        sample: super::spacing::OrdinarySpacingInput,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<CounterAttack> {
        let result = (|| -> Result<_> {
            let index = self.resolve(id)?;
            ensure!(
                !matches!(
                    self.actors[index].source,
                    super::ScriptActorSource::Player | super::ScriptActorSource::AlternatePlayer
                ),
                "selected-player combat counter unavailable"
            );
            let (selected_player, armed) = self.spacing_weapon(id, inputs)?;
            ensure!(
                selected_player != Some(id),
                "selected-player combat counter unavailable"
            );
            if !armed {
                return Ok(CounterAttack::Idle);
            }
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let target = combat.target.context("combat target unavailable")?;
            let target_index = self.resolve(target)?;
            let target_combat = self.actors[target_index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("target combat unavailable")?;
            let window = target_combat.spacing_ranges;
            if sample.distance <= window[0] || sample.distance >= window[1] {
                return Ok(CounterAttack::Idle);
            }
            let own_heading = self.spatial(id)?.local.angles[1];
            let target_heading = self.spatial(target)?.local.angles[1];
            if !crate::script::decision::facing_each_other(
                i32::from(target_heading),
                i32::from(own_heading),
            ) || matches!(target_combat.action, -1 | 9)
                || combat.action != -1
                || !combat.defense_indices.iter().any(Option::is_some)
                || target_combat.flags & 0x10 != 0
            {
                return Ok(CounterAttack::Idle);
            }
            let motion = self.current_combat_motion(target)?;
            let near = motion.first_collide_frame;
            let playback = &self.actors[target_index]
                .animation
                .as_ref()
                .context("target animation unavailable")?
                .playback;
            let cursor = playback
                .current_command_cursor()?
                .context("target command cursor unavailable")?;
            let gap_near = near.wrapping_sub(cursor);
            let far = motion
                .first_defend_frame
                .context("target first_defend_frame unavailable")?;
            let gap_far = far.wrapping_sub(
                playback
                    .current_command_cursor()?
                    .context("target command cursor unavailable")?,
            );
            let counter = self.current_combat_counter_attack(target)?;
            let slot = usize::try_from(counter).context("negative counter defense slot")?;
            let record = combat
                .defense_indices
                .get(slot)
                .copied()
                .flatten()
                .with_context(|| format!("counter defense slot {slot}: unpopulated"))?;
            let defense = self
                .combat_defenses
                .get(usize::from(record))
                .with_context(|| {
                    format!("counter defense slot {slot} record {record}: unavailable")
                })?;
            let outcome = counter_attack(
                &CounterAttackInput {
                    armed: true,
                    distance: sample.distance,
                    target_window: (window[0], window[1]),
                    facing_ok: true,
                    target_selected: target_combat.action,
                    own_selected: combat.action,
                    has_defense: true,
                    target_marked: false,
                    gap_near,
                    gap_far,
                    counter_index: counter,
                    counter_start: defense
                        .first_collide_frame
                        .context("counter first_collide_frame unavailable")?,
                    counter_duration: defense
                        .collide_duration
                        .context("counter collide_duration unavailable")?,
                    pacing_sum: sample.tick_counter.wrapping_add(combat.allocation_index),
                    attack_pacing: combat.decision.attack_pacing,
                    target_selector: target_combat.retained_action_indices[1],
                },
                || random.draw(),
            )?;
            match outcome {
                CounterAttack::Idle => {}
                CounterAttack::Attack { index: counter } => {
                    let combat = self.actors[index]
                        .control
                        .as_mut()
                        .expect("admitted control")
                        .combat
                        .as_mut()
                        .expect("admitted combat");
                    combat.flags |= 2;
                    combat.retained_action_indices[0] = counter;
                    self.start_ordinary_combat_attack(id, inputs, sounds)?;
                    self.actors[target_index]
                        .control
                        .as_mut()
                        .expect("admitted target control")
                        .combat
                        .as_mut()
                        .expect("admitted target combat")
                        .flags |= 0x10;
                }
                CounterAttack::Sidestep(direction) => {
                    let movement = match direction {
                        Sidestep::Right => super::spacing::OrdinaryMovement::Right,
                        Sidestep::Left => super::spacing::OrdinaryMovement::Left,
                        Sidestep::Back => super::spacing::OrdinaryMovement::Backward,
                    };
                    self.move_ordinary(id, movement, 1, 0, inputs, sounds)?;
                }
            }
            Ok(outcome)
        })();
        result.with_context(|| format!("world {} instance {} combat counter", self.world, id.index))
    }
}
