//! Perception native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result, ScriptActorId, obstacle};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_player_sight(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::PlayerInSight {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerInSight requires a selected player")?;
            let id = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let visible = if let Some(geometry) = self.inputs.locomotion_geometry {
                self.registry.player_in_sight(
                    id,
                    player,
                    obstacle::OrdinaryObstacleQuery {
                        scene: geometry.ground.scene,
                        animations: geometry.ground.animations,
                        prepared: geometry.ground.prepared,
                        vertical_offset: 0,
                        respond: false,
                    },
                )?
            } else {
                // A receiver without collision gathers no candidates, so the test reads the list
                // the latest gather left.
                let source = self
                    .inputs
                    .player_collision
                    .context("PlayerInSight requires the world's collision geometry")?;
                self.registry.player_in_sight_retained(
                    id,
                    player,
                    source.scene,
                    source.animations,
                )?
            };
            return Ok(Some(CallResult::Complete(i32::from(visible))));
        }
        Ok(None)
    }
}
