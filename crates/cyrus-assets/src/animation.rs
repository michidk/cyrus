//! Validated animation cache records and source-frame lookup.
use anyhow::Context;
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Per-world animation metadata and decoded geometry from preservation's parsers.
#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct Animations {
    /// Version 2 uses preservation's twenty-bit file frame references.
    #[serde(default)]
    pub version: u32,
    /// WORLD.INI identifier, retained even when several worlds share a map.
    pub world: u32,
    /// Preservation runtime metadata, including actor groups, RAAN references and MPOB IDs.
    pub metadata: serde_json::Value,
    /// Original animation and script payloads, including fields not decoded in metadata.
    pub sections: BTreeMap<String, Vec<u8>>,
    /// Models keyed by uppercase filename stem; coordinates retain parser units and axes.
    pub models: BTreeMap<String, AnimationModel>,
    /// Referenced models that could not be decoded, with contextual reasons.
    pub unsupported: BTreeMap<String, String>,
}

/// Frame geometry in source vertex/face order, without guessed interpolation or timing.
#[derive(Serialize, Deserialize)]
pub struct AnimationModel {
    /// Base positions in the coordinate system returned by rgpre.
    pub base_positions: Vec<[f32; 3]>,
    /// Base face normals, indexed by face.
    pub base_normals: Vec<[f32; 3]>,
    /// Original polygon vertex indices, before GLB triangulation or vertex duplication.
    pub faces: Vec<Vec<u32>>,
    /// Original frame encoding, required before interpreting animated coordinates.
    pub encoding: String,
    /// Frame-table order, including frame zero. Values are retained exactly as rgpre decodes them.
    pub positions: Vec<Vec<[f32; 3]>>,
    /// Per-frame face normals; frame zero uses `base_normals`.
    pub normals: Vec<Vec<[f32; 3]>>,
}

impl Animations {
    pub fn require_version(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 2,
            "world {}: animation schema {} unsupported; re-export the world",
            self.world,
            self.version
        );
        Ok(())
    }

    /// Rejects inconsistent geometry before exposing an exported asset to the runtime.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.require_version()?;
        anyhow::ensure!(
            self.metadata["actors"].is_array(),
            "world {}: missing animation actors",
            self.world
        );
        for (name, model) in &self.models {
            let context = format!("world {} animation {name}", self.world);
            anyhow::ensure!(
                matches!(
                    model.encoding.as_str(),
                    "static_3d"
                        | "static_3dc"
                        | "animated_i16"
                        | "animated_i16_absolute"
                        | "animated_i32"
                ),
                "{context}: unsupported frame encoding {}",
                model.encoding
            );
            anyhow::ensure!(
                !model.positions.is_empty() && model.positions.len() == model.normals.len(),
                "{context}: inconsistent frame tables"
            );
            anyhow::ensure!(
                model.base_normals.len() == model.faces.len(),
                "{context}: inconsistent base normals"
            );
            anyhow::ensure!(
                model
                    .faces
                    .iter()
                    .flatten()
                    .all(|&i| (i as usize) < model.base_positions.len()),
                "{context}: face vertex out of range"
            );
            for (index, positions) in model.positions.iter().enumerate() {
                anyhow::ensure!(
                    positions.len() == model.base_positions.len(),
                    "{context} frame {index}: vertex count mismatch"
                );
                anyhow::ensure!(
                    (index == 0 && model.normals[index].is_empty())
                        || model.normals[index].len() == model.faces.len(),
                    "{context} frame {index}: normal count mismatch"
                );
            }
            anyhow::ensure!(
                model
                    .base_positions
                    .iter()
                    .chain(&model.base_normals)
                    .chain(model.positions.iter().flatten())
                    .chain(model.normals.iter().flatten())
                    .flatten()
                    .all(|v| v.is_finite()),
                "{context}: nonfinite geometry"
            );
        }
        Ok(())
    }
}

#[derive(Default, TypePath)]
pub struct AnimationLoader;
impl AssetLoader for AnimationLoader {
    type Asset = Animations;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> anyhow::Result<Animations> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let animations: Animations = serde_json::from_slice(&bytes)?;
        animations.validate()?;
        Ok(animations)
    }
    fn extensions(&self) -> &[&str] {
        &["animations"]
    }
}

/// A resolved RAAN model/frame pair; this is not an attachment vertex selection.
#[derive(Debug, PartialEq, Eq)]
pub struct ModelFrame {
    /// Uppercase model key in the world's validated animation asset.
    pub model: String,
    /// Zero-based model frame index.
    pub frame: usize,
}

impl Animations {
    /// Resolves a file ShowFrame reference across RAAN entries in their original order.
    pub fn resolve_frame(&self, actor: &str, reference: u32) -> anyhow::Result<ModelFrame> {
        let context = format!(
            "world {} actor {actor} frame reference {reference}",
            self.world
        );
        let actor = self.actor(actor).with_context(|| context.clone())?;
        let entries = actor["raan_entries"]
            .as_array()
            .with_context(|| format!("{context}: RAAN entries absent"))?;
        let mut remaining = u64::from(reference);
        for entry in entries {
            let count = entry["frame_count"]
                .as_u64()
                .with_context(|| format!("{context}: RAAN frame count absent"))?;
            if remaining < count {
                let path = entry["file_path"]
                    .as_str()
                    .with_context(|| format!("{context}: RAAN model path absent"))?;
                let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
                let model = filename
                    .rsplit_once('.')
                    .map_or(filename, |(stem, _)| stem)
                    .to_ascii_uppercase();
                let frame = usize::try_from(remaining)?;
                let data = self
                    .models
                    .get(&model)
                    .with_context(|| format!("{context}: model {model} unavailable"))?;
                anyhow::ensure!(
                    frame < data.positions.len(),
                    "{context}: model {model} has no frame {frame}"
                );
                return Ok(ModelFrame { model, frame });
            }
            remaining -= count;
        }
        anyhow::bail!("{context}: outside RAAN frame table")
    }

    pub fn actor(&self, name: &str) -> anyhow::Result<&serde_json::Value> {
        self.metadata["actors"]
            .as_array()
            .with_context(|| format!("world {} actor {name}: actor table absent", self.world))?
            .iter()
            .find(|actor| {
                actor["script_name"]
                    .as_str()
                    .is_some_and(|script| script.eq_ignore_ascii_case(name))
            })
            .with_context(|| format!("world {} actor {name}: absent", self.world))
    }
}
