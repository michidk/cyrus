use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use cyrus_game::AppState;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::player::Player;
use cyrus_game::player::PlayerPlugin;
use cyrus_game::player::input::PlayerBindings;
use cyrus_game::player::input::PlayerInput;
use cyrus_render::camera::FlycamSettings;
use cyrus_render::camera::ViewCamera;

#[test]
fn flycam_moves_only_the_camera_and_returns_to_the_player() {
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
    app.insert_resource(cyrus_game::script::CameraDrive { enabled: false });
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    assert!(!app.world().resource::<FlycamSettings>().enabled);
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
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(100),
    ));
    app.world_mut().resource_mut::<FlycamSettings>().enabled = true;
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
    assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    assert_eq!(*app.world().get::<Transform>(entity).unwrap(), before);
    app.world_mut().resource_mut::<FlycamSettings>().enabled = false;
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
    assert!(app.world().resource::<FlycamSettings>().enabled);
    assert!(!app.world().contains_resource::<InitialView>());
    let camera = app
        .world_mut()
        .query_filtered::<&Transform, With<ViewCamera>>()
        .single(app.world())
        .unwrap();
    assert_eq!(*camera, pose);
}
