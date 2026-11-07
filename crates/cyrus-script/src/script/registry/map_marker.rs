//! Console marker placement: the loaded map's own markers and the saved user markers.
use super::ScriptWorld;
use anyhow::{Context, Result};
use cyrus_world::heading::wrap;

/// The state one saved user marker holds (preservation `engine/console.md`, marker file section).
/// Positions are Q8 runtime units; every angle is in the engine's 0 through 2047 units. The camera
/// pair is the published camera pose at the command boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedPlacement {
    pub position: [i32; 3],
    pub orientation: [i32; 3],
    pub camera_position: [i32; 3],
    pub camera_angles: [i32; 3],
}

impl ScriptWorld {
    /// Console `getmapmarker`: move the selected player to map marker `marker` and keep its
    /// heading. The marker must lie below the map table's count and be present; positions are the
    /// marker's map units times 256, and the previous-position snapshot follows at once (preservation
    /// `engine/console.md`, marker file section). Unlike [`Self::place_selected_player`] this clears
    /// no ledge, fall, slide or rope state, since the original handler only moves the position.
    /// The original's built-in 128-slot table, used when no map table is loaded, is not modelled.
    pub fn move_selected_player_to_map_marker(&mut self, marker: i32) -> Result<()> {
        let position = self
            .markers
            .as_ref()
            .context("getmapmarker: the map's marker table is not prepared")?
            .position(marker)?;
        let player = self
            .selected_player()
            .context("getmapmarker: selected player unavailable")?;
        let index = self.resolve(player)?;
        let actor = self
            .spatial
            .as_mut()
            .context("getmapmarker: spatial state not prepared")?
            .actor_mut(index)?;
        let yaw = actor.local.angles[1];
        actor.relocate(position, (yaw & 2047) as u16);
        self.refresh_transform(player)
    }

    /// Console `setmarker`: the selected player's position and orientation and the published
    /// camera pose, as a saved user marker holds them.
    pub fn selected_player_placement(&self) -> Result<SavedPlacement> {
        let player = self
            .selected_player()
            .context("setmarker: selected player unavailable")?;
        let index = self.resolve(player)?;
        let pose = &self
            .spatial
            .as_ref()
            .context("setmarker: spatial state not prepared")?
            .actor(index)?
            .local;
        let camera = self
            .camera
            .as_ref()
            .context("setmarker: camera session unavailable")?
            .current;
        Ok(SavedPlacement {
            position: pose.position,
            orientation: pose.angles.map(i32::from),
            camera_position: camera.position,
            camera_angles: camera.angles,
        })
    }

    /// Console `getmarker`: move the selected player to the saved position and orientation (each
    /// angle wrapped to 0 through 2047), with position and angle history following at once.
    /// Restore the published camera values; the next camera update retains its normal ownership.
    /// Like `getmapmarker` this clears no ledge, fall, slide or rope state.
    pub fn restore_selected_player_placement(&mut self, placement: &SavedPlacement) -> Result<()> {
        self.camera
            .as_ref()
            .context("getmarker: camera session unavailable")?;
        let player = self
            .selected_player()
            .context("getmarker: selected player unavailable")?;
        let index = self.resolve(player)?;
        let actor = self
            .spatial
            .as_mut()
            .context("getmarker: spatial state not prepared")?
            .actor_mut(index)?;
        let angles = placement.orientation.map(|angle| wrap(angle) as u16);
        actor.relocate(placement.position, angles[1]);
        actor.local.angles = angles.map(i32::from);
        actor.snapshot();
        self.refresh_transform(player)?;
        let camera = self.camera.as_mut().expect("validated camera session");
        camera.current = super::super::camera::CameraPose {
            position: placement.camera_position,
            angles: placement.camera_angles,
        };
        Ok(())
    }
}
