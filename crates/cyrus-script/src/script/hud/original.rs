//! Saved HUD ownership is decoded without acquisition callbacks or storage writes.
use super::*;
use rgpre::import::savegame::{SaveInventory, SaveSessionGlobals};

impl ScriptHud {
    /// Construct the original-load replacement independently of the live HUD.
    /// Inventory, duplicate tags, compass, bitmap slots and candle countdown come
    /// from the save. The input-release latch retains its explicit prior owner.
    /// The unsaved journal notification retains its prior owner; exact rendered
    /// icon playback remains part of the enclosing host service transaction.
    pub fn from_original_load(
        prior: &Self,
        data: &HudAsset,
        preferences: &Preferences,
        inventory: &SaveInventory,
        saved: &SaveSessionGlobals,
        logbook: Option<&str>,
    ) -> Result<Self> {
        let mut snapshot = prior.snapshot()?;
        snapshot.inventory =
            super::super::Inventory::from_original_load(data, inventory, &snapshot.inventory)?
                .snapshot();
        snapshot.compass_on = saved.compass_display != 0;
        snapshot.health_feedback_ticks = saved
            .health_candle_display
            .try_into()
            .context("original SVG2 candle countdown is negative")?;
        snapshot.next_slot = saved
            .power_ups
            .alternation
            .try_into()
            .context("original SVG2 power-up alternation is negative")?;
        snapshot.powerups = [None; 2];
        for (slot, entry) in snapshot.powerups.iter_mut().enumerate() {
            let frame = saved.power_ups.frame_indices[slot];
            if frame != -1 {
                *entry = Some((
                    saved.power_ups.frames[slot]
                        .try_into()
                        .context("original SVG2 power-up id exceeds byte range")?,
                    frame
                        .try_into()
                        .context("original SVG2 power-up frame exceeds byte range")?,
                ));
            }
        }
        snapshot.journal = super::super::Journal::original_snapshot(
            snapshot.journal,
            data,
            &saved.journal_tags,
            logbook,
        )?;
        Self::from_snapshot(data, preferences, snapshot)
    }
}
