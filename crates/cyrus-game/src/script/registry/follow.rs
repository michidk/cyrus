//! Supporting actor motion runs after collision and before ordinary axis locks.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result};
use bevy::math::{Mat3, Vec3};
use cyrus_collision::collision_mesh::BodyProbe;
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
        let special = (10..=19).contains(&control.living_state);
        let Some(owner) = (if special {
            actor.model.ledge_support
        } else {
            actor.model.support.actor
        }) else {
            return Ok(false);
        };
        let owner_index = self.resolve(owner)?;
        if self.actors[owner_index].attributes[24] == 0 {
            return Ok(false);
        }
        let admitted = if !special && selected_player == Some(id) && actor.attributes[1] & 1 != 0 {
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
            let offset = Matrix::orientation(previous_angles).inverse_transform(offset)?;
            let offset = Matrix::orientation(current_angles).transform(offset)?;
            Ok(std::array::from_fn(|axis| {
                current_position[axis].wrapping_add(offset[axis])
            }))
        };
        let turn = current_angles[1].wrapping_sub(previous_angles[1]);
        let heading = rider.angles[1].wrapping_add(turn) & 2047;
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
        if special && let Some((point, ledge_heading)) = self.globals.selected_ledge_anchor {
            self.globals.selected_ledge_anchor =
                Some((carry(point)?, ledge_heading.wrapping_add(turn) & 2047));
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
        let rows = Matrix::collision_orientation(angles)
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
            (queued == 24).then_some(128),
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
    /// tick subtraction. Living state does not participate in early clearing.
    pub(super) fn advance_action_transition_counter(&mut self, ticks: u32) {
        if self.action_transition_counter == 0 {
            return;
        }
        if self.directional_state.iter().any(|word| word & 255 == 0)
            && (self.action_transition_counter as i8) < 3
        {
            self.action_transition_counter = 0;
            return;
        }
        self.directional_state = self.directional_state.map(|value| value & !128);
        let remaining = self.action_transition_counter.wrapping_sub(ticks as u8) as i8;
        self.action_transition_counter = remaining.max(0) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::super::ActorControlState;

    #[test]
    fn action_counter_preserves_direction_edges_and_signed_byte_timing() {
        for (counter, directions, ticks, expected_counter, expected_directions) in [
            (0, [128, 255], 1, 0, [128, 255]),
            (2, [0, 255], 0, 0, [0, 255]),
            (2, [128, 255], 1, 1, [0, 127]),
            (6, [0, 255], 2, 4, [0, 127]),
            (3, [1, 1], 6, 0, [1, 1]),
            (1, [1, 1], 255, 2, [1, 1]),
            (255, [0, 255], 1, 0, [0, 255]),
            (6, [128, 255], 258, 4, [0, 127]),
        ] {
            for living_state in [0, 9, 10, 15, 17, 19, 20, 255] {
                let mut control = ActorControlState {
                    living_state,
                    directional_state: directions,
                    action_transition_counter: counter,
                    ..Default::default()
                };
                control.advance_action_transition_counter(ticks);
                assert_eq!(control.action_transition_counter, expected_counter);
                assert_eq!(control.directional_state, expected_directions);
                assert_eq!(control.living_state, living_state);
            }
        }
    }
}
