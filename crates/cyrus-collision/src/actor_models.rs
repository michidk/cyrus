//! Prepared live static-model geometry; mutable pose and collision flags remain host-owned.
use crate::collision_mesh::{CollisionMesh, ModelPose, WorldSegmentContact};
use anyhow::{Context, Result};
use cyrus_assets::collision::ActorCollisionModels;
use std::collections::BTreeMap;

struct Model {
    vertices: Vec<[f32; 3]>,
    mesh: CollisionMesh,
    radius: f32,
    version: Option<[u8; 4]>,
    textures: Option<Vec<Option<[u16; 2]>>>,
}

/// Initial MPOB model bindings and immutable query geometry. A missing entry is
/// unsupported, distinct from a successfully prepared scene with no static actors.
pub struct ActorModelCollision {
    object_count: usize,
    models: BTreeMap<String, Model>,
    instances: BTreeMap<usize, String>,
    unsupported: BTreeMap<usize, String>,
    collision_flags: BTreeMap<usize, u8>,
}

impl ActorModelCollision {
    pub fn new(source: &ActorCollisionModels) -> Result<Self> {
        source.validate()?;
        let models = source
            .models
            .iter()
            .map(|(name, model)| {
                Ok((
                    name.clone(),
                    Model {
                        vertices: model.vertices.clone(),
                        mesh: CollisionMesh::new(model)
                            .with_context(|| format!("live model {name}"))?,
                        radius: model.radius,
                        version: model.version,
                        textures: model.textures.clone(),
                    },
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            object_count: source.object_count,
            models,
            instances: source.instances.clone(),
            unsupported: source.unsupported.clone(),
            collision_flags: source.collision_flags.clone(),
        })
    }

    fn model(&self, actor: usize) -> Result<&Model> {
        if let Some(reason) = self.unsupported.get(&actor) {
            anyhow::bail!("MPOB {actor}: {reason}");
        }
        let name = self
            .instances
            .get(&actor)
            .with_context(|| format!("MPOB {actor}: no prepared live static model"))?;
        self.models
            .get(name)
            .with_context(|| format!("MPOB {actor} model {name}: prepared geometry absent"))
    }

    /// Whether any initial actor carries a prepared model.
    pub fn has_models(&self) -> bool {
        !self.instances.is_empty()
    }

    /// Original header radius in map units, before any actor shape radius scaling.
    pub fn radius(&self, actor: usize) -> Result<f32> {
        Ok(self.model(actor)?.radius)
    }

    /// Source-order static vertex in original model units, before actor transforms.
    pub fn vertex(&self, actor: usize, vertex: usize) -> Result<[f32; 3]> {
        let model = self.model(actor)?;
        model.vertices.get(vertex).copied().with_context(|| {
            format!(
                "MPOB {actor} vertex {vertex}: outside {} source vertices",
                model.vertices.len()
            )
        })
    }

    /// Initial static bounds use original vertices, not polygon bounds or rendered meshes.
    pub fn initial_bounds(
        &self,
        actor: usize,
        system_scale: i32,
        radius_scale: i32,
    ) -> Result<crate::actor_shape::ActorBounds> {
        crate::actor_shape::ActorBounds::from_vertices(
            &self.model(actor)?.vertices,
            system_scale,
            radius_scale,
        )
        .with_context(|| format!("MPOB {actor} initial static bounds"))
    }

    /// Loaded initial static resource, distinguishing a model-less actor from unsupported data.
    pub fn loaded_radius(&self, actor: usize) -> Result<Option<f32>> {
        anyhow::ensure!(
            actor < self.object_count,
            "MPOB {actor}: outside object table"
        );
        if self.unsupported.contains_key(&actor) || self.instances.contains_key(&actor) {
            self.radius(actor).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Initial ordinary actor metadata; animation/model-less actors also have a known byte.
    pub fn collision_flags(&self, actor: usize) -> Result<u8> {
        if let Some(reason) = self.unsupported.get(&actor) {
            anyhow::bail!("MPOB {actor}: {reason}");
        }
        self.collision_flags
            .get(&actor)
            .copied()
            .with_context(|| format!("MPOB {actor}: collision metadata absent; re-export"))
    }

    /// Query the caller's current stored model pose; never infer it from renderer state.
    pub fn segment_contacts(
        &self,
        actor: usize,
        pose: &ModelPose,
        start: [f32; 3],
        end: [f32; 3],
        sidedness: bool,
    ) -> Result<Vec<WorldSegmentContact>> {
        self.model(actor)?
            .mesh
            .world_segment_contacts(pose, start, end, sidedness)
            .with_context(|| format!("MPOB {actor} live model segment"))
    }

    /// Body geometry uses the host's stored model pose and original actor spheres.
    pub fn body_contacts(
        &self,
        actor: usize,
        pose: &ModelPose,
        probe: &crate::collision_mesh::BodyProbe<'_>,
    ) -> Result<Vec<crate::collision_mesh::WorldSphereContact>> {
        self.model(actor)?
            .mesh
            .world_sphere_contacts(pose, probe)
            .with_context(|| format!("MPOB {actor} live model body query"))
    }

    /// Installed face material; mutable actor surface assignment is a separate stage.
    pub fn face_surface(
        &self,
        actor: usize,
        face: usize,
        assignments: Option<&BTreeMap<u16, u8>>,
    ) -> Result<u8> {
        let model = self.model(actor)?;
        super::collision_mesh::face_surface(
            model.version,
            model.textures.as_deref(),
            face,
            assignments,
        )
        .with_context(|| format!("MPOB {actor} live model face {face}"))
    }
}
