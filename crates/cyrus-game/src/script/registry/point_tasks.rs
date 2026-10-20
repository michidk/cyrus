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
            // The receiver's location list; an out-of-range selector uses its home.
            MoveTarget::Location(location) => {
                let locations = &self
                    .registry
                    .spatial
                    .as_ref()
                    .context("MoveNodeLocation spatial state unavailable")?
                    .actor(self.index)?
                    .locations;
                let count = i32::try_from(locations.len())
                    .context("MoveNodeLocation location count exceeds signed range")?;
                *if location < 0 || location > count {
                    locations
                        .first()
                        .context("MoveNodeLocation home unavailable")?
                } else {
                    locations.get(location as usize).context(
                        "MoveNodeLocation selector equals location count: original out-of-range access unsupported",
                    )?
                }
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
