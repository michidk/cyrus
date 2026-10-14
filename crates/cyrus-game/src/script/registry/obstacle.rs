//! Ordinary forward segment response retains the pre-script candidate list.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_collision::collision_mesh::IntegerSegmentContact;

/// Explicit forward-query inputs. Vertical offset is in map units; response
/// writes logical X/Z only. State transitions belong to the enclosing update.
#[derive(Clone, Copy)]
pub struct OrdinaryObstacleQuery<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub prepared: &'a super::model::OrdinaryCollisionQuery,
    pub vertical_offset: i32,
    pub respond: bool,
}

/// Closest accepted map-unit point and ordered combined normal with scale 256.
#[derive(Debug, PartialEq, Eq)]
pub struct OrdinaryForwardHit {
    pub point: [i32; 3],
    pub normal: [i32; 3],
}

/// Select a point by masked rank while combining all facing normals in order.
/// Equal ranks retain the earlier point. The facing threshold and 2047 wrap
/// correction follow preservation's ordinary forward segment response rules.
pub fn ordinary_forward_hit(
    movement: u16,
    contacts: &[IntegerSegmentContact],
) -> Option<OrdinaryForwardHit> {
    let mut best = None;
    let mut rank = u32::MAX;
    let mut normal = [0i32; 3];
    for contact in contacts {
        if crate::player::contact::facing_difference(movement, contact.normal).abs() >= 512 {
            continue;
        }
        let candidate_rank = contact.rank & 0x7fff;
        if candidate_rank < rank {
            rank = candidate_rank;
            best = Some(contact.point);
        }
        normal = if normal == [0; 3] {
            contact.normal
        } else {
            super::super::spatial::normalized_direction(std::array::from_fn(|axis| {
                normal[axis].wrapping_add(contact.normal[axis])
            }))
            .map(|component| component >> 8)
        };
    }
    best.map(|point| OrdinaryForwardHit { point, normal })
}

impl ScriptWorld {
    /// Probe previous-to-current effective motion against retained face models.
    /// Candidate membership stays fixed while model poses are read at query time.
    /// State fifteen runs its distinct bounds query and returns no forward hit.
    /// A hit without response leaves position unchanged, including its Y component.
    pub fn resolve_ordinary_forward_obstacle(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("ordinary forward obstacle control unavailable")?;
        if control.living_state == 15 {
            self.resolve_ordinary_bounds_obstacle(id, query)?;
            return Ok(false);
        }
        let excluded = (control.queued_motion == 24).then_some(128);
        let spatial = self.spatial(id)?;
        let current = spatial.query_pose()?.position;
        let previous = if spatial.parent.is_none() {
            spatial.previous_local_position
        } else {
            spatial
                .previous_effective_position
                .context("attached forward query previous effective position unavailable")?
        };
        let movement = crate::player::motion::displacement_heading(
            i64::from(current[0].wrapping_sub(previous[0])),
            i64::from(current[2].wrapping_sub(previous[2])),
        );
        let endpoints = |position: [i32; 3]| {
            let mut result = position.map(|value| value >> 8);
            result[1] = result[1].wrapping_add(query.vertical_offset);
            result
        };
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let contacts = query
            .scene
            .static_collision
            .runtime_segment_contacts(
                &query.prepared.candidates,
                query
                    .scene
                    .actor_collision
                    .as_ref()
                    .context("ordinary forward live geometry unavailable; re-export world")?,
                &poses,
                endpoints(previous),
                endpoints(current),
                true,
                excluded,
            )?
            .into_iter()
            .map(|(source, contact)| {
                contact.integer().with_context(|| {
                    format!(
                        "world {} instance {} forward contact {source:?}",
                        self.world, id.index
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let Some(hit) = ordinary_forward_hit(movement, &contacts) else {
            return Ok(false);
        };
        if query.respond {
            let spheres = self.ordinary_overlap_spheres(id, query.animations)?;
            let radius = spheres
                .first()
                .context("ordinary forward selected first sphere unavailable")?
                .radius
                .trunc();
            ensure!(
                radius.is_finite() && (0.0..2147483648.0).contains(&f64::from(radius)),
                "world {} instance {}: forward sphere radius outside integer range",
                self.world,
                id.index
            );
            let position = &mut self
                .spatial
                .as_mut()
                .context("ordinary forward spatial state unavailable")?
                .actor_mut(id.index)?
                .local
                .position;
            for axis in [0, 2] {
                position[axis] = hit.point[axis]
                    .wrapping_mul(256)
                    .wrapping_add(hit.normal[axis].wrapping_mul(radius as i32));
            }
        }
        Ok(true)
    }
}
