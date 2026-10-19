//! Point navigation retains walking, direct visibility, held-node routes and command transitions.
//!
//! Destinations and tolerances use Q8 local coordinates. The point command owns
//! its retained phase/history and pending registration; movement owns collision,
//! sounds and animation effects. Completion releases nodes and publishes the
//! native return byte without registering another pending update.
//!
//! Destination search uses the current enabled map without filtering reservations.
//! Route claims preserve reservation order. Direct phases share live visibility,
//! facing and walking services. Initialization,
//! jump continuation and collision/hit transitions remain explicit unsupported
//! branches; supported phases do not imply complete automatic combat traversal.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Walk toward a local Q8 horizontal destination with a Q8 arrival tolerance.
    /// Preservation's coordinate-walking rule selects WalkForward at attribute 93
    /// zero and MoveForward otherwise. An overshoot restores playback, displayed
    /// frame, packed movement counters and local position before snapping X/Z.
    /// Movement direction, collision responses and sound effects remain owned by
    /// movement. The enclosing point wrapper owns deferred map registration.
    pub fn walk_ordinary_point(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        mut sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            let query_position = self.query_position(id)?;
            let retained_query_position = (self.actors[index].attributes[26] != 0
                || self.spatial(id)?.linked_position.is_some())
            .then_some(query_position);
            let position = self.spatial(id)?.local.position;
            let movement_flags = self.actors[index]
                .control
                .as_ref()
                .context("coordinate walking control unavailable")?
                .movement_flags;
            if !self.point_destination_arrived(index, destination, tolerance, inputs)? {
                let animation = self.actors[index]
                    .animation
                    .as_ref()
                    .context("coordinate walking animation unavailable")?;
                let checkpoint = animation.playback.checkpoint()?;
                let frame = animation.frame;
                let movement = if self.actors[index].attributes[93] == 0 {
                    super::spacing::OrdinaryMovement::WalkForward
                } else {
                    super::spacing::OrdinaryMovement::Forward
                };
                self.move_ordinary(
                    id,
                    movement,
                    1,
                    0,
                    inputs,
                    sounds
                        .as_mut()
                        .context("coordinate walking requires synchronous sounds")?,
                )?;
                let origin = retained_query_position.unwrap_or(self.spatial(id)?.local.position);
                let bearing = super::super::spatial::query_bearing(
                    destination[0].wrapping_sub(origin[0]),
                    destination[2].wrapping_sub(origin[2]),
                );
                let heading = self.spatial(id)?.local.angles[1];
                let difference = heading.wrapping_sub(bearing) & 2047;
                if !(256..=1792).contains(&difference) {
                    return Ok(1);
                }
                self.globals.locomotion_called &= !4;
                let actor = &mut self.actors[index];
                actor
                    .control
                    .as_mut()
                    .expect("admitted coordinate walking control")
                    .movement_flags = movement_flags;
                let animation = actor
                    .animation
                    .as_mut()
                    .expect("admitted coordinate walking animation");
                animation.playback.restore_checkpoint(&checkpoint)?;
                animation.frame = frame;
            }
            self.spatial
                .as_mut()
                .expect("admitted coordinate walking spatial state")
                .actor_mut(index)?
                .local
                .position = [destination[0], position[1], destination[2]];
            self.admit_point_default(id, inputs, sounds)?;
            Ok(0)
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} coordinate walking to {destination:?}",
                self.world, id.index
            )
        })
    }

    /// Invoke the ordinary point wrapper with a Q8 destination and arrival tolerance.
    /// Map validation precedes shared pause consumption. A consumed pause returns one
    /// even when it reaches zero. Immediate arrival and falling/queued motion use live
    /// actor owners. Phases one and 27 support held-node arrival and facing,
    /// including attribute-90 delay admission. Phase nine retains countdown and idle
    /// requests on the session random stream. Phase two walks and claims held nodes;
    /// phase eight selects routes and phases 11/21/22 continue direct walking.
    /// Initialization, jump routes and hit reactions remain unavailable. Requests require
    /// nominal ticks and a synchronous sound sink; random is required only on
    /// admitted draw paths.
    pub fn move_ordinary_point(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            self.actors[index]
                .control
                .as_ref()
                .context("point control unavailable")?
                .point_command
                .load(&mut self.globals.native_command);
            self.validate_walk_navigation(id)?;
            let result = if self.globals.native_command.pause_ticks != 0 {
                let ticks = inputs
                    .nominal_ticks
                    .context("point pause requires nominal_ticks")?;
                self.globals.native_command.pause_ticks = self
                    .globals
                    .native_command
                    .pause_ticks
                    .saturating_sub(ticks);
                1
            } else {
                self.move_point_entry(id, destination, tolerance, inputs, random, sounds)?
            };
            let shared = &mut self.globals.native_command;
            self.actors[index]
                .control
                .as_mut()
                .expect("admitted point control")
                .point_command
                .save(shared);
            let frame_scale = inputs
                .frame_scale
                .context("point wrapper requires frame_scale")?;
            let spatial = self
                .spatial
                .as_mut()
                .context("point spatial state unavailable")?
                .actor_mut(index)?;
            super::super::navigation::limit_point_displacement(spatial, frame_scale)?;
            Ok(result)
        })();
        result.with_context(|| format!("world {} actor {} point movement", self.world, id.index))
    }

    fn move_point_entry(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<i32> {
        let index = id.index;
        let control = self.actors[index]
            .control
            .as_ref()
            .expect("admitted point control");
        anyhow::ensure!(
            control.body_response_flags & 4 == 0
                || self.globals.native_command.point_phase == 27
                || control.overlap_contact_identifier == 1,
            "point hit-reaction random service unsupported"
        );
        let shared = &mut self.globals.native_command;
        shared.point_counter = if control.collision_response_auxiliary[0] & 128 != 0
            && control.navigation.next != -1
            && control.navigation.map.is_some()
        {
            shared.point_counter.wrapping_add(1)
        } else {
            0
        };
        if shared.point_counter > 8 {
            shared.point_phase = 24;
        }
        anyhow::ensure!(
            !self.globals.walk_route.target_exclusion,
            "point held-target exclusion admission unsupported"
        );
        if control.falling_state != 0 || control.queued_motion != 0 {
            shared.point_phase = u8::from(control.navigation.next != -1);
            let actor = &mut self.actors[index];
            actor.attributes[1] = 13;
            actor
                .control
                .as_mut()
                .expect("admitted point control")
                .death_selector = 7;
            return Ok(1);
        }
        anyhow::ensure!(
            control.motion_direction != 15,
            "point direction-15 movement unsupported"
        );
        let phase = shared.point_phase;
        if matches!(phase, 11 | 22)
            || !self.point_destination_arrived(index, destination, tolerance, inputs)?
        {
            return self.move_point_held_node(id, destination, tolerance, inputs, random, sounds);
        }
        let position = &mut self
            .spatial
            .as_mut()
            .expect("admitted point spatial state")
            .actor_mut(index)?
            .local
            .position;
        position[0] = destination[0];
        position[2] = destination[2];
        self.admit_point_default(id, inputs, sounds)?;
        self.globals.native_command_return = 0;
        self.release_held_walk_nodes(id)?;
        let actor = &mut self.actors[index];
        let control = actor.control.as_mut().expect("admitted point control");
        if control.death_selector == 7 {
            control.death_selector = 0;
        }
        actor.attributes[1] = 13;
        Ok(0)
    }

    fn point_destination_arrived(
        &self,
        index: usize,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
    ) -> Result<bool> {
        let position = self
            .spatial
            .as_ref()
            .context("point spatial state unavailable")?
            .actor(index)?
            .local
            .position;
        let speed = self.ordinary_group_speed(index);
        let speed = if speed != 0 {
            speed
        } else {
            [8, 19, 17, 12, 9, 13, 10]
                .into_iter()
                .map(|attribute| self.actors[index].attributes[attribute])
                .filter(|&value| value != 0)
                .min()
                .unwrap_or(8)
        };
        let scale = inputs
            .frame_scale
            .context("point arrival requires frame_scale")?;
        let distance = super::super::spatial::rounded_distance([
            position[0].wrapping_sub(destination[0]),
            0,
            position[2].wrapping_sub(destination[2]),
        ])?;
        let threshold = i32::from(speed)
            .wrapping_add(2)
            .wrapping_mul(256)
            .wrapping_mul(scale as i32)
            >> 8;
        Ok(distance.wrapping_sub(tolerance) <= threshold)
    }

    fn admit_point_default(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<()> {
        let index = id.index;
        let actor = &mut self.actors[index];
        if self.updates.animation_system
            && !actor
                .control
                .as_ref()
                .expect("admitted control")
                .object_update_excluded
            && let Some(animation) = actor.animation.as_mut()
            && animation.playback.current_group() != Some(animation.playback.default_group())
        {
            let ticks = inputs
                .nominal_ticks
                .context("point default animation requires nominal_ticks")?;
            let sounds = sounds.context("point default animation requires synchronous sounds")?;
            let surface = actor.surface;
            let position = super::animation_sound_position(self.spatial.as_ref(), index);
            let group = animation.playback.default_group();
            let frame =
                animation
                    .playback
                    .force_request_with_sounds(group, 0, ticks, &mut |sound| {
                        sounds(
                            id,
                            super::super::ScriptSound::Animation {
                                sound,
                                surface,
                                position,
                            },
                        )
                    })?;
            animation.frame = frame.or(animation.frame);
        }
        Ok(())
    }

    fn advance_point_delay(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<()> {
        let ticks = inputs
            .nominal_ticks
            .context("point delay requires nominal_ticks")?;
        let command = &mut self.globals.native_command;
        let word = command.point_word;
        command.point_word = if (i32::from(word as i16) as u32) < ticks {
            0
        } else {
            word.wrapping_sub(ticks as u16)
        };
        if command.point_word == 0 {
            command.point_phase = 2;
            return Ok(());
        }
        let (_, holding_weapon) = self.spacing_weapon(id, inputs)?;
        if holding_weapon {
            self.globals.native_command.point_phase = 2;
            return Ok(());
        }
        let animation = self.actors[id.index]
            .animation
            .as_ref()
            .context("point delay animation unavailable")?;
        let current = animation
            .playback
            .current_group()
            .context("point delay current animation group unavailable")?;
        if current == animation.playback.default_group() {
            let random = random.context("point delay requires shared engine random stream")?;
            if random.draw() & 31 == 31 {
                let group = loop {
                    let choice = random.draw() & 3;
                    if choice < 3 {
                        break (choice + 1) as u16;
                    }
                };
                self.request_ordinary_group(
                    id,
                    group,
                    false,
                    ticks,
                    sounds.context("point delay requires synchronous sounds")?,
                )?;
            }
        }
        Ok(())
    }

    fn move_point_held_node(
        &mut self,
        id: ScriptActorId,
        point: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        mut sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<i32> {
        let index = id.index;
        let actor = &mut self.actors[index];
        let control = actor.control.as_ref().expect("admitted point control");
        let navigation = control.navigation;
        anyhow::ensure!(
            navigation.map.is_some(),
            "ordinary combat routed point navigation unsupported: map selection"
        );
        actor.attributes[1] = 12;
        anyhow::ensure!(
            control.collision_transition_flags & 1 == 0
                && control.collision_response_auxiliary[0] & 0x82 == 0
                && control.body_response_flags & 0x0a == 0,
            "point route collision response unsupported"
        );
        let phase = self.globals.native_command.point_phase;
        anyhow::ensure!(
            matches!(phase, 1 | 2 | 8 | 9 | 11 | 21 | 22 | 27),
            "ordinary combat routed point navigation unsupported: phase {phase}"
        );
        if phase == 8 {
            if let Some(result) = self.advance_point_route(id, point, inputs)? {
                return Ok(result);
            }
        } else if matches!(phase, 11 | 21 | 22) {
            if let Some(result) = self.advance_point_direct(id, point, tolerance, inputs, sounds)? {
                return Ok(result);
            }
        } else if phase == 9 {
            self.advance_point_delay(id, inputs, random, sounds)?;
        } else if navigation.next == -1 {
            self.release_held_walk_nodes(id)?;
            self.globals.native_command.point_phase = 0;
        } else {
            let destination = self
                .walk_nodes
                .as_ref()
                .context("walk-node navigation not prepared")?
                .position(navigation.map.expect("admitted point map"), navigation.next)?;
            if phase == 2 {
                let exited = self.actors[index]
                    .animation
                    .as_ref()
                    .context("point walking animation unavailable")?
                    .playback
                    .exit_reached();
                if !exited {
                    let facing = self.point_facing_inputs(id, inputs)?;
                    self.face_ordinary_point(
                        id,
                        destination,
                        facing,
                        sounds
                            .as_mut()
                            .context("point facing requires synchronous sounds")?,
                    )?;
                }
                if self.walk_ordinary_point(
                    id,
                    destination,
                    0,
                    inputs,
                    sounds.as_mut().map(|sink| {
                        sink as &mut dyn FnMut(
                            ScriptActorId,
                            super::super::ScriptSound,
                        ) -> Result<()>
                    }),
                )? == 0
                {
                    self.admit_point_default(id, inputs, sounds)?;
                    if self.point_destination_arrived(index, destination, 0, inputs)? {
                        self.assign_walk_node(
                            id,
                            navigation.next,
                            super::super::WalkNodeSlot::Current,
                        )?;
                        self.globals.native_command.point_phase = 8;
                    } else {
                        self.globals.native_command.point_phase = 0;
                    }
                }
            } else {
                let scale = inputs
                    .frame_scale
                    .context("point node arrival requires frame_scale")?;
                let threshold = (i32::from(self.ordinary_group_speed(index)) + 1)
                    .wrapping_mul(256)
                    .wrapping_mul(scale as i32)
                    >> 8;
                if self.snap_ordinary_horizontal(index, destination, threshold)? {
                    self.admit_point_default(id, inputs, sounds)?;
                    self.assign_walk_node(
                        id,
                        navigation.next,
                        super::super::WalkNodeSlot::Current,
                    )?;
                    self.globals.native_command.point_phase = 8;
                } else {
                    let facing_inputs = self.point_facing_inputs(id, inputs)?;
                    let facing = self.face_ordinary_point(
                        id,
                        destination,
                        facing_inputs,
                        sounds.context("point facing requires synchronous sounds")?,
                    )?;
                    if facing == 0 {
                        self.globals.native_command.point_phase = 2;
                        let delay = self.actors[index].attributes[90];
                        if delay != 0 {
                            let random = random
                                .context("point facing requires shared engine random stream")?;
                            if random.rnd(i32::from(delay))? == 0 {
                                self.globals.native_command.point_phase = 9;
                                self.globals.native_command.point_word =
                                    ((random.draw() & 127) + 16) as u16;
                            }
                        }
                    }
                }
            }
        }
        self.actors[index]
            .control
            .as_mut()
            .expect("admitted point control")
            .death_selector = 7;
        if !self.map_updates.contains(&index) {
            self.map_updates.push(index);
        }
        Ok(1)
    }

    fn complete_point(&mut self, id: ScriptActorId, result: u8) -> Result<i32> {
        self.globals.native_command_return = result;
        self.release_held_walk_nodes(id)?;
        let actor = &mut self.actors[id.index];
        let control = actor.control.as_mut().expect("admitted point control");
        if control.death_selector == 7 {
            control.death_selector = 0;
        }
        actor.attributes[1] = 13;
        Ok(0)
    }

    fn point_visibility(
        &self,
        id: ScriptActorId,
        point: [i32; 3],
        inputs: ScriptInputs<'_>,
    ) -> Result<bool> {
        let geometry = inputs
            .locomotion_geometry
            .context("point visibility geometry unavailable")?;
        self.point_direct_visible(
            id,
            geometry
                .environment
                .selected_player
                .context("point visibility selected player unavailable")?,
            point,
            super::obstacle::OrdinaryObstacleQuery {
                scene: geometry.ground.scene,
                animations: geometry.ground.animations,
                prepared: geometry.ground.prepared,
                vertical_offset: 0,
                respond: false,
            },
        )
    }

    // None continues through the common pending tail; Some completes the wrapper.
    fn advance_point_route(
        &mut self,
        id: ScriptActorId,
        point: [i32; 3],
        inputs: ScriptInputs<'_>,
    ) -> Result<Option<i32>> {
        let navigation = self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted point control")
            .navigation;
        if navigation.current == -1 {
            self.release_held_walk_nodes(id)?;
            self.globals.native_command.point_phase = 0;
            return Ok(None);
        }
        let Some(destination) = self.nearest_current_walk_node(id, point)? else {
            return self.complete_point(id, 2).map(Some);
        };
        let destination = destination as i32;
        if destination == navigation.current {
            self.release_held_walk_nodes(id)?;
            if self.actors[id.index].attributes[95] != 0 {
                self.globals.native_command.point_phase = 21;
            } else if self.point_visibility(id, point, inputs)? {
                self.globals.native_command.point_phase = 22;
            } else {
                return self.complete_point(id, 4).map(Some);
            }
        } else {
            let next = self.next_walk_node(id, destination, None)?;
            if next == -1 {
                return self.complete_point(id, 3).map(Some);
            }
            if !self.assign_walk_node(id, next, super::super::WalkNodeSlot::Next)? {
                return self.complete_point(id, 7).map(Some);
            }
            self.globals.native_command.point_phase = if self.globals.walk_route.link_flags[0] == 0
            {
                1
            } else {
                12
            };
        }
        Ok(None)
    }

    fn advance_point_direct(
        &mut self,
        id: ScriptActorId,
        point: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
        mut sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
    ) -> Result<Option<i32>> {
        let phase = self.globals.native_command.point_phase;
        if phase == 21 {
            self.admit_point_default(id, inputs, sounds)?;
            return self.complete_point(id, 0).map(Some);
        }
        self.release_held_walk_nodes(id)?;
        if self.actors[id.index].attributes[95] != 0 {
            self.globals.native_command.point_phase = 21;
            return Ok(None);
        }
        if phase == 22 {
            let facing = self.point_facing_inputs(id, inputs)?;
            if self.face_ordinary_point(
                id,
                point,
                facing,
                sounds.context("point facing requires synchronous sounds")?,
            )? == 0
            {
                self.globals.native_command.point_phase = 11;
            }
            return Ok(None);
        }
        if !self.point_visibility(id, point, inputs)? {
            self.globals.native_command.point_phase = 0;
            return Ok(None);
        }
        self.actors[id.index].attributes[1] = 13;
        if self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted point control")
            .sliding_state
            != 0
        {
            self.globals.native_command.point_phase = 0;
        }
        if self.walk_ordinary_point(
            id,
            point,
            tolerance,
            inputs,
            sounds.as_mut().map(|sink| {
                sink as &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>
            }),
        )? == 0
        {
            return self.complete_point(id, 0).map(Some);
        }
        if !self.actors[id.index]
            .animation
            .as_ref()
            .context("point walking animation unavailable")?
            .playback
            .exit_reached()
        {
            let facing = self.point_facing_inputs(id, inputs)?;
            self.face_ordinary_point(
                id,
                point,
                facing,
                sounds.context("point facing requires synchronous sounds")?,
            )?;
        }
        if !self.map_updates.contains(&id.index) {
            self.map_updates.push(id.index);
        }
        if !self.point_visibility(id, point, inputs)? {
            self.globals.native_command.point_phase = 0;
        }
        Ok(None)
    }

    fn point_facing_inputs(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
    ) -> Result<super::slope::OrdinarySlopeInputs> {
        let (selected_player, holding_weapon) = self.spacing_weapon(id, inputs)?;
        Ok(super::slope::OrdinarySlopeInputs {
            selected_player,
            holding_weapon,
            combat_active: self.actors[id.index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| combat.flags & 4 != 0),
            frame_scale: inputs
                .frame_scale
                .context("point facing requires frame_scale")?,
            ticks: inputs
                .nominal_ticks
                .context("point facing requires nominal_ticks")?,
        })
    }
}
