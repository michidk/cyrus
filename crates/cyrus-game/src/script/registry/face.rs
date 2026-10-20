//! The receiver's side of `FaceAngle` and `FacePos`: one ordinary turn step per task invocation.
use super::{GameHost, ScriptActorId};
use crate::script::tasks::{FaceHost, FaceTarget};
use anyhow::{Context, Result};

/// Attribute that gates every facing task: a receiver whose turn rate is zero cannot turn and
/// completes at once.
const TURN_RATE_ATTRIBUTE: usize = 15;

impl FaceHost for GameHost<'_, '_, '_, '_> {
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
    pub(super) fn face_call(
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
                face: Some(&mut *self),
                ..Default::default()
            },
        );
        self.registry.actors[self.index].tasks = tasks;
        result
    }

    /// Later invocation of a facing task.
    pub(super) fn face_poll(
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
                face: Some(&mut *self),
                ..Default::default()
            },
        );
        self.registry.actors[self.index].tasks = tasks;
        result
    }
}
