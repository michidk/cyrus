//! Serialized player scene inputs, before game-owned runtime preparation.
use crate::collision::CollisionScene;
use serde::{Deserialize, Serialize};
/// Installed ordinary-grounded movement settings, in original engine units.
#[derive(Debug, Serialize, Deserialize)]
pub struct MovementSettings {
    pub standing_height: i32,
    pub post_collide_height: u32,
    pub slide_range: i32,
    /// Installed [game] slide_speed; absent old caches cannot perform slope motion.
    #[serde(default)]
    pub slide_speed: Option<i32>,
    /// Installed [game] slide_hurt_height (map units) and slide_hurt_zap; absent in old caches.
    #[serde(default)]
    pub slide_hurt: Option<(i32, i32)>,
    /// Installed [cyrus] tap_time, the double-tap window in frames; absent in old caches.
    #[serde(default)]
    pub tap_time: Option<i32>,
    /// Installed [cyrus] sheath_sword_delay (zero initializes to 6), the armed look-and-action hold
    /// in frames before the sword sheathes; absent in old caches.
    #[serde(default)]
    pub sheath_sword_delay: Option<i32>,
    /// Installed COMBAT.INI [misc] cyrus_defend_interval, the frames between the player's defence
    /// stances; absent in old caches.
    #[serde(default)]
    pub defend_interval: Option<i32>,
    /// Effective [game] swim_depth in map units; zero initializes to 40.
    /// Missing older cache/config values remain unknown.
    #[serde(default)]
    pub swim_depth: Option<i32>,
    pub turn_speed: u32,
    pub turn_max_speed: u32,
    pub poly_push_units: u32,
    /// Installed joystick bindings and dead zone; absent in old caches.
    #[serde(default)]
    pub joystick: Option<JoystickSettings>,
}

/// KEYS.INI joystick slot and SYSTEM.INI `[cyrus] joy_tolerance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoystickSettings {
    /// `key_*[1]` input codes in up, down, left, right, a, b, c, d order; zero is unbound.
    pub codes: [u8; 8],
    /// Axis dead zone in the original's gameport loop counts.
    pub tolerance: i32,
}

impl JoystickSettings {
    /// Reads the `key_*[1]` slot of KEYS.INI `[input]`.
    // shortcut: parsed here until redguard-preservation releases `KeysIni::joystick_codes`
    // (d8ee1cc6, unreleased after 0.10.4); then use it and delete this parser.
    pub fn parse(keys_ini: &str, tolerance: i32) -> Result<Self, String> {
        let mut codes = [0; 8];
        let mut section = String::new();
        for line in keys_ini.lines() {
            let line = line.split(';').next().unwrap_or_default().trim();
            if line.starts_with('[') {
                section = line.to_ascii_lowercase();
            } else if let (true, Some((key, value))) = (section == "[input]", line.split_once('='))
            {
                let key = key.trim().to_ascii_lowercase();
                if let Some(index) = ["up", "down", "left", "right", "a", "b", "c", "d"]
                    .iter()
                    .position(|name| key == format!("key_{name}[1]"))
                {
                    codes[index] = value
                        .trim()
                        .parse::<u8>()
                        .ok()
                        .filter(|code| *code <= 139)
                        .ok_or_else(|| format!("KEYS.INI [input] {key}: expected code 0..139"))?;
                }
            }
        }
        Ok(Self { codes, tolerance })
    }
}

#[derive(Serialize, Deserialize)]
pub struct SceneData {
    pub version: u32,
    pub world: u32,
    /// Player model header radii in map units, keyed by uppercase filename stem.
    #[serde(default)]
    pub model_radii: std::collections::BTreeMap<String, f32>,
    pub control_codes: Option<[u8; 15]>,
    pub settings: MovementSettings,
    pub definition: serde_json::Value,
    pub heights: Option<Vec<i16>>,
    pub water: Option<Vec<bool>>,
    pub terrain_textures: Option<Vec<u8>>,
    pub collision: Option<CollisionScene>,
}

impl SceneData {
    /// Validates cache structure before native publication or runtime preparation.
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::Context;
        anyhow::ensure!(
            self.version == 2,
            "world {}: unsupported player schema {}; re-export",
            self.world,
            self.version
        );
        for (model, radius) in &self.model_radii {
            anyhow::ensure!(
                radius.is_finite() && *radius >= 0.0,
                "world {} player model {model}: invalid radius {radius}",
                self.world
            );
        }
        self.collision
            .as_ref()
            .with_context(|| {
                format!(
                    "world {}: missing player collision data; re-export",
                    self.world
                )
            })?
            .validate()
            .with_context(|| format!("world {} collision", self.world))?;
        match (&self.heights, &self.water, &self.terrain_textures) {
            (Some(heights), Some(water), textures) => {
                anyhow::ensure!(
                    heights.len() == 256 * 256 && water.len() == heights.len(),
                    "world {} player terrain: terrain support: expected 256-by-256 height and water grids",
                    self.world
                );
                anyhow::ensure!(
                    textures.as_ref().is_none_or(|t| t.len() == heights.len()),
                    "world {} player terrain: terrain material: expected 256-by-256 texture grid",
                    self.world
                );
            }
            (None, None, None) => {}
            _ => anyhow::bail!("world {}: incomplete player terrain", self.world),
        }
        Ok(())
    }
}
