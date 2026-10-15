//! Shared native/browser graphics, camera, HUD and subtitles.
pub mod animation;
pub mod camera;
pub mod console;
mod console_environment;
pub mod environment;
mod flat;
pub mod fog;
pub mod hud;
pub mod lighting;
pub mod player_model;
pub mod presentation_clock;
mod sampling;
pub mod script_transform;
mod sky;
#[cfg(not(target_arch = "wasm32"))]
pub mod subtitles;
mod terrain;
pub use terrain::TextureAdmission as TerrainTextureAdmission;
mod water;
use bevy::prelude::*;
pub use water::{TerrainCandidate, TerrainMeshReplacement, TerrainPlaybackSnapshot, WaterPlayback};
/// Shared world presentation for native and browser hosts.
pub struct WorldRenderPlugin;
impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        flat::register(app);
        app.add_plugins((
            camera::CameraPlugin,
            console::ConsoleRenderPlugin,
            animation::AnimationRenderPlugin,
            script_transform::ScriptTransformPlugin,
            player_model::PlayerModelPlugin,
            environment::EnvironmentPlugin,
        ))
        .add_systems(
            Update,
            scene_presentation.after(cyrus_world::lifecycle::WorldChange),
        )
        .add_systems(
            Update,
            scene_ready
                .after(scene_presentation)
                .before(camera::script_view),
        );
    }
}
#[derive(Component)]
struct SceneEntity;

/// Current world whose rendered scene has finished spawning, before actor effects run.
/// The handle identifies that world's animation cache; presence does not establish gameplay readiness.
#[derive(Resource)]
pub struct ReadyScene(pub Handle<cyrus_assets::animation::Animations>);

fn scene_ready(
    mut commands: Commands,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    spawner: Res<bevy::world_serialization::WorldInstanceSpawner>,
    roots: Query<&bevy::world_serialization::WorldInstance, With<SceneEntity>>,
) {
    if let Some(prepared) = prepared
        && let Ok(instance) = roots.single()
        && spawner.instance_is_ready(**instance)
    {
        commands.insert_resource(ReadyScene(prepared.animations.clone()));
    } else {
        commands.remove_resource::<ReadyScene>();
    }
}
fn scene_presentation(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    old: Query<Entity, With<SceneEntity>>,
    mut frame: ResMut<camera::FrameScene>,
) {
    let Some(prepared) = prepared else {
        return;
    };
    if !prepared.is_changed() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.insert_resource(environment::EnvironmentRequest(
        assets.load(prepared.environment_path.clone()),
    ));
    frame.0 = true;
    commands.spawn((
        SceneEntity,
        cyrus_game::script::ScriptScene::new(prepared.animations.clone()),
        WorldAssetRoot(
            assets.load(GltfAssetLabel::Scene(0).from_asset(prepared.scene_path.clone())),
        ),
    ));
}
