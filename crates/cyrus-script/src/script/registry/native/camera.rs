//! Camera native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result, apply_player_lockout};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_camera(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::CameraController {
            anyhow::ensure!(
                arguments.is_empty(),
                "cameraController requires no arguments"
            );
            let service = self
                .inputs
                .camera_update
                .context("cameraController: camera update service unavailable")?;
            self.registry.run_camera_controller(service, self.inputs)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::XResetCamera {
            anyhow::ensure!(arguments.is_empty(), "XResetCamera requires no arguments");
            let service = self
                .inputs
                .camera_update
                .context("XResetCamera: camera update service unavailable")?;
            let player = self
                .registry
                .selected_player_index()
                .context("XResetCamera: selected player unavailable")?;
            let camera = self
                .registry
                .camera
                .as_ref()
                .context("XResetCamera: camera session unavailable")?
                .camera_actor
                .index;
            anyhow::ensure!(
                self.index != player && self.index != camera,
                "XResetCamera called by the player or camera actor is unsupported"
            );
            let reset = self.registry.reset_camera_session(player, service);
            self.finish_camera_update(reset.is_ok())?;
            reset?;
            let lockout = self.registry.globals.lockout_player(0);
            apply_player_lockout(
                &mut self.registry.actors,
                &mut self.registry.activation,
                Some(player),
                lockout,
            );
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
