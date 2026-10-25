//! `UseItem`: the selected player uses an owned item, which starts the item's use script as a shell.
use super::GameHost;
use anyhow::{Context, Result};

/// Item-type codes of ITEM.INI that `UseItem` treats specially.
const WEAPON_ITEM: u32 = 1;
const TORCH_ITEM: u32 = 3;
/// Item 83 requests the island map; presentation is host-owned and not yet wired.
const ISLAND_MAP_ITEM: u16 = 83;

impl GameHost<'_, '_, '_, '_> {
    /// Use `item` for the receiver. Every refusal that is not an error (player lockout, an edge
    /// state, a second attempt in one world-loop iteration, shell context, deep water, a held
    /// weapon, a torch being lit or doused, a closed inventory) leaves the world unchanged. The
    /// compass toggles; a torch item lights the torch and is held, and any use while a torch burns
    /// puts it out; a general item starts its use script unless the item's instance limit is
    /// reached, then consumes its drop list and flags. A weapon item becomes the weapon attribute
    /// and is drawn, and is consumed only when the draw began. The island-map item is explicitly
    /// unsupported until a presentation host is available.
    pub(super) fn use_item(&mut self, item: u16) -> Result<()> {
        let hud = self
            .inputs
            .hud
            .context("UseItem: inventory host unavailable")?;
        let lockout = self
            .registry
            .globals
            .lockout()
            .context("UseItem: player lockout state unavailable")?;
        let selected = self
            .registry
            .selected_player_index()
            .context("UseItem: selected player unavailable")?;
        let player = &self.registry.actors[selected];
        let control = player
            .control
            .as_ref()
            .context("UseItem: selected player control unavailable")?;
        if lockout == 1 || (10..=19).contains(&control.living_state) {
            return Ok(());
        }
        if std::mem::replace(&mut self.registry.globals.item_use_latch, true) {
            return Ok(());
        }
        let shell = self
            .inputs
            .shell_execution
            .context("UseItem: shell context unavailable")?;
        if !hud.inventory_ready()? || shell {
            return Ok(());
        }
        anyhow::ensure!(
            self.index == selected,
            "UseItem: the receiver is not the selected player"
        );
        let (holding, _, _) = self.actor_weapon_predicates(selected, "IsHoldingWeapon")?;
        let group = player.bound_animation_group().flatten();
        let torch_change =
            matches!(control.living_state, 0x21 | 0x22) || matches!(group, Some(0x95 | 0x96));
        let deep_water = matches!(player.surface, Some(2 | 4));
        if deep_water || holding || torch_change {
            return Ok(());
        }
        let burning = control.torch_burning != 0;
        let player_id = self.registry.id(selected)?;
        if burning {
            // While a torch burns, any item use puts it out instead.
            return self.douse_torch(player_id, selected);
        }
        let used = hud.begin_item_use(item)?;
        if item == 0 {
            hud.toggle_compass();
            return Ok(());
        }
        if used.item_type == WEAPON_ITEM {
            self.registry.actors[selected].attributes[23] = item as u8;
            self.draw_sword(player_id)?;
            // Only a draw that began is consumed.
            let control = self.registry.actors[selected]
                .control
                .as_ref()
                .context("UseItem: selected player control unavailable")?;
            if control.living_state != 20 {
                return Ok(());
            }
        }
        if used.item_type == TORCH_ITEM {
            let torch = self.torch_inputs(selected, "UseItem")?;
            let ticks = self
                .inputs
                .nominal_ticks
                .context("UseItem nominal ticks unavailable")?;
            let sounds = self
                .sounds
                .as_deref_mut()
                .context("UseItem sound handler unavailable")?;
            self.registry.light_torch(player_id, torch, ticks, sounds)?;
            let control = self.registry.actors[selected]
                .control
                .as_mut()
                .context("UseItem: selected player control unavailable")?;
            // Only a torch that began lighting is held and consumed.
            if control.living_state != 33 {
                return Ok(());
            }
            control.held_item = Some(item as u8);
        }
        anyhow::ensure!(
            item != ISLAND_MAP_ITEM,
            "UseItem({item}): island-map presentation host unavailable"
        );
        if let Some((name, limit)) = used.script
            && (limit < 1 || self.registry.running_shells(&name) < limit as usize)
        {
            self.registry.spawn_shell(&name)?;
        }
        hud.finish_item_use(item)
    }

    fn douse_torch(&mut self, player: super::ScriptActorId, index: usize) -> Result<()> {
        let torch = self.torch_inputs(index, "UseItem")?;
        let ticks = self
            .inputs
            .nominal_ticks
            .context("UseItem nominal ticks unavailable")?;
        let sounds = self
            .sounds
            .as_deref_mut()
            .context("UseItem sound handler unavailable")?;
        self.registry.unlight_torch(player, torch, ticks, sounds)
    }
}
