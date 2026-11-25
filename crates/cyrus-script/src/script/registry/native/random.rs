//! Random native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_random(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::Rnd || function == Native::QuickRnd {
            let [operand] = arguments else {
                anyhow::bail!("{function} requires one argument")
            };
            let operand = operand.integer()?;
            let random = self
                .random
                .as_deref_mut()
                .with_context(|| format!("{function}: shared engine random stream unavailable"))?;
            let value = if function == Native::Rnd {
                random.rnd(operand)?
            } else {
                random.quick_rnd(operand)
            };
            return Ok(Some(CallResult::Complete(value)));
        }
        Ok(None)
    }
}
