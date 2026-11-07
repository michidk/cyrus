//! The quick sword and quick health keys: the world loop's key handler for the selected player.
use super::{GameHost, ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::QuickKeys;
use crate::script::inventory::{FLAMING_SABRE, HEALTH_POTION, SABRE, SOUL_SWORD};
use anyhow::{Context, Result};
use cyrus_soup::{Attribute, Native};

/// Items the sword key looks for, in order; the third is used without being selected.
const SWORDS: [u16; 3] = [SABRE, SOUL_SWORD, FLAMING_SABRE];
/// The installed health potion and the health below which it heals a fixed amount.
const POTION: (u16, u8, i32) = (HEALTH_POTION, 0x56, 15);
const MAXIMUM_HEALTH: i32 = 100;

impl ScriptWorld {
    /// Run the key handler once for the selected player. A lockout, the sword-hold attribute, or
    /// the absence of a selected player leaves the world unchanged.
    pub fn run_quick_keys(
        &mut self,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let held = inputs
            .activation_use_held
            .or_else(|| inputs.activation.map(|activation| activation.use_held))
            .or_else(|| inputs.player_controller.map(|input| input.held.action));
        if self
            .globals
            .activation_consumes_action(held.unwrap_or(true))
        {
            return Ok(());
        }
        let Some(keys) = inputs
            .quick_keys
            .filter(|keys| keys.sword || keys.health || keys.use_selected)
        else {
            return Ok(());
        };
        self.with_selected_host(inputs, random, sounds, |host| host.quick_keys(keys))
            .map(|_| ())
    }

    /// `SheathSword` for the selected player, as the armed look-and-action hold requests it.
    pub(super) fn sheath_selected_player(
        &mut self,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        self.with_selected_host(inputs, random, sounds, |host| {
            let id = host.registry.id(host.index)?;
            host.sheath_sword(Native::SheathSword, id)
        })
        .map(|_| ())
    }

    /// Run `run` on a host bound to the selected player; nothing runs without one.
    fn with_selected_host<R>(
        &mut self,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
        run: impl FnOnce(&mut GameHost<'_, '_, '_, '_>) -> Result<R>,
    ) -> Result<Option<R>> {
        let Some(index) = self.selected_player_index() else {
            return Ok(None);
        };
        let native_source_program = match self.globals.native_source {
            crate::script::activation::NativeSource::Actor { generation, index }
                if generation == self.generation =>
            {
                self.actors
                    .get(index)
                    .and_then(|actor| actor.runtime.as_ref())
                    .map(cyrus_soup::Runtime::shared_program)
            }
            _ => None,
        };
        let mut host = GameHost {
            native_source_program,
            world: self.world,
            animation_system: self.updates.animation_system,
            task_system: self.updates.task_system,
            index,
            generation: self.generation,
            ordinary_actors: None,
            walk_map_count: self.walk_nodes.as_ref().map(|nodes| nodes.map_count()),
            registry: self,
            sounds: Some(sounds),
            random: Some(random),
            inputs,
        };
        run(&mut host).map(Some)
    }
}

impl GameHost<'_, '_, '_, '_> {
    fn quick_keys(&mut self, keys: QuickKeys) -> Result<()> {
        let lockout = self
            .registry
            .globals
            .lockout()
            .context("quick keys: player lockout state unavailable")?;
        if lockout == 1 || self.registry.actors[self.index].attributes[Attribute::PlayerType] == 1 {
            return Ok(());
        }
        let hud = self
            .inputs
            .hud
            .context("quick keys: inventory host unavailable")?;
        let id = self.registry.id(self.index)?;
        if keys.sword {
            let (holding, drawing, sheathing) = self
                .weapon_predicates(self.index)
                .context("quick sword key")?;
            if holding && !drawing && !sheathing {
                self.sheath_sword(Native::SheathSword, id)?;
            } else if !drawing && !sheathing {
                for sword in SWORDS {
                    if hud.item_count(sword)? != 0 {
                        // The last sword is used without being selected.
                        if sword != FLAMING_SABRE {
                            hud.select_item(sword)?;
                        }
                        self.use_item(sword)?;
                        break;
                    }
                }
            }
        }
        if keys.health {
            self.quick_health()?;
        }
        // The original, observed with a potion selected: an activation prompt owner takes the key
        // and the item is not used, and a player hanging on a rope does not use the item either.
        let on_rope = self.registry.actors[self.index]
            .control
            .as_ref()
            .is_some_and(|control| control.rope_attached);
        if keys.use_selected
            && !on_rope
            && self.registry.activation.owner().is_none()
            && let Some(selected) = hud.selected_item()?
        {
            self.use_item(selected)?;
        }
        Ok(())
    }

    /// Drink the first owned potion while health is below its maximum.
    fn quick_health(&mut self) -> Result<()> {
        let hud = self
            .inputs
            .hud
            .context("quick keys: inventory host unavailable")?;
        let health = i32::from(self.registry.actors[self.index].attributes[Attribute::Health]);
        if health >= MAXIMUM_HEALTH {
            return Ok(());
        }
        let (potion, threshold, heal) = POTION;
        if hud.item_count(potion)? == 0 {
            return Ok(());
        }
        let id = self.registry.id(self.index)?;
        let sounds = self
            .sounds
            .as_deref_mut()
            .context("quick keys: sound handler unavailable")?;
        if health < i32::from(threshold) {
            self.registry.apply_selected_additive_health(
                id,
                heal,
                self.inputs,
                self.random.as_deref_mut(),
                sounds,
            )?;
        } else {
            self.registry.apply_selected_death_health(
                id,
                MAXIMUM_HEALTH - health,
                None,
                self.inputs,
                self.random.as_deref_mut(),
                sounds,
            )?;
        }
        hud.drop_one(potion)?;
        Ok(())
    }
}
