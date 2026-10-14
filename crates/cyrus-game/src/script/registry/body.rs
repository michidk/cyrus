//! Ordinary primary body queries use the actor's initial shape and current pose.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use bevy::math::{Mat3, Vec3};
use cyrus_collision::{
    collision_mesh::BodyProbe,
    collision_world::{PrimaryBodyQuery, RuntimeBodyContact},
};

/// Caller-admitted primary body inputs. Exclusion and terrain admission belong
/// to the response stage; this query supplies neither movement nor rollback.
pub struct OrdinaryBodyQuery<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub prepared: &'a super::model::OrdinaryCollisionQuery,
    pub sidedness: bool,
    pub excluded_candidate_type: Option<u8>,
    pub terrain: Option<RuntimeBodyContact>,
}

/// Explicit residual inputs after primary response and overlap. The update
/// snapshot supplies the first working record's angles. No terrain is appended.
pub struct OrdinaryResidualQuery<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub prepared: &'a super::model::OrdinaryCollisionQuery,
}

impl ScriptWorld {
    /// Query the first working shape at the current effective origin, retaining
    /// the update's copied angles and applying state-dependent common radii.
    /// The initial actor shape remains unchanged. Reuses pre-script candidate
    /// admission, while reading each admitted model's stored presentation pose.
    pub fn query_ordinary_residual_body(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryResidualQuery<'_>,
    ) -> Result<Vec<RuntimeBodyContact>> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("ordinary residual control unavailable")?;
        let shape = self.ordinary_collision_shape(id)?;
        let initial = shape
            .body
            .first()
            .context("ordinary residual first sphere unavailable")?
            .radius;
        let radius = if control.living_state == 15 {
            initial >> 1
        } else if (control.falling_state != 0 && control.motion_direction == 3)
            || control.falling_state & 127 != 0
            || control.queued_motion & 127 != 0
        {
            initial.wrapping_mul(200) >> 8
        } else {
            initial
        };
        let spheres = shape
            .body
            .iter()
            .map(|sphere| {
                (
                    Vec3::from_array(sphere.center.map(|value| value as f32)),
                    radius as f32,
                )
            })
            .collect::<Vec<_>>();
        let angles = query.prepared.working_angles;
        let rows = cyrus_world::transform::Matrix::collision_orientation(angles)
            .0
            .map(Vec3::from_array);
        query.scene.static_collision.runtime_residual_body_contacts(
            &query.prepared.candidates,
            query
                .scene
                .actor_collision
                .as_ref()
                .context("ordinary residual live geometry unavailable; re-export world")?,
            &poses,
            cyrus_collision::collision_world::ResidualBodyQuery {
                probe: BodyProbe {
                    position: self.spatial(id)?.query_pose()?.position.map(i64::from),
                    rotation: Mat3::from_cols(rows[0], rows[1], rows[2]).transpose(),
                    spheres: &spheres,
                    sidedness: false,
                },
                excluded_candidate_type: (control.queued_motion == 24).then_some(128),
            },
        )
    }

    /// Apply residual normals after the primary and overlap stages. Uses the
    /// post-response horizontal distance with the installed push minimum.
    /// A nonempty contact set marks bit four even when its horizontal normal
    /// cancels. This does not perform later support following or axis restoration.
    pub fn resolve_ordinary_residual_contacts(
        &mut self,
        id: ScriptActorId,
        contacts: &[RuntimeBodyContact],
        settings: &crate::player::data::MovementSettings,
    ) -> Result<()> {
        use crate::player::contact;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        ensure!(
            actor
                .placement()
                .is_some_and(|placement| placement.object_type == 1)
                && actor.control.is_some(),
            "world {} instance {index}: ordinary residual response object path unavailable",
            self.world
        );
        ensure!(
            contacts.iter().all(|body| !matches!(
                body.source,
                cyrus_collision::ground::SupportSource::Terrain
            )),
            "world {} instance {index}: residual response cannot include terrain",
            self.world
        );
        let spatial = self.spatial(id)?;
        let previous = spatial.previous_local_position.map(i64::from);
        let mut position = spatial.local.position.map(i64::from);
        let distance = contact::residual_distance(previous, position)?;
        let normals = contacts.iter().map(|body| body.normal).collect::<Vec<_>>();
        let marked =
            contact::separate_residual(distance, &mut position, &normals, settings.poly_push_units);
        let mut result = [0; 3];
        for axis in 0..3 {
            result[axis] = i32::try_from(position[axis]).with_context(|| {
                format!(
                    "world {} instance {index}: residual response axis {axis} outside Q8 range",
                    self.world
                )
            })?;
        }
        self.spatial
            .as_mut()
            .context("ordinary residual spatial state unavailable")?
            .actor_mut(index)?
            .local
            .position = result;
        if marked {
            self.actors[index]
                .control
                .as_mut()
                .expect("validated ordinary control")
                .body_response_flags |= 4;
        }
        Ok(())
    }

    /// Resolve the primary wall loop for ordinary grounded states zero and
    /// fifteen, after support response. Retains the initial movement heading
    /// and displacement across contacts. Residual separation and support
    /// following are later stages, not performed here. Nonzero runtime motion
    /// bytes require their state-specific response instead.
    pub fn resolve_ordinary_primary_contacts(
        &mut self,
        id: ScriptActorId,
        contacts: &[RuntimeBodyContact],
        settings: &crate::player::data::MovementSettings,
    ) -> Result<()> {
        use crate::player::{contact, motion};
        let index = self.resolve(id)?;
        ensure!(
            contacts.len() <= 16,
            "world {} instance {index}: primary body response exceeds sixteen contacts",
            self.world
        );
        let actor = &self.actors[index];
        ensure!(
            actor
                .placement()
                .is_some_and(|placement| placement.object_type == 1),
            "world {} instance {index}: ordinary body response object path unavailable",
            self.world
        );
        let control = actor
            .control
            .as_ref()
            .context("ordinary body response control unavailable")?;
        ensure!(
            matches!(control.living_state, 0 | 15)
                && control.sliding_state == 0
                && control.falling_state == 0
                && control.queued_motion == 0,
            "world {} instance {index}: state-specific ordinary body response unavailable",
            self.world
        );
        let living_state = control.living_state;
        let spatial = self.spatial(id)?;
        let previous = spatial.previous_local_position.map(i64::from);
        let attempted = spatial.local.position.map(i64::from);
        let mut heading = spatial.local.angles[1];
        let movement = if previous[0] == attempted[0] && previous[2] == attempted[2] {
            heading
        } else {
            motion::displacement_heading(attempted[0] - previous[0], attempted[2] - previous[2])
        };
        let mut current = attempted;
        let mut flags = control.body_response_flags;
        for body in contacts {
            if body.classification == 4
                || contact::facing_difference(movement, body.normal).abs() >= 512
            {
                continue;
            }
            match contact::resolve_wall(
                previous,
                attempted,
                current,
                body.normal,
                settings.slide_range,
            ) {
                contact::WallResponse::Rollback(position) => {
                    current = position;
                    flags |= 2;
                }
                contact::WallResponse::Projected(position) => {
                    current = position;
                    if living_state == 0 && current != previous {
                        heading = contact::wall_heading(heading, movement, body.normal)
                            .rem_euclid(2048) as u16;
                        flags |= 1;
                    } else {
                        flags |= 2;
                    }
                }
            }
        }
        let mut position = [0; 3];
        for axis in 0..3 {
            position[axis] = i32::try_from(current[axis]).with_context(|| {
                format!(
                    "world {} instance {index}: body response axis {axis} outside Q8 range",
                    self.world
                )
            })?;
        }
        let spatial = self
            .spatial
            .as_mut()
            .context("ordinary body response spatial state unavailable")?
            .actor_mut(index)?;
        spatial.local.position = position;
        spatial.local.angles[1] = heading;
        self.actors[index]
            .control
            .as_mut()
            .expect("validated ordinary control")
            .body_response_flags = flags;
        Ok(())
    }

    /// Collect primary contacts in retained model order with original integer
    /// normals and owners. Rebuilds collision orientation from current local
    /// angles and initial body spheres; other models retain presentation poses.
    /// Does not change support owners, surface state, or logical motion.
    pub fn query_ordinary_body(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryBodyQuery<'_>,
    ) -> Result<Vec<RuntimeBodyContact>> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let spheres = self
            .ordinary_collision_shape(id)?
            .body
            .iter()
            .map(|sphere| {
                (
                    Vec3::from_array(sphere.center.map(|value| value as f32)),
                    sphere.radius as f32,
                )
            })
            .collect::<Vec<_>>();
        let pose = self.spatial(id)?.local;
        let rows = cyrus_world::transform::Matrix::collision_orientation(pose.angles)
            .0
            .map(Vec3::from_array);
        query.scene.static_collision.runtime_primary_body_contacts(
            &query.prepared.candidates,
            query
                .scene
                .actor_collision
                .as_ref()
                .context("ordinary body live geometry unavailable; re-export world")?,
            &poses,
            PrimaryBodyQuery {
                probe: BodyProbe {
                    position: pose.position.map(i64::from),
                    rotation: Mat3::from_cols(rows[0], rows[1], rows[2]).transpose(),
                    spheres: &spheres,
                    sidedness: query.sidedness,
                },
                excluded_candidate_type: query.excluded_candidate_type,
                terrain: query.terrain,
            },
        )
    }
}
