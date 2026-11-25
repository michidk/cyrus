//! Model native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_model_visibility(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::HideMe || function == Native::ShowMe {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| {
                    format!("{function}: receiver visibility lifecycle unavailable")
                })?;
            control.hidden = function == Native::HideMe;
            if control.hidden {
                self.registry.actors[self.index].model.current = None;
            } else if self.registry.actors[self.index]
                .model
                .static_radius
                .is_some()
                || (self.registry.actors[self.index].model.prepared && {
                    let data = self
                        .inputs
                        .animations
                        .context("ShowMe: model path metadata unavailable")?
                        .0;
                    self.registry.require_model_identity(data)?;
                    matches!(
                        data.actor(self.registry.actors[self.index].name())?["model_load_path"]
                            .as_str(),
                        Some("static")
                    )
                })
            {
                let spatial = self
                    .registry
                    .spatial
                    .as_mut()
                    .context("ShowMe: model spatial state unavailable")?;
                let state = spatial.actor_mut(self.index)?;
                if state.parent.is_none() {
                    state.refresh_transform()?;
                }
                self.registry.actors[self.index]
                    .model
                    .show_static(state.query_pose()?);
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
