//! Inspection camera, independent of gameplay pose and input.
mod persistence;
pub(crate) mod projection;
use bevy::{camera::primitives::Aabb, prelude::*};
use cyrus_game::AppState;
use cyrus_script::player::Player;
use cyrus_script::player::input::InputSample;
use cyrus_script::player::input::InputSuppressed;
pub use persistence::{CameraCandidate, CameraSnapshot};
/// The view camera is independent of the player transform.
#[derive(Component)]
pub struct ViewCamera;

/// A view detached from the player: the camera stays where it was put (the control protocol's
/// `flycam` command, an explicit inspection view), player input is suppressed and the world
/// streams around the camera. It has no controls of its own; flying is the fly camera mod's
/// (`mods/flycam`), which detaches the view the same way through [`CameraTakeover`].
#[derive(Resource, Default)]
pub struct DetachedView {
    pub enabled: bool,
}

impl DetachedView {
    /// Whether the camera is free of the player: detached, or driven by a mod this frame.
    pub fn free(&self, takeover: &CameraTakeover) -> bool {
        self.enabled || takeover.0.is_some()
    }
}

/// Camera pose a mod drives this frame. Replaced every frame, so a mod that stops
/// writing hands the camera back; while set, player input is suppressed and the view is free.
#[derive(Resource, Default)]
pub struct CameraTakeover(pub Option<Transform>);

/// Frame the inspection camera on a freshly loaded scene, without moving the player.
#[derive(Resource, Default)]
pub struct FrameScene(pub bool);

/// One-shot inspection pose in exported scene coordinates, consumed on first framing.
#[derive(Resource)]
pub struct InitialView(pub Transform);

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

/// Modern camera: lock and hide the cursor during gameplay that reads input, so the mouse turns
/// the follow camera; release it everywhere else.
// A Bevy system: its parameters are independent resources and queries.
#[allow(clippy::too_many_arguments)]
fn grab_cursor(
    state: Option<Res<State<cyrus_game::AppState>>>,
    suppressed: Res<InputSuppressed>,
    pending_world: Option<Res<cyrus_world::lifecycle::WorldPreparationPending>>,
    logbook: Option<Res<cyrus_script::logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    fidelity: Res<cyrus_game::fidelity::Fidelity>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    look: Option<Res<cyrus_script::player::input::ModernLook>>,
    mut windows: Query<&mut bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
) {
    use bevy::window::CursorGrabMode;
    // The Modern camera, or a mod asking for it, turns with the captured mouse.
    let grab = (fidelity.camera == cyrus_game::fidelity::Mode::Modern
        || look.is_some_and(|look| look.orbit))
        && state.is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
        && !suppressed.0
        && pending_world.is_none()
        && !logbook.is_some_and(|screen| screen.suspended())
        // The F1 settings block the console while open.
        && !console.is_some_and(|console| console.open || console.blocked);
    let mode = if grab {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    // Browsers lock the pointer only in response to a click and release it on Escape, so a
    // click while the lock is wanted requests it again.
    let clicked = mouse.is_some_and(|mouse| mouse.get_just_pressed().next().is_some());
    for mut cursor in &mut windows {
        if cursor.grab_mode != mode || (grab && clicked) {
            cursor.grab_mode = mode;
            cursor.visible = !grab;
        }
    }
}

/// The view transform of a driven camera: the render position in map units (the inverse of
/// `script_view`'s mapping) and a look direction taken from the published angles. The angle
/// triple's orientation matrix carries the local +Z axis along the look direction with the
/// engine X axis mirrored, and the scene axes negate X and Y (**Inferred** from the camera
/// orientation stage's angles for known offsets; the comparison with the original decides).
pub fn driven_view_transform(camera: &cyrus_script::script::PublishedCamera) -> Option<Transform> {
    // The driver samples shake once over the interpolated angles. Render reads do not
    // consume another impulse or feed the wobble back into the next frame's glide.
    let angles = camera.view_angles;
    let forward = cyrus_world::transform::Matrix::orientation(angles)
        .transform([0, 0, 256])
        .ok()?;
    let look = Vec3::new(forward[0] as f32, -(forward[1] as f32), forward[2] as f32);
    let [x, y, z] = camera.render.position;
    Some(
        Transform::from_xyz(-x / 20.0, -y / 20.0, (z - (65_536.0 - 1.0 / 256.0)) / 20.0)
            .looking_to(look.normalize_or_zero(), Vec3::Y),
    )
}

/// Hand the loaded world's map terrain heights to the camera driver.
fn provide_camera_terrain(
    request: Option<Res<crate::environment::EnvironmentRequest>>,
    environments: Option<Res<Assets<cyrus_assets::environment::Environment>>>,
    mut frame: ResMut<cyrus_script::script::CameraFrameState>,
    mut provided: Local<Option<AssetId<cyrus_assets::environment::Environment>>>,
) {
    let (Some(request), Some(environments)) = (request, environments) else {
        return;
    };
    let id = request.0.id();
    if *provided == Some(id) {
        return;
    }
    if let Some(environment) = environments.get(&request.0) {
        frame.terrain = environment.terrain_heights.clone().map(std::sync::Arc::new);
        *provided = Some(id);
    }
}

/// Hand the loaded world's model lighting and palette to the examine view.
fn provide_examine_look(
    request: Option<Res<crate::environment::EnvironmentRequest>>,
    environments: Option<Res<Assets<cyrus_assets::environment::Environment>>>,
    screen: Option<ResMut<cyrus_script::item_viewer::ExamineScreen>>,
    mut provided: Local<Option<AssetId<cyrus_assets::environment::Environment>>>,
) {
    let (Some(request), Some(environments), Some(mut screen)) = (request, environments, screen)
    else {
        return;
    };
    let id = request.0.id();
    if *provided == Some(id) && screen.look.is_some() {
        return;
    }
    if let Some(environment) = environments.get(&request.0) {
        let [x, y, z] = environment.model_lighting.direction;
        screen.look = Some(cyrus_script::item_viewer::ExamineLook {
            ambient: environment.model_lighting.ambient,
            directional: environment.model_lighting.directional,
            direction: [-x, -y, z],
            palette: environment.world_palette.clone(),
        });
        *provided = Some(id);
    }
}

/// Draw the driven camera. Active only when the driver is switched on and has published a pose.
fn drive_view_camera(
    drive: Res<cyrus_script::script::CameraDrive>,
    frame: Res<cyrus_script::script::CameraFrameState>,
    mut camera: Query<&mut Transform, With<ViewCamera>>,
) {
    let (true, Some(published), Ok(mut camera)) =
        (drive.enabled, frame.published.as_ref(), camera.single_mut())
    else {
        return;
    };
    if let Some(transform) = driven_view_transform(published) {
        *camera = transform;
    }
}

// With the camera drive disabled (tests and diagnostics), the view takes the player pose directly.
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
        // The game camera is part of the script world. Keep the render camera attached to that
        // pose so its height, offset and collision follow gameplay rather than the actor origin.
        app.insert_resource(cyrus_script::script::CameraDrive { enabled: true });
        app.init_resource::<cyrus_script::script::CameraDrive>()
            .init_resource::<cyrus_script::script::CameraFrameState>()
            .init_resource::<crate::fog::RenderDistance>()
            .init_resource::<FrameScene>()
            .init_resource::<DetachedView>()
            .init_resource::<CameraTakeover>()
            .init_resource::<InputSuppressed>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(Update, suppress.before(InputSample))
            .add_systems(Update, grab_cursor)
            .add_systems(
                Update,
                script_view
                    .before(cyrus_script::script::ModelPreparation)
                    .after(follow_player)
                    .before(cyrus_script::script::OrdinaryUpdates)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    frame_scene.run_if(|f: Res<FrameScene>, camera: Res<DetachedView>| {
                        f.0 && camera.enabled
                    }),
                    follow_player
                        .after(cyrus_script::player::placement::PlayerPlacement)
                        .run_if(
                            |f: Res<DetachedView>,
                             drive: Res<cyrus_script::script::CameraDrive>,
                             t: Res<CameraTakeover>| {
                                !f.enabled && !drive.enabled && t.0.is_none()
                            },
                        ),
                    provide_camera_terrain.before(cyrus_script::script::OrdinaryUpdates),
                    provide_examine_look,
                    drive_view_camera
                        .after(cyrus_script::script::OrdinaryUpdates)
                        .run_if(|f: Res<DetachedView>| !f.enabled),
                    apply_takeover,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}
// A Bevy system: its parameters are independent resources and queries.
#[allow(clippy::too_many_arguments)]
pub(crate) fn script_view(
    mut commands: Commands,
    request: Option<Res<cyrus_world::animation::AnimationRequest>>,
    frame: Res<FrameScene>,
    ready: Option<Res<crate::ReadyScene>>,
    flycam: Res<DetachedView>,
    takeover: Res<CameraTakeover>,
    camera: Query<&Transform, With<ViewCamera>>,
    placed_player: Query<&cyrus_script::player::placement::PlayerPose, With<Player>>,
    drive: Option<Res<cyrus_script::script::CameraDrive>>,
    driven: Option<Res<cyrus_script::script::CameraFrameState>>,
) {
    let (Some(request), Ok(camera)) = (request, camera.single()) else {
        commands.remove_resource::<cyrus_script::script::ScriptView>();
        return;
    };
    if ready.is_none_or(|ready| ready.0 != request.0)
        || (flycam.enabled && frame.0)
        || (!flycam.free(&takeover) && placed_player.single().is_err())
    {
        commands.remove_resource::<cyrus_script::script::ScriptView>();
        return;
    }
    // A driven camera's world view is its published position, as the original builds its map list.
    // Until the first pose exists the camera has not moved from its spawn, so the window is built
    // around the player instead, which the first established pose is within a camera distance of.
    if drive.as_deref().is_some_and(|drive| drive.enabled) && !flycam.free(&takeover) {
        let position = match driven.as_deref().and_then(|state| state.published.as_ref()) {
            Some(published) => Some(published.pose.position),
            None => placed_player.single().ok().and_then(|pose| {
                let [x, y, z] = pose.position;
                Some([
                    i32::try_from(x).ok()?,
                    i32::try_from(y).ok()?,
                    i32::try_from(z).ok()?,
                ])
            }),
        };
        match position {
            Some(position) => commands.insert_resource(cyrus_script::script::ScriptView {
                source: request.0.clone(),
                position,
            }),
            None => commands.remove_resource::<cyrus_script::script::ScriptView>(),
        }
        return;
    }
    let position = camera.translation;
    commands.insert_resource(cyrus_script::script::ScriptView {
        source: request.0.clone(),
        position: [
            (-f64::from(position.x) * 20.0 * 256.0).floor() as i32,
            (-f64::from(position.y) * 20.0 * 256.0).floor() as i32,
            ((f64::from(position.z) * 20.0 + 65535.99609375) * 256.0).floor() as i32,
        ],
    });
}

fn suppress(
    flycam: Res<DetachedView>,
    takeover: Res<CameraTakeover>,
    mut suppressed: ResMut<InputSuppressed>,
) {
    suppressed.0 = flycam.enabled || takeover.0.is_some();
}

fn apply_takeover(
    takeover: Res<CameraTakeover>,
    mut camera: Query<&mut Transform, With<ViewCamera>>,
) {
    if let (Some(pose), Ok(mut camera)) = (takeover.0, camera.single_mut()) {
        *camera = pose;
    }
}
fn spawn(
    mut commands: Commands,
    initial: Option<Res<InitialView>>,
    mut flycam: ResMut<DetachedView>,
    cameras: Query<(), With<ViewCamera>>,
) {
    if !cameras.is_empty() {
        return;
    }
    if initial.is_some() {
        flycam.enabled = true;
    }
    commands.spawn((
        ViewCamera,
        Camera3d::default(),
        Msaa::Off,
        Projection::custom(projection::GameProjection::default()),
        Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::new(100.0, 40.0, 100.0), Vec3::Y),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_requests_the_modern_cursor_lock_again() {
        use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(AppState::InGame)
            .init_resource::<InputSuppressed>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(cyrus_game::fidelity::Fidelity {
                camera: cyrus_game::fidelity::Mode::Modern,
                ..Default::default()
            })
            .add_systems(Update, grab_cursor);
        let window = app
            .world_mut()
            .spawn((CursorOptions::default(), PrimaryWindow))
            .id();
        let changed = |app: &App| {
            app.world()
                .entity(window)
                .get_change_ticks::<CursorOptions>()
                .expect("cursor options")
                .changed
        };
        app.update();
        let cursor = app.world().get::<CursorOptions>(window).expect("cursor");
        assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
        assert!(!cursor.visible);
        let locked = changed(&app);
        app.update();
        assert_eq!(changed(&app), locked, "no request without a click");
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_ne!(changed(&app), locked, "a click requests the lock again");
    }

    #[test]
    fn driven_view_uses_render_angles_during_a_glide() {
        let camera = cyrus_script::script::PublishedCamera {
            pose: cyrus_script::script::camera::CameraPose {
                position: [0, 0, 0],
                angles: [0, 0, 0],
            },
            render: cyrus_script::script::camera::RenderPose {
                position: [0.0, 0.0, 0.0],
                angles: [0, 512, 0],
            },
            view_angles: [0, 512, 0],
        };
        let transform = driven_view_transform(&camera).expect("valid camera pose");
        let expected = cyrus_world::transform::Matrix::orientation([0, 512, 0])
            .transform([0, 0, 256])
            .expect("orientation transform");
        let expected = Vec3::new(
            expected[0] as f32,
            -(expected[1] as f32),
            expected[2] as f32,
        )
        .normalize();
        assert!(transform.forward().dot(expected) > 0.999);
    }

    #[test]
    fn driven_view_reuses_the_sampled_shake_without_changing_camera_poses() {
        let published = cyrus_script::script::PublishedCamera {
            pose: cyrus_script::script::camera::CameraPose {
                position: [0; 3],
                angles: [0; 3],
            },
            render: cyrus_script::script::camera::RenderPose {
                position: [20.0, 40.0, 60.0],
                angles: [64, 512, 0],
            },
            view_angles: [32, 512, 0],
        };
        let mut frame = cyrus_script::script::CameraFrameState::default();
        frame.published = Some(published);
        let mut app = App::new();
        app.insert_resource(cyrus_script::script::CameraDrive { enabled: true })
            .insert_resource(frame)
            .add_systems(Update, drive_view_camera);
        let entity = app
            .world_mut()
            .spawn((ViewCamera, Transform::default()))
            .id();
        let expected = cyrus_world::transform::Matrix::orientation([32, 512, 0])
            .transform([0, 0, 256])
            .unwrap();
        let expected = Vec3::new(
            expected[0] as f32,
            -(expected[1] as f32),
            expected[2] as f32,
        )
        .normalize();
        for _ in 0..3 {
            app.update();
            let transform = app.world().get::<Transform>(entity).unwrap();
            assert!(transform.forward().dot(expected) > 0.99999);
            assert_eq!(
                app.world()
                    .resource::<cyrus_script::script::CameraFrameState>()
                    .published,
                Some(published)
            );
        }
    }

    #[test]
    fn game_camera_disables_multisampling() {
        let mut app = App::new();
        app.init_resource::<DetachedView>()
            .add_systems(Update, spawn);
        app.update();
        let mut cameras = app.world_mut().query_filtered::<&Msaa, With<ViewCamera>>();
        assert_eq!(*cameras.single(app.world()).unwrap(), Msaa::Off);
    }

    #[test]
    fn entry_preserves_a_published_camera_and_controls() {
        let mut app = App::new();
        app.insert_resource(DetachedView { enabled: false })
            .add_systems(Update, spawn);
        let camera = app
            .world_mut()
            .spawn((ViewCamera, Transform::from_xyz(12.0, 34.0, 56.0)))
            .id();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<ViewCamera>>()
                .iter(app.world())
                .collect::<Vec<_>>(),
            vec![camera]
        );
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            Vec3::new(12.0, 34.0, 56.0)
        );
        assert!(!app.world().resource::<DetachedView>().enabled);
    }

    #[test]
    fn ordinary_view_uses_camera_and_waits_for_scene_framing() {
        use cyrus_script::script::ScriptView;
        use cyrus_world::animation::AnimationRequest;
        let mut app = App::new();
        let source = Handle::default();
        app.insert_resource(AnimationRequest(source.clone()))
            .insert_resource(crate::ReadyScene(source.clone()))
            .insert_resource(FrameScene(true))
            .insert_resource(DetachedView { enabled: true })
            .init_resource::<CameraTakeover>()
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
        app.world_mut().resource_mut::<DetachedView>().enabled = false;
        app.world_mut().resource_mut::<FrameScene>().0 = true;
        app.update();
        assert!(!app.world().contains_resource::<ScriptView>());
        app.world_mut().spawn((
            Player,
            cyrus_script::player::placement::PlayerPose {
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
