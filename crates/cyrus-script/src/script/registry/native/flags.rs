//! Flags native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_flags(
        &mut self,
        function: Native,
        arguments: &[Argument],
        flags: &mut [i32],
    ) -> Result<Option<CallResult>> {
        if let Some(value) = self.registry.flag_state.call(function, arguments, flags)? {
            return Ok(Some(CallResult::Complete(value)));
        }
        Ok(None)
    }
}
