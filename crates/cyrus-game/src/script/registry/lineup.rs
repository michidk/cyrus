//! The selected player's side of the lineup natives: the helpers the movers call, over live state.
//!
//! The mover decisions are `script::lineup`'s. A helper that cannot run (missing inputs, a step
//! whose admission input is absent) answers conservatively and records its error; the native then
//! fails with it rather than continuing on a guess.
use super::{GameHost, ScriptActorId};
use crate::script::alignment::{self, CloseStep, Facing};
use crate::script::lineup::{LineupHost, LineupPort, StepPort, close_step_run};
use anyhow::{Context, Result};

pub(super) struct PlayerLineup<'h, 'a, 's, 'i, 'r> {
    host: &'h mut GameHost<'a, 's, 'i, 'r>,
    player: ScriptActorId,
    error: Option<anyhow::Error>,
}

impl<'h, 'a, 's, 'i, 'r> PlayerLineup<'h, 'a, 's, 'i, 'r> {
    pub(super) fn new(host: &'h mut GameHost<'a, 's, 'i, 'r>) -> Result<Self> {
        let player = host
            .selected_player()
            .context("lineup selected player unavailable")?;
        let index = host.registry.resolve(player)?;
        anyhow::ensure!(
            index != host.index,
            "lineup receiver is the selected player"
        );
        Ok(Self {
            host,
            player,
            error: None,
        })
    }

    fn record<T>(&mut self, fallback: T, result: Result<T>) -> T {
        result.unwrap_or_else(|error| {
            self.error.get_or_insert(error);
            fallback
        })
    }

    fn pose(&self) -> Result<([i32; 3], [u16; 3])> {
        let local = self
            .host
            .registry
            .spatial(self.player)
            .context("lineup selected-player pose unavailable")?
            .local;
        Ok((local.position, local.angles))
    }

    fn frame_scale(&self) -> Result<i32> {
        Ok(self
            .host
            .inputs
            .frame_scale
            .context("lineup requires frame_scale")? as i32)
    }

    fn speed(&self) -> u8 {
        let actor = &self.host.registry.actors[self.player.index];
        alignment::speed(
            actor
                .animation
                .as_ref()
                .and_then(|animation| animation.playback.current_group()),
            Some(&actor.attributes[..]),
        )
    }

    fn horizontal_distance(&self, x: i32, z: i32) -> Result<i32> {
        let (position, _) = self.pose()?;
        crate::script::spatial::rounded_distance([
            position[0].wrapping_sub(x),
            0,
            position[2].wrapping_sub(z),
        ])
    }

    fn set_pose(&mut self, position: Option<[i32; 3]>, angles: Option<[u16; 3]>) -> Result<()> {
        let index = self.player.index;
        let spatial = self
            .host
            .registry
            .spatial
            .as_mut()
            .context("lineup spatial state not prepared")?
            .actor_mut(index)?;
        if let Some(position) = position {
            spatial.local.position = position;
            if !self.host.registry.map_updates.contains(&index) {
                self.host.registry.map_updates.push(index);
            }
        }
        if let Some(angles) = angles
            && spatial.local.angles != angles
        {
            spatial.local.angles = angles;
            spatial.refresh_transform()?;
        }
        Ok(())
    }

    /// One facing or turning step toward a heading (`target` heading) with the support-response
    /// turn; true while the turn animation is still requested.
    fn turn(&mut self, heading_target: u16) -> Result<bool> {
        let index = self.player.index;
        let (_, mut angles) = self.pose()?;
        let actor = &self.host.registry.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("lineup selected-player control unavailable")?;
        let (holding_weapon, _, _) = self
            .host
            .actor_weapon_predicates(index, "IsHoldingWeapon")?;
        let combat_mode = control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags & 4 != 0);
        let rate = actor.attributes[if holding_weapon || combat_mode {
            21
        } else {
            15
        }];
        let torch = control.torch_burning != 0;
        let scale = self
            .host
            .inputs
            .frame_scale
            .context("lineup requires frame_scale")?;
        let turn = crate::script::ordinary_slope_turn(
            angles[1],
            heading_target,
            rate,
            scale,
            holding_weapon,
            torch,
        );
        angles[1] = turn.heading;
        self.set_pose(None, Some(angles))?;
        if let Some(group) = turn.group {
            let ticks = self
                .host
                .inputs
                .nominal_ticks
                .context("lineup requires nominal_ticks")?;
            let sounds = self
                .host
                .sounds
                .as_mut()
                .context("lineup turn requires synchronous sounds")?;
            self.host.registry.request_ordinary_group(
                self.player,
                group,
                false,
                ticks,
                &mut **sounds,
            )?;
        }
        Ok(turn.group.is_some())
    }

    fn walk(&mut self, step: CloseStep) -> Result<()> {
        let function = match step {
            CloseStep::Forward => "WalkForward",
            CloseStep::Right => "MoveRight",
            CloseStep::Left => "MoveLeft",
            CloseStep::Backward => "WalkBackward",
        };
        let inputs = self.host.inputs;
        let sounds = self
            .host
            .sounds
            .as_mut()
            .context("lineup step requires synchronous sounds")?;
        self.host
            .registry
            .step_player(self.player, function, inputs, &mut **sounds)
    }
}

/// Position and the ordinary rollback record a rejected close step restores.
pub(super) struct Saved {
    position: [i32; 3],
    rollback: super::recovery::OrdinaryRollbackSnapshot,
}

impl StepPort for PlayerLineup<'_, '_, '_, '_, '_> {
    type Saved = Option<Saved>;
    fn position(&self) -> [i32; 3] {
        self.pose()
            .map(|(position, _)| position)
            .unwrap_or_default()
    }
    fn heading(&self) -> u16 {
        self.pose().map(|(_, angles)| angles[1]).unwrap_or_default()
    }
    fn save(&mut self) -> Self::Saved {
        let result = (|| -> Result<Saved> {
            let (position, _) = self.pose()?;
            let sounds = self
                .host
                .sounds
                .as_mut()
                .context("lineup step requires synchronous sounds")?;
            let rollback = self
                .host
                .registry
                .capture_ordinary_rollback(self.player, &mut **sounds)?;
            Ok(Saved { position, rollback })
        })();
        match result {
            Ok(saved) => Some(saved),
            Err(error) => {
                self.error.get_or_insert(error);
                None
            }
        }
    }
    fn restore(&mut self, saved: Self::Saved) {
        let Some(saved) = saved else { return };
        let result = (|| -> Result<()> {
            let sounds = self
                .host
                .sounds
                .as_mut()
                .context("lineup step requires synchronous sounds")?;
            self.host.registry.restore_ordinary_rollback(
                self.player,
                &saved.rollback,
                &mut **sounds,
            )?;
            self.set_pose(Some(saved.position), None)
        })();
        self.record((), result);
    }
    fn walk(&mut self, step: CloseStep) {
        let result = PlayerLineup::walk(self, step);
        self.record((), result);
    }
    fn clear_called(&mut self, step: CloseStep) {
        self.host.registry.globals.locomotion_called &= !match step {
            CloseStep::Left => 1,
            CloseStep::Right => 2,
            CloseStep::Forward => 4,
            CloseStep::Backward => 8,
        };
    }
    fn set_horizontal(&mut self, x: i32, z: i32) {
        let result = self
            .pose()
            .and_then(|(position, _)| self.set_pose(Some([x, position[1], z]), None));
        self.record((), result);
    }
}

impl LineupPort for PlayerLineup<'_, '_, '_, '_, '_> {
    fn stale_response(&self) -> bool {
        let control = self.host.registry.actors[self.player.index]
            .control
            .as_ref();
        control.is_some_and(|control| {
            alignment::response_expired(
                control.body_response_flags != 0
                    || control.collision_transition_flags != 0
                    || control.collision_response_auxiliary != [0, 0],
                control.collision_response_age,
            )
        })
    }
    fn level_roll(&mut self) {
        let result = (|| -> Result<()> {
            let (_, mut angles) = self.pose()?;
            let ticks = self
                .host
                .inputs
                .nominal_ticks
                .context("lineup requires nominal_ticks")?;
            angles[2] = alignment::level_roll(angles[2], ticks);
            self.set_pose(None, Some(angles))
        })();
        self.record((), result);
    }
    fn path_clear(&mut self, _: [i32; 3]) -> bool {
        // The visibility query needs collision geometry bound to the player; a receiver's call
        // carries geometry bound to the receiver.
        self.error.get_or_insert_with(|| {
            anyhow::anyhow!("PlayerPickupLineUp path check: player-bound geometry unavailable")
        });
        false
    }
    fn face_point(&mut self, x: i32, z: i32) -> bool {
        let result = (|| -> Result<bool> {
            let (position, angles) = self.pose()?;
            let index = self.player.index;
            let actor = &self.host.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("lineup selected-player control unavailable")?;
            let (holding_weapon, _, _) = self
                .host
                .actor_weapon_predicates(index, "IsHoldingWeapon")?;
            let combat_mode = control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.flags & 4 != 0);
            let facing = alignment::face(alignment::FacingInput {
                position,
                heading: angles[1],
                target: [x, position[1], z],
                speed: self.speed(),
                frame_scale: self.frame_scale()? as u32,
                rate_attribute: actor.attributes[if holding_weapon || combat_mode {
                    21
                } else {
                    15
                }],
                holding_weapon,
                torch: control.torch_burning != 0,
            })?;
            match facing {
                Facing::Arrived => self.set_pose(Some([x, position[1], z]), None)?,
                Facing::Turning { heading, group } => {
                    let mut angles = angles;
                    angles[1] = heading;
                    self.set_pose(None, Some(angles))?;
                    if let Some(group) = group {
                        let ticks = self
                            .host
                            .inputs
                            .nominal_ticks
                            .context("lineup requires nominal_ticks")?;
                        let sounds = self
                            .host
                            .sounds
                            .as_mut()
                            .context("lineup turn requires synchronous sounds")?;
                        self.host.registry.request_ordinary_group(
                            self.player,
                            group,
                            false,
                            ticks,
                            &mut **sounds,
                        )?;
                    }
                }
            }
            Ok(facing.ongoing())
        })();
        self.record(false, result)
    }
    fn arrival(&mut self, x: i32, z: i32) -> bool {
        let result = (|| -> Result<bool> {
            let distance = self.horizontal_distance(x, z)?;
            if distance > alignment::arrival_threshold(self.speed(), self.frame_scale()?) {
                return Ok(false);
            }
            let (position, _) = self.pose()?;
            self.set_pose(Some([x, position[1], z]), None)?;
            Ok(true)
        })();
        self.record(false, result)
    }
    fn permitted(&mut self, x: i32, z: i32) -> bool {
        let result = (|| -> Result<bool> {
            let state = self.host.registry.actors[self.player.index]
                .control
                .as_ref()
                .context("lineup selected-player control unavailable")?
                .death_selector;
            Ok(alignment::movement_permitted(
                state,
                self.speed(),
                self.frame_scale()?,
                self.horizontal_distance(x, z)?,
            ))
        })();
        self.record(true, result)
    }
    fn step(&mut self, step: CloseStep, x: i32, z: i32) -> bool {
        close_step_run(self, step, x, z)
    }
    fn turn_to(&mut self, heading: u16) -> bool {
        let result = self.turn(heading);
        self.record(false, result)
    }
    fn heading(&self) -> u16 {
        StepPort::heading(self)
    }
    fn relative_bearing(&self, x: i32, z: i32) -> u16 {
        let (position, angles) = self.pose().unwrap_or_default();
        alignment::relative_bearing([x - position[0], z - position[2]], angles[1])
    }
    fn default_animation(&mut self) -> bool {
        let inputs = self.host.inputs;
        let sounds: Option<
            &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
        > = match self.host.sounds.as_mut() {
            Some(sounds) => Some(&mut **sounds),
            None => None,
        };
        let result = self
            .host
            .registry
            .admit_point_default(self.player, inputs, sounds);
        self.record((), result);
        true
    }
    fn set_called_flags(&mut self, value: bool) {
        let flags = &mut self.host.registry.globals.locomotion_called;
        *flags = if value { *flags | 0xf } else { *flags & !0xf };
    }
    fn abandon(&mut self, clear_hook_flag: bool) {
        self.host.registry.globals.native_command_return = 6;
        if clear_hook_flag
            && let Some(control) = self.host.registry.actors[self.player.index]
                .control
                .as_mut()
        {
            control.script_control &= !2;
        }
    }
}

impl LineupHost for PlayerLineup<'_, '_, '_, '_, '_> {
    fn reference(&mut self) -> Result<([i32; 3], u16)> {
        let local = self
            .host
            .registry
            .spatial(self.host.registry.id(self.host.index)?)
            .context("lineup receiver pose unavailable")?
            .local;
        Ok((local.position, local.angles[1]))
    }
    fn failure(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }
}

impl GameHost<'_, '_, '_, '_> {
    /// First invocation of a lineup native. The receiver's native tasks leave the actor for the
    /// call so the player-side helpers can use the world, including the nested player step.
    pub(super) fn lineup_call(
        &mut self,
        function: &str,
        kind: super::CallKind,
        arguments: &[super::Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<Option<super::CallResult>> {
        let mut port = PlayerLineup::new(self)?;
        let mut tasks = std::mem::take(&mut port.host.registry.actors[port.host.index].tasks);
        let result = tasks.call(
            (function, kind),
            arguments,
            invocation,
            &mut None,
            &mut None,
            crate::script::tasks::NativeServices {
                lineup: Some(&mut port),
                ..Default::default()
            },
        );
        let index = port.host.index;
        port.host.registry.actors[index].tasks = tasks;
        result
    }

    /// Later invocation of a lineup native's continuation.
    pub(super) fn lineup_poll(
        &mut self,
        token: u64,
        arguments: &[super::Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<bool> {
        let mut port = PlayerLineup::new(self)?;
        let mut tasks = std::mem::take(&mut port.host.registry.actors[port.host.index].tasks);
        let result = tasks.poll(
            token,
            arguments,
            invocation,
            &mut None,
            &mut None,
            crate::script::tasks::NativeServices {
                lineup: Some(&mut port),
                ..Default::default()
            },
        );
        let index = port.host.index;
        port.host.registry.actors[index].tasks = tasks;
        result
    }
}
