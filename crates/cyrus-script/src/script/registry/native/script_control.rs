//! Script control native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_script_control(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::DisableHook || function == Native::EnableHook {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver script-control unavailable"))?;
            // Bit 0x02 of the script-control byte suppresses the native hook.
            if function == Native::DisableHook {
                control.script_control |= 2;
            } else {
                control.script_control &= !2;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // Enable*/Disable* clear and set one bit of the script-control byte: 0x01 script,
        // 0x04 tasks, 0x08 object, 0x10 combat (0x02 is the hook, handled above).
        let control_bit = match function {
            Native::EnableScript => Some((true, 0x01)),
            Native::DisableScript => Some((false, 0x01)),
            Native::EnableTasks => Some((true, 0x04)),
            Native::DisableTasks => Some((false, 0x04)),
            Native::EnableObject => Some((true, 0x08)),
            Native::DisableObject => Some((false, 0x08)),
            Native::EnableCombat => Some((true, 0x10)),
            Native::DisableCombat => Some((false, 0x10)),
            _ => None,
        };

        if let Some((enable, bit)) = control_bit {
            anyhow::ensure!(arguments.is_empty(), "{function}: wrong argument count");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: script-control lifecycle unavailable"))?;
            if enable {
                control.script_control &= !bit;
            } else {
                control.script_control |= bit;
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
