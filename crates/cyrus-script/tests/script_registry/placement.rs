use super::*;

#[test]
fn place_selected_player_moves_the_logical_player_and_clears_ledge_and_fall_state() {
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let player = world.selected_player().unwrap();
    world
        .place_selected_player([256 * 1000, -256 * 50, 256 * 2000], 513)
        .unwrap();
    let spatial = world.spatial(player).unwrap();
    assert_eq!(spatial.local.position, [256 * 1000, -256 * 50, 256 * 2000]);
    assert_eq!(spatial.local.angles[1], 513);
    assert_eq!(spatial.previous_local_position, spatial.local.position);
    assert!(
        !world.actors()[player.index]
            .control_state()
            .unwrap()
            .stable_position_valid,
        "a retained stable position would restore the old spot after the placement"
    );
}

#[test]
fn getmapmarker_moves_the_selected_player_to_a_present_map_marker_and_keeps_its_heading() {
    let (scripts, _, _, _) = player_walking_fixture_with(|_| {});
    let (mut world, mut data, _) = static_model_fixture_session(scripts, None, [10, 20, 30], false);
    data.metadata["markers"] = serde_json::json!([
        {"map_position": [3, -2, 5], "reserved": 1},
        {"map_position": [9, 9, 9], "reserved": 0},
    ]);
    world.prepare_markers(&data).unwrap();
    let player = world.selected_player().unwrap();
    world
        .initialize_player_spatial(player, [2560, -1536, 7680], [0; 3])
        .unwrap();
    world
        .place_selected_player([256 * 100, 256 * 7, 256 * 200], 513)
        .unwrap();
    world.move_selected_player_to_map_marker(0).unwrap();
    let spatial = world.spatial(player).unwrap();
    assert_eq!(spatial.local.position, [3 * 256, -2 * 256, 5 * 256]);
    assert_eq!(spatial.local.angles[1], 513, "the heading is kept");
    assert_eq!(spatial.previous_local_position, spatial.local.position);
    // A marker without its presence byte, or beyond the table, moves nothing.
    for (marker, text) in [
        (1, "unavailable"),
        (2, "marker table"),
        (-1, "marker table"),
    ] {
        let error = world
            .move_selected_player_to_map_marker(marker)
            .unwrap_err()
            .to_string();
        assert!(error.contains(text), "{error}");
        assert_eq!(
            world.spatial(player).unwrap().local.position,
            [3 * 256, -2 * 256, 5 * 256]
        );
    }
}

#[test]
fn getmarker_restores_a_saved_position_and_orientation_with_angles_wrapped() {
    use cyrus_script::script::SavedPlacement;
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let player = world.selected_player().unwrap();
    world.place_selected_player([0, 0, 0], 5).unwrap();
    let mut saved = SavedPlacement {
        position: [256 * 40, -256 * 3, 256 * 50],
        orientation: [10, 700, 2047],
        camera_position: [1, 2, 3],
        camera_angles: [4, 5, 6],
    };
    world.restore_selected_player_placement(&saved).unwrap();
    let spatial = world.spatial(player).unwrap();
    assert_eq!(spatial.local.position, saved.position);
    assert_eq!(spatial.local.angles, [10, 700, 2047]);
    assert_eq!(spatial.previous_local_position, saved.position);
    assert_eq!(spatial.angle_history, [10, 700, 2047]);
    assert_eq!(world.camera().unwrap().current.position, [1, 2, 3]);
    assert_eq!(world.camera().unwrap().current.angles, [4, 5, 6]);
    // Each angle is wrapped to 0 through 2047, as the original's orientation setter masks them.
    saved.orientation = [2048 + 3, -1, 4095];
    world.restore_selected_player_placement(&saved).unwrap();
    assert_eq!(world.spatial(player).unwrap().local.angles, [3, 2047, 2047]);
    assert_eq!(
        world.spatial(player).unwrap().angle_history,
        [3, 2047, 2047]
    );
}

#[test]
fn setmarker_placement_holds_the_position_orientation_and_published_camera_pose() {
    use cyrus_script::script::camera::CameraPose;
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    world
        .initialize_camera_pose(CameraPose {
            position: [256 * 7, 256 * 8, 256 * 9],
            angles: [11, 22, 33],
        })
        .unwrap();
    world
        .place_selected_player([256 * 40, -256 * 3, 256 * 50], 700)
        .unwrap();
    let saved = world.selected_player_placement().unwrap();
    assert_eq!(saved.position, [256 * 40, -256 * 3, 256 * 50]);
    assert_eq!(saved.orientation[1], 700);
    assert_eq!(saved.camera_position, [256 * 7, 256 * 8, 256 * 9]);
    assert_eq!(saved.camera_angles, [11, 22, 33]);
    // Restoring a placement taken elsewhere writes the saved player and published camera pose.
    world.place_selected_player([0, 0, 0], 5).unwrap();
    world.restore_selected_player_placement(&saved).unwrap();
    let player = world.selected_player().unwrap();
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        saved.position
    );
    assert_eq!(
        world.selected_player_placement().unwrap().camera_position,
        saved.camera_position
    );
}

#[test]
fn set_selected_torch_writes_the_burning_counter_of_the_selected_player() {
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let player = world.selected_player().unwrap();
    world.set_selected_torch(200).unwrap();
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .torch_burning,
        200
    );
    world.set_selected_torch(0).unwrap();
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .torch_burning,
        0
    );
}

#[test]
fn set_actor_attribute_writes_one_attribute_byte_and_rejects_bad_targets() {
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    world.set_actor_attribute(0, 3, 0).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Health], 0);
    world.set_actor_attribute(0, 3, 77).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Health], 77);
    assert!(world.set_actor_attribute(usize::MAX, 3, 1).is_err());
    assert!(world.set_actor_attribute(0, 256, 1).is_err());
}

#[test]
fn mark_actor_dead_sets_the_state_is_dead_reads() {
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    world.mark_actor_dead(0).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Health], 0);
    assert!(world.mark_actor_dead(usize::MAX).is_err());
}

#[test]
fn console_marker_dispatch_restores_placement_and_preserves_failed_operations() {
    use bevy::prelude::App;
    use cyrus_script::console::{
        GameConsolePlugin,
        markers::{MarkerStorage, MarkerStore},
    };
    use cyrus_script::script::{ScriptActors, camera::CameraPose};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Stored {
        bytes: Option<Vec<u8>>,
        reject_save: bool,
    }
    struct Store(Arc<Mutex<Stored>>);
    impl MarkerStore for Store {
        fn load(&self) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().bytes.clone())
        }
        fn save(&self, bytes: &[u8]) -> anyhow::Result<()> {
            let mut state = self.0.lock().unwrap();
            anyhow::ensure!(!state.reject_save, "marker storage rejected save");
            state.bytes = Some(bytes.to_vec());
            Ok(())
        }
    }
    let (_, mut script, _, _) = player_walking_fixture_with(|_| {});
    script
        .initialize_camera_pose(CameraPose {
            position: [256, 512, 768],
            angles: [11, 22, 33],
        })
        .unwrap();
    script
        .place_selected_player([10240, -768, 12800], 700)
        .unwrap();
    let saved = script.selected_player_placement().unwrap();
    let mut actors = ScriptActors::default();
    actors.current = Some(script);
    let stored = Arc::new(Mutex::new(Stored::default()));
    let mut app = App::new();
    app.add_plugins(GameConsolePlugin)
        .insert_resource(actors)
        .insert_resource(MarkerStorage(Box::new(Store(stored.clone()))));
    let submit = |app: &mut App, line: &str| {
        cyrus_console::submit(app.world_mut(), line, cyrus_console::Source::Interactive)
    };
    assert_eq!(submit(&mut app, "setmarker 17").unwrap(), ["setmarker: 17"]);
    let accepted = stored.lock().unwrap().bytes.clone().unwrap();
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .place_selected_player([0, 0, 0], 5)
        .unwrap();
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .restore_selected_player_placement(&cyrus_script::script::SavedPlacement {
            position: [0, 0, 0],
            orientation: [0, 5, 0],
            camera_position: [-256, -512, -768],
            camera_angles: [44, 55, 66],
        })
        .unwrap();
    let displaced = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .selected_player_placement()
        .unwrap();
    assert_ne!(displaced.position, saved.position);
    assert_ne!(displaced.camera_position, saved.camera_position);
    stored.lock().unwrap().reject_save = true;
    assert!(
        submit(&mut app, "setmarker 17")
            .unwrap_err()
            .to_string()
            .contains("rejected save")
    );
    assert_eq!(stored.lock().unwrap().bytes.as_ref().unwrap(), &accepted);
    assert!(submit(&mut app, "getmarker 99").is_err());
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .selected_player_placement()
            .unwrap(),
        displaced
    );
    assert_eq!(submit(&mut app, "gm 17").unwrap(), ["getmarker: 17"]);
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .selected_player_placement()
            .unwrap(),
        saved
    );
    {
        let mut state = stored.lock().unwrap();
        state.reject_save = false;
        state.bytes = Some(b"truncated".to_vec());
    }
    assert!(submit(&mut app, "getmarker 17").is_err());
    assert!(submit(&mut app, "setmarker 17").is_err());
    assert_eq!(
        stored.lock().unwrap().bytes.as_deref(),
        Some(b"truncated".as_slice())
    );
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .selected_player_placement()
            .unwrap(),
        saved
    );
}
