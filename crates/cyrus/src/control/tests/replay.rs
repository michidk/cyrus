//! Replay admission contracts.
use super::*;

#[test]
fn replay_world_pauses_unless_a_schedule_owns_the_frame_loop() {
    use super::replay_gate_open;
    // An in-game world past its first two updates is held between requests.
    assert!(!replay_gate_open(
        false,
        false,
        true,
        Some(2),
        false,
        false,
        false
    ));
    assert!(!replay_gate_open(
        false,
        false,
        true,
        Some(900),
        false,
        false,
        false
    ));
    // A pending scheduled boundary, startup, loading and a fresh world keep time running.
    assert!(replay_gate_open(
        true,
        false,
        true,
        Some(900),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false,
        false,
        false,
        Some(900),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false,
        false,
        true,
        Some(1),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false,
        false,
        true,
        Some(0),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false, false, true, None, false, false, false
    ));
    // An open examine view closes on real-time ticks, so a held world must keep time running.
    assert!(replay_gate_open(
        false,
        true,
        true,
        Some(900),
        false,
        false,
        false
    ));
    // A speech line still exporting or loading holds a started world even inside a schedule,
    // but never loading, a fresh world, a world-less game or an open examine view.
    assert!(!replay_gate_open(
        true,
        false,
        true,
        Some(900),
        true,
        false,
        false
    ));
    assert!(replay_gate_open(
        true,
        false,
        false,
        Some(900),
        true,
        false,
        false
    ));
    assert!(replay_gate_open(
        true,
        false,
        true,
        Some(1),
        true,
        false,
        false
    ));
    assert!(replay_gate_open(
        true, false, true, None, true, false, false
    ));
    assert!(replay_gate_open(
        true,
        true,
        true,
        Some(900),
        true,
        false,
        false
    ));
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
    if super::replay_gate_open(false, false, true, Some(0), false, true, false) {
        clock.advance_elapsed(0, std::time::Duration::ZERO);
    }
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), before);
    assert!(super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        false,
        true,
        false
    ));
    assert!(!super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        true,
        true,
        false
    ));
}

#[test]
fn exact_entry_requires_a_schedule_from_the_first_ordinary_body() {
    for updates in [0, 1, 2] {
        assert!(!super::replay_gate_open(
            false,
            false,
            true,
            Some(updates),
            false,
            false,
            true
        ));
        assert!(super::replay_gate_open(
            true,
            false,
            true,
            Some(updates),
            false,
            false,
            true
        ));
    }
    assert!(super::replay_gate_open(
        false,
        false,
        false,
        Some(0),
        false,
        false,
        true
    ));
}

#[test]
fn exact_entry_holds_the_loading_to_game_transition_before_any_random_draw() {
    let mut app = script_app();
    app.insert_resource(super::ReplayStep(std::time::Duration::from_millis(40)))
        .insert_resource(super::ReplayEntryHold)
        .init_resource::<cyrus_script::script::ReplayHold>();
    app.world_mut()
        .resource_mut::<NextState<cyrus_game::AppState>>()
        .set(cyrus_game::AppState::InGame);
    app.add_systems(
        Update,
        |state: Res<State<cyrus_game::AppState>>,
         hold: Res<cyrus_script::script::ReplayHold>,
         mut random: ResMut<cyrus_game::random::EngineRandom>| {
            if *state.get() == cyrus_game::AppState::InGame && !hold.0 {
                random.draw();
            }
        },
    );
    let initial = app
        .world()
        .resource::<cyrus_game::random::EngineRandom>()
        .state();
    for _ in 0..3 {
        app.update();
        assert_eq!(
            *app.world().resource::<State<cyrus_game::AppState>>().get(),
            cyrus_game::AppState::InGame
        );
        assert!(app.world().resource::<cyrus_script::script::ReplayHold>().0);
        assert_eq!(
            app.world()
                .resource::<cyrus_game::random::EngineRandom>()
                .state(),
            initial
        );
    }
}

fn origin_app() -> App {
    let mut app = frame_app();
    app.insert_resource(super::ReplayStep(std::time::Duration::from_millis(40)));
    app.insert_resource(cyrus_script::script::ReplayHold(true));
    app.insert_resource(super::ReplayEntryHold);
    app.world_mut().resource_mut::<CurrentScene>().world = Some(17);
    app
}

fn origin_command(app: &App, previous: u32, sample: u32) -> serde_json::Value {
    let actors = app.world().resource::<cyrus_script::script::ScriptActors>();
    json!({"type":"set_replay_clock_origin", "generation":actors.current.as_ref().unwrap().generation(),
        "expected_update":0, "expected_bios_sample":previous, "bios_sample":sample})
}

fn origin_request(app: &mut App, cmd: serde_json::Value) -> Response {
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, response) = channel();
    tx.send(Pending {
        request: serde_json::from_value(json!({"id":71,"cmd":cmd})).unwrap(),
        reply,
    })
    .unwrap();
    app.update();
    response.try_recv().unwrap()
}

#[test]
fn replay_origin_acknowledges_actual_sample_without_changing_timing_or_random() {
    use cyrus_script::clock::SimulationClock;
    let mut app = origin_app();
    let before =
        serde_json::to_value(app.world().resource::<SimulationClock>().snapshot()).unwrap();
    let random = app
        .world()
        .resource::<cyrus_game::random::EngineRandom>()
        .state();
    let cmd = origin_command(&app, 0, u32::MAX - 1);
    let reply = origin_request(&mut app, cmd);
    assert!(reply.ok, "{:?}", reply.error);
    let ack = reply.result.unwrap();
    assert_eq!(ack["bios_sample"], u32::MAX - 1);
    assert_eq!(ack["previous_bios_sample"], 0);
    assert_eq!(ack["generation"], origin_command(&app, 0, 0)["generation"]);
    assert_eq!(ack["ordinary_update"], 0);
    assert_eq!(ack["continuation"], before);
    let state = origin_request(&mut app, json!({"type":"state"}))
        .result
        .unwrap();
    assert_eq!(state["clock"]["bios_sample"], ack["bios_sample"]);
    assert_eq!(state["clock"]["continuation"], before);
    assert_eq!(
        app.world()
            .resource::<cyrus_game::random::EngineRandom>()
            .state(),
        random
    );
    // The next recorded interval is an unsigned wrapping delta from this origin.
    let mut clock = app.world_mut().resource_mut::<SimulationClock>();
    let next = clock.bios_sample().wrapping_add(3);
    clock.advance_recorded(next, 118).unwrap();
    assert_eq!(clock.bios_sample(), 1);
    let state = origin_request(&mut app, json!({"type":"state"}))
        .result
        .unwrap();
    assert_eq!(state["clock"]["bios_sample"], 1);
}

#[test]
fn replay_origin_rejects_stale_identity_sample_and_repeat_without_mutation() {
    use cyrus_script::clock::SimulationClock;
    for field in ["generation", "expected_update", "expected_bios_sample"] {
        let mut app = origin_app();
        let before =
            serde_json::to_value(app.world().resource::<SimulationClock>().snapshot()).unwrap();
        let mut cmd = origin_command(&app, 0, 1110847);
        cmd[field] = json!(99);
        let reply = origin_request(&mut app, cmd);
        assert!(!reply.ok, "{field}");
        assert!(reply.error.unwrap().contains("stale"), "{field}");
        let clock = app.world().resource::<SimulationClock>();
        assert_eq!(clock.bios_sample(), 0);
        assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), before);
        // A rejected command does not consume the one permitted setup.
        let cmd = origin_command(&app, 0, 1110847);
        assert!(origin_request(&mut app, cmd).ok);
        let cmd = origin_command(&app, 1110847, 3);
        assert!(!origin_request(&mut app, cmd).ok);
        assert_eq!(
            app.world().resource::<SimulationClock>().bios_sample(),
            1110847
        );
    }
}

#[test]
fn replay_origin_rejects_nonreplay_unheld_unready_and_accepted_input() {
    for condition in [
        "nonreplay",
        "unheld",
        "unready",
        "press-release",
        "pending-frame",
    ] {
        let mut app = origin_app();
        match condition {
            "nonreplay" => {
                app.world_mut().remove_resource::<super::ReplayStep>();
            }
            "unheld" => {
                app.world_mut().remove_resource::<super::ReplayEntryHold>();
            }
            "unready" => {
                app.world_mut()
                    .remove_resource::<cyrus_render::ReadyScene>();
            }
            "press-release" => {
                for action in ["press", "release"] {
                    assert!(
                        origin_request(
                            &mut app,
                            json!({"type":"input","key":"AltLeft","action":action})
                        )
                        .ok
                    );
                }
            }
            "pending-frame" => {
                let (tx, rx) = channel();
                app.insert_resource(Inbox(Mutex::new(rx)));
                let (reply, _response) = channel();
                tx.send(Pending {request:serde_json::from_value(json!({"id":70,"cmd":{"type":"replay_frame","frame_scale":118,"elapsed_bios_ticks":0}})).unwrap(),reply}).unwrap();
                app.update();
            }
            _ => unreachable!(),
        }
        let cmd = origin_command(&app, 0, 1110847);
        assert!(!origin_request(&mut app, cmd).ok, "{condition}");
        assert_eq!(
            app.world()
                .resource::<cyrus_script::clock::SimulationClock>()
                .bios_sample(),
            0,
            "{condition}"
        );
    }
}

#[test]
fn replay_origin_rejects_a_world_after_its_first_body() {
    let mut app = trace_app();
    app.insert_resource(super::ReplayStep(std::time::Duration::from_millis(40)));
    app.insert_resource(cyrus_script::script::ReplayHold(true));
    app.insert_resource(super::ReplayEntryHold);
    app.world_mut().resource_mut::<CurrentScene>().world = Some(17);
    let animations = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .animations
        .clone();
    app.insert_resource(cyrus_render::ReadyScene(animations));
    app.update();
    app.world_mut().resource_mut::<TraceOrdinary>().0 = 0;
    let sample = app
        .world()
        .resource::<cyrus_script::clock::SimulationClock>()
        .bios_sample();
    let mut cmd = origin_command(&app, sample, 1110847);
    cmd["expected_update"] = json!(1);
    let response = origin_request(&mut app, cmd);
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("requires update0"));
    assert_eq!(
        app.world()
            .resource::<cyrus_script::clock::SimulationClock>()
            .bios_sample(),
        sample
    );
}

#[test]
fn aligned_replay_rejects_a_synthetic_interval_before_scheduling() {
    let mut app = origin_app();
    let cmd = origin_command(&app, 0, 1110847);
    assert!(origin_request(&mut app, cmd).ok);
    let response = origin_request(&mut app, json!({"type":"replay_frame","frame_scale":118}));
    assert!(!response.ok);
    assert!(
        response
            .error
            .unwrap()
            .contains("observed elapsed_bios_ticks")
    );
    assert_eq!(
        app.world()
            .resource::<cyrus_script::clock::SimulationClock>()
            .bios_sample(),
        1110847
    );
    assert!(
        !app.world()
            .resource::<super::input_schedule::InputScheduler>()
            .pending()
    );
}

#[test]
fn replay_origin_ack_and_state_in_the_same_drain_publish_one_current_sample() {
    let mut app = origin_app();
    let cmd = origin_command(&app, 0, 1110847);
    let (tx, rx) = channel();
    app.insert_resource(Inbox(Mutex::new(rx)));
    let (reply, responses) = channel();
    for (id, cmd) in [(1, cmd), (2, json!({"type":"state"}))] {
        tx.send(Pending {
            request: serde_json::from_value(json!({"id":id,"cmd":cmd})).unwrap(),
            reply: reply.clone(),
        })
        .unwrap();
    }
    app.update();
    let ack = responses.try_recv().unwrap();
    assert!(ack.ok, "{:?}", ack.error);
    let state = responses.try_recv().unwrap();
    assert!(state.ok, "{:?}", state.error);
    let ack = ack.result.unwrap();
    let state = state.result.unwrap();
    assert_eq!(ack["bios_sample"], 1110847);
    assert_eq!(state["clock"]["bios_sample"], ack["bios_sample"]);
    assert_eq!(state["clock"]["continuation"], ack["continuation"]);
}
