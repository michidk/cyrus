//! Inventory models for the examine view, exported once and shaded at view time with the loaded
//! world's palette and lighting (preservation `engine/hud.md` examine view).
use crate::menu::MenuCorner;
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Installed `INVENTRY.ROB` models by uppercase stem, with their shared texture images.
#[derive(Asset, TypePath, Serialize, Deserialize, Default)]
pub struct ExamineArtwork {
    pub models: BTreeMap<String, ExamineModel>,
    /// Palette-indexed images; an index of zero is transparent.
    pub textures: Vec<ExamineTexture>,
}

/// One model's first-frame geometry, in original model units.
#[derive(Serialize, Deserialize)]
pub struct ExamineModel {
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<ExamineFace>,
}

#[derive(Serialize, Deserialize)]
pub struct ExamineFace {
    pub corners: Vec<MenuCorner>,
    /// Index into [`ExamineArtwork::textures`].
    pub texture: Option<usize>,
    /// Palette index of a solid-color face.
    pub solid: Option<u8>,
    /// The normal the engine lights at each corner.
    pub normals: Vec<[f32; 3]>,
    /// Lighting brightness byte `(tex_hi * 4) & 255`.
    pub brightness: u8,
}

#[derive(Serialize, Deserialize)]
pub struct ExamineTexture {
    pub width: u16,
    pub height: u16,
    pub indices: Vec<u8>,
}

impl ExamineArtwork {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, model) in &self.models {
            for face in &model.faces {
                anyhow::ensure!(
                    face.corners.len() >= 3
                        && face.normals.len() == face.corners.len()
                        && face.corners.iter().all(|c| c.vertex < model.vertices.len())
                        && face.texture.is_none_or(|id| id < self.textures.len())
                        && (face.texture.is_some() || face.solid.is_some()),
                    "examine: model {name} has an invalid face"
                );
            }
        }
        anyhow::ensure!(
            self.textures.iter().all(|t| t.width > 0
                && t.height > 0
                && t.indices.len() == usize::from(t.width) * usize::from(t.height)),
            "examine: invalid texture dimensions"
        );
        Ok(())
    }
}
