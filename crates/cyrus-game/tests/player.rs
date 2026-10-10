use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use cyrus_game::AppState;
use cyrus_game::fidelity::{Fidelity, Mode};
use cyrus_game::player::input::{PlayerBindings, PlayerInput};
use cyrus_game::player::{FlycamSettings, Player, PlayerPlugin, ViewCamera};

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
fn flycam_moves_only_the_camera_and_returns_to_the_player() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29]).unwrap())
        .insert_resource(FlycamSettings {
            enabled: false,
            ..default()
        })
        .add_plugins(PlayerPlugin);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
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
fn installed_keyboard_bindings_load_through_plugin() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let install = match cyrus_game::install::GameInstall::open(&root) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("skipped: no install ({error})");
            return;
        }
    };
    let text = std::fs::read_to_string(install.root.join("KEYS.INI")).unwrap();
    let parsed = rgpre::import::keys_ini::KeysIni::parse(&text).unwrap();
    PlayerBindings::from_codes(parsed.keyboard_codes()).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(install)
        .insert_resource(FlycamSettings {
            enabled: false,
            ..default()
        })
        .add_plugins(PlayerPlugin);
    app.update();
    // Exercise the plugin loader with the install's configured bindings.
    let mut keys = ButtonInput::default();
    for key in [
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ShiftLeft,
        KeyCode::Space,
        KeyCode::AltLeft,
        KeyCode::ControlLeft,
    ] {
        keys.press(key);
    }
    let expected = PlayerBindings::from_codes(parsed.keyboard_codes())
        .unwrap()
        .sample(&keys);
    *app.world_mut().resource_mut::<ButtonInput<KeyCode>>() = keys;
    app.update();
    assert_eq!(*app.world().resource::<PlayerInput>(), expected);
}

#[test]
fn modern_keys_and_held_run_switch_without_changing_faithful_bindings() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29]).unwrap())
        .insert_resource(FlycamSettings {
            enabled: false,
            ..default()
        })
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
    app.world_mut().resource_mut::<FlycamSettings>().enabled = true;
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
    assert_eq!(walk.advance(8, 161, false), 0);
    let mut z = 10_502_144;
    for (scale, observed_z) in [(161, 10_502_572), (161, 10_503_216), (153, 10_504_440)] {
        z += walk.advance(8, scale, true);
        assert_eq!(z, observed_z);
    }
    assert_eq!(walk.advance(8, 153, false), 0);
    assert_eq!(walk.advance(8, 153, true), 1224);
}

#[test]
fn walking_startup_uses_actor_speed_and_rejects_unsupported_step_types() {
    use cyrus_game::player::walk::WalkingStartup;

    for step_type in 0..=u8::MAX {
        assert_eq!(WalkingStartup::new(step_type).is_some(), step_type == 1);
    }
    let mut walk = WalkingStartup::new(1).unwrap();
    assert_eq!(walk.advance(7, 256, true), 448);
    assert_eq!(walk.advance(7, 256, true), 597);
    assert_eq!(walk.advance(7, 256, true), 896);
    assert_eq!(walk.advance(7, 256, true), 1792);
    assert_eq!(walk.advance(0, 256, true), 0);
    assert_eq!(walk.advance(9, 256, true), 2304);
}
