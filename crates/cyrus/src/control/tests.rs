//! Public native control and ordering contracts.
#[test]
fn replay_world_pauses_unless_a_schedule_owns_the_frame_loop() {
    use super::replay_gate_open;
    // An in-game world past its first two updates is held between requests.
    assert!(!replay_gate_open(false, false, true, Some(2), false, false));
    assert!(!replay_gate_open(
        false,
        false,
        true,
        Some(900),
        false,
        false
    ));
    // A pending scheduled boundary, startup, loading and a fresh world keep time running.
    assert!(replay_gate_open(true, false, true, Some(900), false, false));
    assert!(replay_gate_open(
        false,
        false,
        false,
        Some(900),
        false,
        false
    ));
    assert!(replay_gate_open(false, false, true, Some(1), false, false));
    assert!(replay_gate_open(false, false, true, Some(0), false, false));
    assert!(replay_gate_open(false, false, true, None, false, false));
    // An open examine view closes on real-time ticks, so a held world must keep time running.
    assert!(replay_gate_open(false, true, true, Some(900), false, false));
    // A speech line still exporting or loading holds a started world even inside a schedule,
    // but never loading, a fresh world, a world-less game or an open examine view.
    assert!(!replay_gate_open(true, false, true, Some(900), true, false));
    assert!(replay_gate_open(true, false, false, Some(900), true, false));
    assert!(replay_gate_open(true, false, true, Some(1), true, false));
    assert!(replay_gate_open(true, false, true, None, true, false));
    assert!(replay_gate_open(true, true, true, Some(900), true, false));
}

#[test]
fn restored_replay_starts_held_without_consuming_the_saved_clock() {
    use cyrus_script::clock::SimulationClock;
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: true,
        use_smooth_divisor: true,
    })
    .unwrap();
    clock.advance_recorded(0, 118).unwrap();
    let before = serde_json::to_value(clock.snapshot()).unwrap();
    // This is the restored world's first host update: its diagnostic count is
    // zero, but a complete continuation needs no fresh-world startup frames.
    if super::replay_gate_open(false, false, true, Some(0), false, true) {
        clock.advance_elapsed(0, std::time::Duration::ZERO);
    }
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), before);
    assert!(super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        false,
        true
    ));
    assert!(!super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        true,
        true
    ));
}

mod fixtures;
use super::*;
use cyrus_assets::surface::Surface;
use cyrus_control::Request;
use fixtures::*;
use std::sync::mpsc::channel;

#[test]
fn public_fidelity_uses_registry_areas_and_reports_current_choices() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    for area in Area::ALL {
        let name = serde_json::to_value(area).unwrap();
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 1,
                cmd: Command::SetFidelity {
                    area: name.as_str().unwrap().into(),
                    mode: "modern".into(),
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        let response = responses.try_recv().unwrap();
        assert!(response.error.is_none(), "{response:?}");
        assert_eq!(app.world().resource::<Fidelity>().mode(area), Mode::Modern);
    }
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 2,
            cmd: Command::SetFidelity {
                area: "player_controller".into(),
                mode: "modern".into(),
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    let error = responses.try_recv().unwrap().error.unwrap();
    for area in Area::ALL {
        let name = serde_json::to_value(area).unwrap();
        assert!(error.contains(name.as_str().unwrap()), "{error}");
    }
}

#[test]
fn rendered_actor_generation_wait_survives_unready_replacement_scene() {
    let mut app = script_app();
    app.insert_state(cyrus_game::AppState::InGame);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 83,
            cmd: Command::RenderedActor {
                index: 0,
                after_generation: Some(0),
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(
        responses.try_recv().is_err(),
        "logical replacement must wait for rendered readiness"
    );
    let animations = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    let actor = app
        .world()
        .resource::<cyrus_script::script::ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .id(0)
        .unwrap();
    let owner = app
        .world_mut()
        .spawn((
            Name::new("ACTOR"),
            cyrus_script::script::ScriptBinding {
                actor,
                source: Handle::default(),
            },
        ))
        .id();
    let loading_model = app
        .world_mut()
        .spawn((WorldAssetRoot(Handle::default()), ChildOf(owner)))
        .id();
    app.insert_resource(cyrus_render::ReadyScene(animations));
    app.update();
    assert!(
        responses.try_recv().is_err(),
        "selected model instance must settle before observation"
    );
    app.world_mut().despawn(loading_model);
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 83);
    assert_eq!(
        response.error.as_deref(),
        Some("rendered actor observation: game clock absent")
    );
}

#[test]
fn rendered_actor_generation_wait_survives_the_empty_world_handoff() {
    let mut app = app();
    app.insert_state(cyrus_game::AppState::InGame);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 82,
            cmd: Command::RenderedActor {
                index: 0,
                after_generation: Some(1),
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(responses.try_recv().is_err());
    app.world_mut()
        .resource_mut::<rendered_actor::PendingRenderedActor>()
        .deadline = std::time::Instant::now();
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 82);
    assert!(response.error.unwrap().contains("generation wait expired"));
}

#[test]
fn rendered_actor_observation_replies_after_update_and_rejects_absent_owners() {
    let mut app = app();
    app.insert_state(cyrus_game::AppState::InGame);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 81,
            cmd: Command::RenderedActor {
                index: 0,
                after_generation: None,
            },
        },
        reply,
    })
    .unwrap();
    app.add_systems(
        Update,
        |pending: Option<Res<rendered_actor::PendingRenderedActor>>| {
            assert!(
                pending.is_some(),
                "request must wait until presentation updates complete"
            );
        },
    );
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 81);
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("current world absent"));
    assert!(
        !app.world()
            .contains_resource::<rendered_actor::PendingRenderedActor>()
    );
}

#[test]
fn bound_light_request_observes_update_and_preserves_later_command_order() {
    let mut app = app();
    app.insert_state(cyrus_game::AppState::InGame);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 42,
            cmd: Command::BoundLightImage {
                name: "selected-mesh".into(),
                primitive: 0,
            },
        },
        reply: reply.clone(),
    })
    .unwrap();
    tx.send(Pending {
        request: Request {
            id: 43,
            cmd: Command::Ping,
        },
        reply,
    })
    .unwrap();
    app.add_systems(Update, |mut commands: Commands| {
        commands.spawn(Name::new("selected-mesh"));
        commands.spawn(Name::new("selected-mesh"));
    });
    assert!(responses.try_recv().is_err());
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 42);
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("ambiguous"));
    assert!(responses.try_recv().is_err());
    assert!(
        !app.world()
            .contains_resource::<light_image::PendingLightImage>()
    );
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 43);
    assert_eq!(response.result.unwrap(), json!("pong"));
    assert!(responses.try_recv().is_err());
}

#[test]
fn public_recorded_frame_requires_explicit_replay_mode() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    let request = serde_json::from_value(json!({"id":93,"cmd":{
        "type":"replay_frame","frame_scale":118
    }}))
    .unwrap();
    tx.send(Pending { request, reply }).unwrap();
    app.update();
    let response = responses.try_recv().unwrap();
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("--replay-step-micros"));
    assert!(
        !app.world()
            .contains_resource::<cyrus_script::script::ReplayFrameScale>()
    );
}

#[test]
fn public_schedule_input_rejects_unavailable_world_without_pressing() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    let request = serde_json::from_value(json!({"id":91,"cmd":{"type":"schedule_input","events":[
        {"after_updates":0,"key":"ArrowUp","action":"press"},
        {"after_updates":2,"key":"ArrowUp","action":"release"}
    ]}}))
    .unwrap();
    tx.send(Pending { request, reply }).unwrap();
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 91);
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("no world"));
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ArrowUp)
    );
}

#[test]
fn public_clock_acquisition_rejects_unavailable_world_without_pressing() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    let request = serde_json::from_value(json!({"id":92,"cmd":{
            "type":"acquire_input","key":"ArrowUp","endpoints":[1024,1334],"normal_frame_rate":12,"max_updates":8
        }})).unwrap();
    tx.send(Pending { request, reply }).unwrap();
    app.update();
    let response = responses.try_recv().unwrap();
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("no world"));
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ArrowUp)
    );
}

#[test]
fn effect_geometry_waits_for_an_admitted_update_after_scene_readiness() {
    let mut app = script_app();
    app.insert_state(cyrus_game::AppState::InGame);
    let animations = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(animations));
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 84,
            cmd: Command::EffectGeometry {
                after_generation: Some(0),
            },
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(
        responses.try_recv().is_err(),
        "asset readiness is not an admitted world update"
    );
    app.world_mut()
        .resource_mut::<effect_geometry::PendingEffectGeometry>()
        .deadline = std::time::Instant::now();
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 84);
    assert!(response.error.unwrap().contains("deadline expired"));
}

#[test]
fn armed_effect_geometry_request_allows_load_commands_and_has_a_deadline() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    tx.send(Pending {
        request: Request {
            id: 40,
            cmd: Command::EffectGeometry {
                after_generation: Some(0),
            },
        },
        reply: reply.clone(),
    })
    .unwrap();
    tx.send(Pending {
        request: Request {
            id: 41,
            cmd: Command::Ping,
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(responses.try_recv().is_err());
    app.update();
    assert_eq!(responses.try_recv().unwrap().id, 41);
    assert!(responses.try_recv().is_err());
    app.world_mut()
        .resource_mut::<effect_geometry::PendingEffectGeometry>()
        .deadline = std::time::Instant::now();
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 40);
    assert!(response.error.unwrap().contains("deadline expired"));
    assert!(
        !app.world()
            .contains_resource::<effect_geometry::PendingEffectGeometry>()
    );
}

#[test]
fn effect_geometry_request_is_one_shot_and_keeps_following_commands_ordered() {
    let mut app = app();
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    for (id, cmd) in [
        (
            42,
            Command::EffectGeometry {
                after_generation: None,
            },
        ),
        (43, Command::Ping),
    ] {
        tx.send(Pending {
            request: Request { id, cmd },
            reply: reply.clone(),
        })
        .unwrap();
    }
    app.update();
    let response = responses.try_recv().unwrap();
    assert_eq!(response.id, 42);
    assert!(response.error.unwrap().contains("service absent"));
    assert!(responses.try_recv().is_err());
    app.update();
    assert_eq!(responses.try_recv().unwrap().id, 43);
    assert!(
        !app.world()
            .contains_resource::<effect_geometry::PendingEffectGeometry>()
    );
}

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

#[test]
fn placement_acknowledges_application_before_later_clock_advance() {
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
    let placement = || Command::PlacePlayer {
        x: 10,
        y: 20,
        z: 30,
        heading: 40,
    };
    let unavailable = run_commands(&mut app, vec![placement()]).remove(0);
    assert_eq!(unavailable["acknowledged"], false);
    assert!(unavailable["boundary"].is_null());
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: false,
        use_smooth_divisor: false,
    })
    .unwrap();
    clock.advance(0);
    let scale = clock.elapsed_frame_scale();
    app.insert_resource(clock)
        .add_systems(Update, |mut clock: ResMut<SimulationClock>| {
            clock.advance(18);
        });
    let result = run_commands(&mut app, vec![placement()]).remove(0);
    assert_eq!(result["acknowledged"], true);
    let boundary = &result["boundary"];
    assert_eq!(boundary["elapsed_scale"], scale);
    assert_eq!(boundary["phase"], "before_ordinary_update");
    assert_eq!(
        boundary["player"]["spatial"]["local"]["position"],
        json!([2560, 5120, 7680])
    );
    assert!(
        app.world()
            .resource::<SimulationClock>()
            .elapsed_frame_scale()
            > scale
    );
}

#[test]
fn scheduled_frame_waits_for_the_observed_clock_and_rejects_skipped_clock() {
    use bevy::render::view::screenshot::Screenshot;
    use cyrus_control::FrameBoundary;
    use cyrus_script::{clock::SimulationClock, script::ScriptActors};
    let mut app = frame_app();
    let generation = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .generation();
    let scale = app
        .world()
        .resource::<SimulationClock>()
        .elapsed_frame_scale();
    let reply = request_frame_at(
        &mut app,
        std::path::Path::new("scheduled-frame.png"),
        None,
        Some(FrameBoundary::Clock {
            generation,
            elapsed_scale: scale + 256,
        }),
    );
    assert!(reply.try_recv().is_err());
    assert_eq!(
        app.world_mut()
            .query::<&Screenshot>()
            .iter(app.world())
            .count(),
        0
    );
    app.world_mut()
        .resource_mut::<SimulationClock>()
        .advance(18);
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&Screenshot>()
            .iter(app.world())
            .count(),
        1
    );
    assert!(reply.try_recv().is_err());

    let mut app = frame_app();
    let reply = request_frame_at(
        &mut app,
        std::path::Path::new("skipped-frame.png"),
        None,
        Some(FrameBoundary::Clock {
            generation,
            elapsed_scale: scale - 1,
        }),
    );
    let response = reply.try_recv().unwrap();
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("clock skipped"));
    assert_eq!(
        app.world_mut()
            .query::<&Screenshot>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn presentation_capture_freezes_loading_state_without_a_ready_world() {
    let mut app = frame_app();
    app.insert_resource(State::new(cyrus_game::AppState::Loading));
    app.world_mut()
        .remove_resource::<cyrus_world::lifecycle::PreparedWorld>();
    app.world_mut()
        .remove_resource::<cyrus_render::ReadyScene>();
    app.world_mut()
        .remove_resource::<cyrus_script::script::ScriptActors>();
    app.world_mut()
        .remove_resource::<cyrus_script::clock::SimulationClock>();
    // An unrelated black UI background must not be reported as a world-entry fade.
    app.world_mut().spawn(BackgroundColor(Color::BLACK));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("loading-frame.png");
    let _ = std::fs::remove_file(&path);
    let (tx, rx) = channel();
    let (reply, results) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    tx.send(Pending {
        request: Request {
            id: 42,
            cmd: serde_json::from_value(json!({
                "type": "capture_presentation_frame", "path": path,
            }))
            .unwrap(),
        },
        reply,
    })
    .unwrap();
    app.update();
    assert!(results.try_recv().is_err());
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<Screenshot>>()
        .single(app.world())
        .unwrap();
    app.insert_resource(State::new(cyrus_game::AppState::Menu));
    let response = finish_capture(&mut app, entity, 1, &results);
    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["boundary"]["state"], "Loading");
    assert_eq!(result["boundary"]["phase"], "render_extraction");
    assert_eq!(result["boundary"]["fade_opacity_256"], json!([]));
    assert!(result["boundary"]["render_frame"].is_u64());
    assert!(result["boundary"].get("player").is_none());
    assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn gameplay_frame_clock_pose_and_pcm_cursor_survive_delayed_readback() {
    let mut app = frame_app();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned-frame.png");
    let raw = path.with_extension("raw");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&raw, [0u8; 8]).unwrap();
    let reply = request_frame(&mut app, &path, Some(raw.display().to_string()));
    assert!(reply.try_recv().is_err(), "GPU completion is asynchronous");
    let busy = request_frame(&mut app, &path.with_extension("busy.png"), None)
        .try_recv()
        .unwrap();
    assert!(!busy.ok);
    assert!(busy.error.unwrap().contains("already pending"));
    app.world_mut()
        .resource_mut::<cyrus_script::clock::SimulationClock>()
        .advance(18);
    app.world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current
        .as_mut()
        .unwrap()
        .place_selected_player([999; 3], 0)
        .unwrap();
    std::fs::write(&raw, [0u8; 40]).unwrap();
    app.world_mut()
        .query_filtered::<&mut Transform, With<Player>>()
        .single_mut(app.world_mut())
        .unwrap()
        .translation = Vec3::splat(9.);
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<Screenshot>>()
        .single(app.world())
        .unwrap();
    let response = finish_capture(&mut app, entity, 2, &reply);
    assert!(response.ok, "{:?}", response.error);
    let result = response.result.unwrap();
    assert_eq!(result["boundary"]["elapsed_scale"], 256);
    assert_eq!(result["boundary"]["pcm_delivery_frames"], 2);
    assert_eq!(
        result["boundary"]["player"]["spatial"]["local"]["position"],
        json!([0, 0, 0])
    );
    assert_eq!(result["boundary"]["phase"], "render_extraction");
    assert_eq!(
        result["boundary"]["presentation"]["translation"],
        json!([0., 0., 0.])
    );
    assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(raw).unwrap();
}

#[test]
fn gameplay_frame_rejects_unavailable_world_and_missing_pcm_before_readback() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rejected-frame.png");
    let mut unavailable = app();
    let response = request_frame(&mut unavailable, &path, None)
        .try_recv()
        .unwrap();
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("active world"));
    let mut ready = frame_app();
    let response = request_frame(
        &mut ready,
        &path,
        Some(path.with_extension("missing").display().to_string()),
    )
    .try_recv()
    .unwrap();
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("PCM"));
    assert!(!path.exists());
}

#[test]
fn speech_command_admits_uppercase_installed_labels() {
    let mut app = app();
    app.init_resource::<cyrus_audio::sound::Speech>()
        .add_message::<cyrus_audio::sound::SoundRequest>();
    app.world_mut()
        .resource_mut::<cyrus_audio::sound::Speech>()
        .0
        .insert(
            "cisw".into(),
            cyrus_assets::sound::VoiceClip {
                audio: Handle::default(),
                bits_per_sample: 8,
                base_volume: 127,
                seconds: 2.5,
                text: "description".into(),
            },
        );
    let result = run_commands(
        &mut app,
        vec![Command::PlaySpeech {
            label: "CISW".into(),
        }],
    );
    assert_eq!(result[0]["label"], "CISW");
    let requests = app
        .world_mut()
        .resource_mut::<Messages<cyrus_audio::sound::SoundRequest>>()
        .drain()
        .collect::<Vec<_>>();
    assert!(
        matches!(requests.as_slice(), [cyrus_audio::sound::SoundRequest::Speech(label)] if label == "CISW")
    );
}

#[test]
fn capture_readiness_requires_the_current_world_to_finish_spawning() {
    let mut app = script_app();
    app.insert_state(cyrus_game::AppState::InGame);
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["state"], "InGame");
    assert_eq!(state["scene_ready"], false);
    // The engine generator's state is read-only diagnostics for replay determinism checks.
    assert_eq!(state["random_state"], 1);

    let current = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(current));
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["scene_ready"], true);

    app.world_mut()
        .resource_mut::<cyrus_world::lifecycle::PreparedWorld>()
        .animations = default();
    let state = run_commands(&mut app, vec![Command::State]).remove(0);
    assert_eq!(state["scene_ready"], false);
}

#[test]
fn cheat_command_supplies_player_distance_state_to_script_steps() {
    use cyrus_game::cheats::{CheatStore, Cheats};
    struct Memory;
    impl CheatStore for Memory {
        fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
            Ok(cyrus_assets::cheats::empty())
        }
        fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let mut app = app();
    let dialogue = json!({"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"});
    let mut script_json = json!({
        "version":30,"dialogue":dialogue,"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},
        "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},
        "actor_pool_capacity":10,"content_id":"","world":17,
        "definition":{"functions":[
            {"name":"NullFunction","param_count":0,"kind":"function"},
            {"name":"PlayerDistance","param_count":0,"kind":"function"}
        ],"flags":[],"references":[],"attributes":[]},
        "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
            "namespace":"ACTOR","instance_variables":[[0]],"location_offsets":[],"attributes":vec![0u8; 256],"native_hook":null,
            "program":{"version":4,"entry":0,"variables":[0],"steps":{
                "0":{"next":1,"operation":{"type":"assign","destination":{"type":"local","index":0},
                    "formula":{"first":{"type":"call","call":{"function_id":1,"kind":"function","target":{"type":"me"},"function":"PlayerDistance","arguments":[]}},"rest":[]}}},
                "1":{"next":2,"operation":{"type":"end","target":0}}
            }}
        }},
        "instances":[{"object_id":0,"actor":"ACTOR","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}],"unsupported":{}
    });
    script_json["weapons"] = json!({});
    script_json["combat_attacks"] = json!([]);
    script_json["combat_clank_effects"] = json!([null, null]);
    script_json["combat_defenses"] = json!([]);
    let mut scripts: cyrus_assets::script::Scripts = serde_json::from_value(script_json).unwrap();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut actors = cyrus_script::script::ScriptActors::default();
    actors.current = Some(cyrus_script::script::ScriptWorld::prepare(&scripts, 1).unwrap());
    app.insert_resource(actors)
        .insert_resource(Cheats::new(Memory));
    let step = |stage| Command::StepScript {
        generation: 1,
        index: 0,
        stage,
        frame_scale: Some(256),
        nominal_ticks: Some(0),
        distance_override: None,
        fallback_position: None,
    };
    let responses = run_commands(
        &mut app,
        vec![
            Command::Cheat {
                name: "neversaydie".into(),
                value: Some(7),
            },
            step(cyrus_control::ScriptStage::PrepareSpatial),
            step(cyrus_control::ScriptStage::Main),
            Command::State,
        ],
    );
    assert_eq!(responses[0]["effect"], "player_distance");
    assert_eq!(responses[2]["actors"][0]["locals"], json!([9_999_999]));
    assert_eq!(responses[3]["cheats"]["ready"], true);
    assert_eq!(responses[3]["cheats"]["entries"][10]["value"], 7);
}

#[test]
fn node_less_script_sound_is_captured_without_poisoning_the_actor() {
    let mut app = script_app();
    let responses = run_commands(&mut app, bind_and_step(None));
    assert_eq!(
        responses[1]["captured_sounds"],
        json!([{
            "actor":{"generation":1,"index":0},
            "sound":{"type":"play","effect":-17,"rate_offset_hz":-64}
        }])
    );
    let response = run_commands(
        &mut app,
        vec![Command::StepScript {
            generation: 1,
            index: 0,
            stage: cyrus_control::ScriptStage::Tasks,
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            distance_override: None,
            fallback_position: None,
        }],
    );
    assert_eq!(response[0]["captured_sounds"], json!([]));
    let response = run_commands(
        &mut app,
        vec![Command::StepScript {
            generation: 1,
            index: 0,
            stage: cyrus_control::ScriptStage::Animation,
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            distance_override: None,
            fallback_position: None,
        }],
    );
    assert_eq!(
        response[0]["captured_sounds"],
        json!([{
            "actor":{"generation":1,"index":0},
            "sound":{"type":"check_sample"}
        }])
    );
    let actors = app.world().resource::<cyrus_script::script::ScriptActors>();
    assert!(
        actors.current.as_ref().unwrap().actors()[0]
            .failure()
            .is_none()
    );
}

#[test]
fn script_binding_routes_sound_in_the_same_command_drain() {
    use cyrus_world::{animation::AnimationSound, sound::SoundCue};
    let mut app = script_app();
    app.add_message::<SoundCue>();
    let source = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    let entity = app
        .world_mut()
        .spawn((
            Name::new("actor-node"),
            cyrus_script::script::ScriptBinding {
                actor: cyrus_script::script::ScriptActorId {
                    generation: 0,
                    index: 0,
                },
                source,
            },
            cyrus_world::animation::AnimationPose {
                model: "OLD".into(),
                frame: 9,
            },
        ))
        .id();
    let responses = run_commands(&mut app, bind_and_step(Some("actor-node")));
    assert_eq!(responses[1]["captured_sounds"], json!([]));
    assert!(
        app.world()
            .get::<cyrus_world::animation::AnimationPose>(entity)
            .is_none()
    );
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert_eq!(cues.len(), 1);
    assert!(matches!(cues[0], SoundCue::Animation {
            actor,
            sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(Surface::UNKNOWN)
        } if actor == entity));
    assert_eq!(
        app.world()
            .get::<cyrus_script::script::ScriptBinding>(entity)
            .unwrap()
            .actor
            .index,
        0
    );
}

#[test]
fn animation_sound_uses_unnamed_player_bindings() {
    use cyrus_world::{animation::AnimationSound, sound::SoundCue};
    let mut app = script_app();
    app.add_message::<SoundCue>();
    let entity = app.world_mut().spawn(Name::new("player")).id();
    let mut commands = bind_and_step(Some("player"));
    let step = commands.pop().unwrap();
    run_commands(&mut app, commands);
    app.world_mut().entity_mut(entity).remove::<Name>();
    let responses = run_commands(&mut app, vec![step]);
    assert_eq!(responses[0]["captured_sounds"], json!([]));
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert_eq!(cues.len(), 1);
    assert!(matches!(cues[0], SoundCue::Animation {
            actor, sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(Surface::UNKNOWN)
        } if actor == entity));
}
