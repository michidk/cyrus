//! Mesh frame selection from rgpre's GLB source mapping and decoded model frames.
use super::{AnimationModel, AnimationRequest, Animations};
use anyhow::Context;
use bevy::camera::primitives::MeshAabb;
use bevy::gltf::GltfExtras;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{
    AssetId, Assets, ChildOf, Commands, Component, Entity, Handle, Mesh, Mesh3d, Name, Query, Res,
    ResMut, error,
};
use serde::{Deserialize, Serialize};

/// An explicit model frame on a GLB model node. This component supplies no clock or activation.
#[derive(Component, Clone, PartialEq, Eq)]
pub struct AnimationPose {
    /// Uppercase RAAN model key; placed GLB node names identify instances, not models.
    pub model: String,
    /// Frame-table index, including base frame zero.
    pub frame: usize,
}

/// Versioned rgpre mapping in GLB primitive vertex order. Each pair is vertex and face.
#[derive(Deserialize, Serialize)]
pub struct MeshFrameBinding {
    /// Mapping schema version; only version one is supported.
    pub version: u32,
    /// Original vertex and face indices for every exported primitive vertex.
    pub source_vertices: Vec<[u32; 2]>,
}

/// One selected frame in GLB coordinates and primitive vertex order.
pub struct FrameGeometry {
    /// Positions use rgpre's export axes and twenty engine units per GLB unit.
    pub positions: Vec<[f32; 3]>,
    /// Decoded face normals use the export axes, without invented interpolation.
    pub normals: Vec<[f32; 3]>,
}

impl AnimationModel {
    /// Projects an explicit frame through an exporter mapping; rejects unsupported encodings.
    pub fn frame_geometry(
        &self,
        frame: usize,
        binding: &MeshFrameBinding,
    ) -> anyhow::Result<FrameGeometry> {
        anyhow::ensure!(
            binding.version == 1,
            "mesh frame mapping version {} unsupported",
            binding.version
        );
        anyhow::ensure!(
            frame == 0
                || matches!(
                    self.encoding.as_str(),
                    "animated_i32" | "animated_i16_absolute"
                ),
            "frame {frame}: playback for {} is unsupported",
            self.encoding
        );
        let positions = self
            .positions
            .get(frame)
            .with_context(|| format!("frame {frame}: outside model frame table"))?;
        let normals = if frame == 0 {
            &self.base_normals
        } else {
            self.normals
                .get(frame)
                .with_context(|| format!("frame {frame}: missing normal table"))?
        };
        let mut output = FrameGeometry {
            positions: Vec::with_capacity(binding.source_vertices.len()),
            normals: Vec::with_capacity(binding.source_vertices.len()),
        };
        for &[vertex, face] in &binding.source_vertices {
            let polygon = self
                .faces
                .get(face as usize)
                .with_context(|| format!("frame {frame}: source face {face} out of range"))?;
            anyhow::ensure!(
                polygon.contains(&vertex),
                "frame {frame}: vertex {vertex} does not belong to face {face}"
            );
            let &[x, y, z] = positions
                .get(vertex as usize)
                .with_context(|| format!("frame {frame}: source vertex {vertex} out of range"))?;
            let &[nx, ny, nz] = normals
                .get(face as usize)
                .with_context(|| format!("frame {frame}: normal for face {face} absent"))?;
            output.positions.push([-x / 20.0, -y / 20.0, z / 20.0]);
            output.normals.push([-nx, -ny, nz]);
        }
        Ok(output)
    }
}

#[derive(Component)]
pub(super) struct BoundFrameMesh {
    source: AssetId<Animations>,
    node: Entity,
    name: String,
    mapping: MeshFrameBinding,
    original: Handle<Mesh>,
    frame: usize,
}

#[derive(Component)]
pub(super) struct RejectedPose {
    source: AssetId<Animations>,
    model: String,
    frame: usize,
}

#[derive(Deserialize)]
struct PrimitiveExtras {
    rgpre_geometry: MeshFrameBinding,
}

// Bevy supplies independent scene, animation, mesh, and hierarchy resources.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn apply_poses(
    mut commands: Commands,
    request: Res<AnimationRequest>,
    animations: Res<Assets<Animations>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    mut primitives: Query<(
        Entity,
        &ChildOf,
        &mut Mesh3d,
        Option<&GltfExtras>,
        Option<&mut BoundFrameMesh>,
        Option<&RejectedPose>,
    )>,
    nodes: Query<(&Name, &AnimationPose)>,
) {
    let Some(mut meshes) = meshes else {
        return;
    };
    let animation = animations.get(&request.0);
    for (entity, parent, mut mesh, extras, bound, rejected) in &mut primitives {
        if let Some(mut bound) = bound {
            if bound.source != request.0.id()
                || nodes.get(bound.node).is_err()
                || nodes
                    .get(bound.node)
                    .is_ok_and(|(_, pose)| pose.model != bound.name)
            {
                mesh.0 = bound.original.clone();
                if let Some(bounds) = meshes.get(&mesh.0).and_then(Mesh::get_aabb) {
                    commands.entity(entity).insert(bounds);
                }
                commands.entity(entity).remove::<BoundFrameMesh>();
                continue;
            }
            let Ok((_, pose)) = nodes.get(bound.node) else {
                continue;
            };
            if bound.frame == pose.frame {
                continue;
            }
            let Some(animation) = animation else {
                continue;
            };
            let result = animation
                .models
                .get(&bound.name)
                .context("model missing from current animation asset")
                .and_then(|model| model.frame_geometry(pose.frame, &bound.mapping));
            match result {
                Ok(frame) => {
                    if let Some(mut private) = meshes.get_mut(&mesh.0) {
                        private.insert_attribute(Mesh::ATTRIBUTE_POSITION, frame.positions);
                        private.insert_attribute(Mesh::ATTRIBUTE_NORMAL, frame.normals);
                        private.final_aabb = None;
                        if let Some(bounds) = private.get_aabb() {
                            commands.entity(entity).insert(bounds);
                        }
                        bound.frame = pose.frame;
                    }
                }
                Err(error) => {
                    error!(
                        "world {} animation {}: {error:#}",
                        animation.world, bound.name
                    );
                    bound.frame = pose.frame;
                }
            }
            continue;
        }
        let Ok((name, pose)) = nodes.get(parent.parent()) else {
            continue;
        };
        if rejected.is_some_and(|rejected| {
            rejected.source == request.0.id()
                && rejected.model == pose.model
                && rejected.frame == pose.frame
        }) {
            continue;
        }
        let Some(animation) = animation else {
            continue;
        };
        let Some(original) = meshes.get(&mesh.0) else {
            continue;
        };
        let result = (|| -> anyhow::Result<(MeshFrameBinding, FrameGeometry)> {
            let model = animation
                .models
                .get(&pose.model)
                .with_context(|| format!("requested model {} unavailable", pose.model))?;
            let extras: PrimitiveExtras = serde_json::from_str(
                &extras
                    .context("GLB source mapping absent; re-export the world")?
                    .value,
            )?;
            let base = model.frame_geometry(0, &extras.rgpre_geometry)?;
            let VertexAttributeValues::Float32x3(positions) =
                original.try_attribute(Mesh::ATTRIBUTE_POSITION)?
            else {
                anyhow::bail!("GLB positions are not Float32x3");
            };
            anyhow::ensure!(
                *positions == base.positions,
                "GLB base geometry does not match animation model; re-export the world"
            );
            let frame = model.frame_geometry(pose.frame, &extras.rgpre_geometry)?;
            Ok((extras.rgpre_geometry, frame))
        })();
        match result {
            Ok((mapping, frame)) => {
                let mut private = original.clone();
                private.insert_attribute(Mesh::ATTRIBUTE_POSITION, frame.positions);
                private.insert_attribute(Mesh::ATTRIBUTE_NORMAL, frame.normals);
                private.final_aabb = None;
                if let Some(bounds) = private.get_aabb() {
                    commands.entity(entity).insert(bounds);
                }
                let original = mesh.0.clone();
                mesh.0 = meshes.add(private);
                commands.entity(entity).remove::<RejectedPose>();
                commands.entity(entity).insert(BoundFrameMesh {
                    source: request.0.id(),
                    node: parent.parent(),
                    name: pose.model.clone(),
                    mapping,
                    original,
                    frame: pose.frame,
                });
            }
            Err(error) => {
                error!("world {} animation {}: {error:#}", animation.world, name);
                commands.entity(entity).insert(RejectedPose {
                    source: request.0.id(),
                    model: pose.model.clone(),
                    frame: pose.frame,
                });
            }
        }
    }
}
