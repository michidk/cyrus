//! Lossless inverse mapping of retained inventory state to original SVIT records.
use super::*;
use rgpre::import::savegame::{SaveInventory, SaveItemState, encode_save_inventory};

impl InventorySnapshot {
    /// Encode the saved initialized item range, excluding the unsaved terminal item.
    /// Requires retained original representations and complete current push/use history.
    /// This produces one record payload, not a complete SAVEGAME slot.
    pub fn export_original(&self, hud: &HudAsset) -> Result<Vec<u8>> {
        let inventory = Inventory::from_snapshot(hud, self.clone())
            .context("SAVEGAME.SAV SVIT: native inventory snapshot validation")?;
        let encoding = inventory
            .original_encoding
            .as_ref()
            .context("SAVEGAME.SAV SVIT: original encoding metadata absent")?;
        let count = hud
            .inventory
            .len()
            .checked_sub(1)
            .context("SAVEGAME.SAV SVIT: initialized item list is empty")?;
        anyhow::ensure!(
            encoding
                .flags
                .keys()
                .copied()
                .eq(hud.inventory.keys().copied().take(count)),
            "SAVEGAME.SAV SVIT: original item owner/order census differs from ITEM.INI"
        );
        let items = inventory.items.iter().take(count).map(|(&id, item)| {
            let original = &encoding.flags[&id];
            let history = inventory.saved_counts.get(&id)
                .with_context(|| format!("SAVEGAME.SAV SVIT item {id}: saved push history absent"))?;
            let flag = |raw: u8, current: bool, field: &str| -> Result<u8> {
                if (raw != 0) == current { return Ok(raw); }
                anyhow::ensure!(raw <= 1,
                    "SAVEGAME.SAV SVIT item {id}: changed noncanonical {field} representation unsupported");
                Ok(u8::from(current))
            };
            let signed = |value: u32, field: &str| -> Result<i32> {
                value.try_into().with_context(|| format!(
                    "SAVEGAME.SAV SVIT item {id}: {field} exceeds supported signed range"))
            };
            Ok(SaveItemState {
                hide: flag(original.hide, item.hidden, "hide")?,
                pushed: flag(original.pushed, history.pushed, "pushed")?,
                total: signed(item.total, "total")?,
                player_max: signed(item.player_max, "player_max")?,
                player_total: signed(item.count, "player_total")?,
                item_used: inventory.used[&id].with_context(|| format!(
                    "SAVEGAME.SAV SVIT item {id}: use counter absent"))?,
                saved_player_total: history.value,
            }.into())
        }).collect::<Result<Vec<_>>>()?;
        let selected_item = match inventory.selected {
            Some(id) => i32::from(id),
            None => {
                anyhow::ensure!(
                    encoding.selected == -1,
                    "SAVEGAME.SAV SVIT: absent selection has unsupported retained identity {}",
                    encoding.selected
                );
                -1
            }
        };
        encode_save_inventory(&SaveInventory {
            items,
            selected_item,
        })
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgpre::import::savegame::{load_save_slot, parse_save_inventory};

    #[test]
    fn installed_changed_inventory_roundtrip_and_missing_state_rejection() -> Result<()> {
        let Some(root) = std::env::var_os("REDGUARD_DIR") else {
            eprintln!("SKIP inventory export: REDGUARD_DIR absent");
            return Ok(());
        };
        let Some(slot) = std::env::var_os("CYRUS_ORIGINAL_SAVE") else {
            eprintln!("SKIP inventory export: CYRUS_ORIGINAL_SAVE absent");
            return Ok(());
        };
        let install =
            cyrus_assets::install::GameInstall::locate(Some(std::path::Path::new(&root)))?;
        let files = cyrus_assets::native::Files::open(&install)?;
        let prepared = cyrus_assets::game::prepare(&files, "hud/overlay.hud")?;
        let hud: HudAsset = serde_json::from_slice(&prepared["hud/overlay.hud"])?;
        let slot = load_save_slot(&slot)?;
        let original = &slot
            .game
            .sections
            .iter()
            .find(|s| s.name == *b"SVIT")
            .context("installed SAVEGAME.SAV SVIT absent")?
            .data;
        let saved = parse_save_inventory(original, hud.inventory.len() - 1)?;
        let mut native = Inventory::from_original(&hud, &saved)?;
        assert_eq!(native.snapshot().export_original(&hud)?, *original);
        let id = *hud
            .inventory
            .keys()
            .next()
            .context("empty installed inventory")?;
        let item = native.items.get_mut(&id).unwrap();
        item.count = 23;
        item.total = 31;
        item.player_max = 29;
        native.used.insert(id, Some(u32::MAX));
        native.pop_count(u32::from(id))?;
        native.items.get_mut(&id).unwrap().count = 23;
        native.push_count(u32::from(id), 17)?;
        let terminal = *hud.inventory.keys().last().unwrap();
        native.items.get_mut(&terminal).unwrap().count = 77;
        assert!(
            native.original_encoding.as_ref().unwrap().flags[&id].pushed <= 1,
            "installed first item has noncanonical pushed state"
        );
        native.select(id)?;
        let snapshot: InventorySnapshot =
            serde_json::from_slice(&serde_json::to_vec(&native.snapshot())?)?;
        assert_eq!(snapshot.items[&terminal].count, 77);
        let bytes = snapshot.export_original(&hud)?;
        assert_ne!(bytes, *original);
        let decoded = parse_save_inventory(&bytes, saved.items.len())?;
        let state = decoded.items[0].state();
        assert_eq!(state.hide, saved.items[0].state().hide);
        assert_eq!(
            (state.player_total, state.total, state.player_max),
            (17, 31, 29)
        );
        assert_eq!(
            (state.item_used, state.saved_player_total, state.pushed),
            (u32::MAX, 23, 1)
        );
        assert_eq!(decoded.items[1..], saved.items[1..]);
        assert_eq!(decoded.selected_item, i32::from(id));
        let mut missing = snapshot.clone();
        missing.original_encoding = None;
        Inventory::from_snapshot(&hud, missing.clone())?;
        assert!(
            missing
                .export_original(&hud)
                .unwrap_err()
                .to_string()
                .contains("metadata absent")
        );
        let mut missing = snapshot.clone();
        missing.items.get_mut(&id).unwrap().used = None;
        assert!(
            missing
                .export_original(&hud)
                .unwrap_err()
                .to_string()
                .contains("use counter absent")
        );
        let mut missing = snapshot.clone();
        missing.saved_counts.remove(&id);
        assert!(
            missing
                .export_original(&hud)
                .unwrap_err()
                .to_string()
                .contains("push history absent")
        );
        let mut mismatch = snapshot.clone();
        mismatch
            .original_encoding
            .as_mut()
            .unwrap()
            .flags
            .remove(&id);
        assert!(
            mismatch
                .export_original(&hud)
                .unwrap_err()
                .to_string()
                .contains("census differs")
        );
        let mut noncanonical = snapshot.clone();
        noncanonical
            .original_encoding
            .as_mut()
            .unwrap()
            .flags
            .get_mut(&id)
            .unwrap()
            .pushed = 0xfe;
        noncanonical.saved_counts.get_mut(&id).unwrap().pushed = false;
        assert!(
            noncanonical
                .export_original(&hud)
                .unwrap_err()
                .to_string()
                .contains("noncanonical pushed")
        );
        noncanonical.saved_counts.get_mut(&id).unwrap().pushed = true;
        assert_eq!(
            parse_save_inventory(&noncanonical.export_original(&hud)?, saved.items.len())?.items[0]
                .state()
                .pushed,
            0xfe
        );
        let mut invalid_selection = snapshot;
        invalid_selection.selected = None;
        invalid_selection
            .original_encoding
            .as_mut()
            .unwrap()
            .selected = i32::MAX;
        assert!(
            invalid_selection
                .export_original(&hud)
                .unwrap_err()
                .to_string()
                .contains("selection")
        );
        Ok(())
    }
}
