//! Menu native-call dispatch.
use super::super::{Argument, CallKind, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_menu(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function
            && [Native::MenuNew, Native::MenuAddItem, Native::MenuSelection].contains(&function)
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let expected = if function == Native::MenuAddItem {
                3
            } else {
                0
            };
            anyhow::ensure!(values.len() == expected, "{function}: wrong argument count");
            let globals = &mut self.registry.globals;
            match function {
                Native::MenuNew => globals.menu.clear(),
                Native::MenuSelection => {
                    return Ok(Some(CallResult::Complete(globals.menu.selection())));
                }
                _ => {
                    let limit = globals
                        .dialogue
                        .as_ref()
                        .context("menuAddItem: dialogue settings unavailable")?
                        .settings()
                        .max_menu_items as usize;
                    let hud = self
                        .inputs
                        .hud
                        .context("menuAddItem: installed text unavailable")?;
                    // A label without an installed entry adds nothing.
                    let bytes = values[0].to_le_bytes();
                    if let Some(text) = std::str::from_utf8(&bytes)
                        .ok()
                        .map(|label| hud.activation_label(label))
                        .transpose()?
                        .flatten()
                    {
                        let remembered = globals.highlighted_choice().unwrap_or("").to_owned();
                        globals.menu.add(
                            limit,
                            &text,
                            values[0],
                            values[1],
                            values[2],
                            &remembered,
                        );
                    }
                }
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
