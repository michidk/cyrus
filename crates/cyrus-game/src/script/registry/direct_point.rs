//! The direct point wrapper behind `MovePosition`, `MoveMarker` and `MoveLocation`.
//!
//! It walks to a Q8 destination without claiming walk nodes: the receiver's held nodes are released
//! on entry, and the retained phase lives in the caller-loaded native command. Phase 0 turns toward
//! the destination (22), phase 22 faces it until the turn finishes (11), phase 11 walks and keeps
//! facing, phase 9 is the countdown shared with the node wrapper and phase 21 returns the actor to
//! its default animation group and completes. Any other phase restarts from 0. Falling or queued
//! motion interrupts every phase.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

type SoundSink<'a> =
    Option<&'a mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>;

impl ScriptWorld {
    /// One direct point step on the actor's own retained point command, the form engine callers
    /// use; a script task runs [`Self::move_point_direct`] on its slot's command instead.
    pub fn move_ordinary_direct(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: SoundSink<'_>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            self.actors[index]
                .control
                .as_ref()
                .context("direct point control unavailable")?
                .point_command
                .load(&mut self.globals.native_command);
            let result =
                self.move_point_direct(id, destination, tolerance, inputs, random, sounds)?;
            let shared = &self.globals.native_command;
            self.actors[index]
                .control
                .as_mut()
                .expect("admitted point control")
                .point_command
                .save(shared);
            Ok(result)
        })();
        result.with_context(|| format!("world {} actor {} direct point", self.world, id.index))
    }

    /// One direct point step toward a Q8 destination within a Q8 arrival tolerance. Returns one
    /// while the walk continues and zero once it completes. A receiver hanging from a ledge needs
    /// the ledge release, which is unavailable.
    pub(super) fn move_point_direct(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        mut sounds: SoundSink<'_>,
    ) -> Result<i32> {
        let index = self.resolve(id)?;
        self.release_held_walk_nodes(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("direct point control unavailable")?;
        if control.falling_state != 0 || control.queued_motion != 0 {
            self.globals.native_command.point_phase = 0;
            self.actors[index].attributes[1] = 13;
            self.mark_direct_point_active(index);
            return Ok(1);
        }
        anyhow::ensure!(
            control.living_state != 15,
            "direct point ledge release unsupported"
        );
        let phase = self.globals.native_command.point_phase;
        if !matches!(phase, 11 | 22)
            && self.point_destination_arrived(index, destination, tolerance, inputs)?
        {
            if self.updates.animation_system
                && let Some(group) = self.actors[index]
                    .animation
                    .as_ref()
                    .map(|animation| animation.playback.default_group())
            {
                let ticks = inputs
                    .nominal_ticks
                    .context("direct point default animation requires nominal_ticks")?;
                self.request_ordinary_group(
                    id,
                    group,
                    true,
                    ticks,
                    sounds
                        .as_deref_mut()
                        .context("direct point default animation requires synchronous sounds")?,
                )?;
            }
            return self.complete_point(id, 0);
        }
        self.actors[index].attributes[1] = 12;
        match phase {
            0 => self.globals.native_command.point_phase = 22,
            22 => {
                let facing = self.point_facing_inputs(id, inputs)?;
                let turning = self.face_ordinary_point(
                    id,
                    destination,
                    facing,
                    sounds
                        .as_deref_mut()
                        .context("direct point facing requires synchronous sounds")?,
                )?;
                if turning == 0 {
                    self.globals.native_command.point_phase = 11;
                }
            }
            11 => {
                self.actors[index].attributes[1] = 13;
                let sliding = self.actors[index]
                    .control
                    .as_ref()
                    .expect("admitted point control")
                    .sliding_state
                    != 0;
                if sliding {
                    self.globals.native_command.point_phase = 0;
                }
                let walking = self.walk_ordinary_point(
                    id,
                    destination,
                    tolerance,
                    inputs,
                    sounds.as_mut().map(|sink| {
                        &mut **sink
                            as &mut dyn FnMut(
                                ScriptActorId,
                                super::super::ScriptSound,
                            ) -> Result<()>
                    }),
                )?;
                if walking == 0 {
                    return self.complete_point(id, 0);
                }
                let exit_reached = self.actors[index]
                    .animation
                    .as_ref()
                    .context("direct point walking animation unavailable")?
                    .playback
                    .exit_reached();
                if !exit_reached {
                    let facing = self.point_facing_inputs(id, inputs)?;
                    self.face_ordinary_point(
                        id,
                        destination,
                        facing,
                        sounds
                            .as_deref_mut()
                            .context("direct point facing requires synchronous sounds")?,
                    )?;
                }
                if !self.map_updates.contains(&index) {
                    self.map_updates.push(index);
                }
            }
            9 => self.advance_point_delay(id, inputs, random, sounds)?,
            21 => {
                self.admit_point_default(id, inputs, sounds)?;
                return self.complete_point(id, 0);
            }
            _ => self.globals.native_command.point_phase = 0,
        }
        self.mark_direct_point_active(index);
        Ok(1)
    }

    /// The common tail of a direct step that stays pending: the lifecycle selector reads seven.
    fn mark_direct_point_active(&mut self, index: usize) {
        self.actors[index]
            .control
            .as_mut()
            .expect("admitted point control")
            .death_selector = 7;
    }
}
