//! Public acquisition command contracts.
use super::*;

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
