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
    TorchRetention,
    ActorUpdateSkip,
    MagicCarpet,
    DamageSuppression,
    PlayerCollisionSkip,
    TaskSystem,
    AnimationSystem,
    Unsupported,
}

/// Runtime state is shared by every command source and retained across worlds.
#[derive(Resource)]
pub struct Cheats {
    store: Option<Box<dyn CheatStore>>,
    state: Result<CheatFile, String>,
}

/// Complete raw cheat values with their decoded slot layout; no storage binding.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CheatSnapshot {
    version: u32,
    names: Vec<Option<String>>,
    values: Vec<u32>,
}

/// Validated state for coordinated publication, preserving the current host store.
pub struct CheatReplacement(CheatFile);

impl CheatReplacement {
    pub fn validate(&self, live: &Cheats) -> Result<()> {
        anyhow::ensure!(
            self.0.entries().iter().map(|entry| &entry.name).eq(live
                .state()?
                .entries()
                .iter()
                .map(|entry| &entry.name)),
            "cheat replacement slot layout differs"
        );
        Ok(())
    }

    /// Call after aggregate preflight; changes only retained values, without I/O.
    pub fn publish(self, live: &mut Cheats) {
        live.state = Ok(self.0);
    }
}

impl CheatSnapshot {
    pub fn prepare(self, live: &Cheats) -> Result<CheatReplacement> {
        anyhow::ensure!(self.version == 1, "cheat snapshot version unsupported");
        let state = live.state()?;
        anyhow::ensure!(
            self.names
                .iter()
                .eq(state.entries().iter().map(|entry| &entry.name)),
            "cheat snapshot slot layout differs"
        );
        Ok(CheatReplacement(state.with_values(&self.values)?))
    }
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
    pub fn snapshot(&self) -> Result<CheatSnapshot> {
        let state = self.state()?;
        Ok(CheatSnapshot {
            version: 1,
            names: state
                .entries()
                .iter()
                .map(|entry| entry.name.clone())
                .collect(),
            values: state.entries().iter().map(|entry| entry.value).collect(),
        })
    }
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
            "goldfinger" => CheatEffect::TorchRetention,
            "drno" => CheatEffect::ActorUpdateSkip,
            "magiccarpet" => CheatEffect::MagicCarpet,
            "drevil" => CheatEffect::DamageSuppression,
            "moonraker" => CheatEffect::PlayerCollisionSkip,
            "task" => CheatEffect::TaskSystem,
            "animation" => CheatEffect::AnimationSystem,
            _ => CheatEffect::Unsupported,
        }
    }

    /// The drno state: the ordinary main stage is skipped for every actor except the selected
    /// player and the camera actor. Active for any nonzero value.
    pub fn actor_update_skipped(&self) -> Result<bool> {
        Ok(self.state()?.value("drno").context("missing drno state")? != 0)
    }

    /// The yeahbaby state: the selected player's ground-support probe is skipped while a vertical
    /// debug key is held or the Home toggle is set. Active for any nonzero value. The ground
    /// bypass, the Page Up and Page Down movement and the Insert step with its camera limiter
    /// suspension are integrated. The catalog reports the effect unsupported pending complete
    /// native and browser acceptance.
    pub fn ground_bypass(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("yeahbaby")
            .context("missing yeahbaby state")?
            != 0)
    }

    /// The magiccarpet state: the selected player's ground-support probe clears its falling byte
    /// and ends. Active only for the value 1, not for any other nonzero value.
    pub fn magic_carpet(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("magiccarpet")
            .context("missing magiccarpet state")?
            == 1)
    }

    /// The moonraker state: the selected player skips the forward obstacle probes and the actor
    /// overlap pass of the late collision stage. Active for any nonzero value.
    pub fn player_collision_skipped(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("moonraker")
            .context("missing moonraker state")?
            != 0)
    }

    /// The `task` state: the task scheduling system runs only for the value 1.
    pub fn task_system(&self) -> Result<bool> {
        Ok(self.state()?.value("task").context("missing task state")? == 1)
    }

    /// The `animation` state as the animation system's enable: the value 1. The original's other
    /// checks accept any nonzero value, a split that is not modelled, so a value of 2 or more
    /// disables the system here.
    pub fn animation_system(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("animation")
            .context("missing animation state")?
            == 1)
    }

    /// The ordinary animated-player draw is suppressed while the `animation` state is zero; it does
    /// not keep a frozen pose. Other values leave it drawn.
    pub fn animated_player_hidden(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("animation")
            .context("missing animation state")?
            == 0)
    }

    /// The goldfinger state for the selected player's burning torch, active for any nonzero value.
    pub fn torch_retained(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("goldfinger")
            .context("missing goldfinger state")?
            != 0)
    }

    /// The original PlayerDistance override is active for any nonzero value.
    pub fn distance_override(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("neversaydie")
            .context("missing neversaydie state")?
            != 0)
    }

    /// Raw nonzero drevil state: the selected player takes no health change and starts no death
    /// response from a combat hit or a direct-damage message, and its death-owning health does not
    /// start death at zero.
    pub fn death_suppressed(&self) -> Result<bool> {
        Ok(self
            .state()?
            .value("drevil")
            .context("missing drevil state")?
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
                    let mut saved = self.state()?.clone();
                    saved.set("savecheats", 0)?;
                    self.store
                        .as_ref()
                        .context("cheat storage is not initialized")?
                        .save(&saved)?;
                    self.state
                        .as_mut()
                        .map_err(|error| anyhow::anyhow!("{error}"))?
                        .set("savecheats", 1)?;
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

    /// Explicit CHT import is the recovery path for malformed local state.
    /// Validate before calling; commit runtime state only after durable storage succeeds.
    pub fn import(&mut self, state: CheatFile) -> Result<()> {
        self.store
            .as_ref()
            .context("cheat storage is not initialized")?
            .save(&state)?;
        self.state = Ok(state);
        Ok(())
    }

    /// Console toggles use parity, unlike gameplay's nonzero toggle.
    pub fn console(&mut self, name: &str, value: Option<u32>) -> Result<CheatEffect> {
        if matches!(Self::effect(name), CheatEffect::List | CheatEffect::Save) {
            return self.execute(name, None);
        }
        let previous = self
            .state()?
            .value(name)
            .with_context(|| format!("unknown cheat {name:?}"))?;
        self.execute(name, Some(value.unwrap_or((previous & 1) ^ 1)))
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

/// Host-owned file dialogs are exposed through the existing debug UI.
#[derive(Resource, Default)]
pub struct CheatTransfer {
    pub available: bool,
    pub action: Option<CheatFileAction>,
    pub status: Option<String>,
}
#[derive(Clone, Copy)]
pub enum CheatFileAction {
    Import,
    Export,
}

#[cfg(test)]
mod system_switch_tests {
    use super::*;

    struct Memory;
    impl CheatStore for Memory {
        fn load(&self) -> Result<CheatFile> {
            Ok(cyrus_assets::cheats::empty())
        }
        fn save(&self, _: &CheatFile) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn task_and_animation_are_on_only_for_the_value_one() {
        let mut cheats = Cheats::new(Memory);
        for name in ["task", "animation"] {
            assert!(matches!(
                Cheats::effect(name),
                CheatEffect::TaskSystem | CheatEffect::AnimationSystem
            ));
        }
        for (value, on) in [(0, false), (1, true), (2, false), (7, false)] {
            cheats.execute("task", Some(value)).unwrap();
            cheats.execute("animation", Some(value)).unwrap();
            assert_eq!(cheats.task_system().unwrap(), on, "task {value}");
            assert_eq!(cheats.animation_system().unwrap(), on, "animation {value}");
        }
    }
}
