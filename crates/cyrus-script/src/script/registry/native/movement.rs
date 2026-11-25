//! Movement native-call dispatch.
use super::super::{
    Argument, CallKind, CallResult, GameHost, LocomotionGeometry, Native, Result, ScriptActorId,
    ground, ledge_states, transition,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_jump(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::Jump {
            anyhow::ensure!(arguments.is_empty(), "Jump requires no arguments");
            if !self
                .registry
                .script_jump_admitted(id, self.selected_player())?
            {
                return Ok(Some(CallResult::Complete(0)));
            }
            let environment = transition::ObstacleEnvironment {
                selected_player: self.selected_player(),
                auto_grab: false,
                item_system_ready: self
                    .inputs
                    .hud
                    .map(crate::script::ScriptHud::inventory_ready)
                    .transpose()?
                    .unwrap_or(false),
                shell_execution: self.inputs.shell_execution.unwrap_or(false),
                ticks: self
                    .inputs
                    .nominal_ticks
                    .context("Jump needs nominal ticks")?,
            };
            let prepared;
            let geometry = if let Some(geometry) = self.inputs.locomotion_geometry {
                Some(geometry)
            } else if environment.selected_player == Some(id)
                && let Some(source) = self.inputs.player_collision
            {
                prepared = self.registry.prepare_ordinary_collision_query(
                    id,
                    source.scene,
                    source.animations,
                    source.view,
                    source.actors,
                )?;
                Some(LocomotionGeometry {
                    ground: ground::OrdinaryGroundQuery {
                        scene: source.scene,
                        animations: source.animations,
                        prepared: &prepared,
                        distance: source.scene.settings.post_collide_height,
                        sidedness: false,
                    },
                    environment,
                })
            } else {
                None
            };
            let timing = ledge_states::LedgeTiming {
                fine_ticks: if environment.selected_player == Some(id) {
                    self.inputs
                        .fine_ticks
                        .context("Jump needs fine ticks for selected-player edges")?
                } else {
                    0
                },
                frame_scale: self.inputs.frame_scale,
            };
            let mut missing_sound = |_, _| anyhow::bail!("Jump sound handler unavailable");
            self.registry.begin_script_jump(
                id,
                geometry,
                environment,
                timing,
                self.sounds.as_deref_mut().unwrap_or(&mut missing_sound),
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
