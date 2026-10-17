//! Line of sight between two actors follows preservation's combat decision "In sight" rules.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result};
use cyrus_collision::{distance::rounded_distance, ground::SupportSource};

impl ScriptWorld {
    /// The actor's query position: the attached pose, the linked position, or the local
    /// position, in that order (the selection shared by the fighting query).
    fn sight_position(&self, index: usize) -> Result<[i32; 3]> {
        let actor = self
            .spatial
            .as_ref()
            .context("in-sight spatial owner unavailable")?
            .actor(index)?;
        if actor.parent.is_some() {
            Ok(actor.query_pose()?.position)
        } else {
            Ok(actor.linked_position.unwrap_or(actor.local.position))
        }
    }

    /// Whether `target` is within `limit` (Q8 units) of the receiver with nothing in its
    /// prepared candidate list between them. The segment between the two query positions, in
    /// whole map units, is tested against the receiver's candidates collected before its
    /// update; hits on the target's own model do not block. **Inferred:** the model test treats
    /// both faces of a surface as blocking.
    pub fn actor_in_sight(
        &self,
        id: ScriptActorId,
        target: ScriptActorId,
        limit: i32,
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let target_index = self.resolve(target)?;
        let from = self.sight_position(id.index)?;
        let to = self.sight_position(target_index)?;
        let distance = rounded_distance(std::array::from_fn(|axis| {
            to[axis].wrapping_sub(from[axis])
        }))?;
        if distance > limit {
            return Ok(false);
        }
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let contacts = query.scene.static_collision.runtime_segment_contacts(
            &query.prepared.candidates,
            query
                .scene
                .actor_collision
                .as_ref()
                .context("in-sight live geometry unavailable; re-export world")?,
            &poses,
            from.map(|value| value >> 8),
            to.map(|value| value >> 8),
            false,
            None,
        )?;
        Ok(contacts.iter().all(|(source, _)| {
            matches!(source, SupportSource::Actor { actor, .. } if *actor == target_index)
        }))
    }
}
