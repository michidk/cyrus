//! Camera replacement preserves view state without framing or player movement.
use super::{FlycamSettings, FrameScene, InitialView, ViewCamera, projection::GameProjection};
use anyhow::{Context, Result, ensure};
use bevy::prelude::{
    Commands, Entity, PerspectiveProjection, Projection, Quat, Transform, Vec3, Vec4, With, World,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct Pose {
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}
impl Pose {
    fn capture(value: &Transform) -> Self {
        Self {
            translation: value.translation.to_array(),
            rotation: value.rotation.to_array(),
            scale: value.scale.to_array(),
        }
    }
    fn restore(self) -> Result<Transform> {
        ensure!(
            self.translation
                .iter()
                .chain(&self.rotation)
                .chain(&self.scale)
                .all(|v| v.is_finite()),
            "camera pose contains nonfinite values"
        );
        let rotation = Quat::from_array(self.rotation);
        ensure!(
            rotation.is_normalized(),
            "camera rotation is not normalized"
        );
        ensure!(
            self.scale.iter().all(|v| *v != 0.0),
            "camera scale is singular"
        );
        Ok(Transform {
            translation: Vec3::from_array(self.translation),
            rotation,
            scale: Vec3::from_array(self.scale),
        })
    }
}

/// View, framing and inspection controls at a completed presentation boundary.
#[derive(Clone, Serialize, Deserialize)]
pub struct CameraSnapshot {
    version: u32,
    active: Option<bool>,
    pose: Pose,
    projection: [f32; 4],
    near_clip_plane: [f32; 4],
    flycam: bool,
    speed: f32,
    frame_scene: bool,
    initial: Option<Pose>,
}

/// Off-live camera components and resources; no framing occurs during construction.
pub struct CameraCandidate {
    active: Option<bool>,
    transform: Transform,
    projection: Projection,
    flycam: FlycamSettings,
    frame_scene: FrameScene,
    initial: Option<InitialView>,
}

impl CameraSnapshot {
    /// Return None only when no view camera exists; incomplete or multiple cameras reject.
    pub fn capture(world: &World) -> Result<Option<Self>> {
        let count = world
            .try_query_filtered::<Entity, With<ViewCamera>>()
            .map_or(0, |mut query| query.iter(world).count());
        if count == 0 {
            return Ok(None);
        }
        ensure!(
            count == 1,
            "camera snapshot requires exactly one view camera"
        );
        let mut query = world
            .try_query_filtered::<(Entity, &Transform, &Projection), With<ViewCamera>>()
            .context("camera components absent")?;
        let (entity, pose, projection) = query
            .single(world)
            .context("camera pose/projection absent")?;
        let Projection::Custom(custom) = projection else {
            anyhow::bail!("camera snapshot requires the installed game projection");
        };
        let GameProjection(projection) = custom
            .get()
            .context("camera projection implementation unsupported")?;
        let flycam = world
            .get_resource::<FlycamSettings>()
            .context("camera controls absent")?;
        let frame = world
            .get_resource::<FrameScene>()
            .context("camera framing owner absent")?;
        let saved = Self {
            version: 1,
            active: world
                .get::<bevy::prelude::Camera>(entity)
                .map(|camera| camera.is_active),
            pose: Pose::capture(pose),
            projection: [
                projection.fov,
                projection.aspect_ratio,
                projection.near,
                projection.far,
            ],
            near_clip_plane: projection.near_clip_plane.to_array(),
            flycam: flycam.enabled,
            speed: flycam.speed,
            frame_scene: frame.0,
            initial: world
                .get_resource::<InitialView>()
                .map(|view| Pose::capture(&view.0)),
        };
        saved.clone().restore()?;
        Ok(Some(saved))
    }

    pub fn restore(self) -> Result<CameraCandidate> {
        ensure!(self.version == 1, "camera snapshot version unsupported");
        let [fov, aspect_ratio, near, far] = self.projection;
        ensure!(
            self.projection.iter().all(|v| v.is_finite())
                && fov > 0.0
                && fov < std::f32::consts::PI
                && aspect_ratio > 0.0
                && near > 0.0
                && far > near,
            "camera projection outside supported range"
        );
        ensure!(
            self.speed.is_finite() && self.speed >= 0.0,
            "camera inspection speed outside supported range"
        );
        let near_clip_plane = Vec4::from_array(self.near_clip_plane);
        ensure!(
            near_clip_plane.is_finite() && near_clip_plane.truncate().is_normalized(),
            "camera near clip plane is invalid"
        );
        Ok(CameraCandidate {
            active: self.active,
            transform: self.pose.restore()?,
            projection: Projection::custom(GameProjection(PerspectiveProjection {
                fov,
                aspect_ratio,
                near,
                far,
                near_clip_plane,
            })),
            flycam: FlycamSettings {
                enabled: self.flycam,
                speed: self.speed,
            },
            frame_scene: FrameScene(self.frame_scene),
            initial: self
                .initial
                .map(|pose| pose.restore().map(InitialView))
                .transpose()?,
        })
    }
}
impl CameraCandidate {
    /// Saved camera position in rendered world units for off-live lighting preparation.
    pub fn position(&self) -> Vec3 {
        self.transform.translation
    }

    /// Queue onto the prepared camera entity during the complete session commit.
    pub fn queue(self, commands: &mut Commands, entity: Entity) {
        commands
            .entity(entity)
            .insert((ViewCamera, self.transform, self.projection));
        if let Some(active) = self.active {
            commands.queue(move |world: &mut World| {
                if let Some(mut camera) = world.get_mut::<bevy::prelude::Camera>(entity) {
                    camera.is_active = active;
                }
            });
        }
        commands.insert_resource(self.flycam);
        commands.insert_resource(self.frame_scene);
        if let Some(initial) = self.initial {
            commands.insert_resource(initial);
        } else {
            commands.remove_resource::<InitialView>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_replacement_retains_pose_projection_and_pending_framing() {
        let mut original = World::new();
        original.insert_resource(FlycamSettings {
            enabled: true,
            speed: 72.0,
        });
        original.insert_resource(FrameScene(true));
        original.insert_resource(InitialView(Transform::from_xyz(11.0, 22.0, 33.0)));
        original.spawn((
            ViewCamera,
            Transform::from_xyz(17.0, -2.0, 93.0),
            Projection::custom(GameProjection::default()),
        ));
        let saved = CameraSnapshot::capture(&mut original).unwrap().unwrap();
        let bytes = serde_json::to_vec(&saved).unwrap();
        let mut invalid = serde_json::to_value(&saved).unwrap();
        invalid["projection"][2] = (-1).into();
        assert!(
            serde_json::from_value::<CameraSnapshot>(invalid)
                .unwrap()
                .restore()
                .is_err()
        );
        let replacement = saved.restore().unwrap();
        let mut fresh = World::new();
        let camera = fresh.spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        replacement.queue(&mut Commands::new(&mut queue, &fresh), camera);
        assert!(CameraSnapshot::capture(&mut fresh).unwrap().is_none());
        queue.apply(&mut fresh);
        assert_eq!(
            serde_json::to_vec(&CameraSnapshot::capture(&mut fresh).unwrap().unwrap()).unwrap(),
            bytes
        );
        fresh.spawn(ViewCamera);
        assert!(CameraSnapshot::capture(&mut fresh).is_err());
    }
}
