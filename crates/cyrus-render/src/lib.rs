//! Shared native/browser graphics, camera, menu, HUD and audio presentation.
pub mod animation;
pub mod camera;
pub mod environment;
pub mod fog;
pub mod hud;
pub mod lighting;
#[cfg(not(target_arch = "wasm32"))]
pub mod loading;
pub mod music;
pub mod player_model;
pub mod script_transform;
mod sky;
pub mod sound;
#[cfg(target_arch = "wasm32")]
mod sound_bank;
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
        WorldAssetRoot(
            assets.load(GltfAssetLabel::Scene(0).from_asset(prepared.scene_path.clone())),
        ),
    ));
    commands.spawn((
        SceneEntity,
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
}
