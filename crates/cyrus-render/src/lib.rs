//! Shared native/browser graphics, camera, HUD and subtitles.
pub mod animation;
pub mod camera;
pub mod console;
mod console_environment;
pub mod environment;
mod flat;
pub use flat::prepare_scene_flats;
pub mod fog;
pub mod hud;
pub mod lighting;
pub mod persistence;
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
pub use water::{
    CameraTerrainView, PreparedTerrainMesh, TerrainCandidate, TerrainMeshReplacement,
    TerrainPlaybackSnapshot, WaterPlayback,
};
/// Shared world presentation for native and browser hosts.
pub struct WorldRenderPlugin;
impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        flat::register(app);
        app.add_plugins(persistence::SceneIdentityPlugin);
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
    restored: Option<Res<persistence::scene::RestoredScene>>,
    old: Query<Entity, With<SceneEntity>>,
    mut frame: ResMut<camera::FrameScene>,
) {
    let Some(prepared) = prepared else {
        return;
    };
    if !prepared.is_changed() {
        return;
    }
    if let Some(restored) = restored {
        commands.remove_resource::<persistence::scene::RestoredScene>();
        if restored.matches(&prepared) {
            return;
        }
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

#[cfg(test)]
mod session_scene_tests {
    use super::*;
    #[test]
    fn adopted_scene_skips_first_prepared_change_without_reframing() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .insert_resource(camera::FrameScene(false))
            .add_systems(Update, scene_presentation);
        let prepared = cyrus_world::lifecycle::PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(17),
            animations: Handle::default(),
            scripts: Handle::default(),
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        };
        app.insert_resource(persistence::scene::RestoredScene::new(&prepared));
        app.insert_resource(prepared);
        let root = app.world_mut().spawn(SceneEntity).id();
        app.update();
        assert!(app.world().get::<SceneEntity>(root).is_some());
        assert!(!app.world().resource::<camera::FrameScene>().0);
        assert!(
            !app.world()
                .contains_resource::<persistence::scene::RestoredScene>()
        );
        app.update();
        assert!(app.world().get::<SceneEntity>(root).is_some());
        assert!(!app.world().resource::<camera::FrameScene>().0);
    }
}
