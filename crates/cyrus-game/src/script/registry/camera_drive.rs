//! The original follow camera as a per-frame driver of the world's camera session.
//!
//! The native and browser game views use this camera by default. `CameraDrive::enabled` remains
//! a resource so hosts can disable it for inspection or diagnostics.
use super::{ScriptActorId, ScriptWorld, camera::SceneCameraUpdate};
use crate::script::camera::{
    CameraHitSelection, CameraPose, CameraTerrainHeights, CameraUpdateSettings, GlidePlayer,
    RenderPose,
};
use anyhow::{Context, Result};
use bevy::prelude::Resource;

/// Switch for the camera driver.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct CameraDrive {
    pub enabled: bool,
}

/// The camera as the last driven frame left it, for the presentation to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PublishedCamera {
    pub pose: CameraPose,
    pub render: RenderPose,
}

/// Driver state kept between frames: whether setup ran, the hit history and the published camera.
#[derive(Resource, Default)]
pub struct CameraFrameState {
    pub(super) world: Option<u64>,
    pub(super) hits: Option<CameraHitSelection>,
    pub published: Option<PublishedCamera>,
    /// The world's map terrain, supplied by the presentation once its environment has loaded.
    pub terrain: Option<std::sync::Arc<cyrus_assets::environment::TerrainHeights>>,
}

impl CameraFrameState {
    /// Drop publication and collision history when the driven world is no longer present.
    ///
    /// The presentation resource outlives a world replacement. Keeping the previous world's
    /// publication here would make `script_view` and the renderer continue to consume that pose
    /// while the replacement world is loading (or after a failed camera setup).
    fn clear_world(&mut self) {
        self.world = None;
        self.hits = None;
        self.published = None;
        self.terrain = None;
    }
}

/// The world's exact map terrain heights, as the camera's terrain stage reads them. **Inferred**:
/// the retained, wave-animated window is not consulted, so over water the base height is used.
pub(super) struct MapTerrain<'a>(&'a cyrus_assets::environment::TerrainHeights);

impl CameraTerrainHeights for MapTerrain<'_> {
    fn retained_center(&self) -> [i32; 2] {
        // Far outside the map, so no sample selects the retained grid.
        [-1000, -1000]
    }
    fn retained_height(&self, _: [i32; 2]) -> Result<f32> {
        anyhow::bail!("camera retained terrain heights are not available")
    }
    fn map_height(&self, [x, z]: [i32; 2]) -> Result<f32> {
        let side = self.0.side;
        let row = side as i64 - i64::from(z);
        anyhow::ensure!(
            (0..side as i32).contains(&x) && (0..side as i64).contains(&row),
            "camera terrain cell [{x}, {z}]: outside the {side}-wide map"
        );
        Ok(self.0.heights[row as usize * side + x as usize])
    }
}

fn empty_hits() -> CameraHitSelection {
    CameraHitSelection {
        minimum_position: [0; 3],
        maximum_position: [0; 3],
        minimum_normal: [0; 3],
        maximum_normal: [0; 3],
    }
}

/// The map-unit view the engine builds its map list around: the published camera position.
fn view_of(pose: CameraPose) -> Result<[i32; 2]> {
    let view = [pose.position[0] >> 8, pose.position[2] >> 8];
    anyhow::ensure!(
        view.iter().all(|axis| (0..=65535).contains(axis)),
        "camera position {:?} outside the map",
        pose.position
    );
    Ok(view)
}

impl ScriptWorld {
    /// The selected player's weapon and rope state for the render step. **Inferred**: a weapon
    /// attribute or a drawn weapon counts as holding (the original's predicate also needs the
    /// inventory owner and shell context).
    pub fn selected_glide_player(&self) -> Result<GlidePlayer> {
        let index = self
            .selected_player_index()
            .context("camera glide: selected player unavailable")?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("camera glide: selected player control unavailable")?;
        Ok(GlidePlayer {
            holding_weapon: actor.attributes[110] != 0 || control.weapon_drawn,
            rope_attached: control.rope_attached,
        })
    }

    /// Establish the camera once per world: compute the first pose from record zero's candidate,
    /// with the limiter bypassed as the original's setup pass does, then cut the render camera.
    fn establish_camera(
        &mut self,
        scene: &crate::player::data::PlayerScene,
        actors: &[ScriptActorId],
        hits: &mut CameraHitSelection,
        frame_scale: i32,
    ) -> Result<CameraPose> {
        let camera = self.camera.as_ref().context("camera session unavailable")?;
        let record = &camera.records[camera.selected.index()];
        let bound = self.resolve(record.actor)?;
        let (position, angles) = self.camera_binding_pose(bound)?;
        let mut record = record.clone();
        let candidate = record.candidate(position, angles)?;
        let pose = CameraPose {
            position: candidate,
            angles: [0; 3],
        };
        let camera_index = self.resolve(camera.camera_actor)?;
        if self
            .spatial
            .as_ref()
            .context("camera setup spatial state not prepared")?
            .is_initialized(camera_index)?
        {
            self.replace_camera_pose(pose)?;
        } else {
            self.initialize_camera_pose(pose)?;
        }
        let limits = self.camera_limits;
        let settings = CameraUpdateSettings::installed(&limits, frame_scale, false, false);
        let mut geometry = self.scene_camera_geometry(
            scene,
            view_of(self.camera.as_ref().expect("initialized").current)?,
            actors,
            limits,
            None::<MapTerrain<'_>>,
        )?;
        let selected = self.camera.as_ref().expect("initialized").selected.index();
        self.camera.as_mut().expect("initialized").records[selected].flags |= 0x01;
        let result = self.update_camera(hits, &settings, &mut geometry);
        let camera = self.camera.as_mut().expect("initialized");
        camera.records[selected].flags &= !0x01;
        let pose = result?;
        // Setup stores its first pose as both the current and the previous pose.
        camera.previous = camera.current;
        camera.reset_render();
        Ok(pose)
    }
}

/// The camera's side of one driven frame, before the world update: establish the camera on first
/// use and build the pose update the location natives run. `None` when the world has no camera.
pub(super) fn begin<'a>(
    world: &mut ScriptWorld,
    scene: &'a crate::player::data::PlayerScene,
    state: &mut CameraFrameState,
    terrain: Option<&'a cyrus_assets::environment::TerrainHeights>,
    view: [i32; 2],
    frame_scale: i32,
) -> Result<Option<SceneCameraUpdate<'a, MapTerrain<'a>>>> {
    if world.camera.is_none() {
        state.clear_world();
        return Ok(None);
    }
    // Hit selections and the published pose belong to the camera session that produced them.
    // A new ScriptWorld must start from the camera's zeroed shared state rather than carrying
    // collision outputs or a render pose from the previous world into its first frame.
    if state.world != Some(world.generation()) {
        state.hits = None;
        state.published = None;
        state.terrain = None;
    }
    let actors = world.ordinary_actors(view)?.to_vec();
    let mut hits = state.hits.take().unwrap_or_else(empty_hits);
    let established = if state.world != Some(world.generation()) {
        let result = world.establish_camera(scene, &actors, &mut hits, frame_scale);
        if result.is_ok() {
            state.world = Some(world.generation());
        }
        result.map(|_| ())
    } else {
        Ok(())
    };
    let limits = world.camera_limits;
    let settings = CameraUpdateSettings::installed(&limits, frame_scale, false, terrain.is_some());
    let built = established.and_then(|()| {
        let current = world.camera.as_ref().expect("checked above").current;
        world.scene_camera_update(
            scene,
            view_of(current)?,
            &actors,
            limits,
            settings,
            terrain.map(MapTerrain),
            hits.clone(),
        )
    });
    if built.is_err() {
        state.hits = Some(hits);
    }
    built.map(Some)
}

/// The camera's side of one driven frame, after the world update: run the pose update and the
/// render step with the hit history the natives may have advanced, and publish the result.
pub(super) fn end(
    world: &mut ScriptWorld,
    scene: &crate::player::data::PlayerScene,
    state: &mut CameraFrameState,
    update: SceneCameraUpdate<'_, MapTerrain<'_>>,
    terrain: Option<&cyrus_assets::environment::TerrainHeights>,
    view: [i32; 2],
    frame_scale: i32,
) -> Result<()> {
    let hits = update.into_hits();
    let result = (|| -> Result<PublishedCamera> {
        let actors = world.ordinary_actors(view)?.to_vec();
        let current = world
            .camera
            .as_ref()
            .context("camera session unavailable")?
            .current;
        let limits = world.camera_limits;
        let settings =
            CameraUpdateSettings::installed(&limits, frame_scale, false, terrain.is_some());
        let mut geometry = world.scene_camera_geometry(
            scene,
            view_of(current)?,
            &actors,
            limits,
            terrain.map(MapTerrain),
        )?;
        // The pose update already ran at the camera actor's slot in the update; this is the render
        // step with the terrain stage it glides through.
        let player = world.selected_glide_player()?;
        world.present_camera(player, true, &mut |position| {
            if settings.terrain_enabled {
                crate::script::camera::correct_terrain(
                    position,
                    &mut geometry,
                    settings.body_clearance,
                )?;
            }
            Ok(())
        })?;
        let camera = world.camera.as_ref().expect("checked above");
        Ok(PublishedCamera {
            pose: camera.current,
            render: camera.render,
        })
    })();
    state.hits = Some(hits);
    state.published = Some(result?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_frame_state_drops_publication_when_world_disappears() {
        let mut state = CameraFrameState {
            world: Some(7),
            hits: Some(CameraHitSelection {
                minimum_position: [1, 2, 3],
                maximum_position: [4, 5, 6],
                minimum_normal: [7, 8, 9],
                maximum_normal: [10, 11, 12],
            }),
            published: Some(PublishedCamera {
                pose: CameraPose {
                    position: [13, 14, 15],
                    angles: [16, 17, 18],
                },
                render: RenderPose {
                    position: [19.0, 20.0, 21.0],
                    angles: [22, 23, 24],
                },
            }),
            terrain: None,
        };

        state.clear_world();

        assert!(state.world.is_none());
        assert!(state.hits.is_none());
        assert!(state.published.is_none());
    }
}
