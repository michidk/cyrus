//! Live session camera services: first pose establishment and one atomic update.
use super::{ScriptActorId, ScriptWorld};
use crate::script::camera::{
    CameraGeometry, CameraHitSelection, CameraPose, CameraUpdateInputs, CameraUpdateSettings,
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
