//! Combat contact queries follow preservation's SOUP motion-contact rules.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};
use cyrus_assets::animation::Animations;
use cyrus_collision::{actor_models::ActorModelCollision, distance::rounded_distance};

impl ScriptWorld {
    /// Test a local Q8 point against the current attack motion's orientation limit.
    /// Wrapping displacement shifts right eight before normalized bearing conversion.
    /// The absolute heading difference must be strictly below the wrapping sum of
    /// `arc` and `arc_center`, following preservation's contact-orientation rules.
    /// This is distinct from the first-motion defence-selection arc interval.
    pub fn combat_contact_orientation(&self, id: ScriptActorId, point: [i32; 3]) -> Result<bool> {
        let result: Result<bool> = (|| {
            let own = self.spatial(id)?.local;
            let delta =
                std::array::from_fn(|axis| point[axis].wrapping_sub(own.position[axis]) >> 8);
            let [x, _, z] = crate::script::spatial::normalized_direction(delta);
            let bearing = i32::from(crate::script::spatial::query_bearing(x, z));
            let difference =
                crate::script::decision::heading_difference(i32::from(own.angles[1]), bearing)
                    .abs();
            let motion = self.current_combat_motion(id)?;
            let center = motion.arc_center.context("arc_center unavailable")?;
            let arc = motion.arc.context("arc unavailable")?;
            Ok(difference < arc.wrapping_add(center))
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} combat contact orientation",
                self.world, id.index,
            )
        })
    }

    /// Query range/arc contact in original service order. Zero candidate radius
    /// checks current-motion orientation, first-motion three-dimensional range,
    /// then shared elevation. A nonzero candidate radius resolves its model point
    /// before orientation and uses horizontal distance with strict range/radius
    /// boundaries. Attribute 126's sphere/segment branch remains unsupported.
    pub fn combat_range_contact(
        &self,
        id: ScriptActorId,
        candidate: ScriptActorId,
        data: &Animations,
        statics: Option<&ActorModelCollision>,
    ) -> Result<bool> {
        let result: Result<bool> = (|| {
            let candidate_index = self.resolve(candidate)?;
            let profile = self.actors[candidate_index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .map(|combat| combat.contact_profile)
                .unwrap_or_default();
            let point = if profile.radius == 0 {
                self.spatial(candidate)?.local.position
            } else {
                self.actor_model_point(candidate, data, statics, i32::from(profile.vertex))?
            };
            if profile.radius != 0 {
                anyhow::ensure!(
                    *self.actors[candidate_index]
                        .attributes
                        .get(126)
                        .context("candidate attribute 126 unavailable")?
                        == 0,
                    "attribute 126 sphere/segment contact unsupported"
                );
            }
            if !self.combat_contact_orientation(id, point)? {
                return Ok(false);
            }
            let to = self.query_position(candidate)?;
            let from = self.query_position(id)?;
            let distance = rounded_distance(std::array::from_fn(|axis| {
                if axis == 1 && profile.radius != 0 {
                    0
                } else {
                    to[axis].wrapping_sub(from[axis])
                }
            }))?;
            let combat = self.actors[self.resolve(id)?]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let selector = combat.retained_action_indices[1];
            if selector == -1 {
                return Ok(false);
            }
            let slot = usize::try_from(selector).context("negative receiver attack slot")?;
            let motion = self
                .combat_attack(id, slot)?
                .with_context(|| format!("attack slot {slot}: unpopulated"))?
                .motions
                .first()
                .context("first attack motion unavailable")?;
            if distance <= motion.min_range || distance >= motion.max_range {
                return Ok(false);
            }
            if profile.radius != 0 {
                return Ok(distance < profile.radius);
            }
            motion
                .elevation
                .context("first attack motion elevation unavailable")?;
            crate::script::decision::PacedDefenceQueries::in_reach(
                &mut super::defence::LivePacedQueries {
                    world: self,
                    id,
                    target: candidate,
                },
                slot,
            )
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} range contact against instance {}",
                self.world, id.index, candidate.index,
            )
        })
    }

    /// Query the current motion's sphere against a candidate's local position.
    /// Resolves the authored model vertex before distance and radius. Coordinate
    /// differences wrap at 32 bits; signed distance must be strictly below the
    /// authored radius. Zero radius does not bypass geometry. Dispatch between
    /// sphere and range/arc contact belongs to the caller.
    pub fn combat_sphere_contact(
        &self,
        id: ScriptActorId,
        candidate: ScriptActorId,
        data: &Animations,
        statics: Option<&ActorModelCollision>,
    ) -> Result<bool> {
        let result: Result<bool> = (|| {
            let motion = self.current_combat_motion(id)?;
            let vertex = motion.col_vertex.context("col_vertex unavailable")?;
            let point = self.actor_model_point(id, data, statics, vertex)?;
            let position = self.spatial(candidate)?.local.position;
            let distance = rounded_distance(std::array::from_fn(|axis| {
                point[axis].wrapping_sub(position[axis])
            }))?;
            let radius = motion
                .col_sphere_size
                .context("col_sphere_size unavailable")?;
            Ok(distance < radius)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} sphere contact against instance {}",
                self.world, id.index, candidate.index
            )
        })
    }
}
