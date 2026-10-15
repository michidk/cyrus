//! Primary mixed body collection retains original owners and stops at sixteen.
use super::StaticCollision;
use crate::{
    actor_models::ActorModelCollision,
    candidates::{CandidateOwner, CollisionCandidates, ModelInstance},
    collision_mesh::{BodyProbe, ModelPose, WorldSphereContact},
    ground::{GroundSupport, SupportSource},
};
use anyhow::{Context, Result, ensure};
use bevy::math::Mat3;
use std::collections::BTreeMap;

/// World integer contact after nearest-even conversion, before motion response.
#[derive(Debug, PartialEq)]
pub struct RuntimeBodyContact {
    pub source: SupportSource,
    pub point: [i32; 3],
    pub normal: [i32; 3],
    pub classification: u8,
}

/// Caller-owned body shape and source-type exclusion. The ordinary response
/// stage supplies its airborne exclusion; geometry cannot infer actor state.
pub struct PrimaryBodyQuery<'a> {
    pub probe: BodyProbe<'a>,
    pub excluded_candidate_type: Option<u8>,
    /// Optional already-admitted terrain contact from the owning ground stage.
    pub terrain: Option<RuntimeBodyContact>,
}

/// Residual model scan has no primary contact cap or terrain contribution.
/// The caller supplies the first working shape with its adjusted common radius.
pub struct ResidualBodyQuery<'a> {
    pub probe: BodyProbe<'a>,
    pub excluded_candidate_type: Option<u8>,
}

impl RuntimeBodyContact {
    fn from_model(source: SupportSource, contact: WorldSphereContact) -> Result<Self> {
        let convert = |values: [f32; 3]| -> Result<[i32; 3]> {
            let mut result = [0; 3];
            for axis in 0..3 {
                let value = values[axis].round_ties_even();
                ensure!(
                    value.is_finite() && (-2147483648.0..2147483648.0).contains(&f64::from(value)),
                    "body contact {source:?} axis {axis}: outside integer range"
                );
                result[axis] = value as i32;
            }
            Ok(result)
        };
        let point = convert(contact.point.to_array())?;
        let normal = convert(contact.normal.to_array().map(|value| value * 256.0))?;
        let classification = GroundSupport {
            height: 0.0,
            normal,
        }
        .classification();
        Ok(Self {
            source,
            point,
            normal,
            classification,
        })
    }
}

impl StaticCollision {
    pub(super) fn runtime_blocks_body(
        &self,
        candidates: &CollisionCandidates,
        actors: &ActorModelCollision,
        poses: &BTreeMap<usize, ModelPose>,
        probe: &BodyProbe<'_>,
        excluded_candidate_type: Option<u8>,
    ) -> Result<bool> {
        Ok(!self
            .runtime_model_body_contacts(
                candidates,
                actors,
                poses,
                probe,
                excluded_candidate_type,
                Some(1),
            )?
            .is_empty())
    }

    /// Collect static/live body contacts in retained candidate and model order.
    /// Returns after the sixteenth model contact, without inspecting later poses
    /// or appending terrain. This is separate from residual separation queries.
    pub fn runtime_primary_body_contacts(
        &self,
        candidates: &CollisionCandidates,
        actors: &ActorModelCollision,
        poses: &BTreeMap<usize, ModelPose>,
        query: PrimaryBodyQuery<'_>,
    ) -> Result<Vec<RuntimeBodyContact>> {
        let mut result = self.runtime_model_body_contacts(
            candidates,
            actors,
            poses,
            &query.probe,
            query.excluded_candidate_type,
            Some(16),
        )?;
        if result.len() == 16 {
            return Ok(result);
        }
        if let Some(terrain) = query.terrain {
            ensure!(
                terrain.source == SupportSource::Terrain,
                "primary body terrain contact has nonterrain owner"
            );
            result.push(terrain);
        }
        Ok(result)
    }

    /// Requery admitted static/live models after primary response and overlap.
    /// Preserves all model-local contacts, with no primary sixteen-contact cap
    /// or terrain addition. Unsupported transient geometry remains explicit.
    pub fn runtime_residual_body_contacts(
        &self,
        candidates: &CollisionCandidates,
        actors: &ActorModelCollision,
        poses: &BTreeMap<usize, ModelPose>,
        query: ResidualBodyQuery<'_>,
    ) -> Result<Vec<RuntimeBodyContact>> {
        self.runtime_model_body_contacts(
            candidates,
            actors,
            poses,
            &query.probe,
            query.excluded_candidate_type,
            None,
        )
    }

    fn runtime_model_body_contacts(
        &self,
        candidates: &CollisionCandidates,
        actors: &ActorModelCollision,
        poses: &BTreeMap<usize, ModelPose>,
        probe: &BodyProbe<'_>,
        excluded_candidate_type: Option<u8>,
        limit: Option<usize>,
    ) -> Result<Vec<RuntimeBodyContact>> {
        ensure!(
            probe.rotation.is_finite(),
            "body query: nonfinite actor rotation"
        );
        ensure!(
            probe.spheres.len() <= 24,
            "body query: more than 24 spheres unsupported"
        );
        for &(center, radius) in probe.spheres {
            ensure!(
                center.is_finite() && radius.is_finite() && radius >= 0.0,
                "body query: invalid sphere"
            );
        }
        let mut result = Vec::new();
        for candidate in &candidates.faces {
            if excluded_candidate_type == Some(candidate.candidate_type) {
                continue;
            }
            let (contacts, owner) = match candidate.owner {
                CandidateOwner::Static(index) => {
                    ensure!(
                        candidate.model == ModelInstance::StaticPlacement(index),
                        "static body candidate {index}: model identity mismatch"
                    );
                    let placement = self
                        .placements
                        .get(index)
                        .context("body candidate outside static placements")?;
                    let model = self
                        .models
                        .get(&placement.model)
                        .context("body candidate static geometry unavailable")?;
                    let pose = ModelPose::new(
                        placement.position.map(|value| value as f32),
                        Mat3::from_cols_array(
                            &placement.rotation.map(|value| value as f32 / 268_435_456.0),
                        ),
                    )?;
                    (
                        model.mesh.world_sphere_contacts(&pose, probe)?,
                        CandidateOwner::Static(index),
                    )
                }
                CandidateOwner::Actor(index) => {
                    ensure!(
                        candidate.model == ModelInstance::Runtime(index),
                        "live body candidate {index}: model identity mismatch"
                    );
                    let pose = poses.get(&index).with_context(|| {
                        format!("live body candidate MPOB {index}: stored model pose unavailable")
                    })?;
                    (
                        actors.body_contacts(index, pose, probe)?,
                        CandidateOwner::Actor(index),
                    )
                }
                CandidateOwner::Transient => anyhow::bail!(
                    "transient body candidate {:?}: geometry owner unavailable",
                    candidate.model
                ),
            };
            for contact in contacts {
                let source = match owner {
                    CandidateOwner::Static(model) => SupportSource::Model {
                        model,
                        face: contact.face,
                    },
                    CandidateOwner::Actor(actor) => SupportSource::Actor {
                        actor,
                        face: contact.face,
                    },
                    CandidateOwner::Transient => {
                        unreachable!("transient body geometry is rejected")
                    }
                };
                result.push(RuntimeBodyContact::from_model(source, contact)?);
                if limit == Some(result.len()) {
                    return Ok(result);
                }
            }
        }
        Ok(result)
    }
}
