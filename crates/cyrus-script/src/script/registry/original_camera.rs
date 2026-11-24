//! Original camera restoration shares the initialized actor ownership census.
use super::*;
use crate::script::camera::{CameraBinding, CameraFlags, CameraRecord, CameraState};
use rgpre::import::savegame::{
    SaveActorBinding, SaveActorOwners, SaveCameraGlobals, SaveCameras, SaveObject,
};

impl ScriptWorld {
    /// Prepare logical camera records, poses and original-load finalization before
    /// replacing the off-live camera service. Saved pointer words never select actors.
    /// Call after native actor restoration and before admitting the complete session.
    pub fn restore_original_camera(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
        saved: &SaveCameras,
        globals: &SaveCameraGlobals,
    ) -> Result<()> {
        let initialized_camera = self
            .camera
            .as_ref()
            .context("original camera service absent")?;
        let mut initialized = Vec::with_capacity(bindings.len());
        let mut objects = Vec::with_capacity(bindings.len());
        let mut receivers = Vec::with_capacity(bindings.len());
        let mut seen = BTreeSet::new();
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            anyhow::ensure!(
                seen.insert(index),
                "original camera duplicate actor receiver {index}"
            );
            let actor = &self.actors[index];
            let expected = match &actor.source {
                ScriptActorSource::Player => -2,
                ScriptActorSource::AlternatePlayer => -3,
                ScriptActorSource::Camera => 2,
                ScriptActorSource::Map(placement)
                    if !placement.object_type.has_separate_record() =>
                {
                    i32::from(
                        actor
                            .runtime_identifier
                            .context("original camera actor runtime identity absent")?
                            as u16,
                    )
                }
                _ => anyhow::bail!("original camera actor {index}: unsupported saved owner"),
            };
            anyhow::ensure!(
                expected == object.object_id,
                "original camera actor {index}: saved identity differs"
            );
            initialized.push(SaveActorBinding {
                object_id: expected,
                runtime_identifier: actor
                    .runtime_identifier
                    .context("original camera runtime identity absent")?
                    as u16,
            });
            objects.push(object.clone());
            receivers.push(id);
        }
        let owners = SaveActorOwners::bind(&objects, &initialized)?;
        let bound = saved.bind(
            &owners,
            std::array::from_fn(|slot| initialized_camera.records[slot].angle_offset),
        )?;
        let mut records = Vec::with_capacity(9);
        for (slot, record) in bound.into_iter().enumerate() {
            let record = record.with_context(|| {
                format!("original camera slot {slot}: unbound record is unsupported")
            })?;
            let values = record.values;
            records.push(CameraRecord {
                actor: receivers[record.actor_receiver],
                binding: CameraBinding::LocalPose,
                position_offset: values.position_offset,
                angle_offset: values.angle_offset,
                request: values.request,
                target_offset: values.target_offset,
                bound_position_history: values.bound_position_history,
                basis: values.basis,
                distance: values.distance,
                minimum_distance: values.minimum_distance,
                maximum_movement: values.maximum_movement,
                flags: CameraFlags::from_bits(values.flags as u8),
            });
        }
        let records: [CameraRecord; 9] = records
            .try_into()
            .map_err(|_| anyhow::anyhow!("original camera record census differs"))?;
        let next = CameraState::from_original(records, globals, initialized_camera.camera_actor)?;
        anyhow::ensure!(
            globals.obstacle_size_q8 & 255 == 0,
            "original camera fractional obstacle size is unsupported"
        );
        let vertical_limit = globals
            .maximum_vertical_velocity
            .checked_neg()
            .context("original camera vertical velocity cannot be represented")?;
        let mut limits = self.camera_limits;
        limits.obstacle_size = globals.obstacle_size_q8 >> 8;
        limits.max_acc = globals.maximum_acceleration;
        limits.max_y_vel = vertical_limit;
        self.camera_limits = limits;
        self.camera_settings.limits = limits;
        self.camera_settings.static_rope_threshold = globals.static_rope_threshold;
        self.camera_settings.static_rope_aim = globals.static_rope_aim;
        self.camera = Some(next);
        Ok(())
    }
}
