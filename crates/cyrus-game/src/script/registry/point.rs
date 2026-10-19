//! Point-navigation wrappers retain actor command fields around shared core execution.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Invoke the ordinary point wrapper with a Q8 destination and arrival tolerance.
    /// Map validation precedes shared pause consumption. A consumed pause returns one
    /// even when it reaches zero. Nonpaused route phases remain explicitly unavailable.
    pub fn move_ordinary_point(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        tolerance: i32,
        inputs: ScriptInputs<'_>,
    ) -> Result<i32> {
        let result = (|| {
            let index = self.resolve(id)?;
            self.actors[index]
                .control
                .as_ref()
                .context("point control unavailable")?
                .point_command
                .load(&mut self.globals.native_command);
            self.validate_walk_navigation(id)?;
            let shared = &mut self.globals.native_command;
            anyhow::ensure!(
                shared.pause_ticks != 0,
                "ordinary combat routed point navigation unsupported: phase {} destination {destination:?} tolerance {tolerance}",
                shared.point_phase
            );
            let ticks = inputs
                .nominal_ticks
                .context("point pause requires nominal_ticks")?;
            shared.pause_ticks = shared.pause_ticks.saturating_sub(ticks);
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
            Ok(1)
        })();
        result.with_context(|| format!("world {} actor {} point movement", self.world, id.index))
    }
}
