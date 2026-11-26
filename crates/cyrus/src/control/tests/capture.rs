//! Public capture command contracts.
use super::*;

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
