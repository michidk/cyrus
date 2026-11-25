//! Tasks native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_clear_tasks(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // KillMyTasks clears all six of the receiver's task slots at once, without running a handler.
        if function == Native::KillMyTasks {
            anyhow::ensure!(arguments.is_empty(), "KillMyTasks requires no arguments");
            return Ok(Some(CallResult::CompleteAfterClearingTasks(0)));
        }
        Ok(None)
    }
}
