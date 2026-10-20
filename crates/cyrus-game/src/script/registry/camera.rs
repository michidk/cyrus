//! Live session camera services: first pose establishment and one atomic update.
use super::{ScriptActorId, ScriptWorld};
use crate::script::camera::{
    CameraGeometry, CameraHitSelection, CameraPose, CameraUpdateInputs, CameraUpdateSettings,
    GlidePlayer,
};
use anyhow::{Context, Result};

fn wrapped_angles(angles: [i32; 3]) -> Result<[u16; 3]> {
    anyhow::ensure!(
        angles.iter().all(|angle| (0..2048).contains(angle)),
        "camera pose angles {angles:?} outside the wrapped range"
    );
    Ok(angles.map(|angle| angle as u16))
}

impl ScriptWorld {
    /// Establish the camera's first published pose. Setup computes it with one pose pass
    /// that omits movement limiting; the caller supplies that pose. Both history poses
    /// start equal, and the CAMERA actor's spatial state is created once.
    pub fn initialize_camera_pose(&mut self, pose: CameraPose) -> Result<()> {
        let camera = self
            .camera
            .as_ref()
            .with_context(|| format!("world {}: camera session unavailable", self.world))?;
        let index = self.resolve(camera.camera_actor)?;
        let angles = wrapped_angles(pose.angles)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .initialize_player(index, &self.actors[index], pose.position, angles)
            .with_context(|| {
                format!(
                    "world {} camera actor {index}: pose initialization",
                    self.world
                )
            })?;
        let camera = self.camera.as_mut().expect("checked above");
        camera.current = pose;
        camera.previous = pose;
        camera.reset_render();
        Ok(())
    }

    /// The render camera's per-frame step: glide to the published pose or cut to it. The caller
    /// supplies the selected player's weapon and rope state, whether the frame is a normal
    /// main-loop frame, and the terrain correction for a gliding position (Q8 units, in place).
    /// See `CameraState::present`; a failed correction leaves the camera untouched.
    pub fn present_camera(
        &mut self,
        player: GlidePlayer,
        in_frame: bool,
        terrain: &mut dyn FnMut(&mut [i32; 3]) -> Result<()>,
    ) -> Result<()> {
        let camera = self
            .camera
            .as_mut()
            .with_context(|| format!("world {}: camera session unavailable", self.world))?;
        camera.present(&self.camera_glide, &player, in_frame, terrain)
    }

    /// Run one camera update for the selected record's bound actor and publish it: the
    /// session state, the hit history and both logical actor positions change together.
    /// Every identity and pose is validated before any state changes, and a geometry or
    /// correction error leaves the world and `hits` untouched.
    /// An attached bound actor is rejected; its effective-pose binding is not established.
    pub fn update_camera(
        &mut self,
        hits: &mut CameraHitSelection,
        settings: &CameraUpdateSettings,
        geometry: &mut impl CameraGeometry,
    ) -> Result<CameraPose> {
        let context = |what: &str| format!("world {} camera update: {what}", self.world);
        let camera = self
            .camera
            .as_ref()
            .with_context(|| context("session unavailable"))?;
        let bound = self.resolve(camera.records[camera.selected.index()].actor)?;
        anyhow::ensure!(
            self.actors[bound].attributes[26] == 0,
            "{}",
            context("attached bound actor unsupported")
        );
        let spatial = self
            .spatial
            .as_ref()
            .with_context(|| context("spatial state not prepared"))?;
        let bound_pose = spatial
            .actor(bound)
            .with_context(|| context("bound actor pose"))?
            .local;
        for id in [camera.camera_actor, camera.alternate_actor] {
            let id: ScriptActorId = id;
            spatial
                .actor(self.resolve(id)?)
                .with_context(|| context("published camera actor pose"))?;
        }
        let update = camera.prepare_pose(
            hits,
            &CameraUpdateInputs {
                bound_position: bound_pose.position,
                bound_angles: bound_pose.angles,
                movement: settings.movement,
                body_radius: settings.body_radius,
                body_clearance: settings.body_clearance,
                terrain_enabled: settings.terrain_enabled,
            },
            geometry,
        )?;
        let spatial = self.spatial.as_mut().expect("checked above");
        for (id, position) in update.actor_positions {
            let actor = spatial
                .actor_mut(id.index)
                .expect("published actor pose checked above");
            actor.local.position = position;
            if let Some(effective) = &mut actor.effective {
                effective.position = position;
            }
        }
        let pose = update.state.current;
        *hits = update.hits;
        self.camera = Some(update.state);
        Ok(pose)
    }
}

/// Production camera geometry over the loaded scene and the world's live models, built once per
/// update by [`ScriptWorld::scene_camera_geometry`].
///
/// **Inferred** from the original's code, not yet compared by execution: the segment query filters
/// the engine's map list (the static window around the view, then live actors that are not
/// animation-excluded) with its own bounds rules, and the contact query gathers with the shared
/// gatherer at radius 200 excluding the bound actor's model, then queries the camera body (two
/// spheres at the body origin, radii `camera_size` and `camera_scape_size`, identity orientation)
/// without a contact cap or terrain contact. Both use the original's one-sided mode.
pub struct SceneCameraGeometry<'a, T> {
    scene: &'a crate::player::data::PlayerScene,
    view: [i32; 2],
    live: Vec<cyrus_collision::candidates::LiveActor>,
    poses: std::collections::BTreeMap<usize, cyrus_collision::collision_mesh::ModelPose>,
    limits: cyrus_assets::script::CameraLimits,
    terrain: Option<T>,
}

impl<T> SceneCameraGeometry<'_, T> {
    fn owner(&self, excluded: ScriptActorId) -> Option<cyrus_collision::candidates::ModelInstance> {
        self.live
            .iter()
            .any(|actor| actor.actor == excluded.index)
            .then_some(cyrus_collision::candidates::ModelInstance::Runtime(
                excluded.index,
            ))
    }
}

impl<T: crate::script::camera::CameraTerrainHeights> CameraGeometry for SceneCameraGeometry<'_, T> {
    fn segment(
        &mut self,
        start: [i32; 3],
        end: [i32; 3],
        excluded: ScriptActorId,
    ) -> Result<Vec<cyrus_collision::collision_mesh::IntegerSegmentContact>> {
        let mut models = self.scene.static_collision.window_models(self.view)?;
        models.extend(
            self.live
                .iter()
                .filter(|actor| !actor.animation_excluded)
                .filter_map(|actor| actor.model),
        );
        let collected = crate::script::camera::collect_segment_models(
            start,
            end,
            self.limits.obstacle_size.wrapping_shl(8),
            self.owner(excluded),
            &models,
        )?;
        let (start, end) = (start.map(|value| value >> 8), end.map(|value| value >> 8));
        let mut contacts = Vec::new();
        for model in collected {
            for contact in self.scene.static_collision.model_segment_contacts(
                model,
                self.scene.actor_collision.as_ref(),
                &self.poses,
                start,
                end,
                false,
            )? {
                contacts.push(contact.integer()?);
            }
        }
        Ok(contacts)
    }

    fn contacts(&mut self, position: [i32; 3], excluded: ScriptActorId) -> Result<Vec<[i32; 3]>> {
        use bevy::math::{Mat3, Vec3};
        let query = cyrus_collision::candidates::CandidateQuery::without_model(
            position,
            self.owner(excluded),
        );
        let candidates =
            self.scene
                .static_collision
                .runtime_candidates(self.view, &query, &self.live, &[])?;
        let spheres = [
            (Vec3::ZERO, self.limits.camera_size as f32),
            (Vec3::ZERO, self.limits.camera_scape_size as f32),
        ];
        let actors = self
            .scene
            .actor_collision
            .as_ref()
            .context("camera contacts: live model collision unavailable (re-export)")?;
        let found = self.scene.static_collision.runtime_residual_body_contacts(
            &candidates,
            actors,
            &self.poses,
            cyrus_collision::collision_world::ResidualBodyQuery {
                probe: cyrus_collision::collision_mesh::BodyProbe {
                    position: position.map(i64::from),
                    rotation: Mat3::IDENTITY,
                    spheres: &spheres,
                    sidedness: false,
                },
                excluded_candidate_type: None,
            },
        )?;
        Ok(found.into_iter().map(|contact| contact.normal).collect())
    }

    fn terrain_height(&mut self, horizontal: [i32; 2]) -> Result<f32> {
        let terrain = self
            .terrain
            .as_ref()
            .context("camera terrain heights unavailable")?;
        Ok(crate::script::camera::sample_terrain(horizontal, terrain)?.0)
    }
}

impl ScriptWorld {
    /// Gather the live models and stored poses the camera's geometry queries need. `actors` is the
    /// ordinary update order, `view` the map-unit position the engine builds its map list around
    /// (the published camera position), and `terrain` the retained and map heights when terrain
    /// correction is enabled.
    pub fn scene_camera_geometry<'a, T: crate::script::camera::CameraTerrainHeights>(
        &self,
        scene: &'a crate::player::data::PlayerScene,
        view: [i32; 2],
        actors: &[ScriptActorId],
        limits: cyrus_assets::script::CameraLimits,
        terrain: Option<T>,
    ) -> Result<SceneCameraGeometry<'a, T>> {
        anyhow::ensure!(
            scene.world == self.world,
            "world {} camera geometry scene identity mismatch",
            self.world
        );
        let live = self.live_collision_inputs(actors)?;
        let mut poses = std::collections::BTreeMap::new();
        for actor in &live {
            if let Some(record) = self.model_presentation(self.id(actor.actor)?)? {
                poses.insert(actor.actor, record.collision_pose()?);
            }
        }
        Ok(SceneCameraGeometry {
            scene,
            view,
            live,
            poses,
            limits,
            terrain,
        })
    }
}

/// The pose update the location natives run, over the loaded scene: the same geometry and hit
/// history as the per-frame update, behind the shared handle `ScriptInputs::camera_update` takes.
pub struct SceneCameraUpdate<'a, T> {
    geometry: std::cell::RefCell<SceneCameraGeometry<'a, T>>,
    hits: std::cell::RefCell<CameraHitSelection>,
    settings: CameraUpdateSettings,
    published: std::cell::RefCell<Vec<(ScriptActorId, [i32; 3])>>,
}

impl<T> std::fmt::Debug for SceneCameraUpdate<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneCameraUpdate")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl<T> SceneCameraUpdate<'_, T> {
    /// The hit history after the updates so far, for the owner to retain between frames.
    pub fn into_hits(self) -> CameraHitSelection {
        self.hits.into_inner()
    }
}

impl<T: crate::script::camera::CameraTerrainHeights> crate::script::camera::CameraUpdateService
    for SceneCameraUpdate<'_, T>
{
    fn update(
        &self,
        state: &mut crate::script::camera::CameraState,
        bound_position: [i32; 3],
        bound_angles: [u16; 3],
    ) -> Result<()> {
        let update = state.prepare_pose(
            &self.hits.borrow(),
            &CameraUpdateInputs {
                bound_position,
                bound_angles,
                movement: self.settings.movement,
                body_radius: self.settings.body_radius,
                body_clearance: self.settings.body_clearance,
                terrain_enabled: self.settings.terrain_enabled,
            },
            &mut *self.geometry.borrow_mut(),
        )?;
        *state = update.state;
        *self.hits.borrow_mut() = update.hits;
        *self.published.borrow_mut() = update.actor_positions.to_vec();
        Ok(())
    }

    fn take_published(&self) -> Vec<(ScriptActorId, [i32; 3])> {
        std::mem::take(&mut *self.published.borrow_mut())
    }
}

impl ScriptWorld {
    /// Build the pose update the location natives need for one scheduler pass.
    #[allow(clippy::too_many_arguments)] // The scene, view, actor order, limits, settings, terrain and hit history are independent owners' inputs.
    pub fn scene_camera_update<'a, T: crate::script::camera::CameraTerrainHeights>(
        &self,
        scene: &'a crate::player::data::PlayerScene,
        view: [i32; 2],
        actors: &[ScriptActorId],
        limits: cyrus_assets::script::CameraLimits,
        settings: CameraUpdateSettings,
        terrain: Option<T>,
        hits: CameraHitSelection,
    ) -> Result<SceneCameraUpdate<'a, T>> {
        Ok(SceneCameraUpdate {
            geometry: std::cell::RefCell::new(
                self.scene_camera_geometry(scene, view, actors, limits, terrain)?,
            ),
            hits: std::cell::RefCell::new(hits),
            settings,
            published: Default::default(),
        })
    }

    /// One frame of the camera: the pose update (publishing the logical camera actors), then the
    /// render step. The terrain stage the glide runs uses the same geometry and clearance.
    pub fn advance_camera_frame(
        &mut self,
        hits: &mut CameraHitSelection,
        settings: &CameraUpdateSettings,
        geometry: &mut impl CameraGeometry,
        player: GlidePlayer,
        in_frame: bool,
    ) -> Result<CameraPose> {
        let pose = self.update_camera(hits, settings, geometry)?;
        self.present_camera(player, in_frame, &mut |position| {
            if settings.terrain_enabled {
                crate::script::camera::correct_terrain(
                    position,
                    geometry,
                    settings.body_clearance,
                )?;
            }
            Ok(())
        })?;
        Ok(pose)
    }
}
