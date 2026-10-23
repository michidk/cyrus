//! Marker-based entry from preservation's player and RGM documentation.
mod persistence;
use super::Player;
use anyhow::Context;
use bevy::prelude::{
    AssetId, Assets, Commands, Component, Entity, Local, Query, Res, Resource, SystemSet,
    Transform, Vec3, With,
};
use cyrus_world::animation::{ActorAnimation, AnimationRequest, AnimationSound, Animations};
pub use persistence::{PlayerPlacementCandidate, PlayerPlacementSnapshot};
use serde::Deserialize;

/// Explicit marker entry for scene inspection. Scripted world transitions are not implemented.
#[derive(
    Resource, Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
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

/// Result of marker settling for this presentation entry. A retained inspection
/// pose does not establish ground support or admit automatic player movement.
#[derive(Component, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlayerEntrySupport {
    /// Static settling completed; dynamic support and controller guards remain separate.
    StaticSettled,
    /// The marker remains visible for inspection after unsupported settling.
    InspectionOnly(String),
}

/// Marker placement must finish before presentation supplies an ordinary actor view.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerPlacement;

/// Startup settling ranks the live models of the ordinary actors too, which only the script world
/// can pose. Returned (as the source of an `anyhow` error) when the static pass alone cannot decide.
#[derive(Debug)]
pub struct ActorModelSupportRequired(pub String);

impl std::fmt::Display for ActorModelSupportRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ActorModelSupportRequired {}

/// Apply startup settling against the prepared static world and terrain (water cells included).
/// Refuses water-terrain support while the world has actor models, since a deck among them could
/// outrank it ([`ActorModelSupportRequired`]); [`settle_with_actors`] decides those cases.
pub fn settle_static(
    scene: &super::data::PlayerScene,
    pose: &mut PlayerPose,
) -> anyhow::Result<()> {
    settle(scene, pose, None)
}

/// Startup settling with the contacts of the live actor models, in candidate order.
pub fn settle_with_actors(
    scene: &super::data::PlayerScene,
    pose: &mut PlayerPose,
    actors: Vec<cyrus_collision::ground::ModelSupport>,
) -> anyhow::Result<()> {
    settle(scene, pose, Some(actors))
}

fn settle(
    scene: &super::data::PlayerScene,
    pose: &mut PlayerPose,
    actors: Option<Vec<cyrus_collision::ground::ModelSupport>>,
) -> anyhow::Result<()> {
    let definition = scene.definition.as_ref().with_context(|| {
        format!(
            "world {} startup settling: player definition unavailable",
            scene.world
        )
    })?;
    // Startup settling reads the terrain height the clearance probe reads: water cells count, and
    // only the height ranks (the normal is unused here).
    let terrain = scene.terrain.as_ref().and_then(|terrain| {
        terrain
            .height(pose.position)
            .map(|height| cyrus_collision::ground::GroundSupport {
                height,
                normal: [0; 3],
            })
    });
    let actors_known = actors.is_some();
    if let Some(support) = scene.static_collision.startup_support(
        pose.position,
        definition.positive_extent[1],
        terrain,
        actors.unwrap_or_default(),
    )? {
        if !actors_known
            && support.source() == cyrus_collision::ground::SupportSource::Terrain
            && scene
                .terrain
                .as_ref()
                .is_some_and(|terrain| terrain.sample(pose.position).is_none())
            && scene
                .actor_collision
                .as_ref()
                .is_some_and(|models| models.has_models())
        {
            return Err(anyhow::Error::new(ActorModelSupportRequired(format!(
                "world {} startup settling at {:?}: water or terrain edge unsupported without actor model support",
                scene.world, pose.position
            ))));
        }
        pose.position[1] = support.actor_height(
            definition.positive_extent[1],
            scene.settings.standing_height,
        );
    } else {
        anyhow::ensure!(
            scene.terrain.is_none() || terrain.is_some(),
            "world {} startup settling at {:?}: terrain height-grid edge unsupported without model support",
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
    mut player: Query<(Entity, &mut Transform, Option<&persistence::RestoredEntry>), With<Player>>,
    mut applied: Local<Option<AppliedEntry>>,
) {
    let (Some(request), Some(assets), Ok((entity, mut transform, restored))) =
        (request, assets, player.single_mut())
    else {
        return;
    };
    let (Some(scene_request), Some(scenes)) = scene else {
        commands
            .entity(entity)
            .remove::<(PlayerPose, PlayerEntrySupport)>();
        return;
    };
    let key = AppliedEntry {
        entity,
        animation: request.0.id(),
        scene: scene_request.0.id(),
        marker: *entry,
    };
    if let Some(restored) = restored {
        commands
            .entity(entity)
            .remove::<persistence::RestoredEntry>();
        if restored.animation == key.animation
            && restored.scene == key.scene
            && restored.marker == key.marker
        {
            *applied = Some(key);
            return;
        }
    }
    if applied.as_ref() == Some(&key) {
        return;
    }
    // Old-world coordinates must not be used while the next world's data is loading.
    commands
        .entity(entity)
        .remove::<(PlayerPose, PlayerEntrySupport)>();
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
            let support = match settle_static(scene, &mut pose) {
                Ok(()) => PlayerEntrySupport::StaticSettled,
                Err(error) => {
                    // The script world settles against its actor models when it initializes the
                    // entry; until then the marker is retained.
                    if error.downcast_ref::<ActorModelSupportRequired>().is_some() {
                        log::debug!("player settling deferred to the script world: {error:#}");
                    } else {
                        error!("player settling unavailable; retaining marker: {error:#}");
                    }
                    PlayerEntrySupport::InspectionOnly(format!("{error:#}"))
                }
            };
            position.y -= (pose.position[1] - initial_y) as f32 / 5120.0;
            transform.translation = position;
            commands.entity(entity).insert((pose, support));
        }
        Err(error) => error!("player entry: {error:#}"),
    }
    *applied = Some(key);
}

use log::error;
