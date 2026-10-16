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
