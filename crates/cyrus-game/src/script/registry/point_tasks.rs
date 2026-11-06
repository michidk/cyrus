//! The receiver's side of the point tasks: one ordinary turn step per `FaceAngle`/`FacePos`
//! invocation and one point-wrapper step per `MoveNodeMarker`/`MoveNodeLocation`/`MoveNodePosition`.
use super::{GameHost, ScriptActorId};
use crate::script::tasks::{FaceTarget, MoveTarget, PointHost, PointWrapper};
use anyhow::{Context, Result};
use cyrus_script::Attribute;
use cyrus_script::Native;

// The exact half-turn keeps the sign of the unwrapped difference.
fn tilt_difference(current: i32, target: i32) -> i32 {
    let difference = (target & 2047) - (current & 2047);
    let magnitude = difference.wrapping_abs() & 2047;
    if magnitude > 1024 {
        if difference < 0 {
            2048 - magnitude
        } else {
            magnitude - 2048
        }
    } else if difference < 0 {
        -magnitude
    } else {
        magnitude
    }
}

impl PointHost for GameHost<'_, '_, '_, '_> {
    fn goto_location(&mut self, location: i32) -> Result<i32> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let destination = self.receiver_location("GotoLocation", location)?;
        self.registry.actors[self.index]
            .control
            .as_ref()
            .context("GotoLocation control owner unavailable")?;
        if self.registry.arrive_ordinary_point(id, destination)? {
            self.registry.actors[self.index]
                .control
                .as_mut()
                .expect("admitted GotoLocation control")
                .ai_type = crate::script::AiType::NONE;
            return Ok(0);
        }
        // Timing is demanded only by the nonarrival refresh, after the selector/location writes.
        let actor = &mut self.registry.actors[self.index];
        let control = actor
            .control
            .as_mut()
            .expect("admitted GotoLocation control");
        control.ai_type = crate::script::AiType::FIND_LOCATION;
        control.ai.var1 = location as i16;
        if control.ai_delay != 0 && control.ai_delay != 255 {
            let ticks = self
                .inputs
                .nominal_ticks
                .context("GotoLocation refresh requires nominal_ticks")?;
            control.ai_delay = u32::from(control.ai_delay).saturating_sub(ticks) as u8;
        }
        actor.attributes[Attribute::Collide] = 4;
        Ok(i32::from(control.ai.var1 != -1))
    }

    fn player_offset(&mut self, offset: [i32; 3]) -> Result<()> {
        self.registry
            .offset_selected_player(self.index, offset, true)
    }

    fn player_tilt(
        &mut self,
        angle: i32,
        strength: i32,
        divisor: i32,
        reversed: bool,
    ) -> Result<()> {
        let name = if reversed {
            "PlayerTiltRevx"
        } else {
            "PlayerTilt"
        };
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        anyhow::ensure!(
            self.registry.actors[self.index].attributes[Attribute::Slave] == 0,
            "{name} world {} actor {}: original diagnostic 137: slave receiver",
            self.registry.world,
            self.index
        );
        let shape = self.registry.ordinary_collision_shape(id)?;
        let extent =
            |axis: usize| shape.positive_extent[axis].wrapping_add(shape.negative_extent[axis]);
        let limit = extent(0).max(extent(2)).wrapping_shl(8) / 2;
        let player = self
            .selected_player()
            .with_context(|| format!("{name} selected player unavailable"))?;
        let associated = self.registry.model_support(player)?.actor == Some(id);
        let spatial = self
            .registry
            .spatial
            .as_ref()
            .with_context(|| format!("{name} spatial state unavailable"))?;
        let receiver = spatial.actor(self.index)?;
        let mut targets = [0, 0];
        if associated {
            let player_position = spatial.actor(player.index)?.local.position;
            let effective = self.registry.query_position(id)?;
            let bearing = super::super::spatial::query_bearing(
                effective[0].wrapping_sub(player_position[0]),
                effective[2].wrapping_sub(player_position[2]),
            );
            // PlayerTiltRevx leaves out the receiver heading and negates both targets.
            let heading = if reversed {
                0
            } else {
                receiver.local.angles[1]
            };
            let a = (-i32::from(bearing))
                .wrapping_add(heading)
                .wrapping_add(angle)
                & 2047;
            let pitch = -(if a < 1024 { 512 - a } else { a - 1536 });
            let roll = if a < 512 {
                a
            } else if a < 1536 {
                1024 - a
            } else {
                a - 2048
            };
            let (pitch, roll) = if reversed {
                (-pitch, -roll)
            } else {
                (pitch, roll)
            };
            let distance = crate::script::spatial::rounded_distance([
                receiver.local.position[0].wrapping_sub(player_position[0]),
                0,
                receiver.local.position[2].wrapping_sub(player_position[2]),
            ])?
            .min(limit);
            let reciprocal = 16_777_216i32
                .checked_div(limit)
                .with_context(|| format!("{name} distance limit causes original division fault"))?;
            let factor = reciprocal.wrapping_mul(distance) >> 16;
            let strength = strength.wrapping_mul(factor) >> 8;
            targets = [pitch, roll].map(|target| (target.wrapping_mul(strength) >> 8) & 2047);
        }
        let current = receiver.local.angles;
        if [current[0], current[2]] == targets {
            return Ok(());
        }
        let ticks = self
            .inputs
            .nominal_ticks
            .with_context(|| format!("{name} requires nominal_ticks"))? as i32;
        let mut angles = current;
        for (axis, target) in [0, 2].into_iter().zip(targets) {
            let difference = tilt_difference(current[axis], target);
            let step = if divisor == 0 {
                difference
            } else {
                (difference / divisor).max(1)
            };
            let step = step.wrapping_mul(ticks);
            let value = if difference > step {
                current[axis].wrapping_add(step)
            } else {
                target
            };
            angles[axis] = value;
        }
        self.registry
            .spatial
            .as_mut()
            .unwrap()
            .actor_mut(self.index)?
            .refresh_rotation(angles)
            .with_context(|| format!("{name} orientation refresh"))
    }

    fn dragon_breath(&mut self) -> Result<bool> {
        self.breathe()
    }

    fn projectile(&mut self) -> Result<bool> {
        self.fly_projectile()
    }

    fn move_to(
        &mut self,
        wrapper: PointWrapper,
        target: MoveTarget,
        tolerance: i32,
    ) -> Result<i32> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let destination = match target {
            MoveTarget::Position(position) => position,
            // Marker positions are map units shifted to Q8; the target height drops by the
            // receiver's positive Y extent before the shift.
            MoveTarget::Marker(marker) => {
                let position = self
                    .registry
                    .markers
                    .as_ref()
                    .context("move task markers not prepared")?
                    .position(marker)?;
                let extent = self.registry.ordinary_collision_shape(id)?.positive_extent[1];
                [
                    position[0],
                    position[1].wrapping_sub(extent.wrapping_shl(8)),
                    position[2],
                ]
            }
            MoveTarget::Location(location) => {
                self.receiver_location("move task location", location)?
            }
        };
        let inputs = self.inputs;
        let sounds = self.sounds.as_mut().map(|sink| {
            &mut **sink as &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>
        });
        // A script task keeps its point state in its own slot's native command, which the task
        // pass has already loaded.
        let (world, index) = (self.registry.world, self.index);
        let random = self.random.as_deref_mut();
        match wrapper {
            PointWrapper::Node => {
                self.registry
                    .move_point_wrapper(id, destination, tolerance, inputs, random, sounds)
            }
            PointWrapper::Direct => {
                self.registry
                    .move_point_direct(id, destination, tolerance, inputs, random, sounds)
            }
        }
        .with_context(|| format!("world {world} actor {index} point movement"))
    }

    fn face(&mut self, target: FaceTarget) -> Result<i32> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        if self.registry.actors[self.index].attributes[Attribute::TurnSpeed] == 0 {
            // A receiver that cannot turn completes every facing task at once.
            return Ok(0);
        }
        let inputs = self.registry.point_facing_inputs(id, self.inputs)?;
        let sounds = self
            .sounds
            .as_mut()
            .context("facing requires synchronous sounds")?;
        match target {
            // Truncation keeps the heading modulo 2048, which is all the turn reads.
            FaceTarget::Angle(angle) => {
                self.registry
                    .face_ordinary_angle(id, angle as u16, inputs, &mut **sounds)
            }
            FaceTarget::Point([x, z]) => {
                self.registry
                    .face_ordinary_point(id, [x, 0, z], inputs, &mut **sounds)
            }
            FaceTarget::Actor(actor) => {
                // The target's local X/Z are read on every step; a stale or absent target is an
                // explicit error (the original dereferences it).
                let position = self
                    .registry
                    .spatial(actor)
                    .context("FaceObject target")?
                    .local
                    .position;
                self.registry.face_ordinary_point(
                    id,
                    [position[0], 0, position[2]],
                    inputs,
                    &mut **sounds,
                )
            }
        }
    }
}

impl GameHost<'_, '_, '_, '_> {
    /// A position from the receiver's location list; an out-of-range selector uses its home.
    pub(super) fn receiver_location(&self, context: &str, location: i32) -> Result<[i32; 3]> {
        let locations = &self
            .registry
            .spatial
            .as_ref()
            .with_context(|| format!("{context} spatial state unavailable"))?
            .actor(self.index)?
            .locations;
        let count = i32::try_from(locations.len())
            .with_context(|| format!("{context} location count exceeds signed range"))?;
        let position = if location < 0 || location > count {
            locations
                .first()
                .with_context(|| format!("{context} home unavailable"))?
        } else {
            locations.get(location as usize).with_context(|| {
                format!("{context} selector equals location count: original out-of-range access unsupported")
            })?
        };
        Ok(*position)
    }

    /// First invocation of a facing task. The receiver's native tasks leave the actor for the call
    /// so the facing step can use the world.
    pub(super) fn point_call(
        &mut self,
        function: Native,
        kind: super::CallKind,
        arguments: &[super::Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<Option<super::CallResult>> {
        let mut tasks = std::mem::take(&mut self.registry.actors[self.index].tasks);
        let result = tasks.call(
            (function, kind),
            arguments,
            invocation,
            &mut None,
            &mut None,
            crate::script::tasks::NativeServices {
                point: Some(&mut *self),
                ..Default::default()
            },
        );
        self.registry.actors[self.index].tasks = tasks;
        result
    }

    /// Later invocation of a facing task.
    pub(super) fn point_poll(
        &mut self,
        token: u64,
        arguments: &[super::Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<bool> {
        let mut tasks = std::mem::take(&mut self.registry.actors[self.index].tasks);
        let result = tasks.poll(
            token,
            arguments,
            invocation,
            &mut None,
            &mut None,
            crate::script::tasks::NativeServices {
                point: Some(&mut *self),
                ..Default::default()
            },
        );
        self.registry.actors[self.index].tasks = tasks;
        result
    }
}

/// `AtNodeLocation`: the location's height must lie within half the receiver's total Y extent in
/// whole map units (truncating), and its horizontal rounded distance within `distance`.
pub(super) fn within_location(
    position: [i32; 3],
    target: [i32; 3],
    total_height: i32,
    distance: i32,
) -> Result<bool> {
    if (target[1].wrapping_sub(position[1])).wrapping_abs() >> 8 > total_height / 2 {
        return Ok(false);
    }
    let horizontal = crate::script::spatial::rounded_distance([
        target[0].wrapping_sub(position[0]),
        0,
        target[2].wrapping_sub(position[2]),
    ])?;
    Ok(horizontal <= distance)
}

#[cfg(test)]
mod tests {
    use super::within_location;

    #[test]
    fn at_node_location_tests_height_in_map_units_then_horizontal_distance() {
        let origin = [1000, 5000, -2000];
        // Total height 50: half is 25 map units, 6400 Q8 units, compared after the shift.
        for (target, total, distance, expected) in [
            ([1000 + 768, 5000, -2000 + 1024], 50, 1279, false),
            ([1000 + 768, 5000, -2000 + 1024], 50, 1280, true),
            ([1000, 5000 + 25 * 256 + 255, -2000], 50, 0, true),
            ([1000, 5000 + 26 * 256, -2000], 50, 0, false),
            ([1000, 5000 - 26 * 256, -2000], 50, 0, false),
            ([1000, 5000 - 25 * 256, -2000], 50, 0, true),
            // An odd total truncates: 49 / 2 is 24.
            ([1000, 5000 + 25 * 256, -2000], 49, 0, false),
            ([1000, 5000 + 24 * 256 + 255, -2000], 49, 0, true),
            // The height difference is ignored by the horizontal distance.
            ([1000, 5000 + 24 * 256, -2000], 49, -1, false),
            ([1000, 5000 + 24 * 256, -2000], 49, 0, true),
        ] {
            assert_eq!(
                within_location(origin, target, total, distance).unwrap(),
                expected,
                "target {target:?} total {total} distance {distance}"
            );
        }
    }
}
