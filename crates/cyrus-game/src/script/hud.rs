//! Script power-up slots and accepted logbook writes, independent of HUD rendering.
use crate::preferences::{PreferenceKey, PreferenceStore, Preferences};
use anyhow::{Context, Result};
use bevy::prelude::Resource;
use cyrus_assets::hud::HudAsset;
use cyrus_script::Argument;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// Shared native/browser script HUD state. Rules follow preservation's engine/hud.md.
/// Slot state and duplicate tags survive world changes within this game session.
#[derive(Resource, Default)]
pub struct ScriptHud(Mutex<State>);

#[derive(Default)]
struct State {
    data: Option<(usize, BTreeMap<String, String>)>,
    powerups: [Option<(u8, u8)>; 2],
    next_slot: usize,
    seen: Vec<i32>,
    entries: Option<Result<Vec<String>, String>>,
    store: Option<Arc<dyn PreferenceStore>>,
    notification: bool,
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
        state.data = Some((hud.powerup_sizes.len(), hud.labels.clone()));
        state.store = preferences.store();
        state.entries = Some(
            (|| {
                let store = state
                    .store
                    .as_ref()
                    .context("local logbook store unavailable")?;
                let entries = store
                    .load(PreferenceKey::Logbook)?
                    .map(|text| serde_json::from_str::<Vec<String>>(&text))
                    .transpose()?
                    .unwrap_or_default();
                Ok(entries)
            })()
            .map_err(|error: anyhow::Error| format!("local logbook: {error:#}")),
        );
    }

    /// Installed script HUD inputs have been supplied by the host.
    pub fn ready(&self) -> bool {
        self.0
            .lock()
            .expect("script HUD lock poisoned")
            .data
            .is_some()
    }

    /// Current slots and a consumed notification request. Rendering owns icon animation.
    pub fn presentation(&self) -> ([Option<(u8, u8)>; 2], bool) {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        (state.powerups, std::mem::take(&mut state.notification))
    }

    /// Accepted logbook text, or the retained local-state load error.
    pub fn entries(&self) -> Result<Vec<String>> {
        let state = self.0.lock().expect("script HUD lock poisoned");
        state
            .entries
            .as_ref()
            .context("script HUD data unavailable")?
            .clone()
            .map_err(anyhow::Error::msg)
    }

    /// Control diagnostics without consuming pending presentation requests or exposing RTX text.
    pub fn inspect(&self) -> serde_json::Value {
        let state = self.0.lock().expect("script HUD lock poisoned");
        serde_json::json!({
            "ready": state.data.is_some(),
            "powerups": state.powerups,
            "logbook_entries": state.entries.as_ref().and_then(|entries| entries.as_ref().ok()).map(Vec::len),
            "logbook_error": state.entries.as_ref().and_then(|entries| entries.as_ref().err()),
        })
    }

    pub(super) fn handles(function: &str) -> bool {
        ["ShowBitmap", "UnShowBitmap", "AddLog"]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
    }

    pub(super) fn call(&self, function: &str, arguments: &[Argument]) -> Result<i32> {
        let mut state = self.0.lock().expect("script HUD lock poisoned");
        let (frames, _) = state
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
            let tag = tag.integer()?;
            // Zero remains an empty slot; an unmatched tag is not retained after 400 tags.
            if tag != 0 && state.seen.contains(&tag) {
                return Ok(0);
            }
            if tag != 0 && state.seen.len() < 400 {
                state.seen.push(tag);
            }
            let bytes = tag.to_le_bytes();
            let text = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|label| state.data.as_ref().unwrap().1.get(label))
                .cloned();
            if let Some(text) = text {
                let mut entries = state
                    .entries
                    .as_ref()
                    .unwrap()
                    .clone()
                    .map_err(anyhow::Error::msg)?;
                entries.push(text);
                let serialized = serde_json::to_string_pretty(&entries)?;
                state
                    .store
                    .as_ref()
                    .context("local logbook store unavailable")?
                    .save(PreferenceKey::Logbook, &serialized)
                    .context("AddLog local logbook write")?;
                state.entries = Some(Ok(entries));
                state.notification = true;
            }
        }
        Ok(0)
    }
}
