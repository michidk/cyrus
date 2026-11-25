//! Activation native-call dispatch.
use super::super::{
    Argument, CallKind, CallResult, GameHost, Native, Result, ScriptActorId, activation,
    apply_player_lockout,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_activation(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::LockoutPlayer {
            let [value] = arguments else {
                anyhow::bail!("LockoutPlayer requires one argument")
            };
            let value = value.integer()?;
            let player = self
                .inputs
                .activation
                .map(|inputs| inputs.player.index)
                .or_else(|| self.registry.selected_player_index())
                .context("LockoutPlayer selected player unavailable")?;
            let lockout = self.registry.globals.lockout_player(value);
            apply_player_lockout(
                &mut self.registry.actors,
                &mut self.registry.activation,
                Some(player),
                lockout,
            );
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function {
            let automatic;
            let activation = if self.inputs.activation.is_none()
                && (function == Native::Activate || function == Native::TorchActivate)
                && let Some(use_held) = self.inputs.activation_use_held
            {
                anyhow::ensure!(
                    self.inputs.shell_execution == Some(false),
                    "ordinary activation requires an established non-shell context"
                );
                let hud = self
                    .inputs
                    .hud
                    .context("activation HUD owner unavailable")?;
                automatic = activation::ordinary_inputs(
                    &self.registry.actors,
                    self.generation,
                    &self.registry.globals,
                    hud,
                    use_held,
                    Some((self.index, arguments)),
                )?;
                Some(&automatic)
            } else {
                self.inputs.activation
            };
            let selected_player = activation.map(|inputs| {
                (
                    &self.registry.actors[inputs.player.index],
                    self.registry.globals.lockout().unwrap_or(inputs.lockout),
                )
            });
            if let Some(value) = self.registry.activation.call(
                id,
                function,
                arguments,
                activation,
                &self.registry.actors[self.index].attributes,
                selected_player,
            )? {
                if value != 0 {
                    self.registry.globals.activation_consumed_use = true;
                }
                return Ok(Some(CallResult::Complete(value)));
            }
        }
        Ok(None)
    }
}
