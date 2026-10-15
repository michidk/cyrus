//! Session inventory, compass use, power-up slots and accepted logbook writes.
use crate::preferences::Preferences;
use anyhow::{Context, Result};
use bevy::prelude::Resource;
use cyrus_assets::hud::HudAsset;
use cyrus_script::Argument;
use std::sync::Mutex;

/// Result of the current item-use gates, before consuming the held-use latch.
#[derive(Clone, Copy)]
pub enum CompassUse {
    /// Lockout, edge state or another interaction defers the attempt.
    Blocked,
    /// This attempt consumes the press without using an owned compass.
    Rejected,
    /// The selected, owned compass passed item-use eligibility.
    Accepted,
}

/// Shared native/browser script HUD state. Rules follow preservation's engine/hud.md.
/// Inventory, slots, compass state and duplicate tags survive world changes within the session.
#[derive(Resource, Default)]
pub struct ScriptHud(Mutex<State>);

#[derive(Default)]
struct State {
    data: Option<usize>,
    inventory: Option<super::inventory::Inventory>,
    inventory_error: Option<String>,
    powerups: [Option<(u8, u8)>; 2],
    next_slot: usize,
    journal: Option<super::Journal>,
    compass_on: bool,
    use_consumed: bool,
}

impl std::fmt::Debug for ScriptHud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHud").finish_non_exhaustive()
    }
}

impl ScriptHud {
    /// Configure once from the installed atlas and RTX catalog, retaining local load errors.
    pub fn configure(&self, hud: &HudAsset, preferences: &Preferences) {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        if state.data.is_some() {
            return;
        }
        match super::inventory::Inventory::new(hud) {
            Ok(inventory) => state.inventory = Some(inventory),
            Err(error) => state.inventory_error = Some(format!("{error:#}")),
        }
        state.data = Some(hud.powerup_sizes.len());
        state.journal = Some(super::Journal::load(hud, preferences));
    }

    /// Installed script HUD inputs have been supplied by the host.
    pub fn ready(&self) -> bool {
        let state = self.0.lock().expect("script HUD lock poisoned");
        state.data.is_some() && state.inventory_error.is_none()
    }

    /// Current slots and a consumed notification request. Rendering owns icon animation.
    pub fn presentation(&self) -> ([Option<(u8, u8)>; 2], bool) {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        (
            state.powerups,
            state
                .journal
                .as_mut()
                .is_some_and(super::Journal::take_notification),
        )
    }

    /// Use item 0 once per held input. The display flag survives world changes.
    /// Release rearms use; blocked attempts retain it, rejected attempts consume it.
    /// Save-game restore and new-game reset remain owned by the unfinished lifecycle.
    pub fn compass_input(&self, held: bool, attempt: CompassUse) -> bool {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        if !held {
            state.use_consumed = false;
        } else if !state.use_consumed && !matches!(attempt, CompassUse::Blocked) {
            state.use_consumed = true;
            if matches!(attempt, CompassUse::Accepted) {
                state.compass_on = !state.compass_on;
            }
        }
        state.compass_on
    }

    /// Repair exhausted selection and optionally cycle once through visible owned items.
    pub fn inventory(&self, cycle: Option<bool>) -> Option<super::InventoryView> {
        self.0
            .lock()
            .expect("script HUD lock poisoned")
            .inventory
            .as_mut()
            .map(|items| items.view(cycle))
    }

    /// Apply installed recursive prerequisites and ordered acquisition effects.
    /// Empty-selection repair calls the selected-player assignment before count increment.
    /// The callback must not reenter this HUD, whose inventory lock remains held.
    pub fn add_item(
        &self,
        id: u16,
        amount: i32,
        assign_weapon: &mut dyn FnMut(u8) -> Result<()>,
    ) -> Result<()> {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        if let Some(error) = &state.inventory_error {
            anyhow::bail!("inventory initialization: {error}");
        }
        state
            .inventory
            .as_mut()
            .context("installed inventory data unavailable")?
            .add(id, amount, assign_weapon)
    }

    /// Selected record ID, independent of ownership or visibility; zero when closed.
    pub fn active_item(&self) -> Result<u16> {
        let state = self.0.lock().expect("script HUD lock poisoned");
        if let Some(error) = &state.inventory_error {
            anyhow::bail!("inventory initialization: {error}");
        }
        Ok(state
            .inventory
            .as_ref()
            .and_then(super::inventory::Inventory::selected)
            .unwrap_or(0))
    }

    pub(super) fn inventory_ready(&self) -> Result<bool> {
        let state = self.0.lock().expect("script HUD lock poisoned");
        if let Some(error) = &state.inventory_error {
            anyhow::bail!("inventory initialization: {error}");
        }
        Ok(state.inventory.is_some())
    }

    pub(super) fn owned_weapon(&self, id: u8) -> Result<bool> {
        let state = self.0.lock().expect("script HUD lock poisoned");
        if let Some(error) = &state.inventory_error {
            anyhow::bail!("inventory initialization: {error}");
        }
        Ok(state
            .inventory
            .as_ref()
            .is_some_and(|items| items.owned_weapon(id)))
    }

    pub(super) fn validate_item(&self, id: u16) -> Result<()> {
        let state = self.0.lock().expect("script HUD lock poisoned");
        state
            .inventory
            .as_ref()
            .context("installed inventory data unavailable")?
            .count(id)?;
        Ok(())
    }

    /// Script selection accepts any defined ID; the item overlay repairs exhausted selection.
    pub fn select_item(&self, id: u16) -> Result<()> {
        self.0
            .lock()
            .expect("script HUD lock poisoned")
            .inventory
            .as_mut()
            .context("installed inventory data unavailable")?
            .select(id)
    }

    /// Accepted logbook text, or the retained local-state load error.
    pub fn entries(&self) -> Result<Vec<String>> {
        let state = self.0.lock().expect("script HUD lock poisoned");
        Ok(state
            .journal
            .as_ref()
            .context("script HUD data unavailable")?
            .entries()?
            .to_vec())
    }

    /// Capture accepted text, duplicate tags and pending notification without effects.
    /// Unconfigured or malformed local journal state cannot be saved.
    pub fn snapshot_journal(&self) -> Result<super::JournalSnapshot> {
        self.0
            .lock()
            .expect("script HUD lock poisoned")
            .journal
            .as_ref()
            .context("script HUD data unavailable")?
            .snapshot()
    }

    /// Control diagnostics without consuming pending presentation requests or exposing RTX text.
    pub fn inspect(&self) -> serde_json::Value {
        let state = self.0.lock().expect("script HUD lock poisoned");
        serde_json::json!({
            "ready": state.data.is_some() && state.inventory_error.is_none(),
            "powerups": state.powerups,
            "compass_on": state.compass_on,
            "inventory_error": state.inventory_error,
            "inventory": state.inventory.as_ref().map(super::inventory::Inventory::inspect),
            "logbook_entries": state.journal.as_ref().and_then(|journal| journal.entries().ok()).map(<[String]>::len),
            "logbook_error": state.journal.as_ref().and_then(|journal| journal.entries().err()).map(|error| error.to_string()),
        })
    }

    pub(super) fn handles(function: &str) -> bool {
        [
            "ShowBitmap",
            "UnShowBitmap",
            "AddLog",
            "DropItem",
            "HaveItem",
            "SelectItem",
        ]
        .iter()
        .any(|name| function.eq_ignore_ascii_case(name))
    }

    pub(super) fn call(&self, function: &str, arguments: &[Argument]) -> Result<i32> {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        if ["DropItem", "HaveItem", "SelectItem"]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
        {
            let expected = if function.eq_ignore_ascii_case("DropItem") {
                2
            } else {
                1
            };
            anyhow::ensure!(
                arguments.len() == expected,
                "{function} requires {expected} arguments"
            );
            if (function.eq_ignore_ascii_case("DropItem")
                || function.eq_ignore_ascii_case("HaveItem"))
                && state.inventory.is_none()
                && state.inventory_error.is_none()
            {
                return Ok(0);
            }
            let id = u16::try_from(arguments[0].integer()?)
                .context("inventory item ID outside u16 range")?;
            if let Some(error) = &state.inventory_error {
                anyhow::bail!("inventory initialization: {error}");
            }
            let items = state
                .inventory
                .as_mut()
                .context("installed inventory data unavailable")?;
            if function.eq_ignore_ascii_case("HaveItem") {
                return i32::try_from(items.count(id)?)
                    .context("item count outside script integer range");
            }
            if function.eq_ignore_ascii_case("DropItem") {
                items.remove(id, arguments[1].integer()?)?;
            } else {
                items.select(id)?;
            }
            return Ok(0);
        }
        let frames = state
            .data
            .as_ref()
            .context("installed script HUD data unavailable")?;
        if function.eq_ignore_ascii_case("ShowBitmap") {
            let [id, frame] = arguments else {
                anyhow::bail!("ShowBitmap requires two arguments")
            };
            let (id, frame) = (id.integer()?, frame.integer()?);
            anyhow::ensure!(
                (0..=1).contains(&id) && frame >= 0 && (frame as usize) < *frames,
                "ShowBitmap({id}, {frame}): invalid id or installed atlas frame"
            );
            let (id, frame) = (
                id as u8,
                u8::try_from(frame).context("ShowBitmap frame exceeds HUD range")?,
            );
            if !state
                .powerups
                .iter()
                .flatten()
                .any(|(_, shown)| *shown == frame)
            {
                let slot = state
                    .powerups
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(state.next_slot);
                state.powerups[slot] = Some((id, frame));
                state.next_slot = 1 - slot;
            }
        } else if function.eq_ignore_ascii_case("UnShowBitmap") {
            let [id] = arguments else {
                anyhow::bail!("UnShowBitmap requires one argument")
            };
            let id = id.integer()?;
            if let Some(slot) = state
                .powerups
                .iter_mut()
                .find(|slot| slot.is_some_and(|(shown, _)| i32::from(shown) == id))
            {
                *slot = None;
            }
        } else {
            let [tag] = arguments else {
                anyhow::bail!("AddLog requires one argument")
            };
            state
                .journal
                .as_mut()
                .context("script HUD data unavailable")?
                .add_log(tag.integer()?)?;
        }
        Ok(0)
    }
}
