//! State-fifteen bounds segments retain stored extent magnitudes and contact order.
use super::{
    ScriptActorId, ScriptWorld, ground::OrdinaryCollisionShape, obstacle::OrdinaryObstacleQuery,
};
use anyhow::{Context, Result};
use cyrus_collision::collision_mesh::IntegerSegmentContact;
use cyrus_world::transform::Matrix;

/// The two original bounds diagonals, each with independent signed rank limits.
#[derive(Clone, Copy, Debug)]
pub enum BoundsSegment {
    First,
    Second,
}

/// Build original map-unit endpoints from stored extents, retained actor matrix
/// and effective Q8 origin. Negative-side magnitudes are used without negation.
pub fn ordinary_bounds_segments(
    shape: &OrdinaryCollisionShape,
    matrix: Matrix,
    origin: [i32; 3],
) -> Result<[[[i32; 3]; 2]; 2]> {
    let [px, py, pz] = shape.positive_extent;
    let [nx, _, nz] = shape.negative_extent;
    let local = [
        [[px, py.wrapping_add(2), pz], [nx, py.wrapping_sub(2), nz]],
        [[nx, py.wrapping_add(2), pz], [px, py.wrapping_sub(2), nz]],
    ];
    let origin = origin.map(|value| value >> 8);
    let mut result = [[[0; 3]; 2]; 2];
    for segment in 0..2 {
        for endpoint in 0..2 {
            let rotated = matrix.transform(local[segment][endpoint])?;
            result[segment][endpoint] =
                std::array::from_fn(|axis| rotated[axis].wrapping_add(origin[axis]));
        }
    }
    Ok(result)
}

/// Accumulate normals only when a qualifying contact improves its segment's
/// positive or negative rank limit. Rank zero and normal Y outside -41..=160
/// contribute nothing. Earlier contributions remain; the sum is not normalized.
pub fn ordinary_bounds_separation(contacts: &[(BoundsSegment, IntegerSegmentContact)]) -> [i32; 3] {
    let mut negative = [-999999i32; 2];
    let mut positive = [999999i32; 2];
    let mut sum = [0i32; 3];
    for (segment, contact) in contacts {
        if !(-41..=160).contains(&contact.normal[1]) {
            continue;
        }
        let index = match segment {
            BoundsSegment::First => 0,
            BoundsSegment::Second => 1,
        };
        let rank = contact.rank as i32;
        let selected = if rank < 0 && rank > negative[index] {
            negative[index] = rank;
            true
        } else if rank > 0 && rank < positive[index] {
            positive[index] = rank;
            true
        } else {
            false
        };
        if selected {
            sum = std::array::from_fn(|axis| sum[axis].wrapping_add(contact.normal[axis]));
        }
    }
    sum
}

impl ScriptWorld {
    /// Run the bounds stage explicitly. The enclosing forward stage chooses it
    /// for living state fifteen, independently of vertical offset/response flag.
    /// Every candidate's first and second segments run before the next candidate.
    /// Nonzero separation writes all logical Q8 axes and marks body-response bit six.
    pub fn resolve_ordinary_bounds_obstacle(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<()> {
        use cyrus_collision::candidates::CollisionCandidates;
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("ordinary bounds obstacle control unavailable")?;
        let excluded = (control.queued_motion == 24).then_some(128);
        let spatial = self.spatial(id)?;
        let segments = ordinary_bounds_segments(
            self.ordinary_collision_shape(id)?,
            spatial.local.matrix,
            spatial.query_pose()?.position,
        )?;
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let models = query
            .scene
            .actor_collision
            .as_ref()
            .context("ordinary bounds live geometry unavailable; re-export world")?;
        let mut contacts = Vec::new();
        for candidate in &query.prepared.candidates.faces {
            if excluded == Some(candidate.candidate_type) {
                continue;
            }
            let selected = CollisionCandidates {
                faces: vec![*candidate],
                actors: vec![],
            };
            for (segment, endpoints) in [BoundsSegment::First, BoundsSegment::Second]
                .into_iter()
                .zip(segments)
            {
                for (source, contact) in query.scene.static_collision.runtime_segment_contacts(
                    &selected,
                    models,
                    &poses,
                    endpoints[0],
                    endpoints[1],
                    false,
                    None,
                )? {
                    contacts.push((
                        segment,
                        contact.integer().with_context(|| {
                            format!(
                                "world {} instance {} bounds contact {source:?}",
                                self.world, id.index
                            )
                        })?,
                    ));
                }
            }
        }
        let separation = ordinary_bounds_separation(&contacts);
        if separation != [0; 3] {
            let position = &mut self
                .spatial
                .as_mut()
                .context("ordinary bounds spatial state unavailable")?
                .actor_mut(id.index)?
                .local
                .position;
            *position = std::array::from_fn(|axis| position[axis].wrapping_add(separation[axis]));
            self.actors[id.index]
                .control
                .as_mut()
                .context("ordinary bounds control unavailable")?
                .body_response_flags |= 0x40;
        }
        Ok(())
    }
}
