//! Player native-call dispatch.
use super::super::{
    Argument, CallKind, CallResult, GameHost, LivingState, Native, Result, ScriptActorId,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_player_motion(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        // PlayerMoved compares the selected player's position with its snapshot taken at the
        // ordinary actor-update boundary.
        if function == Native::PlayerMoved {
            anyhow::ensure!(arguments.is_empty(), "PlayerMoved requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerMoved: selected player unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(player.index)?;
            return Ok(Some(CallResult::Complete(i32::from(
                spatial.local.position != spatial.previous_local_position,
            ))));
        }

        if function == Native::IsBouncing {
            anyhow::ensure!(arguments.is_empty(), "IsBouncing requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_ref()
                .context("IsBouncing receiver control unavailable")?;
            return Ok(Some(CallResult::Complete(i32::from(
                control.living_state.is_bounce(),
            ))));
        }

        if function == Native::GetMovementSpeed {
            anyhow::ensure!(
                arguments.is_empty(),
                "GetMovementSpeed requires no arguments"
            );
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.ordinary_group_speed(self.index),
            ))));
        }

        if function == Native::MovePos {
            anyhow::ensure!(arguments.len() == 3, "MovePos requires three arguments");
            let destination = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            return Ok(Some(CallResult::Complete(
                self.registry
                    .move_position_pending(id, destination, self.inputs)?,
            )));
        }

        // Selected-player angle queries require a current model; movement speed only
        // requires animation state (preservation SOUP.md, player motion queries).
        if [
            Native::PlayerTurned,
            Native::PlayerTurnedDirection,
            Native::PlayerMovementSpeed,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .with_context(|| format!("{function}: selected player unavailable"))?;
            let index = self.registry.resolve(player)?;
            if function == Native::PlayerMovementSpeed {
                let actor = &self.registry.actors[index];
                let value = actor
                    .animation
                    .as_ref()
                    .and_then(|animation| animation.playback.current_group())
                    .and_then(crate::script::player_status::movement_speed_attribute)
                    .map_or(0, |attribute| i32::from(actor.attributes[attribute]));
                return Ok(Some(CallResult::Complete(value)));
            }
            if self.registry.actors[index].model.current.is_none() {
                return Ok(Some(CallResult::Complete(0)));
            }
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(index)?;
            let value = if function == Native::PlayerTurned {
                crate::script::player_status::player_turned(
                    spatial.local.angles,
                    spatial.angle_history,
                )
            } else {
                crate::script::player_status::player_turned_direction(
                    spatial.local.angles[1],
                    spatial.angle_history[1],
                )
            };
            return Ok(Some(CallResult::Complete(value)));
        }

        // PlayerOnScape: the selected player's last ground response was a terrain contact.
        if function == Native::PlayerOnScape {
            anyhow::ensure!(arguments.is_empty(), "PlayerOnScape requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerOnScape: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("PlayerOnScape: selected-player control unavailable")?;
            return Ok(Some(CallResult::Complete(i32::from(
                control.ground_kind == crate::script::registry::ground::GroundKind::TERRAIN,
            ))));
        }

        // PlayerHanging reports whether the selected player is hanging from this receiver's
        // model. The ledge transition retains the supporting actor separately from ordinary
        // ground ownership; static ledges therefore return zero. This is the identity contract
        // published by preservation's PlayerHanging native.
        if function == Native::PlayerHanging {
            anyhow::ensure!(arguments.is_empty(), "PlayerHanging requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerHanging: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("PlayerHanging: selected-player control unavailable")?;
            let receiver = self.registry.id(self.index)?;
            let hanging = control.living_state == LivingState::HANGING
                && self.registry.actors[index].model.ledge_support == Some(receiver);
            return Ok(Some(CallResult::Complete(i32::from(hanging))));
        }

        // PlayerStatus classifies the selected player; the motion bytes are the inferred
        // actor bytes +0x1b1 through +0x1b3 and the living state +0x1b4.
        if function == Native::PlayerStatus {
            anyhow::ensure!(arguments.is_empty(), "PlayerStatus requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerStatus: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let actor = &self.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("PlayerStatus: selected-player control unavailable")?;
            let animation = actor
                .animation
                .as_ref()
                .context("PlayerStatus: selected-player animation unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(index)?;
            return Ok(Some(CallResult::Complete(
                crate::script::player_status::player_status(
                    crate::script::player_status::PlayerStatusInput {
                        queued_motion: control.queued_motion,
                        falling_state: control.falling_state,
                        sliding_state: control.sliding_state,
                        living_state: control.living_state,
                        group: animation.playback.current_group(),
                        moved: spatial.local.position != spatial.previous_local_position,
                    },
                ) as i32,
            )));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_player_placement(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::PlayerForceTalk {
            let [distance] = arguments else {
                anyhow::bail!("PlayerForceTalk requires one argument")
            };
            self.registry
                .force_talk_selected_player(self.index, distance.integer()?)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::PlayerOffset {
            let [x, y, z] = arguments else {
                anyhow::bail!("PlayerOffset requires three arguments")
            };
            self.registry.offset_selected_player(
                self.index,
                [x.integer()?, y.integer()?, z.integer()?],
                false,
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
