//! Player and camera. Two controllers exist behind the fidelity switch:
//! `faithful` is a placeholder until the original scheme is derived from KEYS.INI
//! and observation of the original game; `modern` is WASD + mouse look.
//! Neither constant below is verified against the original.

use crate::AppState;
use crate::fidelity::{Area, Mode, in_mode};
use bevy::camera::primitives::Aabb;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;

pub struct PlayerPlugin;

#[derive(Component)]
pub struct Player;

#[derive(Resource)]
pub struct FlycamSettings {
    pub speed: f32,
}

impl Default for FlycamSettings {
    fn default() -> Self {
        // unverified: inspection speed, adjustable from the debug menu.
        Self { speed: 100.0 }
    }
}

/// Set when a freshly spawned scene should place the player at its bounds.
/// shortcut: start positions come from the scene data in the original; until
/// that is read, the player is framed on the scene's bounding box.
#[derive(Resource, Default)]
pub struct FrameScene(pub bool);

// unverified placeholders; replace with values derived from the original.
const MOVE_SPEED: f32 = 12.0;
const TURN_SPEED: f32 = 2.0;
const MOUSE_SENSITIVITY: f32 = 0.0025;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameScene>()
            .init_resource::<FlycamSettings>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(
                Update,
                (
                    frame_scene.run_if(|f: Res<FrameScene>| f.0),
                    faithful_controller.run_if(in_mode(Area::PlayerController, Mode::Faithful)),
                    modern_controller.run_if(in_mode(Area::PlayerController, Mode::Modern)),
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn spawn(mut commands: Commands) {
    commands.spawn((
        Player,
        Camera3d::default(),
        Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::new(100.0, 40.0, 100.0), Vec3::Y),
    ));
}

// The scene-bounds query excludes the fullscreen sky.
#[allow(clippy::type_complexity)]
fn frame_scene(
    mut frame: ResMut<FrameScene>,
    meshes: Query<(&Aabb, &GlobalTransform), (With<Mesh3d>, Without<crate::sky::Sky>)>,
    mut player: Query<&mut Transform, With<Player>>,
) {
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
    t.translation = center + Vec3::new(0.0, size * 0.15, size * 0.35);
    t.look_at(center, Vec3::Y);
    frame.0 = false;
}

/// Tank-style: arrows turn and walk, no mouse.
fn faithful_controller(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player: Query<&mut Transform, With<Player>>,
) {
    let Ok(mut t) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let mut yaw = 0.0;
    if keys.pressed(KeyCode::ArrowLeft) {
        yaw += TURN_SPEED * dt;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        yaw -= TURN_SPEED * dt;
    }
    t.rotate_y(yaw);
    let forward = t.forward();
    let mut step = 0.0;
    if keys.pressed(KeyCode::ArrowUp) {
        step += MOVE_SPEED * dt;
    }
    if keys.pressed(KeyCode::ArrowDown) {
        step -= MOVE_SPEED * dt;
    }
    t.translation += forward * step;
}

/// WASD + mouse look while the right button is held, Space/Shift for up/down.
fn modern_controller(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    flycam: Res<FlycamSettings>,
    time: Res<Time>,
    mut player: Query<&mut Transform, With<Player>>,
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
