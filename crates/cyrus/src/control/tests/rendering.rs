//! Public rendering command contracts.
use super::*;

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
