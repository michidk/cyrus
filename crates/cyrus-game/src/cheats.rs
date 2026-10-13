//! Shared cheat state and explicit effects; hosts own CHT storage and presentation.
use anyhow::{Context, Result};
use bevy::prelude::Resource;
use cyrus_assets::cheats::CheatFile;
use serde::Serialize;

/// Host storage returns decoded state and writes a complete CHT document.
/// A missing file uses empty state; a malformed file remains an error.
pub trait CheatStore: Send + Sync + 'static {
    fn load(&self) -> Result<CheatFile>;
    fn save(&self, state: &CheatFile) -> Result<()>;
}

/// The supported effect of a named slot in the current runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheatEffect {
    List,
    Save,
    PlayerDistance,
    NoEffect,
    Unsupported,
}

/// Runtime state is shared by every command source and retained across worlds.
#[derive(Resource)]
pub struct Cheats {
    store: Option<Box<dyn CheatStore>>,
    state: Result<CheatFile, String>,
}

impl Default for Cheats {
    fn default() -> Self {
        Self {
            store: None,
            state: Err("cheat storage is not initialized".into()),
        }
    }
}

impl Cheats {
    pub fn new(store: impl CheatStore) -> Self {
        let state = store
            .load()
            .map_err(|error| format!("REDGUARD.CHT: {error:#}"));
        Self {
            store: Some(Box::new(store)),
            state,
        }
    }

    pub fn state(&self) -> Result<&CheatFile> {
        self.state
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    pub fn effect(name: &str) -> CheatEffect {
        match name {
            "oracle" => CheatEffect::List,
            "savecheats" => CheatEffect::Save,
            "neversaydie" => CheatEffect::PlayerDistance,
            "oddjob" => CheatEffect::NoEffect,
            _ => CheatEffect::Unsupported,
        }
    }

    /// The original PlayerDistance override is active for any nonzero value.
    pub fn distance_override(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("neversaydie")
            .context("missing neversaydie state")?
            != 0)
    }

    /// Apply a named toggle or explicit raw value. Unsupported effects still retain
    /// their state for compatible CHT persistence and report their support status.
    /// `oracle` and `savecheats` without a value perform their documented actions.
    pub fn execute(&mut self, name: &str, value: Option<u32>) -> Result<CheatEffect> {
        let previous = self
            .state()?
            .value(name)
            .with_context(|| format!("unknown cheat {name:?}"))?;
        let effect = Self::effect(name);
        if value.is_none() {
            match effect {
                CheatEffect::List => return Ok(effect),
                CheatEffect::Save => {
                    self.store
                        .as_ref()
                        .context("cheat storage is not initialized")?
                        .save(self.state()?)?;
                    return Ok(effect);
                }
                _ => {}
            }
        }
        let state = self
            .state
            .as_mut()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        state.set(name, value.unwrap_or(u32::from(previous == 0)))?;
        Ok(effect)
    }

    pub fn inspect(&self) -> serde_json::Value {
        match self.state() {
            Ok(state) => serde_json::json!({
                "ready": true,
                "entries": state.entries().iter().filter_map(|entry| entry.name.as_ref().map(|name|
                    serde_json::json!({"name":name,"value":entry.value,"effect":Self::effect(name)})
                )).collect::<Vec<_>>()
            }),
            Err(error) => serde_json::json!({"ready":false,"error":error.to_string()}),
        }
    }
}
