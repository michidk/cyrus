//! The quick sword and quick health keys: the world loop's key handler for the selected player.
use super::{GameHost, ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::QuickKeys;
use anyhow::{Context, Result};

/// Items the sword key looks for, in order; the third is used without being selected.
const SWORDS: [u16; 3] = [1, 13, 75];
/// Health potions the health key drinks, in order, with the health below which they heal a fixed
/// amount and that amount; at or above it the health is set to its maximum.
const POTIONS: [(u16, u8, i32); 3] = [(0x42, 0x56, 15), (4, 0x56, 15), (0x41, 0x60, 5)];
/// The empty flask a drunk-out potion leaves behind.
const FLASK: u16 = 0x40;
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
        let Some(keys) = inputs.quick_keys.filter(|keys| keys.sword || keys.health) else {
            return Ok(());
        };
        let Some(index) = self.selected_player_index() else {
            return Ok(());
        };
        let native_source_program = match self.globals.native_source {
            crate::script::activation::NativeSource::Actor { generation, index }
                if generation == self.generation =>
            {
                self.actors
                    .get(index)
                    .and_then(|actor| actor.runtime.as_ref())
                    .map(cyrus_script::Runtime::shared_program)
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
        host.quick_keys(keys)
    }
}

impl GameHost<'_, '_, '_, '_> {
    fn quick_keys(&mut self, keys: QuickKeys) -> Result<()> {
        let lockout = self
            .registry
            .globals
            .lockout()
            .context("quick keys: player lockout state unavailable")?;
        if lockout == 1 || self.registry.actors[self.index].attributes[105] == 1 {
            return Ok(());
        }
        let hud = self
            .inputs
            .hud
            .context("quick keys: inventory host unavailable")?;
        let id = self.registry.id(self.index)?;
        if keys.sword {
            let (holding, drawing, sheathing) =
                self.actor_weapon_predicates(self.index, "QuickSword")?;
            if holding && !drawing && !sheathing {
                self.sheath_sword("SheathSword", id)?;
            } else if !drawing && !sheathing {
                for sword in SWORDS {
                    if hud.item_count(sword)? != 0 {
                        // The last sword is used without being selected.
                        if sword != SWORDS[2] {
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
        Ok(())
    }

    /// Drink the first owned potion while health is below its maximum.
    fn quick_health(&mut self) -> Result<()> {
        let hud = self
            .inputs
            .hud
            .context("quick keys: inventory host unavailable")?;
        let health = i32::from(self.registry.actors[self.index].attributes[3]);
        if health >= MAXIMUM_HEALTH {
            return Ok(());
        }
        let Some(&(potion, threshold, heal)) = POTIONS
            .iter()
            .find(|(potion, ..)| hud.item_count(*potion).is_ok_and(|count| count != 0))
        else {
            return Ok(());
        };
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
        // A drunk-out potion of the first and last kinds leaves an empty flask, selected.
        if potion != 4 && hud.item_count(potion)? == 0 {
            self.add_session_item(FLASK, 1)?;
            hud.select_item(FLASK)?;
        }
        Ok(())
    }
}
