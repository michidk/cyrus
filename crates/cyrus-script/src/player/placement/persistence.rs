//! Off-live player presentation replacement, preserving marker-entry completion.
use super::{MarkerEntry, PlayerEntrySupport, PlayerPose};
use anyhow::{Result, ensure};
use bevy::prelude::*;
use cyrus_assets::animation::Animations;
use serde::{Deserialize, Serialize};

/// Presentation paired with the separately restored authoritative logical player.
#[derive(Clone, Serialize, Deserialize)]
pub struct PlayerPlacementSnapshot {
    version: u32,
    world: u32,
    marker: MarkerEntry,
    position: [i64; 3],
    heading: u16,
    support: PlayerEntrySupport,
    translation: [u32; 3],
    rotation: [u32; 4],
    scale: [u32; 3],
}

/// Insert with the prepared player entity during the complete session commit.
/// Construction performs no marker settling, initialization, scripts or audio.
#[derive(Bundle)]
pub struct PlayerPlacementCandidate {
    pose: PlayerPose,
    support: PlayerEntrySupport,
    transform: Transform,
    restored: RestoredEntry,
}

impl PlayerPlacementCandidate {
    /// Relocate an off-live candidate without inventing saved ground ownership.
    /// Ordinary collision queries must establish support after publication.
    #[cfg(feature = "original-save-import")]
    pub fn import_original_pose(&mut self, position_q8: [i32; 3], heading: u16) -> Result<()> {
        ensure!(
            heading < 2048,
            "original player placement heading outside wrapped range"
        );
        let delta = std::array::from_fn::<_, 3, _>(|axis| {
            (i64::from(position_q8[axis]) - self.pose.position[axis]) as f32
                / (256.0 * cyrus_game::ENGINE_UNIT_SCALE)
        });
        self.transform.translation += Vec3::new(-delta[0], -delta[1], delta[2]);
        self.pose.position = position_q8.map(i64::from);
        self.pose.heading = heading;
        self.support = PlayerEntrySupport::InspectionOnly(
            "Original save ground/controller continuation is unsupported.".into(),
        );
        Ok(())
    }
}

#[derive(Component)]
pub(crate) struct RestoredEntry {
    pub animation: AssetId<Animations>,
    pub scene: AssetId<super::super::data::PlayerScene>,
    pub marker: MarkerEntry,
}

impl PlayerPlacementSnapshot {
    /// Capture after pose projection at a completed scheduler boundary.
    pub fn capture(
        world: u32,
        marker: MarkerEntry,
        pose: &PlayerPose,
        support: &PlayerEntrySupport,
        transform: &Transform,
    ) -> Result<Self> {
        validate(pose.heading, transform)?;
        Ok(Self {
            version: 1,
            world,
            marker,
            position: pose.position,
            heading: pose.heading,
            support: support.clone(),
            translation: transform.translation.to_array().map(f32::to_bits),
            rotation: transform.rotation.to_array().map(f32::to_bits),
            scale: transform.scale.to_array().map(f32::to_bits),
        })
    }

    /// Marker selection to restore with this candidate's presentation resources.
    pub fn marker_entry(&self) -> MarkerEntry {
        self.marker
    }

    /// The host must validate the complete world's source identity before supplying
    /// its matching prepared animation/player-scene handles here.
    pub fn restore(
        self,
        expected_world: u32,
        animation: Handle<Animations>,
        scene: Handle<super::super::data::PlayerScene>,
    ) -> Result<PlayerPlacementCandidate> {
        ensure!(
            self.version == 1,
            "player placement snapshot version unsupported"
        );
        ensure!(
            self.world == expected_world,
            "player placement world {} differs from {expected_world}",
            self.world
        );
        let transform = Transform {
            translation: Vec3::from_array(self.translation.map(f32::from_bits)),
            rotation: Quat::from_array(self.rotation.map(f32::from_bits)),
            scale: Vec3::from_array(self.scale.map(f32::from_bits)),
        };
        validate(self.heading, &transform)?;
        Ok(PlayerPlacementCandidate {
            pose: PlayerPose {
                position: self.position,
                heading: self.heading,
            },
            support: self.support,
            transform,
            restored: RestoredEntry {
                animation: animation.id(),
                scene: scene.id(),
                marker: self.marker,
            },
        })
    }
}

fn validate(heading: u16, transform: &Transform) -> Result<()> {
    ensure!(
        heading < 2048,
        "player placement heading outside wrapped range"
    );
    ensure!(
        transform.translation.is_finite()
            && transform.rotation.is_finite()
            && transform.scale.is_finite()
            && transform.rotation.is_normalized(),
        "player placement transform invalid"
    );
    Ok(())
}

#[cfg(all(test, feature = "original-save-import"))]
mod original_import_tests {
    use super::*;

    #[test]
    fn original_pose_relocates_only_the_off_live_candidate() {
        let original = PlayerPose {
            position: [0; 3],
            heading: 0,
        };
        let snapshot = PlayerPlacementSnapshot::capture(
            15,
            MarkerEntry::default(),
            &original,
            &PlayerEntrySupport::StaticSettled,
            &Transform::default(),
        )
        .unwrap();
        let mut candidate = snapshot
            .restore(15, Handle::default(), Handle::default())
            .unwrap();
        candidate
            .import_original_pose([5120, -5120, 10240], 518)
            .unwrap();
        assert!(candidate.import_original_pose([0; 3], 2048).is_err());
        assert_eq!(candidate.pose.position, [5120, -5120, 10240]);
        assert_eq!(candidate.pose.heading, 518);
        assert_eq!(candidate.transform.translation, Vec3::new(-1.0, 1.0, 2.0));
        assert!(matches!(
            candidate.support,
            PlayerEntrySupport::InspectionOnly(_)
        ));
        assert_eq!(original.position, [0; 3]);
    }
}
