//! Marker-based entry from preservation's player and RGM documentation.
use super::Player;
use anyhow::Context;
use bevy::prelude::{
    AssetId, Assets, Commands, Component, Entity, Local, Query, Res, Resource, SystemSet,
    Transform, Vec3, With,
};
use cyrus_world::animation::{ActorAnimation, AnimationRequest, AnimationSound, Animations};
use serde::Deserialize;

/// Explicit marker entry for scene inspection. Scripted world transitions are not implemented.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkerEntry {
    pub marker: usize,
    /// Original heading units, wrapped modulo 2048 during placement.
    pub heading: i32,
}

/// Runtime player placement. Positions retain eight fractional bits.
#[derive(Component, Debug, PartialEq, Eq)]
pub struct PlayerPose {
    pub position: [i64; 3],
    pub heading: u16,
}

/// Marker placement must finish before presentation supplies an ordinary actor view.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerPlacement;

/// Apply startup settling against the prepared static world and dry terrain.
/// Caller must account for dynamic/transient support separately before enabling gameplay.
pub fn settle_static(
    scene: &super::data::PlayerScene,
    pose: &mut PlayerPose,
) -> anyhow::Result<()> {
    let definition = scene.definition.as_ref().with_context(|| {
        format!(
            "world {} startup settling: player definition unavailable",
            scene.world
        )
    })?;
    let terrain = scene
        .terrain
        .as_ref()
        .and_then(|terrain| terrain.sample(pose.position));
    if let Some(support) = scene.static_collision.startup_support(
        pose.position,
        definition.positive_extent[1],
        terrain,
    )? {
        pose.position[1] = support.actor_height(
            definition.positive_extent[1],
            scene.settings.standing_height,
        );
    } else {
        anyhow::ensure!(
            scene.terrain.is_none() || terrain.is_some(),
            "world {} startup settling at {:?}: water or terrain edge unsupported without model support",
            scene.world,
            pose.position
        );
    }
    Ok(())
}

/// Initialize the animated CYRUS definition using preservation player.md's entry order.
/// Every request reuses the caller's nominal ticks. The returned file frame remains
/// selected even while presentation has not yet resolved its model reference.
pub fn initial_animation(
    data: &Animations,
    nominal_ticks: u32,
    sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
) -> anyhow::Result<(ActorAnimation, Option<u32>)> {
    initial_role_animation(data, "CYRUS", nominal_ticks, sounds)
}

pub(crate) fn initial_role_animation(
    data: &Animations,
    actor: &str,
    nominal_ticks: u32,
    sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
) -> anyhow::Result<(ActorAnimation, Option<u32>)> {
    let (mut animation, mut frame) =
        ActorAnimation::initialize(data, actor, nominal_ticks, sounds)?;
    frame = animation
        .force_request_with_sounds(0, 0, nominal_ticks, sounds)?
        .or(frame);
    Ok((animation, frame))
}

#[derive(Deserialize)]
struct Marker {
    map_position: [i32; 3],
    position: [f32; 3],
    reserved: u8,
}

#[derive(PartialEq)]
pub(super) struct AppliedEntry {
    entity: Entity,
    animation: AssetId<Animations>,
    scene: AssetId<super::data::PlayerScene>,
    marker: MarkerEntry,
}

pub(crate) fn placement(
    data: &Animations,
    entry: MarkerEntry,
) -> anyhow::Result<(PlayerPose, Vec3)> {
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
    scene: (
        Option<Res<super::data::PlayerSceneRequest>>,
        Option<Res<Assets<super::data::PlayerScene>>>,
    ),
    mut player: Query<(Entity, &mut Transform), With<Player>>,
    mut applied: Local<Option<AppliedEntry>>,
) {
    let (Some(request), Some(assets), Ok((entity, mut transform))) =
        (request, assets, player.single_mut())
    else {
        return;
    };
    let (Some(scene_request), Some(scenes)) = scene else {
        commands.entity(entity).remove::<PlayerPose>();
        return;
    };
    let key = AppliedEntry {
        entity,
        animation: request.0.id(),
        scene: scene_request.0.id(),
        marker: *entry,
    };
    if applied.as_ref() == Some(&key) {
        return;
    }
    // Old-world coordinates must not be used while the next world's data is loading.
    commands.entity(entity).remove::<PlayerPose>();
    let Some(data) = assets.get(&request.0) else {
        return;
    };
    let Some(scene) = scenes.get(&scene_request.0) else {
        return;
    };
    if scene.world != data.world {
        return;
    }
    match placement(data, *entry) {
        Ok((mut pose, mut position)) => {
            let initial_y = pose.position[1];
            if let Err(error) = settle_static(scene, &mut pose) {
                // shortcut: retain inspection marker until dynamic/water support is implemented.
                error!("player settling unavailable; retaining marker: {error:#}");
            }
            position.y -= (pose.position[1] - initial_y) as f32 / 5120.0;
            transform.translation = position;
            commands.entity(entity).insert(pose);
        }
        Err(error) => error!("player entry: {error:#}"),
    }
    *applied = Some(key);
}

use log::error;
