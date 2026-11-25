//! World native-call dispatch.
use super::super::{Argument, CallKind, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_map_status(
        &mut self,
        function: Native,
    ) -> Result<Option<CallResult>> {
        // IsWithinMap is constant true. LoadWorld, WonGame, PushPlayer and PopPlayer only raise
        // a request that the original's main loop consumes; the host takes it from the globals.
        if function == Native::IsWithinMap {
            return Ok(Some(CallResult::Complete(1)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_world_control(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::SpeedScale {
            let [value] = arguments else {
                anyhow::bail!("SpeedScale requires one argument")
            };
            let scale = self
                .inputs
                .frame_scale
                .context("SpeedScale requires frame_scale")?;
            return Ok(Some(CallResult::Complete(
                value.integer()?.wrapping_mul(scale as i32) >> 8,
            )));
        }

        if kind == CallKind::Function && function == Native::MainloopExit {
            let [code] = arguments else {
                anyhow::bail!("MainloopExit requires one argument")
            };
            let code = code.integer()?;
            if code >= 5000 {
                self.registry.globals.request_mut().mainloop_exit = Some(code);
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // updatePlayerViewAttrib is an empty routine in the original, with no portable result.
        if kind == CallKind::Function && function == Native::UpdatePlayerViewAttrib {
            anyhow::ensure!(
                arguments.is_empty(),
                "updatePlayerViewAttrib requires no arguments"
            );
            return Ok(Some(CallResult::CompleteWithoutValue));
        }

        if kind == CallKind::Function && function == Native::QueueMovie {
            let [movie] = arguments else {
                anyhow::bail!("QueueMovie requires one argument")
            };
            self.registry.globals.queue_movie(movie.integer()?);
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_world_request(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::LoadWorld {
            let [world, a, b] = arguments else {
                anyhow::bail!("LoadWorld requires three arguments")
            };
            self.registry.globals.request_mut().load_world =
                Some([world.integer()?, a.integer()?, b.integer()?]);
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::WonGame {
            self.registry.globals.request_mut().won_game = true;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::PushPlayer || function == Native::PopPlayer {
            let request = self.registry.globals.request_mut();
            if function == Native::PushPlayer {
                request.push_player = true;
            } else {
                request.pop_player = true;
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
