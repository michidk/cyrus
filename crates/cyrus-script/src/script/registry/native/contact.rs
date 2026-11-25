//! Contact native-call dispatch.
use super::super::{Argument, Attribute, CallResult, GameHost, Native, Result, ScriptActorId};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_player_contact(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // PlayerCollide holds while the selected player stands on the receiver or its body touched
        // the receiver; PlayerCollideStatic only reads the contact list.
        if function == Native::PlayerCollide || function == Native::PlayerCollideStatic {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .with_context(|| format!("{function} selected player unavailable"))?;
            let receiver = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let contacts = &self.registry.globals.player_contacts;
            let touching = if function == Native::PlayerCollide {
                self.registry.actors[self.registry.resolve(player)?]
                    .model
                    .support
                    .actor
                    == Some(receiver)
                    || contacts.touches_actor(self.index)
            } else {
                contacts.touches_static(self.index)
            };
            return Ok(Some(CallResult::Complete(i32::from(touching))));
        }

        // CollidePlayerWeapon(x, y, z, radius): while the selected player swings a drawn weapon,
        // whether the weapon tip is within the radius of the point offset from the receiver.
        if function == Native::CollidePlayerWeapon {
            anyhow::ensure!(arguments.len() == 4, "{function} requires four arguments");
            let player = self
                .selected_player()
                .context("CollidePlayerWeapon selected player unavailable")?;
            let player_index = self.registry.resolve(player)?;
            let (holding, _, _) =
                self.actor_weapon_predicates(player_index, Native::IsHoldingWeapon)?;
            let actor = &self.registry.actors[player_index];
            let swinging = actor
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| {
                    combat.decision.attack_state == crate::script::ExecutorState::DEFENDING
                });
            if !holding || actor.attributes[Attribute::PlayerType] == 1 || !swinging {
                return Ok(Some(CallResult::Complete(0)));
            }
            let tip = actor
                .model
                .weapon
                .as_ref()
                .context(
                    "CollidePlayerWeapon: the selected player's weapon endpoints are unavailable",
                )?
                .endpoints[1];
            let receiver = self.registry.id(self.index)?;
            let origin = self.registry.query_position(receiver)?;
            let offsets = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            let center = std::array::from_fn(|axis| origin[axis].wrapping_add(offsets[axis]) >> 8);
            return Ok(Some(CallResult::Complete(i32::from(
                cyrus_collision::distance::point_in_sphere(
                    tip,
                    center,
                    arguments[3].integer()? >> 8,
                ),
            ))));
        }

        if function == Native::PlayerStand {
            anyhow::ensure!(arguments.is_empty(), "PlayerStand requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerStand selected player unavailable")?;
            anyhow::ensure!(
                player.generation == self.generation && player.index < self.registry.actors.len(),
                "PlayerStand selected player identity is stale"
            );
            let receiver = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            return Ok(Some(CallResult::Complete(i32::from(
                self.registry.actors[player.index].model.support.actor == Some(receiver),
            ))));
        }
        Ok(None)
    }
}
