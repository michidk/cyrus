//! Shared native/browser graphics, camera, menu, HUD and cinematic presentation.
pub mod animation;
pub mod camera;
pub mod environment;
pub mod fog;
pub mod hud;
pub mod lighting;
#[cfg(not(target_arch = "wasm32"))]
pub mod loading;
pub mod player_model;
pub mod script_transform;
mod sky;
pub mod startup;
#[cfg(not(target_arch = "wasm32"))]
pub mod subtitles;
mod water;
use bevy::prelude::*;
/// Shared world presentation for native and browser hosts.
pub struct WorldRenderPlugin;
impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            camera::CameraPlugin,
            animation::AnimationRenderPlugin,
            script_transform::ScriptTransformPlugin,
            player_model::PlayerModelPlugin,
            environment::EnvironmentPlugin,
        ))
        .add_systems(Update, audio_listener.before(cyrus_audio::ApplyPreferences))
        .add_systems(
            Update,
            scene_presentation.after(cyrus_world::lifecycle::WorldChange),
        );
    }
}
#[derive(Component)]
struct SceneEntity;
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

fn audio_listener(
    mut commands: Commands,
    camera: Query<&GlobalTransform, With<camera::ViewCamera>>,
) {
    if let Ok(pose) = camera.single() {
        commands.insert_resource(cyrus_audio::AudioListener(*pose));
    } else {
        commands.remove_resource::<cyrus_audio::AudioListener>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_listener_requires_one_camera_and_clears_stale_pose() {
        let mut app = App::new();
        app.add_systems(Update, audio_listener);
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_audio::AudioListener>()
        );
        let pose = GlobalTransform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let first = app.world_mut().spawn((camera::ViewCamera, pose)).id();
        app.update();
        assert_eq!(app.world().resource::<cyrus_audio::AudioListener>().0, pose);
        let second = app
            .world_mut()
            .spawn((camera::ViewCamera, GlobalTransform::IDENTITY))
            .id();
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_audio::AudioListener>()
        );
        app.world_mut().despawn(second);
        app.update();
        assert!(
            app.world()
                .contains_resource::<cyrus_audio::AudioListener>()
        );
        app.world_mut().despawn(first);
        app.update();
        assert!(
            !app.world()
                .contains_resource::<cyrus_audio::AudioListener>()
        );
    }
}
