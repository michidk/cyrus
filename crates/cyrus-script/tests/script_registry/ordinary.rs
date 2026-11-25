use super::*;

fn ordered_world() -> ScriptWorld {
    let mut data = cache();
    let placements = [
        (1, 1, 0, 0),
        (1, 1, 0, 0),
        (2, 1, 0, 0),
        (1, 2, 0, 0),
        (0, 1, 0, 0),
        (0, 1, 0, 0),
        (0, 0, 0, 0),
        (120, 120, 1, 0),
        (120, 120, 1, 0),
        (1, 1, 0, 2),
        (20, 20, 0, 0),
        (1, 0, 0, 0),
        (1, 17, 0, 0),
        (17, 17, 0, 0),
    ];
    let template = data.instances[0].clone();
    data.instances = placements
        .into_iter()
        .enumerate()
        .map(|(index, (x, z, active, object_type))| Instance {
            object_id: index as u32,
            actor: if index == 5 {
                "BORDER".into()
            } else {
                "LOGIC".into()
            },
            map_position: [x * 512, 0, z * 512],
            active,
            object_type: PlacementType::from_byte(object_type),
            ..template.clone()
        })
        .collect();
    data.definition.functions.push(Function {
        name: Native::SetMyAttr,
        param_count: 2,
        kind: CallKind::Function,
    });
    let actor = data.actors.get_mut("LOGIC").unwrap();
    actor.instances = 14;
    actor.instance_variables = vec![vec![4]; 14];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Call {
                    call: Call {
                        function_id: 1,
                        kind: CallKind::Function,
                        target: Object::Me,
                        function: Native::SetMyAttr,
                        arguments: vec![Value::Integer { value: 113 }, Value::Integer { value: 0 }],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    let mut role = actor.clone();
    role.instances = 1;
    role.instance_variables = vec![vec![4]];
    for name in ["CYRUS", "GREMLIN", "CAMERA", "BORDER"] {
        let mut actor = role.clone();
        actor.namespace = Some(name.to_ascii_lowercase());
        if name == "BORDER" {
            actor.attributes[Attribute::ShortMapping] = 1;
        }
        data.actors.insert(name.into(), actor);
    }
    data.content_id = data.content_identity().unwrap();
    ScriptWorld::prepare(&data, 19).unwrap()
}

#[test]
fn ordinary_actor_lists_preserve_cells_borders_and_persistent_creation_order() {
    let mut world = ordered_world();
    let ids = world.ordinary_actors([4608, 4608]).unwrap();
    assert_eq!(
        ids.iter().map(|id| id.index).collect::<Vec<_>>(),
        [11, 4, 1, 0, 2, 3, 12, 16, 15, 14, 8, 7]
    );
    assert!(ids.iter().all(|id| id.generation == 19));
    let lower = world.ordinary_actors([0, 0]).unwrap().to_vec();
    assert_eq!(world.ordinary_actors([i32::MIN, -1]).unwrap(), lower);
    let upper = world.ordinary_actors([65535, 65535]).unwrap().to_vec();
    assert_eq!(world.ordinary_actors([65536, i32::MAX]).unwrap(), upper);
    assert_eq!(
        world
            .ordinary_actors([65535, 65535])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [16, 15, 14, 8, 7]
    );
}

#[test]
fn ordinary_border_selection_changes_only_when_the_view_starting_cell_changes() {
    let mut world = ordered_world();
    assert!(
        !world
            .ordinary_actors([4608, 4608])
            .unwrap()
            .iter()
            .any(|id| id.index == 5)
    );
    world.step(world.id(5).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[5].attributes()[Attribute::ShortMapping], 0);
    assert!(
        !world
            .ordinary_actors([5119, 4608])
            .unwrap()
            .iter()
            .any(|id| id.index == 5)
    );
    world.ordinary_actors([5120, 4608]).unwrap();
    let ids = world.ordinary_actors([4608, 4608]).unwrap();
    assert_eq!(
        ids.iter().map(|id| id.index).collect::<Vec<_>>(),
        [11, 5, 4, 1, 0, 2, 3, 12, 16, 15, 14, 8, 7]
    );
}

#[test]
fn ordinary_actor_window_uses_the_original_upper_edge_origin() {
    let mut data = cache();
    data.instances[0].map_position = [109 * 512, 0, 110 * 512];
    data.instances[1].map_position = [126 * 512, 0, 126 * 512];
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    assert_eq!(
        world
            .ordinary_actors([65535, 65535])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [0]
    );
}

#[test]
fn native_hook_updates_only_its_receiver_without_advancing_the_script() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::SetMyAttr,
        param_count: 2,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
        function_id: 1,
        function: Native::SetMyAttr,
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![Value::Integer { value: 6 }, Value::Integer { value: 19 }],
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::NativeHook,
            ScriptInputs::default(),
            None,
            None,
            0,
        )
        .unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Weight], 19);
    assert_eq!(world.actors()[1].attributes()[Attribute::Weight], 0);
    assert_eq!(
        world.actors()[0]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
}

#[test]
fn disabling_a_script_finishes_its_invocation_and_freezes_later_main_task_passes() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    for (name, param_count, kind) in [
        (Native::DisableScript, 0, CallKind::Function),
        (Native::EnableScript, 0, CallKind::Function),
        (Native::Wait, 1, CallKind::Task),
    ] {
        scripts.definition.functions.push(Function {
            name,
            param_count,
            kind,
        });
    }
    let control_call = |enabled| Call {
        function_id: if enabled { 2 } else { 1 },
        function: if enabled {
            Native::EnableScript
        } else {
            Native::DisableScript
        },
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![],
    };
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.native_hook = Some(control_call(true));
    actor.program.steps = [
        Operation::Call {
            call: Call {
                function_id: 3,
                function: Native::Wait,
                kind: CallKind::Multitask,
                target: Object::Me,
                arguments: vec![Value::Integer { value: 100 }],
            },
        },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(control_call(false)),
                },
                rest: vec![],
            },
        },
        Operation::Increment {
            destination: Place::Flag { index: 0 },
            delta: 1,
        },
        Operation::End { target: 2 },
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
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 31).unwrap();
    let id = world.id(0).unwrap();
    let inputs = ScriptInputs {
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.flags(), [13]);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .script_control
            .bits(),
        1
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .script_control
            .bits(),
        0
    );
    for _ in 0..200 {
        world
            .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
            .unwrap();
    }
    assert_eq!(world.flags(), [13]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .advance(id, ScriptStage::NativeHook, inputs, None, None, 20)
        .unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .script_control
            .bits(),
        0
    );
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.flags(), [14]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
}

#[test]
fn ordinary_hook_observes_unpaused_multitasks_and_installed_task_switch() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for task_system in [false, true] {
        for paused in [false, true] {
            let mut operations = vec![wait(
                1,
                Native::Wait,
                CallKind::Multitask,
                vec![Value::Integer { value: 100 }],
            )];
            if paused {
                operations.push(Operation::TaskPause { label: 0 });
            }
            operations.push(Operation::End { target: 0 });
            let mut scripts = wait_cache(operations);
            scripts.updates.task_system = task_system;
            scripts.definition.functions.push(Function {
                name: Native::SetMyAttr,
                param_count: 2,
                kind: CallKind::Function,
            });
            scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
                function_id: 4,
                function: Native::SetMyAttr,
                kind: CallKind::Function,
                target: Object::Me,
                arguments: vec![Value::Integer { value: 6 }, Value::Integer { value: 19 }],
            });
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
            let id = world.id(0).unwrap();
            let inputs = ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            };
            world
                .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
            assert!(!world.actors()[0].runtime().unwrap().has_blocking_task());
            world
                .advance(id, ScriptStage::OrdinaryHook, inputs, None, None, 20)
                .unwrap();
            assert_eq!(
                world.actors()[0].attributes()[Attribute::Weight],
                if !task_system || paused { 19 } else { 0 }
            );
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        }
    }
}

#[test]
fn disabled_task_system_preserves_main_continuation_and_task_state() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = wait_cache(vec![
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 3 }],
        ),
        set_flag(99),
        Operation::End { target: 0 },
    ]);
    scripts.updates.task_system = false;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let inputs = ScriptInputs {
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(
        world.actors()[0]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    for _ in 0..10 {
        world
            .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
            .unwrap();
    }
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .advance(id, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .advance(id, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn deferred_movement_relocates_cells_and_retains_nearby_cached_order() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (excluded, distant_view) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut scripts = cache();
        scripts.instances[0].map_position = [1024, 0, 512];
        scripts.instances[1].map_position = [512, 0, 512];
        scripts.definition.functions.push(Function {
            name: Native::MoveObjectAxis,
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            1,
            Native::MoveObjectAxis,
            CallKind::Task,
            vec![
                Value::Integer { value: 0 },
                Value::Integer { value: -262144 },
                Value::Integer { value: 2 },
            ],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let original = world.ordinary_actors([0, 0]).unwrap().to_vec();
        assert_eq!(
            original.iter().map(|id| id.index).collect::<Vec<_>>(),
            [1, 0]
        );
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [131072, 0, 131072]
        );
        let view = if distant_view {
            [6000 * 256, 0, 0]
        } else {
            [0; 3]
        };
        world.flush_map_updates(view, excluded).unwrap();
        let cached: Vec<_> = world
            .ordinary_actors([0, 0])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect();
        assert_eq!(
            cached,
            if distant_view && !excluded {
                vec![0, 1]
            } else {
                vec![1, 0]
            }
        );
        // The excluded pass consumes its registration; a later admitted pass cannot replay it.
        world.flush_map_updates([0; 3], false).unwrap();
        world.ordinary_actors([6000, 6000]).unwrap();
        let rebuilt: Vec<_> = world
            .ordinary_actors([0, 0])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect();
        assert_eq!(rebuilt, if excluded { vec![1, 0] } else { vec![0, 1] });
    }
}

#[test]
fn ordinary_ground_retains_live_owner_excludes_self_and_has_no_player_fallback() {
    use cyrus_collision::ground::SupportSource;
    use cyrus_script::script::{
        ModelSupportOwners, OrdinaryGroundQuery, ScriptInputs, ScriptStage,
    };
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveByAxis,
        param_count: 3,
        kind: CallKind::Task,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    1,
                    Native::MoveByAxis,
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 1 },
                        Value::Integer { value: 256 },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    let (mut world, data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let support_actor = world.id(1).unwrap();
    let order = [id, support_actor];
    world.refresh_model_presentations(&data, &order).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &order)
        .unwrap();
    let other_prepared = world
        .prepare_ordinary_collision_query(support_actor, &scene, &data, [10, 30], &order)
        .unwrap();
    let query = |prepared| OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared,
        distance: 50,
        sidedness: false,
    };
    let support = world
        .query_ordinary_ground(id, query(&prepared))
        .unwrap()
        .unwrap();
    assert_eq!(support.source(), SupportSource::Actor { actor: 1, face: 0 });
    assert_eq!(support.actor_height(0, -2), 18 * 256);
    assert_eq!(
        world.model_support(id).unwrap(),
        ModelSupportOwners {
            actor: Some(support_actor),
            static_placement: None,
        }
    );
    assert_eq!(world.actor_surface(id).unwrap(), Surface::UNKNOWN);
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            support_actor,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    world
        .advance(support_actor, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(
        world.spatial(support_actor).unwrap().local.position[1],
        21 * 256
    );
    assert_eq!(
        world
            .query_ordinary_ground(id, query(&prepared))
            .unwrap()
            .unwrap()
            .actor_height(0, -2),
        18 * 256
    );
    world
        .refresh_model_presentations(&data, &[support_actor])
        .unwrap();
    assert_eq!(
        world
            .query_ordinary_ground(id, query(&prepared))
            .unwrap()
            .unwrap()
            .actor_height(0, -2),
        19 * 256
    );
    assert!(
        world
            .query_ordinary_ground(id, query(&other_prepared))
            .is_err()
    );
    let self_prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
        .unwrap();
    let empty_prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
        .unwrap();
    let body_query = |prepared| cyrus_script::script::OrdinaryBodyQuery {
        scene: &scene,
        animations: &data,
        prepared,
        sidedness: false,
        excluded_candidate_type: None,
        terrain: None,
    };
    let contacts = world
        .query_ordinary_body(id, body_query(&prepared))
        .unwrap();
    assert!(!contacts.is_empty());
    let residual = world
        .query_ordinary_residual_body(
            id,
            cyrus_script::script::OrdinaryResidualQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                sidedness: false,
            },
        )
        .unwrap();
    assert!(!residual.is_empty());
    // With the engine's sidedness flag enabled, this fixture's face is seen from behind and rejected.
    assert!(
        world
            .query_ordinary_residual_body(
                id,
                cyrus_script::script::OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: true,
                },
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        residual
            .iter()
            .all(|contact| contact.source == SupportSource::Actor { actor: 1, face: 0 })
    );
    assert!(
        contacts
            .iter()
            .all(|contact| contact.source == SupportSource::Actor { actor: 1, face: 0 })
    );
    assert!(
        world
            .query_ordinary_body(id, body_query(&self_prepared))
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.model_support(id).unwrap().actor, Some(support_actor));
    assert!(
        world
            .query_ordinary_body(
                cyrus_script::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                body_query(&prepared)
            )
            .is_err()
    );
    // Only the queried actor remains. Its current model must not support itself.
    assert!(
        world
            .query_ordinary_ground(id, query(&self_prepared))
            .unwrap()
            .is_none()
    );
    // Query misses do not clear owners; the admitted update boundary does.
    assert_eq!(world.model_support(id).unwrap().actor, Some(support_actor));
    world.set_model_support(id, Default::default()).unwrap();
    assert!(
        world
            .query_ordinary_ground(id, query(&empty_prepared))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        world.model_support(id).unwrap(),
        ModelSupportOwners::default()
    );
    assert!(
        world
            .query_ordinary_ground(
                cyrus_script::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                query(&prepared)
            )
            .is_err()
    );
}

#[test]
fn ordinary_collision_admission_is_retained_across_script_movement() {
    use cyrus_script::script::{OrdinaryGroundQuery, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveByAxis,
        param_count: 3,
        kind: CallKind::Task,
    });
    let mut steps = BTreeMap::new();
    for (offset, delta, target) in [(0, 256000, 2), (2, -256000, 0)] {
        steps.insert(
            offset,
            Step {
                next: offset + 1,
                operation: wait(
                    1,
                    Native::MoveByAxis,
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer { value: delta },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        );
        steps.insert(
            offset + 1,
            Step {
                next: offset + 2,
                operation: Operation::End { target },
            },
        );
    }
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    let (mut world, data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            other,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.spatial(other).unwrap().local.position[0], 1010 * 256);
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let actors = [id, other];
    let retained = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &actors)
        .unwrap();
    world
        .advance(
            other,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.spatial(other).unwrap().local.position[0], 10 * 256);
    world.refresh_model_presentations(&data, &[other]).unwrap();
    let fresh = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &actors)
        .unwrap();
    let query = |prepared| OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared,
        distance: 50,
        sidedness: false,
    };
    assert!(
        world
            .query_ordinary_ground(id, query(&retained))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        world
            .query_ordinary_ground(id, query(&fresh))
            .unwrap()
            .unwrap()
            .source(),
        cyrus_collision::ground::SupportSource::Actor { actor: 1, face: 0 }
    );
}

#[test]
fn ordinary_collision_preparation_precedes_script_and_admitted_late_response() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for (available, terrain) in [(true, false), (true, true), (false, false)] {
        let mut scripts = cache();
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .extend([(Attribute::Collide, 1), (Attribute::FallDamage, 1)]);
        for placement in &mut scripts.instances {
            placement.active = 1;
        }
        let (mut world, data, models) = static_model_fixture_at(scripts, None, [600, 20, 600]);
        let mut scene = ordinary_collision_scene(world.world, models);
        if terrain {
            scene.terrain = Some(
                cyrus_collision::ground::TerrainGround::new(
                    vec![20; 256 * 256],
                    vec![false; 256 * 256],
                    None,
                )
                .unwrap(),
            );
        }
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(1),
                        ..Default::default()
                    },
                    scene: available.then_some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [if available { 5 } else { 4 }],
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables(),
            [if available { 10 } else { 9 }]
        );
        for actor in world.actors().iter().take(2) {
            if available {
                assert!(actor.failure().is_none());
            } else {
                assert!(
                    actor
                        .failure()
                        .unwrap()
                        .contains("collision scene unavailable")
                );
            }
        }
    }
}

#[test]
fn ordinary_combat_respects_scheduler_shell_without_activation_inputs() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for shell in [true, false] {
        let mut scripts = cache();
        scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
            (Attribute::Collide, 1),
            (Attribute::Combat, 1),
            (Attribute::ReservedCollide, 7),
        ]);
        for placement in &mut scripts.instances {
            placement.active = 1;
        }
        let (mut world, mut data, models) = static_model_fixture_at(scripts, None, [600, 20, 600]);
        for actor in data.metadata["actors"].as_array_mut().unwrap() {
            actor["model_load_path"] = serde_json::json!("animation_table");
            actor["animation_groups"] = serde_json::json!([
                {"group_index":0,"anim_id":1,"anim_type":0,
                 "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}
            ]);
        }
        let scene = ordinary_collision_scene(world.world, models);
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world
            .send_combat_message(sender, receiver, CombatMessageCode::HIT)
            .unwrap();
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        shell_execution: Some(shell),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        if shell {
            assert!(
                world.actors()[0].failure().is_none(),
                "{:?}",
                world.actors()[0].failure()
            );
            assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 1);
            assert_eq!(
                world
                    .consume_combat_message(receiver)
                    .unwrap()
                    .unwrap()
                    .code,
                CombatMessageCode::HIT
            );
        } else {
            assert_eq!(
                world.actors()[0].attributes()[Attribute::Collide],
                7,
                "{:?}",
                world.actors()[0].failure()
            );
            assert!(
                world.actors()[0]
                    .failure()
                    .unwrap()
                    .contains("negative target attack slot")
            );
            assert!(world.consume_combat_message(receiver).unwrap().is_none());
        }
    }
}

#[test]
fn a_falling_actor_hitting_a_wall_keeps_falling_without_turning() {
    use cyrus_collision::{collision_world::RuntimeBodyContact, ground::SupportSource};
    use cyrus_script::{
        player::data::MovementSettings,
        script::{ScriptInputs, ScriptStage},
    };
    for (slide_range, expected) in [(-60000, [0, 0, 2019]), (0, [0; 3])] {
        let mut scripts = cache();
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.angles = [0, 344, 0];
        }
        scripts.definition.functions.push(Function {
            name: Native::MoveByAxis,
            param_count: 3,
            kind: CallKind::Task,
        });
        let mut steps = BTreeMap::new();
        let falling = scripts.definition.references.len() as u16;
        scripts.definition.references.push(Property::FallFlag);
        for (offset, axis, delta) in [(0, 0, -3341), (1, 2, 1893)] {
            steps.insert(
                offset,
                Step {
                    next: offset + 1,
                    operation: wait(
                        1,
                        Native::MoveByAxis,
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: axis },
                            Value::Integer { value: delta },
                            Value::Integer { value: 1 },
                        ],
                    ),
                },
            );
        }
        steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: falling,
                    },
                    formula: Formula {
                        first: Value::Integer { value: 0x81 },
                        rest: vec![],
                    },
                },
            },
        );
        steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::End { target: 0 },
            },
        );
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.sliding_state,
                control.falling_state,
                control.queued_motion,
                control.last_collide.bits()
            ),
            (0, JumpMotion::NONE, JumpMotion::NONE, 0)
        );
        world.prepare_spatial().unwrap();
        world.snapshot_spatial(id).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().falling_state,
            JumpMotion::from_byte(0x81)
        );
        let contacts = [RuntimeBodyContact {
            source: SupportSource::Model { model: 0, face: 0 },
            point: [0; 3],
            normal: [256, 0, 0],
            classification: SurfaceClass::Wall,
        }];
        let settings = MovementSettings {
            standing_height: -2,
            post_collide_height: 50,
            slide_range,
            slide_speed: None,
            slide_hurt: None,
            tap_time: None,
            sheath_sword_delay: None,
            defend_interval: None,
            swim_depth: None,
            max_remap_objects: None,
            turn_speed: 12,
            turn_max_speed: 48,
            poly_push_units: 16,
            joystick: None,
        };
        let mut transitions = 0;
        world
            .resolve_ordinary_primary_contacts(id, &contacts, &settings, &mut |_| {
                transitions += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(transitions, 1);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(world.spatial(id).unwrap().local.angles[1], 344);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            2
        );
    }
}

#[test]
fn ordinary_primary_response_owns_grounded_pose_heading_and_marks() {
    use cyrus_collision::{collision_world::RuntimeBodyContact, ground::SupportSource};
    use cyrus_script::{
        player::data::MovementSettings,
        script::{ScriptInputs, ScriptStage},
    };
    for (normals, slide_range, expected, heading, flags, sliding) in [
        (
            vec![([256, 0, 0], SurfaceClass::Wall)],
            -60000,
            [0, 0, 2019],
            Some(313),
            1,
            0,
        ),
        (
            vec![
                ([256, 0, 0], SurfaceClass::Wall),
                ([0, 0, -256], SurfaceClass::Wall),
            ],
            -60000,
            [-3564, 0, 0],
            None,
            1,
            0,
        ),
        (
            vec![([256, 0, 0], SurfaceClass::Wall)],
            0,
            [0; 3],
            Some(344),
            2,
            0,
        ),
        (
            vec![([256, 0, 0], SurfaceClass::Floor)],
            -60000,
            [-3341, 0, 1893],
            Some(344),
            0,
            0,
        ),
        (
            vec![([256, 0, 0], SurfaceClass::Wall)],
            -60000,
            [0, 0, 2019],
            Some(344),
            2,
            1,
        ),
        (
            vec![([256, 0, 0], SurfaceClass::Wall)],
            0,
            [-3341, 0, 1893],
            Some(344),
            0,
            1,
        ),
    ] {
        let mut scripts = cache();
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.angles = [0, 344, 0];
        }
        scripts.definition.functions.push(Function {
            name: Native::MoveByAxis,
            param_count: 3,
            kind: CallKind::Task,
        });
        let mut steps = BTreeMap::new();
        for (offset, axis, delta) in [(0, 0, -3341), (1, 2, 1893)] {
            steps.insert(
                offset,
                Step {
                    next: offset + 1,
                    operation: wait(
                        1,
                        Native::MoveByAxis,
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: axis },
                            Value::Integer { value: delta },
                            Value::Integer { value: 1 },
                        ],
                    ),
                },
            );
        }
        let slide_flag = scripts.definition.references.len() as u16;
        scripts.definition.references.push(Property::SlideFlag);
        steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: slide_flag,
                    },
                    formula: Formula {
                        first: Value::Integer { value: sliding },
                        rest: vec![],
                    },
                },
            },
        );
        steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::End { target: 0 },
            },
        );
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.sliding_state,
                control.falling_state,
                control.queued_motion,
                control.last_collide.bits()
            ),
            (0, JumpMotion::NONE, JumpMotion::NONE, 0)
        );
        world.prepare_spatial().unwrap();
        world.snapshot_spatial(id).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, [-3341, 0, 1893]);
        let contacts = normals
            .into_iter()
            .map(|(normal, classification)| RuntimeBodyContact {
                source: SupportSource::Model { model: 0, face: 0 },
                point: [0; 3],
                normal,
                classification,
            })
            .collect::<Vec<_>>();
        let settings = MovementSettings {
            standing_height: -2,
            post_collide_height: 50,
            slide_range,
            slide_speed: None,
            slide_hurt: None,
            tap_time: None,
            sheath_sword_delay: None,
            defend_interval: None,
            swim_depth: None,
            max_remap_objects: None,
            turn_speed: 12,
            turn_max_speed: 48,
            poly_push_units: 16,
            joystick: None,
        };
        world
            .resolve_ordinary_primary_contacts(id, &contacts, &settings, &mut |_| {
                unreachable!("grounded contacts run no motion transition")
            })
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        if let Some(heading) = heading {
            assert_eq!(world.spatial(id).unwrap().local.angles[1], heading);
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            flags
        );
        assert_eq!(world.actor_surface(id).unwrap(), Surface::UNKNOWN);
        let residue = |normal| RuntimeBodyContact {
            source: SupportSource::Model { model: 0, face: 0 },
            point: [0; 3],
            normal,
            classification: SurfaceClass::Wall,
        };
        world
            .resolve_ordinary_residual_contacts(
                id,
                &[residue([256, 0, 0]), residue([-256, 0, 0])],
                &settings,
            )
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            flags | 4
        );
        world
            .resolve_ordinary_residual_contacts(id, &[residue([256, 0, 0])], &settings)
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [expected[0] + 4096, expected[1], expected[2]]
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            flags | 4
        );
    }
}

#[test]
fn ordinary_collision_loader_distinguishes_authored_static_and_model_less_shapes() {
    use cyrus_collision::{actor_shape::BodySphere, collision_world::StaticCollision};
    use cyrus_script::player::data::{MovementSettings, PlayerScene};
    for (path, primary, selector, expected_y, generated) in [
        ("static", -1, -1, 0, true),
        ("static", 0, 1, 62, true),
        ("animation_table", 0, -1, 62, true),
        ("animation_table", -1, -1, 12, false),
        ("none", 0, -1, 12, false),
    ] {
        let (mut world, mut data, models) = static_model_fixture(cache());
        let id = world.id(0).unwrap();
        data.metadata["actors"][0]["model_load_path"] = path.into();
        data.metadata["actors"][0]["mpsz_bounds_primary"] = primary.into();
        data.metadata["mpob_objects"][0]["model_id"] = selector.into();
        data.metadata["mpsz_entries"] = serde_json::json!([{
            "total_extent":[48,128,49], "pos_extent":[24,62,24],
            "neg_extent":[24,66,25], "center_offset":[0,0,0], "flags":2,
        }]);
        let mut scene = PlayerScene {
            source_identity: None,
            world: world.world,
            model_radii: Default::default(),
            control_codes: None,
            settings: MovementSettings {
                standing_height: -2,
                post_collide_height: 50,
                slide_range: 1,
                slide_speed: None,
                slide_hurt: None,
                tap_time: None,
                sheath_sword_delay: None,
                defend_interval: None,
                swim_depth: None,
                max_remap_objects: None,
                turn_speed: 1,
                turn_max_speed: 1,
                poly_push_units: 1,
                joystick: None,
            },
            definition: None,
            terrain: None,
            collision: cyrus_assets::collision::CollisionScene {
                sphere_object_scale: Some(256),
                ..Default::default()
            },
            static_collision: StaticCollision::new(&Default::default()).unwrap(),
            actor_collision: Some(models),
            unavailable: None,
        };
        if path == "static" && selector == -1 {
            scene.collision.sphere_object_scale = None;
            assert!(
                world
                    .prepare_ordinary_collision_shape(id, &data, &scene)
                    .unwrap_err()
                    .to_string()
                    .contains("sphere_object_scale")
            );
            assert!(world.ordinary_collision_shape(id).is_err());
            scene.collision.sphere_object_scale = Some(256);
        }
        world
            .prepare_ordinary_collision_shape(id, &data, &scene)
            .unwrap();
        let shape = world.ordinary_collision_shape(id).unwrap();
        assert_eq!(
            shape.positive_extent[1], expected_y,
            "{path}/{primary}/{selector}"
        );
        assert_eq!(shape.additional.is_some(), generated);
        if !generated {
            assert_eq!(
                shape.body,
                [BodySphere {
                    center: [0; 3],
                    radius: 12
                }]
            );
        } else if expected_y == 62 {
            assert_eq!(
                shape.body,
                [
                    BodySphere {
                        center: [0, -42, 0],
                        radius: 24
                    },
                    BodySphere {
                        center: [0, -12, 0],
                        radius: 24
                    }
                ]
            );
        }
        assert!(
            world
                .prepare_ordinary_collision_shape(id, &data, &scene)
                .is_err()
        );
    }
}

#[test]
fn ordinary_script_wrapper_counts_and_clears_prior_response_marks() {
    let (mut world, _, _) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
    world.retain_ordinary_water_position(id).unwrap();
    let snapshot = world
        .capture_ordinary_rollback(id, &mut |_, _| Ok(()))
        .unwrap();
    world.set_actor_surface(id, Surface::WATER).unwrap();
    let mark = |world: &mut ScriptWorld| {
        assert!(
            world
                .rollback_ordinary_water(id, None, &snapshot, &mut |_, _| Ok(()))
                .unwrap()
        );
    };
    mark(&mut world);
    assert!(
        world
            .finish_ordinary_script_response(id, None)
            .unwrap_err()
            .to_string()
            .contains("nominal_ticks")
    );
    let state = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (state.last_collide_count, state.last_collide.bits()),
        (0, CollisionFlags::ROLLBACK.bits())
    );
    for (ticks, expected) in [(1, 1), (255, 0), (200, 200), (7, 200)] {
        mark(&mut world);
        world
            .finish_ordinary_script_response(id, Some(ticks))
            .unwrap();
        let state = world.actors()[0].control_state().unwrap();
        assert_eq!(state.last_collide_count, expected);
        assert_eq!(
            (state.last_collide.bits(), state.overlap_contact_identifier),
            (0, 0)
        );
    }
    world.finish_ordinary_script_response(id, None).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide_count,
        0
    );
}

#[test]
fn drno_leaves_the_response_timers_of_a_skipped_actor_untouched() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.updates.task_system = false;
    let (mut world, _, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
    world.retain_ordinary_water_position(id).unwrap();
    let snapshot = world
        .capture_ordinary_rollback(id, &mut |_, _| Ok(()))
        .unwrap();
    world.set_actor_surface(id, Surface::WATER).unwrap();
    assert!(
        world
            .rollback_ordinary_water(id, None, &snapshot, &mut |_, _| Ok(()))
            .unwrap()
    );
    let main = |world: &mut ScriptWorld, skipped: bool| {
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    actor_update_skipped: Some(skipped),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let state = world.actors()[0].control_state().unwrap();
        (state.last_collide_count, state.last_collide.bits() >> 16)
    };
    // Skipped: the task-off response finish does not run, so the mark is neither aged nor cleared.
    assert_eq!(main(&mut world, true), (0, 1));
    // Not skipped: the same stage counts the mark into the age and clears it.
    assert_eq!(main(&mut world, false), (1, 0));
}

#[test]
fn drno_skips_the_ordinary_main_stage_of_an_actor_that_is_neither_the_player_nor_the_camera() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let run = |skipped: Option<bool>| {
        let mut scripts = wait_cache(vec![
            wait(
                1,
                Native::Wait,
                CallKind::Task,
                vec![Value::Integer { value: 3 }],
            ),
            set_flag(99),
            Operation::End { target: 0 },
        ]);
        scripts.updates.task_system = true;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    actor_update_skipped: skipped,
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let runtime = world.actors()[0].runtime().unwrap();
        (
            runtime.active_tasks(),
            runtime.program_counter(Thread::Main),
        )
    };
    let ordinary = run(None);
    assert_ne!(
        ordinary,
        (0, Some(0)),
        "the ordinary stage starts the script"
    );
    assert_eq!(run(Some(false)), ordinary, "the cheat off changes nothing");
    assert_eq!(
        run(Some(true)),
        (0, Some(0)),
        "drno leaves the actor's main stage unstarted"
    );
}

#[test]
fn drno_leaves_the_selected_players_ordinary_main_stage_running() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for skipped in [false, true] {
        let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
        let player = world.selected_player().unwrap();
        world
            .advance(
                player,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    actor_update_skipped: Some(skipped),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[player.index].attributes()[Attribute::ForwardWalk],
            2,
            "the player's script ran with drno {skipped}"
        );
    }
}
