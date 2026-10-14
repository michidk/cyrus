//! World script caches and validated installed clock/update settings.
pub mod inventory;
use anyhow::Context;
use cyrus_script::program::{Call, Definition, Program};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Validated installed `[system]` inputs for the shared FPS-derived game clock.
/// Rates are in nominal frames per BIOS measurement window. A stored normal rate
/// of zero is resolved to twelve during preparation, following preservation's
/// `docs/engine/player.md#fps-derived-frame-scale`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClockSettings {
    pub normal_frame_rate: u32,
    pub min_frame_rate: u32,
    pub max_frame_rate: u32,
    pub use_smooth_fps: bool,
    pub use_smooth_divisor: bool,
}

impl ClockSettings {
    /// Prepare shared startup and world timing from installed typed settings.
    #[cfg(feature = "parsers")]
    pub fn from_system(system: &rgpre::import::system_ini::SystemSettings) -> anyhow::Result<Self> {
        let rate = |name: &str, value: Option<i32>| -> anyhow::Result<u32> {
            let value = value.with_context(|| format!("SYSTEM.INI [system] {name} missing"))?;
            u32::try_from(value).with_context(|| format!("SYSTEM.INI [system] {name} is negative"))
        };
        let normal = rate("normal_frame_rate", system.normal_frame_rate)?;
        let settings = ClockSettings {
            normal_frame_rate: if normal == 0 { 12 } else { normal },
            min_frame_rate: rate("min_frame_rate", system.min_frame_rate)?,
            max_frame_rate: rate("max_frame_rate", system.max_frame_rate)?,
            use_smooth_fps: system
                .use_smooth_fps
                .context("SYSTEM.INI [system] use_smooth_fps missing")?,
            use_smooth_divisor: system
                .use_smooth_divisor
                .context("SYSTEM.INI [system] use_smooth_divisor missing")?,
        };
        settings.validate()?;
        Ok(settings)
    }

    /// Reject invalid divisors and rates whose original signed numerator overflows.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=i32::MAX as u32 / 256).contains(&self.normal_frame_rate),
            "SYSTEM.INI [system] normal_frame_rate {} is unsupported",
            self.normal_frame_rate
        );
        anyhow::ensure!(
            self.min_frame_rate > 0
                && self.min_frame_rate <= self.max_frame_rate
                && self.max_frame_rate <= i32::MAX as u32,
            "SYSTEM.INI [system] invalid min_frame_rate {} / max_frame_rate {}",
            self.min_frame_rate,
            self.max_frame_rate
        );
        Ok(())
    }
}

/// Installed switches for ordinary actor updates, separate from per-actor control flags.
/// Eligibility and stage ordering follow preservation's engine/animation.md and SOUP.md.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateSettings {
    pub task_system: bool,
    pub animation_system: bool,
    pub disable_slaves: bool,
    pub disable_master_slaves: bool,
}

impl UpdateSettings {
    /// Missing switches are unsupported rather than assigned invented defaults.
    #[cfg(feature = "parsers")]
    pub fn from_system(settings: &rgpre::import::system_ini::SystemIni) -> anyhow::Result<Self> {
        Ok(Self {
            task_system: settings
                .system
                .task_system
                .context("SYSTEM.INI [system] task_system missing")?,
            animation_system: settings
                .system
                .animation_system
                .context("SYSTEM.INI [system] animation_system missing")?,
            disable_slaves: settings
                .debug
                .disable_slaves
                .context("SYSTEM.INI [debug] disable_slaves missing")?,
            disable_master_slaves: settings
                .debug
                .disable_master_slaves
                .context("SYSTEM.INI [debug] disable_master_slaves missing")?,
        })
    }
}

/// Script metadata remains available even for actors without animation geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub header_index: usize,
    /// Byte offset within RASC data, retained for foreign task operand identity.
    pub code_origin: u32,
    pub instances: u16,
    pub namespace: Option<String>,
    pub instance_variables: Vec<Vec<i32>>,
    /// Authored Q8 RALC offsets relative to each instance's original home.
    pub location_offsets: Vec<[i32; 3]>,
    pub program: Program,
    /// Independent actor-load attribute defaults; model-path message groups are assigned later.
    pub attributes: BTreeMap<String, u8>,
    /// Per-update RAHK native call, distinct from the RASC entry continuation.
    pub native_hook: Option<Call>,
}

/// Authored MPOB order identifies logical objects independently of mesh availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub object_id: u32,
    pub actor: String,
    pub object_type: u8,
    pub active: u8,
    /// Authored MPOB model override; empty when absent.
    pub model_name: String,
    /// Raw MPOB model-mode byte, retained without boolean conversion.
    pub model_mode: u8,
    /// Packed map coordinates retained in their original field units.
    pub map_position: [i64; 3],
    pub angles: [u32; 3],
}

#[derive(bevy::prelude::Asset, bevy::prelude::TypePath, Debug, Clone, Serialize, Deserialize)]
pub struct Scripts {
    pub version: u32,
    pub clock: ClockSettings,
    pub updates: UpdateSettings,
    /// Initial `neversaydie` cheat state, loaded from optional REDGUARD.CHT.
    pub player_distance_override: bool,
    /// Installed reusable actor-pool capacity, including the engine's minimum of ten.
    pub actor_pool_capacity: u32,
    pub content_id: String,
    pub world: u32,
    pub definition: Definition,
    pub actors: BTreeMap<String, Actor>,
    pub instances: Vec<Instance>,
    pub unsupported: BTreeMap<String, String>,
}

impl Scripts {
    /// Stable identity covers all definitions, actor programs and unsupported records.
    pub fn content_identity(&self) -> anyhow::Result<String> {
        let bytes = serde_json::to_vec(&(
            self.version,
            &self.clock,
            &self.updates,
            self.player_distance_override,
            self.actor_pool_capacity,
            self.world,
            &self.definition,
            &self.actors,
            &self.instances,
            &self.unsupported,
        ))?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 10,
            "world {}: script schema {} unsupported; re-export the world",
            self.world,
            self.version
        );
        self.clock
            .validate()
            .with_context(|| format!("world {} script clock", self.world))?;
        anyhow::ensure!(
            (10..=i32::MAX as u32).contains(&self.actor_pool_capacity),
            "world {}: invalid script actor pool capacity {}",
            self.world,
            self.actor_pool_capacity
        );
        anyhow::ensure!(
            self.content_id == self.content_identity()?,
            "world {}: script content identity mismatch; re-export the world",
            self.world
        );
        anyhow::ensure!(
            self.definition
                .functions
                .first()
                .is_some_and(
                    |function| function.name == "NullFunction" && function.param_count == 0
                ),
            "world {}: null function metadata absent",
            self.world
        );
        for (name, actor) in &self.actors {
            anyhow::ensure!(
                actor.instance_variables.len() == usize::from(actor.instances)
                    && actor
                        .instance_variables
                        .iter()
                        .all(|values| values.len() == actor.program.variables.len()),
                "world {} actor {name}: incomplete instance locals",
                self.world
            );
            if let Some(hook) = &actor.native_hook {
                actor
                    .program
                    .validate_native_hook(hook, &self.definition)
                    .with_context(|| format!("world {} actor {name} native hook", self.world))?;
            }
            actor
                .program
                .validate_definition(&self.definition)
                .with_context(|| format!("world {} actor {name} script", self.world))?;
        }
        Ok(())
    }
}

#[derive(Default, bevy::prelude::TypePath)]
pub struct ScriptLoader;
impl bevy::asset::AssetLoader for ScriptLoader {
    type Asset = Scripts;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn bevy::asset::io::Reader,
        _: &(),
        context: &mut bevy::asset::LoadContext<'_>,
    ) -> anyhow::Result<Scripts> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let scripts: Scripts = serde_json::from_slice(&bytes)
            .with_context(|| format!("script cache {}; re-export the world", context.path()))?;
        scripts.validate()?;
        Ok(scripts)
    }
    fn extensions(&self) -> &[&str] {
        &["scripts"]
    }
}
