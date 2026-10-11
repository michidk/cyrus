//! Shared world identity and prepared cache references.
use crate::animation::AnimationRequest;
use bevy::{asset::AssetPath, prelude::*};
use cyrus_assets::animation::Animations;
use cyrus_assets::script::{ScriptLoader, Scripts};

/// WORLD.INI ID or map stem, retaining the control protocol's scene string.
#[derive(Resource, Debug, Clone)]
pub struct SceneRequest(pub String);
#[derive(Resource, Debug, Clone, Default)]
pub struct CurrentScene {
    pub name: String,
    /// Resolved WORLD.INI identity, distinct from an alias or numeric request string.
    pub world: Option<u32>,
}
/// A host has completed preparation; sidecar loading remains asynchronous.
#[derive(Resource)]
pub struct PreparedWorld {
    pub requested_name: String,
    /// Native preparation can resolve this immediately; browser metadata loads asynchronously.
    pub world: Option<u32>,
    pub animations: Handle<Animations>,
    /// Script records do not require a rendered mesh.
    pub scripts: Handle<Scripts>,
    pub player_path: AssetPath<'static>,
    pub scene_path: AssetPath<'static>,
    pub environment_path: AssetPath<'static>,
}
impl PreparedWorld {
    /// Cache URLs are chosen once by the host and retained for every consumer.
    pub fn from_cache(name: String, assets: &AssetServer) -> Self {
        Self {
            world: None,
            animations: assets.load(format!("cache://{name}.animations")),
            scripts: assets.load(format!("cache://{name}.scripts")),
            player_path: format!("cache://{name}.player").into(),
            scene_path: format!("cache://{name}.glb").into(),
            environment_path: format!("cache://{name}.environment").into(),
            requested_name: name,
        }
    }
}
/// Commits prepared world identity before sidecar and presentation consumers.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldChange;
pub struct WorldPlugin;
impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Scripts>()
            .init_asset_loader::<ScriptLoader>()
            .init_resource::<CurrentScene>()
            .init_resource::<WorldMetadataStatus>()
            .add_systems(
                Update,
                (commit, metadata_status).chain().in_set(WorldChange),
            );
    }
}
fn commit(
    mut commands: Commands,
    prepared: Option<Res<PreparedWorld>>,
    mut current: ResMut<CurrentScene>,
) {
    let Some(prepared) = prepared else {
        return;
    };
    if !prepared.is_changed() {
        return;
    }
    commands.insert_resource(AnimationRequest(prepared.animations.clone()));
    current.name.clone_from(&prepared.requested_name);
    current.world = prepared.world;
}

/// Animation metadata readiness only; graphics and player sidecars load independently.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub enum WorldMetadataStatus {
    #[default]
    NotRequested,
    Loading,
    Loaded,
    Failed(String),
}
fn metadata_status(
    prepared: Option<Res<PreparedWorld>>,
    assets: Res<Assets<Animations>>,
    server: Res<AssetServer>,
    mut current: ResMut<CurrentScene>,
    mut status: ResMut<WorldMetadataStatus>,
) {
    let Some(prepared) = prepared else {
        return;
    };
    if let Some(data) = assets.get(&prepared.animations) {
        if current.world != Some(data.world) {
            current.world = Some(data.world);
        }
        status.set_if_neq(WorldMetadataStatus::Loaded);
    } else {
        status.set_if_neq(match server.get_load_state(prepared.animations.id()) {
            Some(bevy::asset::LoadState::Failed(error)) => WorldMetadataStatus::Failed(format!(
                "world {} animation metadata: {error}",
                prepared.requested_name
            )),
            _ => WorldMetadataStatus::Loading,
        });
    }
}
