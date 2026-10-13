//! Shared actor host contracts use synthetic scripts and explicit stage timing.
use cyrus_assets::{animation::Animations, script::*};
use cyrus_game::script::ScriptSound;
use cyrus_game::script::{ScriptActorId, ScriptStage, ScriptWorld};
use cyrus_script::Thread;
use cyrus_world::animation::AnimationSound;
use serde_json::json;
use std::collections::BTreeMap;

fn call(function_id: u16, kind: CallKind, target: Object) -> Operation {
    let (function, arguments) = match function_id {
        1 => (
            "PushAnimation",
            vec![Value::Integer { value: 7 }, Value::Integer { value: 1 }],
        ),
        2 => ("Wait", vec![Value::Integer { value: 2 }]),
        _ => unreachable!(),
    };
    let local = target == Object::Me;
    let call = Call {
        function_id,
        kind,
        target,
        function: function.into(),
        arguments,
    };
    if local {
        Operation::Call { call }
    } else {
        Operation::ObjectCall { call }
    }
}
fn flag() -> Operation {
    Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Integer { value: 9 },
            rest: vec![],
        },
    }
}
fn scripts(operations: Vec<Operation>) -> Scripts {
    let actor = |name: &str, operations: Vec<Operation>| Actor {
        header_index: 0,
        code_origin: 0,
        instances: 1,
        namespace: Some(name.into()),
        instance_variables: vec![vec![]],
        attributes: BTreeMap::new(),
        native_hook: None,
        program: Program {
            version: 2,
            entry: 0,
            variables: vec![],
            strings: vec!["OTHER".into()],
            steps: operations
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
                .collect(),
        },
    };
    let mut result = Scripts {
        version: 7,
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 10,
        content_id: String::new(),
        world: 17,
        definition: Definition {
            functions: [
                ("NullFunction", 0, CallKind::Function),
                ("PushAnimation", 2, CallKind::Task),
                ("Wait", 1, CallKind::Task),
            ]
            .into_iter()
            .map(|(name, param_count, kind)| Function {
                name: name.into(),
                param_count,
                kind,
            })
            .collect(),
            flags: vec![Flag {
                name: "Ready".into(),
                flag_type: "NUMBER".into(),
                initial_value: 0,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([
            ("ACTOR".into(), actor("ACTOR", operations)),
            (
                "OTHER".into(),
                actor("OTHER", vec![Operation::End { target: 0 }]),
            ),
        ]),
        instances: ["ACTOR", "OTHER"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| Instance {
                object_id: index as u32,
                actor: name.into(),
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
    result.actors.get_mut("OTHER").unwrap().code_origin = 100;
    result.content_id = result.content_identity().unwrap();
    result
}
fn animations(scripts: &Scripts, sound: bool) -> Animations {
    let mut commands = vec![
        json!({"opcode":0,"frame_reference":1}),
        json!({"opcode":2,"value":0}),
        json!({"opcode":1}),
    ];
    if sound {
        commands.insert(0, json!({"opcode":4,"sound_param":-17,"volume_shift":-1}));
    }
    Animations {
        version: 2,
        world: scripts.world,
        sections: BTreeMap::new(),
        models: BTreeMap::new(),
        unsupported: BTreeMap::new(),
        metadata: json!({"script_content_id": scripts.content_id, "actors": (["ACTOR", "OTHER"].map(|name| json!({"script_name":name,"animation_groups":[
            {"group_index":1,"anim_type":0,"anim_id":1,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
            {"group_index":7,"anim_type":0,"anim_id":1,"commands":commands}
        ]})))}),
    }
}
fn stage(world: &mut ScriptWorld, id: ScriptActorId, stage: ScriptStage) {
    world
        .advance(
            id,
            stage,
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            30,
        )
        .unwrap();
}
const BODY: ScriptStage = ScriptStage::Body(Thread::Main);
const TASKS: ScriptStage = ScriptStage::Tasks;

#[test]
fn animation_wait_and_flags_share_one_runtime_and_separate_stages() {
    let scripts = scripts(vec![
        call(1, CallKind::Multitask, Object::Me),
        call(2, CallKind::Task, Object::Me),
        flag(),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, BODY);
    assert_eq!(world.animation_frame(id).unwrap(), Some(1));
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
    assert_eq!(world.flags(), [0]);
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    stage(&mut world, id, BODY);
    assert_eq!(world.flags(), [9]);
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.animation_frame(id).unwrap(), Some(0));
}

#[test]
fn routed_animation_owns_receiver_task_and_emits_receiver_identity_synchronously() {
    let scripts = scripts(vec![
        call(1, CallKind::Task, Object::Named { string: 0 }),
        flag(),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 4).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    world
        .bind_animation(receiver, &animations(&scripts, true), 1)
        .unwrap();
    stage(&mut world, receiver, ScriptStage::Animation);
    let mut events = Vec::new();
    world
        .advance(
            caller,
            BODY,
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |id, sound| {
                events.push((id, sound));
                Ok(())
            }),
            30,
        )
        .unwrap();
    assert_eq!(
        events,
        [(
            receiver,
            ScriptSound::Animation(AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            })
        )]
    );
    assert_eq!(world.animation_frame(receiver).unwrap(), Some(1));
    assert_eq!(world.animation_frame(caller).unwrap(), None);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.flags(), [9]);
}

#[test]
fn pause_resume_and_stop_keep_wait_and_animation_tokens_distinct() {
    let scripts = scripts(vec![
        call(1, CallKind::Multitask, Object::Me),
        call(2, CallKind::Multitask, Object::Me),
        Operation::TaskPause { label: 0 },
        Operation::TaskPause { label: 1 },
        Operation::Yield,
        Operation::TaskResume { label: 1 },
        Operation::TaskStop { label: 0 },
        Operation::Yield,
        flag(),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    stage(&mut world, id, BODY);
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
    stage(&mut world, id, BODY);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.animation_frame(id).unwrap(), Some(1));
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    stage(&mut world, id, BODY);
    assert_eq!(world.flags(), [9]);
}

#[test]
fn missing_ticks_and_stale_or_mismatched_identity_fail_before_mutation() {
    let scripts = scripts(vec![
        call(1, CallKind::Task, Object::Me),
        flag(),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 2).unwrap();
    let id = world.id(0).unwrap();
    let mut asset = animations(&scripts, false);
    asset.world += 1;
    assert!(
        world
            .bind_animation(id, &asset, 1)
            .unwrap_err()
            .to_string()
            .contains("belongs to world")
    );
    asset.world = scripts.world;
    asset.metadata["script_content_id"] = "wrong".into();
    assert!(
        world
            .bind_animation(id, &asset, 1)
            .unwrap_err()
            .to_string()
            .contains("generation mismatch")
    );
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    for stage in [BODY, TASKS, ScriptStage::Animation] {
        assert!(
            world
                .advance(
                    id,
                    stage,
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: None,
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    30
                )
                .unwrap_err()
                .to_string()
                .contains("nominal_ticks")
        );
        assert!(
            world
                .advance(
                    ScriptActorId {
                        generation: 1,
                        ..id
                    },
                    stage,
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    30
                )
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
    }
    let unbound = world.id(1).unwrap();
    assert!(
        world
            .advance(
                unbound,
                BODY,
                cyrus_game::script::ScriptInputs {
                    nominal_ticks: None,
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                30
            )
            .unwrap_err()
            .to_string()
            .contains("nominal_ticks")
    );
    assert!(world.actors()[1].failure().is_none());
    assert_eq!(world.flags(), [0]);
    assert_eq!(world.animation_frame(id).unwrap(), None);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(
        world.actors()[0]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    stage(&mut world, id, BODY);
    assert_eq!(world.animation_frame(id).unwrap(), Some(1));
}

#[test]
fn animation_stage_sound_failure_stops_later_effects_and_is_visible() {
    let scripts = scripts(vec![flag(), Operation::End { target: 0 }]);
    let mut data = animations(&scripts, true);
    data.metadata["actors"][0]["animation_groups"][0]["commands"] = json!([
        {"opcode":4,"sound_param":-17,"volume_shift":-1},
        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.bind_animation(id, &data, 1).unwrap();
    let mut calls = 0;
    let mut emit = |_, _| -> anyhow::Result<()> {
        calls += 1;
        anyhow::bail!("sound unavailable")
    };
    let error = world
        .advance(
            id,
            ScriptStage::Animation,
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut emit),
            30,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("world 17 instance 0 animation stage"));
    assert!(error.contains("sound unavailable"));
    assert_eq!(
        world
            .advance(
                id,
                BODY,
                cyrus_game::script::ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut emit),
                30
            )
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(
        world
            .advance(
                id,
                ScriptStage::Animation,
                cyrus_game::script::ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut emit),
                30
            )
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(calls, 1);
    assert_eq!(world.flags(), [0]);
    assert_eq!(world.actors()[0].failure(), Some(error.as_str()));
}

#[test]
fn changing_count_after_animation_task_started_does_not_reset_finite_playback() {
    let mut start = call(1, CallKind::Multitask, Object::Me);
    let Operation::Call { call } = &mut start else {
        unreachable!()
    };
    call.arguments[1] = Value::Read {
        place: Place::Local { index: 0 },
    };
    let mut scripts = scripts(vec![
        start,
        Operation::Yield,
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Integer { value: 100 },
                rest: vec![],
            },
        },
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    let actor = scripts.actors.get_mut("ACTOR").unwrap();
    actor.program.variables = vec![1];
    actor.instance_variables = vec![vec![1]];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    stage(&mut world, id, BODY);
    stage(&mut world, id, TASKS);
    stage(&mut world, id, BODY);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [100]);
    stage(&mut world, id, TASKS);
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.animation_frame(id).unwrap(), Some(0));
}

#[test]
fn unsupported_animation_group_is_not_narrowed_into_a_valid_request() {
    let mut start = call(1, CallKind::Task, Object::Me);
    let Operation::Call { call } = &mut start else {
        unreachable!()
    };
    call.arguments[0] = Value::Integer { value: 65_543 };
    let scripts = scripts(vec![start, Operation::End { target: 0 }]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    let error = world
        .advance(
            id,
            BODY,
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            30,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("PushAnimation group outside supported range"));
    assert_eq!(world.animation_frame(id).unwrap(), None);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn simultaneous_animation_tasks_share_playback_but_complete_independently() {
    let scripts = scripts(vec![
        call(1, CallKind::Multitask, Object::Me),
        call(1, CallKind::Multitask, Object::Me),
        Operation::Yield,
        flag(),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    stage(&mut world, id, BODY);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, ScriptStage::Animation);
    stage(&mut world, id, TASKS);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    stage(&mut world, id, BODY);
    assert_eq!(world.flags(), [9]);
}

#[test]
fn routed_animation_failure_is_reported_on_both_receiver_and_caller() {
    let scripts = scripts(vec![
        call(1, CallKind::Task, Object::Named { string: 0 }),
        flag(),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    world
        .bind_animation(receiver, &animations(&scripts, true), 1)
        .unwrap();
    let mut calls = 0;
    let mut fail = |_: ScriptActorId, _: ScriptSound| {
        calls += 1;
        anyhow::bail!("sound sink failed")
    };
    assert!(
        world
            .advance(
                caller,
                BODY,
                cyrus_game::script::ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut fail),
                30
            )
            .is_err()
    );
    for id in [caller, receiver] {
        assert!(
            world.actors()[id.index]
                .failure()
                .unwrap()
                .contains("sound sink failed")
        );
        assert!(
            world
                .advance(
                    id,
                    ScriptStage::Animation,
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    Some(&mut fail),
                    30
                )
                .is_err()
        );
    }
    assert_eq!(calls, 1);
    assert_eq!(world.flags(), [0]);
}

#[test]
fn missing_animation_group_keeps_requesting_fallback_without_completing_task() {
    let mut request = call(1, CallKind::Task, Object::Me);
    let Operation::Call { call } = &mut request else {
        unreachable!()
    };
    call.arguments[0] = Value::Integer { value: 8 };
    let scripts = scripts(vec![request, flag(), Operation::End { target: 0 }]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &animations(&scripts, false), 1)
        .unwrap();
    stage(&mut world, id, BODY);
    for _ in 0..3 {
        stage(&mut world, id, ScriptStage::Animation);
        stage(&mut world, id, TASKS);
        stage(&mut world, id, BODY);
        assert_eq!(world.animation_frame(id).unwrap(), Some(0));
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(world.flags(), [0]);
    }
}

fn axis_task_scripts(function: &str, axis: i32, delta: i32, duration: i32) -> Scripts {
    let mut data = scripts(vec![
        Operation::Call {
            call: Call {
                function_id: 3,
                kind: CallKind::Task,
                target: Object::Me,
                function: function.into(),
                arguments: [axis, delta, duration]
                    .map(|value| Value::Integer { value })
                    .to_vec(),
            },
        },
        flag(),
        Operation::End { target: 0 },
    ]);
    data.definition.functions.push(Function {
        name: function.into(),
        param_count: 3,
        kind: CallKind::Task,
    });
    data.content_id = data.content_identity().unwrap();
    data
}

#[test]
fn rotation_changes_local_angles_and_completes_before_body_continues() {
    let data = axis_task_scripts("RotateByAxis", 1, 128, 4);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    stage(&mut world, id, BODY);
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 32, 0]);
    assert_eq!(world.flags(), [0]);
    for expected in [64, 96, 128] {
        stage(&mut world, id, TASKS);
        assert_eq!(world.spatial(id).unwrap().local.angles, [0, expected, 0]);
    }
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    stage(&mut world, id, BODY);
    assert_eq!(world.flags(), [9]);
}

#[test]
fn rotation_zero_scale_and_nominal_tick_fallback_remain_distinct() {
    use cyrus_game::script::ScriptInputs;
    let data = axis_task_scripts("RotateByAxis", -1, -128, 4);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let zero = ScriptInputs {
        frame_scale: Some(0),
        nominal_ticks: Some(0),
        ..Default::default()
    };
    world.advance(id, BODY, zero, None, 30).unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0; 3]);
    let tick = ScriptInputs {
        nominal_ticks: Some(256),
        ..zero
    };
    world.advance(id, TASKS, tick, None, 30).unwrap();
    // The fallback sign follows the saved target, not the negative delta.
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 0, 1]);
    world.advance(id, TASKS, zero, None, 30).unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 0, 1]);
}

#[test]
fn spatial_timing_is_checked_before_script_mutation() {
    use cyrus_game::script::ScriptInputs;
    let data = axis_task_scripts("RotateByAxis", 0, 128, 4);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    assert!(
        world
            .advance(id, BODY, ScriptInputs::default(), None, 30)
            .is_err()
    );
    assert_eq!(world.spatial(id).unwrap().local.angles, [0; 3]);
    assert!(world.actors()[0].failure().is_none());
    stage(&mut world, id, BODY);
    assert_eq!(world.spatial(id).unwrap().local.angles, [32, 0, 0]);
}

#[test]
fn routed_rotation_keeps_receiver_angles_and_task_slots() {
    let mut data = axis_task_scripts("RotateByAxis", 1, 128, 4);
    let operation = &mut data
        .actors
        .get_mut("ACTOR")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation;
    let Operation::Call { call } = operation else {
        unreachable!()
    };
    call.target = Object::Named { string: 0 };
    *operation = Operation::ObjectCall { call: call.clone() };
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    stage(&mut world, caller, BODY);
    assert_eq!(world.spatial(caller).unwrap().local.angles, [0; 3]);
    assert_eq!(world.spatial(receiver).unwrap().local.angles, [0, 32, 0]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    stage(&mut world, receiver, TASKS);
    assert_eq!(world.spatial(receiver).unwrap().local.angles, [0, 64, 0]);
}

#[test]
fn attached_facing_stays_composed_until_family_refresh() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    use cyrus_script::Argument;
    let mut data = axis_task_scripts("RotateByAxis", 1, 128, 4);
    data.actors
        .get_mut("ACTOR")
        .unwrap()
        .attributes
        .insert("attr_26".into(), 1);
    data.actors
        .get_mut("OTHER")
        .unwrap()
        .attributes
        .insert("attr_25".into(), 1);
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let id = world.id(0).unwrap();
    world.snapshot_spatial(id).unwrap();
    stage(&mut world, id, BODY);
    let query = QueryInputs {
        player: Some(PlayerQuery {
            position: [0; 3],
            heading: 0,
        }),
        distance_override: None,
        fallback_position: None,
        linked_position: None,
    };
    let args = [Argument::Integer(0), Argument::Integer(0)];
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 32);
    assert_eq!(
        world
            .spatial(id)
            .unwrap()
            .query("PlayerFacing", &args, query)
            .unwrap(),
        Some(1)
    );
    world.refresh_families().unwrap();
    assert_eq!(world.spatial(id).unwrap().effective.unwrap().angles[1], 32);
    assert_eq!(
        world
            .spatial(id)
            .unwrap()
            .query("PlayerFacing", &args, query)
            .unwrap(),
        Some(0)
    );
}

#[test]
fn cancelling_rotation_retains_last_local_angle() {
    let mut data = axis_task_scripts("RotateByAxis", 1, 128, 4);
    let actor = data.actors.get_mut("ACTOR").unwrap();
    let Operation::Call { call } = &mut actor.program.steps.get_mut(&0).unwrap().operation else {
        unreachable!()
    };
    call.kind = CallKind::Multitask;
    actor.program.steps.get_mut(&1).unwrap().operation = Operation::TaskStop { label: 0 };
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    stage(&mut world, id, BODY);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 32);
    stage(&mut world, id, TASKS);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 32);
}

#[test]
fn rotation_reevaluates_delta_without_replacing_saved_target() {
    let mut data = axis_task_scripts("RotateByAxis", 1, 128, 4);
    data.definition.flags[0].initial_value = 128;
    let actor = data.actors.get_mut("ACTOR").unwrap();
    let Operation::Call { call } = &mut actor.program.steps.get_mut(&0).unwrap().operation else {
        unreachable!()
    };
    call.kind = CallKind::Multitask;
    call.arguments[1] = Value::Read {
        place: Place::Flag { index: 0 },
    };
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    stage(&mut world, id, BODY);
    assert_eq!(world.flags(), [9]);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 32);
    stage(&mut world, id, TASKS);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 34);
    for _ in 0..41 {
        stage(&mut world, id, TASKS);
    }
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 126);
    stage(&mut world, id, TASKS);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 128);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn routed_sound_commands_keep_receiver_and_authored_operands() {
    let definitions = [
        ("FlatSound", vec![12, -4, -10000]),
        ("EndMySound", vec![12]),
        ("EndSound", vec![12]),
        ("EndMySounds", vec![]),
    ];
    let mut data = scripts(vec![Operation::End { target: 0 }]);
    let mut operations = Vec::new();
    data.definition.flags = (0..definitions.len())
        .map(|index| Flag {
            name: format!("Result{index}"),
            flag_type: "NUMBER".into(),
            initial_value: 99,
        })
        .collect();
    for (index, (name, values)) in definitions.iter().enumerate() {
        let function_id = data.definition.functions.len() as u16;
        data.definition.functions.push(Function {
            name: (*name).into(),
            param_count: values.len() as u8,
            kind: CallKind::Function,
        });
        operations.push(Operation::Assign {
            destination: Place::Flag {
                index: index as u16,
            },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id,
                        kind: CallKind::Function,
                        target: Object::Named { string: 0 },
                        function: (*name).into(),
                        arguments: values
                            .iter()
                            .map(|value| Value::Integer { value: *value })
                            .collect(),
                    }),
                },
                rest: vec![],
            },
        });
    }
    operations.push(Operation::End { target: 0 });
    data.actors.get_mut("ACTOR").unwrap().program.steps = operations
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
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 18).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    let mut events = Vec::new();
    world
        .advance(
            caller,
            ScriptStage::Body(Thread::Main),
            Default::default(),
            Some(&mut |id, sound| {
                events.push((id, sound));
                Ok(())
            }),
            100,
        )
        .unwrap();
    assert_eq!(world.flags(), [0, 0, 0, 0]);
    assert_eq!(
        events,
        vec![
            (
                receiver,
                ScriptSound::Flat {
                    effect: 12,
                    volume: -4,
                    rate_offset_hz: -10000
                }
            ),
            (
                receiver,
                ScriptSound::StopEffect {
                    effect: 12,
                    owned: true
                }
            ),
            (
                receiver,
                ScriptSound::StopEffect {
                    effect: 12,
                    owned: false
                }
            ),
            (receiver, ScriptSound::StopOwned),
        ]
    );
}

fn activation_world(function: &str) -> (ScriptWorld, cyrus_game::script::ActivationInputs) {
    activation_world_with_attributes(function, BTreeMap::new(), BTreeMap::new())
}

fn activation_world_with_attributes(
    function: &str,
    receiver_attributes: BTreeMap<String, u8>,
    player_attributes: BTreeMap<String, u8>,
) -> (ScriptWorld, cyrus_game::script::ActivationInputs) {
    use cyrus_game::script::{ActivationActor, ActivationInputs};
    let mut data = scripts(vec![Operation::End { target: 0 }]);
    for (name, count) in [(function, 1), ("Deactivate", 0)] {
        data.definition.functions.push(Function {
            name: name.into(),
            param_count: count,
            kind: CallKind::Function,
        });
    }
    let mut player = data.actors["OTHER"].clone();
    player.namespace = Some("CYRUS".into());
    player.code_origin = 200;
    player.attributes = player_attributes;
    data.actors.insert("CYRUS".into(), player);
    data.actors.get_mut("OTHER").unwrap().attributes = receiver_attributes;
    for name in ["ACTOR", "OTHER"] {
        let actor = data.actors.get_mut(name).unwrap();
        if name == "OTHER" {
            actor.program.strings[0] = "ACTOR".into();
        }
        actor.program.steps = [
            Operation::Assign {
                destination: Place::Flag { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(Call {
                            function_id: 3,
                            kind: CallKind::Function,
                            target: Object::Named { string: 0 },
                            function: function.into(),
                            arguments: vec![Value::Integer {
                                value: i32::from_le_bytes(*b"#OPN"),
                            }],
                        }),
                    },
                    rest: vec![],
                },
            },
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
        actor.program.entry = 2;
        actor.program.steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::Call {
                    call: Call {
                        function_id: 4,
                        kind: CallKind::Function,
                        target: Object::Me,
                        function: "Deactivate".into(),
                        arguments: vec![],
                    },
                },
            },
        );
        actor.program.steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::End { target: 2 },
            },
        );
    }
    data.content_id = data.content_identity().unwrap();
    let world = ScriptWorld::prepare(&data, 7).unwrap();
    let actor = ActivationActor {
        state: 0,
        on_rope: false,
        water_state: 0,
        torch_burning: 0,
        weapon_assigned: false,
        weapon_drawn: false,
        animation: Some(cyrus_game::script::ActivationAnimation { group: Some(0) }),
    };
    let inputs = ActivationInputs {
        player: world.id(2).unwrap(),
        actors: (0..3).map(|index| (index, actor)).collect(),
        labels: BTreeMap::from([("#opn".into(), "Open".into())]),
        lockout: 0,
        dialogue_open: false,
        lineup_result: 0,
        use_held: true,
        item_system_ready: true,
        shell_execution: false,
        render_disabled: false,
    };
    (world, inputs)
}

fn activate_world(world: &mut ScriptWorld, inputs: &cyrus_game::script::ActivationInputs) {
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Interrupt),
            cyrus_game::script::ScriptInputs {
                activation: Some(inputs),
                ..Default::default()
            },
            None,
            30,
        )
        .unwrap();
}

#[test]
fn routed_activation_retains_receiver_owner_and_prompt_until_drawn() {
    use cyrus_game::script::ActivationOverlay;
    let (mut world, inputs) = activation_world("Activate");
    activate_world(&mut world, &inputs);
    assert_eq!(world.flags(), [1]);
    assert_eq!(world.activation().owner(), Some(world.id(1).unwrap()));
    assert_eq!(world.activation().prompt(), Some("Open"));
    assert_eq!(
        world.activation_overlay(&inputs, 22, false, 24).unwrap(),
        ActivationOverlay::Hidden
    );
    assert_eq!(world.activation().prompt(), Some("Open"));
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Prompt("Open".into())
    );
    assert_eq!(world.activation().prompt(), None);
    activate_world(&mut world, &inputs);
    assert_eq!(world.flags(), [1], "the existing owner may activate again");
    // Deactivation on the caller clears holdoff but cannot release a foreign owner.
    world
        .advance(world.id(0).unwrap(), BODY, Default::default(), None, 30)
        .unwrap();
    assert_eq!(world.activation().owner(), Some(world.id(1).unwrap()));
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Prompt("Open".into())
    );
    world
        .advance(world.id(1).unwrap(), BODY, Default::default(), None, 30)
        .unwrap();
    assert_eq!(world.activation().owner(), None);
}

#[test]
fn activation_requires_receiver_eligibility_and_matching_torch_state() {
    for (function, burning, expected) in [
        ("Activate", 0, 1),
        ("Activate", 1, 0),
        ("TorchActivate", 0, 0),
        ("TorchActivate", 1, 1),
    ] {
        let (mut world, mut inputs) = activation_world(function);
        inputs.actors.get_mut(&2).unwrap().torch_burning = burning;
        activate_world(&mut world, &inputs);
        assert_eq!(world.flags(), [expected], "{function} burning={burning}");
    }
    for state in [10, 19] {
        let (mut world, mut inputs) = activation_world("Activate");
        inputs.actors.get_mut(&1).unwrap().state = state;
        activate_world(&mut world, &inputs);
        assert_eq!(world.flags(), [0]);
        assert_eq!(world.activation().prompt(), None);
    }
    let (mut world, mut inputs) = activation_world("Activate");
    inputs.actors.get_mut(&0).unwrap().on_rope = true;
    inputs.actors.get_mut(&2).unwrap().on_rope = true;
    activate_world(&mut world, &inputs);
    assert_eq!(
        world.flags(),
        [1],
        "rope admission uses the receiver, not caller or player"
    );
    assert_eq!(
        world.activation_overlay(&inputs, 22, false, 24).unwrap(),
        cyrus_game::script::ActivationOverlay::Hidden
    );
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        cyrus_game::script::ActivationOverlay::Hidden
    );
    assert_eq!(world.activation().prompt(), Some("Open"));
}

#[test]
fn missing_activation_labels_do_not_claim_owner_and_stale_inputs_fail_before_execution() {
    let (mut world, mut inputs) = activation_world("Activate");
    inputs.labels.clear();
    activate_world(&mut world, &inputs);
    assert_eq!(world.flags(), [0]);
    assert_eq!(world.activation().owner(), None);
    assert_eq!(world.activation().prompt(), None);
    let (mut world, mut inputs) = activation_world("Activate");
    inputs.player.generation += 1;
    assert!(
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Interrupt),
                cyrus_game::script::ScriptInputs {
                    activation: Some(&inputs),
                    ..Default::default()
                },
                None,
                30
            )
            .is_err()
    );
    assert!(world.actors()[0].failure().is_none());
    assert_eq!(world.activation().owner(), None);
}

#[test]
fn activation_shared_and_weapon_gates_reject_without_claiming_a_prompt() {
    for gate in 0..11 {
        let (mut world, mut inputs) = activation_world("Activate");
        match gate {
            0 => inputs.lockout = 1,
            1 => inputs.dialogue_open = true,
            2 => inputs.lineup_result = 1,
            3 => inputs.actors.get_mut(&1).unwrap().on_rope = true,
            4 => inputs.actors.get_mut(&2).unwrap().state = 20,
            5 => inputs.actors.get_mut(&2).unwrap().state = 21,
            6 => {
                inputs
                    .actors
                    .get_mut(&2)
                    .unwrap()
                    .animation
                    .as_mut()
                    .unwrap()
                    .group = Some(91)
            }
            7 => {
                inputs
                    .actors
                    .get_mut(&2)
                    .unwrap()
                    .animation
                    .as_mut()
                    .unwrap()
                    .group = Some(129)
            }
            8 => {
                let player = inputs.actors.get_mut(&2).unwrap();
                player.weapon_assigned = true;
                player.weapon_drawn = true;
            }
            9 => inputs.actors.get_mut(&1).unwrap().state = 10,
            10 => inputs.actors.get_mut(&1).unwrap().state = 19,
            _ => unreachable!(),
        }
        activate_world(&mut world, &inputs);
        assert_eq!(world.flags(), [0], "gate {gate}");
        assert_eq!(world.activation().owner(), None, "gate {gate}");
        assert_eq!(world.activation().prompt(), None, "gate {gate}");
    }
    for gate in 0..5 {
        let (mut world, mut inputs) = activation_world("Activate");
        inputs.actors.get_mut(&2).unwrap().state = 20;
        match gate {
            0 => inputs.item_system_ready = false,
            1 => inputs.shell_execution = true,
            2 => inputs.actors.get_mut(&2).unwrap().animation = None,
            3 => {
                inputs.actors.get_mut(&2).unwrap().state = 0;
                inputs.lockout = 257;
            }
            4 => {
                inputs.actors.get_mut(&2).unwrap().state = 0;
                inputs.actors.get_mut(&1).unwrap().state = 20;
            }
            _ => unreachable!(),
        }
        activate_world(&mut world, &inputs);
        assert_eq!(world.flags(), [1], "admitted gate {gate}");
    }
}

#[test]
fn activation_prompt_waits_for_presentation_and_override_zero_crossings() {
    use cyrus_game::script::ActivationOverlay;
    let (mut world, mut inputs) = activation_world("Activate");
    activate_world(&mut world, &inputs);
    inputs.lockout = 1;
    assert_eq!(
        world.activation_overlay(&inputs, 100, false, 24).unwrap(),
        ActivationOverlay::Hidden
    );
    inputs.lockout = 0;
    assert_eq!(
        world.activation_overlay(&inputs, 22, false, 24).unwrap(),
        ActivationOverlay::Hidden
    );
    for gate in 0..5 {
        inputs.dialogue_open = gate == 0;
        inputs.render_disabled = gate == 1;
        let player = inputs.actors.get_mut(&2).unwrap();
        player.on_rope = gate == 2;
        player.state = if gate == 3 { 10 } else { 0 };
        player.water_state = if gate == 4 { 4 } else { 0 };
        assert_eq!(
            world.activation_overlay(&inputs, 0, false, 24).unwrap(),
            ActivationOverlay::Hidden
        );
        assert_eq!(world.activation().prompt(), Some("Open"));
    }
    inputs.actors.get_mut(&2).unwrap().water_state = 0;
    assert_eq!(
        world.activation_overlay(&inputs, 0, true, 24).unwrap(),
        ActivationOverlay::Prompt("Open".into())
    );
    inputs.use_held = false;
    activate_world(&mut world, &inputs);
    assert_eq!(world.flags(), [0]);
    assert_eq!(
        world.activation_overlay(&inputs, 24, true, 24).unwrap(),
        ActivationOverlay::Item {
            visible: true,
            cycle_allowed: true
        }
    );
    assert_eq!(world.activation().prompt(), Some("Open"));
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Prompt("Open".into())
    );
    let (fresh, _) = activation_world("Activate");
    assert_eq!(fresh.activation().owner(), None);
    assert_eq!(fresh.activation().prompt(), None);
}

#[test]
fn activation_reads_player_and_receiver_attributes_with_weapon_override() {
    for (player_type, special, holding, expected) in [(1, 0, 0, 0), (1, 1, 0, 1), (0, 0, 1, 0)] {
        let (mut world, mut inputs) = activation_world_with_attributes(
            "Activate",
            BTreeMap::from([("attr_106".into(), special)]),
            BTreeMap::from([
                ("attr_105".into(), player_type),
                ("attr_110".into(), holding),
            ]),
        );
        inputs.item_system_ready = false;
        inputs.shell_execution = true;
        inputs.actors.get_mut(&2).unwrap().animation = None;
        activate_world(&mut world, &inputs);
        assert_eq!(world.flags(), [expected]);
    }
}

#[test]
fn activation_owner_blocks_a_different_receiver_after_prompt_consumption() {
    let (mut world, inputs) = activation_world("Activate");
    activate_world(&mut world, &inputs);
    world.activation_overlay(&inputs, 22, false, 24).unwrap();
    world.activation_overlay(&inputs, 0, false, 24).unwrap();
    world
        .advance(
            world.id(1).unwrap(),
            ScriptStage::Body(Thread::Interrupt),
            cyrus_game::script::ScriptInputs {
                activation: Some(&inputs),
                ..Default::default()
            },
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.flags(), [0]);
    assert_eq!(world.activation().owner(), Some(world.id(1).unwrap()));
    assert_eq!(world.activation().prompt(), None);
}

#[test]
fn activation_item_path_keeps_rendering_and_weapon_cycling_gates_separate() {
    use cyrus_game::script::ActivationOverlay;
    let (mut world, mut inputs) = activation_world("Activate");
    inputs.render_disabled = true;
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Item {
            visible: false,
            cycle_allowed: true
        }
    );
    let player = inputs.actors.get_mut(&2).unwrap();
    player.weapon_assigned = true;
    player.weapon_drawn = true;
    inputs.render_disabled = false;
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Item {
            visible: true,
            cycle_allowed: false
        }
    );
    inputs.item_system_ready = false;
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Hidden
    );
}

#[test]
fn empty_rtx_text_still_activates_an_existing_entry() {
    let (mut world, mut inputs) = activation_world("Activate");
    inputs.labels.insert("#opn".into(), String::new());
    activate_world(&mut world, &inputs);
    assert_eq!(world.flags(), [1]);
    assert_eq!(world.activation().prompt(), Some(""));
    assert_eq!(world.activation().owner(), Some(world.id(1).unwrap()));
}

#[test]
fn activation_distinguishes_animation_state_from_its_current_group() {
    use cyrus_game::script::{ActivationAnimation, ScriptInputs};
    for (state, armed) in [(20, false), (21, false), (0, true)] {
        for (present, bound) in [(false, false), (true, false), (false, true)] {
            let (mut world, mut inputs) = activation_world("Activate");
            let player = inputs.actors.get_mut(&inputs.player.index).unwrap();
            player.state = state;
            player.weapon_assigned = armed;
            player.weapon_drawn = armed;
            player.animation = present.then_some(ActivationAnimation { group: None });
            if bound {
                let data = Animations {
                    version: 2,
                    world: world.world,
                    sections: BTreeMap::new(),
                    models: BTreeMap::new(),
                    unsupported: BTreeMap::new(),
                    metadata: json!({
                        "script_content_id": world.content_id,
                        "actors": [{"script_name": "CYRUS", "animation_groups": []}]
                    }),
                };
                world.bind_animation(inputs.player, &data, 0).unwrap();
            }
            world
                .advance(
                    world.id(0).unwrap(),
                    ScriptStage::Body(Thread::Interrupt),
                    ScriptInputs {
                        activation: Some(&inputs),
                        nominal_ticks: Some(0),
                        ..Default::default()
                    },
                    None,
                    30,
                )
                .unwrap();
            let admitted = !present && !bound;
            assert_eq!(
                world.flags(),
                [i32::from(admitted)],
                "state={state}, armed={armed}, present={present}, bound={bound}"
            );
            assert_eq!(world.activation().prompt(), admitted.then_some("Open"));
            assert_eq!(
                world.activation().owner(),
                admitted.then(|| world.id(1).unwrap())
            );
        }
    }
}

#[test]
fn ordinary_rotation_lock_restores_angle_after_script_task_without_cancelling_it() {
    for (axis, attribute, component) in [(0, 38, 0), (1, 37, 1), (2, 39, 2)] {
        let mut data = axis_task_scripts("RotateByAxis", axis, 128, 4);
        data.definition.functions.push(Function {
            name: "SetMyAttr".into(),
            kind: CallKind::Function,
            param_count: 2,
        });
        let actor = data.actors.get_mut("ACTOR").unwrap();
        actor.program.steps = std::mem::take(&mut actor.program.steps)
            .into_iter()
            .map(|(offset, mut step)| {
                step.next += 1;
                (offset + 1, step)
            })
            .collect();
        actor.program.steps.insert(
            0,
            Step {
                next: 1,
                operation: Operation::Call {
                    call: Call {
                        function_id: 4,
                        kind: CallKind::Function,
                        target: Object::Me,
                        function: "SetMyAttr".into(),
                        arguments: vec![
                            Value::Integer { value: attribute },
                            Value::Integer { value: 1 },
                        ],
                    },
                },
            },
        );
        data.content_id = data.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&data, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        world.snapshot_spatial(id).unwrap();
        stage(&mut world, id, BODY);
        assert_eq!(world.actors()[0].attributes()[attribute as usize], 1);
        assert_eq!(world.spatial(id).unwrap().local.angles[component], 32);
        world.restore_axis_locks(id).unwrap();
        assert_eq!(world.spatial(id).unwrap().local.angles[component], 0);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world.refresh_transform(id).unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.matrix,
            cyrus_world::transform::Matrix::IDENTITY
        );
    }
}

#[test]
fn axis_movement_preserves_quantization_and_elapsed_position_contracts() {
    for (function, delta, expected) in [
        ("MoveByAxis", 2560, [512, 1024, 1536, 2560]),
        ("MoveByAxis", -2560, [-768, -1536, -2304, -2560]),
        ("sMoveByAxis", 2560, [0, 640, 1280, 2560]),
        ("sMoveByAxis", -2560, [0, -640, -1280, -2560]),
        ("MoveByAxis", 100, [0, 0, 0, 100]),
        ("MoveByAxis", -100, [-100, -100, -100, -100]),
    ] {
        let data = axis_task_scripts(function, 1, delta, 4);
        let mut world = ScriptWorld::prepare(&data, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        for (index, position) in expected.into_iter().enumerate() {
            stage(&mut world, id, if index == 0 { BODY } else { TASKS });
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                [0, position, 0],
                "{function} {delta} invocation {index}"
            );
        }
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        stage(&mut world, id, BODY);
        assert_eq!(world.flags(), [9]);
    }
}

#[test]
fn movement_zero_target_is_reinitialized_even_after_initial_completion() {
    let mut data = axis_task_scripts("MoveByAxis", 0, -256, 1);
    data.instances[0].map_position[0] = 1;
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    stage(&mut world, id, BODY);
    assert_eq!(world.spatial(id).unwrap().local.position, [0; 3]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    stage(&mut world, id, TASKS);
    assert_eq!(world.spatial(id).unwrap().local.position, [-256, 0, 0]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn axis_movement_keeps_distinct_invalid_axis_and_zero_duration_rules() {
    for (function, axis, duration, expected) in [
        ("MoveByAxis", -1, 4, [0, 0, 512]),
        ("sMoveByAxis", -1, 4, [0, 0, 0]),
        ("MoveByAxis", 0, 0, [0, 0, 0]),
    ] {
        let data = axis_task_scripts(function, axis, 2560, duration);
        let mut world = ScriptWorld::prepare(&data, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        stage(&mut world, id, BODY);
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
    }
    let data = axis_task_scripts("sMoveByAxis", 0, 2560, 0);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let error = world
        .advance(
            id,
            BODY,
            cyrus_game::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            30,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("sMoveByAxis division by zero"));
    assert_eq!(world.spatial(id).unwrap().local.position, [0; 3]);
}
