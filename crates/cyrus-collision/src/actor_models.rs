//! Prepared live static-model geometry; mutable pose and collision flags remain host-owned.
use crate::collision_mesh::{CollisionMesh, ModelPose, WorldSegmentContact};
use anyhow::{Context, Result};
use cyrus_assets::collision::ActorCollisionModels;
use std::collections::BTreeMap;

struct Model {
    mesh: CollisionMesh,
    radius: f32,
    version: Option<[u8; 4]>,
    textures: Option<Vec<Option<[u16; 2]>>>,
}

/// Initial MPOB model bindings and immutable query geometry. A missing entry is
/// unsupported, distinct from a successfully prepared scene with no static actors.
pub struct ActorModelCollision {
    models: BTreeMap<String, Model>,
    instances: BTreeMap<usize, String>,
    unsupported: BTreeMap<usize, String>,
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
            models,
            instances: source.instances.clone(),
            unsupported: source.unsupported.clone(),
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

    /// Original header radius in map units, before any actor shape radius scaling.
    pub fn radius(&self, actor: usize) -> Result<f32> {
        Ok(self.model(actor)?.radius)
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
