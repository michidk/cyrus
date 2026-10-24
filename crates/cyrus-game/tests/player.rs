use bevy::input::mouse::MouseButton;
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
    assert!(PlayerBindings::from_codes([130; 8]).is_err());
    assert!(PlayerBindings::from_codes([76; 8]).is_err());
    assert_eq!(
        PlayerBindings::from_codes([0; 8]).unwrap().sample(&keys),
        PlayerInput::default()
    );
}

#[test]
fn mouse_bindings_read_the_release_edge_not_the_held_button() {
    // Codes 128 and 129 are the left and right button release, one update each.
    let bindings = PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 128]).unwrap();
    let keys = ButtonInput::<KeyCode>::default();
    let mut mouse = ButtonInput::<MouseButton>::default();
    let action =
        |mouse: &ButtonInput<MouseButton>| bindings.sample_with_mouse(&keys, Some(mouse)).action;
    mouse.press(MouseButton::Left);
    assert!(!action(&mouse), "held button does not read as pressed");
    mouse.clear();
    assert!(!action(&mouse));
    mouse.release(MouseButton::Left);
    assert!(action(&mouse), "release reads as pressed for one update");
    mouse.clear();
    assert!(!action(&mouse));
    // Without a mouse resource the binding stays inactive.
    assert!(!bindings.sample(&keys).action);
    assert_eq!(cyrus_game::player::input::mouse_release_code(&mouse), None);
    mouse.press(MouseButton::Right);
    mouse.release(MouseButton::Right);
    assert_eq!(
        cyrus_game::player::input::mouse_release_code(&mouse),
        Some(129)
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
    assert!(
        app.world()
            .resource::<cyrus_game::player::input::DialogueSkip>()
            .0
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    app.update();
    assert!(app.world().resource::<PlayerInput>().walk);
    assert!(
        !app.world()
            .resource::<cyrus_game::player::input::DialogueSkip>()
            .0
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftRight);
    app.update();
    assert!(!app.world().resource::<PlayerInput>().walk);
    assert!(
        !app.world()
            .resource::<cyrus_game::player::input::DialogueSkip>()
            .0
    );
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
    app.world_mut()
        .insert_resource(cyrus_world::lifecycle::WorldPreparationPending);
    app.world_mut()
        .resource_mut::<cyrus_game::player::input::InputSuppressed>()
        .0 = false;
    app.world_mut()
        .resource_mut::<cyrus_game::fidelity::Fidelity>()
        .player_controller = Mode::Faithful;
    app.update();
    assert_eq!(
        *app.world().resource::<PlayerInput>(),
        PlayerInput::default(),
        "LoadWorld preparation suspends gameplay input"
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
fn ground_turn_delta_is_signed_and_turned_wraps() {
    use cyrus_game::player::turn::{GroundTurn, turned};
    let mut turn = GroundTurn::default();
    turn.update([true, false], 12, 48);
    assert_eq!(turn.delta(256), -12);
    turn.update([false, true], 12, 48);
    assert_eq!(turn.delta(256), 12);
    turn.update([false, false], 12, 48);
    turn.update([true, true], 12, 48);
    assert_eq!(turn.delta(256), 0);
    assert_eq!(turned(5, 12), 2041);
    assert_eq!(turned(2040, -12), 4);
}

#[test]
fn running_turn_leans_to_thirty_two_and_relaxes_by_eight() {
    use cyrus_game::player::turn::{lean, relax};
    // Left banks up by 4 to 32, right down by 4 to 2016, then both hold.
    assert_eq!(lean(0, -1, true), 4);
    assert_eq!(lean(28, -1, true), 32);
    assert_eq!(lean(32, -1, true), 32);
    assert_eq!(lean(0, 1, true), 2040);
    assert_eq!(lean(2044, 1, true), 2040);
    assert_eq!(lean(0x7e0, 1, true), 0x7e0);
    // Banked the other way, a left turn returns by 8 and wraps to zero.
    assert_eq!(lean(2040, -1, true), 0);
    assert_eq!(lean(2000, -1, true), 2008);
    // Walking or no input relaxes toward zero from either side.
    assert_eq!(lean(20, -1, false), 12);
    assert_eq!(lean(20, 0, true), 12);
    assert_eq!(
        (relax(5), relax(0x400), relax(0x7f8), relax(0x7f9)),
        (0, 0x3f8, 0, 0)
    );
    assert_eq!(relax(0x401), 0x409);
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
fn marker_entry_waits_for_matching_world_data_and_settles_once() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::player::data::{PlayerScene, PlayerSceneRequest};
    use cyrus_game::player::placement::MarkerEntry;
    use cyrus_game::player::placement::PlayerEntrySupport;
    use cyrus_game::player::placement::PlayerPose;
    use cyrus_world::animation::AnimationRequest;

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Animations>>()
        .init_resource::<Assets<PlayerScene>>()
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
    assert!(app.world().get::<PlayerPose>(player).is_none());
    let scene = PlayerScene::decode(
        &serde_json::to_vec(&serde_json::json!({
            "version":2,"world":1,
            "settings":{"standing_height":-2,"post_collide_height":50,"slide_range":-60000,
                "turn_speed":12,"turn_max_speed":48,"poly_push_units":16},
            "definition":{
                "actors":[{"script_name":"CYRUS","attributes":{},"mpsz_bounds_primary":0,
                    "raex":{"unknown_12":256}}],
                "mpsz_entries":[{"total_extent":[48,128,49],"pos_extent":[24,62,11],
                    "neg_extent":[24,66,38],"center_offset":[0,0,0],"flags":0}]
            },
            "heights":vec![-680;256*256],"water":vec![false;256*256],
            "collision":{"placements":[],"models":{},"unsupported":{}}
        }))
        .unwrap(),
    )
    .unwrap();
    let scene_handle = app
        .world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .add(scene);
    app.world_mut()
        .insert_resource(PlayerSceneRequest(scene_handle.clone()));
    app.world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .get_mut(&scene_handle)
        .unwrap()
        .world = 2;
    app.update();
    assert!(app.world().get::<PlayerPose>(player).is_none());
    app.world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .get_mut(&scene_handle)
        .unwrap()
        .world = 1;
    app.update();
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap(),
        &PlayerPose {
            position: [10_448_896, -190_464, 10_502_144],
            heading: 2047,
        }
    );
    assert_eq!(
        app.world().get::<Transform>(player).unwrap().translation,
        Vec3::new(-2040.8, 37.2, -1225.5998)
    );
    assert_eq!(
        app.world().get::<PlayerEntrySupport>(player),
        Some(&PlayerEntrySupport::StaticSettled)
    );
    // Entry is one-shot; later updates must not undo movement or ground settling.
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation
        .y = 40.0;
    app.update();
    assert_eq!(
        app.world().get::<Transform>(player).unwrap().translation.y,
        40.0
    );
    app.world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .get_mut(&scene_handle)
        .unwrap()
        .terrain = Some(
        cyrus_collision::ground::TerrainGround::new(
            vec![-680; 256 * 256],
            vec![true; 256 * 256],
            None,
        )
        .unwrap(),
    );
    app.world_mut().resource_mut::<MarkerEntry>().heading = 0;
    app.update();
    // Water cells supply their height to startup settling like dry cells.
    assert_eq!(
        app.world().get::<PlayerEntrySupport>(player),
        Some(&PlayerEntrySupport::StaticSettled)
    );
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap().position,
        [10_448_896, -190_464, 10_502_144]
    );
    assert_eq!(
        app.world().get::<Transform>(player).unwrap().translation,
        Vec3::new(-2040.8, 37.2, -1225.5998)
    );
    app.world_mut().resource_mut::<MarkerEntry>().marker = 1;
    app.update();
    assert!(app.world().get::<PlayerPose>(player).is_none());
    assert!(app.world().get::<PlayerEntrySupport>(player).is_none());
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
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
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

#[test]
fn restored_player_placement_survives_marker_entry_without_settling() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::player::data::{PlayerScene, PlayerSceneRequest};
    use cyrus_game::player::placement::{
        MarkerEntry, PlayerEntrySupport, PlayerPlacementSnapshot, PlayerPose,
    };
    use cyrus_world::animation::AnimationRequest;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Animations>>()
        .init_resource::<Assets<PlayerScene>>()
        .init_resource::<AnimationRequest>()
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
    let marker = MarkerEntry {
        marker: 7,
        heading: -23,
    };
    let pose = PlayerPose {
        position: [12345, -67890, 45678],
        heading: 2025,
    };
    let transform = Transform::from_xyz(-1.25, 8.5, 22.0).with_rotation(Quat::from_rotation_y(0.7));
    let support = PlayerEntrySupport::InspectionOnly("retained support failure".into());
    let snapshot =
        PlayerPlacementSnapshot::capture(17, marker, &pose, &support, &transform).unwrap();
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let snapshot: PlayerPlacementSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert!(
        snapshot
            .clone()
            .restore(18, Handle::default(), Handle::default())
            .is_err()
    );
    app.insert_resource(snapshot.marker_entry());
    app.insert_resource(PlayerSceneRequest(Handle::default()));
    app.world_mut().entity_mut(entity).insert(
        snapshot
            .restore(17, Handle::default(), Handle::default())
            .unwrap(),
    );
    app.update();
    app.update();
    assert_eq!(app.world().get::<PlayerPose>(entity), Some(&pose));
    assert_eq!(
        app.world().get::<PlayerEntrySupport>(entity),
        Some(&support)
    );
    assert_eq!(app.world().get::<Transform>(entity), Some(&transform));
    // A later explicit entry is not mistaken for restoration.
    app.world_mut().resource_mut::<MarkerEntry>().marker = 8;
    app.update();
    assert!(app.world().get::<PlayerPose>(entity).is_none());
}

use cyrus_game::player::turn::{GroundTurnInput, ground_turn};

/// Rows run through the original controller's keyboard turn block over synthetic actors with the
/// published rotation table (research oracle): `(walk key, forward, left accumulator, right
/// accumulator, heading, roll, current group, torch, scale, heading after, roll after, turn
/// group or -1)`.
#[test]
fn the_keyboard_turn_matches_the_original_controller() {
    type Row = (u8, u8, i64, i64, u16, u16, u16, u8, i64, u16, u16, i32);
    let rows: &[Row] = &[
        (0, 0, 0, 0, 0, 0, 0, 0, 256, 0, 0, -1),
        (0, 1, 0, 0, 0, 0, 0, 0, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 0, 1, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 18, 0, 256, 0, 0, -1),
        (0, 1, 0, 0, 0, 0, 18, 0, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 18, 1, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 30, 0, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 30, 1, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 5, 0, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 0, 5, 1, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 20, 0, 0, 256, 0, 4, -1),
        (0, 1, 0, 0, 0, 20, 0, 0, 256, 0, 4, -1),
        (0, 0, 0, 0, 0, 20, 0, 1, 256, 0, 4, -1),
        (0, 0, 0, 0, 0, 20, 18, 0, 256, 0, 12, -1),
        (0, 1, 0, 0, 0, 20, 18, 0, 256, 0, 12, -1),
        (0, 0, 0, 0, 0, 20, 18, 1, 256, 0, 12, -1),
        (0, 0, 0, 0, 0, 20, 30, 0, 256, 0, 12, -1),
        (0, 0, 0, 0, 0, 20, 30, 1, 256, 0, 12, -1),
        (0, 0, 0, 0, 0, 20, 5, 0, 256, 0, 4, -1),
        (0, 0, 0, 0, 0, 20, 5, 1, 256, 0, 4, -1),
        (0, 0, 0, 0, 0, 1025, 0, 0, 256, 0, 1041, -1),
        (0, 1, 0, 0, 0, 1025, 0, 0, 256, 0, 1041, -1),
        (0, 0, 0, 0, 0, 1025, 0, 1, 256, 0, 1041, -1),
        (0, 0, 0, 0, 0, 1025, 18, 0, 256, 0, 1033, -1),
        (0, 1, 0, 0, 0, 1025, 18, 0, 256, 0, 1033, -1),
        (0, 0, 0, 0, 0, 1025, 18, 1, 256, 0, 1033, -1),
        (0, 0, 0, 0, 0, 1025, 30, 0, 256, 0, 1033, -1),
        (0, 0, 0, 0, 0, 1025, 30, 1, 256, 0, 1033, -1),
        (0, 0, 0, 0, 0, 1025, 5, 0, 256, 0, 1041, -1),
        (0, 0, 0, 0, 0, 1025, 5, 1, 256, 0, 1041, -1),
        (0, 0, 0, 0, 0, 2032, 0, 0, 256, 0, 0, -1),
        (0, 1, 0, 0, 0, 2032, 0, 0, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 2032, 0, 1, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 2032, 18, 0, 256, 0, 2040, -1),
        (0, 1, 0, 0, 0, 2032, 18, 0, 256, 0, 2040, -1),
        (0, 0, 0, 0, 0, 2032, 18, 1, 256, 0, 2040, -1),
        (0, 0, 0, 0, 0, 2032, 30, 0, 256, 0, 2040, -1),
        (0, 0, 0, 0, 0, 2032, 30, 1, 256, 0, 2040, -1),
        (0, 0, 0, 0, 0, 2032, 5, 0, 256, 0, 0, -1),
        (0, 0, 0, 0, 0, 2032, 5, 1, 256, 0, 0, -1),
        (0, 0, 0, 0, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (0, 1, 0, 0, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 0, 1, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (0, 1, 0, 0, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 18, 1, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 30, 1, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 0, 5, 1, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 20, 0, 0, 256, 2047, 4, -1),
        (0, 1, 0, 0, 2047, 20, 0, 0, 256, 2047, 4, -1),
        (0, 0, 0, 0, 2047, 20, 0, 1, 256, 2047, 4, -1),
        (0, 0, 0, 0, 2047, 20, 18, 0, 256, 2047, 12, -1),
        (0, 1, 0, 0, 2047, 20, 18, 0, 256, 2047, 12, -1),
        (0, 0, 0, 0, 2047, 20, 18, 1, 256, 2047, 12, -1),
        (0, 0, 0, 0, 2047, 20, 30, 0, 256, 2047, 12, -1),
        (0, 0, 0, 0, 2047, 20, 30, 1, 256, 2047, 12, -1),
        (0, 0, 0, 0, 2047, 20, 5, 0, 256, 2047, 4, -1),
        (0, 0, 0, 0, 2047, 20, 5, 1, 256, 2047, 4, -1),
        (0, 0, 0, 0, 2047, 1025, 0, 0, 256, 2047, 1041, -1),
        (0, 1, 0, 0, 2047, 1025, 0, 0, 256, 2047, 1041, -1),
        (0, 0, 0, 0, 2047, 1025, 0, 1, 256, 2047, 1041, -1),
        (0, 0, 0, 0, 2047, 1025, 18, 0, 256, 2047, 1033, -1),
        (0, 1, 0, 0, 2047, 1025, 18, 0, 256, 2047, 1033, -1),
        (0, 0, 0, 0, 2047, 1025, 18, 1, 256, 2047, 1033, -1),
        (0, 0, 0, 0, 2047, 1025, 30, 0, 256, 2047, 1033, -1),
        (0, 0, 0, 0, 2047, 1025, 30, 1, 256, 2047, 1033, -1),
        (0, 0, 0, 0, 2047, 1025, 5, 0, 256, 2047, 1041, -1),
        (0, 0, 0, 0, 2047, 1025, 5, 1, 256, 2047, 1041, -1),
        (0, 0, 0, 0, 2047, 2032, 0, 0, 256, 2047, 0, -1),
        (0, 1, 0, 0, 2047, 2032, 0, 0, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 2032, 0, 1, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 2032, 18, 0, 256, 2047, 2040, -1),
        (0, 1, 0, 0, 2047, 2032, 18, 0, 256, 2047, 2040, -1),
        (0, 0, 0, 0, 2047, 2032, 18, 1, 256, 2047, 2040, -1),
        (0, 0, 0, 0, 2047, 2032, 30, 0, 256, 2047, 2040, -1),
        (0, 0, 0, 0, 2047, 2032, 30, 1, 256, 2047, 2040, -1),
        (0, 0, 0, 0, 2047, 2032, 5, 0, 256, 2047, 0, -1),
        (0, 0, 0, 0, 2047, 2032, 5, 1, 256, 2047, 0, -1),
        (0, 0, 12, 0, 0, 0, 0, 0, 256, 12, 0, 14),
        (0, 1, 12, 0, 0, 0, 0, 0, 256, 12, 0, 14),
        (0, 0, 12, 0, 0, 0, 0, 0, 180, 8, 0, 14),
        (0, 1, 12, 0, 0, 0, 0, 0, 180, 8, 0, 14),
        (0, 0, 12, 0, 0, 0, 0, 1, 256, 12, 0, 16),
        (0, 0, 12, 0, 0, 0, 0, 1, 180, 8, 0, 16),
        (0, 0, 12, 0, 0, 0, 18, 0, 256, 12, 4, 14),
        (0, 1, 12, 0, 0, 0, 18, 0, 256, 12, 4, 14),
        (0, 0, 12, 0, 0, 0, 18, 0, 180, 8, 4, 14),
        (0, 1, 12, 0, 0, 0, 18, 0, 180, 8, 4, 14),
        (0, 0, 12, 0, 0, 0, 18, 1, 256, 12, 4, 16),
        (0, 0, 12, 0, 0, 0, 18, 1, 180, 8, 4, 16),
        (0, 0, 12, 0, 0, 0, 30, 0, 256, 12, 4, 14),
        (0, 0, 12, 0, 0, 0, 30, 0, 180, 8, 4, 14),
        (0, 0, 12, 0, 0, 0, 30, 1, 256, 12, 4, 16),
        (0, 0, 12, 0, 0, 0, 30, 1, 180, 8, 4, 16),
        (0, 0, 12, 0, 0, 0, 5, 0, 256, 12, 0, 14),
        (0, 0, 12, 0, 0, 0, 5, 0, 180, 8, 0, 14),
        (0, 0, 12, 0, 0, 0, 5, 1, 256, 12, 0, 16),
        (0, 0, 12, 0, 0, 0, 5, 1, 180, 8, 0, 16),
        (0, 0, 12, 0, 0, 20, 0, 0, 256, 12, 12, 14),
        (0, 1, 12, 0, 0, 20, 0, 0, 256, 12, 12, 14),
        (0, 0, 12, 0, 0, 20, 0, 0, 180, 8, 12, 14),
        (0, 1, 12, 0, 0, 20, 0, 0, 180, 8, 12, 14),
        (0, 0, 12, 0, 0, 20, 0, 1, 256, 12, 12, 16),
        (0, 0, 12, 0, 0, 20, 0, 1, 180, 8, 12, 16),
        (0, 0, 12, 0, 0, 20, 18, 0, 256, 12, 24, 14),
        (0, 1, 12, 0, 0, 20, 18, 0, 256, 12, 24, 14),
        (0, 0, 12, 0, 0, 20, 18, 0, 180, 8, 24, 14),
        (0, 1, 12, 0, 0, 20, 18, 0, 180, 8, 24, 14),
        (0, 0, 12, 0, 0, 20, 18, 1, 256, 12, 24, 16),
        (0, 0, 12, 0, 0, 20, 18, 1, 180, 8, 24, 16),
        (0, 0, 12, 0, 0, 20, 30, 0, 256, 12, 24, 14),
        (0, 0, 12, 0, 0, 20, 30, 0, 180, 8, 24, 14),
        (0, 0, 12, 0, 0, 20, 30, 1, 256, 12, 24, 16),
        (0, 0, 12, 0, 0, 20, 30, 1, 180, 8, 24, 16),
        (0, 0, 12, 0, 0, 20, 5, 0, 256, 12, 12, 14),
        (0, 0, 12, 0, 0, 20, 5, 0, 180, 8, 12, 14),
        (0, 0, 12, 0, 0, 20, 5, 1, 256, 12, 12, 16),
        (0, 0, 12, 0, 0, 20, 5, 1, 180, 8, 12, 16),
        (0, 0, 12, 0, 0, 1025, 0, 0, 256, 12, 1033, 14),
        (0, 1, 12, 0, 0, 1025, 0, 0, 256, 12, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 0, 0, 180, 8, 1033, 14),
        (0, 1, 12, 0, 0, 1025, 0, 0, 180, 8, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 0, 1, 256, 12, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 0, 1, 180, 8, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 18, 0, 256, 12, 1033, 14),
        (0, 1, 12, 0, 0, 1025, 18, 0, 256, 12, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 18, 0, 180, 8, 1033, 14),
        (0, 1, 12, 0, 0, 1025, 18, 0, 180, 8, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 18, 1, 256, 12, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 18, 1, 180, 8, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 30, 0, 256, 12, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 30, 0, 180, 8, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 30, 1, 256, 12, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 30, 1, 180, 8, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 5, 0, 256, 12, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 5, 0, 180, 8, 1033, 14),
        (0, 0, 12, 0, 0, 1025, 5, 1, 256, 12, 1033, 16),
        (0, 0, 12, 0, 0, 1025, 5, 1, 180, 8, 1033, 16),
        (0, 0, 12, 0, 0, 2032, 0, 0, 256, 12, 2040, 14),
        (0, 1, 12, 0, 0, 2032, 0, 0, 256, 12, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 0, 0, 180, 8, 2040, 14),
        (0, 1, 12, 0, 0, 2032, 0, 0, 180, 8, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 0, 1, 256, 12, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 0, 1, 180, 8, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 18, 0, 256, 12, 2040, 14),
        (0, 1, 12, 0, 0, 2032, 18, 0, 256, 12, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 18, 0, 180, 8, 2040, 14),
        (0, 1, 12, 0, 0, 2032, 18, 0, 180, 8, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 18, 1, 256, 12, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 18, 1, 180, 8, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 30, 0, 256, 12, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 30, 0, 180, 8, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 30, 1, 256, 12, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 30, 1, 180, 8, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 5, 0, 256, 12, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 5, 0, 180, 8, 2040, 14),
        (0, 0, 12, 0, 0, 2032, 5, 1, 256, 12, 2040, 16),
        (0, 0, 12, 0, 0, 2032, 5, 1, 180, 8, 2040, 16),
        (0, 0, 12, 0, 2047, 0, 0, 0, 256, 11, 0, 14),
        (0, 1, 12, 0, 2047, 0, 0, 0, 256, 11, 0, 14),
        (0, 0, 12, 0, 2047, 0, 0, 0, 180, 7, 0, 14),
        (0, 1, 12, 0, 2047, 0, 0, 0, 180, 7, 0, 14),
        (0, 0, 12, 0, 2047, 0, 0, 1, 256, 11, 0, 16),
        (0, 0, 12, 0, 2047, 0, 0, 1, 180, 7, 0, 16),
        (0, 0, 12, 0, 2047, 0, 18, 0, 256, 11, 4, 14),
        (0, 1, 12, 0, 2047, 0, 18, 0, 256, 11, 4, 14),
        (0, 0, 12, 0, 2047, 0, 18, 0, 180, 7, 4, 14),
        (0, 1, 12, 0, 2047, 0, 18, 0, 180, 7, 4, 14),
        (0, 0, 12, 0, 2047, 0, 18, 1, 256, 11, 4, 16),
        (0, 0, 12, 0, 2047, 0, 18, 1, 180, 7, 4, 16),
        (0, 0, 12, 0, 2047, 0, 30, 0, 256, 11, 4, 14),
        (0, 0, 12, 0, 2047, 0, 30, 0, 180, 7, 4, 14),
        (0, 0, 12, 0, 2047, 0, 30, 1, 256, 11, 4, 16),
        (0, 0, 12, 0, 2047, 0, 30, 1, 180, 7, 4, 16),
        (0, 0, 12, 0, 2047, 0, 5, 0, 256, 11, 0, 14),
        (0, 0, 12, 0, 2047, 0, 5, 0, 180, 7, 0, 14),
        (0, 0, 12, 0, 2047, 0, 5, 1, 256, 11, 0, 16),
        (0, 0, 12, 0, 2047, 0, 5, 1, 180, 7, 0, 16),
        (0, 0, 12, 0, 2047, 20, 0, 0, 256, 11, 12, 14),
        (0, 1, 12, 0, 2047, 20, 0, 0, 256, 11, 12, 14),
        (0, 0, 12, 0, 2047, 20, 0, 0, 180, 7, 12, 14),
        (0, 1, 12, 0, 2047, 20, 0, 0, 180, 7, 12, 14),
        (0, 0, 12, 0, 2047, 20, 0, 1, 256, 11, 12, 16),
        (0, 0, 12, 0, 2047, 20, 0, 1, 180, 7, 12, 16),
        (0, 0, 12, 0, 2047, 20, 18, 0, 256, 11, 24, 14),
        (0, 1, 12, 0, 2047, 20, 18, 0, 256, 11, 24, 14),
        (0, 0, 12, 0, 2047, 20, 18, 0, 180, 7, 24, 14),
        (0, 1, 12, 0, 2047, 20, 18, 0, 180, 7, 24, 14),
        (0, 0, 12, 0, 2047, 20, 18, 1, 256, 11, 24, 16),
        (0, 0, 12, 0, 2047, 20, 18, 1, 180, 7, 24, 16),
        (0, 0, 12, 0, 2047, 20, 30, 0, 256, 11, 24, 14),
        (0, 0, 12, 0, 2047, 20, 30, 0, 180, 7, 24, 14),
        (0, 0, 12, 0, 2047, 20, 30, 1, 256, 11, 24, 16),
        (0, 0, 12, 0, 2047, 20, 30, 1, 180, 7, 24, 16),
        (0, 0, 12, 0, 2047, 20, 5, 0, 256, 11, 12, 14),
        (0, 0, 12, 0, 2047, 20, 5, 0, 180, 7, 12, 14),
        (0, 0, 12, 0, 2047, 20, 5, 1, 256, 11, 12, 16),
        (0, 0, 12, 0, 2047, 20, 5, 1, 180, 7, 12, 16),
        (0, 0, 12, 0, 2047, 1025, 0, 0, 256, 11, 1033, 14),
        (0, 1, 12, 0, 2047, 1025, 0, 0, 256, 11, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 0, 0, 180, 7, 1033, 14),
        (0, 1, 12, 0, 2047, 1025, 0, 0, 180, 7, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 0, 1, 256, 11, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 0, 1, 180, 7, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 18, 0, 256, 11, 1033, 14),
        (0, 1, 12, 0, 2047, 1025, 18, 0, 256, 11, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 18, 0, 180, 7, 1033, 14),
        (0, 1, 12, 0, 2047, 1025, 18, 0, 180, 7, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 18, 1, 256, 11, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 18, 1, 180, 7, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 30, 0, 256, 11, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 30, 0, 180, 7, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 30, 1, 256, 11, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 30, 1, 180, 7, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 5, 0, 256, 11, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 5, 0, 180, 7, 1033, 14),
        (0, 0, 12, 0, 2047, 1025, 5, 1, 256, 11, 1033, 16),
        (0, 0, 12, 0, 2047, 1025, 5, 1, 180, 7, 1033, 16),
        (0, 0, 12, 0, 2047, 2032, 0, 0, 256, 11, 2040, 14),
        (0, 1, 12, 0, 2047, 2032, 0, 0, 256, 11, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 0, 0, 180, 7, 2040, 14),
        (0, 1, 12, 0, 2047, 2032, 0, 0, 180, 7, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 0, 1, 256, 11, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 0, 1, 180, 7, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 18, 0, 256, 11, 2040, 14),
        (0, 1, 12, 0, 2047, 2032, 18, 0, 256, 11, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 18, 0, 180, 7, 2040, 14),
        (0, 1, 12, 0, 2047, 2032, 18, 0, 180, 7, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 18, 1, 256, 11, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 18, 1, 180, 7, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 30, 0, 256, 11, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 30, 0, 180, 7, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 30, 1, 256, 11, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 30, 1, 180, 7, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 5, 0, 256, 11, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 5, 0, 180, 7, 2040, 14),
        (0, 0, 12, 0, 2047, 2032, 5, 1, 256, 11, 2040, 16),
        (0, 0, 12, 0, 2047, 2032, 5, 1, 180, 7, 2040, 16),
        (0, 0, 0, 12, 0, 0, 0, 0, 256, 2036, 0, 15),
        (0, 1, 0, 12, 0, 0, 0, 0, 256, 2036, 0, 15),
        (0, 0, 0, 12, 0, 0, 0, 1, 256, 2036, 0, 17),
        (0, 0, 0, 12, 0, 0, 18, 0, 256, 2036, 2040, 15),
        (0, 1, 0, 12, 0, 0, 18, 0, 256, 2036, 2040, 15),
        (0, 0, 0, 12, 0, 0, 18, 1, 256, 2036, 2040, 17),
        (0, 0, 0, 12, 0, 0, 30, 0, 256, 2036, 2040, 15),
        (0, 0, 0, 12, 0, 0, 30, 1, 256, 2036, 2040, 17),
        (0, 0, 0, 12, 0, 0, 5, 0, 256, 2036, 0, 15),
        (0, 0, 0, 12, 0, 0, 5, 1, 256, 2036, 0, 17),
        (0, 0, 0, 12, 0, 20, 0, 0, 256, 2036, 12, 15),
        (0, 1, 0, 12, 0, 20, 0, 0, 256, 2036, 12, 15),
        (0, 0, 0, 12, 0, 20, 0, 1, 256, 2036, 12, 17),
        (0, 0, 0, 12, 0, 20, 18, 0, 256, 2036, 12, 15),
        (0, 1, 0, 12, 0, 20, 18, 0, 256, 2036, 12, 15),
        (0, 0, 0, 12, 0, 20, 18, 1, 256, 2036, 12, 17),
        (0, 0, 0, 12, 0, 20, 30, 0, 256, 2036, 12, 15),
        (0, 0, 0, 12, 0, 20, 30, 1, 256, 2036, 12, 17),
        (0, 0, 0, 12, 0, 20, 5, 0, 256, 2036, 12, 15),
        (0, 0, 0, 12, 0, 20, 5, 1, 256, 2036, 12, 17),
        (0, 0, 0, 12, 0, 1025, 0, 0, 256, 2036, 1033, 15),
        (0, 1, 0, 12, 0, 1025, 0, 0, 256, 2036, 1033, 15),
        (0, 0, 0, 12, 0, 1025, 0, 1, 256, 2036, 1033, 17),
        (0, 0, 0, 12, 0, 1025, 18, 0, 256, 2036, 1025, 15),
        (0, 1, 0, 12, 0, 1025, 18, 0, 256, 2036, 1025, 15),
        (0, 0, 0, 12, 0, 1025, 18, 1, 256, 2036, 1025, 17),
        (0, 0, 0, 12, 0, 1025, 30, 0, 256, 2036, 1025, 15),
        (0, 0, 0, 12, 0, 1025, 30, 1, 256, 2036, 1025, 17),
        (0, 0, 0, 12, 0, 1025, 5, 0, 256, 2036, 1033, 15),
        (0, 0, 0, 12, 0, 1025, 5, 1, 256, 2036, 1033, 17),
        (0, 0, 0, 12, 0, 2032, 0, 0, 256, 2036, 2040, 15),
        (0, 1, 0, 12, 0, 2032, 0, 0, 256, 2036, 2040, 15),
        (0, 0, 0, 12, 0, 2032, 0, 1, 256, 2036, 2040, 17),
        (0, 0, 0, 12, 0, 2032, 18, 0, 256, 2036, 2028, 15),
        (0, 1, 0, 12, 0, 2032, 18, 0, 256, 2036, 2028, 15),
        (0, 0, 0, 12, 0, 2032, 18, 1, 256, 2036, 2028, 17),
        (0, 0, 0, 12, 0, 2032, 30, 0, 256, 2036, 2028, 15),
        (0, 0, 0, 12, 0, 2032, 30, 1, 256, 2036, 2028, 17),
        (0, 0, 0, 12, 0, 2032, 5, 0, 256, 2036, 2040, 15),
        (0, 0, 0, 12, 0, 2032, 5, 1, 256, 2036, 2040, 17),
        (0, 0, 0, 12, 2047, 0, 0, 0, 256, 2035, 0, 15),
        (0, 1, 0, 12, 2047, 0, 0, 0, 256, 2035, 0, 15),
        (0, 0, 0, 12, 2047, 0, 0, 1, 256, 2035, 0, 17),
        (0, 0, 0, 12, 2047, 0, 18, 0, 256, 2035, 2040, 15),
        (0, 1, 0, 12, 2047, 0, 18, 0, 256, 2035, 2040, 15),
        (0, 0, 0, 12, 2047, 0, 18, 1, 256, 2035, 2040, 17),
        (0, 0, 0, 12, 2047, 0, 30, 0, 256, 2035, 2040, 15),
        (0, 0, 0, 12, 2047, 0, 30, 1, 256, 2035, 2040, 17),
        (0, 0, 0, 12, 2047, 0, 5, 0, 256, 2035, 0, 15),
        (0, 0, 0, 12, 2047, 0, 5, 1, 256, 2035, 0, 17),
        (0, 0, 0, 12, 2047, 20, 0, 0, 256, 2035, 12, 15),
        (0, 1, 0, 12, 2047, 20, 0, 0, 256, 2035, 12, 15),
        (0, 0, 0, 12, 2047, 20, 0, 1, 256, 2035, 12, 17),
        (0, 0, 0, 12, 2047, 20, 18, 0, 256, 2035, 12, 15),
        (0, 1, 0, 12, 2047, 20, 18, 0, 256, 2035, 12, 15),
        (0, 0, 0, 12, 2047, 20, 18, 1, 256, 2035, 12, 17),
        (0, 0, 0, 12, 2047, 20, 30, 0, 256, 2035, 12, 15),
        (0, 0, 0, 12, 2047, 20, 30, 1, 256, 2035, 12, 17),
        (0, 0, 0, 12, 2047, 20, 5, 0, 256, 2035, 12, 15),
        (0, 0, 0, 12, 2047, 20, 5, 1, 256, 2035, 12, 17),
        (0, 0, 0, 12, 2047, 1025, 0, 0, 256, 2035, 1033, 15),
        (0, 1, 0, 12, 2047, 1025, 0, 0, 256, 2035, 1033, 15),
        (0, 0, 0, 12, 2047, 1025, 0, 1, 256, 2035, 1033, 17),
        (0, 0, 0, 12, 2047, 1025, 18, 0, 256, 2035, 1025, 15),
        (0, 1, 0, 12, 2047, 1025, 18, 0, 256, 2035, 1025, 15),
        (0, 0, 0, 12, 2047, 1025, 18, 1, 256, 2035, 1025, 17),
        (0, 0, 0, 12, 2047, 1025, 30, 0, 256, 2035, 1025, 15),
        (0, 0, 0, 12, 2047, 1025, 30, 1, 256, 2035, 1025, 17),
        (0, 0, 0, 12, 2047, 1025, 5, 0, 256, 2035, 1033, 15),
        (0, 0, 0, 12, 2047, 1025, 5, 1, 256, 2035, 1033, 17),
        (0, 0, 0, 12, 2047, 2032, 0, 0, 256, 2035, 2040, 15),
        (0, 1, 0, 12, 2047, 2032, 0, 0, 256, 2035, 2040, 15),
        (0, 0, 0, 12, 2047, 2032, 0, 1, 256, 2035, 2040, 17),
        (0, 0, 0, 12, 2047, 2032, 18, 0, 256, 2035, 2028, 15),
        (0, 1, 0, 12, 2047, 2032, 18, 0, 256, 2035, 2028, 15),
        (0, 0, 0, 12, 2047, 2032, 18, 1, 256, 2035, 2028, 17),
        (0, 0, 0, 12, 2047, 2032, 30, 0, 256, 2035, 2028, 15),
        (0, 0, 0, 12, 2047, 2032, 30, 1, 256, 2035, 2028, 17),
        (0, 0, 0, 12, 2047, 2032, 5, 0, 256, 2035, 2040, 15),
        (0, 0, 0, 12, 2047, 2032, 5, 1, 256, 2035, 2040, 17),
        (0, 0, 36, 0, 0, 0, 0, 0, 256, 36, 0, 14),
        (0, 1, 36, 0, 0, 0, 0, 0, 256, 36, 0, 14),
        (0, 0, 36, 0, 0, 0, 0, 1, 256, 36, 0, 16),
        (0, 0, 36, 0, 0, 0, 18, 0, 256, 36, 4, 14),
        (0, 1, 36, 0, 0, 0, 18, 0, 256, 36, 4, 14),
        (0, 0, 36, 0, 0, 0, 18, 1, 256, 36, 4, 16),
        (0, 0, 36, 0, 0, 0, 30, 0, 256, 36, 4, 14),
        (0, 0, 36, 0, 0, 0, 30, 1, 256, 36, 4, 16),
        (0, 0, 36, 0, 0, 0, 5, 0, 256, 36, 0, 14),
        (0, 0, 36, 0, 0, 0, 5, 1, 256, 36, 0, 16),
        (0, 0, 36, 0, 0, 20, 0, 0, 256, 36, 12, 14),
        (0, 1, 36, 0, 0, 20, 0, 0, 256, 36, 12, 14),
        (0, 0, 36, 0, 0, 20, 0, 1, 256, 36, 12, 16),
        (0, 0, 36, 0, 0, 20, 18, 0, 256, 36, 24, 14),
        (0, 1, 36, 0, 0, 20, 18, 0, 256, 36, 24, 14),
        (0, 0, 36, 0, 0, 20, 18, 1, 256, 36, 24, 16),
        (0, 0, 36, 0, 0, 20, 30, 0, 256, 36, 24, 14),
        (0, 0, 36, 0, 0, 20, 30, 1, 256, 36, 24, 16),
        (0, 0, 36, 0, 0, 20, 5, 0, 256, 36, 12, 14),
        (0, 0, 36, 0, 0, 20, 5, 1, 256, 36, 12, 16),
        (0, 0, 36, 0, 0, 1025, 0, 0, 256, 36, 1033, 14),
        (0, 1, 36, 0, 0, 1025, 0, 0, 256, 36, 1033, 14),
        (0, 0, 36, 0, 0, 1025, 0, 1, 256, 36, 1033, 16),
        (0, 0, 36, 0, 0, 1025, 18, 0, 256, 36, 1033, 14),
        (0, 1, 36, 0, 0, 1025, 18, 0, 256, 36, 1033, 14),
        (0, 0, 36, 0, 0, 1025, 18, 1, 256, 36, 1033, 16),
        (0, 0, 36, 0, 0, 1025, 30, 0, 256, 36, 1033, 14),
        (0, 0, 36, 0, 0, 1025, 30, 1, 256, 36, 1033, 16),
        (0, 0, 36, 0, 0, 1025, 5, 0, 256, 36, 1033, 14),
        (0, 0, 36, 0, 0, 1025, 5, 1, 256, 36, 1033, 16),
        (0, 0, 36, 0, 0, 2032, 0, 0, 256, 36, 2040, 14),
        (0, 1, 36, 0, 0, 2032, 0, 0, 256, 36, 2040, 14),
        (0, 0, 36, 0, 0, 2032, 0, 1, 256, 36, 2040, 16),
        (0, 0, 36, 0, 0, 2032, 18, 0, 256, 36, 2040, 14),
        (0, 1, 36, 0, 0, 2032, 18, 0, 256, 36, 2040, 14),
        (0, 0, 36, 0, 0, 2032, 18, 1, 256, 36, 2040, 16),
        (0, 0, 36, 0, 0, 2032, 30, 0, 256, 36, 2040, 14),
        (0, 0, 36, 0, 0, 2032, 30, 1, 256, 36, 2040, 16),
        (0, 0, 36, 0, 0, 2032, 5, 0, 256, 36, 2040, 14),
        (0, 0, 36, 0, 0, 2032, 5, 1, 256, 36, 2040, 16),
        (0, 0, 36, 0, 2047, 0, 0, 0, 256, 35, 0, 14),
        (0, 1, 36, 0, 2047, 0, 0, 0, 256, 35, 0, 14),
        (0, 0, 36, 0, 2047, 0, 0, 1, 256, 35, 0, 16),
        (0, 0, 36, 0, 2047, 0, 18, 0, 256, 35, 4, 14),
        (0, 1, 36, 0, 2047, 0, 18, 0, 256, 35, 4, 14),
        (0, 0, 36, 0, 2047, 0, 18, 1, 256, 35, 4, 16),
        (0, 0, 36, 0, 2047, 0, 30, 0, 256, 35, 4, 14),
        (0, 0, 36, 0, 2047, 0, 30, 1, 256, 35, 4, 16),
        (0, 0, 36, 0, 2047, 0, 5, 0, 256, 35, 0, 14),
        (0, 0, 36, 0, 2047, 0, 5, 1, 256, 35, 0, 16),
        (0, 0, 36, 0, 2047, 20, 0, 0, 256, 35, 12, 14),
        (0, 1, 36, 0, 2047, 20, 0, 0, 256, 35, 12, 14),
        (0, 0, 36, 0, 2047, 20, 0, 1, 256, 35, 12, 16),
        (0, 0, 36, 0, 2047, 20, 18, 0, 256, 35, 24, 14),
        (0, 1, 36, 0, 2047, 20, 18, 0, 256, 35, 24, 14),
        (0, 0, 36, 0, 2047, 20, 18, 1, 256, 35, 24, 16),
        (0, 0, 36, 0, 2047, 20, 30, 0, 256, 35, 24, 14),
        (0, 0, 36, 0, 2047, 20, 30, 1, 256, 35, 24, 16),
        (0, 0, 36, 0, 2047, 20, 5, 0, 256, 35, 12, 14),
        (0, 0, 36, 0, 2047, 20, 5, 1, 256, 35, 12, 16),
        (0, 0, 36, 0, 2047, 1025, 0, 0, 256, 35, 1033, 14),
        (0, 1, 36, 0, 2047, 1025, 0, 0, 256, 35, 1033, 14),
        (0, 0, 36, 0, 2047, 1025, 0, 1, 256, 35, 1033, 16),
        (0, 0, 36, 0, 2047, 1025, 18, 0, 256, 35, 1033, 14),
        (0, 1, 36, 0, 2047, 1025, 18, 0, 256, 35, 1033, 14),
        (0, 0, 36, 0, 2047, 1025, 18, 1, 256, 35, 1033, 16),
        (0, 0, 36, 0, 2047, 1025, 30, 0, 256, 35, 1033, 14),
        (0, 0, 36, 0, 2047, 1025, 30, 1, 256, 35, 1033, 16),
        (0, 0, 36, 0, 2047, 1025, 5, 0, 256, 35, 1033, 14),
        (0, 0, 36, 0, 2047, 1025, 5, 1, 256, 35, 1033, 16),
        (0, 0, 36, 0, 2047, 2032, 0, 0, 256, 35, 2040, 14),
        (0, 1, 36, 0, 2047, 2032, 0, 0, 256, 35, 2040, 14),
        (0, 0, 36, 0, 2047, 2032, 0, 1, 256, 35, 2040, 16),
        (0, 0, 36, 0, 2047, 2032, 18, 0, 256, 35, 2040, 14),
        (0, 1, 36, 0, 2047, 2032, 18, 0, 256, 35, 2040, 14),
        (0, 0, 36, 0, 2047, 2032, 18, 1, 256, 35, 2040, 16),
        (0, 0, 36, 0, 2047, 2032, 30, 0, 256, 35, 2040, 14),
        (0, 0, 36, 0, 2047, 2032, 30, 1, 256, 35, 2040, 16),
        (0, 0, 36, 0, 2047, 2032, 5, 0, 256, 35, 2040, 14),
        (0, 0, 36, 0, 2047, 2032, 5, 1, 256, 35, 2040, 16),
        (0, 0, 0, 48, 0, 0, 0, 0, 256, 2000, 0, 15),
        (0, 1, 0, 48, 0, 0, 0, 0, 256, 2000, 0, 15),
        (0, 0, 0, 48, 0, 0, 0, 0, 180, 2015, 0, 15),
        (0, 1, 0, 48, 0, 0, 0, 0, 180, 2015, 0, 15),
        (0, 0, 0, 48, 0, 0, 0, 1, 256, 2000, 0, 17),
        (0, 0, 0, 48, 0, 0, 0, 1, 180, 2015, 0, 17),
        (0, 0, 0, 48, 0, 0, 18, 0, 256, 2000, 2040, 15),
        (0, 1, 0, 48, 0, 0, 18, 0, 256, 2000, 2040, 15),
        (0, 0, 0, 48, 0, 0, 18, 0, 180, 2015, 2040, 15),
        (0, 1, 0, 48, 0, 0, 18, 0, 180, 2015, 2040, 15),
        (0, 0, 0, 48, 0, 0, 18, 1, 256, 2000, 2040, 17),
        (0, 0, 0, 48, 0, 0, 18, 1, 180, 2015, 2040, 17),
        (0, 0, 0, 48, 0, 0, 30, 0, 256, 2000, 2040, 15),
        (0, 0, 0, 48, 0, 0, 30, 0, 180, 2015, 2040, 15),
        (0, 0, 0, 48, 0, 0, 30, 1, 256, 2000, 2040, 17),
        (0, 0, 0, 48, 0, 0, 30, 1, 180, 2015, 2040, 17),
        (0, 0, 0, 48, 0, 0, 5, 0, 256, 2000, 0, 15),
        (0, 0, 0, 48, 0, 0, 5, 0, 180, 2015, 0, 15),
        (0, 0, 0, 48, 0, 0, 5, 1, 256, 2000, 0, 17),
        (0, 0, 0, 48, 0, 0, 5, 1, 180, 2015, 0, 17),
        (0, 0, 0, 48, 0, 20, 0, 0, 256, 2000, 12, 15),
        (0, 1, 0, 48, 0, 20, 0, 0, 256, 2000, 12, 15),
        (0, 0, 0, 48, 0, 20, 0, 0, 180, 2015, 12, 15),
        (0, 1, 0, 48, 0, 20, 0, 0, 180, 2015, 12, 15),
        (0, 0, 0, 48, 0, 20, 0, 1, 256, 2000, 12, 17),
        (0, 0, 0, 48, 0, 20, 0, 1, 180, 2015, 12, 17),
        (0, 0, 0, 48, 0, 20, 18, 0, 256, 2000, 12, 15),
        (0, 1, 0, 48, 0, 20, 18, 0, 256, 2000, 12, 15),
        (0, 0, 0, 48, 0, 20, 18, 0, 180, 2015, 12, 15),
        (0, 1, 0, 48, 0, 20, 18, 0, 180, 2015, 12, 15),
        (0, 0, 0, 48, 0, 20, 18, 1, 256, 2000, 12, 17),
        (0, 0, 0, 48, 0, 20, 18, 1, 180, 2015, 12, 17),
        (0, 0, 0, 48, 0, 20, 30, 0, 256, 2000, 12, 15),
        (0, 0, 0, 48, 0, 20, 30, 0, 180, 2015, 12, 15),
        (0, 0, 0, 48, 0, 20, 30, 1, 256, 2000, 12, 17),
        (0, 0, 0, 48, 0, 20, 30, 1, 180, 2015, 12, 17),
        (0, 0, 0, 48, 0, 20, 5, 0, 256, 2000, 12, 15),
        (0, 0, 0, 48, 0, 20, 5, 0, 180, 2015, 12, 15),
        (0, 0, 0, 48, 0, 20, 5, 1, 256, 2000, 12, 17),
        (0, 0, 0, 48, 0, 20, 5, 1, 180, 2015, 12, 17),
        (0, 0, 0, 48, 0, 1025, 0, 0, 256, 2000, 1033, 15),
        (0, 1, 0, 48, 0, 1025, 0, 0, 256, 2000, 1033, 15),
        (0, 0, 0, 48, 0, 1025, 0, 0, 180, 2015, 1033, 15),
        (0, 1, 0, 48, 0, 1025, 0, 0, 180, 2015, 1033, 15),
        (0, 0, 0, 48, 0, 1025, 0, 1, 256, 2000, 1033, 17),
        (0, 0, 0, 48, 0, 1025, 0, 1, 180, 2015, 1033, 17),
        (0, 0, 0, 48, 0, 1025, 18, 0, 256, 2000, 1025, 15),
        (0, 1, 0, 48, 0, 1025, 18, 0, 256, 2000, 1025, 15),
        (0, 0, 0, 48, 0, 1025, 18, 0, 180, 2015, 1025, 15),
        (0, 1, 0, 48, 0, 1025, 18, 0, 180, 2015, 1025, 15),
        (0, 0, 0, 48, 0, 1025, 18, 1, 256, 2000, 1025, 17),
        (0, 0, 0, 48, 0, 1025, 18, 1, 180, 2015, 1025, 17),
        (0, 0, 0, 48, 0, 1025, 30, 0, 256, 2000, 1025, 15),
        (0, 0, 0, 48, 0, 1025, 30, 0, 180, 2015, 1025, 15),
        (0, 0, 0, 48, 0, 1025, 30, 1, 256, 2000, 1025, 17),
        (0, 0, 0, 48, 0, 1025, 30, 1, 180, 2015, 1025, 17),
        (0, 0, 0, 48, 0, 1025, 5, 0, 256, 2000, 1033, 15),
        (0, 0, 0, 48, 0, 1025, 5, 0, 180, 2015, 1033, 15),
        (0, 0, 0, 48, 0, 1025, 5, 1, 256, 2000, 1033, 17),
        (0, 0, 0, 48, 0, 1025, 5, 1, 180, 2015, 1033, 17),
        (0, 0, 0, 48, 0, 2032, 0, 0, 256, 2000, 2040, 15),
        (0, 1, 0, 48, 0, 2032, 0, 0, 256, 2000, 2040, 15),
        (0, 0, 0, 48, 0, 2032, 0, 0, 180, 2015, 2040, 15),
        (0, 1, 0, 48, 0, 2032, 0, 0, 180, 2015, 2040, 15),
        (0, 0, 0, 48, 0, 2032, 0, 1, 256, 2000, 2040, 17),
        (0, 0, 0, 48, 0, 2032, 0, 1, 180, 2015, 2040, 17),
        (0, 0, 0, 48, 0, 2032, 18, 0, 256, 2000, 2028, 15),
        (0, 1, 0, 48, 0, 2032, 18, 0, 256, 2000, 2028, 15),
        (0, 0, 0, 48, 0, 2032, 18, 0, 180, 2015, 2028, 15),
        (0, 1, 0, 48, 0, 2032, 18, 0, 180, 2015, 2028, 15),
        (0, 0, 0, 48, 0, 2032, 18, 1, 256, 2000, 2028, 17),
        (0, 0, 0, 48, 0, 2032, 18, 1, 180, 2015, 2028, 17),
        (0, 0, 0, 48, 0, 2032, 30, 0, 256, 2000, 2028, 15),
        (0, 0, 0, 48, 0, 2032, 30, 0, 180, 2015, 2028, 15),
        (0, 0, 0, 48, 0, 2032, 30, 1, 256, 2000, 2028, 17),
        (0, 0, 0, 48, 0, 2032, 30, 1, 180, 2015, 2028, 17),
        (0, 0, 0, 48, 0, 2032, 5, 0, 256, 2000, 2040, 15),
        (0, 0, 0, 48, 0, 2032, 5, 0, 180, 2015, 2040, 15),
        (0, 0, 0, 48, 0, 2032, 5, 1, 256, 2000, 2040, 17),
        (0, 0, 0, 48, 0, 2032, 5, 1, 180, 2015, 2040, 17),
        (0, 0, 0, 48, 2047, 0, 0, 0, 256, 1999, 0, 15),
        (0, 1, 0, 48, 2047, 0, 0, 0, 256, 1999, 0, 15),
        (0, 0, 0, 48, 2047, 0, 0, 0, 180, 2014, 0, 15),
        (0, 1, 0, 48, 2047, 0, 0, 0, 180, 2014, 0, 15),
        (0, 0, 0, 48, 2047, 0, 0, 1, 256, 1999, 0, 17),
        (0, 0, 0, 48, 2047, 0, 0, 1, 180, 2014, 0, 17),
        (0, 0, 0, 48, 2047, 0, 18, 0, 256, 1999, 2040, 15),
        (0, 1, 0, 48, 2047, 0, 18, 0, 256, 1999, 2040, 15),
        (0, 0, 0, 48, 2047, 0, 18, 0, 180, 2014, 2040, 15),
        (0, 1, 0, 48, 2047, 0, 18, 0, 180, 2014, 2040, 15),
        (0, 0, 0, 48, 2047, 0, 18, 1, 256, 1999, 2040, 17),
        (0, 0, 0, 48, 2047, 0, 18, 1, 180, 2014, 2040, 17),
        (0, 0, 0, 48, 2047, 0, 30, 0, 256, 1999, 2040, 15),
        (0, 0, 0, 48, 2047, 0, 30, 0, 180, 2014, 2040, 15),
        (0, 0, 0, 48, 2047, 0, 30, 1, 256, 1999, 2040, 17),
        (0, 0, 0, 48, 2047, 0, 30, 1, 180, 2014, 2040, 17),
        (0, 0, 0, 48, 2047, 0, 5, 0, 256, 1999, 0, 15),
        (0, 0, 0, 48, 2047, 0, 5, 0, 180, 2014, 0, 15),
        (0, 0, 0, 48, 2047, 0, 5, 1, 256, 1999, 0, 17),
        (0, 0, 0, 48, 2047, 0, 5, 1, 180, 2014, 0, 17),
        (0, 0, 0, 48, 2047, 20, 0, 0, 256, 1999, 12, 15),
        (0, 1, 0, 48, 2047, 20, 0, 0, 256, 1999, 12, 15),
        (0, 0, 0, 48, 2047, 20, 0, 0, 180, 2014, 12, 15),
        (0, 1, 0, 48, 2047, 20, 0, 0, 180, 2014, 12, 15),
        (0, 0, 0, 48, 2047, 20, 0, 1, 256, 1999, 12, 17),
        (0, 0, 0, 48, 2047, 20, 0, 1, 180, 2014, 12, 17),
        (0, 0, 0, 48, 2047, 20, 18, 0, 256, 1999, 12, 15),
        (0, 1, 0, 48, 2047, 20, 18, 0, 256, 1999, 12, 15),
        (0, 0, 0, 48, 2047, 20, 18, 0, 180, 2014, 12, 15),
        (0, 1, 0, 48, 2047, 20, 18, 0, 180, 2014, 12, 15),
        (0, 0, 0, 48, 2047, 20, 18, 1, 256, 1999, 12, 17),
        (0, 0, 0, 48, 2047, 20, 18, 1, 180, 2014, 12, 17),
        (0, 0, 0, 48, 2047, 20, 30, 0, 256, 1999, 12, 15),
        (0, 0, 0, 48, 2047, 20, 30, 0, 180, 2014, 12, 15),
        (0, 0, 0, 48, 2047, 20, 30, 1, 256, 1999, 12, 17),
        (0, 0, 0, 48, 2047, 20, 30, 1, 180, 2014, 12, 17),
        (0, 0, 0, 48, 2047, 20, 5, 0, 256, 1999, 12, 15),
        (0, 0, 0, 48, 2047, 20, 5, 0, 180, 2014, 12, 15),
        (0, 0, 0, 48, 2047, 20, 5, 1, 256, 1999, 12, 17),
        (0, 0, 0, 48, 2047, 20, 5, 1, 180, 2014, 12, 17),
        (0, 0, 0, 48, 2047, 1025, 0, 0, 256, 1999, 1033, 15),
        (0, 1, 0, 48, 2047, 1025, 0, 0, 256, 1999, 1033, 15),
        (0, 0, 0, 48, 2047, 1025, 0, 0, 180, 2014, 1033, 15),
        (0, 1, 0, 48, 2047, 1025, 0, 0, 180, 2014, 1033, 15),
        (0, 0, 0, 48, 2047, 1025, 0, 1, 256, 1999, 1033, 17),
        (0, 0, 0, 48, 2047, 1025, 0, 1, 180, 2014, 1033, 17),
        (0, 0, 0, 48, 2047, 1025, 18, 0, 256, 1999, 1025, 15),
        (0, 1, 0, 48, 2047, 1025, 18, 0, 256, 1999, 1025, 15),
        (0, 0, 0, 48, 2047, 1025, 18, 0, 180, 2014, 1025, 15),
        (0, 1, 0, 48, 2047, 1025, 18, 0, 180, 2014, 1025, 15),
        (0, 0, 0, 48, 2047, 1025, 18, 1, 256, 1999, 1025, 17),
        (0, 0, 0, 48, 2047, 1025, 18, 1, 180, 2014, 1025, 17),
        (0, 0, 0, 48, 2047, 1025, 30, 0, 256, 1999, 1025, 15),
        (0, 0, 0, 48, 2047, 1025, 30, 0, 180, 2014, 1025, 15),
        (0, 0, 0, 48, 2047, 1025, 30, 1, 256, 1999, 1025, 17),
        (0, 0, 0, 48, 2047, 1025, 30, 1, 180, 2014, 1025, 17),
        (0, 0, 0, 48, 2047, 1025, 5, 0, 256, 1999, 1033, 15),
        (0, 0, 0, 48, 2047, 1025, 5, 0, 180, 2014, 1033, 15),
        (0, 0, 0, 48, 2047, 1025, 5, 1, 256, 1999, 1033, 17),
        (0, 0, 0, 48, 2047, 1025, 5, 1, 180, 2014, 1033, 17),
        (0, 0, 0, 48, 2047, 2032, 0, 0, 256, 1999, 2040, 15),
        (0, 1, 0, 48, 2047, 2032, 0, 0, 256, 1999, 2040, 15),
        (0, 0, 0, 48, 2047, 2032, 0, 0, 180, 2014, 2040, 15),
        (0, 1, 0, 48, 2047, 2032, 0, 0, 180, 2014, 2040, 15),
        (0, 0, 0, 48, 2047, 2032, 0, 1, 256, 1999, 2040, 17),
        (0, 0, 0, 48, 2047, 2032, 0, 1, 180, 2014, 2040, 17),
        (0, 0, 0, 48, 2047, 2032, 18, 0, 256, 1999, 2028, 15),
        (0, 1, 0, 48, 2047, 2032, 18, 0, 256, 1999, 2028, 15),
        (0, 0, 0, 48, 2047, 2032, 18, 0, 180, 2014, 2028, 15),
        (0, 1, 0, 48, 2047, 2032, 18, 0, 180, 2014, 2028, 15),
        (0, 0, 0, 48, 2047, 2032, 18, 1, 256, 1999, 2028, 17),
        (0, 0, 0, 48, 2047, 2032, 18, 1, 180, 2014, 2028, 17),
        (0, 0, 0, 48, 2047, 2032, 30, 0, 256, 1999, 2028, 15),
        (0, 0, 0, 48, 2047, 2032, 30, 0, 180, 2014, 2028, 15),
        (0, 0, 0, 48, 2047, 2032, 30, 1, 256, 1999, 2028, 17),
        (0, 0, 0, 48, 2047, 2032, 30, 1, 180, 2014, 2028, 17),
        (0, 0, 0, 48, 2047, 2032, 5, 0, 256, 1999, 2040, 15),
        (0, 0, 0, 48, 2047, 2032, 5, 0, 180, 2014, 2040, 15),
        (0, 0, 0, 48, 2047, 2032, 5, 1, 256, 1999, 2040, 17),
        (0, 0, 0, 48, 2047, 2032, 5, 1, 180, 2014, 2040, 17),
        (0, 0, 24, 24, 0, 0, 0, 0, 256, 0, 0, -1),
        (0, 1, 24, 24, 0, 0, 0, 0, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 0, 1, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 18, 0, 256, 0, 0, -1),
        (0, 1, 24, 24, 0, 0, 18, 0, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 18, 1, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 30, 0, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 30, 1, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 5, 0, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 0, 5, 1, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 20, 0, 0, 256, 0, 4, -1),
        (0, 1, 24, 24, 0, 20, 0, 0, 256, 0, 4, -1),
        (0, 0, 24, 24, 0, 20, 0, 1, 256, 0, 4, -1),
        (0, 0, 24, 24, 0, 20, 18, 0, 256, 0, 12, -1),
        (0, 1, 24, 24, 0, 20, 18, 0, 256, 0, 12, -1),
        (0, 0, 24, 24, 0, 20, 18, 1, 256, 0, 12, -1),
        (0, 0, 24, 24, 0, 20, 30, 0, 256, 0, 12, -1),
        (0, 0, 24, 24, 0, 20, 30, 1, 256, 0, 12, -1),
        (0, 0, 24, 24, 0, 20, 5, 0, 256, 0, 4, -1),
        (0, 0, 24, 24, 0, 20, 5, 1, 256, 0, 4, -1),
        (0, 0, 24, 24, 0, 1025, 0, 0, 256, 0, 1041, -1),
        (0, 1, 24, 24, 0, 1025, 0, 0, 256, 0, 1041, -1),
        (0, 0, 24, 24, 0, 1025, 0, 1, 256, 0, 1041, -1),
        (0, 0, 24, 24, 0, 1025, 18, 0, 256, 0, 1033, -1),
        (0, 1, 24, 24, 0, 1025, 18, 0, 256, 0, 1033, -1),
        (0, 0, 24, 24, 0, 1025, 18, 1, 256, 0, 1033, -1),
        (0, 0, 24, 24, 0, 1025, 30, 0, 256, 0, 1033, -1),
        (0, 0, 24, 24, 0, 1025, 30, 1, 256, 0, 1033, -1),
        (0, 0, 24, 24, 0, 1025, 5, 0, 256, 0, 1041, -1),
        (0, 0, 24, 24, 0, 1025, 5, 1, 256, 0, 1041, -1),
        (0, 0, 24, 24, 0, 2032, 0, 0, 256, 0, 0, -1),
        (0, 1, 24, 24, 0, 2032, 0, 0, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 2032, 0, 1, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 2032, 18, 0, 256, 0, 2040, -1),
        (0, 1, 24, 24, 0, 2032, 18, 0, 256, 0, 2040, -1),
        (0, 0, 24, 24, 0, 2032, 18, 1, 256, 0, 2040, -1),
        (0, 0, 24, 24, 0, 2032, 30, 0, 256, 0, 2040, -1),
        (0, 0, 24, 24, 0, 2032, 30, 1, 256, 0, 2040, -1),
        (0, 0, 24, 24, 0, 2032, 5, 0, 256, 0, 0, -1),
        (0, 0, 24, 24, 0, 2032, 5, 1, 256, 0, 0, -1),
        (0, 0, 24, 24, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (0, 1, 24, 24, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 0, 1, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (0, 1, 24, 24, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 18, 1, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 30, 1, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 0, 5, 1, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 20, 0, 0, 256, 2047, 4, -1),
        (0, 1, 24, 24, 2047, 20, 0, 0, 256, 2047, 4, -1),
        (0, 0, 24, 24, 2047, 20, 0, 1, 256, 2047, 4, -1),
        (0, 0, 24, 24, 2047, 20, 18, 0, 256, 2047, 12, -1),
        (0, 1, 24, 24, 2047, 20, 18, 0, 256, 2047, 12, -1),
        (0, 0, 24, 24, 2047, 20, 18, 1, 256, 2047, 12, -1),
        (0, 0, 24, 24, 2047, 20, 30, 0, 256, 2047, 12, -1),
        (0, 0, 24, 24, 2047, 20, 30, 1, 256, 2047, 12, -1),
        (0, 0, 24, 24, 2047, 20, 5, 0, 256, 2047, 4, -1),
        (0, 0, 24, 24, 2047, 20, 5, 1, 256, 2047, 4, -1),
        (0, 0, 24, 24, 2047, 1025, 0, 0, 256, 2047, 1041, -1),
        (0, 1, 24, 24, 2047, 1025, 0, 0, 256, 2047, 1041, -1),
        (0, 0, 24, 24, 2047, 1025, 0, 1, 256, 2047, 1041, -1),
        (0, 0, 24, 24, 2047, 1025, 18, 0, 256, 2047, 1033, -1),
        (0, 1, 24, 24, 2047, 1025, 18, 0, 256, 2047, 1033, -1),
        (0, 0, 24, 24, 2047, 1025, 18, 1, 256, 2047, 1033, -1),
        (0, 0, 24, 24, 2047, 1025, 30, 0, 256, 2047, 1033, -1),
        (0, 0, 24, 24, 2047, 1025, 30, 1, 256, 2047, 1033, -1),
        (0, 0, 24, 24, 2047, 1025, 5, 0, 256, 2047, 1041, -1),
        (0, 0, 24, 24, 2047, 1025, 5, 1, 256, 2047, 1041, -1),
        (0, 0, 24, 24, 2047, 2032, 0, 0, 256, 2047, 0, -1),
        (0, 1, 24, 24, 2047, 2032, 0, 0, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 2032, 0, 1, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 2032, 18, 0, 256, 2047, 2040, -1),
        (0, 1, 24, 24, 2047, 2032, 18, 0, 256, 2047, 2040, -1),
        (0, 0, 24, 24, 2047, 2032, 18, 1, 256, 2047, 2040, -1),
        (0, 0, 24, 24, 2047, 2032, 30, 0, 256, 2047, 2040, -1),
        (0, 0, 24, 24, 2047, 2032, 30, 1, 256, 2047, 2040, -1),
        (0, 0, 24, 24, 2047, 2032, 5, 0, 256, 2047, 0, -1),
        (0, 0, 24, 24, 2047, 2032, 5, 1, 256, 2047, 0, -1),
        (0, 0, 48, 12, 0, 0, 0, 0, 256, 36, 0, 14),
        (0, 1, 48, 12, 0, 0, 0, 0, 256, 36, 0, 14),
        (0, 0, 48, 12, 0, 0, 0, 0, 180, 25, 0, 14),
        (0, 1, 48, 12, 0, 0, 0, 0, 180, 25, 0, 14),
        (0, 0, 48, 12, 0, 0, 0, 1, 256, 36, 0, 16),
        (0, 0, 48, 12, 0, 0, 0, 1, 180, 25, 0, 16),
        (0, 0, 48, 12, 0, 0, 18, 0, 256, 36, 4, 14),
        (0, 1, 48, 12, 0, 0, 18, 0, 256, 36, 4, 14),
        (0, 0, 48, 12, 0, 0, 18, 0, 180, 25, 4, 14),
        (0, 1, 48, 12, 0, 0, 18, 0, 180, 25, 4, 14),
        (0, 0, 48, 12, 0, 0, 18, 1, 256, 36, 4, 16),
        (0, 0, 48, 12, 0, 0, 18, 1, 180, 25, 4, 16),
        (0, 0, 48, 12, 0, 0, 30, 0, 256, 36, 4, 14),
        (0, 0, 48, 12, 0, 0, 30, 0, 180, 25, 4, 14),
        (0, 0, 48, 12, 0, 0, 30, 1, 256, 36, 4, 16),
        (0, 0, 48, 12, 0, 0, 30, 1, 180, 25, 4, 16),
        (0, 0, 48, 12, 0, 0, 5, 0, 256, 36, 0, 14),
        (0, 0, 48, 12, 0, 0, 5, 0, 180, 25, 0, 14),
        (0, 0, 48, 12, 0, 0, 5, 1, 256, 36, 0, 16),
        (0, 0, 48, 12, 0, 0, 5, 1, 180, 25, 0, 16),
        (0, 0, 48, 12, 0, 20, 0, 0, 256, 36, 12, 14),
        (0, 1, 48, 12, 0, 20, 0, 0, 256, 36, 12, 14),
        (0, 0, 48, 12, 0, 20, 0, 0, 180, 25, 12, 14),
        (0, 1, 48, 12, 0, 20, 0, 0, 180, 25, 12, 14),
        (0, 0, 48, 12, 0, 20, 0, 1, 256, 36, 12, 16),
        (0, 0, 48, 12, 0, 20, 0, 1, 180, 25, 12, 16),
        (0, 0, 48, 12, 0, 20, 18, 0, 256, 36, 24, 14),
        (0, 1, 48, 12, 0, 20, 18, 0, 256, 36, 24, 14),
        (0, 0, 48, 12, 0, 20, 18, 0, 180, 25, 24, 14),
        (0, 1, 48, 12, 0, 20, 18, 0, 180, 25, 24, 14),
        (0, 0, 48, 12, 0, 20, 18, 1, 256, 36, 24, 16),
        (0, 0, 48, 12, 0, 20, 18, 1, 180, 25, 24, 16),
        (0, 0, 48, 12, 0, 20, 30, 0, 256, 36, 24, 14),
        (0, 0, 48, 12, 0, 20, 30, 0, 180, 25, 24, 14),
        (0, 0, 48, 12, 0, 20, 30, 1, 256, 36, 24, 16),
        (0, 0, 48, 12, 0, 20, 30, 1, 180, 25, 24, 16),
        (0, 0, 48, 12, 0, 20, 5, 0, 256, 36, 12, 14),
        (0, 0, 48, 12, 0, 20, 5, 0, 180, 25, 12, 14),
        (0, 0, 48, 12, 0, 20, 5, 1, 256, 36, 12, 16),
        (0, 0, 48, 12, 0, 20, 5, 1, 180, 25, 12, 16),
        (0, 0, 48, 12, 0, 1025, 0, 0, 256, 36, 1033, 14),
        (0, 1, 48, 12, 0, 1025, 0, 0, 256, 36, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 0, 0, 180, 25, 1033, 14),
        (0, 1, 48, 12, 0, 1025, 0, 0, 180, 25, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 0, 1, 256, 36, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 0, 1, 180, 25, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 18, 0, 256, 36, 1033, 14),
        (0, 1, 48, 12, 0, 1025, 18, 0, 256, 36, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 18, 0, 180, 25, 1033, 14),
        (0, 1, 48, 12, 0, 1025, 18, 0, 180, 25, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 18, 1, 256, 36, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 18, 1, 180, 25, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 30, 0, 256, 36, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 30, 0, 180, 25, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 30, 1, 256, 36, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 30, 1, 180, 25, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 5, 0, 256, 36, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 5, 0, 180, 25, 1033, 14),
        (0, 0, 48, 12, 0, 1025, 5, 1, 256, 36, 1033, 16),
        (0, 0, 48, 12, 0, 1025, 5, 1, 180, 25, 1033, 16),
        (0, 0, 48, 12, 0, 2032, 0, 0, 256, 36, 2040, 14),
        (0, 1, 48, 12, 0, 2032, 0, 0, 256, 36, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 0, 0, 180, 25, 2040, 14),
        (0, 1, 48, 12, 0, 2032, 0, 0, 180, 25, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 0, 1, 256, 36, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 0, 1, 180, 25, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 18, 0, 256, 36, 2040, 14),
        (0, 1, 48, 12, 0, 2032, 18, 0, 256, 36, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 18, 0, 180, 25, 2040, 14),
        (0, 1, 48, 12, 0, 2032, 18, 0, 180, 25, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 18, 1, 256, 36, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 18, 1, 180, 25, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 30, 0, 256, 36, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 30, 0, 180, 25, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 30, 1, 256, 36, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 30, 1, 180, 25, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 5, 0, 256, 36, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 5, 0, 180, 25, 2040, 14),
        (0, 0, 48, 12, 0, 2032, 5, 1, 256, 36, 2040, 16),
        (0, 0, 48, 12, 0, 2032, 5, 1, 180, 25, 2040, 16),
        (0, 0, 48, 12, 2047, 0, 0, 0, 256, 35, 0, 14),
        (0, 1, 48, 12, 2047, 0, 0, 0, 256, 35, 0, 14),
        (0, 0, 48, 12, 2047, 0, 0, 0, 180, 24, 0, 14),
        (0, 1, 48, 12, 2047, 0, 0, 0, 180, 24, 0, 14),
        (0, 0, 48, 12, 2047, 0, 0, 1, 256, 35, 0, 16),
        (0, 0, 48, 12, 2047, 0, 0, 1, 180, 24, 0, 16),
        (0, 0, 48, 12, 2047, 0, 18, 0, 256, 35, 4, 14),
        (0, 1, 48, 12, 2047, 0, 18, 0, 256, 35, 4, 14),
        (0, 0, 48, 12, 2047, 0, 18, 0, 180, 24, 4, 14),
        (0, 1, 48, 12, 2047, 0, 18, 0, 180, 24, 4, 14),
        (0, 0, 48, 12, 2047, 0, 18, 1, 256, 35, 4, 16),
        (0, 0, 48, 12, 2047, 0, 18, 1, 180, 24, 4, 16),
        (0, 0, 48, 12, 2047, 0, 30, 0, 256, 35, 4, 14),
        (0, 0, 48, 12, 2047, 0, 30, 0, 180, 24, 4, 14),
        (0, 0, 48, 12, 2047, 0, 30, 1, 256, 35, 4, 16),
        (0, 0, 48, 12, 2047, 0, 30, 1, 180, 24, 4, 16),
        (0, 0, 48, 12, 2047, 0, 5, 0, 256, 35, 0, 14),
        (0, 0, 48, 12, 2047, 0, 5, 0, 180, 24, 0, 14),
        (0, 0, 48, 12, 2047, 0, 5, 1, 256, 35, 0, 16),
        (0, 0, 48, 12, 2047, 0, 5, 1, 180, 24, 0, 16),
        (0, 0, 48, 12, 2047, 20, 0, 0, 256, 35, 12, 14),
        (0, 1, 48, 12, 2047, 20, 0, 0, 256, 35, 12, 14),
        (0, 0, 48, 12, 2047, 20, 0, 0, 180, 24, 12, 14),
        (0, 1, 48, 12, 2047, 20, 0, 0, 180, 24, 12, 14),
        (0, 0, 48, 12, 2047, 20, 0, 1, 256, 35, 12, 16),
        (0, 0, 48, 12, 2047, 20, 0, 1, 180, 24, 12, 16),
        (0, 0, 48, 12, 2047, 20, 18, 0, 256, 35, 24, 14),
        (0, 1, 48, 12, 2047, 20, 18, 0, 256, 35, 24, 14),
        (0, 0, 48, 12, 2047, 20, 18, 0, 180, 24, 24, 14),
        (0, 1, 48, 12, 2047, 20, 18, 0, 180, 24, 24, 14),
        (0, 0, 48, 12, 2047, 20, 18, 1, 256, 35, 24, 16),
        (0, 0, 48, 12, 2047, 20, 18, 1, 180, 24, 24, 16),
        (0, 0, 48, 12, 2047, 20, 30, 0, 256, 35, 24, 14),
        (0, 0, 48, 12, 2047, 20, 30, 0, 180, 24, 24, 14),
        (0, 0, 48, 12, 2047, 20, 30, 1, 256, 35, 24, 16),
        (0, 0, 48, 12, 2047, 20, 30, 1, 180, 24, 24, 16),
        (0, 0, 48, 12, 2047, 20, 5, 0, 256, 35, 12, 14),
        (0, 0, 48, 12, 2047, 20, 5, 0, 180, 24, 12, 14),
        (0, 0, 48, 12, 2047, 20, 5, 1, 256, 35, 12, 16),
        (0, 0, 48, 12, 2047, 20, 5, 1, 180, 24, 12, 16),
        (0, 0, 48, 12, 2047, 1025, 0, 0, 256, 35, 1033, 14),
        (0, 1, 48, 12, 2047, 1025, 0, 0, 256, 35, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 0, 0, 180, 24, 1033, 14),
        (0, 1, 48, 12, 2047, 1025, 0, 0, 180, 24, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 0, 1, 256, 35, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 0, 1, 180, 24, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 18, 0, 256, 35, 1033, 14),
        (0, 1, 48, 12, 2047, 1025, 18, 0, 256, 35, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 18, 0, 180, 24, 1033, 14),
        (0, 1, 48, 12, 2047, 1025, 18, 0, 180, 24, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 18, 1, 256, 35, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 18, 1, 180, 24, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 30, 0, 256, 35, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 30, 0, 180, 24, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 30, 1, 256, 35, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 30, 1, 180, 24, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 5, 0, 256, 35, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 5, 0, 180, 24, 1033, 14),
        (0, 0, 48, 12, 2047, 1025, 5, 1, 256, 35, 1033, 16),
        (0, 0, 48, 12, 2047, 1025, 5, 1, 180, 24, 1033, 16),
        (0, 0, 48, 12, 2047, 2032, 0, 0, 256, 35, 2040, 14),
        (0, 1, 48, 12, 2047, 2032, 0, 0, 256, 35, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 0, 0, 180, 24, 2040, 14),
        (0, 1, 48, 12, 2047, 2032, 0, 0, 180, 24, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 0, 1, 256, 35, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 0, 1, 180, 24, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 18, 0, 256, 35, 2040, 14),
        (0, 1, 48, 12, 2047, 2032, 18, 0, 256, 35, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 18, 0, 180, 24, 2040, 14),
        (0, 1, 48, 12, 2047, 2032, 18, 0, 180, 24, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 18, 1, 256, 35, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 18, 1, 180, 24, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 30, 0, 256, 35, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 30, 0, 180, 24, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 30, 1, 256, 35, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 30, 1, 180, 24, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 5, 0, 256, 35, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 5, 0, 180, 24, 2040, 14),
        (0, 0, 48, 12, 2047, 2032, 5, 1, 256, 35, 2040, 16),
        (0, 0, 48, 12, 2047, 2032, 5, 1, 180, 24, 2040, 16),
        (1, 0, 0, 0, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 0, 0, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 0, 0, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 0, 0, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 0, 0, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 0, 0, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 0, 0, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 0, 0, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 0, 0, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 0, 0, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 0, 0, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 0, 0, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 0, 0, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 0, 0, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 0, 0, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 0, 0, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 0, 0, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 0, 0, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 0, 0, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 0, 0, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 0, 0, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 0, 0, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 0, 0, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 0, 0, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 0, 0, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 0, 0, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 0, 0, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 0, 0, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 0, 0, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 0, 0, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 0, 0, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 0, 0, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 12, 0, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 0, 0, 180, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 18, 0, 180, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 30, 0, 180, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 12, 0, 0, 0, 5, 0, 180, 0, 0, -1),
        (1, 0, 12, 0, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 12, 0, 0, 20, 0, 0, 180, 0, 12, -1),
        (1, 0, 12, 0, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 12, 0, 0, 20, 18, 0, 180, 0, 20, -1),
        (1, 0, 12, 0, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 12, 0, 0, 20, 30, 0, 180, 0, 20, -1),
        (1, 0, 12, 0, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 12, 0, 0, 20, 5, 0, 180, 0, 12, -1),
        (1, 0, 12, 0, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 12, 0, 0, 1025, 0, 0, 180, 0, 1033, -1),
        (1, 0, 12, 0, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 12, 0, 0, 1025, 18, 0, 180, 0, 1025, -1),
        (1, 0, 12, 0, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 12, 0, 0, 1025, 30, 0, 180, 0, 1025, -1),
        (1, 0, 12, 0, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 12, 0, 0, 1025, 5, 0, 180, 0, 1033, -1),
        (1, 0, 12, 0, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 12, 0, 0, 2032, 0, 0, 180, 0, 2040, -1),
        (1, 0, 12, 0, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 12, 0, 0, 2032, 18, 0, 180, 0, 2032, -1),
        (1, 0, 12, 0, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 12, 0, 0, 2032, 30, 0, 180, 0, 2032, -1),
        (1, 0, 12, 0, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 12, 0, 0, 2032, 5, 0, 180, 0, 2040, -1),
        (1, 0, 12, 0, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 0, 0, 180, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 18, 0, 180, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 30, 0, 180, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 0, 5, 0, 180, 2047, 0, -1),
        (1, 0, 12, 0, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 12, 0, 2047, 20, 0, 0, 180, 2047, 12, -1),
        (1, 0, 12, 0, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 12, 0, 2047, 20, 18, 0, 180, 2047, 20, -1),
        (1, 0, 12, 0, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 12, 0, 2047, 20, 30, 0, 180, 2047, 20, -1),
        (1, 0, 12, 0, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 12, 0, 2047, 20, 5, 0, 180, 2047, 12, -1),
        (1, 0, 12, 0, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 12, 0, 2047, 1025, 0, 0, 180, 2047, 1033, -1),
        (1, 0, 12, 0, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 12, 0, 2047, 1025, 18, 0, 180, 2047, 1025, -1),
        (1, 0, 12, 0, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 12, 0, 2047, 1025, 30, 0, 180, 2047, 1025, -1),
        (1, 0, 12, 0, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 12, 0, 2047, 1025, 5, 0, 180, 2047, 1033, -1),
        (1, 0, 12, 0, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 12, 0, 2047, 2032, 0, 0, 180, 2047, 2040, -1),
        (1, 0, 12, 0, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 12, 0, 2047, 2032, 18, 0, 180, 2047, 2032, -1),
        (1, 0, 12, 0, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 12, 0, 2047, 2032, 30, 0, 180, 2047, 2032, -1),
        (1, 0, 12, 0, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 12, 0, 2047, 2032, 5, 0, 180, 2047, 2040, -1),
        (1, 0, 0, 12, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 0, 12, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 0, 12, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 0, 12, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 0, 12, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 0, 12, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 0, 12, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 0, 12, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 0, 12, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 0, 12, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 0, 12, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 0, 12, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 0, 12, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 0, 12, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 0, 12, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 0, 12, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 0, 12, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 0, 12, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 0, 12, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 0, 12, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 0, 12, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 0, 12, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 0, 12, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 0, 12, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 0, 12, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 0, 12, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 0, 12, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 0, 12, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 0, 12, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 0, 12, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 0, 12, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 0, 12, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 36, 0, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 36, 0, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 36, 0, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 36, 0, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 36, 0, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 36, 0, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 36, 0, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 36, 0, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 36, 0, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 36, 0, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 36, 0, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 36, 0, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 36, 0, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 36, 0, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 36, 0, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 36, 0, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 36, 0, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 36, 0, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 36, 0, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 36, 0, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 36, 0, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 36, 0, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 36, 0, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 36, 0, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 36, 0, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 36, 0, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 36, 0, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 36, 0, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 36, 0, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 36, 0, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 36, 0, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 36, 0, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 0, 48, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 0, 0, 180, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 18, 0, 180, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 30, 0, 180, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 0, 48, 0, 0, 5, 0, 180, 0, 0, -1),
        (1, 0, 0, 48, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 0, 48, 0, 20, 0, 0, 180, 0, 12, -1),
        (1, 0, 0, 48, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 0, 48, 0, 20, 18, 0, 180, 0, 20, -1),
        (1, 0, 0, 48, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 0, 48, 0, 20, 30, 0, 180, 0, 20, -1),
        (1, 0, 0, 48, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 0, 48, 0, 20, 5, 0, 180, 0, 12, -1),
        (1, 0, 0, 48, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 0, 48, 0, 1025, 0, 0, 180, 0, 1033, -1),
        (1, 0, 0, 48, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 0, 48, 0, 1025, 18, 0, 180, 0, 1025, -1),
        (1, 0, 0, 48, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 0, 48, 0, 1025, 30, 0, 180, 0, 1025, -1),
        (1, 0, 0, 48, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 0, 48, 0, 1025, 5, 0, 180, 0, 1033, -1),
        (1, 0, 0, 48, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 0, 48, 0, 2032, 0, 0, 180, 0, 2040, -1),
        (1, 0, 0, 48, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 0, 48, 0, 2032, 18, 0, 180, 0, 2032, -1),
        (1, 0, 0, 48, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 0, 48, 0, 2032, 30, 0, 180, 0, 2032, -1),
        (1, 0, 0, 48, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 0, 48, 0, 2032, 5, 0, 180, 0, 2040, -1),
        (1, 0, 0, 48, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 0, 0, 180, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 18, 0, 180, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 30, 0, 180, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 0, 5, 0, 180, 2047, 0, -1),
        (1, 0, 0, 48, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 0, 48, 2047, 20, 0, 0, 180, 2047, 12, -1),
        (1, 0, 0, 48, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 0, 48, 2047, 20, 18, 0, 180, 2047, 20, -1),
        (1, 0, 0, 48, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 0, 48, 2047, 20, 30, 0, 180, 2047, 20, -1),
        (1, 0, 0, 48, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 0, 48, 2047, 20, 5, 0, 180, 2047, 12, -1),
        (1, 0, 0, 48, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 0, 48, 2047, 1025, 0, 0, 180, 2047, 1033, -1),
        (1, 0, 0, 48, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 0, 48, 2047, 1025, 18, 0, 180, 2047, 1025, -1),
        (1, 0, 0, 48, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 0, 48, 2047, 1025, 30, 0, 180, 2047, 1025, -1),
        (1, 0, 0, 48, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 0, 48, 2047, 1025, 5, 0, 180, 2047, 1033, -1),
        (1, 0, 0, 48, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 0, 48, 2047, 2032, 0, 0, 180, 2047, 2040, -1),
        (1, 0, 0, 48, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 0, 48, 2047, 2032, 18, 0, 180, 2047, 2032, -1),
        (1, 0, 0, 48, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 0, 48, 2047, 2032, 30, 0, 180, 2047, 2032, -1),
        (1, 0, 0, 48, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 0, 48, 2047, 2032, 5, 0, 180, 2047, 2040, -1),
        (1, 0, 24, 24, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 24, 24, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 24, 24, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 24, 24, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 24, 24, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 24, 24, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 24, 24, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 24, 24, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 24, 24, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 24, 24, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 24, 24, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 24, 24, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 24, 24, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 24, 24, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 24, 24, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 24, 24, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 24, 24, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 24, 24, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 24, 24, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 24, 24, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 24, 24, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 24, 24, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 24, 24, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 24, 24, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 24, 24, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 24, 24, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 24, 24, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 24, 24, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 24, 24, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 24, 24, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 24, 24, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 24, 24, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 48, 12, 0, 0, 0, 0, 256, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 0, 0, 180, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 18, 0, 256, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 18, 0, 180, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 30, 0, 256, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 30, 0, 180, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 5, 0, 256, 0, 0, -1),
        (1, 0, 48, 12, 0, 0, 5, 0, 180, 0, 0, -1),
        (1, 0, 48, 12, 0, 20, 0, 0, 256, 0, 12, -1),
        (1, 0, 48, 12, 0, 20, 0, 0, 180, 0, 12, -1),
        (1, 0, 48, 12, 0, 20, 18, 0, 256, 0, 20, -1),
        (1, 0, 48, 12, 0, 20, 18, 0, 180, 0, 20, -1),
        (1, 0, 48, 12, 0, 20, 30, 0, 256, 0, 20, -1),
        (1, 0, 48, 12, 0, 20, 30, 0, 180, 0, 20, -1),
        (1, 0, 48, 12, 0, 20, 5, 0, 256, 0, 12, -1),
        (1, 0, 48, 12, 0, 20, 5, 0, 180, 0, 12, -1),
        (1, 0, 48, 12, 0, 1025, 0, 0, 256, 0, 1033, -1),
        (1, 0, 48, 12, 0, 1025, 0, 0, 180, 0, 1033, -1),
        (1, 0, 48, 12, 0, 1025, 18, 0, 256, 0, 1025, -1),
        (1, 0, 48, 12, 0, 1025, 18, 0, 180, 0, 1025, -1),
        (1, 0, 48, 12, 0, 1025, 30, 0, 256, 0, 1025, -1),
        (1, 0, 48, 12, 0, 1025, 30, 0, 180, 0, 1025, -1),
        (1, 0, 48, 12, 0, 1025, 5, 0, 256, 0, 1033, -1),
        (1, 0, 48, 12, 0, 1025, 5, 0, 180, 0, 1033, -1),
        (1, 0, 48, 12, 0, 2032, 0, 0, 256, 0, 2040, -1),
        (1, 0, 48, 12, 0, 2032, 0, 0, 180, 0, 2040, -1),
        (1, 0, 48, 12, 0, 2032, 18, 0, 256, 0, 2032, -1),
        (1, 0, 48, 12, 0, 2032, 18, 0, 180, 0, 2032, -1),
        (1, 0, 48, 12, 0, 2032, 30, 0, 256, 0, 2032, -1),
        (1, 0, 48, 12, 0, 2032, 30, 0, 180, 0, 2032, -1),
        (1, 0, 48, 12, 0, 2032, 5, 0, 256, 0, 2040, -1),
        (1, 0, 48, 12, 0, 2032, 5, 0, 180, 0, 2040, -1),
        (1, 0, 48, 12, 2047, 0, 0, 0, 256, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 0, 0, 180, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 18, 0, 256, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 18, 0, 180, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 30, 0, 256, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 30, 0, 180, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 5, 0, 256, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 0, 5, 0, 180, 2047, 0, -1),
        (1, 0, 48, 12, 2047, 20, 0, 0, 256, 2047, 12, -1),
        (1, 0, 48, 12, 2047, 20, 0, 0, 180, 2047, 12, -1),
        (1, 0, 48, 12, 2047, 20, 18, 0, 256, 2047, 20, -1),
        (1, 0, 48, 12, 2047, 20, 18, 0, 180, 2047, 20, -1),
        (1, 0, 48, 12, 2047, 20, 30, 0, 256, 2047, 20, -1),
        (1, 0, 48, 12, 2047, 20, 30, 0, 180, 2047, 20, -1),
        (1, 0, 48, 12, 2047, 20, 5, 0, 256, 2047, 12, -1),
        (1, 0, 48, 12, 2047, 20, 5, 0, 180, 2047, 12, -1),
        (1, 0, 48, 12, 2047, 1025, 0, 0, 256, 2047, 1033, -1),
        (1, 0, 48, 12, 2047, 1025, 0, 0, 180, 2047, 1033, -1),
        (1, 0, 48, 12, 2047, 1025, 18, 0, 256, 2047, 1025, -1),
        (1, 0, 48, 12, 2047, 1025, 18, 0, 180, 2047, 1025, -1),
        (1, 0, 48, 12, 2047, 1025, 30, 0, 256, 2047, 1025, -1),
        (1, 0, 48, 12, 2047, 1025, 30, 0, 180, 2047, 1025, -1),
        (1, 0, 48, 12, 2047, 1025, 5, 0, 256, 2047, 1033, -1),
        (1, 0, 48, 12, 2047, 1025, 5, 0, 180, 2047, 1033, -1),
        (1, 0, 48, 12, 2047, 2032, 0, 0, 256, 2047, 2040, -1),
        (1, 0, 48, 12, 2047, 2032, 0, 0, 180, 2047, 2040, -1),
        (1, 0, 48, 12, 2047, 2032, 18, 0, 256, 2047, 2032, -1),
        (1, 0, 48, 12, 2047, 2032, 18, 0, 180, 2047, 2032, -1),
        (1, 0, 48, 12, 2047, 2032, 30, 0, 256, 2047, 2032, -1),
        (1, 0, 48, 12, 2047, 2032, 30, 0, 180, 2047, 2032, -1),
        (1, 0, 48, 12, 2047, 2032, 5, 0, 256, 2047, 2040, -1),
        (1, 0, 48, 12, 2047, 2032, 5, 0, 180, 2047, 2040, -1),
    ];
    for &(
        walk,
        forward,
        left,
        right,
        heading,
        roll,
        group,
        torch,
        scale,
        heading_after,
        roll_after,
        turn_group,
    ) in rows
    {
        let delta = ((right * scale) >> 8) - ((left * scale) >> 8);
        let result = ground_turn(
            heading,
            roll,
            GroundTurnInput {
                delta,
                running: matches!(group, 18 | 30),
                torch: torch != 0,
                walk_key: walk != 0,
                forward_or_backward: forward != 0,
                eligible: true,
                animation_barred: false,
            },
        );
        assert_eq!(
            (
                result.heading,
                result.roll,
                result.group.map_or(-1, i32::from)
            ),
            (heading_after, roll_after, turn_group),
            "walk {walk} forward {forward} left {left} right {right} heading {heading} roll {roll} group {group} torch {torch} scale {scale}"
        );
    }
}

#[test]
fn a_barred_turn_animation_still_turns_the_heading() {
    // Drawn weapon with a combat record, or surface 2 or 4: preservation's player.md ground-turn rules.
    let input = |animation_barred| GroundTurnInput {
        delta: -48,
        running: false,
        torch: false,
        walk_key: false,
        forward_or_backward: false,
        eligible: true,
        animation_barred,
    };
    let free = ground_turn(100, 0, input(false));
    let barred = ground_turn(100, 0, input(true));
    assert_eq!(free.group, Some(14));
    assert_eq!(barred.group, None);
    assert_eq!((barred.heading, barred.roll), (free.heading, free.roll));
    assert_eq!(barred.heading, 148);
}

#[test]
fn the_fight_turn_attributes_apply_to_a_weapon_holder_outside_the_running_groups_or_a_hand_item_51()
{
    use cyrus_game::player::turn::fight_values_selected as fight;
    assert!(!fight(false, Some(8), None), "sheathed, empty hand");
    assert!(fight(true, Some(8), None), "weapon held, standing");
    assert!(
        fight(true, None, Some(0)),
        "weapon held, no animation group"
    );
    assert!(!fight(true, Some(18), None), "weapon held, group 18");
    assert!(!fight(true, Some(31), Some(3)), "weapon held, group 31");
    assert!(fight(false, Some(8), Some(51)), "hand item 51");
    assert!(
        fight(true, Some(18), Some(51)),
        "hand item 51 overrides the running groups"
    );
}
