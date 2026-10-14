//! Logical actors execute independently of geometry through explicit public stages.
use cyrus_assets::script::*;
use cyrus_game::script::ScriptWorld;
use cyrus_script::program::*;
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
        version: 10,
        player_distance_override: false,
        updates: cyrus_assets::script::UpdateSettings {
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
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
                location_offsets: vec![],
                program,
                attributes: BTreeMap::new(),
                native_hook: None,
            },
        )]),
        instances: (0..2)
            .map(|id| Instance {
                object_id: id,
                actor: "LOGIC".into(),
                object_type: 0,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
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
fn map_animation_initialization_retains_independent_frames_and_rejects_reinitialization() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},
                        {"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]}]}]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    for object_type in [0, 2, 3, 4, 5, 6] {
        let mut unsupported = scripts.clone();
        unsupported.instances[0].object_type = object_type;
        unsupported.content_id = unsupported.content_identity().unwrap();
        data.metadata["script_content_id"] = unsupported.content_id.clone().into();
        let mut world = ScriptWorld::prepare(&unsupported, 1).unwrap();
        let id = world.id(0).unwrap();
        let error = world
            .initialize_map_animation(id, &data, 1, &mut |_, _| {
                panic!("unsupported map type dispatched an animation sound")
            })
            .unwrap_err();
        assert!(error.to_string().contains("requires map object type 1"));
        assert_eq!(world.animation_frame(id).unwrap(), None);
    }
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    for path in ["none", "static", ""] {
        data.metadata["actors"][0]["model_load_path"] = path.into();
        assert!(
            world
                .initialize_map_animation(first, &data, 1, &mut |_, _| Ok(()))
                .is_err()
        );
        assert_eq!(world.animation_frame(first).unwrap(), None);
    }
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    for id in [first, second] {
        world
            .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    }
    world
        .advance(
            first,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            0,
        )
        .unwrap();
    assert_eq!(world.animation_frame(first).unwrap(), Some(11));
    assert_eq!(world.animation_frame(second).unwrap(), Some(10));
    assert!(
        world
            .initialize_map_animation(first, &data, 1, &mut |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(world.animation_frame(first).unwrap(), Some(11));
}

#[test]
fn animation_initialization_does_not_repeat_a_failed_sound_effect() {
    use cyrus_assets::animation::Animations;
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":4,"sound_param":-17,"volume_shift":-1},
                        {"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut calls = 0;
    for _ in 0..2 {
        let error = world
            .initialize_map_animation(id, &data, 1, &mut |receiver, _| {
                assert_eq!(receiver, id);
                calls += 1;
                anyhow::bail!("sound unavailable")
            })
            .unwrap_err();
        assert!(error.to_string().contains("sound unavailable"));
    }
    assert_eq!(calls, 1);
    assert!(world.actors()[0].failure().is_some());
    assert!(
        world.actors()[0]
            .animation_failure()
            .unwrap()
            .contains("sound unavailable")
    );
    assert!(!world.actors()[0].animation_initialized());
    assert!(world.actors()[0].runtime().unwrap().failure().is_none());
    assert_eq!(world.animation_frame(id).unwrap(), None);
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
    use cyrus_game::{AppState, clock::SimulationClock};
    use cyrus_world::lifecycle::PreparedWorld;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .insert_resource(State::new(AppState::InGame))
        .add_plugins(ScriptPlugin);
    let data = cache();
    let content_id = data.content_id.clone();
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(Animations {
            version: 2,
            world: 7,
            metadata: serde_json::json!({"actors": [], "markers": [], "walk_node_maps": [], "script_content_id": content_id}),
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
    {
        let mut clock = app.world_mut().resource_mut::<SimulationClock>();
        for _ in 0..12 {
            clock.advance(0);
        }
        assert_eq!(clock.advance(18).frame_scale, 236);
    }
    app.insert_resource(State::new(AppState::Menu));
    app.update();
    app.insert_resource(State::new(AppState::InGame));
    app.update();
    assert_eq!(
        app.world_mut()
            .resource_mut::<SimulationClock>()
            .advance(18)
            .frame_scale,
        236
    );
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
        let mut clock = app.world_mut().resource_mut::<SimulationClock>();
        for _ in 0..12 {
            assert_eq!(clock.advance(18).frame_scale, 256);
        }
        assert_eq!(clock.advance(18).frame_scale, 236);
    }
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

fn attribute_call(id: u16, name: &str, index: i32, value: Option<i32>) -> Call {
    let mut arguments = vec![Value::Integer { value: index }];
    if let Some(value) = value {
        arguments.push(Value::Integer { value });
    }
    Call {
        function_id: id,
        target: Object::Me,
        kind: CallKind::Function,
        function: name.into(),
        arguments,
    }
}
fn attribute_cache(operations: Vec<Operation>) -> Scripts {
    let mut scripts = wait_cache(operations);
    scripts.definition.functions.truncate(1);
    for (name, param_count) in [
        ("SetAttribute", 2),
        ("SetMyAttr", 2),
        ("GetAttribute", 1),
        ("GetMyAttr", 1),
    ] {
        scripts.definition.functions.push(Function {
            name: name.into(),
            kind: CallKind::Function,
            param_count,
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
#[test]
fn attribute_writes_truncate_and_only_set_attribute_updates_backup_bytes() {
    let scripts = attribute_cache(vec![
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(attribute_call(1, "SetAttribute", 1, Some(-2))),
                },
                rest: vec![],
            },
        },
        Operation::Yield,
        Operation::Call {
            call: attribute_call(1, "SetAttribute", 6, Some(0x123)),
        },
        Operation::Call {
            call: attribute_call(2, "SetMyAttr", 1, Some(511)),
        },
        Operation::Call {
            call: attribute_call(2, "SetMyAttr", 6, Some(-1)),
        },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(attribute_call(3, "GetAttribute", 115, None)),
                },
                rest: vec![(
                    1,
                    Value::Call {
                        call: Box::new(attribute_call(4, "GetMyAttr", 119, None)),
                    },
                )],
            },
        },
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [289]);
    let mut expected = [0; 256];
    expected[1] = 255;
    expected[6] = 255;
    expected[115] = 35;
    expected[119] = 254;
    assert_eq!(world.actors()[0].attributes(), &expected);
    assert_eq!(world.actors()[1].attributes(), &[0; 256]);
}
#[test]
fn set_my_attribute_ignores_out_of_range_indices_without_mutation() {
    for index in [i32::MIN, -1, 256, i32::MAX] {
        let scripts = attribute_cache(vec![
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(attribute_call(2, "SetMyAttr", index, Some(5))),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
        assert_eq!(world.actors()[0].attributes(), &[0; 256]);
    }
}
#[test]
fn routed_attribute_writes_use_receiver_locals_and_are_visible_in_the_same_stage() {
    let mut write = attribute_call(1, "SetAttribute", 1, Some(0));
    write.target = Object::Named { string: 0 };
    write.arguments[1] = Value::Read {
        place: Place::Local { index: 0 },
    };
    let mut read = attribute_call(3, "GetAttribute", 119, None);
    read.target = Object::Named { string: 0 };
    let mut scripts = attribute_cache(vec![
        Operation::ObjectCall { call: write },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(read),
                },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["LoGiC".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.step(world.id(1).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].attributes()[1], 4);
    assert_eq!(world.actors()[0].attributes()[119], 4);
    assert_eq!(world.actors()[1].attributes(), &[0; 256]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [4]);
}
#[test]
fn unsupported_attribute_access_fails_before_mutation_and_remains_sticky() {
    for (id, name, index, value, reason) in [
        (1, "SetAttribute", -1, Some(9), "index outside"),
        (1, "SetAttribute", 256, Some(9), "index outside"),
        (3, "GetAttribute", 256, None, "index outside"),
    ] {
        let scripts = attribute_cache(vec![
            Operation::Call {
                call: attribute_call(id, name, index, value),
            },
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let actor = world.id(0).unwrap();
        let error = world.step(actor, Thread::Main, 20).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
        assert!(
            error.contains("world 7") && error.contains("instruction 0"),
            "{error}"
        );
        assert_eq!(world.actors()[0].attributes(), &[0; 256]);
        assert_eq!(world.flags(), [12]);
        assert_eq!(
            world.step(actor, Thread::Main, 20).unwrap_err().to_string(),
            error
        );
    }
}

#[test]
fn nested_families_compose_original_positions_and_keep_generation_identity() {
    let mut scripts = cache();
    let template = scripts.actors["LOGIC"].clone();
    scripts.actors.clear();
    scripts.instances.clear();
    for (index, (name, parent, master, position, angles)) in [
        ("ROOT", 0, 20, [100, 200, 300], [0, 0, 0]),
        ("PLATFORM", 20, 24, [100, 500, 300], [0, 0, 0]),
        ("LEVER", 24, 0, [110, 480, 330], [0, 512, 0]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut actor = template.clone();
        actor.instances = 1;
        actor.instance_variables.truncate(1);
        actor.attributes = BTreeMap::from([("attr_25".into(), master), ("attr_26".into(), parent)]);
        scripts.actors.insert(name.into(), actor);
        scripts.instances.push(Instance {
            object_id: index as u32,
            actor: name.into(),
            object_type: 0,
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            map_position: position,
            angles,
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 12).unwrap();
    world.prepare_spatial().unwrap();
    let lever = world.id(2).unwrap();
    assert_eq!(
        world.spatial(lever).unwrap().local.position,
        [2560, -5120, 7680]
    );
    assert!(world.spatial(lever).unwrap().effective.is_none());
    world.refresh_families().unwrap();
    let pose = world.spatial(lever).unwrap();
    assert_eq!(pose.effective.unwrap().position, [28160, 122880, 84480]);
    assert_eq!(pose.effective.unwrap().angles, [0, 512, 0]);
    assert_eq!(pose.previous_effective_position, Some([2560, -5120, 7680]));
    assert!(world.prepare_spatial().is_err());
    let replacement = ScriptWorld::prepare(&scripts, 13).unwrap();
    assert!(
        replacement
            .spatial(lever)
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
}

#[test]
fn spatial_distance_requires_explicit_override_and_fallback_state() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let actor = world.spatial(world.id(0).unwrap()).unwrap();
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [256, -512, 768],
            heading: 2047,
        }),
        distance_override: None,
        fallback_position: None,
        linked_position: Some(None),
    };
    assert!(actor.query("PlayerDistance", &[], inputs).is_err());
    inputs.distance_override = Some(false);
    assert_eq!(
        actor.query("PlayerDistance", &[], inputs).unwrap(),
        Some(1536)
    );
    assert_eq!(
        actor
            .query(
                "PlayerFacing",
                &[Argument::Integer(0), Argument::Integer(1)],
                inputs
            )
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        actor
            .query(
                "PlayerFacing",
                &[Argument::Integer(0), Argument::Integer(-1)],
                inputs
            )
            .unwrap(),
        Some(0)
    );
    inputs.player = None;
    assert!(
        actor
            .query(
                "PlayerFacing",
                &[Argument::Integer(0), Argument::Integer(1)],
                inputs
            )
            .is_err()
    );
    assert!(actor.query("PlayerDistance", &[], inputs).is_err());
    inputs.fallback_position = Some([1, 2, 3]);
    assert_eq!(actor.query("PlayerDistance", &[], inputs).unwrap(), Some(6));
    inputs.distance_override = Some(true);
    assert_eq!(
        actor.query("PlayerDistance", &[], inputs).unwrap(),
        Some(9_999_999)
    );
}

#[test]
fn ambiguous_attachment_preparation_does_not_install_partial_state() {
    let mut data = cache();
    data.instances[1].active = 1;
    data.actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_25".into(), 1);
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    let error = world.prepare_spatial().unwrap_err();
    assert!(format!("{error:#}").contains("duplicate masters"));
    assert!(
        world
            .spatial(world.id(0).unwrap())
            .unwrap_err()
            .to_string()
            .contains("not prepared")
    );
    assert_eq!(world.flags(), [12]);
}

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
            object_type,
            ..template.clone()
        })
        .collect();
    data.definition.functions.push(Function {
        name: "SetMyAttr".into(),
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
                        function: "SetMyAttr".into(),
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
            actor.attributes.insert("attr_113".into(), 1);
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
    assert!(world.ordinary_actors([-1, 0]).is_err());
    assert!(world.ordinary_actors([65536, 0]).is_err());
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
    assert_eq!(world.actors()[5].attributes()[113], 0);
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
fn living_map_default_writes_do_not_change_the_combat_death_default() {
    use cyrus_assets::animation::Animations;
    for combat_attribute in [0, 1] {
        for (function, name) in [(1, "SetAttribute"), (2, "SetMyAttr")] {
            let mut scripts = attribute_cache(vec![
                Operation::Call {
                    call: attribute_call(function, name, 44, Some(-2)),
                },
                Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(attribute_call(3, "GetAttribute", 44, None)),
                        },
                        rest: vec![],
                    },
                },
                Operation::End { target: 0 },
            ]);
            scripts.instances[0].object_type = 1;
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .attributes
                .insert("attr_14".into(), combat_attribute);
            scripts.content_id = scripts.content_identity().unwrap();
            let data = Animations {
                version: 2,
                world: scripts.world,
                metadata: serde_json::json!({
                    "script_content_id": scripts.content_id,
                    "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                            "commands":[{"opcode":0,"frame_reference":10},
                                {"opcode":2,"value":0}]}]}]
                }),
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            };
            let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
            let id = world.id(0).unwrap();
            world
                .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
                .unwrap();
            world
                .advance(
                    id,
                    cyrus_game::script::ScriptStage::Body(Thread::Main),
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            let actor = &world.actors()[0];
            assert_eq!(actor.attributes()[44], 254);
            assert_eq!(actor.runtime().unwrap().variables(), [254]);
            assert_eq!(
                actor.control_state().unwrap().combat_death_default,
                if combat_attribute == 0 { None } else { Some(0) }
            );
            assert_eq!(world.animation_frame(id).unwrap(), Some(10));
            assert_eq!(world.actors()[1].attributes()[44], 0);
        }
    }
}

#[test]
fn standalone_default_writes_require_role_lifecycle_before_mutating() {
    for (function, name) in [(1, "SetAttribute"), (2, "SetMyAttr")] {
        let mut write = attribute_call(function, name, 44, Some(7));
        write.target = Object::Player;
        let mut scripts = attribute_cache(vec![
            Operation::ObjectCall { call: write },
            Operation::End { target: 0 },
        ]);
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let error = world
            .step(world.id(0).unwrap(), Thread::Main, 20)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("attribute 44 actor lifecycle unavailable")
        );
        let player = world
            .actors()
            .iter()
            .find(|actor| actor.name() == "CYRUS")
            .unwrap();
        assert!(player.control_state().is_none());
        assert_eq!(player.attributes()[44], 0);
    }
}

#[test]
fn map_default_selection_observes_script_writes_and_updates_the_shared_frame() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = attribute_cache(vec![
        Operation::Call {
            call: attribute_call(1, "SetAttribute", 44, Some(262)),
        },
        Operation::Yield,
        Operation::Call {
            call: attribute_call(2, "SetMyAttr", 110, Some(1)),
        },
        Operation::End { target: 0 },
    ]);
    scripts.instances[0].object_type = 1;
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups": ([0, 6, 8, 91, 129].into_iter().map(|group| serde_json::json!({
                    "group_index": group, "anim_id": 1, "anim_type": 0,
                    "commands": [{"opcode": 0, "frame_reference": group * 10},
                        {"opcode": 2, "value": 0}]
                })).collect::<Vec<_>>()) }]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(0));
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    world
        .select_map_animation_default(id, false, 1, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(60));
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    world
        .select_map_animation_default(id, false, 1, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(80));
    assert_eq!(world.actors()[0].attributes()[44], 6);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat_death_default,
        None
    );
    for (initial_group, items_ready, expected_frame) in [
        (129, true, 60),
        (129, false, 60),
        (91, true, 80),
        (91, false, 60),
    ] {
        let mut selected = ScriptWorld::prepare(&scripts, 1).unwrap();
        selected.bind_animation(id, &data, initial_group).unwrap();
        selected
            .advance(id, ScriptStage::Animation, inputs, None, None, 20)
            .unwrap();
        selected
            .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
            .unwrap();
        selected
            .select_map_animation_default(id, items_ready, 1, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(selected.animation_frame(id).unwrap(), Some(expected_frame));
    }
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    assert!(
        replacement
            .select_map_animation_default(id, false, 1, &mut |_, _| Ok(()))
            .is_err()
    );
}

#[test]
fn spatial_axis_locks_restore_independent_saved_axes_and_leave_the_matrix_unchanged() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let template = world.spatial(world.id(0).unwrap()).unwrap();
    for (attribute, position, angles) in [
        (40, [100, 500, 600], [400, 500, 600]),
        (41, [400, 200, 600], [400, 500, 600]),
        (42, [400, 500, 300], [400, 500, 600]),
        (38, [400, 500, 600], [100, 500, 600]),
        (37, [400, 500, 600], [400, 200, 600]),
        (39, [400, 500, 600], [400, 500, 300]),
    ] {
        for slave in [0, 1] {
            let mut pose = template.clone();
            pose.local.position = [100, 200, 300];
            pose.local.angles = [100, 200, 300];
            pose.snapshot();
            pose.local.position = [400, 500, 600];
            pose.local.angles = [400, 500, 600];
            let matrix = pose.local.matrix;
            let mut attributes = [0; 256];
            attributes[attribute] = 255;
            attributes[26] = slave;
            pose.restore_axis_locks(&attributes).unwrap();
            assert_eq!(
                pose.local.position,
                if slave == 0 {
                    position
                } else {
                    [400, 500, 600]
                }
            );
            assert_eq!(
                pose.local.angles,
                if slave == 0 { angles } else { [400, 500, 600] }
            );
            assert_eq!(pose.local.matrix, matrix);
        }
    }
    let mut pose = template.clone();
    assert!(pose.restore_axis_locks(&[0; 255]).is_err());
}

#[test]
fn native_hook_updates_only_its_receiver_without_advancing_the_script() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "SetMyAttr".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
        function_id: 1,
        function: "SetMyAttr".into(),
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
    assert_eq!(world.actors()[0].attributes()[6], 19);
    assert_eq!(world.actors()[1].attributes()[6], 0);
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
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    for (name, param_count, kind) in [
        ("DisableScript", 0, CallKind::Function),
        ("EnableScript", 0, CallKind::Function),
        ("Wait", 1, CallKind::Task),
    ] {
        scripts.definition.functions.push(Function {
            name: name.into(),
            param_count,
            kind,
        });
    }
    let control_call = |enabled| Call {
        function_id: if enabled { 2 } else { 1 },
        function: if enabled {
            "EnableScript"
        } else {
            "DisableScript"
        }
        .into(),
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
                function: "Wait".into(),
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
    assert_eq!(world.actors()[0].control_state().unwrap().script_control, 1);
    assert_eq!(world.actors()[1].control_state().unwrap().script_control, 0);
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
    assert_eq!(world.actors()[0].control_state().unwrap().script_control, 0);
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.flags(), [14]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
}

#[test]
fn ordinary_hook_observes_unpaused_multitasks_and_installed_task_switch() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for task_system in [false, true] {
        for paused in [false, true] {
            let mut operations = vec![wait(
                1,
                "Wait",
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
                name: "SetMyAttr".into(),
                param_count: 2,
                kind: CallKind::Function,
            });
            scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
                function_id: 4,
                function: "SetMyAttr".into(),
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
                world.actors()[0].attributes()[6],
                if !task_system || paused { 19 } else { 0 }
            );
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        }
    }
}

#[test]
fn disabled_task_system_preserves_main_continuation_and_task_state() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = wait_cache(vec![
        wait(1, "Wait", CallKind::Task, vec![Value::Integer { value: 3 }]),
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

fn marker_world(position: [i32; 3], marker: i32, distance: i32) -> ScriptWorld {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "MoveByAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        },
        Function {
            name: "AtNodeMarker".into(),
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    let mut operations: Vec<_> = position
        .into_iter()
        .enumerate()
        .map(|(axis, value)| {
            wait(
                1,
                "MoveByAxis",
                CallKind::Multitask,
                vec![
                    Value::Integer { value: axis as i32 },
                    Value::Integer { value },
                    Value::Integer { value: 1 },
                ],
            )
        })
        .collect();
    operations.push(Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 2,
                    function: "AtNodeMarker".into(),
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![
                        Value::Integer { value: marker },
                        Value::Integer { value: distance },
                    ],
                }),
            },
            rest: vec![],
        },
    });
    operations.push(Operation::End { target: 0 });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":[{"map_position":[0,0,0],"reserved":1},{"map_position":[0,0,0],"reserved":0}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let result = world.advance(
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
    );
    if matches!(marker, 0) {
        result.unwrap();
    } else {
        assert!(result.is_err());
    }
    world
}

#[test]
fn node_marker_query_uses_euclidean_distance_including_the_boundary_and_vertical_axis() {
    for (position, distance, expected) in [
        ([768, 1024, 0], 1279, 0),
        ([768, 1024, 0], 1280, 1),
        ([0, 0, 0], -1, 0),
        ([0, 0, 0], 0, 1),
        ([40265340, 53687120, 1], 67108900, 0),
        ([40265340, 53687120, 1], 67108904, 1),
    ] {
        assert_eq!(
            marker_world(position, 0, distance).flags(),
            [expected],
            "position {position:?} distance {distance}"
        );
    }
}

#[test]
fn unavailable_marker_query_is_contextual_and_sticky() {
    for (marker, message) in [(-1, "outside"), (1, "unavailable"), (128, "outside")] {
        let mut world = marker_world([0; 3], marker, 0);
        let error = world.actors()[0].failure().unwrap().to_owned();
        assert!(
            error.contains("world 7")
                && error.contains(&format!("marker {marker}"))
                && error.contains(message),
            "{error}"
        );
        assert_eq!(world.flags(), [12]);
        let id = world.id(0).unwrap();
        assert!(world.step(id, Thread::Main, 20).is_err());
        assert_eq!(world.actors()[0].failure(), Some(error.as_str()));
    }
}

#[test]
fn marker_preparation_rejects_malformed_or_mismatched_data_before_installing_state() {
    use cyrus_assets::animation::Animations;
    let scripts = cache();
    let data = |markers| Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":markers}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let valid = serde_json::json!([{"map_position":[0,0,0],"reserved":1}]);
    for malformed in [
        serde_json::Value::Null,
        serde_json::json!([{"map_position":[0,0],"reserved":1}]),
        serde_json::json!([{"map_position":[8388608,0,0],"reserved":1}]),
        serde_json::json!(vec![
            serde_json::json!({"map_position":[0,0,0],"reserved":1});
            129
        ]),
    ] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let error = world.prepare_markers(&data(malformed)).unwrap_err();
        assert!(format!("{error:#}").contains("world 7 marker"));
        world.prepare_markers(&data(valid.clone())).unwrap();
        assert!(world.prepare_markers(&data(valid.clone())).is_err());
    }
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut mismatch = data(valid.clone());
    mismatch.metadata["script_content_id"] = "old".into();
    assert!(
        world
            .prepare_markers(&mismatch)
            .unwrap_err()
            .to_string()
            .contains("cache generation")
    );
    mismatch.metadata["script_content_id"] = scripts.content_id.clone().into();
    mismatch.world += 1;
    assert!(world.prepare_markers(&mismatch).is_err());
    world.prepare_markers(&data(valid)).unwrap();
}

#[test]
fn routed_node_marker_query_reads_the_receivers_local_attached_position() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "AtNodeMarker".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["child".into()];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Flag { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id: 1,
                                function: "AtNodeMarker".into(),
                                kind: CallKind::Function,
                                target: Object::Named { string: 0 },
                                arguments: vec![
                                    Value::Integer { value: 0 },
                                    Value::Integer { value: 1280 },
                                ],
                            }),
                        },
                        rest: vec![],
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
    let mut child = actor.clone();
    child.namespace = Some("child".into());
    child.attributes.insert("attr_26".into(), 1);
    actor.attributes.insert("attr_25".into(), 1);
    scripts.actors.insert("CHILD".into(), child);
    scripts.instances[0].map_position = [100, 0, 0];
    scripts.instances[1].map_position = [103, 4, 0];
    scripts.instances[1].actor = "CHILD".into();
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":[{"map_position":[0,0,0],"reserved":2}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let parent = world.id(0).unwrap();
    let child = world.id(1).unwrap();
    assert_eq!(world.spatial(child).unwrap().local.position, [768, 1024, 0]);
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position,
        [26368, 1024, 0]
    );
    world
        .advance(
            parent,
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
    assert_eq!(world.flags(), [1]);
    assert_eq!(
        world.actors()[1]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
}

fn random_scripts(function: &str, arguments: Vec<Value>, target: Object) -> Scripts {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: function.into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["logic".into()];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id: 1,
                                function: function.into(),
                                kind: CallKind::Function,
                                target,
                                arguments,
                            }),
                        },
                        rest: vec![],
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
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn random_stream_is_shared_by_routed_actors_and_survives_world_replacement() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    let scripts = random_scripts(
        "QuickRnd",
        vec![Value::Integer { value: -1 }],
        Object::Named { string: 0 },
    );
    let mut random = EngineRandom::default();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (index, expected) in [(1, 16838), (0, 5758)] {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].runtime().unwrap().variables(),
            [expected]
        );
    }
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [16838]);
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    replacement
        .advance(
            replacement.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(
        replacement.actors()[0].runtime().unwrap().variables(),
        [10113]
    );
    assert_eq!(random.state(), 662824084);
}

#[test]
fn random_handler_requires_shared_state_and_invalid_bounds_fail_stickily() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    for (function, bound, supplied, message) in [
        (
            "QuickRnd",
            127,
            false,
            "shared engine random stream unavailable",
        ),
        ("Rnd", 0, true, "positive bound"),
        ("Rnd", -1, true, "positive bound"),
    ] {
        let scripts = random_scripts(function, vec![Value::Integer { value: bound }], Object::Me);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let mut random = EngineRandom::default();
        let id = world.id(0).unwrap();
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                supplied.then_some(&mut random),
                20,
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains(message), "{error}");
        assert!(world.actors()[0].failure().is_some());
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
        assert_eq!(random.state(), 1);
        assert!(
            world
                .advance(
                    id,
                    ScriptStage::Body(Thread::Main),
                    Default::default(),
                    None,
                    Some(&mut random),
                    20
                )
                .is_err()
        );
        assert_eq!(random.state(), 1);
    }
}

#[test]
fn rnd_script_masks_before_reducing_and_consumes_one_draw() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    let scripts = random_scripts("Rnd", vec![Value::Integer { value: 3 }], Object::Me);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut random = EngineRandom::default();
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [2]);
    assert_eq!(random.state(), 1103527590);
}

fn wander_world(radius: i32, movement: i32, maps: serde_json::Value) -> ScriptWorld {
    use cyrus_assets::animation::Animations;
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "MoveByAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        },
        Function {
            name: "Wander".into(),
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    scripts.instances[0].map_position = [1000, 2, 2000];
    let mut operations = Vec::new();
    for (axis, delta) in [(0, movement), (1, 256)] {
        operations.push(wait(
            1,
            "MoveByAxis",
            CallKind::Multitask,
            vec![
                Value::Integer { value: axis },
                Value::Integer { value: delta },
                Value::Integer { value: 1 },
            ],
        ));
    }
    operations.push(Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 2,
                    function: "Wander".into(),
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![
                        Value::Integer { value: radius },
                        Value::Integer { value: 32768 },
                    ],
                }),
            },
            rest: vec![],
        },
    });
    operations.push(Operation::End { target: 0 });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id, "walk_node_maps": maps}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.prepare_spatial().unwrap();
    world
}

#[test]
fn empty_map_wander_consumes_original_draws_and_restores_excess_displacement() {
    use cyrus_game::{
        random::EngineRandom,
        script::{ScriptInputs, ScriptStage},
    };
    for (radius, movement, scale, state, restore) in [
        (4, 0, 256, 2524885223, false),
        (5, 0, 256, 3295386429, false),
        (1204, 0, 256, 2524885223, false),
        (1204, 8448, 256, 2524885223, true),
        (1204, 8192, 256, 2524885223, false),
        (1204, 256, 0, 2524885223, true),
    ] {
        let mut world = wander_world(radius * 256, movement, serde_json::json!([]));
        let id = world.id(0).unwrap();
        let mut random = EngineRandom::default();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(scale),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [movement]);
        assert_eq!(random.state(), state, "radius {radius}");
        assert_eq!(world.actors()[0].attributes()[94], 1);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if restore {
                [256000, 512, 512000]
            } else {
                [256000 + movement, 768, 512000]
            }
        );
    }
}

#[test]
fn wander_rejects_nonempty_maps_and_zero_radius_without_consuming_random() {
    use cyrus_game::{
        random::EngineRandom,
        script::{ScriptInputs, ScriptStage},
    };
    for (maps, radius, expected) in [
        (serde_json::json!([{}]), 1024, "walk-node maps unsupported"),
        (serde_json::json!([]), 255, "positive map-unit radius"),
    ] {
        let mut world = wander_world(radius, 0, maps);
        let id = world.id(0).unwrap();
        let mut random = EngineRandom::default();
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(random.state(), 1);
        assert!(world.actors()[0].failure().is_some());
    }
}

#[test]
fn visibility_calls_return_zero_without_disabling_the_receivers_script() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (function, hidden) in [("HideMe", true), ("ShowMe", false), ("isDead", false)] {
        let mut scripts = random_scripts(function, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        for _ in 0..2 {
            world
                .advance(
                    id,
                    ScriptStage::OrdinaryMain,
                    ScriptInputs {
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!(control.hidden, hidden);
            assert!(!control.object_update_excluded);
            assert_eq!(control.script_control, 0);
            assert!(!world.actors()[1].control_state().unwrap().hidden);
        }
    }
}

#[test]
fn automatic_world_updates_advance_authored_actors_and_pause_in_menu() {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    use cyrus_assets::animation::Animations;
    use cyrus_game::{
        AppState,
        script::{ScriptActors, ScriptPlugin, ScriptView},
    };
    use cyrus_world::{
        animation::{AnimationRequest, AnimationTick},
        lifecycle::PreparedWorld,
    };
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = 1;
        placement.active = 1;
    }
    scripts.definition.functions.push(Function {
        name: "PlayerFacing".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&1)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    function: "PlayerFacing".into(),
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![Value::Integer { value: 0 }, Value::Integer { value: 0 }],
                }),
            },
            rest: vec![],
        },
    };
    scripts.definition.functions.push(Function {
        name: "PlayerDistance".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.definition.flags.push(Flag {
        name: "Distance".into(),
        flag_type: "NUMBER".into(),
        initial_value: -1,
    });
    let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
    program.steps.insert(
        3,
        Step {
            next: 4,
            operation: Operation::End { target: 0 },
        },
    );
    program.steps.insert(
        2,
        Step {
            next: 3,
            operation: Operation::Assign {
                destination: Place::Flag { index: 1 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(Call {
                            function_id: 2,
                            function: "PlayerDistance".into(),
                            target: Object::Me,
                            kind: CallKind::Function,
                            arguments: vec![],
                        }),
                    },
                    rest: vec![],
                },
            },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id, "markers": [], "walk_node_maps": [],
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},
                        {"opcode":0,"frame_reference":11}, {"opcode":0,"frame_reference":12},
                        {"opcode":0,"frame_reference":13}, {"opcode":2,"value":0}]}]}]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .add_message::<AnimationTick>()
        .add_plugins(ScriptPlugin);
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.insert_resource(PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(7),
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: [256000, 0, 256000],
    });
    let player = app
        .world_mut()
        .spawn((
            cyrus_game::player::Player,
            cyrus_game::player::placement::PlayerPose {
                position: [256, 512, 768],
                heading: 0,
            },
        ))
        .id();
    app.world_mut()
        .spawn(cyrus_game::player::placement::PlayerPose {
            position: [999, 999, 999],
            heading: 1024,
        });
    app.update();
    let snapshot = |app: &App| {
        let actors = app.world().resource::<ScriptActors>();
        actors
            .current
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "script preparation failed: {:?}",
                    app.world().resource::<cyrus_game::script::ScriptStatus>()
                )
            })
            .inspect(0, 2)
    };
    assert_eq!(
        snapshot(&app)["actors"][0]["animation"],
        serde_json::Value::Null
    );
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let first = snapshot(&app);
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [1, 1536]
    );
    app.world_mut()
        .get_mut::<cyrus_game::player::placement::PlayerPose>(player)
        .unwrap()
        .heading = 1024;
    assert_eq!(first["actors"][0]["failure"], serde_json::Value::Null);
    assert_eq!(first["actors"][0]["locals"], serde_json::json!([5]));
    assert!(first["actors"][0]["animation"]["frame"].is_number());
    app.update();
    let next = snapshot(&app);
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [0, 1536]
    );
    assert_eq!(next["actors"][0]["locals"], serde_json::json!([6]));
    assert_ne!(
        next["actors"][0]["animation"]["frame"],
        first["actors"][0]["animation"]["frame"]
    );
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    app.update();
    app.update();
    assert_eq!(snapshot(&app), next);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    assert_eq!(
        snapshot(&app)["actors"][0]["locals"],
        serde_json::json!([7])
    );
    app.init_resource::<cyrus_console::Console>();
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = true;
    let before_console = snapshot(&app);
    let before_clock = app
        .world()
        .resource::<cyrus_game::clock::SimulationClock>()
        .step();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(60),
    ));
    for _ in 0..3 {
        app.update();
        assert_eq!(snapshot(&app), before_console);
        assert_eq!(
            app.world()
                .resource::<cyrus_game::clock::SimulationClock>()
                .step(),
            before_clock
        );
    }
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = false;
    app.update();
    assert_eq!(
        snapshot(&app)["actors"][0]["locals"],
        serde_json::json!([8])
    );
    assert_eq!(
        app.world()
            .resource::<cyrus_game::clock::SimulationClock>()
            .step()
            .frame_scale,
        before_clock.frame_scale
    );
    app.insert_resource(bevy::time::TimeUpdateStrategy::Automatic);
    let prepared = app.world().resource::<PreparedWorld>();
    let replacement = PreparedWorld {
        requested_name: prepared.requested_name.clone(),
        world: prepared.world,
        scripts: prepared.scripts.clone(),
        animations: prepared.animations.clone(),
        player_path: prepared.player_path.clone(),
        scene_path: prepared.scene_path.clone(),
        environment_path: prepared.environment_path.clone(),
    };
    let mut changed_scripts = app
        .world()
        .resource::<Assets<Scripts>>()
        .get(&replacement.scripts)
        .unwrap()
        .clone();
    changed_scripts.player_distance_override = true;
    changed_scripts.content_id = changed_scripts.content_identity().unwrap();
    let mut changed_animations = Animations {
        version: 2,
        world: changed_scripts.world,
        metadata: app
            .world()
            .resource::<Assets<Animations>>()
            .get(&replacement.animations)
            .unwrap()
            .metadata
            .clone(),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    changed_animations.metadata["script_content_id"] = changed_scripts.content_id.clone().into();
    let mut replacement = replacement;
    replacement.scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(changed_scripts);
    replacement.animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(changed_animations);
    app.insert_resource(AnimationRequest(replacement.animations.clone()));
    app.insert_resource(ScriptView {
        source: replacement.animations.clone(),
        position: [256000, 0, 256000],
    });
    app.insert_resource(replacement);
    app.update();
    let replaced = snapshot(&app);
    assert_ne!(replaced["generation"], first["generation"]);
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [0, 9_999_999]
    );
    assert_eq!(replaced["actors"][0]["locals"], serde_json::json!([5]));
    struct EmptyCheatStore;
    impl cyrus_game::cheats::CheatStore for EmptyCheatStore {
        fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
            Ok(cyrus_assets::cheats::empty())
        }
        fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
            Ok(())
        }
    }
    app.insert_resource(cyrus_game::cheats::Cheats::new(EmptyCheatStore));
    for (value, expected) in [(0, 1536), (1, 9_999_999), (0, 1536)] {
        app.world_mut()
            .resource_mut::<cyrus_game::cheats::Cheats>()
            .execute("neversaydie", Some(value))
            .unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .flags()[1],
            expected
        );
    }
}

#[test]
fn placed_distance_queries_use_receiver_owned_initial_link_state() {
    use cyrus_game::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.instances[1].map_position = [10, 0, 0];
    scripts.definition.functions.push(Function {
        name: "PlayerDistance".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    function: "PlayerDistance".into(),
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    for (index, expected) in [(0, 256), (1, 2304)] {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [256, 0, 0],
                            heading: 0,
                        }),
                        distance_override: Some(false),
                        fallback_position: None,
                        linked_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [expected]);
    }
}

#[test]
fn deep_water_query_reads_receivers_stored_surface_without_a_support_query() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "IsInDeepWater".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: "IsInDeepWater".into(),
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    for surface in 0..=u8::MAX {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let first = world.id(0).unwrap();
        let second = world.id(1).unwrap();
        world.set_actor_surface(first, surface).unwrap();
        world.step(first, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [i32::from(surface == 2 || surface == 4)]);
        world.step(second, Thread::Main, 20).unwrap();
        assert_eq!(
            world.flags(),
            [0],
            "another actor retains its allocation surface"
        );
        assert!(
            world
                .set_actor_surface(
                    cyrus_game::script::ScriptActorId {
                        generation: 2,
                        index: 0
                    },
                    surface
                )
                .is_err()
        );
    }
}

#[test]
fn fresh_player_deep_water_query_has_zero_surface_before_grounded_updates() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "IsInDeepWater".into(),
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
                    function: "IsInDeepWater".into(),
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
    let player = world.id(0).unwrap();
    world.step(player, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [0]);
    assert!(world.actors()[0].failure().is_none());
}

#[test]
fn object_axis_task_moves_in_receiver_orientation_and_does_not_snap_at_target() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (axis, angles, delta, expected) in [
        (0, [0, 0, 0], 512, [256, 0, 0]),
        (1, [0, 0, 0], -512, [0, -256, 0]),
        (2, [0, 512, 0], 512, [-256, 0, 0]),
        (2, [512, 0, 0], 512, [0, 256, 0]),
        (0, [0, 0, 512], 512, [0, -256, 0]),
        (9, [0, 0, 0], 512, [0, 0, 0]),
    ] {
        let mut scripts = cache();
        scripts.definition.functions.push(Function {
            name: "MoveObjectAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts.instances[0].angles = angles;
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
            "MoveObjectAxis",
            CallKind::Task,
            vec![
                Value::Integer { value: axis },
                Value::Integer { value: delta },
                Value::Integer { value: 2 },
            ],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
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
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world
            .advance(
                id,
                ScriptStage::Tasks,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            world.spatial(world.id(1).unwrap()).unwrap().local.position,
            [0; 3]
        );
    }
}

#[test]
fn deferred_movement_relocates_cells_and_retains_nearby_cached_order() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (excluded, distant_view) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut scripts = cache();
        scripts.instances[0].map_position = [1024, 0, 512];
        scripts.instances[1].map_position = [512, 0, 512];
        scripts.definition.functions.push(Function {
            name: "MoveObjectAxis".into(),
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
            "MoveObjectAxis",
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
fn positional_sound_uses_the_routed_receivers_pose_and_returns_zero() {
    use cyrus_game::script::{ScriptSound, ScriptStage};
    let mut scripts = random_scripts(
        "Sound",
        vec![
            Value::Integer { value: 7 },
            Value::Integer { value: -40 },
            Value::Integer { value: 2000 },
        ],
        Object::Named { string: 0 },
    );
    scripts.definition.functions.last_mut().unwrap().param_count = 3;
    scripts.instances[0].map_position = [3, 4, 5];
    scripts.instances[1].map_position = [10, 20, 30];
    scripts.content_id = scripts.content_identity().unwrap();
    for surface in [0, 1, 6, 7, 12, 255] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.refresh_families().unwrap();
        let caller = world.id(1).unwrap();
        let receiver = world.id(0).unwrap();
        world.set_actor_surface(receiver, surface).unwrap();
        let mut cues = Vec::new();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                cyrus_game::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                Some(&mut |actor, sound| {
                    cues.push((actor, sound));
                    Ok(())
                }),
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            cues,
            [(
                receiver,
                ScriptSound::Positional {
                    effect: 7,
                    volume: -40,
                    rate_offset_hz: 2000,
                    position: [768, 1024, 1280],
                }
            )]
        );
        assert_eq!(world.actors()[1].runtime().unwrap().variables(), [0]);
    }
}
