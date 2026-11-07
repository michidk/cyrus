//! AI direct-destination progress composes facing, movement and retained clearance.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::{AiType, ScriptSound};
use crate::script::{OrdinaryMovement, OrdinaryObstacleQuery, OrdinarySlopeInputs};
use anyhow::{Context, Result, ensure};
use cyrus_soup::Attribute;

impl ScriptWorld {
    /// Attempt the direct-destination admission transition within AI progress.
    /// Follows preservation's `SOUP.md#ai-destination-clearance`. Only a mode
    /// other than eleven with zero delay queries retained candidates. A clear
    /// destination releases held nodes before selecting mode eleven. Returns
    /// whether this invocation admitted the destination, without moving or
    /// completing a task. The caller owns arrival checks and scheduling.
    pub fn admit_ai_direct_destination(
        &mut self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        destination: [i32; 3],
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        let result: Result<bool> = (|| {
            let index = self.resolve(id)?;
            let control = self.actors[index]
                .control
                .as_ref()
                .context("AI admission receiver control unavailable")?;
            if control.ai_mode == 11 || control.ai_delay != 0 {
                return Ok(false);
            }
            if !self.ai_destination_clear(id, selected_player, destination, query)? {
                return Ok(false);
            }
            self.release_held_walk_nodes(id)?;
            self.actors[index]
                .control
                .as_mut()
                .expect("admitted AI control")
                .ai_mode = 11;
            Ok(true)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} AI destination {destination:?} admission",
                self.world, id.index
            )
        })
    }

    pub(super) fn move_position_pending(
        &self,
        id: ScriptActorId,
        destination: [i32; 3],
        inputs: ScriptInputs<'_>,
    ) -> Result<i32> {
        let index = self.resolve(id)?;
        let origin = self
            .query_position(id)
            .context("MovePos receiver position unavailable")?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("MovePos receiver control unavailable")?;
        if control.ai_type == AiType::POINT_NAVIGATION {
            return Ok(1);
        }
        let speed = i32::from(self.ordinary_group_speed(index)).wrapping_shl(8);
        if speed == 0 {
            return Ok(1);
        }
        let scale = inputs.frame_scale.context("MovePos requires frame_scale")?;
        let threshold = speed
            .wrapping_mul(2)
            .wrapping_sub(1)
            .wrapping_mul(scale as i32)
            >> 8;
        if threshold <= 0 {
            return Ok(1);
        }
        // The helper converts under masked exceptions, so a length that does not fit a signed
        // 32-bit integer is the integer-indefinite value, never above a positive threshold.
        let distance = cyrus_collision::distance::rounded_distance_or_indefinite([
            destination[0].wrapping_sub(origin[0]),
            0,
            destination[2].wrapping_sub(origin[2]),
        ]);
        Ok(i32::from(distance > threshold))
    }

    /// Advance an admitted AI mode-eleven destination in local Q8 coordinates.
    /// Follows preservation's `SOUP.md#ai-mode-eleven-progress`. The query retains
    /// entry candidates across movement. The caller owns admission and scheduling;
    /// a return of one reports progress and never completes the enclosing task.
    pub fn progress_ai_direct_destination(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        query: OrdinaryObstacleQuery<'_>,
        facing: OrdinarySlopeInputs,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let result = (|| {
            let index = self.resolve(id)?;
            let control = self.actors[index]
                .control
                .as_ref()
                .context("AI progress receiver control unavailable")?;
            ensure!(
                control.ai_mode == 11,
                "AI destination progress requires mode eleven"
            );
            let player = facing
                .selected_player
                .context("AI destination progress selected player unavailable")?;
            self.actors[index].attributes[Attribute::Collide] = 15;
            let control = self.actors[index]
                .control
                .as_mut()
                .context("AI progress receiver control unavailable")?;
            if control.sliding_state != 0 {
                control.ai_mode = 0;
                control.ai_delay = 255;
            }
            let difference = self.face_ordinary_point(id, destination, facing, sounds)?;
            if difference.unsigned_abs() < 256
                && self.move_position_pending(id, destination, inputs)? != 0
            {
                self.move_ordinary(id, OrdinaryMovement::Forward, 1, 0, inputs, sounds)?;
                self.map_updates.register(index)?;
            }
            let clear = self.ai_destination_clear(id, player, destination, query)?;
            let control = self.actors[index]
                .control
                .as_mut()
                .context("AI progress receiver control unavailable")?;
            if !clear || !control.last_collide.is_empty() {
                control.ai_mode = 0;
                control.ai_delay = 255;
            }
            Ok(1)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} AI destination {destination:?} progress",
                self.world, id.index
            )
        })
    }
}
