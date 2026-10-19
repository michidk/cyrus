//! Selected ordinary actor spheres remain separate from body-query working shapes.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::animation::Animations;
use cyrus_collision::actor_overlap::{OverlapSphere, actor_separation};

impl ScriptWorld {
    /// Query two logical actors' current selected overlap shapes. Self never
    /// overlaps; both identities must belong to this world generation. This
    /// performs no admission, weighted response, rollback or contact-state writes.
    pub fn query_ordinary_actor_overlap(
        &self,
        actor: ScriptActorId,
        other: ScriptActorId,
        data: &Animations,
    ) -> Result<i32> {
        self.resolve(actor)?;
        self.resolve(other)?;
        self.require_model_identity(data)?;
        if actor == other {
            return Ok(0);
        }
        actor_separation(
            &self.ordinary_overlap_spheres(actor, data)?,
            &self.ordinary_overlap_spheres(other, data)?,
        )
        .with_context(|| {
            format!(
                "world {} actor pair {}/{} overlap",
                self.world, actor.index, other.index
            )
        })
    }

    /// Prepare selected overlap spheres at the current local runtime origin and
    /// retained actor matrix. Static-model actors use one loaded-model radius.
    /// Vertex-bound RAVC shapes need their animated vertex owner and remain
    /// explicitly unsupported here, rather than falling back to initial bounds.
    pub fn ordinary_overlap_spheres(
        &self,
        id: ScriptActorId,
        data: &Animations,
    ) -> Result<Vec<OverlapSphere>> {
        self.require_model_identity(data)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        ensure!(
            actor.ordinary_collision_role(),
            "world {} instance {index}: ordinary overlap object path unavailable",
            self.world
        );
        ensure!(
            actor.model.prepared,
            "world {} instance {index}: overlap model resources not prepared",
            self.world
        );
        let pose = self.spatial(id)?.local;
        let origin = pose.position.map(|value| (value >> 8) as f32);
        if actor.model.static_radius.is_some() {
            let model = self.model_presentation(id)?.with_context(|| {
                format!(
                    "world {} instance {index}: overlap loaded static model unavailable",
                    self.world
                )
            })?;
            return Ok(vec![OverlapSphere {
                center: origin,
                radius: model.radius,
            }]);
        }
        let definition = data.actor(actor.name())?;
        let count = definition["ravc_count"].as_i64().with_context(|| {
            format!(
                "world {} instance {index}: overlap RAVC count absent",
                self.world
            )
        })?;
        ensure!(
            count == 0,
            "world {} instance {index}: vertex-bound overlap shape unsupported",
            self.world
        );
        self.ordinary_collision_shape(id)?
            .body
            .iter()
            .take(32)
            .map(|sphere| {
                let local = pose
                    .matrix
                    .transform_stored(sphere.center.map(|value| value as f32))?;
                let center = std::array::from_fn(|axis| origin[axis] + local[axis]);
                ensure!(
                    center.iter().all(|value| value.is_finite()),
                    "world {} instance {index}: nonfinite overlap sphere center",
                    self.world
                );
                Ok(OverlapSphere {
                    center,
                    radius: sphere.radius as f32,
                })
            })
            .collect()
    }
}

/// Result of the ordinary overlap stage's weight arithmetic. Distances are Q8;
/// the host owns directions, contact IDs, position writes and the ordered pass.
#[derive(Debug, PartialEq, Eq)]
pub enum OrdinaryOverlapMotion {
    NoContact,
    /// Actor-pair separation below three ends the pass without displacement.
    Touch,
    /// Restore the updating actor's previous position; the other actor stays put.
    Rollback,
    Separate {
        actor: i32,
        other: i32,
    },
}

/// Original ordinary weight split, independent of selected-player overrides.
/// `other_weight` is Some for an actor pair and None for a static placement.
/// Authored weight zero is immovable; nonzero weight uses 256 minus the byte.
/// Separation is the nonnegative map-unit result of the sphere overlap query.
/// Follows preservation's weighted actor overlap response rules.
pub fn ordinary_overlap_motion(
    weight: u8,
    other_weight: Option<u8>,
    separation: i32,
) -> Result<OrdinaryOverlapMotion> {
    ensure!(separation >= 0, "ordinary overlap: negative separation");
    if separation == 0 {
        return Ok(OrdinaryOverlapMotion::NoContact);
    }
    let mobility = |value: u8| {
        if value == 0 {
            0
        } else {
            256 - i32::from(value)
        }
    };
    let actor = mobility(weight);
    if let Some(other_weight) = other_weight {
        if separation < 3 {
            return Ok(OrdinaryOverlapMotion::Touch);
        }
        let other = mobility(other_weight);
        if actor == 0 && other == 0 {
            return Ok(OrdinaryOverlapMotion::Rollback);
        }
        let divisor = if actor == 0 {
            other + 256
        } else if other == 0 {
            actor + 256
        } else {
            actor + other
        };
        let factor = 131072 / divisor;
        Ok(OrdinaryOverlapMotion::Separate {
            actor: overlap_distance(factor, actor, separation),
            other: overlap_distance(factor, other, separation),
        })
    } else if actor == 0 {
        Ok(OrdinaryOverlapMotion::Rollback)
    } else {
        Ok(OrdinaryOverlapMotion::Separate {
            actor: overlap_distance(131072 / actor, actor, separation),
            other: 0,
        })
    }
}

fn overlap_distance(factor: i32, mobility: i32, separation: i32) -> i32 {
    if mobility == 0 {
        0
    } else {
        (factor.wrapping_mul(mobility).wrapping_mul(separation) >> 9).wrapping_add(255)
    }
}

impl ScriptWorld {
    /// Apply one admitted ordinary actor-pair overlap in the original pass order.
    /// Touch tells the caller to end that pass. Positions and contact state are
    /// authoritative logical state; retained model presentations are not refreshed.
    /// Grounded support states skip their current support actor after tiny contact.
    /// Airborne collision and selected-player overrides require separate owners.
    /// Admission follows preservation's [grounded overlap rules](https://github.com/michidk/redguard-preservation/commit/5c686bc).
    pub fn resolve_ordinary_actor_overlap(
        &mut self,
        id: ScriptActorId,
        other: ScriptActorId,
        data: &Animations,
    ) -> Result<OrdinaryOverlapMotion> {
        self.resolve(id)?;
        self.resolve(other)?;
        self.require_model_identity(data)?;
        if id == other
            || self.actors[id.index].attributes[26] != 0
            || self.actors[other.index].attributes[26] != 0
        {
            return Ok(OrdinaryOverlapMotion::NoContact);
        }
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("ordinary overlap control unavailable")?;
        ensure!(
            control.falling_state == 0,
            "world {} instance {}: airborne overlap response unsupported",
            self.world,
            id.index
        );
        ensure!(
            self.actors[other.index].control.is_some(),
            "ordinary other overlap control unavailable"
        );
        let support_bypass = (27..=32).contains(&control.living_state)
            && self.actors[id.index].model.support.actor == Some(other);
        let separation = self.query_ordinary_actor_overlap(id, other, data)?;
        if separation > 0 && self.actors[other.index].attributes[1] & 8 != 0 {
            self.actors[other.index]
                .control
                .as_mut()
                .expect("validated overlap control")
                .body_response_flags |= 4;
        }
        if separation >= 3 && support_bypass {
            return Ok(OrdinaryOverlapMotion::NoContact);
        }
        let response = ordinary_overlap_motion(
            self.actors[id.index].attributes[6],
            Some(self.actors[other.index].attributes[6]),
            separation,
        )?;
        if response == OrdinaryOverlapMotion::NoContact {
            return Ok(response);
        }
        let actor_identifier = self.actors[id.index]
            .runtime_identifier
            .context("ordinary overlap actor identifier unavailable")?
            as u16;
        let other_identifier = self.actors[other.index]
            .runtime_identifier
            .context("ordinary overlap other identifier unavailable")?
            as u16;
        let mut actor_position = self.spatial(id)?.local.position;
        let mut other_position = self.spatial(other)?.local.position;
        match response {
            OrdinaryOverlapMotion::Rollback => {
                actor_position = self.spatial(id)?.previous_local_position
            }
            OrdinaryOverlapMotion::Separate {
                actor,
                other: distance,
            } => {
                let heading = crate::player::motion::displacement_heading(
                    i64::from(other_position[0].wrapping_sub(actor_position[0])),
                    i64::from(other_position[2].wrapping_sub(actor_position[2])),
                );
                if actor != 0 {
                    actor_position = cyrus_world::transform::translate_forward(
                        actor_position,
                        heading.wrapping_add(1024) & 2047,
                        actor,
                    )?;
                }
                if distance != 0 {
                    other_position = cyrus_world::transform::translate_forward(
                        other_position,
                        heading,
                        distance,
                    )?;
                }
            }
            OrdinaryOverlapMotion::Touch => {}
            OrdinaryOverlapMotion::NoContact => unreachable!("handled absent overlap"),
        }
        let spatial = self
            .spatial
            .as_mut()
            .context("ordinary overlap spatial state unavailable")?;
        spatial.actor_mut(id.index)?.local.position = actor_position;
        spatial.actor_mut(other.index)?.local.position = other_position;
        if response == OrdinaryOverlapMotion::Touch {
            let other_control = self.actors[other.index]
                .control
                .as_mut()
                .expect("validated overlap control");
            other_control.body_response_flags |= 4;
            other_control.overlap_contact_identifier = actor_identifier;
        }
        if matches!(
            response,
            OrdinaryOverlapMotion::Touch | OrdinaryOverlapMotion::Separate { .. }
        ) {
            let control = self.actors[id.index]
                .control
                .as_mut()
                .expect("validated overlap control");
            control.body_response_flags |= 4;
            control.overlap_contact_identifier = other_identifier;
        }
        Ok(response)
    }
}

/// Outcome of the retained actor-style pass, preserving repair work across
/// later rollback or misses. Tiny pair contact exits without polygon repair.
#[derive(Debug, PartialEq, Eq)]
pub enum OrdinaryOverlapPass {
    Complete { needs_polygon_repair: bool },
    Touch,
}

impl ScriptWorld {
    /// Resolve an admitted actor-style static placement using the updating
    /// actor's selected spheres. Unlike actor pairs, tiny positive separation
    /// still displaces or rolls back. Retains the pre-script candidate identity.
    pub fn resolve_ordinary_static_overlap(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        data: &Animations,
        prepared: &super::model::OrdinaryCollisionQuery,
        candidate: &cyrus_collision::candidates::ActorCandidate,
    ) -> Result<OrdinaryOverlapMotion> {
        self.require_collision_query(id, prepared, scene, data)?;
        ensure!(
            prepared.candidates.actors.contains(candidate),
            "world {} instance {}: static overlap candidate not retained",
            self.world,
            id.index
        );
        if self.actors[id.index].attributes[26] != 0 {
            return Ok(OrdinaryOverlapMotion::NoContact);
        }
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("ordinary static overlap control unavailable")?;
        ensure!(
            control.falling_state == 0,
            "world {} instance {}: airborne static overlap response unsupported",
            self.world,
            id.index
        );
        let sphere = scene
            .static_collision
            .static_actor_overlap_sphere(candidate)?;
        let separation = cyrus_collision::actor_overlap::static_separation(
            &self.ordinary_overlap_spheres(id, data)?,
            sphere,
        )?;
        let response =
            ordinary_overlap_motion(self.actors[id.index].attributes[6], None, separation)?;
        let position = self.spatial(id)?.local.position;
        let position = match response {
            OrdinaryOverlapMotion::Rollback => self.spatial(id)?.previous_local_position,
            OrdinaryOverlapMotion::Separate { actor, .. } => {
                let heading = crate::player::motion::displacement_heading(
                    (f64::from(sphere.center[0]) - f64::from(position[0] >> 8)).trunc() as i64,
                    (f64::from(sphere.center[2]) - f64::from(position[2] >> 8)).trunc() as i64,
                );
                cyrus_world::transform::translate_forward(
                    position,
                    heading.wrapping_add(1024) & 2047,
                    actor,
                )?
            }
            OrdinaryOverlapMotion::NoContact => return Ok(response),
            OrdinaryOverlapMotion::Touch => {
                unreachable!("static overlaps never take pair touch path")
            }
        };
        self.spatial
            .as_mut()
            .context("ordinary static overlap spatial state unavailable")?
            .actor_mut(id.index)?
            .local
            .position = position;
        if matches!(response, OrdinaryOverlapMotion::Separate { .. }) {
            let control = self.actors[id.index]
                .control
                .as_mut()
                .expect("validated static overlap control");
            control.body_response_flags |= 4;
            control.overlap_contact_identifier = 0;
        }
        Ok(response)
    }

    /// Process retained actor-style candidates in original order. Each query
    /// reads current logical positions after preceding responses. A tiny pair
    /// contact ends the pass before inspecting later candidates. Polygon repair
    /// after weighted displacement belongs to the later response stage.
    pub fn resolve_ordinary_overlaps(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        data: &Animations,
        prepared: &super::model::OrdinaryCollisionQuery,
    ) -> Result<OrdinaryOverlapPass> {
        use cyrus_collision::candidates::{CandidateOwner, ModelInstance};
        self.require_collision_query(id, prepared, scene, data)?;
        if self.actors[id.index].attributes[26] != 0 {
            return Ok(OrdinaryOverlapPass::Complete {
                needs_polygon_repair: false,
            });
        }
        let mut needs_polygon_repair = false;
        for candidate in &prepared.candidates.actors {
            let response = match candidate.owner {
                CandidateOwner::Static(_) => {
                    self.resolve_ordinary_static_overlap(id, scene, data, prepared, candidate)?
                }
                CandidateOwner::Actor(index) => {
                    ensure!(
                        candidate.model.is_none()
                            || candidate.model == Some(ModelInstance::Runtime(index)),
                        "world {} instance {}: overlap actor {index} model identity mismatch",
                        self.world,
                        id.index
                    );
                    self.resolve_ordinary_actor_overlap(id, self.id(index)?, data)?
                }
                CandidateOwner::Transient => anyhow::bail!(
                    "world {} instance {}: transient actor-style overlap owner unavailable",
                    self.world,
                    id.index
                ),
            };
            if response == OrdinaryOverlapMotion::Touch {
                return Ok(OrdinaryOverlapPass::Touch);
            }
            if matches!(response, OrdinaryOverlapMotion::Separate { .. }) {
                needs_polygon_repair = true;
            }
        }
        Ok(OrdinaryOverlapPass::Complete {
            needs_polygon_repair,
        })
    }
}
