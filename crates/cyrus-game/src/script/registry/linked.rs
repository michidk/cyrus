//! Linked-position records: `Offset` and `UnOffset`, the `PlayerOffset` natives and the per-update
//! refresh.
//!
//! An actor with an owner is positioned at the owner's effective position plus its stored offset
//! rotated by the owner's orientation, plus its own local position. `Offset` converts the actor's
//! current position into that offset and zeroes the local position, so the actor then moves with
//! its owner; `UnOffset` converts back. The record keeps a cached copy of the position for
//! effective-position queries, refreshed at the start of each update and published as the model
//! position in the model stage.
use super::ScriptWorld;
use anyhow::{Context, Result, ensure};

impl ScriptWorld {
    /// The owner-relative position of `index`: the owner's effective position and the offset
    /// rotated by the owner's matrix, plus the actor's own local position. Additions wrap.
    fn linked_world_position(&self, index: usize) -> Result<Option<[i32; 3]>> {
        let id = self.id(index)?;
        let spatial = self.spatial(id)?;
        let Some(link) = spatial.link else {
            return Ok(None);
        };
        let owner = self.id(link.owner)?;
        let base = self.query_position(owner)?;
        let rotated = self.spatial(owner)?.local.matrix.transform(link.offset)?;
        Ok(Some(std::array::from_fn(|axis| {
            rotated[axis].wrapping_add(base[axis].wrapping_add(spatial.local.position[axis]))
        })))
    }

    /// `Offset(x, y, z)` called on `owner` by `index`: attach the caller to the owner at its
    /// current position plus the arguments, expressed in the owner's frame. An actor already
    /// attached to this owner keeps its offset; one attached to another owner is detached first.
    /// An attached (slave) caller is fatal in the original and unsupported here.
    pub(super) fn offset_actor(
        &mut self,
        index: usize,
        owner: usize,
        arguments: [i32; 3],
    ) -> Result<()> {
        let (id, owner_id) = (self.id(index)?, self.id(owner)?);
        ensure!(
            self.actors[index].attributes[26] == 0,
            "Offset: an attached actor cannot be offset"
        );
        if let Some(link) = self.spatial(id)?.link {
            if link.owner == owner {
                return Ok(());
            }
            self.unoffset_actor(index)?;
        }
        let base = self.query_position(owner_id)?;
        let local = self.spatial(id)?.local.position;
        let delta = std::array::from_fn(|axis| {
            local[axis]
                .wrapping_sub(base[axis])
                .wrapping_add(arguments[axis])
        });
        let offset = self
            .spatial(owner_id)?
            .local
            .matrix
            .inverse_transform(delta)?;
        let spatial = self
            .spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?;
        spatial.link_to(owner, offset);
        spatial.local.position = [0; 3];
        Ok(())
    }

    /// `UnOffset`: give the actor its owner-relative position as local coordinates, publish it as
    /// the model position and clear the owner. An actor without an owner is unchanged.
    pub(super) fn unoffset_actor(&mut self, index: usize) -> Result<()> {
        let Some(position) = self.linked_world_position(index)? else {
            return Ok(());
        };
        let spatial = self
            .spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?;
        spatial.unlink();
        spatial.local.position = position;
        self.actors[index].model.position = Some(position.map(|value| (value >> 8) as f32));
        Ok(())
    }

    /// Refresh the cached position of a linked actor; the previous cache is kept. Runs right after
    /// the update-boundary snapshot.
    pub(super) fn refresh_linked_position(&mut self, index: usize) -> Result<()> {
        let Some(position) = self.linked_world_position(index)? else {
            return Ok(());
        };
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .set_link_cache(position);
        Ok(())
    }

    /// The model position of a linked actor in map units, ahead of any water processing.
    pub(super) fn linked_model_position(&self, index: usize) -> Result<Option<[f32; 3]>> {
        Ok(self
            .linked_world_position(index)?
            .map(|position| position.map(|value| (value >> 8) as f32)))
    }

    /// `PlayerOffset` and each `PlayerOffsettask` step: place the selected player at the
    /// receiver's effective position plus `offset` rotated by the receiver's matrix. A following
    /// step also gives the player the receiver's orientation and angle history.
    pub(super) fn offset_selected_player(
        &mut self,
        receiver: usize,
        offset: [i32; 3],
        follow: bool,
    ) -> Result<()> {
        let receiver_id = self.id(receiver)?;
        let player = self
            .selected_player()
            .context("PlayerOffset: selected player unavailable")?;
        let player_index = self.resolve(player)?;
        let base = self.query_position(receiver_id)?;
        let source = self.spatial(receiver_id)?;
        let rotated = source.local.matrix.transform(offset)?;
        let (matrix, angles, history) = (
            source.local.matrix,
            source.local.angles,
            source.angle_history,
        );
        let target = self
            .spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(player_index)?;
        target.local.position = std::array::from_fn(|axis| base[axis].wrapping_add(rotated[axis]));
        if follow {
            target.local.matrix = matrix;
            target.local.angles = angles;
            target.angle_history = history;
        }
        Ok(())
    }
}
