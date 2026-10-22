//! Supporting actor motion runs after collision and before ordinary axis locks.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_world::transform::Matrix;

impl ScriptWorld {
    /// Carry an ordinary nonplayer rider using its retained supporting actor.
    /// Positions are local Q8; rotation uses the support's update-entry angles,
    /// independently of stable ground and retained model presentation. Follows
    /// preservation's animation.md supporting actor motion rules. Living states 10 through 19
    /// follow the separate ledge support and carry the retained ledge point and heading along;
    /// they skip the selected player's collision admission. Selected-player ordinary follow and
    /// attached/linked support paths require their owners. Unsupported paths fail before
    /// changing the rider's pose.
    pub fn follow_ordinary_support(
        &mut self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
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
        ensure!(
            special || selected_player != Some(id),
            "world {} instance {index}: selected-player support-follow collision admission unsupported",
            self.world
        );
        let support = self.spatial(owner)?;
        ensure!(
            support.parent.is_none() && support.linked_position.is_none(),
            "world {} instance {index} support {owner_index}: attached or linked support-follow snapshots unavailable",
            self.world
        );
        let current = support.local;
        let previous_position = support.previous_local_position;
        let previous_angles = support.angle_history;
        let rider = self.spatial(id)?.local;
        // The support's translation, then its rotation about its own origin, carry a point.
        let carry = |point: [i32; 3]| -> Result<[i32; 3]> {
            let translated: [i32; 3] = std::array::from_fn(|axis| {
                point[axis]
                    .wrapping_add(current.position[axis].wrapping_sub(previous_position[axis]))
            });
            let offset =
                std::array::from_fn(|axis| translated[axis].wrapping_sub(current.position[axis]));
            let offset = Matrix::orientation(previous_angles).inverse_transform(offset)?;
            let offset = Matrix::orientation(current.angles).transform(offset)?;
            Ok(std::array::from_fn(|axis| {
                current.position[axis].wrapping_add(offset[axis])
            }))
        };
        let turn = current.angles[1].wrapping_sub(previous_angles[1]);
        let position = carry(rider.position)?;
        let heading = rider.angles[1].wrapping_add(turn) & 2047;
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

impl super::ActorControlState {
    /// Advance the retained action counter before ordinary animation work.
    /// Preservation's SOUP action-counter rule uses signed bytes and wrapping
    /// tick subtraction. Living state does not participate in early clearing.
    pub(super) fn advance_action_transition_counter(&mut self, ticks: u32) {
        if self.action_transition_counter == 0 {
            return;
        }
        if self.directional_state.contains(&0) && (self.action_transition_counter as i8) < 3 {
            self.action_transition_counter = 0;
            return;
        }
        self.directional_state = self.directional_state.map(|value| value & 127);
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
