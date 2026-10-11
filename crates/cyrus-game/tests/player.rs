use bevy::prelude::*;
use cyrus_game::AppState;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use cyrus_game::player::Player;
use cyrus_game::player::PlayerPlugin;
use cyrus_game::player::input::PlayerBindings;
use cyrus_game::player::input::PlayerInput;

#[test]
fn keyboard_input_preserves_opposing_actions_and_releases() {
    let bindings = PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29]).unwrap();
    let mut keys = ButtonInput::default();
    keys.press(KeyCode::ArrowUp);
    keys.press(KeyCode::ArrowDown);
    keys.press(KeyCode::Space);
    assert_eq!(
        bindings.sample(&keys),
        PlayerInput {
            forward: true,
            backward: true,
            jump: true,
            ..default()
        }
    );
    keys.release_all();
    assert_eq!(bindings.sample(&keys), PlayerInput::default());
    keys.press(KeyCode::ShiftLeft);
    assert_eq!(
        bindings.sample(&keys),
        PlayerInput {
            walk: true,
            ..default()
        }
    );
    keys.release_all();
    keys.press(KeyCode::ControlLeft);
    assert_eq!(
        bindings.sample(&keys),
        PlayerInput {
            action: true,
            ..default()
        }
    );
    keys.release_all();
    assert!(PlayerBindings::from_codes([128; 8]).is_err());
    assert!(PlayerBindings::from_codes([76; 8]).is_err());
    assert_eq!(
        PlayerBindings::from_codes([0; 8]).unwrap().sample(&keys),
        PlayerInput::default()
    );
}

#[test]
fn modern_keys_and_held_run_switch_without_changing_faithful_bindings() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29]).unwrap())
        .add_plugins(PlayerPlugin);
    app.world_mut().resource_mut::<Fidelity>().player_controller = Mode::Modern;
    for key in [
        KeyCode::KeyW,
        KeyCode::KeyD,
        KeyCode::Space,
        KeyCode::ShiftLeft,
    ] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    app.update();
    assert_eq!(
        *app.world().resource::<PlayerInput>(),
        PlayerInput {
            forward: true,
            right: true,
            jump: true,
            ..default()
        }
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    app.update();
    assert!(app.world().resource::<PlayerInput>().walk);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftRight);
    app.update();
    assert!(!app.world().resource::<PlayerInput>().walk);
    app.world_mut().resource_mut::<Fidelity>().player_controller = Mode::Faithful;
    app.update();
    assert_eq!(
        *app.world().resource::<PlayerInput>(),
        PlayerInput {
            jump: true,
            ..default()
        }
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);
    app.update();
    assert!(app.world().resource::<PlayerInput>().forward);
    app.world_mut()
        .resource_mut::<cyrus_game::player::input::InputSuppressed>()
        .0 = true;
    app.world_mut().resource_mut::<Fidelity>().player_controller = Mode::Modern;
    app.update();
    assert_eq!(
        *app.world().resource::<PlayerInput>(),
        PlayerInput::default()
    );
}

#[test]
fn ground_turn_matches_recorded_original_headings() {
    let mut turn = cyrus_game::player::turn::GroundTurn::default();
    let mut heading = 1734;
    // The recording skips one display frame between 1707 and 1644.
    for (scale, observed) in [
        (192, Some(1725)),
        (192, Some(1707)),
        (192, None),
        (192, Some(1644)),
        (192, Some(1608)),
        (192, Some(1572)),
        (180, Some(1539)),
        (180, Some(1506)),
        (180, Some(1473)),
        (180, Some(1440)),
    ] {
        heading = turn.advance(heading, [false, true], 12, 48, scale);
        if let Some(observed) = observed {
            assert_eq!(heading, observed);
        }
    }
    assert_eq!(turn.advance(heading, [false, false], 12, 48, 180), 1440);
    assert_eq!(turn.advance(1440, [false, true], 12, 48, 180), 1432);
}

#[test]
fn ground_turn_rounds_directions_separately_and_preserves_overshoot() {
    let mut turn = cyrus_game::player::turn::GroundTurn::default();
    assert_eq!(turn.advance(0, [false, true], 12, 48, 180), 2040);
    assert_eq!(turn.advance(2040, [true, true], 12, 48, 180), 2032);
    assert_eq!(turn.advance(2032, [true, true], 12, 48, 180), 2023);
    let mut turn = cyrus_game::player::turn::GroundTurn::default();
    assert_eq!(turn.advance(0, [true, false], 12, 20, 256), 12);
    assert_eq!(turn.advance(12, [true, false], 12, 20, 256), 36);
    assert_eq!(turn.advance(36, [true, false], 12, 20, 256), 60);
}

#[test]
fn walking_startup_matches_original_animation_gated_displacements() {
    let mut walk = cyrus_game::player::walk::WalkingStartup::new(1).unwrap();
    assert_eq!(walk.advance(8, 0, 161, false), 0);
    let mut z = 10_502_144;
    for (scale, observed_z) in [(161, 10_502_572), (161, 10_503_216), (153, 10_504_440)] {
        z += walk.advance(8, 0, scale, true);
        assert_eq!(z, observed_z);
    }
    assert_eq!(walk.advance(8, 0, 153, false), 0);
    assert_eq!(walk.advance(8, 0, 153, true), 1224);
}

#[test]
fn walking_startup_uses_actor_speed_and_rejects_unsupported_step_types() {
    use cyrus_game::player::walk::WalkingStartup;

    for step_type in 0..=u8::MAX {
        assert_eq!(WalkingStartup::new(step_type).is_some(), step_type == 1);
    }
    let mut walk = WalkingStartup::new(1).unwrap();
    assert_eq!(walk.advance(7, 0, 256, true), 448);
    assert_eq!(walk.advance(7, 0, 256, true), 597);
    assert_eq!(walk.advance(7, 0, 256, true), 896);
    assert_eq!(walk.advance(7, 0, 256, true), 1792);
    assert_eq!(walk.advance(0, 0, 256, true), 0);
    assert_eq!(walk.advance(9, 0, 256, true), 2304);
}

#[test]
fn marker_entry_waits_for_world_data_and_keeps_exact_position() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::player::placement::MarkerEntry;
    use cyrus_game::player::placement::PlayerPose;
    use cyrus_world::animation::AnimationRequest;

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Animations>>()
        .init_resource::<AnimationRequest>()
        .insert_resource(MarkerEntry {
            marker: 0,
            heading: -1,
        })
        .add_plugins(PlayerPlugin);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    assert!(app.world().get::<PlayerPose>(player).is_none());
    let data = Animations {
        version: 2,
        world: 1,
        metadata: serde_json::json!({"markers": [
            {"map_position": [40816, -699, 41024], "position": [-2040.8, 34.95, -1225.5998], "reserved": 1},
            {"map_position": [0, 0, 0], "position": [0.0, 0.0, 0.0], "reserved": 0}
        ]}),
        sections: default(),
        models: default(),
        unsupported: default(),
    };
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.world_mut().resource_mut::<AnimationRequest>().0 = handle;
    app.update();
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap(),
        &PlayerPose {
            position: [10_448_896, -178_944, 10_502_144],
            heading: 2047,
        }
    );
    assert_eq!(
        app.world().get::<Transform>(player).unwrap().translation,
        Vec3::new(-2040.8, 34.95, -1225.5998)
    );
    // Entry is one-shot; later updates must not undo movement or ground settling.
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation
        .y = 37.2;
    app.update();
    assert_eq!(
        app.world().get::<Transform>(player).unwrap().translation.y,
        37.2
    );
    app.world_mut().resource_mut::<MarkerEntry>().marker = 1;
    app.update();
    assert!(app.world().get::<PlayerPose>(player).is_none());
    app.world_mut().resource_mut::<MarkerEntry>().marker = 9;
    app.update();
    assert!(app.world().get::<PlayerPose>(player).is_none());
}

#[test]
fn walking_release_restarts_but_animation_suppression_does_not() {
    let mut walk = cyrus_game::player::walk::WalkingStartup::new(1).unwrap();
    assert_eq!(walk.advance(8, 0, 256, false), 0);
    walk.finish_update(true);
    assert_eq!(walk.advance(8, 0, 256, true), 682);
    walk.finish_update(true);
    assert_eq!(walk.advance(8, 0, 256, true), 1024);
    walk.finish_update(true);
    assert_eq!(walk.advance(8, 0, 256, true), 2048);
    walk.finish_update(false);
    walk.finish_update(false);
    assert_eq!(walk.advance(8, 0, 256, true), 512);
}

#[test]
fn installed_island_marker_matches_observed_original_entry() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (marker observation check)");
        return;
    };
    let path = std::fs::read_dir(install.maps())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("island.rgm")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let metadata = rgpre::import::rgm::export_rgm_metadata_json(&map, None);
    assert_eq!(
        metadata["markers"][0]["map_position"],
        serde_json::json!([40816, -699, 41024])
    );
    assert_eq!(metadata["markers"][0]["reserved"], 1);
}
