//! Inspection camera, independent of gameplay pose and input.
pub(crate) mod projection;
use bevy::{camera::primitives::Aabb, input::mouse::AccumulatedMouseMotion, prelude::*};
use cyrus_game::AppState;
use cyrus_game::player::Player;
use cyrus_game::player::input::InputSample;
use cyrus_game::player::input::InputSuppressed;
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
        Self {
            enabled: false,
            speed: 150.0,
        }
    }
}

/// Frame the inspection camera on a freshly loaded scene, without moving the player.
#[derive(Resource, Default)]
pub struct FrameScene(pub bool);

/// One-shot inspection pose in exported scene coordinates, consumed on first framing.
#[derive(Resource)]
pub struct InitialView(pub Transform);

const MOUSE_SENSITIVITY: f32 = 0.0025;

// The scene-bounds query excludes the fullscreen sky.
#[allow(clippy::type_complexity)]
fn frame_scene(
    mut commands: Commands,
    initial: Option<Res<InitialView>>,
    mut frame: ResMut<FrameScene>,
    meshes: Query<(&Aabb, &GlobalTransform), (With<Mesh3d>, Without<crate::sky::Sky>)>,
    mut player: Query<&mut Transform, With<ViewCamera>>,
    distance: Res<crate::fog::RenderDistance>,
    fidelity: Res<cyrus_game::fidelity::Fidelity>,
) {
    if let Some(initial) = initial {
        if let Ok(mut camera) = player.single_mut() {
            *camera = initial.0;
            frame.0 = false;
            commands.remove_resource::<InitialView>();
        }
        return;
    }
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

pub struct CameraPlugin;
impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::fog::RenderDistance>()
            .init_resource::<FrameScene>()
            .init_resource::<FlycamSettings>()
            .init_resource::<InputSuppressed>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(Update, suppress.before(InputSample))
            .add_systems(
                Update,
                script_view
                    .after(follow_player)
                    .before(cyrus_game::script::OrdinaryUpdates)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    frame_scene.run_if(|f: Res<FrameScene>, camera: Res<FlycamSettings>| {
                        f.0 && camera.enabled
                    }),
                    flycam_controller.run_if(
                        |f: Res<FlycamSettings>, console: Option<Res<cyrus_console::Console>>| {
                            f.enabled && !console.is_some_and(|console| console.open)
                        },
                    ),
                    follow_player
                        .after(cyrus_game::player::placement::PlayerPlacement)
                        .run_if(|f: Res<FlycamSettings>| !f.enabled),
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}
pub(crate) fn script_view(
    mut commands: Commands,
    request: Option<Res<cyrus_world::animation::AnimationRequest>>,
    frame: Res<FrameScene>,
    ready: Option<Res<crate::ReadyScene>>,
    flycam: Res<FlycamSettings>,
    camera: Query<&Transform, With<ViewCamera>>,
    placed_player: Query<
        (),
        (
            With<Player>,
            With<cyrus_game::player::placement::PlayerPose>,
        ),
    >,
) {
    let (Some(request), Ok(camera)) = (request, camera.single()) else {
        commands.remove_resource::<cyrus_game::script::ScriptView>();
        return;
    };
    if ready.is_none_or(|ready| ready.0 != request.0)
        || (flycam.enabled && frame.0)
        || (!flycam.enabled && placed_player.single().is_err())
    {
        commands.remove_resource::<cyrus_game::script::ScriptView>();
        return;
    }
    let position = camera.translation;
    commands.insert_resource(cyrus_game::script::ScriptView {
        source: request.0.clone(),
        position: [
            (-f64::from(position.x) * 20.0 * 256.0).floor() as i32,
            (-f64::from(position.y) * 20.0 * 256.0).floor() as i32,
            ((f64::from(position.z) * 20.0 + 65535.99609375) * 256.0).floor() as i32,
        ],
    });
}

fn suppress(flycam: Res<FlycamSettings>, mut suppressed: ResMut<InputSuppressed>) {
    suppressed.0 = flycam.enabled;
}
fn spawn(
    mut commands: Commands,
    initial: Option<Res<InitialView>>,
    mut flycam: ResMut<FlycamSettings>,
) {
    if initial.is_some() {
        flycam.enabled = true;
    }
    commands.spawn((
        ViewCamera,
        Camera3d::default(),
        Projection::custom(projection::GameProjection::default()),
        Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::new(100.0, 40.0, 100.0), Vec3::Y),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_view_uses_camera_and_waits_for_scene_framing() {
        use cyrus_game::script::ScriptView;
        use cyrus_world::animation::AnimationRequest;
        let mut app = App::new();
        let source = Handle::default();
        app.insert_resource(AnimationRequest(source.clone()))
            .insert_resource(crate::ReadyScene(source.clone()))
            .insert_resource(FrameScene(true))
            .insert_resource(FlycamSettings {
                enabled: true,
                speed: 150.0,
            })
            .add_systems(Update, script_view);
        app.world_mut()
            .spawn((ViewCamera, Transform::from_xyz(-50.0, 0.0, -3000.0)));
        app.world_mut()
            .spawn((Player, Transform::from_xyz(-500.0, 0.0, -2000.0)));
        app.update();
        assert!(!app.world().contains_resource::<ScriptView>());
        app.world_mut().resource_mut::<FrameScene>().0 = false;
        app.world_mut().remove_resource::<crate::ReadyScene>();
        app.update();
        assert!(!app.world().contains_resource::<ScriptView>());
        app.insert_resource(crate::ReadyScene(source.clone()));
        app.update();
        let view = app.world().resource::<ScriptView>();
        assert_eq!(view.source, source);
        assert_eq!([view.position[0] >> 8, view.position[2] >> 8], [1000, 5535]);
        app.world_mut().resource_mut::<FlycamSettings>().enabled = false;
        app.world_mut().resource_mut::<FrameScene>().0 = true;
        app.update();
        assert!(!app.world().contains_resource::<ScriptView>());
        app.world_mut().spawn((
            Player,
            cyrus_game::player::placement::PlayerPose {
                position: [0; 3],
                heading: 0,
            },
        ));
        app.update();
        let view = app.world().resource::<ScriptView>();
        assert_eq!([view.position[0] >> 8, view.position[2] >> 8], [1000, 5535]);
        app.world_mut().remove_resource::<AnimationRequest>();
        app.update();
        assert!(!app.world().contains_resource::<ScriptView>());
    }

    #[test]
    fn captured_view_replaces_framing_once_without_moving_player() {
        let pose = Transform::from_xyz(1.0, 2.0, 3.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI));
        let mut app = App::new();
        app.init_resource::<crate::fog::RenderDistance>()
            .init_resource::<cyrus_game::fidelity::Fidelity>()
            .insert_resource(FrameScene(true))
            .insert_resource(InitialView(pose))
            .add_systems(Update, frame_scene);
        let camera = app
            .world_mut()
            .spawn((ViewCamera, Transform::default()))
            .id();
        let player = app.world_mut().spawn((Player, Transform::default())).id();
        app.update();
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), pose);
        assert_eq!(
            *app.world().get::<Transform>(player).unwrap(),
            Transform::default()
        );
        assert!(!app.world().resource::<FrameScene>().0);
        assert!(!app.world().contains_resource::<InitialView>());
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation
            .x = 8.0;
        app.update();
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation.x,
            8.0
        );
    }
}
