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
    pub turn_speed: u32,
    pub turn_max_speed: u32,
    pub poly_push_units: u32,
}

#[derive(Serialize, Deserialize)]
pub struct SceneData {
    pub version: u32,
    pub world: u32,
    /// Player model header radii in map units, keyed by uppercase filename stem.
    #[serde(default)]
    pub model_radii: std::collections::BTreeMap<String, f32>,
    pub keyboard_codes: Option<[u8; 8]>,
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
