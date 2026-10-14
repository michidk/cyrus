//! Supporting actor motion runs after collision and before ordinary axis locks.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_world::transform::Matrix;

impl ScriptWorld {
    /// Carry an ordinary nonplayer rider using its retained supporting actor.
    /// Positions are local Q8; rotation uses the support's update-entry angles,
    /// independently of stable ground and retained model presentation. Follows
    /// preservation's animation.md supporting actor motion rules. Selected-player,
    /// special living-state and attached/linked support paths require their owners.
    /// Unsupported paths fail before changing the rider's pose.
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
        ensure!(
            !(10..=19).contains(&control.living_state),
            "world {} instance {index}: special support-follow owner unavailable",
            self.world
        );
        let Some(owner) = actor.model.support.actor else {
            return Ok(false);
        };
        let owner_index = self.resolve(owner)?;
        if self.actors[owner_index].attributes[24] == 0 {
            return Ok(false);
        }
        ensure!(
            selected_player != Some(id),
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
        let translated: [i32; 3] = std::array::from_fn(|axis| {
            rider.position[axis]
                .wrapping_add(current.position[axis].wrapping_sub(previous_position[axis]))
        });
        let offset =
            std::array::from_fn(|axis| translated[axis].wrapping_sub(current.position[axis]));
        let offset = Matrix::orientation(previous_angles).inverse_transform(offset)?;
        let offset = Matrix::orientation(current.angles).transform(offset)?;
        let position =
            std::array::from_fn(|axis| current.position[axis].wrapping_add(offset[axis]));
        let heading =
            rider.angles[1].wrapping_add(current.angles[1].wrapping_sub(previous_angles[1])) & 2047;
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
