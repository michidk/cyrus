use super::*;

pub(super) fn routed_cache(caller: Vec<Operation>, receiver: Vec<Operation>) -> Scripts {
    let mut scripts = wait_cache(caller);
    let program_steps = |ops: Vec<Operation>| {
        ops.into_iter()
            .enumerate()
            .map(|(index, operation)| {
                (
                    index as u32 * 16,
                    Step {
                        next: (index as u32 + 1) * 16,
                        operation,
                    },
                )
            })
            .collect()
    };
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let ops = actor
        .program
        .steps
        .values()
        .map(|step| step.operation.clone())
        .collect();
    actor.program.steps = program_steps(ops);
    actor.namespace = Some("caller".into());
    actor.instances = 1;
    actor.instance_variables = vec![vec![4]];
    actor.program.strings = vec!["receiver".into()];
    let mut recipient = actor.clone();
    recipient.header_index = 1;
    recipient.code_origin = 100;
    recipient.namespace = Some("receiver".into());
    recipient.program.steps = program_steps(receiver);
    recipient.program.variables = vec![9];
    recipient.instance_variables = vec![vec![9]];
    scripts.actors.insert("RECEIVER".into(), recipient);
    scripts.instances[1].actor = "RECEIVER".into();
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn object_wait(target: Object, kind: CallKind, duration: Value) -> Operation {
    let Operation::Call { mut call } = wait(1, Native::Wait, kind, vec![duration]) else {
        unreachable!()
    };
    call.target = target;
    Operation::ObjectCall { call }
}

#[test]
fn routed_task_uses_receiver_locals_and_slots_while_the_caller_continues() {
    let scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Task,
                Value::Read {
                    place: Place::Local { index: 0 },
                },
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ],
        vec![set_flag(8), Operation::End { target: 0 }],
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step(receiver, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(receiver, Some(768)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(receiver, Some(1024)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
    world.step(receiver, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [8]);
}

#[test]
fn source_operand_identity_keeps_equal_cursors_distinct_and_receiver_labels_local() {
    let scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Multitask,
                Value::Integer { value: 100 },
            ),
            Operation::TaskStop { label: 3 },
            Operation::End { target: 0 },
        ],
        vec![
            wait(
                1,
                Native::Wait,
                CallKind::Multitask,
                vec![Value::Integer { value: 100 }],
            ),
            Operation::TaskStop { label: 0 },
            Operation::End { target: 0 },
        ],
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step(receiver, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(receiver, Some(256)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
}

#[test]
fn explicit_me_object_task_continues_and_matches_the_embedded_dispatch_label() {
    let scripts = routed_cache(
        vec![
            object_wait(Object::Me, CallKind::Task, Value::Integer { value: 100 }),
            set_flag(7),
            Operation::TaskPause { label: 3 },
            Operation::Yield,
            Operation::TaskResume { label: 3 },
            Operation::TaskStop { label: 3 },
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert!(!world.actors()[0].runtime().unwrap().has_blocking_task());
    world.step_tasks(caller, Some(256)).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn named_functions_resolve_namespace_and_receiver_arguments_case_insensitively() {
    let call = Call {
        function_id: 4,
        target: Object::Named { string: 0 },
        kind: CallKind::Function,
        function: Native::GetMyAttr,
        arguments: vec![Value::Read {
            place: Place::Local { index: 0 },
        }],
    };
    let mut scripts = routed_cache(
        vec![
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(call),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.definition.functions.push(Function {
        name: Native::GetMyAttr,
        param_count: 1,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "ReCeIvEr".into();
    scripts.actors.get_mut("RECEIVER").unwrap().attributes[Attribute::BackwardSpeed] = 255;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Armor] = 11;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [255]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [9]);
}

#[test]
fn failed_receiver_calls_restore_registry_and_keep_failures_sticky() {
    let mut scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Task,
                Value::Integer { value: 100 },
            ),
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.actors.get_mut("RECEIVER").unwrap().namespace = Some("receiver".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "receiver".into();
    let Operation::ObjectCall { call } = &mut scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation
    else {
        unreachable!()
    };
    call.arguments = vec![Value::Read {
        place: Place::Local { index: 0 },
    }];
    scripts
        .actors
        .get_mut("RECEIVER")
        .unwrap()
        .program
        .variables = vec![];
    scripts
        .actors
        .get_mut("RECEIVER")
        .unwrap()
        .instance_variables = vec![vec![]];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    let error = world
        .step(caller, Thread::Main, 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("local variable absent"), "{error}");
    assert!(world.actors().iter().all(|actor| actor.runtime().is_some()));
    assert!(world.actors()[0].runtime().unwrap().failure().is_some());
    assert!(world.actors()[1].runtime().unwrap().failure().is_some());
    assert_eq!(
        world
            .step(caller, Thread::Main, 20)
            .unwrap_err()
            .to_string(),
        error
    );
    assert!(world.step(receiver, Thread::Main, 20).is_err());
}

pub(super) fn add_standalone_receivers(scripts: &mut Scripts) {
    for (index, (name, namespace, local)) in [
        ("CYRUS", None, 9),
        ("GREMLIN", Some("gremlin"), 11),
        ("CAMERA", None, 5),
    ]
    .into_iter()
    .enumerate()
    {
        let mut actor = scripts.actors["LOGIC"].clone();
        actor.instances = 1;
        actor.code_origin = 200 + index as u32 * 100;
        actor.namespace = namespace.map(str::to_owned);
        actor.instance_variables = vec![vec![local]];
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Yield,
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
        scripts.actors.insert(name.into(), actor);
    }
    scripts.content_id = scripts.content_identity().unwrap();
}

#[test]
fn standalone_player_and_camera_receive_tasks_without_map_placements() {
    use cyrus_script::script::ScriptActorSource;
    let mut scripts = wait_cache(vec![
        object_wait(
            Object::Player,
            CallKind::Task,
            Value::Read {
                place: Place::Local { index: 0 },
            },
        ),
        object_wait(
            Object::Camera,
            CallKind::Task,
            Value::Read {
                place: Place::Local { index: 0 },
            },
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    add_standalone_receivers(&mut scripts);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.actors().len(), 5);
    assert_eq!(
        world
            .actors()
            .iter()
            .map(|actor| actor.runtime_identifier)
            .collect::<Vec<_>>(),
        [Some(36), Some(37), Some(1), Some(1), Some(2)]
    );
    assert!(matches!(
        world.actors()[2].source,
        ScriptActorSource::Player
    ));
    assert!(matches!(
        world.actors()[3].source,
        ScriptActorSource::AlternatePlayer
    ));
    assert!(matches!(
        world.actors()[4].source,
        ScriptActorSource::Camera
    ));
    assert!(
        world.actors()[2..]
            .iter()
            .all(|actor| actor.placement().is_none())
    );
    assert_eq!(
        world.inspect(2, 1)["actors"][0]["object_id"],
        serde_json::Value::Null
    );
    let caller = world.id(0).unwrap();
    let player = world.id(2).unwrap();
    let camera = world.id(4).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[2].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.actors()[4].runtime().unwrap().active_tasks(), 1);
    assert_eq!(
        world.actors()[2]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    world.step_tasks(player, Some(7 * 256)).unwrap();
    assert_eq!(world.actors()[2].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(camera, Some(5 * 256)).unwrap();
    assert_eq!(world.actors()[4].runtime().unwrap().active_tasks(), 0);
    world.step_tasks(player, Some(5 * 256)).unwrap();
    assert_eq!(world.actors()[2].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[2].runtime().unwrap().variables(), [9]);
    assert_eq!(world.actors()[4].runtime().unwrap().variables(), [5]);
}

#[test]
fn signed_identifier_order_resolves_duplicate_names_across_the_low_word_wrap() {
    let mut scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Task,
                Value::Integer { value: 100 },
            ),
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.actor_pool_capacity = 32763;
    scripts.actors.get_mut("RECEIVER").unwrap().namespace = Some("caller".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "CaLlEr".into();
    scripts.instances[0].object_id = 0;
    scripts.instances[1].object_id = u32::MAX;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.actors()[0].runtime_identifier, Some(32767));
    assert_eq!(world.actors()[1].runtime_identifier, Some(-32768));
    world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
}

#[test]
fn normal_duplicate_names_choose_the_first_registered_runtime_identifier() {
    let mut scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Multitask,
                Value::Integer { value: 100 },
            ),
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.actors.get_mut("RECEIVER").unwrap().namespace = Some("caller".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "caller".into();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn activation_and_separate_map_paths_preserve_identifier_and_local_allocation_order() {
    let mut scripts = cache();
    scripts.actor_pool_capacity = 10;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 5;
    actor
        .instance_variables
        .extend([vec![13], vec![17], vec![21]]);
    let first = scripts.instances[0].clone();
    scripts.instances = [2, 6, 0, 1, 0]
        .into_iter()
        .enumerate()
        .map(|(index, object_type)| Instance {
            object_type: PlacementType::from_byte(object_type),
            active: u8::from(index != 2),
            ..first.clone()
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world
            .actors()
            .iter()
            .map(|actor| actor.runtime_identifier)
            .collect::<Vec<_>>(),
        [None, None, Some(14), Some(16), Some(18)]
    );
    for actor in &world.actors()[..2] {
        assert!(actor.runtime().is_some());
        assert!(actor.unavailable().is_none());
        assert!(actor.control_state().is_none());
    }
    for (index, value) in [4, 9, 13, 17, 21].into_iter().enumerate() {
        assert_eq!(
            world.actors()[index].runtime().unwrap().variables(),
            [value]
        );
    }
}

#[test]
fn registering_an_existing_identifier_keeps_the_first_namespace_only() {
    let mut scripts = wait_cache(vec![
        object_wait(
            Object::Named { string: 0 },
            CallKind::Task,
            Value::Integer { value: 10 },
        ),
        Operation::End { target: 0 },
    ]);
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("CYRUS").unwrap().namespace = Some("first_player".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["gremlin".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = world
        .step(world.id(0).unwrap(), Thread::Main, 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("receiver absent"), "{error}");
    assert!(world.actors()[3].runtime().is_some());
    assert_eq!(world.actors()[3].runtime().unwrap().active_tasks(), 0);
}

pub(super) fn actor_reference_step(
    world: &mut ScriptWorld,
    id: cyrus_script::script::ScriptActorId,
) -> anyhow::Result<()> {
    world.advance(
        id,
        ScriptStage::Body(Thread::Main),
        cyrus_script::script::ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        20,
    )
}

pub(super) fn actor_reference_scripts(object: Object) -> Scripts {
    let mut scripts = heading_property_scripts(295, Object::Me);
    scripts.definition.references[294] = Property::Address;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.variables = vec![0; 3];
    actor.instance_variables = vec![vec![0; 3]; 2];
    let read = |object, reference| Value::Read {
        place: Place::Property { object, reference },
    };
    let assign = |index, first| Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first,
            rest: vec![],
        },
    };
    actor.program.steps = [
        assign(0, read(object, 294)),
        assign(
            1,
            Value::Read {
                place: Place::Local { index: 0 },
            },
        ),
        Operation::Yield,
        assign(2, read(Object::Local { index: 1 }, 295)),
        Operation::End { target: 4 },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, operation)| {
        (
            i as u32,
            Step {
                next: i as u32 + 1,
                operation,
            },
        )
    })
    .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn actor_reference_address_preserves_resolved_identity_without_spatial_or_runtime_id_encoding() {
    use cyrus_soup::{ActorReference, ScriptValue};
    for (object, expected) in [
        (Object::Me, 1),
        (Object::Named { string: 0 }, 0),
        (Object::Player, 2),
        (Object::Camera, 4),
    ] {
        let mut scripts = actor_reference_scripts(object);
        add_standalone_receivers(&mut scripts);
        for name in ["CYRUS", "GREMLIN", "CAMERA"] {
            scripts.actors.get_mut(name).unwrap().instance_variables = vec![vec![0; 3]];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let id = world.id(1).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables(),
            [
                ScriptValue::Actor(ActorReference {
                    generation: 17,
                    index: expected
                }),
                ScriptValue::Actor(ActorReference {
                    generation: 17,
                    index: expected
                }),
                ScriptValue::Integer(0),
            ]
        );
    }
}

#[test]
fn actor_reference_address_rejects_temporary_separate_actor_identity() {
    for object_type in [PlacementType::SCRIPT_ONLY, PlacementType::COLOURED_LIGHT] {
        let mut scripts = actor_reference_scripts(Object::Me);
        scripts.instances[1].object_type = object_type;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        let id = world.id(1).unwrap();
        let error = actor_reference_step(&mut world, id)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("temporary or unallocated reference identity unsupported"),
            "{error}"
        );
        assert_eq!(world.actors()[1].runtime().unwrap().variables(), [0, 0, 0]);
        assert_eq!(
            actor_reference_step(&mut world, id)
                .unwrap_err()
                .to_string(),
            error
        );
    }
}

#[test]
fn actor_reference_local_property_routes_to_captured_actor_and_rejects_integer_alias() {
    let scripts = actor_reference_scripts(Object::Named { string: 0 });
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(1).unwrap();
    actor_reference_step(&mut world, id).unwrap();
    actor_reference_step(&mut world, id).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().variables()[2], 1536);
    let mut scripts = actor_reference_scripts(Object::Me);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Integer { value: 3 },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let id = world.id(1).unwrap();
    actor_reference_step(&mut world, id).unwrap();
    let error = actor_reference_step(&mut world, id)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("integer is not an actor identity"),
        "{error}"
    );
    assert_eq!(world.actors()[1].runtime().unwrap().variables()[2], 0);
    assert_eq!(
        actor_reference_step(&mut world, id)
            .unwrap_err()
            .to_string(),
        error
    );
}

#[test]
fn actor_reference_native_target_preserves_receiver_and_failure_continuations() {
    for invalid_argument in [false, true] {
        let mut scripts = actor_reference_scripts(Object::Named { string: 0 });
        let function_id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::SetRotation,
            kind: CallKind::Function,
            param_count: 3,
        });
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.program.steps.get_mut(&3).unwrap().operation = Operation::ObjectCall {
            call: Call {
                function_id,
                function: Native::SetRotation,
                kind: CallKind::Function,
                target: Object::Local { index: 0 },
                arguments: vec![
                    if invalid_argument {
                        Value::Read {
                            place: Place::Property {
                                object: Object::Me,
                                reference: 294,
                            },
                        }
                    } else {
                        Value::Integer { value: 0 }
                    },
                    Value::Integer { value: 512 },
                    Value::Integer { value: 0 },
                ],
            },
        };
        actor.program.steps.get_mut(&4).unwrap().operation = Operation::Assign {
            destination: Place::Local { index: 2 },
            formula: Formula {
                first: Value::Integer { value: 23 },
                rest: vec![],
            },
        };
        actor.program.steps.insert(
            5,
            Step {
                next: 6,
                operation: Operation::End { target: 5 },
            },
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(1).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        let result = actor_reference_step(&mut world, id);
        assert!(world.actors().iter().all(|actor| actor.runtime().is_some()));
        if invalid_argument {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("integer operand required"), "{error}");
            assert_eq!(world.actors()[1].runtime().unwrap().variables()[2], 0);
            assert_eq!(
                world.spatial(world.id(0).unwrap()).unwrap().local.angles[1],
                1536
            );
            assert_eq!(
                actor_reference_step(&mut world, id)
                    .unwrap_err()
                    .to_string(),
                error
            );
        } else {
            result.unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables()[2], 23);
            assert_eq!(
                world.spatial(world.id(0).unwrap()).unwrap().local.angles[1],
                512
            );
            assert_eq!(world.spatial(id).unwrap().local.angles[1], 0);
        }
    }
}

#[test]
fn actor_reference_snapshot_rebinds_live_identity_and_rejects_stale_generation() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = actor_reference_scripts(Object::Named { string: 0 });
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [0; 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, _) = static_model_fixture_session(scripts.clone(), None, [0; 3], false);
    let id = world.id(1).unwrap();
    actor_reference_step(&mut world, id).unwrap();
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = world.snapshot(&sources).unwrap();
    let mut invalid = serde_json::to_value(&saved).unwrap();
    invalid["actors"][1]["runtime"]["state"]["variables"][1]["generation"] = 999.into();
    let before = world.inspect(0, 10);
    let error = ScriptWorld::from_snapshot(
        serde_json::from_value(invalid).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 101,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .err()
    .unwrap();
    assert!(
        format!("{error:#}").contains("stale or absent saved owner"),
        "{error:#}"
    );
    assert_eq!(world.inspect(0, 10), before);
    let mut restored = ScriptWorld::from_snapshot(
        saved,
        &sources,
        WorldRestoreContext {
            generation: 101,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        restored.actors()[1].runtime().unwrap().variables()[1]
            .actor()
            .unwrap()
            .generation,
        101
    );
    let restored_id = restored.id(1).unwrap();
    actor_reference_step(&mut restored, restored_id).unwrap();
    assert_eq!(restored.actors()[1].runtime().unwrap().variables()[2], 1536);
}

#[test]
fn my_id_reads_the_runtime_identifier() {
    let world = run_native_world(&[(Native::MyId, &[])]).unwrap();
    let actor = &world.actors()[0];
    let expected = i32::from(actor.runtime_identifier.unwrap());
    assert_eq!(actor.runtime().unwrap().variables()[0], expected);
}
