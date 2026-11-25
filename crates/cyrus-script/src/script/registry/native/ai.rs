//! Ai native-call dispatch.
use super::super::{AiType, Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_ai_control(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // SetAIType and SetAIMode store the low byte of their argument in the bytes that
        // MyAIType and MyAIMode read back.
        if function == Native::SetAiType || function == Native::SetAiMode {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()? as u8;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            if function == Native::SetAiType {
                self.registry
                    .set_actor_ai_type(self.index, AiType::from_byte(value));
            } else {
                control.ai_mode = value;
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
