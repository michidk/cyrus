//! The nonplayer blocked-ahead probe follows preservation's engine/SOUP.md "Nonplayer blocked-ahead probe".
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result};
use bevy::math::{Mat3, Vec3};
use cyrus_collision::{collision_mesh::BodyProbe, collision_world::ResidualBodyQuery};
use cyrus_world::animation::group_class;

impl ScriptWorld {
    /// Whether the space ahead of the receiver is blocked by `distance`: only for a receiver with
    /// a combat record whose current animation group has class one, one zero-radius point is
    /// tested against the candidates collected before its update. **Inferred:** the point test
    /// treats both faces of a surface as blocking.
    pub fn blocked_ahead(
        &self,
        id: ScriptActorId,
        distance: i32,
        query: OrdinaryObstacleQuery<'_>,
    ) -> Result<bool> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("blocked-ahead probe control unavailable")?;
        let Some(group) = actor
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group())
        else {
            return Ok(false);
        };
        if control.combat.is_none() || group_class(group) != 1 {
            return Ok(false);
        }
        let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
        let post = i32::try_from(query.scene.settings.post_collide_height)
            .context("blocked-ahead probe post_collide_height exceeds signed range")?;
        let spatial = self.spatial(id)?;
        let offset = spatial.local.matrix.transform([
            0,
            extent
                .wrapping_sub(post)
                .wrapping_sub(distance)
                .wrapping_shl(8),
            -0x3800,
        ])?;
        let origin = spatial.query_pose()?.position;
        let point: [i64; 3] =
            std::array::from_fn(|axis| i64::from(origin[axis]) + i64::from(offset[axis]));
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let contacts =
            query
                .scene
                .static_collision
                .runtime_residual_body_contacts(
                    &query.prepared.candidates,
                    query.scene.actor_collision.as_ref().context(
                        "blocked-ahead probe live geometry unavailable; re-export world",
                    )?,
                    &poses,
                    ResidualBodyQuery {
                        probe: BodyProbe {
                            position: point,
                            rotation: Mat3::IDENTITY,
                            spheres: &[(Vec3::ZERO, 0.0)],
                            sidedness: false,
                        },
                        excluded_candidate_type: (control.queued_motion == 24).then_some(128),
                    },
                )?;
        Ok(!contacts.is_empty())
    }
}
