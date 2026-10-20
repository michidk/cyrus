//! `UseItem`: the selected player uses an owned item, which starts the item's use script as a shell.
use super::GameHost;
use anyhow::{Context, Result};

/// Item-type codes of ITEM.INI that `UseItem` treats specially.
const WEAPON_ITEM: u32 = 1;
const TORCH_ITEM: u32 = 3;
/// The one item the original flags separately when used; its effect is not recovered.
const FLAGGED_ITEM: u16 = 83;

impl GameHost<'_, '_, '_, '_> {
    /// Use `item` for the receiver. Every refusal that is not an error (player lockout, an edge
    /// state, a second attempt in one world-loop iteration, shell context, deep water, a held
    /// weapon, a torch being lit or doused, a closed inventory) leaves the world unchanged. The
    /// compass toggles; a general item starts its use script unless the item's instance limit is
    /// reached, then consumes its drop list and flags. Weapon and torch items, a burning torch and
    /// the flagged item are explicitly unsupported.
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
        anyhow::ensure!(
            control.torch_burning == 0,
            "UseItem: putting out a burning torch unsupported"
        );
        let used = hud.begin_item_use(item)?;
        if item == 0 {
            hud.toggle_compass();
            return Ok(());
        }
        anyhow::ensure!(
            used.item_type != WEAPON_ITEM && used.item_type != TORCH_ITEM,
            "UseItem({item}): weapon and torch items unsupported"
        );
        anyhow::ensure!(
            item != FLAGGED_ITEM,
            "UseItem({item}): the item's separate use flag is unrecovered"
        );
        if let Some((name, limit)) = used.script
            && (limit < 1 || self.registry.running_shells(&name) < limit as usize)
        {
            self.registry.spawn_shell(&name)?;
        }
        hud.finish_item_use(item)
    }
}
