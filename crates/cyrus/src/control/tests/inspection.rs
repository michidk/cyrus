//! Public inspection command contracts.
use super::*;

#[test]
fn console_reply_contains_dispatched_output_and_validation_errors() {
    let mut app = app();
    app.insert_state(cyrus_game::AppState::InGame);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    for (id, line, expected) in [(1, "commands", "console [con]"), (2, "é", "ASCII")] {
        let (reply, result) = channel();
        tx.send(Pending {
            request: Request {
                id,
                cmd: Command::Console {
                    open: Some(true),
                    line: Some(line.into()),
                },
            },
            reply: reply.clone(),
        })
        .unwrap();
        tx.send(Pending {
            request: Request {
                id: id + 100,
                cmd: Command::State,
            },
            reply,
        })
        .unwrap();
        app.update();
        let response = result.try_recv().unwrap();
        assert!(response.ok);
        assert!(serde_json::to_string(&response).unwrap().contains(expected));
        assert!(result.try_recv().is_err());
        app.update();
        let state = result.try_recv().unwrap().result.unwrap();
        assert_eq!(state["console"]["open"], true);
        assert!(
            state["console"]["lines"]
                .as_array()
                .unwrap()
                .iter()
                .any(|line| { line["text"].as_str().unwrap().contains(expected) })
        );
    }
}

#[test]
fn scene_requests_cannot_bypass_install_selection() {
    let mut app = app();
    app.insert_state(cyrus_game::AppState::SelectingInstall);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, result) = channel();
    tx.send(Pending {
        request: Request {
            id: 1,
            cmd: Command::LoadScene {
                name: "START".into(),
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(!result.try_recv().unwrap().ok);
    assert_eq!(
        *app.world().resource::<State<cyrus_game::AppState>>().get(),
        cyrus_game::AppState::SelectingInstall
    );
    assert!(app.world().resource::<SceneRequest>().0.is_empty());
}

#[test]
fn control_taps_survive_keyboard_update_and_release_next_frame() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, result) = channel();
    tx.send(Pending {
        request: Request {
            id: 1,
            cmd: Command::Input {
                key: "Enter".into(),
                action: InputAction::Tap,
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(result.try_recv().unwrap().ok);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Enter)
    );
    app.update();
    let keys = app.world().resource::<ButtonInput<KeyCode>>();
    assert!(!keys.pressed(KeyCode::Enter));
    assert!(keys.just_released(KeyCode::Enter));
}

#[test]
fn public_state_reads_the_live_boundary_instead_of_cached_pre_update_counts() {
    let mut app = script_app();
    app.insert_resource(cyrus_script::script::ScriptStatus::Loaded {
        world: 17,
        content_id: String::new(),
        programs: 1,
        instances: 1,
        prepared: 1,
        waiting: 0,
        failed: 0,
        unsupported: 0,
        actor_flats: 0,
        ordinary_updates: 900,
        restored: true,
        examples: Default::default(),
    });
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["scripts"]["ordinary_updates"], 0);
    assert_eq!(state["scripts"]["restored"], false);
}

#[test]
fn public_script_inspection_exposes_read_only_global_flags() {
    let mut app = script_app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, result) = channel();
    tx.send(Pending {
        request: Request {
            id: 1,
            cmd: Command::Scripts {
                offset: 0,
                limit: 1,
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    let response = result.try_recv().unwrap();
    assert!(response.ok);
    let value = response.result.unwrap();
    assert_eq!(value["world"], 17);
    assert_eq!(value["flags"], json!({"version": 1, "flags": []}));
    assert!(value["player_position_q8"].is_null());
}

#[test]
fn public_player_trace_records_placement_and_every_ordinary_update() {
    let mut app = trace_app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    let request = serde_json::from_value(json!({"id":93,"cmd":{
        "type":"trace_player","x":10,"y":20,"z":30,"heading":0,"updates":3
    }}))
    .unwrap();
    tx.send(Pending {
        request,
        reply: reply.clone(),
    })
    .unwrap();
    app.update();
    assert!(responses.try_recv().is_err());
    app.world_mut().resource_mut::<TraceOrdinary>().0 = 0;
    for _ in 0..2 {
        app.update();
        assert!(
            responses.try_recv().is_err(),
            "paused frames cannot complete the trace"
        );
    }
    app.world_mut().resource_mut::<TraceOrdinary>().0 = 1;
    for _ in 0..1 {
        app.update();
        assert!(
            responses.try_recv().is_err(),
            "trace must wait for all requested updates"
        );
    }
    app.update();
    let response = responses.try_recv().unwrap();
    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    let rows = result["observations"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_trace_rows(rows);
    assert!(responses.try_recv().is_err());
    app.world_mut().resource_mut::<TraceOrdinary>().0 = 2;
    let request = serde_json::from_value(json!({"id":94,"cmd":{
        "type":"trace_player","x":10,"y":20,"z":30,"updates":3
    }}))
    .unwrap();
    tx.send(Pending { request, reply }).unwrap();
    app.update();
    let rejected = responses.try_recv().unwrap();
    assert!(!rejected.ok);
    assert!(rejected.error.unwrap().contains("ordinary update skipped"));
}

#[test]
fn public_player_trace_rejects_invalid_bounds_and_absent_clock() {
    for (updates, error) in [
        (0, "updates must be 1..=255"),
        (256, "updates must be 1..=255"),
        (1, "game clock absent"),
    ] {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        let request = serde_json::from_value(json!({"id":94,"cmd":{
            "type":"trace_player","x":10,"y":20,"z":30,"updates":updates
        }}))
        .unwrap();
        tx.send(Pending { request, reply }).unwrap();
        app.update();
        let response = responses.try_recv().unwrap();
        assert!(!response.ok);
        assert!(response.error.unwrap().contains(error));
    }
}

#[test]
fn public_player_trace_rejects_world_retirement_and_conflicting_placement() {
    use cyrus_script::{clock::SimulationClock, script::ScriptActors};
    let mut app = script_app_with_player(true);
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        world.prepare_spatial().unwrap();
        let player = world.selected_player().unwrap();
        world
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .unwrap();
    }
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: false,
        use_smooth_divisor: false,
    })
    .unwrap();
    clock.advance(0);
    app.insert_resource(clock);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    for (id, cmd) in [
        (
            95,
            json!({"type":"trace_player","x":10,"y":20,"z":30,"updates":2}),
        ),
        (96, json!({"type":"place_player","x":99,"y":99,"z":99})),
    ] {
        tx.send(Pending {
            request: serde_json::from_value(json!({"id":id,"cmd":cmd})).unwrap(),
            reply: reply.clone(),
        })
        .unwrap();
    }
    app.update();
    assert!(responses.try_recv().is_err());
    app.update();
    let rejected = responses.try_recv().unwrap();
    assert_eq!(rejected.id, 96);
    assert!(rejected.error.unwrap().contains("owns placement"));
    let actors = app.world().resource::<ScriptActors>();
    let world = actors.current.as_ref().unwrap();
    assert_eq!(
        world
            .spatial(world.selected_player().unwrap())
            .unwrap()
            .local
            .position,
        [2560, 5120, 7680]
    );
    app.world_mut().resource_mut::<ScriptActors>().current = None;
    app.update();
    let rejected = responses.try_recv().unwrap();
    assert_eq!(rejected.id, 95);
    let error = rejected.error.unwrap();
    assert!(error.contains("world retired"));
    assert!(error.contains("observations:"));
}

fn entry_app() -> (App, tempfile::TempDir) {
    let mut app = script_app();
    app.insert_state(cyrus_game::AppState::InGame);
    app.insert_resource(cyrus_startup::NewGameEntry {
        route: cyrus_startup::NewGameRoute::RetailIntroduction,
        world: 17,
        marker: 3,
    });
    app.insert_resource(cyrus_script::script::ScriptStatus::Loaded {
        world: 17,
        content_id: String::new(),
        programs: 1,
        instances: 1,
        prepared: 1,
        waiting: 0,
        failed: 0,
        unsupported: 0,
        actor_flats: 0,
        ordinary_updates: 0,
        restored: false,
        examples: Default::default(),
    });
    app.insert_resource(cyrus_script::player::placement::MarkerEntry {
        marker: 3,
        heading: 1024,
    });
    // The actual New Game loader retains a numeric scene request alias.
    app.world_mut().resource_mut::<CurrentScene>().name = "17".into();
    app.world_mut().resource_mut::<CurrentScene>().world = Some(17);
    let animations = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(animations));
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("WORLD.INI"),
        "world_map[17]=MAPS\\contract.rgm\nworld_palette[17]=FXART\\contract.col\n",
    )
    .unwrap();
    app.insert_resource(cyrus_assets::install::GameInstall {
        root: directory.path().into(),
    });
    (app, directory)
}

fn read_entry(app: &mut App) -> serde_json::Value {
    run_commands(app, vec![Command::State]).remove(0)["entry"].clone()
}

#[test]
fn public_entry_provenance_survives_book_cleanup_and_requires_ready_matching_world_and_marker() {
    let (mut app, _directory) = entry_app();
    assert!(!app.world().contains_resource::<cyrus_startup::MenuState>());
    assert_eq!(
        read_entry(&mut app),
        json!({"route":"retail_introduction", "world":17,
        "marker":3, "scene":"CONTRACT"})
    );
    let ready = app
        .world_mut()
        .remove_resource::<cyrus_render::ReadyScene>()
        .unwrap();
    assert!(
        read_entry(&mut app).is_null(),
        "unprepared presentation is not entry evidence"
    );
    app.insert_resource(ready);
    let install = app
        .world_mut()
        .remove_resource::<cyrus_assets::install::GameInstall>()
        .unwrap();
    assert!(
        read_entry(&mut app).is_null(),
        "missing installed map identity cannot establish provenance"
    );
    app.insert_resource(install);
    let retired = app
        .world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current
        .take();
    assert!(
        read_entry(&mut app).is_null(),
        "cached loaded status cannot replace a live registry"
    );
    app.world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current = retired;
    app.world_mut()
        .resource_mut::<cyrus_script::player::placement::MarkerEntry>()
        .marker = 2;
    assert!(read_entry(&mut app).is_null());
    app.world_mut()
        .resource_mut::<cyrus_script::player::placement::MarkerEntry>()
        .marker = 3;
    app.world_mut()
        .resource_mut::<cyrus_startup::NewGameEntry>()
        .world = 18;
    assert!(read_entry(&mut app).is_null());
}

#[test]
fn direct_same_world_entry_cannot_inherit_retail_provenance() {
    let (mut app, _directory) = entry_app();
    assert!(!read_entry(&mut app).is_null());
    run_commands(&mut app, vec![Command::LoadScene { name: "17".into() }]);
    assert!(
        !app.world()
            .contains_resource::<cyrus_startup::NewGameEntry>()
    );
    assert!(
        read_entry(&mut app).is_null(),
        "direct same-world entry must not inherit retail provenance"
    );
}

#[test]
fn state_retains_actual_unsigned_bios_sample_without_advancing_clock() {
    use cyrus_script::clock::SimulationClock;
    let mut app = app();
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: false,
        use_smooth_divisor: false,
    })
    .unwrap();
    clock.advance(1110847);
    let before = serde_json::to_value(clock.snapshot()).unwrap();
    app.insert_resource(clock);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    for id in [1, 2] {
        let (reply, result) = channel();
        tx.send(Pending {
            request: Request {
                id,
                cmd: Command::State,
            },
            reply,
        })
        .unwrap();
        app.update();
        let state = result.try_recv().unwrap().result.unwrap();
        assert_eq!(state["clock"]["bios_sample"], 1110847);
        assert_eq!(state["clock"]["continuation"], before);
    }
}

#[test]
fn saved_image_command_rejects_unavailable_capture_with_metadata_context() {
    let mut app = script_app();
    app.insert_state(cyrus_game::AppState::Menu);
    app.world_mut().resource_mut::<CurrentScene>().world = Some(17);
    let animations = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(animations));
    app.init_resource::<crate::original_images::OriginalImages>();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, response) = channel();
    tx.send(Pending {
        request: Request {
            id: 13,
            cmd: Command::ReadSavedImages {
                savegame_path: "/absent/SAVEGAME.SAV".into(),
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    let response = response.try_recv().unwrap();
    assert!(!response.ok);
    let error = response.error.unwrap();
    assert!(
        error.contains("/absent/SAVEGAME.SAV")
            && error.contains("17")
            && error.contains("SCR3/THMB")
            && error.contains("captured images unavailable"),
        "{error}"
    );
}
