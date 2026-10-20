//! The receiver's side of the point tasks: one ordinary turn step per `FaceAngle`/`FacePos`
//! invocation and one point-wrapper step per `MoveNodeMarker`/`MoveNodeLocation`/`MoveNodePosition`.
use super::{GameHost, ScriptActorId};
use crate::script::tasks::{FaceTarget, MoveTarget, PointHost};
use anyhow::{Context, Result};

/// Attribute that gates every facing task: a receiver whose turn rate is zero cannot turn and
/// completes at once.
const TURN_RATE_ATTRIBUTE: usize = 15;

impl PointHost for GameHost<'_, '_, '_, '_> {
    fn move_to(&mut self, target: MoveTarget, tolerance: i32) -> Result<i32> {
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
                    .context("MoveNodeMarker markers not prepared")?
                    .position(marker)?;
                let extent = self.registry.ordinary_collision_shape(id)?.positive_extent[1];
                [
                    position[0],
                    position[1].wrapping_sub(extent.wrapping_shl(8)),
                    position[2],
                ]
            }
            MoveTarget::Location(location) => {
                self.receiver_location("MoveNodeLocation", location)?
            }
        };
        let inputs = self.inputs;
        let sounds = self.sounds.as_mut().map(|sink| {
            &mut **sink as &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>
        });
        self.registry.move_ordinary_point(
            id,
            destination,
            tolerance,
            inputs,
            self.random.as_deref_mut(),
            sounds,
        )
    }

    fn face(&mut self, target: FaceTarget) -> Result<i32> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        if self.registry.actors[self.index].attributes[TURN_RATE_ATTRIBUTE] == 0 {
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
        }
    }
}

impl GameHost<'_, '_, '_, '_> {
    /// A position from the receiver's location list; an out-of-range selector uses its home.
    pub(super) fn receiver_location(&self, native: &str, location: i32) -> Result<[i32; 3]> {
        let locations = &self
            .registry
            .spatial
            .as_ref()
            .with_context(|| format!("{native} spatial state unavailable"))?
            .actor(self.index)?
            .locations;
        let count = i32::try_from(locations.len())
            .with_context(|| format!("{native} location count exceeds signed range"))?;
        let position = if location < 0 || location > count {
            locations
                .first()
                .with_context(|| format!("{native} home unavailable"))?
        } else {
            locations.get(location as usize).with_context(|| {
                format!("{native} selector equals location count: original out-of-range access unsupported")
            })?
        };
        Ok(*position)
    }

    /// First invocation of a facing task. The receiver's native tasks leave the actor for the call
    /// so the facing step can use the world.
    pub(super) fn point_call(
        &mut self,
        function: &str,
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
