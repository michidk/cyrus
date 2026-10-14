//! Ordered static, live-actor and transient candidates from preservation's player rules.
use anyhow::{Result, ensure};
use bevy::math::Vec3;

/// Runtime model-instance identity, distinct from an asset filename shared by actors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelInstance {
    StaticPlacement(usize),
    Runtime(usize),
}

/// Loaded model bounds in original single-precision map coordinates.
#[derive(Clone, Copy, Debug)]
pub struct ModelBounds {
    pub instance: ModelInstance,
    pub position: [f32; 3],
    pub radius: f32,
}

impl ModelBounds {
    fn validate(self) -> Result<()> {
        ensure!(
            self.position.iter().all(|value| value.is_finite())
                && self.radius.is_finite()
                && self.radius >= 0.0,
            "collision model instance {:?}: invalid position or radius",
            self.instance
        );
        Ok(())
    }
}

/// Actor-centered query after the host selects its effective runtime position.
pub struct CandidateQuery {
    origin: Vec3,
    radius: f32,
    own_model: Option<ModelInstance>,
}

impl CandidateQuery {
    /// Position has eight fractional bits. A loaded model supplies twice its
    /// radius, truncated; a missing model supplies the original radius 200.
    pub fn new(position: [i32; 3], model: Option<ModelBounds>) -> Result<Self> {
        let radius = if let Some(model) = model {
            model.validate()?;
            ensure!(
                model.radius < i32::MAX as f32 / 2.0,
                "collision query model {:?}: search radius exceeds signed range",
                model.instance
            );
            (2.0 * model.radius).trunc()
        } else {
            200.0
        };
        Ok(Self {
            origin: Vec3::from_array(position.map(|coordinate| (coordinate >> 8) as f32)),
            radius,
            own_model: model.map(|model| model.instance),
        })
    }

    pub(crate) fn static_query(position: [i64; 3], radius: f32) -> Self {
        Self {
            origin: Vec3::from_array(position.map(|coordinate| (coordinate >> 8) as f32)),
            radius,
            own_model: None,
        }
    }

    fn admits(&self, model: ModelBounds) -> Result<bool> {
        if self.own_model == Some(model.instance) {
            return Ok(false);
        }
        model.validate()?;
        let difference = self.origin - Vec3::from_array(model.position);
        let distance_squared = difference.as_dvec3().length_squared() as f32;
        let combined = self.radius + model.radius;
        Ok(f64::from(distance_squared) <= f64::from(combined).powi(2))
    }
}

/// Static record in the already prepared spatial-list order.
pub struct StaticModel {
    pub placement: usize,
    pub model: Option<ModelBounds>,
    pub collision_flags: u8,
}

/// Live record in ordinary update-list order. The host owns exclusion decisions
/// and supplies the current loaded model rather than a filename approximation.
pub struct LiveActor {
    pub actor: usize,
    pub model: Option<ModelBounds>,
    pub ordinary_excluded: bool,
    pub animation_excluded: bool,
    pub attribute_55: u8,
    pub static_model_path: bool,
    pub collision_flags: u8,
}

/// Transient record in transient-list order. It does not use distance selection.
pub struct TransientModel {
    pub active: bool,
    pub flags: u8,
    pub model: Option<ModelBounds>,
}

/// Owner retained by face contacts and actor-style responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateOwner {
    Static(usize),
    Actor(usize),
    Transient,
}

/// Face-query model and the original support metadata and candidate type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceCandidate {
    pub model: ModelInstance,
    pub owner: CandidateOwner,
    pub collision_flags: u8,
    pub candidate_type: u8,
}

/// Static actor-style models retain their model; live actors retain their owner
/// and obtain their selected runtime shape during response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorCandidate {
    pub model: Option<ModelInstance>,
    pub owner: CandidateOwner,
    pub collision_flags: u8,
}

/// Separate original buffers, each holding at most 64 entries.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CollisionCandidates {
    pub faces: Vec<FaceCandidate>,
    pub actors: Vec<ActorCandidate>,
}

impl CollisionCandidates {
    /// Returns false only when an admitted face entry encounters a saturated
    /// face buffer, ending the static phase while retaining the live phase.
    pub(crate) fn admit_static(
        &mut self,
        query: &CandidateQuery,
        record: &StaticModel,
    ) -> Result<bool> {
        let Some(model) = record.model else {
            return Ok(true);
        };
        if record.collision_flags & 2 != 0 || !query.admits(model)? {
            return Ok(true);
        }
        if record.collision_flags & 1 != 0 {
            if self.actors.len() < 64 {
                self.actors.push(ActorCandidate {
                    model: Some(model.instance),
                    owner: CandidateOwner::Static(record.placement),
                    collision_flags: record.collision_flags,
                });
            }
        } else {
            if self.faces.len() == 64 {
                return Ok(false);
            }
            self.faces.push(FaceCandidate {
                model: model.instance,
                owner: CandidateOwner::Static(record.placement),
                collision_flags: record.collision_flags,
                candidate_type: 0x20,
            });
        }
        Ok(true)
    }

    /// Visit prepared static models, ordinary live actors, then transient models.
    /// A saturated static face list ends only the static phase. Saturation of
    /// both lists at the start of a live iteration ends the complete traversal.
    pub fn build(
        query: &CandidateQuery,
        statics: &[StaticModel],
        live: &[LiveActor],
        transients: &[TransientModel],
    ) -> Result<Self> {
        let mut result = Self::default();
        for record in statics {
            if !result.admit_static(query, record)? {
                break;
            }
        }
        result.append_runtime(query, live, transients)?;
        Ok(result)
    }

    pub(crate) fn append_runtime(
        &mut self,
        query: &CandidateQuery,
        live: &[LiveActor],
        transients: &[TransientModel],
    ) -> Result<()> {
        for record in live {
            if self.faces.len() == 64 && self.actors.len() == 64 {
                return Ok(());
            }
            if record.ordinary_excluded || record.animation_excluded || record.attribute_55 != 0 {
                continue;
            }
            let Some(model) = record.model else {
                continue;
            };
            if !query.admits(model)? {
                continue;
            }
            if record.static_model_path {
                if self.faces.len() < 64 {
                    self.faces.push(FaceCandidate {
                        model: model.instance,
                        owner: CandidateOwner::Actor(record.actor),
                        collision_flags: record.collision_flags,
                        candidate_type: 0x20,
                    });
                }
            } else if self.actors.len() < 64 {
                self.actors.push(ActorCandidate {
                    model: None,
                    owner: CandidateOwner::Actor(record.actor),
                    collision_flags: record.collision_flags,
                });
            }
        }
        for record in transients {
            if !record.active || record.flags & 0x10 == 0 {
                continue;
            }
            let Some(model) = record.model else {
                continue;
            };
            if self.faces.len() == 64 {
                break;
            }
            model.validate()?;
            self.faces.push(FaceCandidate {
                model: model.instance,
                owner: CandidateOwner::Transient,
                collision_flags: 0,
                candidate_type: 0x80,
            });
        }
        Ok(())
    }
}
