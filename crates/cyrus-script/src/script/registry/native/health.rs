//! Health native-call dispatch.
use super::super::{Argument, Attribute, CallResult, CombatFlags, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_change_health(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if [
            Native::ZapObject,
            Native::ZapObjectNoKill,
            Native::ZapPlayer,
            Native::ZapPlayerNoKill,
        ]
        .contains(&function)
        {
            return (self.zap(function, arguments)).map(Some);
        }

        if function == Native::SetHealth || function == Native::AdjustHealth {
            let set = function == Native::SetHealth;
            anyhow::ensure!(
                arguments.len() == if set { 1 } else { 2 },
                "{function}: wrong argument count"
            );
            let selected = self
                .selected_player()
                .is_some_and(|player| player.index == self.index);
            if selected {
                if !set {
                    let id = self.registry.id(self.index)?;
                    let sounds = &mut self.sounds;
                    self.registry.apply_selected_health_command(
                        id,
                        arguments[0].integer()?,
                        arguments[1].integer()?,
                        self.inputs,
                        self.random.as_deref_mut(),
                        &mut |actor, sound| {
                            sounds
                                .as_deref_mut()
                                .context("selected health sound executor unavailable")?(
                                actor, sound,
                            )
                        },
                    )?;
                    return Ok(Some(CallResult::Complete(0)));
                }
                self.inputs
                    .hud
                    .with_context(|| format!("{function}: selected-player HUD owner unavailable"))?
                    .request_replenish_health_feedback();
            }
            let attributes = &mut self.registry.actors[self.index].attributes;
            // Generic admission follows the native selected-player feedback update.
            if attributes[Attribute::Invulnerable] != 0 {
                return Ok(Some(CallResult::Complete(0)));
            }
            if attributes[Attribute::Npc] != 0
                && !self.receiver_weapon_predicates(Native::IsHoldingWeapon)?.0
                && self.registry.actors[self.index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .is_none_or(|combat| !combat.flags.contains(CombatFlags::STANCE))
            {
                return Ok(Some(CallResult::Complete(0)));
            }
            if selected {
                let id = self.registry.id(self.index)?;
                let change = arguments[0].integer()?.wrapping_sub(i32::from(
                    self.registry.actors[self.index].attributes[Attribute::Health],
                ));
                let sounds = &mut self.sounds;
                self.registry.apply_selected_death_health(
                    id,
                    change,
                    None,
                    self.inputs,
                    self.random.as_deref_mut(),
                    &mut |actor, sound| {
                        sounds
                            .as_deref_mut()
                            .context("selected death sound executor unavailable")?(
                            actor, sound
                        )
                    },
                )?;
                return Ok(Some(CallResult::Complete(0)));
            }
            let value = arguments[0].integer()?;
            let health = i32::from(self.registry.actors[self.index].attributes[Attribute::Health]);
            let delta = if set {
                value.wrapping_sub(health)
            } else {
                value
            };
            let kills = set || arguments[1].integer()? == 1;
            self.adjust_nonplayer_health(function, delta, kills)?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_health_query(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // `attr_health` is `attr_health`; Strength and Armor read the combat record.
        if [
            Native::Health,
            Native::PlayerHealth,
            Native::Strength,
            Native::Armor,
            Native::ReplenishHealth,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let index = if function == Native::PlayerHealth {
                self.selected_player()
                    .context("PlayerHealth: selected player unavailable")?
                    .index
            } else {
                self.index
            };
            let selected = self
                .selected_player()
                .is_some_and(|player| player.index == index);
            let actor = &mut self.registry.actors[index];
            let value = match function {
                Native::Health | Native::PlayerHealth => {
                    i32::from(actor.attributes[Attribute::Health])
                }
                Native::ReplenishHealth => {
                    if actor.attributes[Attribute::Health] >= 100 {
                        0
                    } else {
                        if selected {
                            self.inputs
                                .hud
                                .context("ReplenishHealth selected-player HUD owner unavailable")?
                                .request_replenish_health_feedback();
                        }
                        actor.attributes[Attribute::Health] = 100;
                        1
                    }
                }
                _ => {
                    let combat = actor
                        .control
                        .as_ref()
                        .and_then(|control| control.combat.as_ref())
                        .with_context(|| {
                            format!("{function}: receiver combat record unavailable")
                        })?;
                    if function == Native::Strength {
                        combat.strength
                    } else {
                        combat.armor
                    }
                }
            };
            return Ok(Some(CallResult::Complete(value)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_health_parameters(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::SetPotion || function == Native::GetPotion {
            let set = function == Native::SetPotion;
            anyhow::ensure!(
                arguments.len() == 1 + usize::from(set),
                "{function}: wrong argument count"
            );
            let slot = usize::try_from(arguments[0].integer()?)
                .ok()
                .filter(|slot| *slot < self.registry.globals.potions.len())
                .with_context(|| format!("{function}: potion index outside 0..4 unsupported"))?;
            if set {
                self.registry.globals.potions[slot] = arguments[1].integer()?;
                return Ok(Some(CallResult::Complete(1)));
            }
            return Ok(Some(CallResult::Complete(
                self.registry.globals.potions[slot],
            )));
        }

        if function == Native::AdjustStrength || function == Native::AdjustArmor {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let delta = arguments[0].integer()?;
            let combat = self.registry.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            let value = if function == Native::AdjustStrength {
                &mut combat.strength
            } else {
                &mut combat.armor
            };
            *value = value.wrapping_add(delta);
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::SetStrength || function == Native::SetArmor {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()?;
            let combat = self.registry.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            if function == Native::SetStrength {
                combat.strength = value;
            } else {
                combat.armor = value;
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
