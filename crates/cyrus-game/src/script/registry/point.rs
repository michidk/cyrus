//! Point navigation retains actor commands through pauses, immediate arrival and held-node transitions.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Invoke the ordinary point wrapper with a Q8 destination and arrival tolerance.
    /// Map validation precedes shared pause consumption. A consumed pause returns one
    /// even when it reaches zero. Immediate arrival and falling/queued motion use live
    /// actor owners. Held-node arrival supports phases one and 27; walking, later
    /// route phases and hit reactions remain unavailable.
    /// Default-animation requests require nominal ticks and a synchronous sound sink.
    pub fn move_ordinary_point(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
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
                self.move_point_entry(id, destination, tolerance, inputs, sounds)?
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
            return self.move_point_node_arrival(id, inputs, sounds);
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

    fn move_point_node_arrival(
        &mut self,
        id: ScriptActorId,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>>,
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
            matches!(phase, 1 | 27),
            "ordinary combat routed point navigation unsupported: phase {phase}"
        );
        if navigation.next == -1 {
            self.release_held_walk_nodes(id)?;
            self.globals.native_command.point_phase = 0;
        } else {
            let destination = self
                .walk_nodes
                .as_ref()
                .context("walk-node navigation not prepared")?
                .position(navigation.map.expect("admitted point map"), navigation.next)?;
            let scale = inputs
                .frame_scale
                .context("point node arrival requires frame_scale")?;
            let threshold = (i32::from(self.ordinary_group_speed(index)) + 1)
                .wrapping_mul(256)
                .wrapping_mul(scale as i32)
                >> 8;
            anyhow::ensure!(
                self.snap_ordinary_horizontal(index, destination, threshold)?,
                "ordinary combat routed point navigation unsupported: node facing phase {phase}"
            );
            self.admit_point_default(id, inputs, sounds)?;
            self.assign_walk_node(id, navigation.next, super::super::WalkNodeSlot::Current)?;
            self.globals.native_command.point_phase = 8;
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
}
