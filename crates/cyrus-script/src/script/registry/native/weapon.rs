//! Weapon native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_weapon_state(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::CheckPlayerWeapon {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            // The supplied receiver is replaced by the selected player; a closed item system is a
            // no-op. The original's return register is residual state, not a result.
            let hud = self
                .inputs
                .hud
                .with_context(|| format!("{function}: inventory owner unavailable"))?;
            if hud.inventory_ready()? {
                let player = self
                    .selected_player()
                    .with_context(|| format!("{function}: selected player unavailable"))?;
                let shell = self
                    .inputs
                    .shell_execution
                    .with_context(|| format!("{function}: shell context unavailable"))?;
                let ticks = self
                    .inputs
                    .nominal_ticks
                    .with_context(|| format!("{function}: nominal ticks unavailable"))?;
                let sounds = self
                    .sounds
                    .as_mut()
                    .with_context(|| format!("{function}: script sound handler absent"))?;
                self.registry
                    .check_player_weapon(player, hud, shell, ticks, &mut **sounds)
                    .with_context(|| function.to_string())?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        if matches!(
            function,
            Native::IsLightingTorch | Native::IsUnlightingTorch | Native::IsLightingOrUnlighting
        ) {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(Some(CallResult::Complete(i32::from(
                self.receiver_torch_predicate(function)
                    .with_context(|| function.to_string())?,
            ))));
        }

        if function == Native::IsCarryingWeapon {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(Some(CallResult::Complete(i32::from(
                self.receiver_carrying_weapon()
                    .with_context(|| function.to_string())?,
            ))));
        }

        if [
            Native::IsHoldingWeapon,
            Native::IsDrawingSword,
            Native::IsSheathingSword,
            Native::IsDrawingOrSheathing,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let (holding, drawing, sheathing) = self.receiver_weapon_predicates(function)?;
            let result = if function == Native::IsHoldingWeapon {
                holding
            } else if function == Native::IsDrawingSword {
                drawing
            } else if function == Native::IsDrawingOrSheathing {
                drawing || sheathing
            } else {
                sheathing
            };
            return Ok(Some(CallResult::Complete(i32::from(result))));
        }
        Ok(None)
    }
}
