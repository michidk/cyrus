//! Logical actors execute independently of geometry through explicit public stages.
use cyrus_assets::script::*;
use cyrus_game::script::ScriptWorld;
use cyrus_script::{Argument, CallResult, Host, Target, Thread};
use std::collections::BTreeMap;

struct NoServices;
impl Host for NoServices {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: &str,
        _: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        anyhow::bail!("no native services")
    }
    fn poll(
        &mut self,
        _: u64,
        _: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        anyhow::bail!("no tasks")
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}

fn cache() -> Scripts {
    let program = Program {
        version: 2,
        entry: 0,
        variables: vec![4],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Increment {
                        destination: Place::Local { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Assign {
                        destination: Place::Flag { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Local { index: 0 },
                            },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]),
    };
    let mut scripts = Scripts {
        version: 3,
        actor_pool_capacity: 32,
        content_id: String::new(),
        world: 7,
        definition: Definition {
            functions: vec![Function {
                name: "NullFunction".into(),
                param_count: 0,
                kind: CallKind::Function,
            }],
            flags: vec![Flag {
                name: "Counter".into(),
                flag_type: "NUMBER".into(),
                initial_value: 12,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([(
            "LOGIC".into(),
            Actor {
                header_index: 0,
                code_origin: 0,
                instances: 2,
                namespace: Some("logic".into()),
                instance_variables: vec![vec![4], vec![9]],
                program,
                attributes: BTreeMap::new(),
                hooks: vec![],
            },
        )]),
        instances: (0..2)
            .map(|id| Instance {
                object_id: id,
                actor: "LOGIC".into(),
                object_type: 0,
                active: 0,
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
#[test]
fn invisible_instances_have_separate_authored_locals_and_shared_globals() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors().len(), 2);
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    world
        .invoke(first, Thread::Main, &mut NoServices, 20)
        .unwrap();
    assert_eq!(world.flags(), [5]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [9]);
    world
        .invoke(second, Thread::Main, &mut NoServices, 20)
        .unwrap();
    assert_eq!(world.flags(), [10]);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
}
#[test]
fn world_replacement_rejects_old_actor_ids_before_mutating_globals() {
    let scripts = cache();
    let old = ScriptWorld::prepare(&scripts, 1).unwrap().id(0).unwrap();
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    let error = replacement
        .invoke(old, Thread::Main, &mut NoServices, 20)
        .unwrap_err();
    assert!(error.to_string().contains("stale script actor generation"));
    assert_eq!(replacement.flags(), [12]);
}

#[test]
fn inspection_steps_logical_state_and_rejects_stale_generations() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 3).unwrap();
    let id = world.id(1).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    let report = world.inspect(1, 1);
    assert_eq!(report["actors"][0]["locals"][0], 10);
    assert_eq!(report["actors"][0]["id"]["generation"], 3);
    assert_eq!(world.flags(), [10]);
    let old = cyrus_game::script::ScriptActorId {
        generation: 2,
        index: 1,
    };
    assert!(
        world
            .step(old, Thread::Main, 20)
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
    assert!(
        world
            .step_tasks(old, Some(256))
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
}

#[test]
fn researched_attribute_query_reads_unsigned_bytes_and_checks_bounds() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "GetMyAttr".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    scripts.definition.attributes = vec!["first".into()];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes.insert("first".into(), 255);
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: "GetMyAttr".into(),
                    arguments: vec![Value::Integer { value: 0 }],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [255]);
    for index in [-1, 256] {
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        let Operation::Assign { formula, .. } =
            &mut actor.program.steps.get_mut(&0).unwrap().operation
        else {
            unreachable!()
        };
        let Value::Call { call } = &mut formula.first else {
            unreachable!()
        };
        call.arguments = vec![Value::Integer { value: index }];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [0]);
    }
}

#[test]
fn bevy_world_removal_and_same_handle_reload_expire_actor_ids() {
    use bevy::{asset::AssetApp, prelude::*};
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptActors, ScriptPlugin, ScriptStatus};
    use cyrus_world::lifecycle::PreparedWorld;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .add_plugins(ScriptPlugin);
    let data = cache();
    let content_id = data.content_id.clone();
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(Animations {
            version: 2,
            world: 7,
            metadata: serde_json::json!({"actors": [], "script_content_id": content_id}),
            sections: BTreeMap::new(),
            models: BTreeMap::new(),
            unsupported: BTreeMap::new(),
        });
    let scripts = app.world_mut().resource_mut::<Assets<Scripts>>().add(data);
    let prepared = || PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(7),
        scripts: scripts.clone(),
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    };
    let animation_data = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .remove(animations.id())
        .unwrap();
    app.insert_resource(prepared());
    app.update();
    assert!(matches!(
        app.world().resource::<ScriptStatus>(),
        ScriptStatus::Loading
    ));
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .insert(animations.id(), animation_data)
        .unwrap();
    app.update();
    let old = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .id(0)
        .unwrap();
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    assert!(app.world().resource::<ScriptActors>().current.is_none());
    assert!(matches!(
        app.world().resource::<ScriptStatus>(),
        ScriptStatus::NotRequested
    ));
    app.insert_resource(prepared());
    app.update();
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        assert!(
            world
                .step(old, Thread::Main, 20)
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
        let fresh = world.id(0).unwrap();
        world.step(fresh, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [5]);
    }
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&animations)
        .unwrap()
        .metadata["script_content_id"] = serde_json::json!("old-export");
    app.insert_resource(prepared());
    app.update();
    assert!(app.world().resource::<ScriptActors>().current.is_none());
    assert!(
        serde_json::to_string(app.world().resource::<ScriptStatus>())
            .unwrap()
            .contains("generation mismatch")
    );
}

#[test]
fn native_flag_helpers_keep_type_behavior_numeric_values_and_latches_separate() {
    let native = |id, name: &str| Value::Call {
        call: Box::new(Call {
            function_id: id,
            kind: CallKind::Function,
            target: Object::Me,
            function: name.into(),
            arguments: vec![Value::Integer { value: 0 }],
        }),
    };
    let assign = |index, value| Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first: value,
            rest: vec![],
        },
    };
    for (kind, initial, after_two, after_third, final_latch) in [
        ("BOOL", 0, 1, 0, 1),
        ("FLIPFLOP", 0, 0, 1, 1),
        ("COUNTER", 2, 0, 0, 0),
        ("NUMBER", 7, 7, 7, 1),
    ] {
        let mut scripts = cache();
        scripts.definition.flags[0].flag_type = kind.into();
        scripts.definition.flags[0].initial_value = initial;
        scripts.definition.functions.extend(
            ["SetGlobalFlag", "ResetGlobalFlag", "TestGlobalFlag"].map(|name| Function {
                name: name.into(),
                param_count: 1,
                kind: CallKind::Function,
            }),
        );
        let ops = vec![
            assign(0, native(1, "SetGlobalFlag")),
            assign(0, native(1, "SetGlobalFlag")),
            assign(1, native(3, "TestGlobalFlag")),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::Yield,
            assign(0, native(2, "ResetGlobalFlag")),
            assign(1, native(3, "TestGlobalFlag")),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::Yield,
            assign(0, native(1, "SetGlobalFlag")),
            assign(1, native(3, "TestGlobalFlag")),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::End { target: 0 },
        ];
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.program.variables = vec![0; 3];
        actor.instance_variables = vec![vec![0; 3]; 2];
        actor.program.steps = ops
            .into_iter()
            .enumerate()
            .map(|(offset, operation)| {
                (
                    offset as u32,
                    Step {
                        next: offset as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, 1, after_two],
            "{kind}"
        );
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, 0, after_two],
            "{kind}"
        );
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, final_latch, after_third],
            "{kind}"
        );
    }
}

fn wait_cache(ops: Vec<Operation>) -> Scripts {
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "Wait".into(),
            param_count: 1,
            kind: CallKind::Task,
        },
        Function {
            name: "WaitOnTasks".into(),
            param_count: 0,
            kind: CallKind::Task,
        },
        Function {
            name: "WaitNonMulti".into(),
            param_count: 0,
            kind: CallKind::Task,
        },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = ops
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
    scripts
}
fn wait(id: u16, name: &str, kind: CallKind, args: Vec<Value>) -> Operation {
    Operation::Call {
        call: Call {
            function_id: id,
            function: name.into(),
            target: Object::Me,
            kind,
            arguments: args,
        },
    }
}
fn set_flag(value: i32) -> Operation {
    Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Integer { value },
            rest: vec![],
        },
    }
}
#[test]
fn wait_continues_to_flag_change_and_missing_scale_does_not_poison_actor() {
    let scripts = wait_cache(vec![
        wait(1, "Wait", CallKind::Task, vec![Value::Integer { value: 4 }]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert!(
        world
            .step_tasks(id, None)
            .unwrap_err()
            .to_string()
            .contains("requires frame_scale")
    );
    assert!(world.actors()[0].runtime().unwrap().failure().is_none());
    world.step_tasks(id, Some(512)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    world.step_tasks(id, Some(0)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    world.step_tasks(id, Some(256)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(
        world.actors()[1]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
}
#[test]
fn wait_signed_duration_wrap_and_initial_count_excludes_unallocated_slot() {
    for duration in [i32::MIN, -1, 0, 1, 2] {
        let scripts = wait_cache(vec![
            wait(
                1,
                "Wait",
                CallKind::Task,
                vec![Value::Integer { value: duration }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [12]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world.step_tasks(id, Some(256)).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags()[0], if duration == i32::MIN { 12 } else { 7 });
    }
    for (id, name) in [(2, "WaitOnTasks"), (3, "WaitNonMulti")] {
        let scripts = wait_cache(vec![
            wait(id, name, CallKind::Task, vec![]),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let actor = world.id(0).unwrap();
        world.step(actor, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [12]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world.step_tasks(actor, Some(256)).unwrap();
        world.step(actor, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [7]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}
#[test]
fn wait_on_tasks_ignores_paused_slots_and_stop_clears_native_bookkeeping() {
    let scripts = wait_cache(vec![
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(2, "WaitOnTasks", CallKind::Multitask, vec![]),
        Operation::Yield,
        Operation::TaskPause { label: 0 },
        Operation::TaskPause { label: 1 },
        Operation::Yield,
        Operation::TaskResume { label: 0 },
        Operation::TaskStop { label: 1 },
        Operation::Yield,
        Operation::TaskStop { label: 0 },
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.flags(), [7]);
    // A later authored pass allocates fresh host tokens after cancellation.
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
}

#[test]
fn wait_non_multi_counts_blocking_slots_and_observes_same_pass_completion() {
    let mut scripts = wait_cache(vec![
        Operation::TaskPause { label: 6 },
        wait(
            1,
            "Wait",
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::TaskResume { label: 6 },
        wait(3, "WaitNonMulti", CallKind::Task, vec![]),
        set_flag(7),
        Operation::Endint,
        wait(
            1,
            "Wait",
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::End { target: 6 },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.entry = 6;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    world.step(id, Thread::Interrupt, 20).unwrap();
    world.step(id, Thread::Interrupt, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(12800)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    world.step(id, Thread::Interrupt, 20).unwrap();
    assert_eq!(world.flags(), [7]);
}

#[test]
fn wait_non_multi_ignores_multiple_unpaused_multitasks() {
    let scripts = wait_cache(vec![
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(3, "WaitNonMulti", CallKind::Task, vec![]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(256)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
}

fn routed_cache(caller: Vec<Operation>, receiver: Vec<Operation>) -> Scripts {
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
    let Operation::Call { mut call } = wait(1, "Wait", kind, vec![duration]) else {
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
                "Wait",
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
        function: "GetMyAttr".into(),
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
        name: "GetMyAttr".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    scripts.definition.attributes = (0..10).map(|index| format!("attribute_{index}")).collect();
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "ReCeIvEr".into();
    scripts
        .actors
        .get_mut("RECEIVER")
        .unwrap()
        .attributes
        .insert("attribute_9".into(), 255);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attribute_4".into(), 11);
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

fn add_standalone_receivers(scripts: &mut Scripts) {
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
    use cyrus_game::script::ScriptActorSource;
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
    actor.instances = 3;
    actor.instance_variables.push(vec![13]);
    let first = scripts.instances[0].clone();
    scripts.instances = [2, 6, 0, 1, 0]
        .into_iter()
        .enumerate()
        .map(|(index, object_type)| Instance {
            object_type,
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
        assert!(actor.runtime().is_none());
        assert!(
            actor
                .unavailable()
                .unwrap()
                .contains("separate map object path unsupported")
        );
    }
    assert_eq!(world.actors()[2].runtime().unwrap().variables(), [4]);
    assert_eq!(world.actors()[3].runtime().unwrap().variables(), [9]);
    assert_eq!(world.actors()[4].runtime().unwrap().variables(), [13]);
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
