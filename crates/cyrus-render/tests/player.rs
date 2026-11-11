use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use cyrus_game::AppState;
use cyrus_game::fidelity::Fidelity;
use cyrus_render::camera::CameraTakeover;
use cyrus_render::camera::DetachedView;
use cyrus_render::camera::ViewCamera;
use cyrus_script::player::Player;
use cyrus_script::player::PlayerPlugin;
use cyrus_script::player::input::PlayerBindings;
use cyrus_script::player::input::PlayerInput;

#[test]
fn a_flown_camera_moves_only_the_camera_and_returns_to_the_player() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29]).unwrap())
        .add_plugins((PlayerPlugin, cyrus_render::camera::CameraPlugin));
    // The game camera drive is on by default and needs a script world; without one the view camera
    // would keep the flown pose. This test covers the player-transform camera it replaces.
    app.insert_resource(cyrus_script::script::CameraDrive { enabled: false });
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    assert!(!app.world().resource::<DetachedView>().enabled);
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    let before = *app.world().get::<Transform>(entity).unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);
    app.update();
    assert!(app.world().resource::<PlayerInput>().forward);
    assert_eq!(*app.world().get::<Transform>(entity).unwrap(), before);
    // A mod flies the camera by writing a pose every frame.
    let flown = Transform::from_xyz(300.0, 40.0, -200.0);
    app.insert_resource(CameraTakeover(Some(flown)));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.update();
    assert_eq!(
        *app.world().resource::<PlayerInput>(),
        PlayerInput::default()
    );
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<ViewCamera>>()
        .single(app.world())
        .unwrap();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), flown);
    assert_eq!(*app.world().get::<Transform>(entity).unwrap(), before);
    app.insert_resource(CameraTakeover(None));
    app.update();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    assert_eq!(*app.world().get::<Transform>(entity).unwrap(), before);
}

#[test]
fn explicit_inspection_pose_enables_flycam() {
    use cyrus_render::camera::{FrameScene, InitialView};

    let pose = Transform::from_xyz(3.0, 7.0, 11.0);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(InitialView(pose))
        .insert_resource(FrameScene(true))
        .add_plugins(cyrus_render::camera::CameraPlugin);
    app.world_mut().spawn((Player, Transform::IDENTITY));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    app.update();
    assert!(app.world().resource::<DetachedView>().enabled);
    assert!(!app.world().contains_resource::<InitialView>());
    let camera = app
        .world_mut()
        .query_filtered::<&Transform, With<ViewCamera>>()
        .single(app.world())
        .unwrap();
    assert_eq!(*camera, pose);
}
