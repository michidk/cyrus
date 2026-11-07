//! The direct point wrapper behind `MovePosition`, `MoveMarker` and `MoveLocation`.
//!
//! It walks to a Q8 destination without claiming walk nodes: the receiver's held nodes are released
//! on entry, and the retained phase lives in the caller-loaded native command. Phase 0 turns toward
//! the destination (22), phase 22 faces it until the turn finishes (11), phase 11 walks and keeps
//! facing, phase 9 is the countdown shared with the node wrapper and phase 21 returns the actor to
//! its default animation group and completes. Any other phase restarts from 0. Falling or queued
//! motion interrupts every phase.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::AiType;
use crate::script::JumpMotion;
use crate::script::LivingState;
use crate::script::PointPhase;
use anyhow::{Context, Result};
use cyrus_soup::Attribute;

type SoundSink<'a> =
    Option<&'a mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>;

/// The ledge operation performed before a point command's pending exit.
#[derive(Clone, Copy)]
pub(super) enum PointLedgeAction {
    ReleaseHang,
    Advance,
}

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

    /// Bind the selected receiver's ledge services lazily for direct and node point commands.
    pub(super) fn step_point_ledge(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        action: PointLedgeAction,
        sounds: SoundSink<'_>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if self.selected_player() != Some(id) {
            return Ok(());
        }
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("point ledge control unavailable")?;
        match action {
            PointLedgeAction::ReleaseHang => {
                if control.living_state != LivingState::HANGING {
                    return Ok(());
                }
                if self
                    .globals
                    .selected_action_inhibition
                    .context("point hang release inhibition unavailable")?
                    != 0
                {
                    return Ok(());
                }
            }
            PointLedgeAction::Advance => {
                if actor.attributes[Attribute::PlayerType] == 1 || !control.living_state.is_ledge()
                {
                    return Ok(());
                }
            }
        }
        let prepared;
        let geometry = if let Some(geometry) = inputs.locomotion_geometry {
            geometry
        } else {
            let source = inputs
                .player_collision
                .context("point ledge geometry unavailable")?;
            prepared = self.prepare_ordinary_collision_query(
                id,
                source.scene,
                source.animations,
                source.view,
                source.actors,
            )?;
            super::LocomotionGeometry {
                ground: super::ground::OrdinaryGroundQuery {
                    scene: source.scene,
                    animations: source.animations,
                    prepared: &prepared,
                    distance: source.scene.settings.post_collide_height,
                    sidedness: false,
                },
                environment: super::transition::ObstacleEnvironment {
                    selected_player: Some(id),
                    auto_grab: false,
                    item_system_ready: inputs
                        .hud
                        .map(super::super::ScriptHud::inventory_ready)
                        .transpose()?
                        .unwrap_or(false),
                    shell_execution: inputs.shell_execution.unwrap_or(false),
                    ticks: inputs
                        .nominal_ticks
                        .context("point ledge nominal ticks unavailable")?,
                },
            }
        };
        anyhow::ensure!(
            geometry.environment.selected_player == Some(id),
            "point ledge geometry selects another player"
        );
        let timing = super::ledge_states::LedgeTiming {
            fine_ticks: inputs
                .fine_ticks
                .context("point ledge fine ticks unavailable")?,
            frame_scale: inputs.frame_scale,
        };
        let sounds = sounds.context("point ledge sound handler unavailable")?;
        match action {
            PointLedgeAction::ReleaseHang => {
                self.ledge_hang_release(id, geometry.ground, geometry.environment, timing, sounds)?;
            }
            PointLedgeAction::Advance => {
                self.ledge_transition(id, geometry.ground, geometry.environment, timing, sounds)?;
            }
        }
        Ok(())
    }

    /// One direct point step toward a Q8 destination within a Q8 arrival tolerance. Returns one
    /// while the walk continues and zero once it completes. Hanging receivers run the selected
    /// player's hang release, then retain the move as pending without testing arrival.
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
        let motion_pending =
            control.falling_state != JumpMotion::NONE || control.queued_motion != JumpMotion::NONE;
        let hanging = control.living_state == LivingState::HANGING;
        if !motion_pending && hanging {
            self.step_point_ledge(
                id,
                inputs,
                PointLedgeAction::ReleaseHang,
                sounds.as_mut().map(|sink| {
                    &mut **sink
                        as &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>
                }),
            )?;
        }
        if motion_pending || hanging {
            self.globals.native_command.point_phase = PointPhase::START;
            self.actors[index].attributes[Attribute::Collide] = 13;
            self.mark_direct_point_active(index);
            return Ok(1);
        }
        let phase = self.globals.native_command.point_phase;
        if !matches!(phase, PointPhase::DIRECT_WALK | PointPhase::DIRECT_TURN)
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
        self.actors[index].attributes[Attribute::Collide] = 12;
        match phase {
            PointPhase::START => self.globals.native_command.point_phase = PointPhase::DIRECT_TURN,
            PointPhase::DIRECT_TURN => {
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
                    self.globals.native_command.point_phase = PointPhase::DIRECT_WALK;
                }
            }
            PointPhase::DIRECT_WALK => {
                self.actors[index].attributes[Attribute::Collide] = 13;
                let sliding = self.actors[index]
                    .control
                    .as_ref()
                    .expect("admitted point control")
                    .sliding_state
                    != 0;
                if sliding {
                    self.globals.native_command.point_phase = PointPhase::START;
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
                self.map_updates.register(index)?;
            }
            PointPhase::IDLE_PAUSE => self.advance_point_delay(id, inputs, random, sounds)?,
            PointPhase::DIRECT_ARRIVE => {
                self.admit_point_default(id, inputs, sounds)?;
                return self.complete_point(id, 0);
            }
            _ => self.globals.native_command.point_phase = PointPhase::START,
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
            .ai_type = AiType::POINT_NAVIGATION;
    }
}
