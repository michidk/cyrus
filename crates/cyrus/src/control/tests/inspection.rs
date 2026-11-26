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
