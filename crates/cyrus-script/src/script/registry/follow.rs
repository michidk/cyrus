//! Supporting actor motion runs after collision and before ordinary axis locks.
use super::{ScriptActorId, ScriptWorld};
use crate::script::JumpMotion;
use anyhow::{Context, Result};
use bevy::math::{Mat3, Vec3};
use cyrus_collision::collision_mesh::BodyProbe;
use cyrus_soup::Attribute;
use cyrus_world::heading::wrap;
use cyrus_world::transform::Matrix;

/// Inputs of the selected player's destination admission: the world's collision scene and the
/// candidate gather that the original repeats at the follow stage.
#[derive(Clone, Copy)]
pub struct FollowAdmission<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub view: [i32; 2],
    pub actors: &'a [ScriptActorId],
}

impl ScriptWorld {
    /// Carry an ordinary nonplayer rider using its retained supporting actor.
    /// Positions are local Q8; rotation uses the support's update-entry angles,
    /// independently of stable ground and retained model presentation. Follows
    /// preservation's animation.md supporting actor motion rules. Living states 10 through 19
    /// follow the separate ledge support and carry the retained ledge point and heading along;
    /// they skip the selected player's collision admission. The selected player in any other
    /// state keeps the translation and heading turn when its body or its path to the rotated
    /// destination collides there. An attached support carries riders by its effective pose once three family refreshes
    /// ran. Unsupported paths fail before changing the rider's pose.
    pub fn follow_ordinary_support(
        &mut self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
        admission: Option<FollowAdmission<'_>>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("support-follow control unavailable")?;
        let special = control.living_state.is_ledge();
        let Some(owner) = (if special {
            actor.model.ledge_support
        } else {
            actor.model.support.actor
        }) else {
            return Ok(false);
        };
        let owner_index = self.resolve(owner)?;
        if self.actors[owner_index].attributes[Attribute::PlayerAlign] == 0 {
            return Ok(false);
        }
        let admitted = if !special
            && selected_player == Some(id)
            && actor.attributes[Attribute::Collide] & 1 != 0
        {
            Some(admission.with_context(|| {
                format!(
                    "world {} instance {index}: selected-player support-follow collision admission unavailable",
                    self.world
                )
            })?)
        } else {
            None
        };
        let support = self.spatial(owner)?;
        // An attached support publishes its effective pose through the family refresh, which
        // must have run three times before it carries riders.
        let attached = support.parent.is_some();
        if attached
            && self
                .spatial
                .as_ref()
                .context("support-follow spatial state unavailable")?
                .refreshes()
                < 3
        {
            return Ok(false);
        }
        let support = self.spatial(owner)?;
        let (current_position, previous_position, current_angles, previous_angles) = if attached {
            let pose = support.effective.with_context(|| {
                format!(
                    "world {} instance {index} support {owner_index}: attached support pose unavailable",
                    self.world
                )
            })?;
            (
                pose.position,
                support
                    .previous_effective_position
                    .context("attached support previous position unavailable")?,
                pose.angles,
                support
                    .previous_effective_angles
                    .context("attached support previous angles unavailable")?,
            )
        } else if support.linked_position().is_some() {
            // A linked-position record that names an owner supplies the support's positions.
            (
                support.link_cache,
                support.link_previous,
                support.local.angles,
                support.angle_history,
            )
        } else {
            (
                support.local.position,
                support.previous_local_position,
                support.local.angles,
                support.angle_history,
            )
        };
        let rider = self.spatial(id)?.local;
        // The support's translation, then its rotation about its own origin, carry a point.
        let carry = |point: [i32; 3]| -> Result<[i32; 3]> {
            let translated: [i32; 3] = std::array::from_fn(|axis| {
                point[axis]
                    .wrapping_add(current_position[axis].wrapping_sub(previous_position[axis]))
            });
            let offset =
                std::array::from_fn(|axis| translated[axis].wrapping_sub(current_position[axis]));
            let offset = Matrix::orientation(previous_angles.map(|angle| (angle & 2047) as u16))
                .inverse_transform(offset)?;
            let offset = Matrix::orientation(current_angles.map(|angle| (angle & 2047) as u16))
                .transform(offset)?;
            Ok(std::array::from_fn(|axis| {
                current_position[axis].wrapping_add(offset[axis])
            }))
        };
        let turn = current_angles[1].wrapping_sub(previous_angles[1]);
        let heading = wrap(rider.angles[1].wrapping_add(turn));
        let translated: [i32; 3] = std::array::from_fn(|axis| {
            rider.position[axis]
                .wrapping_add(current_position[axis].wrapping_sub(previous_position[axis]))
        });
        let mut position = carry(rider.position)?;
        if let Some(admission) = admitted {
            // The translation and the heading turn stand before the destination is tested.
            let spatial = self
                .spatial
                .as_mut()
                .context("support-follow spatial state unavailable")?
                .actor_mut(index)?;
            spatial.local.position = translated;
            spatial.local.angles[1] = heading;
            if self.follow_destination_blocked(id, position, admission)? {
                position = translated;
            }
        }
        if special {
            // A rider holding a ledge carries the held point and heading with its support.
            let (point, ledge_heading) = self.ledge_anchor(index)?;
            self.set_ledge_anchor(
                index,
                carry(point)?,
                wrap(ledge_heading.wrapping_add((turn & 2047) as u16)),
            );
        }
        let spatial = self
            .spatial
            .as_mut()
            .context("support-follow spatial state unavailable")?;
        let rider = spatial.actor_mut(index)?;
        rider.local.position = position;
        rider.local.angles[1] = heading;
        Ok(true)
    }
}

impl ScriptWorld {
    /// Whether the selected player's body at the carried destination, or the segment from its
    /// update-entry position to it, meets geometry. The candidates are gathered again at the
    /// translated position; the queued jump state excludes candidate type 128 from the body test.
    fn follow_destination_blocked(
        &mut self,
        id: ScriptActorId,
        destination: [i32; 3],
        admission: FollowAdmission<'_>,
    ) -> Result<bool> {
        let FollowAdmission {
            scene,
            animations,
            view,
            actors,
        } = admission;
        let prepared =
            self.prepare_ordinary_collision_query(id, scene, animations, view, actors)?;
        let poses = self.ordinary_collision_poses(&prepared)?;
        let models = scene
            .actor_collision
            .as_ref()
            .context("support-follow live geometry unavailable; re-export world")?;
        let spatial = self.spatial(id)?;
        let (angles, previous) = (spatial.local.angles, spatial.previous_local_position);
        let queued = self.actors[id.index]
            .control
            .as_ref()
            .context("support-follow control unavailable")?
            .queued_motion;
        let spheres: Vec<(Vec3, f32)> = self
            .ordinary_collision_shape(id)?
            .body
            .iter()
            .map(|sphere| {
                (
                    Vec3::from_array(sphere.center.map(|value| value as f32)),
                    sphere.radius as f32,
                )
            })
            .collect();
        let rows = Matrix::collision_orientation(angles.map(|angle| (angle & 2047) as u16))
            .0
            .map(Vec3::from_array);
        let collision = &scene.static_collision;
        let body = BodyProbe {
            position: destination.map(i64::from),
            rotation: Mat3::from_cols(rows[0], rows[1], rows[2]).transpose(),
            spheres: &spheres,
            sidedness: false,
        };
        if collision.runtime_blocks_body(
            &prepared.candidates,
            models,
            &poses,
            &body,
            (queued == JumpMotion::ROPE).then_some(128),
        )? {
            return Ok(true);
        }
        Ok(!collision
            .runtime_segment_contacts(
                &prepared.candidates,
                models,
                &poses,
                previous.map(|value| value >> 8),
                destination.map(|value| value >> 8),
                false,
                None,
            )?
            .is_empty())
    }
}

impl super::ActorControlState {
    /// Advance the retained action counter before ordinary animation work.
    /// Preservation's SOUP action-counter rule uses signed bytes and wrapping
    /// tick subtraction. Early clearing tests the complete queued/falling bytes;
    /// admitted updates clear their raised-arms bits before subtraction.
    pub(super) fn advance_action_transition_counter(&mut self, ticks: u32) {
        if self.action_transition_counter == 0 {
            return;
        }
        if (self.queued_motion == crate::script::JumpMotion::NONE
            || self.falling_state == crate::script::JumpMotion::NONE)
            && (self.action_transition_counter as i8) < 3
        {
            self.action_transition_counter = 0;
            return;
        }
        self.queued_motion = self.queued_motion.with_arms_raised(false);
        self.falling_state = self.falling_state.with_arms_raised(false);
        let remaining = self.action_transition_counter.wrapping_sub(ticks as u8) as i8;
        self.action_transition_counter = remaining.max(0) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::super::ActorControlState;

    #[test]
    fn action_counter_preserves_zero_and_early_clear_motion_bytes() {
        use crate::script::JumpMotion;
        for (counter, queued, falling, ticks, remaining, next_queued, next_falling) in [
            (0, 129, 128, 1, 0, 129, 128),
            (2, 0, 129, 0, 0, 0, 129),
            (2, 129, 128, 1, 1, 1, 0),
            (6, 0, 129, 2, 4, 0, 1),
            (3, 129, 129, 6, 0, 1, 1),
            (1, 129, 129, 255, 2, 1, 1),
            (255, 0, 129, 1, 0, 0, 129),
            (6, 129, 129, 258, 4, 1, 1),
        ] {
            for living_state in
                [0, 9, 10, 15, 17, 19, 20, 255].map(crate::script::LivingState::from_byte)
            {
                let mut control = ActorControlState {
                    living_state,
                    queued_motion: JumpMotion::from_byte(queued),
                    falling_state: JumpMotion::from_byte(falling),
                    actions: [128, 255].into(),
                    action_transition_counter: counter,
                    ..Default::default()
                };
                control.advance_action_transition_counter(ticks);
                assert_eq!(control.action_transition_counter, remaining);
                assert_eq!(control.queued_motion.byte(), next_queued);
                assert_eq!(control.falling_state.byte(), next_falling);
                assert_eq!(control.actions, [128, 255]);
                assert_eq!(control.living_state, living_state);
            }
        }
    }
}
