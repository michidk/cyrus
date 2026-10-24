//! `projectileAI`: one flight step of a created projectile, following preservation's
//! engine/SOUP.md "projectileAI". Returns whether the projectile keeps flying.
use super::GameHost;
use anyhow::{Context, Result};
use cyrus_collision::actor_overlap::OverlapSphere;
use cyrus_collision::candidates::{CandidateOwner, CandidateQuery, ModelBounds, ModelInstance};
use cyrus_collision::distance::{point_in_sphere, rounded_distance};

/// Distance from the parent, in Q8, at which the flight ends.
const MAXIMUM_RANGE: i32 = 0xbb800;
/// Combat message a hit actor receives from the projectile's parent.
const DIRECT_DAMAGE: i32 = 0xd005;
/// Rank a wall contact must undercut to be kept.
const NO_CONTACT_RANK: u32 = 0x7fff;
/// Spheres an actor contributes to the hit test at most.
const MAXIMUM_SPHERES: usize = 128;

impl GameHost<'_, '_, '_, '_> {
    pub(super) fn fly_projectile(&mut self) -> Result<bool> {
        let id = self.registry.id(self.index)?;
        let parent = self.registry.actors[self.index]
            .control
            .as_ref()
            .context("projectileAI control unavailable")?
            .creator;
        let scale = self
            .inputs
            .frame_scale
            .context("projectileAI frame scale unavailable")?;
        let speed_attribute = self.registry.actors[self.index].attributes[8];
        let speed = (i32::from(speed_attribute) << 8).wrapping_mul(scale as i32) >> 8;
        let local = self.registry.spatial(id)?.local;
        let previous = local.position;
        // The step is the shoot offset rotation with the yaw masked to the turn range.
        let step = cyrus_world::transform::Matrix::orientation([
            local.angles[0].wrapping_neg() & 2047,
            local.angles[1] & 2047,
            local.angles[2].wrapping_neg() & 2047,
        ])
        .transform([0, 0, speed])?;
        let position = std::array::from_fn(|axis| previous[axis].wrapping_add(step[axis]));
        self.set_projectile_position(position)?;
        if let Some(parent) = parent {
            let parent_position = self
                .registry
                .spatial(self.registry.id(parent)?)?
                .query_pose()?
                .position;
            let distance = rounded_distance(std::array::from_fn(|axis| {
                position[axis].wrapping_sub(parent_position[axis])
            }))?;
            if distance >= MAXIMUM_RANGE {
                return Ok(false);
            }
        }
        let source = self
            .inputs
            .player_collision
            .context("projectileAI requires the world's collision geometry")?;
        if let Some(terrain) = &source.scene.terrain
            && let Some(height) = terrain.height(position.map(i64::from))
        {
            let ground = (height * 256.0).round_ties_even() as i32;
            if position[1] > ground {
                self.set_projectile_position(previous)?;
                return Ok(false);
            }
        }
        let model = self
            .registry
            .model_presentation(id)?
            .map(|record| ModelBounds {
                instance: ModelInstance::Runtime(self.index),
                position: record.position,
                radius: record.radius,
            });
        let radius = f32::from(speed_attribute);
        let query = CandidateQuery::at(position, radius, model.map(|model| model.instance));
        let candidates = source.scene.static_collision.runtime_candidates(
            source.view,
            &query,
            &self.registry.live_collision_inputs(source.actors)?,
            &[],
        )?;
        let start = previous.map(|value| value >> 8);
        let end = position.map(|value| value >> 8);
        let poses = self.registry.candidate_poses(&candidates)?;
        let mut nearest: Option<(u32, [i32; 3])> = None;
        for face in &candidates.faces {
            if parent.is_some_and(|parent| face.owner == CandidateOwner::Actor(parent)) {
                continue;
            }
            for contact in source.scene.static_collision.model_segment_contacts(
                face.model,
                source.scene.actor_collision.as_ref(),
                &poses,
                start,
                end,
                false,
            )? {
                let limit = nearest.map_or(NO_CONTACT_RANK, |(rank, _)| rank);
                if contact.rank < limit {
                    let point = contact.point.map(|value| value.round_ties_even() as i32);
                    nearest = Some((contact.rank, point));
                }
            }
        }
        if let Some((_, point)) = nearest {
            self.set_projectile_position(point.map(|value| value << 8))?;
            return Ok(false);
        }
        for actor in &candidates.actors {
            let CandidateOwner::Actor(index) = actor.owner else {
                continue;
            };
            if Some(index) == parent {
                continue;
            }
            let target = self.registry.id(index)?;
            let hit = self
                .projectile_spheres(target, source.animations)?
                .into_iter()
                .any(|(center, radius)| {
                    point_in_sphere(start, center, radius) || point_in_sphere(end, center, radius)
                });
            if !hit {
                continue;
            }
            let has_combat = self.registry.actors[index]
                .control
                .as_ref()
                .is_some_and(|control| control.combat.is_some());
            if has_combat && let Some(parent) = parent {
                let sender = self.registry.id(parent)?;
                self.registry
                    .send_combat_message(sender, target, DIRECT_DAMAGE)?;
            }
            self.set_projectile_position(previous)?;
            return Ok(false);
        }
        Ok(true)
    }

    fn set_projectile_position(&mut self, position: [i32; 3]) -> Result<()> {
        self.registry
            .spatial
            .as_mut()
            .context("projectileAI spatial state unavailable")?
            .actor_mut(self.index)?
            .local
            .position = position;
        Ok(())
    }

    /// The target's collision spheres in map units. This shares ordinary overlap admission so
    /// vertex-bound RAVC records replace the initial body shape, as they do for actor overlaps.
    fn projectile_spheres(
        &self,
        target: super::ScriptActorId,
        data: &cyrus_assets::animation::Animations,
    ) -> Result<Vec<([i32; 3], i32)>> {
        let actor = &self.registry.actors[target.index];
        if actor.attributes[56] != 0 {
            return Ok(Vec::new());
        }
        let ravc = data.actor(actor.name())?["ravc_count"]
            .as_i64()
            .unwrap_or(0);
        if ravc != 0 {
            return Ok(self
                .registry
                .ordinary_overlap_spheres(target, data)?
                .into_iter()
                .take(MAXIMUM_SPHERES)
                .map(projectile_sphere)
                .collect());
        }
        let pose = self.registry.spatial(target)?.query_pose()?;
        // Centers are rotated at Q8 and offset by the Q8 position, then shifted back to map units.
        self.registry
            .ordinary_collision_shape(target)?
            .body
            .iter()
            .take(MAXIMUM_SPHERES)
            .map(|sphere| {
                let rotated = pose
                    .matrix
                    .transform(sphere.center.map(|value| value << 8))?;
                Ok((
                    std::array::from_fn(|axis| {
                        rotated[axis].wrapping_add(pose.position[axis]) >> 8
                    }),
                    sphere.radius,
                ))
            })
            .collect()
    }
}

fn projectile_sphere(sphere: OverlapSphere) -> ([i32; 3], i32) {
    (
        sphere.center.map(|value| value.round() as i32),
        sphere.radius.round() as i32,
    )
}
