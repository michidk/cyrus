//! Accepted journal text and AddLog duplicate history, independent of presentation.
use crate::preferences::{PreferenceKey, PreferenceStore, Preferences};
use anyhow::{Context, Result};
use cyrus_assets::hud::HudAsset;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

/// Complete journal state tied to the installed RTX definitions.
/// Decode first, then construct a replacement with [`Journal::from_snapshot`].
/// This is a Cyrus snapshot, not an original save-file record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalSnapshot {
    version: u32,
    labels: BTreeMap<String, String>,
    seen: Vec<i32>,
    entries: Vec<String>,
    notification: bool,
}

/// Accepted text, duplicate history and pending icon request for one session.
/// Rules follow preservation's `docs/engine/hud.md#script-logbook-entries`.
/// Storage is attached by the host and is never part of a snapshot.
pub struct Journal {
    labels: BTreeMap<String, String>,
    seen: Vec<i32>,
    entries: Result<Vec<String>, String>,
    notification: bool,
    store: Option<Arc<dyn PreferenceStore>>,
}

impl Journal {
    pub(super) fn load(hud: &HudAsset, preferences: &Preferences) -> Self {
        let store = preferences.store();
        let entries = (|| -> Result<Vec<String>> {
            let text = store
                .as_ref()
                .context("local logbook store unavailable")?
                .load(PreferenceKey::Logbook)?;
            Ok(text
                .map(|text| serde_json::from_str(&text))
                .transpose()?
                .unwrap_or_default())
        })()
        .map_err(|error| format!("local logbook: {error:#}"));
        Self {
            labels: hud.labels.clone(),
            seen: Vec::new(),
            entries,
            notification: false,
            store,
        }
    }

    /// Capture without consuming notification or reading/writing the backend.
    /// A retained local load error prevents capture of an accepted journal.
    pub fn snapshot(&self) -> Result<JournalSnapshot> {
        Ok(JournalSnapshot {
            version: 1,
            labels: self.labels.clone(),
            seen: self.seen.clone(),
            entries: self.entries()?.to_vec(),
            notification: self.notification,
        })
    }

    /// Validate and construct a separate owner without loading or saving storage,
    /// replaying AddLog, consuming notification, or changing any live HUD.
    /// Unknown tags are valid: AddLog retains tags before installed-text lookup.
    pub fn from_snapshot(
        hud: &HudAsset,
        preferences: &Preferences,
        snapshot: JournalSnapshot,
    ) -> Result<Self> {
        anyhow::ensure!(
            snapshot.version == 1,
            "journal snapshot version {} is unsupported",
            snapshot.version
        );
        anyhow::ensure!(
            snapshot.labels == hud.labels,
            "journal installed RTX definitions differ"
        );
        anyhow::ensure!(
            snapshot.seen.len() <= 400,
            "journal duplicate tag table exceeds 400 slots"
        );
        let mut seen = std::collections::BTreeSet::new();
        for tag in &snapshot.seen {
            anyhow::ensure!(
                *tag != 0,
                "journal duplicate tag table contains empty sentinel"
            );
            anyhow::ensure!(
                seen.insert(*tag),
                "journal duplicate tag table repeats tag {tag}"
            );
        }
        Ok(Self {
            labels: snapshot.labels,
            seen: snapshot.seen,
            entries: Ok(snapshot.entries),
            notification: snapshot.notification,
            store: preferences.store(),
        })
    }

    /// Accepted text verbatim; malformed local state remains an error.
    pub fn entries(&self) -> Result<&[String]> {
        self.entries
            .as_deref()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    /// Consume the pending icon request; capture and reconstruction never do so.
    pub fn take_notification(&mut self) -> bool {
        std::mem::take(&mut self.notification)
    }

    pub(super) fn label(&self, label: &str) -> Option<&str> {
        self.labels.get(label).map(String::as_str)
    }

    /// Apply AddLog's ordered tag retention and installed-text lookup.
    /// Failed writes preserve accepted entries and notification, but retain the tag.
    pub fn add_log(&mut self, tag: i32) -> Result<()> {
        if tag != 0 && self.seen.contains(&tag) {
            return Ok(());
        }
        if tag != 0 && self.seen.len() < 400 {
            self.seen.push(tag);
        }
        let bytes = tag.to_le_bytes();
        let text = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|label| self.labels.get(label));
        if let Some(text) = text {
            let mut entries = self.entries()?.to_vec();
            entries.push(text.clone());
            let serialized = serde_json::to_string_pretty(&entries)?;
            self.store
                .as_ref()
                .context("local logbook store unavailable")?
                .save(PreferenceKey::Logbook, &serialized)
                .context("AddLog local logbook write")?;
            self.entries = Ok(entries);
            self.notification = true;
        }
        Ok(())
    }
}
