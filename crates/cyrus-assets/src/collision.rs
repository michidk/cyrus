//! Portable authored collision inputs; units and ordering follow preservation's player rules.
use anyhow::Context;
use bevy::math::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Original static placements in MPSO order, with models shared by name.
#[derive(Default, Serialize, Deserialize)]
pub struct CollisionScene {
    pub placements: Vec<StaticPlacement>,
    pub models: BTreeMap<String, CollisionModel>,
    /// Missing or ambiguous installed models. They must not become empty collision geometry.
    pub unsupported: BTreeMap<String, String>,
    /// Nonzero SURFACE.INI assignments keyed by packed texture set/image.
    /// Missing keys mean surface zero; a missing table means an old cache.
    #[serde(default)]
    pub texture_surfaces: Option<BTreeMap<u16, u8>>,
}

/// Authored placement coordinates and Q4.28 matrix, before renderer conversion.
#[derive(Clone, Serialize, Deserialize)]
pub struct StaticPlacement {
    pub id: u32,
    pub model: String,
    pub position: [i32; 3],
    pub rotation: [i32; 9],
}

/// Model-local map units, original polygons, and the named ROB segment's flags.
#[derive(Serialize, Deserialize)]
pub struct CollisionModel {
    /// Original model version; missing in older caches.
    #[serde(default)]
    pub version: Option<[u8; 4]>,
    pub radius: f32,
    pub flags: u8,
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub faces: Vec<Vec<usize>>,
    /// Per-face texture set/image, or no texture for a solid-color face.
    /// Absent in older caches; surface-dependent behavior must request re-export.
    #[serde(default)]
    pub textures: Option<Vec<Option<[u16; 2]>>>,
    pub subobjects: Vec<CollisionSubobject>,
}

/// Authored subgroup order and duplicate face references are significant.
#[derive(Clone, Serialize, Deserialize)]
pub struct CollisionSubobject {
    pub center: [f32; 3],
    pub radius: f32,
    /// Authored v4 nodes lack extents and are discarded by runtime preparation.
    pub extents: Option<[f32; 3]>,
    pub faces: Vec<usize>,
}

impl CollisionModel {
    /// Validate authored polygons and normals without constructing runtime query objects.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.radius.is_finite() && self.radius >= 0.0,
            "invalid radius"
        );
        anyhow::ensure!(
            self.faces.len() == self.normals.len(),
            "face/normal count mismatch"
        );
        anyhow::ensure!(
            self.textures
                .as_ref()
                .is_none_or(|textures| textures.len() == self.faces.len()),
            "face/texture count mismatch"
        );
        anyhow::ensure!(
            self.vertices.iter().flatten().all(|v| v.is_finite()),
            "nonfinite vertex"
        );
        for (index, node) in self.subobjects.iter().enumerate() {
            anyhow::ensure!(
                node.center.iter().all(|v| v.is_finite())
                    && node.radius.is_finite()
                    && node.radius >= 0.0
                    && node.extents.is_none_or(|v| v.iter().all(|v| v.is_finite()))
                    && node.faces.iter().all(|&face| face < self.faces.len()),
                "subobject {index}: invalid bounds or face reference"
            );
        }
        for (index, (face, normal)) in self.faces.iter().zip(&self.normals).enumerate() {
            for &vertex in face {
                anyhow::ensure!(
                    vertex < self.vertices.len(),
                    "face {index}: vertex {vertex} outside model"
                );
            }
            anyhow::ensure!(
                face.len() >= 3,
                "face {index}: collision face: expected at least three finite vertices"
            );
            let normal = Vec3::from_array(*normal);
            anyhow::ensure!(
                normal.is_finite() && normal != Vec3::ZERO,
                "face {index}: collision face: expected a finite nonzero normal"
            );
        }
        Ok(())
    }
}

impl CollisionScene {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, model) in &self.models {
            anyhow::ensure!(
                !self.unsupported.contains_key(name),
                "model {name}: both supported and unsupported"
            );
            model.validate().with_context(|| format!("model {name}"))?;
        }
        for placement in &self.placements {
            anyhow::ensure!(
                self.models.contains_key(&placement.model)
                    || self.unsupported.contains_key(&placement.model),
                "placement {}: missing model {}",
                placement.id,
                placement.model
            );
        }
        Ok(())
    }
}
