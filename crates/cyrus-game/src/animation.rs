//! Retained animation inputs; playback timing and actor activation are not inferred.
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod actor;
mod frame;
mod playback;
pub(crate) mod script;
mod sequence;
pub use actor::ActorAnimation;
pub use frame::{AnimationPose, FrameGeometry, MeshFrameBinding};
pub use playback::{AnimationPlayback, AnimationTick};
pub use script::AnimationScript;
pub use sequence::{AnimationSound, FrameSequence, ModelFrame};

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
    fn require_version(&self) -> anyhow::Result<()> {
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
                    "static_3d" | "static_3dc" | "animated_i16" | "animated_i32"
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
struct AnimationLoader;
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

/// Holds the current world's animation data alive until another world is requested.
#[derive(Resource, Default)]
pub struct AnimationRequest(pub Handle<Animations>);

/// Loads animation assets on native and WASM without inventing a playback clock.
pub struct AnimationPlugin;
impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Animations>()
            .init_asset_loader::<AnimationLoader>()
            .init_resource::<AnimationRequest>()
            .add_message::<AnimationTick>()
            .add_systems(PostUpdate, (playback::advance, frame::apply_poses).chain());
    }
}
