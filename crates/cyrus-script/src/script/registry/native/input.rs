//! Input native-call dispatch.
use super::super::{Argument, CallKind, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_input(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::SpacePressed {
            anyhow::ensure!(arguments.is_empty(), "SpacePressed requires no arguments");
            let keys = self
                .inputs
                .script_keys
                .context("SpacePressed keyboard sample unavailable")?;
            return Ok(Some(CallResult::Complete(i32::from(keys.space))));
        }

        // PressKey(code): InKey's key and modifier rules, but a success consumes the key's held
        // state until the key is released (a matching latest event still answers one).
        if kind == CallKind::Function && function == Native::PressKey {
            let [code] = arguments else {
                anyhow::bail!("PressKey requires one argument")
            };
            let code = code.integer()? as u32;
            let keys = self
                .inputs
                .script_keys
                .context("PressKey keyboard sample unavailable")?;
            let pressed = keys
                .press_key(code, &mut self.registry.consumed_letters)
                .with_context(|| {
                    format!("PressKey code {code:#x}: key without a recovered scan code")
                })?;
            return Ok(Some(CallResult::Complete(i32::from(pressed))));
        }

        // InKey(code): whether the key of the code's low byte is down, with the modifiers the
        // higher bytes name. Only the debug shell and a few bridge and door scripts call it.
        if kind == CallKind::Function && function == Native::InKey {
            let [code] = arguments else {
                anyhow::bail!("InKey requires one argument")
            };
            let code = code.integer()? as u32;
            let keys = self
                .inputs
                .script_keys
                .context("InKey keyboard sample unavailable")?;
            let down = keys.in_key(code).with_context(|| {
                format!("InKey code {code:#x}: key without a recovered scan code")
            })?;
            return Ok(Some(CallResult::Complete(i32::from(down))));
        }
        Ok(None)
    }
}
