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
    /// Original header radius in map units, independent of deformed render bounds.
    /// Missing older metadata cannot supply model-distance collision selection.
    #[serde(default)]
    pub radius: Option<f32>,
    /// Base positions in the coordinate system returned by rgpre.
    pub base_positions: Vec<[f32; 3]>,
    /// Base face normals, indexed by face.
    pub base_normals: Vec<[f32; 3]>,
    /// Original polygon vertex indices, before GLB triangulation or vertex duplication.
    pub faces: Vec<Vec<u32>>,
    /// Authored normals by face and polygon corner, shared by every frame; `None`
    /// selects the frame's face normal. Resolved by rgpre's model shading selection.
    #[serde(default)]
    pub corner_normals: Vec<Vec<Option<[f32; 3]>>>,
    /// Original frame encoding, required before interpreting animated coordinates.
    pub encoding: String,
    /// Frame-table order, including frame zero. Values are retained exactly as rgpre decodes them.
    pub positions: Vec<Vec<[f32; 3]>>,
    /// Per-frame face normals; frame zero uses `base_normals`.
    pub normals: Vec<Vec<[f32; 3]>>,
}

/// Installed RAEX contact inputs, separate from an animation model's bounding radius.
/// The original combat constructor retains the signed vertex and shifts the signed
/// size left eight bits. An absent actor-extra record supplies zero for both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatContactProfile {
    /// Signed source-model vertex index. Geometry owners validate it before lookup.
    pub vertex: i16,
    /// Signed contact radius in runtime Q8 units.
    pub radius: i32,
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
                model
                    .radius
                    .is_none_or(|radius| radius.is_finite() && radius >= 0.0),
                "{context}: invalid header radius"
            );
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
            anyhow::ensure!(
                model.corner_normals.len() == model.faces.len()
                    && model
                        .corner_normals
                        .iter()
                        .zip(&model.faces)
                        .all(|(normals, face)| normals.len() == face.len()),
                "{context}: corner normal table absent or inconsistent; re-export the world"
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
                    .chain(model.corner_normals.iter().flatten().flatten())
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

/// Cache filename of a frame-source model drawn with an actor texture set.
/// `None` keeps the model's own textures; the name stays a flat path component.
pub fn frame_source_file(model: &str, texture_set: Option<u16>) -> String {
    match texture_set {
        Some(set) => format!("{model}-{set}.glb"),
        None => format!("{model}.glb"),
    }
}

/// RAEX `texture_id` of an actor metadata record, applied by preservation's
/// [actor texture sets](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/RGM.md#actor-texture-sets).
/// An absent RAEX record or zero keeps the models' own textures.
pub fn actor_texture_set(actor: &serde_json::Value) -> anyhow::Result<Option<u16>> {
    let value = &actor["raex"]["texture_id"];
    if value.is_null() {
        return Ok(None);
    }
    let set = value
        .as_u64()
        .and_then(|set| u16::try_from(set).ok())
        .with_context(|| {
            format!(
                "actor {}: invalid RAEX texture_id {value}",
                actor["script_name"]
            )
        })?;
    Ok((set != 0).then_some(set))
}

/// A resolved RAAN model/frame pair; this is not an attachment vertex selection.
#[derive(Debug, PartialEq, Eq)]
pub struct ModelFrame {
    /// Uppercase model key in the world's validated animation asset.
    pub model: String,
    /// Zero-based model frame index.
    pub frame: usize,
}

pub(crate) fn model_key(path: &str) -> String {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
    filename
        .rsplit_once('.')
        .map_or(filename, |(stem, _)| stem)
        .to_ascii_uppercase()
}

impl Animations {
    /// Resolves a placed object's frame using its actor table or its own MPOB model.
    /// The index is the original MPOB order, not an object ID or render-node index.
    pub fn resolve_map_frame(&self, index: usize, reference: u32) -> anyhow::Result<ModelFrame> {
        let context = format!(
            "world {} MPOB {index} frame reference {reference}",
            self.world
        );
        let placement = self.metadata["mpob_objects"]
            .as_array()
            .and_then(|objects| objects.get(index))
            .with_context(|| format!("{context}: placement absent; re-export the world"))?;
        let name = placement["script_name"]
            .as_str()
            .with_context(|| format!("{context}: actor name absent"))?;
        let actor = self.actor(name)?;
        if actor["raan_entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
        {
            return self.resolve_frame(name, reference).with_context(|| context);
        }
        anyhow::ensure!(
            placement["object_type"] == 1,
            "{context}: separate object frame source unsupported"
        );
        let count = placement["is_static"]
            .as_u64()
            .with_context(|| format!("{context}: placed frame count absent"))?;
        anyhow::ensure!(
            count <= u64::from(u8::MAX),
            "{context}: invalid placed frame count {count}"
        );
        anyhow::ensure!(
            count != 1,
            "{context}: static placement has no animation frame table"
        );
        anyhow::ensure!(
            u64::from(reference) < count,
            "{context}: outside placed frame table"
        );
        let model = model_key(
            placement["model_name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .with_context(|| format!("{context}: placed model absent"))?,
        );
        let data = self
            .models
            .get(&model)
            .with_context(|| format!("{context}: model {model} unavailable"))?;
        let frame = usize::try_from(reference)?;
        anyhow::ensure!(
            frame < data.positions.len(),
            "{context}: model {model} has no frame {frame}"
        );
        Ok(ModelFrame { model, frame })
    }

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
                let model = model_key(path);
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

    /// Read original contact inputs for either a map actor or standalone player.
    /// Missing RAEX is the original zero profile; a present malformed record is an
    /// error. Follows [preservation's combat-construction rules].
    ///
    /// [preservation's combat-construction rules]: https://github.com/michidk/redguard-preservation/commit/a4d6495
    pub fn combat_contact_profile(&self, name: &str) -> anyhow::Result<CombatContactProfile> {
        let actor = self.actor(name)?;
        let Some(extra) = actor.get("raex").filter(|value| !value.is_null()) else {
            return Ok(CombatContactProfile::default());
        };
        let field = |key: &str| -> anyhow::Result<i16> {
            let value = extra[key].as_i64().with_context(|| {
                format!(
                    "world {} actor {name}: RAEX {key} missing or not an integer",
                    self.world
                )
            })?;
            i16::try_from(value).with_context(|| {
                format!(
                    "world {} actor {name}: RAEX {key} {value} outside signed 16-bit range",
                    self.world
                )
            })
        };
        Ok(CombatContactProfile {
            vertex: field("v_vertex")?,
            radius: i32::from(field("v_size")?) << 8,
        })
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
