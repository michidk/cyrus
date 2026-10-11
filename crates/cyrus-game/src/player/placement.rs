//! Marker-based entry from preservation's player and RGM documentation.
use super::Player;
use bevy::prelude::*;
use cyrus_world::animation::{AnimationRequest, Animations};
use serde::Deserialize;

/// Explicit marker entry for scene inspection. Scripted world transitions are not implemented.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkerEntry {
    pub marker: usize,
    /// Original heading units, wrapped modulo 2048 during placement.
    pub heading: i32,
}

/// Initial actor placement, before ground settling. Positions retain eight fractional bits.
#[derive(Component, Debug, PartialEq, Eq)]
pub struct PlayerPose {
    pub position: [i64; 3],
    pub heading: u16,
}

#[derive(Deserialize)]
struct Marker {
    map_position: [i32; 3],
    position: [f32; 3],
    reserved: u8,
}

fn placement(data: &Animations, entry: MarkerEntry) -> anyhow::Result<(PlayerPose, Vec3)> {
    let context = format!("world {} marker {}", data.world, entry.marker);
    let marker = data.metadata["markers"]
        .as_array()
        .and_then(|markers| markers.get(entry.marker))
        .ok_or_else(|| anyhow::anyhow!("{context}: missing marker"))?;
    let marker: Marker = serde_json::from_value(marker.clone()).map_err(|error| {
        anyhow::anyhow!("{context}: invalid marker; re-export the world: {error}")
    })?;
    anyhow::ensure!(marker.reserved == 1, "{context}: marker is unavailable");
    anyhow::ensure!(
        marker.position.iter().all(|value| value.is_finite()),
        "{context}: nonfinite export position"
    );
    anyhow::ensure!(
        marker
            .map_position
            .iter()
            .all(|value| (-8_388_608..=8_388_607).contains(value)),
        "{context}: map coordinate exceeds signed 24-bit range"
    );
    Ok((
        PlayerPose {
            position: marker.map_position.map(|value| i64::from(value) * 256),
            heading: entry.heading.rem_euclid(2048) as u16,
        },
        Vec3::from_array(marker.position),
    ))
}

pub(super) fn enter_marker(
    mut commands: Commands,
    request: Option<Res<AnimationRequest>>,
    assets: Option<Res<Assets<Animations>>>,
    entry: Res<MarkerEntry>,
    mut player: Query<(Entity, &mut Transform), With<Player>>,
    mut applied: Local<Option<(Entity, AssetId<Animations>, MarkerEntry)>>,
) {
    let (Some(request), Some(assets), Ok((entity, mut transform))) =
        (request, assets, player.single_mut())
    else {
        return;
    };
    let key = (entity, request.0.id(), *entry);
    if *applied == Some(key) {
        return;
    }
    // Old-world coordinates must not be used while the next world's data is loading.
    commands.entity(entity).remove::<PlayerPose>();
    let Some(data) = assets.get(&request.0) else {
        return;
    };
    match placement(data, *entry) {
        Ok((pose, position)) => {
            transform.translation = position;
            commands.entity(entity).insert(pose);
        }
        Err(error) => error!("player entry: {error:#}"),
    }
    *applied = Some(key);
}

use log::error;
