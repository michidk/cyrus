use super::*;

#[test]
fn player_hanging_is_false_outside_the_hanging_state() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::PlayerHanging,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            assign_call(1, id, Native::PlayerHanging, &[]).operation,
            set_flag(7),
            Operation::End { target: 0 },
        ]
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    });
    assert!(
        world
            .actors()
            .iter()
            .any(|actor| matches!(actor.source, ScriptActorSource::Player))
    );
    let map = world.id(0).unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    let inputs = ScriptInputs {
        hud: Some(&hud),
        shell_execution: Some(false),
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            map,
            ScriptStage::Body(cyrus_soup::Thread::Main),
            inputs,
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        world.flags(),
        [7],
        "the call completed and the script resumed"
    );
}

#[test]
fn player_on_scape_reads_the_selected_players_ground_response_and_needs_a_player() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // A player that has not run a ground update has no response record: not on terrain.
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::PlayerOnScape,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            assign_call(1, id, Native::PlayerOnScape, &[]).operation,
            Operation::End { target: 0 },
        ]
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    });
    let map = world.id(0).unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    world
        .advance(
            map,
            ScriptStage::Body(cyrus_soup::Thread::Main),
            ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        world.actors()[map.index].runtime().unwrap().variables()[0]
            .integer()
            .unwrap(),
        0
    );
    // Without a selected player the query fails with its name, and it takes no arguments.
    let error = run_native_calls(&[(Native::PlayerOnScape, &[])]).unwrap_err();
    assert!(format!("{error:#}").contains("PlayerOnScape"), "{error:#}");
    let error = run_native_calls(&[(Native::PlayerOnScape, &[1])]).unwrap_err();
    assert!(format!("{error:#}").contains("PlayerOnScape"), "{error:#}");
}

#[test]
fn player_status_classifies_the_selected_player_through_the_public_native() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::PlayerStatus,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            assign_call(1, id, Native::PlayerStatus, &[]).operation,
            Operation::End { target: 0 },
        ]
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    });
    let map = world.id(0).unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    world
        .advance(
            map,
            ScriptStage::Body(cyrus_soup::Thread::Main),
            ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )
        .unwrap();
    let status = world.actors()[map.index].runtime().unwrap().variables()[0]
        .integer()
        .unwrap();
    // A player at rest (no queued, falling or sliding motion, living state zero, no movement
    // since the update entry) matches no rule: class zero.
    assert_eq!(status, 0);
    // Without a selected player the query fails with its name, and it takes no arguments.
    let error = run_native_calls(&[(Native::PlayerStatus, &[])]).unwrap_err();
    assert!(format!("{error:#}").contains("PlayerStatus"), "{error:#}");
    let error = run_native_calls(&[(Native::PlayerStatus, &[1])]).unwrap_err();
    assert!(format!("{error:#}").contains("PlayerStatus"), "{error:#}");
}

#[test]
fn update_player_view_attrib_is_an_empty_call_and_reset_position_keeps_the_home_position() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // updatePlayerViewAttrib is an empty routine in the original: it completes and changes nothing.
    // ResetPosition on an actor already at home leaves it there.
    for (function, arguments, param_count) in [
        (Native::UpdatePlayerViewAttrib, vec![], 0),
        (
            Native::ResetPosition,
            vec![
                Value::Integer { value: 1 },
                Value::Integer { value: 1 },
                Value::Integer { value: 1 },
            ],
            3,
        ),
    ] {
        let mut scripts = random_scripts(function, arguments, Object::Me);
        scripts.definition.functions[1].param_count = param_count;
        if function == Native::UpdatePlayerViewAttrib {
            // Installed scripts call it as a statement: the routine has no numeric result.
            let step = scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .program
                .steps
                .get_mut(&0)
                .unwrap();
            let Operation::Assign { formula, .. } = &step.operation else {
                unreachable!()
            };
            let Value::Call { call } = &formula.first else {
                unreachable!()
            };
            step.operation = Operation::Call {
                call: (**call).clone(),
            };
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let before = world.spatial(id).unwrap().local.position;
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        assert!(world.actors()[0].failure().is_none(), "{function}");
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            before,
            "{function}"
        );
    }
}

#[test]
fn move_player_above_me_names_the_missing_input_instead_of_guessing_a_placement() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // The placement needs the selected player and the installed standing height, which the ground
    // service supplies; a world without them stops with an error that names the native.
    let mut scripts = random_scripts(
        Native::MovePlayerAboveMe,
        [0, 0, 0, 512]
            .map(|value| Value::Integer { value })
            .to_vec(),
        Object::Me,
    );
    scripts.definition.functions[1].param_count = 4;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let error = world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains(Native::MovePlayerAboveMe.name()),
        "{error:#}"
    );
}

#[test]
fn standalone_player_spatial_initialization_is_generation_bound_and_once_only() {
    let mut scripts = cache();
    add_standalone_receivers(&mut scripts);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    let alternate = world.id(3).unwrap();
    let camera = world.id(4).unwrap();
    assert!(world.spatial(player).is_err());
    assert!(
        world
            .initialize_player_spatial(camera, [1, 2, 3], [0; 3])
            .is_err()
    );
    assert!(
        world
            .initialize_player_spatial(player, [1, 2, 3], [0, 2048, 0])
            .is_err()
    );
    assert!(world.spatial(player).is_err());
    world
        .initialize_player_spatial(player, [101, -202, 303], [0, 668, 0])
        .unwrap();
    world
        .initialize_player_spatial(alternate, [404, -505, 606], [0, 1185, 0])
        .unwrap();
    let state = world.spatial(player).unwrap();
    assert_eq!(state.query_pose().unwrap().position, [101, -202, 303]);
    assert_eq!(state.local.angles, [0, 668, 0]);
    assert_eq!(state.previous_local_position, [101, -202, 303]);
    assert_eq!(state.angle_history, [0, 668, 0]);
    assert_eq!(state.water_rollback_position, None);
    assert!(
        world
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .is_err()
    );
    world.refresh_families().unwrap();
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [101, -202, 303]
    );
    assert_eq!(
        world.spatial(alternate).unwrap().local.position,
        [404, -505, 606]
    );
    let mut replacement = ScriptWorld::prepare(&scripts, 8).unwrap();
    replacement.prepare_spatial().unwrap();
    assert!(
        replacement
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .is_err()
    );
}

#[test]
fn restored_alternate_entity_survives_ordinary_binding_updates() {
    use bevy::prelude::*;
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptActorSource, ScriptActors, ScriptBinding, ScriptPlugin};
    use cyrus_world::{animation::AnimationRequest, lifecycle::PreparedWorld};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .add_message::<cyrus_world::animation::AnimationTick>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .insert_resource(State::new(cyrus_game::AppState::InGame))
        .add_plugins(ScriptPlugin);
    let mut data = cache();
    add_standalone_receivers(&mut data);
    let animations = app.world_mut().resource_mut::<Assets<Animations>>().add(Animations {
        version: 2, world: data.world,
        metadata: serde_json::json!({"actors": [], "markers": [], "walk_node_maps": [], "script_content_id": data.content_id}),
        sections: BTreeMap::new(), models: BTreeMap::new(), unsupported: BTreeMap::new(),
    });
    let world_id = data.world;
    let scripts = app.world_mut().resource_mut::<Assets<Scripts>>().add(data);
    app.insert_resource(PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(world_id),
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    })
    .insert_resource(AnimationRequest(animations.clone()));
    let player = app
        .world_mut()
        .spawn((cyrus_script::player::Player, Transform::default()))
        .id();
    app.update();
    app.update();
    let current = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap();
    let index = current
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::AlternatePlayer))
        .unwrap();
    let actor = current.id(index).unwrap();
    let candidate = current.prepare_entity_binding(actor, animations).unwrap();
    let old: Vec<_> = app
        .world_mut()
        .query::<(Entity, &ScriptBinding)>()
        .iter(app.world())
        .filter_map(|(entity, binding)| (binding.actor == actor).then_some(entity))
        .collect();
    assert_eq!(old.len(), 1);
    app.world_mut().despawn(old[0]);
    let replacement = app.world_mut().spawn(Transform::default()).id();
    let mut queue = bevy::ecs::world::CommandQueue::default();
    candidate.queue(
        &mut Commands::new(&mut queue, app.world()),
        replacement,
        player,
    );
    queue.apply(app.world_mut());
    app.update();
    app.update();
    let retained: Vec<_> = app
        .world_mut()
        .query::<(Entity, &ScriptBinding)>()
        .iter(app.world())
        .filter_map(|(entity, binding)| (binding.actor == actor).then_some(entity))
        .collect();
    assert_eq!(retained, [replacement]);
    assert_eq!(
        app.world().get::<ChildOf>(replacement).unwrap().parent(),
        player
    );
}

#[test]
fn player_moved_needs_a_selected_player_with_spatial_state() {
    let error = run_native_world_with(&[(Native::PlayerMoved, &[])], true)
        .map(|_| ())
        .unwrap_err();
    assert!(format!("{error:#}").contains("selected player unavailable"));
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::PlayerMoved,
        param_count: 0,
        kind: CallKind::Function,
    });
    let mut actor = scripts.actors.remove("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.namespace = Some("cyrus".into());
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: Native::PlayerMoved,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.actors.insert("CYRUS".into(), actor);
    scripts.instances.clear();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(0).unwrap();
    let error = world
        .advance(
            player,
            cyrus_script::script::ScriptStage::OrdinaryMain,
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(3),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("spatial state unavailable"));
}

/// Fixture whose `LOGIC` script calls the given natives on every ordinary update.
fn player_swap_fixture(
    calls: &[(Native, &[i32])],
) -> (Scripts, ScriptWorld, cyrus_assets::animation::Animations) {
    let (scripts, world, data, _) = player_walking_fixture_with(|scripts| {
        let mut steps = BTreeMap::new();
        for (index, (name, arguments)) in calls.iter().enumerate() {
            scripts.definition.functions.push(Function {
                name: *name,
                param_count: arguments.len() as u8,
                kind: CallKind::Function,
            });
            let id = scripts.definition.functions.len() as u16 - 1;
            steps.insert(
                index as u32,
                assign_call(index as u32 + 1, id, *name, arguments),
            );
        }
        steps.insert(
            calls.len() as u32,
            Step {
                next: calls.len() as u32 + 1,
                operation: Operation::End { target: 0 },
            },
        );
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    });
    (scripts, world, data)
}

fn update_player_swap_world(world: &mut ScriptWorld, data: &cyrus_assets::animation::Animations) {
    world
        .update_ordinary(
            data,
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
}

#[test]
fn push_and_pop_player_exchange_the_roles_at_the_end_of_a_continuing_iteration() {
    use cyrus_script::script::{ScriptActorSource, WorldLoopOutcome};
    let (_, mut world, mut data) = player_swap_fixture(&[(Native::PushPlayer, &[1])]);
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    update_player_swap_world(&mut world, &data);
    let role = |world: &ScriptWorld, source: ScriptActorSource| {
        world
            .actors()
            .iter()
            .position(|actor| {
                std::mem::discriminant(&actor.source) == std::mem::discriminant(&source)
            })
            .unwrap()
    };
    let cyrus = role(&world, ScriptActorSource::Player);
    let gremlin = role(&world, ScriptActorSource::AlternatePlayer);
    assert!(!world.globals().player_swapped());
    let placement = world.spatial(world.id(cyrus).unwrap()).unwrap().local;
    assert_eq!(
        world.finish_world_loop(1).unwrap(),
        WorldLoopOutcome::Continue
    );
    assert!(world.globals().player_swapped());
    assert_eq!(world.selected_player(), Some(world.id(gremlin).unwrap()));
    let moved = world.spatial(world.id(gremlin).unwrap()).unwrap().local;
    assert_eq!(moved.position, placement.position);
    assert_eq!(moved.angles, placement.angles);
    assert_eq!(
        world
            .spatial(world.id(gremlin).unwrap())
            .unwrap()
            .initial_angles,
        placement.angles
    );
    assert_eq!(
        world.globals_mut().take_host_requests(),
        cyrus_script::script::HostRequests::default()
    );
}

#[test]
fn pop_player_acts_only_while_swapped_and_returns_to_cyrus_in_the_same_iteration_as_push() {
    use cyrus_script::script::WorldLoopOutcome;
    type Calls<'a> = &'a [(Native, &'a [i32])];
    let cases: [(Calls, bool); 3] = [
        (&[(Native::PopPlayer, &[1])], false),
        (
            &[(Native::PushPlayer, &[1]), (Native::PopPlayer, &[1])],
            false,
        ),
        (
            &[(Native::PushPlayer, &[1]), (Native::PushPlayer, &[1])],
            true,
        ),
    ];
    for (calls, swapped) in cases {
        let (_, mut world, mut data) = player_swap_fixture(calls);
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        update_player_swap_world(&mut world, &data);
        let gremlin = world
            .actors()
            .iter()
            .position(|actor| {
                matches!(
                    actor.source,
                    cyrus_script::script::ScriptActorSource::AlternatePlayer
                )
            })
            .unwrap();
        world
            .initialize_player_spatial(world.id(gremlin).unwrap(), [404, -505, 606], [0, 1185, 0])
            .unwrap();
        assert_eq!(
            world.finish_world_loop(1).unwrap(),
            WorldLoopOutcome::Continue
        );
        assert_eq!(world.globals().player_swapped(), swapped, "{calls:?}");
        assert_eq!(
            world.globals_mut().take_host_requests(),
            cyrus_script::script::HostRequests::default()
        );
    }
}
