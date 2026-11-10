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
    /// The model header's bounding radius.
    pub radius: f32,
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
    /// Stored as base64 text: a JSON array of numbers would take four times the space.
    #[serde(with = "base64_bytes")]
    pub indices: Vec<u8>,
}

mod base64_bytes {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn serialize<S: serde::Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
                value | u32::from(*byte) << (16 - 8 * index)
            });
            for position in 0..=chunk.len() {
                text.push(char::from(
                    ALPHABET[(value >> (18 - 6 * position)) as usize & 63],
                ));
            }
        }
        serializer.serialize_str(&text)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let text = <&str as serde::Deserialize>::deserialize(deserializer)?;
        let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
        for chunk in text.as_bytes().chunks(4) {
            let mut value = 0u32;
            for (position, character) in chunk.iter().enumerate() {
                let sextet = ALPHABET
                    .iter()
                    .position(|candidate| candidate == character)
                    .ok_or_else(|| serde::de::Error::custom("invalid base64 character"))?;
                value |= (sextet as u32) << (18 - 6 * position);
            }
            if chunk.len() < 2 {
                return Err(serde::de::Error::custom("truncated base64"));
            }
            for index in 0..chunk.len() - 1 {
                bytes.push((value >> (16 - 8 * index)) as u8);
            }
        }
        Ok(bytes)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_indices_round_trip_through_base64_text() {
        for length in 0..9 {
            let texture = ExamineTexture {
                width: 1,
                height: 1,
                indices: (0..length).map(|n| (n * 37 + 5) as u8).collect(),
            };
            let json = serde_json::to_string(&texture).unwrap();
            let back: ExamineTexture = serde_json::from_str(&json).unwrap();
            assert_eq!(back.indices, texture.indices, "{json}");
        }
        let json = serde_json::to_string(&ExamineTexture {
            width: 1,
            height: 1,
            indices: vec![0, 1, 2],
        })
        .unwrap();
        assert!(json.contains("\"AAEC\""), "{json}");
    }
}
