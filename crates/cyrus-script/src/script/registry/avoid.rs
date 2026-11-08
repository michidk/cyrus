//! Armed nonplayer actors steer away from walls: four stacked-sphere probes around the body set a
//! corner mask, and the mask nudges the actor sideways along its heading.
use super::{ScriptActorId, ScriptWorld, ground::OrdinaryGroundQuery};
use crate::script::CombatFlags;
use anyhow::{Context, Result};
use bevy::math::{Mat3, Vec3};
use cyrus_collision::{collision_mesh::BodyProbe, collision_world::ResidualBodyQuery};

/// Collision-response bits that start a wall-avoidance pass without a running hold timer.
const AVOID_TRIGGER_MARKS: u32 = 0x10bef;
/// Ticks the last probe mask keeps steering after the probe stops reporting walls.
const AVOID_HOLD_TICKS: i32 = 0x10;

impl ScriptWorld {
    /// The wall-avoidance pass for an armed or combat-stance nonplayer actor: skipped without a
    /// combat record, and without a running hold timer or a trigger mark in the collision-response
    /// word. A probe that finds walls stores its mask and restarts the hold timer; one that finds
    /// none lets the timer run down while the stored mask keeps steering. Each set mask bit then
    /// moves the actor by `frame_scale * 0xa00 >> 8` along its heading rotated by 0x200 (mask 2 and
    /// 8), 0x400 (mask 1) or zero (mask 4), with the side last chosen deciding the extra sidestep
    /// of masks 1 and 4 when neither side bit is set.
    pub fn avoid_walls(
        &mut self,
        id: ScriptActorId,
        query: OrdinaryGroundQuery<'_>,
        ticks: u32,
        frame_scale: u32,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let control = self.actors[index]
            .control
            .as_ref()
            .context("wall avoidance control unavailable")?;
        let Some(combat) = control.combat.as_ref() else {
            return Ok(());
        };
        if combat.avoid_hold == 0 && control.last_collide.bits() & AVOID_TRIGGER_MARKS == 0 {
            return Ok(());
        }
        let found = self.probe_wall_corners(id, query)?;
        let combat = self.actors[index]
            .control
            .as_mut()
            .and_then(|control| control.combat.as_mut())
            .expect("checked combat record");
        let mask = if found == 0 {
            if combat.avoid_hold == 0 {
                return Ok(());
            }
            if combat.avoid_hold <= ticks as i32 {
                combat.avoid_hold = 0;
                combat.avoid_mask = 0;
                return Ok(());
            }
            combat.avoid_hold -= ticks as i32;
            combat.avoid_mask
        } else {
            combat.avoid_mask = found;
            combat.avoid_hold = AVOID_HOLD_TICKS;
            found
        };
        if combat.avoid_side == 0 {
            combat.avoid_side = 2;
        }
        let mut side = combat.avoid_side;
        let step = (frame_scale as i32).wrapping_mul(0xa00) >> 8;
        let heading = self.spatial(id)?.local.wrapped_angles()[1];
        let mut moves = vec![];
        let turn = |offset: i32| (i32::from(heading).wrapping_add(offset) & 0x7ff) as u16;
        if mask & 2 != 0 {
            moves.push(turn(0x200));
            side = 8;
        }
        if mask & 8 != 0 {
            moves.push(turn(-0x200));
            side = 2;
        }
        for (bit, last) in [(1, 0x400), (4, 0)] {
            if mask & bit != 0 {
                if mask & 10 == 0 {
                    moves.push(turn(if side == 8 { 0x200 } else { -0x200 }));
                }
                moves.push(turn(last));
            }
        }
        self.actors[index]
            .control
            .as_mut()
            .and_then(|control| control.combat.as_mut())
            .expect("checked combat record")
            .avoid_side = side;
        let spatial = self
            .spatial
            .as_mut()
            .context("wall avoidance spatial state unavailable")?
            .actor_mut(index)?;
        for angle in moves {
            spatial.local.position =
                cyrus_world::transform::translate_forward(spatial.local.position, angle, step)?;
        }
        Ok(())
    }

    /// The corner mask: bit 1 the front point, 4 the back, 2 the right and 8 the left, each a
    /// vertical stack of four spheres (radius a quarter of the total height above the post-collision
    /// height) at the body's corner rotated by the actor matrix, set when the sphere stack meets a
    /// retained model.
    fn probe_wall_corners(&self, id: ScriptActorId, query: OrdinaryGroundQuery<'_>) -> Result<u32> {
        self.require_collision_query(id, query.prepared, query.scene, query.animations)?;
        let shape = self.ordinary_collision_shape(id)?;
        let [px, _, pz] = shape.positive_extent;
        let [mx, my, mz] = shape.negative_extent;
        let post = i32::try_from(query.scene.settings.post_collide_height)
            .context("wall avoidance post_collide_height exceeds signed range")?;
        let total = shape.total_extent[1];
        let radius = total.wrapping_sub(post) / 4;
        let half = radius / 2;
        let base = -(my.wrapping_sub(radius / 3));
        let spheres: Vec<(Vec3, f32)> = (0..4)
            .map(|k| {
                (
                    Vec3::new(0.0, base.wrapping_add(k * radius) as f32, 0.0),
                    radius as f32,
                )
            })
            .collect();
        let spatial = self.spatial(id)?;
        let local = spatial.local;
        let poses = self.ordinary_collision_poses(query.prepared)?;
        let models = query
            .scene
            .actor_collision
            .as_ref()
            .context("wall avoidance live geometry unavailable; re-export world")?;
        let truncate = |value: f32| value as i32;
        let origin_y = truncate(local.position[1] as f32 / 256.0);
        let mut mask = 0;
        for (bit, corner) in [
            (1, [0, 0, pz.wrapping_sub(half)]),
            (4, [0, 0, -(mz.wrapping_sub(half))]),
            (2, [px.wrapping_sub(half), 0, 0]),
            (8, [-(mx.wrapping_sub(half)), 0, 0]),
        ] {
            let rotated = local.matrix.transform(corner)?;
            let x = truncate(rotated[0] as f32 + local.position[0] as f32 / 256.0);
            let z = truncate(rotated[2] as f32 + local.position[2] as f32 / 256.0);
            let contacts = query
                .scene
                .static_collision
                .runtime_residual_body_contacts(
                    &query.prepared.candidates,
                    models,
                    &poses,
                    ResidualBodyQuery {
                        probe: BodyProbe {
                            position: [
                                i64::from(x) << 8,
                                i64::from(origin_y) << 8,
                                i64::from(z) << 8,
                            ],
                            rotation: Mat3::IDENTITY,
                            spheres: &spheres,
                            sidedness: false,
                        },
                        excluded_candidate_type: None,
                    },
                )?;
            if !contacts.is_empty() {
                mask |= bit;
            }
        }
        Ok(mask)
    }
}

/// Whether the combat record puts the actor in combat stance.
pub(super) fn in_combat_stance(combat: &crate::script::CombatControlState) -> bool {
    combat.flags.contains(CombatFlags::STANCE)
}
