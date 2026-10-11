//! Inspection camera and the player input foundation. Faithful locomotion is
//! unsupported until animation, collision and timing are established.

pub mod contact;
pub mod data;
pub mod definition;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod export;
pub mod geometry;
pub mod ground;
pub mod input;
pub mod motion;
pub mod placement;
pub mod turn;
pub mod walk;

use crate::AppState;
use bevy::camera::primitives::Aabb;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;

pub struct PlayerPlugin;

#[derive(Component)]
pub struct Player;

/// The view camera is independent of the player transform.
#[derive(Component)]
pub struct ViewCamera;

/// Debug inspection controls, independent of fidelity settings.
#[derive(Resource)]
pub struct FlycamSettings {
    pub enabled: bool,
    pub speed: f32,
}

impl Default for FlycamSettings {
    fn default() -> Self {
        // Inspection starts enabled while gameplay locomotion is unsupported.
        Self {
            enabled: true,
            speed: 150.0,
        }
    }
}

/// Frame the inspection camera on a freshly loaded scene, without moving the player.
#[derive(Resource, Default)]
pub struct FrameScene(pub bool);

const MOUSE_SENSITIVITY: f32 = 0.0025;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        if app.world().contains_resource::<AssetServer>() {
            app.init_asset::<data::PlayerScene>()
                .init_asset_loader::<data::PlayerSceneLoader>();
        }
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Startup, input::load);
        app.init_resource::<input::PlayerBindings>()
            .init_resource::<input::PlayerInput>()
            .add_systems(Update, input::sample)
            .init_resource::<crate::fog::RenderDistance>()
            .init_resource::<placement::MarkerEntry>()
            .init_resource::<FrameScene>()
            .init_resource::<FlycamSettings>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(
                Update,
                (
                    placement::enter_marker,
                    frame_scene.run_if(|f: Res<FrameScene>, camera: Res<FlycamSettings>| {
                        f.0 && camera.enabled
                    }),
                    flycam_controller.run_if(|f: Res<FlycamSettings>| f.enabled),
                    follow_player.run_if(|f: Res<FlycamSettings>| !f.enabled),
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn spawn(mut commands: Commands) {
    commands.spawn((Player, Transform::default()));
    commands.spawn((
        ViewCamera,
        Camera3d::default(),
        Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::new(100.0, 40.0, 100.0), Vec3::Y),
    ));
}

// The scene-bounds query excludes the fullscreen sky.
#[allow(clippy::type_complexity)]
fn frame_scene(
    mut frame: ResMut<FrameScene>,
    meshes: Query<(&Aabb, &GlobalTransform), (With<Mesh3d>, Without<crate::sky::Sky>)>,
    mut player: Query<&mut Transform, With<ViewCamera>>,
    distance: Res<crate::fog::RenderDistance>,
    fidelity: Res<crate::fidelity::Fidelity>,
) {
    let Some([_, far]) = distance.effective(&fidelity) else {
        return;
    };
    if meshes.is_empty() {
        return;
    }
    let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
    for (aabb, transform) in &meshes {
        let center = transform.transform_point(aabb.center.into());
        let extent = Vec3::from(aabb.half_extents) * transform.compute_transform().scale.abs();
        min = min.min(center - extent);
        max = max.max(center + extent);
    }
    let Ok(mut t) = player.single_mut() else {
        return;
    };
    let center = (min + max) / 2.0;
    let size = (max - min).length().max(1.0);
    let offset = Vec3::new(0.0, size * 0.15, size * 0.35).clamp_length_max(far / 20.0 * 0.5);
    t.translation = center + offset;
    t.look_at(center, Vec3::Y);
    frame.0 = false;
}

/// WASD + mouse look while the right button is held, Space/Shift for up/down.
fn flycam_controller(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    flycam: Res<FlycamSettings>,
    time: Res<Time>,
    mut player: Query<&mut Transform, With<ViewCamera>>,
) {
    let Ok(mut t) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    if buttons.pressed(MouseButton::Right) && mouse.delta != Vec2::ZERO {
        let (mut yaw, mut pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
        yaw -= mouse.delta.x * MOUSE_SENSITIVITY;
        pitch = (pitch - mouse.delta.y * MOUSE_SENSITIVITY).clamp(-1.5, 1.5);
        t.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir += *t.forward();
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= *t.forward();
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += *t.right();
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= *t.right();
    }
    if keys.pressed(KeyCode::Space) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::ShiftLeft) {
        dir -= Vec3::Y;
    }
    if dir != Vec3::ZERO {
        t.translation += dir.normalize() * flycam.speed * dt;
    }
}

// shortcut: the player view uses its pose directly until the original follow camera is implemented.
fn follow_player(
    player: Query<&Transform, (With<Player>, Without<ViewCamera>)>,
    mut camera: Query<&mut Transform, With<ViewCamera>>,
) {
    if let (Ok(player), Ok(mut camera)) = (player.single(), camera.single_mut()) {
        *camera = *player;
    }
}
