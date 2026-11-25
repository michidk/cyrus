use super::*;

#[test]
fn world_loop_falling_timeout_retains_wraps_and_resets_independently_of_inhibition() {
    use cyrus_script::script::{
        ScriptActorSource, WorldLoopOutcome, WorldRestoreContext, WorldSnapshotSources,
    };
    let (scripts, mut world, mut data, _) = player_walking_fixture();
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    world
        .update_ordinary(
            &data,
            [0, 0],
            cyrus_script::script::OrdinaryInputs {
                scene: None,
                scripts: cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    let index = world
        .actors()
        .iter()
        .position(|a| matches!(a.source, ScriptActorSource::Player))
        .unwrap();
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][index]["control"]["falling_state"] = 128.into();
    let globals = std::mem::take(world.globals_mut());
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(world.globals().player_hit_inhibited(), Some(false));
    for (ticks, elapsed, outcome) in [
        (u32::MAX, u32::MAX, WorldLoopOutcome::Continue),
        (1, 0, WorldLoopOutcome::Continue),
        (144, 144, WorldLoopOutcome::Continue),
        (0, 144, WorldLoopOutcome::Continue),
        (1, 145, WorldLoopOutcome::Death),
    ] {
        assert_eq!(world.finish_world_loop(ticks).unwrap(), outcome);
        assert_eq!(
            world.globals().selected_health().unwrap().falling_elapsed(),
            Some(elapsed)
        );
    }
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][index]["control"]["falling_state"] = 0.into();
    let globals = std::mem::take(world.globals_mut());
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 3,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        world.finish_world_loop(100).unwrap(),
        WorldLoopOutcome::Continue
    );
    assert_eq!(
        world.globals().selected_health().unwrap().falling_elapsed(),
        Some(0)
    );
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][index]["control"]["falling_state"] = 1.into();
    let globals = std::mem::take(world.globals_mut());
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 4,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        world.finish_world_loop(144).unwrap(),
        WorldLoopOutcome::Continue
    );
    let mut clock = cyrus_script::clock::SimulationClock::new(scripts.clock).unwrap();
    assert_eq!(clock.advance(0).nominal_ticks, 1);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(cyrus_script::script::ScriptPlugin);
    app.insert_resource(clock);
    app.world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current = Some(world);
    app.world_mut().run_schedule(bevy::prelude::Last);
    assert_eq!(
        app.world()
            .resource::<cyrus_script::script::ScriptActors>()
            .world_loop_outcome,
        Some(WorldLoopOutcome::Death)
    );

    app.world_mut().run_schedule(bevy::prelude::Last);
    assert_eq!(
        app.world()
            .resource::<cyrus_script::script::ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .globals()
            .selected_health()
            .unwrap()
            .falling_elapsed(),
        Some(145)
    );

    let world = app
        .world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current
        .take()
        .unwrap();
    let mut host = bevy::prelude::App::new();
    host.add_plugins(cyrus_script::script::ScriptPlugin)
        .init_resource::<cyrus_script::script::DeathDispatch>()
        .init_resource::<bevy::prelude::NextState<cyrus_game::AppState>>()
        .insert_resource(cyrus_script::clock::SimulationClock::new(scripts.clock).unwrap());
    host.world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current = Some(world);
    let publication = cyrus_script::script::SessionPublicationBoundary::new(
        host.world()
            .resource::<cyrus_script::script::ScriptActors>()
            .current
            .as_ref()
            .unwrap(),
    );
    host.insert_resource(publication);
    host.world_mut().run_schedule(bevy::prelude::Last);
    assert!(
        !host
            .world()
            .contains_resource::<cyrus_script::script::SessionPublicationBoundary>()
    );
    assert!(
        !host
            .world()
            .contains_resource::<cyrus_script::script::DeathBookEntry>()
    );
    assert!(
        host.world()
            .resource::<cyrus_script::script::ScriptActors>()
            .world_loop_outcome
            .is_none()
    );
    host.world_mut().run_schedule(bevy::prelude::Last);
    assert_eq!(
        host.world()
            .resource::<cyrus_script::script::DeathBookEntry>()
            .override_group,
        0
    );
    assert!(matches!(
        host.world()
            .resource::<bevy::prelude::NextState<cyrus_game::AppState>>(),
        bevy::prelude::NextState::Pending(cyrus_game::AppState::Menu)
    ));
    assert!(
        !host
            .world()
            .contains_resource::<cyrus_world::lifecycle::SceneRequest>()
    );
    assert_eq!(
        host.world()
            .resource::<cyrus_script::script::ScriptActors>()
            .world_loop_outcome,
        Some(WorldLoopOutcome::Death)
    );

    // Missing caller context remains unsupported; a known context requests confirmation.
    let world = host
        .world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current
        .take()
        .unwrap();
    let mut confirmation = bevy::prelude::App::new();
    confirmation
        .add_plugins(cyrus_script::script::ScriptPlugin)
        .insert_resource(cyrus_script::script::DeathDispatch {
            last_save: Some(cyrus_script::script::LastSaveSelection {
                slot: 0,
                description: "saved state".into(),
            }),
            book_context: None,
        })
        .init_resource::<bevy::prelude::NextState<cyrus_game::AppState>>()
        .insert_resource(cyrus_script::clock::SimulationClock::new(scripts.clock).unwrap());
    confirmation
        .world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current = Some(world);
    confirmation.world_mut().run_schedule(bevy::prelude::Last);
    assert!(
        !confirmation
            .world()
            .contains_resource::<cyrus_script::script::DeathBookEntry>()
    );
    assert!(
        !confirmation
            .world()
            .contains_resource::<cyrus_world::lifecycle::SceneRequest>()
    );
    assert!(matches!(
        confirmation
            .world()
            .resource::<bevy::prelude::NextState<cyrus_game::AppState>>(),
        bevy::prelude::NextState::Unchanged
    ));
    assert_eq!(
        confirmation
            .world()
            .resource::<cyrus_script::script::DeathDispatch>()
            .last_save
            .as_ref()
            .unwrap()
            .slot,
        0
    );
    let world = confirmation
        .world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current
        .take()
        .unwrap();
    let mut live = bevy::prelude::App::new();
    live.add_plugins(cyrus_script::script::ScriptPlugin)
        .insert_resource(cyrus_script::script::DeathDispatch {
            last_save: Some(cyrus_script::script::LastSaveSelection {
                slot: 0,
                description: "saved state".into(),
            }),
            book_context: Some(3),
        })
        .init_resource::<bevy::prelude::NextState<cyrus_game::AppState>>()
        .insert_resource(cyrus_script::clock::SimulationClock::new(scripts.clock).unwrap());
    live.world_mut()
        .resource_mut::<cyrus_script::script::ScriptActors>()
        .current = Some(world);
    live.world_mut().run_schedule(bevy::prelude::Last);
    let entry = live
        .world()
        .resource::<cyrus_script::script::LastGameEntry>();
    assert_eq!(entry.retained_book_context, 3);
    assert_eq!(entry.save.slot, 0);
    assert_eq!(entry.save.description, "saved state");
    assert!(
        !live
            .world()
            .contains_resource::<cyrus_script::script::DeathBookEntry>()
    );
    assert!(
        !live
            .world()
            .contains_resource::<cyrus_world::lifecycle::SceneRequest>()
    );
    assert!(matches!(
        live.world()
            .resource::<bevy::prelude::NextState<cyrus_game::AppState>>(),
        bevy::prelude::NextState::Pending(cyrus_game::AppState::Menu)
    ));
}
