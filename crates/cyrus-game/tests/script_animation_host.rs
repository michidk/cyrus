//! Shared actor host contracts use synthetic scripts and explicit stage timing.
use cyrus_assets::{animation::Animations, script::*};
use cyrus_game::script::ScriptSound;
use cyrus_game::script::{ScriptActorId, ScriptStage, ScriptWorld};
use cyrus_script::Thread;
use cyrus_script::program::*;
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
        location_offsets: vec![],
        attributes: BTreeMap::new(),
        combat_taunts: vec![],
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
        version: 13,
        combat_attacks: vec![],
        dialogue: cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        },
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
            None,
            30,
        )
        .unwrap();
    assert_eq!(
        events,
        [(
            receiver,
            ScriptSound::Animation {
                position: None,
                sound: AnimationSound::Play {
                    effect: -17,
                    rate_offset_hz: -64
                },
                surface: Some(0),
            }
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
            None,
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
                None,
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
                None,
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
                None,
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
                    None,
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
fn absolute_rotation_matches_original_angle_sequences_and_completion() {
    let cases: &[(&str, u32, i32, i32, &[u16])] = &[
        ("RotateToAxis", 0, 512, 4, &[128, 256, 384, 512]),
        ("RotateToAxis", 0, 1536, 4, &[1920, 1792, 1664, 1536]),
        ("RotateToAxis", 2040, 8, 4, &[2044, 0, 4, 8]),
        ("RotateToAxis", 512, 512, 4, &[512]),
        ("RotateToAxis", 0, 512, 0, &[0]),
        ("sRotateToAxis", 0, 512, 4, &[0, 170, 341, 512]),
        ("sRotateToAxis", 0, 1536, 4, &[0, 1877, 1706, 1536]),
        ("sRotateToAxis", 0, 1500, 4, &[0, 1889, 1730, 1500]),
        ("sRotateToAxis", 2040, 8, 4, &[2040, 2045, 2, 8]),
        ("sRotateByAxis", 0, 512, 4, &[0, 128, 256, 512]),
        ("sRotateByAxis", 0, -512, 4, &[0, 1920, 1792, 1536]),
        ("sRotateByAxis", 2040, 16, 4, &[2040, 2044, 0, 8]),
        ("sRotateByAxis", 0, -1, 4, &[0, 0, 0, 2047]),
        ("sRotateByAxis", 1026, 512, 1, &[1538]),
    ];
    for &(function, initial, target, duration, expected) in cases {
        let mut data = axis_task_scripts(function, 1, target, duration);
        data.instances[0].angles[1] = initial;
        data.content_id = data.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&data, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        for (index, &angle) in expected.iter().enumerate() {
            stage(&mut world, id, if index == 0 { BODY } else { TASKS });
            assert_eq!(
                world.spatial(id).unwrap().local.angles,
                [0, angle, 0],
                "{function}, initial {initial}, target {target}, invocation {index}"
            );
            assert_eq!(
                world.actors()[0].runtime().unwrap().active_tasks(),
                usize::from(index == 0 || index + 1 < expected.len())
            );
        }
        if expected.len() == 1 {
            stage(&mut world, id, TASKS);
            assert_eq!(world.spatial(id).unwrap().local.angles[1], expected[0]);
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        }
        stage(&mut world, id, BODY);
        assert_eq!(world.flags(), [9]);
    }
}

#[test]
fn absolute_rotation_nominal_fallback_preserves_direction_and_scaled_duration_fault_is_contextual()
{
    use cyrus_game::script::ScriptInputs;
    let data = axis_task_scripts("RotateToAxis", 1, 1536, 4);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let zero = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(0),
        ..Default::default()
    };
    world.advance(id, BODY, zero, None, None, 30).unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0; 3]);
    world
        .advance(
            id,
            TASKS,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..zero
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 2047, 0]);
    for (function, duration) in [("sRotateToAxis", 1), ("sRotateByAxis", 0)] {
        let data = axis_task_scripts(function, 1, 512, duration);
        let mut world = ScriptWorld::prepare(&data, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let error = world.advance(id, BODY, zero, None, None, 30).unwrap_err();
        assert!(
            format!("{error:#}").contains(&format!("{function} duration causes division by zero"))
        );
        assert_eq!(world.spatial(id).unwrap().local.angles, [0; 3]);
        assert_eq!(world.flags(), [0]);
    }
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
    world.advance(id, BODY, zero, None, None, 30).unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0; 3]);
    let tick = ScriptInputs {
        nominal_ticks: Some(256),
        ..zero
    };
    world.advance(id, TASKS, tick, None, None, 30).unwrap();
    // The fallback sign follows the saved target, not the negative delta.
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 0, 1]);
    world.advance(id, TASKS, zero, None, None, 30).unwrap();
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
            .advance(id, BODY, ScriptInputs::default(), None, None, 30)
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
        ("StopAllSounds", vec![]),
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
            None,
            100,
        )
        .unwrap();
    assert_eq!(world.flags(), [0, 0, 0, 0, 0]);
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
            (receiver, ScriptSound::StopAll),
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
        .advance(
            world.id(0).unwrap(),
            BODY,
            Default::default(),
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.activation().owner(), Some(world.id(1).unwrap()));
    assert_eq!(
        world.activation_overlay(&inputs, 0, false, 24).unwrap(),
        ActivationOverlay::Prompt("Open".into())
    );
    world
        .advance(
            world.id(1).unwrap(),
            BODY,
            Default::default(),
            None,
            None,
            30,
        )
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
            None,
            30,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("sMoveByAxis division by zero"));
    assert_eq!(world.spatial(id).unwrap().local.position, [0; 3]);
}

#[test]
fn ambient_slots_retain_separate_handles_reset_explicitly_and_cancel_without_playback() {
    use cyrus_game::script::ScriptInputs;
    use cyrus_world::sound::{AmbientSoundDriver, AmbientSoundRequest, AmbientSoundUpdate};
    use std::sync::Mutex;
    #[derive(Debug, Default)]
    struct Driver(Mutex<Vec<AmbientSoundRequest>>);
    impl AmbientSoundDriver for Driver {
        fn update(&self, request: AmbientSoundRequest) -> Result<AmbientSoundUpdate, String> {
            self.0.lock().unwrap().push(request);
            Ok(AmbientSoundUpdate {
                channel: Some(if request.effect == 1 { 4 } else { 7 }),
                active: true,
            })
        }
    }
    let ambient = |effect| Operation::Call {
        call: Call {
            function_id: 3,
            kind: CallKind::Multitask,
            target: Object::Me,
            function: "AmbientSound".into(),
            arguments: vec![
                Value::Integer { value: effect },
                Value::Integer { value: -40 },
                Value::Integer { value: 2000 },
            ],
        },
    };
    let mut data = scripts(vec![
        ambient(1),
        ambient(2),
        call(2, CallKind::Task, Object::Me),
        Operation::TaskStop { label: 0 },
        flag(),
        Operation::End { target: 5 },
    ]);
    data.definition.functions.push(Function {
        name: "AmbientSound".into(),
        param_count: 3,
        kind: CallKind::Multitask,
    });
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 18).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let driver = Driver::default();
    let inputs = ScriptInputs {
        ambient_sound: Some(&driver),
        sound_reset: Some(false),
        frame_scale: Some(256),
        nominal_ticks: Some(0),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 30)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world
        .advance(id, ScriptStage::Tasks, inputs, None, None, 30)
        .unwrap();
    let requests = driver.0.lock().unwrap().clone();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.channel)
            .collect::<Vec<_>>(),
        [None, None, Some(4), Some(7)]
    );
    assert!(requests.iter().all(|request| request.generation == 18
        && request.actor_index == 0
        && request.volume == -40
        && request.rate_offset_hz == 2000
        && !request.object_update_excluded));
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 30)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(
        driver.0.lock().unwrap().len(),
        4,
        "TaskStop issues no playback update"
    );
    world
        .advance(
            id,
            ScriptStage::Tasks,
            ScriptInputs {
                sound_reset: Some(true),
                ..inputs
            },
            None,
            None,
            30,
        )
        .unwrap();
    let requests = driver.0.lock().unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!((requests[4].effect, requests[4].channel), (2, None));
}

fn lockout_data(value: i32, activate: bool) -> Scripts {
    let mut data = scripts(vec![Operation::End { target: 0 }]);
    data.definition.functions.extend([
        Function {
            name: "LockoutPlayer".into(),
            param_count: 1,
            kind: CallKind::Function,
        },
        Function {
            name: "Activate".into(),
            param_count: 1,
            kind: CallKind::Function,
        },
    ]);
    let mut player = data.actors["OTHER"].clone();
    player.namespace = Some("CYRUS".into());
    player.code_origin = 200;
    player.attributes.insert("attr_117".into(), 7);
    data.actors.insert("CYRUS".into(), player);
    data.actors
        .get_mut("ACTOR")
        .unwrap()
        .attributes
        .insert("attr_117".into(), 8);
    data.actors
        .get_mut("OTHER")
        .unwrap()
        .attributes
        .insert("attr_117".into(), 9);
    let native_call = |function_id, function: &str, value| Call {
        function_id,
        function: function.into(),
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![Value::Integer { value }],
    };
    let mut operations = vec![Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(native_call(3, "LockoutPlayer", value)),
            },
            rest: vec![],
        },
    }];
    if activate {
        operations.push(Operation::Call {
            call: native_call(4, "Activate", i32::from_le_bytes(*b"#OPN")),
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
    let mut excluded = data.instances[1].clone();
    excluded.object_id = 2;
    excluded.object_type = 2;
    data.instances.push(excluded);
    data.content_id = data.content_identity().unwrap();
    data
}

fn lockout_world(value: i32, activate: bool) -> ScriptWorld {
    ScriptWorld::prepare(&lockout_data(value, activate), 7).unwrap()
}

#[test]
fn lockout_player_retains_low_word_and_cleans_only_registered_actors() {
    for (value, retained, exact_one) in [
        (1, 1, true),
        (65537, 1, true),
        (-65535, 1, true),
        (0, 0, false),
        (2, 2, false),
        (257, 257, false),
        (-1, 65535, false),
    ] {
        let mut world = lockout_world(value, false);
        assert_eq!(world.globals().lockout(), None);
        world
            .globals_mut()
            .remember_choice("Remembered topic".into());
        let id = world.id(0).unwrap();
        stage(&mut world, id, BODY);
        assert_eq!(world.flags(), [0]);
        assert_eq!(world.globals().lockout(), Some(retained));
        assert_eq!(world.globals().highlighted_choice(), Some(""));
        let actors = world.inspect(0, 4)["actors"].as_array().unwrap().clone();
        assert_eq!(actors[3]["attributes"][117], if exact_one { 1 } else { 0 });
        assert_eq!(actors[0]["attributes"][117], if exact_one { 8 } else { 0 });
        assert_eq!(actors[1]["attributes"][117], if exact_one { 9 } else { 0 });
        assert_eq!(
            actors[2]["attributes"][117], 0,
            "separate object attributes start cleared"
        );
    }
}

#[test]
fn lockout_call_changes_activation_and_overlay_in_the_same_invocation() {
    use cyrus_game::script::ActivationOverlay;
    for (value, caller_lockout, expected_prompt) in [(1, 0, None), (2, 1, Some("Open"))] {
        let mut world = lockout_world(value, true);
        let (_, mut inputs) = activation_world("Activate");
        inputs.player = world.id(3).unwrap();
        inputs.actors.insert(3, inputs.actors[&2]);
        inputs.lockout = caller_lockout;
        inputs.use_held = false;
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                BODY,
                cyrus_game::script::ScriptInputs {
                    activation: Some(&inputs),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap();
        assert_eq!(world.activation().prompt(), expected_prompt);
        assert_eq!(
            world.activation_overlay(&inputs, 0, false, 24).unwrap(),
            if value == 1 {
                ActivationOverlay::Hidden
            } else {
                ActivationOverlay::Prompt("Open".into())
            }
        );
    }
}

#[test]
fn lockout_clears_prompt_override_only_outside_exact_one() {
    use cyrus_game::script::ActivationOverlay;
    for value in [1, 2, 65536] {
        let mut data = lockout_data(value, false);
        let actor = data.actors.get_mut("ACTOR").unwrap();
        let lockout = actor.program.steps[&0].operation.clone();
        actor.program.entry = 2;
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: 4,
                            function: "Activate".into(),
                            kind: CallKind::Function,
                            target: Object::Me,
                            arguments: vec![Value::Integer {
                                value: i32::from_le_bytes(*b"#OPN"),
                            }],
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
            (
                2,
                Step {
                    next: 3,
                    operation: lockout,
                },
            ),
            (
                3,
                Step {
                    next: 4,
                    operation: Operation::End { target: 2 },
                },
            ),
        ]);
        data.content_id = data.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&data, 7).unwrap();
        let (_, mut inputs) = activation_world("Activate");
        inputs.player = world.id(3).unwrap();
        inputs.actors.insert(3, inputs.actors[&2]);
        inputs.use_held = false;
        let id = world.id(0).unwrap();
        let invoke = |world: &mut ScriptWorld, stage| {
            world
                .advance(
                    id,
                    stage,
                    cyrus_game::script::ScriptInputs {
                        activation: Some(&inputs),
                        ..Default::default()
                    },
                    None,
                    None,
                    30,
                )
                .unwrap()
        };
        invoke(&mut world, ScriptStage::Body(Thread::Interrupt));
        assert_eq!(
            world.activation_overlay(&inputs, 0, true, 24).unwrap(),
            ActivationOverlay::Prompt("Open".into())
        );
        invoke(&mut world, ScriptStage::Body(Thread::Interrupt));
        assert_eq!(world.inspect(0, 0)["activation"]["prompt_override"], 24);
        invoke(&mut world, BODY);
        assert_eq!(
            world.activation().prompt(),
            Some("Open"),
            "lockout clears the timer, not pending prompt text"
        );
        assert_eq!(
            world.inspect(0, 0)["activation"]["prompt_override"],
            if value == 1 { 24 } else { 0 }
        );
        assert_eq!(
            world.activation_overlay(&inputs, 0, false, 24).unwrap(),
            if value == 1 {
                ActivationOverlay::Hidden
            } else {
                ActivationOverlay::Prompt("Open".into())
            }
        );
    }
}

#[test]
fn new_game_applies_lockout_cleanup_before_replacing_the_registry() {
    let mut world = lockout_world(1, false);
    let id = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    world.send_combat_message(sender, id, 0xd001).unwrap();
    stage(&mut world, id, BODY);
    world
        .globals_mut()
        .remember_choice("Remembered topic".into());
    let mut registry = cyrus_game::script::ScriptActors::default();
    registry.current = Some(world);
    registry.begin_new_game();
    let world = registry.current.as_ref().unwrap();
    assert_eq!(world.globals().lockout(), Some(0));
    assert_eq!(world.globals().highlighted_choice(), Some(""));
    let actors = world.inspect(0, 4)["actors"].as_array().unwrap().clone();
    assert_eq!(actors[0]["attributes"][117], 0);
    assert_eq!(actors[1]["attributes"][117], 0);
    assert_eq!(actors[2]["attributes"][117], 0);
    assert_eq!(actors[3]["attributes"][117], 0);
    assert_eq!(
        registry
            .current
            .as_mut()
            .unwrap()
            .consume_combat_message(id)
            .unwrap(),
        None
    );
}

fn location_task_scripts(selector: i32, duration: i32) -> Scripts {
    let mut data = scripts(vec![
        Operation::Call {
            call: Call {
                function_id: 3,
                kind: CallKind::Task,
                target: Object::Me,
                function: "MoveToLocation".into(),
                arguments: [selector, duration]
                    .map(|value| Value::Integer { value })
                    .to_vec(),
            },
        },
        flag(),
        Operation::End { target: 0 },
    ]);
    data.definition.functions.push(Function {
        name: "MoveToLocation".into(),
        param_count: 2,
        kind: CallKind::Task,
    });
    data.actors.get_mut("ACTOR").unwrap().location_offsets = vec![[1024, 0, 0]];
    data.content_id = data.content_identity().unwrap();
    data
}

#[test]
fn location_movement_reaches_target_but_finishes_only_after_duration() {
    let data = location_task_scripts(1, 2);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    for (index, expected) in [341, 682, 1024, 1024].into_iter().enumerate() {
        stage(&mut world, id, if index == 0 { BODY } else { TASKS });
        assert_eq!(world.spatial(id).unwrap().local.position, [expected, 0, 0]);
        assert_eq!(
            world.actors()[0].runtime().unwrap().active_tasks(),
            usize::from(index < 3)
        );
    }
    stage(&mut world, id, BODY);
    assert_eq!(world.flags(), [9]);
}

#[test]
fn location_zero_and_invalid_selectors_use_original_home_after_movement() {
    for selector in [0, -1, 3] {
        let mut data = location_task_scripts(selector, -1);
        data.instances[0].map_position = [10, -3, 7];
        let program = &mut data.actors.get_mut("ACTOR").unwrap().program;
        let mut return_home = program.steps[&0].clone();
        return_home.next = 2;
        let mut leave_home = return_home.clone();
        leave_home.next = 1;
        if let Operation::Call { call } = &mut leave_home.operation {
            call.arguments[0] = Value::Integer { value: 1 };
        }
        program.steps = BTreeMap::from([
            (0, leave_home),
            (1, return_home),
            (
                2,
                Step {
                    next: 3,
                    operation: flag(),
                },
            ),
            (
                3,
                Step {
                    next: 4,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        data.content_id = data.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&data, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        stage(&mut world, id, BODY);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [3584, -768, 1792]
        );
        stage(&mut world, id, TASKS);
        stage(&mut world, id, BODY);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [2560, -768, 1792]
        );
        stage(&mut world, id, TASKS);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn location_count_boundary_reports_original_out_of_range_access() {
    let data = location_task_scripts(2, 1);
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
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
            None,
            30,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("selector equals location count"));
}

#[test]
fn attached_location_motion_reads_effective_pose_but_updates_local_coordinates() {
    let mut data = location_task_scripts(1, 2);
    data.definition.attributes = (0..256).map(|i| format!("attr_{i}")).collect();
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
    data.instances[0].map_position = [110, 0, 0];
    data.instances[1].map_position = [100, 0, 0];
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let id = world.id(0).unwrap();
    stage(&mut world, id, BODY);
    assert_eq!(world.spatial(id).unwrap().local.position, [2901, 0, 0]);
}

#[test]
fn authored_locations_are_relative_to_each_instance_original_home() {
    let mut data = location_task_scripts(1, -1);
    data.instances[0].map_position = [10, 0, 0];
    data.instances[1].actor = "ACTOR".into();
    data.instances[1].map_position = [20, 0, 0];
    let actor = data.actors.get_mut("ACTOR").unwrap();
    actor.instances = 2;
    actor.instance_variables.push(vec![]);
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    for (index, expected) in [3584, 6144].into_iter().enumerate() {
        let id = world.id(index).unwrap();
        stage(&mut world, id, BODY);
        assert_eq!(world.spatial(id).unwrap().local.position, [expected, 0, 0]);
    }
}

#[test]
fn location_movement_quantizes_normalized_components_and_snaps_after_overshoot() {
    let mut data = location_task_scripts(1, 0);
    data.actors.get_mut("ACTOR").unwrap().location_offsets = vec![[3, 4, 0]];
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            BODY,
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(384),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position, [4, 5, 0]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    stage(&mut world, id, TASKS);
    assert_eq!(world.spatial(id).unwrap().local.position, [3, 4, 0]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

fn hud_asset() -> cyrus_assets::hud::HudAsset {
    cyrus_assets::hud::HudAsset {
        labels: [
            ("TEST".into(), "A new journal entry.".into()),
            ("EMPT".into(), String::new()),
        ]
        .into(),
        images: vec![],
        candle_mode: 0,
        candle: [0; 2],
        candle_size: [0; 2],
        pickup: [0; 2],
        prompt: [0; 2],
        override_ticks: 0,
        compass: [0; 2],
        compass_size: [0; 2],
        world_compass: BTreeMap::new(),
        powerup_slots: [[0; 2]; 2],
        powerup_sizes: vec![[1; 2]; 3],
        logbook: [0; 2],
        logbook_size: [0; 2],
        walk_mode: false,
        auto_defend: false,
        notices: Default::default(),
        inventory: BTreeMap::new(),
        start_items: vec![],
        items: vec![],
        selected: None,
        bindings: [0; 15],
        font: Default::default(),
        text_alpha: [0; 2],
        player_text_alpha: [0; 2],
    }
}
struct LogStore {
    load: Option<String>,
    fail_save: bool,
    writes: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl cyrus_game::preferences::PreferenceStore for LogStore {
    fn load(&self, _: cyrus_game::preferences::PreferenceKey) -> anyhow::Result<Option<String>> {
        Ok(self.load.clone())
    }
    fn save(&self, _: cyrus_game::preferences::PreferenceKey, text: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fail_save, "disk full");
        self.writes.lock().unwrap().push(text.into());
        Ok(())
    }
}
fn hud_call(
    hud: Option<&cyrus_game::script::ScriptHud>,
    function: &str,
    arguments: &[i32],
) -> anyhow::Result<i32> {
    let mut data = scripts(vec![
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id: 3,
                        kind: CallKind::Function,
                        target: Object::Me,
                        function: function.into(),
                        arguments: arguments
                            .iter()
                            .map(|value| Value::Integer { value: *value })
                            .collect(),
                    }),
                },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]);
    let actor = data.actors.get_mut("ACTOR").unwrap();
    actor.program.variables = vec![0];
    actor.instance_variables = vec![vec![0]];
    data.definition.functions.push(Function {
        name: function.into(),
        param_count: arguments.len().try_into().unwrap(),
        kind: CallKind::Function,
    });
    data.content_id = data.content_identity()?;
    let mut world = ScriptWorld::prepare(&data, 1)?;
    world.advance(
        world.id(0).unwrap(),
        BODY,
        cyrus_game::script::ScriptInputs {
            hud,
            ..Default::default()
        },
        None,
        None,
        30,
    )?;
    anyhow::ensure!(
        world.actors()[0].failure().is_none(),
        "{:?}",
        world.actors()[0].failure()
    );
    Ok(world.actors()[0].runtime().unwrap().variables()[0])
}

#[test]
fn script_powerup_dispatch_validates_installed_frames_and_retains_slot_order() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let hud = ScriptHud::default();
    assert!(hud_call(None, "ShowBitmap", &[0, 0]).is_err());
    assert!(hud_call(Some(&hud), "ShowBitmap", &[0, 0]).is_err());
    hud.configure(&hud_asset(), &Preferences::default());
    hud_call(Some(&hud), "ShowBitmap", &[0, 0]).unwrap();
    hud_call(Some(&hud), "ShowBitmap", &[1, 0]).unwrap();
    assert_eq!(hud.presentation().0, [Some((0, 0)), None]);
    hud_call(Some(&hud), "ShowBitmap", &[1, 1]).unwrap();
    hud_call(Some(&hud), "ShowBitmap", &[0, 2]).unwrap();
    assert_eq!(hud.presentation().0, [Some((0, 2)), Some((1, 1))]);
    hud_call(Some(&hud), "UnShowBitmap", &[0]).unwrap();
    assert_eq!(hud.presentation().0, [None, Some((1, 1))]);
    hud_call(Some(&hud), "ShowBitmap", &[1, 0]).unwrap();
    hud_call(Some(&hud), "UnShowBitmap", &[1]).unwrap();
    assert_eq!(
        hud.presentation().0,
        [None, Some((1, 1))],
        "clears only the first matching ID"
    );
    for values in [[-1, 0], [2, 0], [0, -1], [0, 3]] {
        assert!(hud_call(Some(&hud), "ShowBitmap", &values).is_err());
    }
    assert_eq!(hud.presentation().0, [None, Some((1, 1))]);
}
#[test]
fn script_logbook_dispatch_writes_installed_text_before_notifying_and_suppresses_tags() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let writes = std::sync::Arc::default();
    let preferences = Preferences::new(LogStore {
        load: None,
        fail_save: false,
        writes: std::sync::Arc::clone(&writes),
    });
    let hud = ScriptHud::default();
    hud.configure(&hud_asset(), &preferences);
    let tag = i32::from_le_bytes(*b"TEST");
    hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
    assert_eq!(hud.entries().unwrap(), ["A new journal entry."]);
    assert!(hud.presentation().1);
    hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
    hud_call(Some(&hud), "AddLog", &[i32::from_le_bytes(*b"NONE")]).unwrap();
    assert!(!hud.presentation().1);
    hud_call(Some(&hud), "AddLog", &[i32::from_le_bytes(*b"EMPT")]).unwrap();
    assert!(
        hud.presentation().1,
        "an installed empty entry is still written"
    );
    assert_eq!(hud.entries().unwrap(), ["A new journal entry.", ""]);
    assert_eq!(writes.lock().unwrap().len(), 2);
    let reloaded = ScriptHud::default();
    reloaded.configure(
        &hud_asset(),
        &Preferences::new(LogStore {
            load: writes.lock().unwrap().last().cloned(),
            fail_save: false,
            writes: std::sync::Arc::clone(&writes),
        }),
    );
    assert_eq!(reloaded.entries().unwrap(), hud.entries().unwrap());
    assert!(
        !reloaded.presentation().1,
        "loading retained text is not a new notification"
    );
}
#[test]
fn failed_logbook_storage_does_not_notify_or_overwrite_and_tag_is_still_suppressed() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    for load in [Some("broken".into()), Some("[\"Earlier entry.\"]".into())] {
        let writes = std::sync::Arc::default();
        let preferences = Preferences::new(LogStore {
            load: load.clone(),
            fail_save: true,
            writes: std::sync::Arc::clone(&writes),
        });
        let hud = ScriptHud::default();
        hud.configure(&hud_asset(), &preferences);
        let tag = i32::from_le_bytes(*b"TEST");
        assert!(hud_call(Some(&hud), "AddLog", &[tag]).is_err());
        assert!(!hud.presentation().1);
        assert!(writes.lock().unwrap().is_empty());
        if load.as_deref() == Some("broken") {
            assert!(hud.entries().is_err());
        } else {
            assert_eq!(hud.entries().unwrap(), ["Earlier entry."]);
        }
        hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
        assert!(!hud.presentation().1);
    }
}

#[test]
fn full_logbook_tag_table_does_not_retain_new_tags() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let writes = std::sync::Arc::default();
    let hud = ScriptHud::default();
    hud.configure(
        &hud_asset(),
        &Preferences::new(LogStore {
            load: None,
            fail_save: false,
            writes: std::sync::Arc::clone(&writes),
        }),
    );
    for tag in 1..=400 {
        hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
    }
    let tag = i32::from_le_bytes(*b"TEST");
    hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
    hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
    assert_eq!(
        hud.entries().unwrap(),
        ["A new journal entry.", "A new journal entry."]
    );
}

#[test]
fn journal_replacement_preserves_text_tags_and_pending_notification_without_io() {
    use cyrus_game::{
        preferences::Preferences,
        script::{Journal, JournalSnapshot, ScriptHud},
    };
    let asset = hud_asset();
    let writes = std::sync::Arc::default();
    let preferences = Preferences::new(LogStore {
        load: Some("[\"Earlier Ω entry.\"]".into()),
        fail_save: false,
        writes: std::sync::Arc::clone(&writes),
    });
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    let tag = i32::from_le_bytes(*b"TEST");
    hud_call(Some(&hud), "AddLog", &[tag]).unwrap();
    hud_call(Some(&hud), "AddLog", &[i32::from_le_bytes(*b"NONE")]).unwrap();
    let encoded = serde_json::to_string(&hud.snapshot_journal().unwrap()).unwrap();
    assert!(
        hud.presentation().1,
        "capture must not consume the pending notification"
    );
    let before = writes.lock().unwrap().clone();
    struct CountLoads(LogStore, std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl cyrus_game::preferences::PreferenceStore for CountLoads {
        fn load(
            &self,
            key: cyrus_game::preferences::PreferenceKey,
        ) -> anyhow::Result<Option<String>> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            cyrus_game::preferences::PreferenceStore::load(&self.0, key)
        }
        fn save(
            &self,
            key: cyrus_game::preferences::PreferenceKey,
            text: &str,
        ) -> anyhow::Result<()> {
            cyrus_game::preferences::PreferenceStore::save(&self.0, key, text)
        }
    }
    let loads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let candidate_preferences = Preferences::new(CountLoads(
        LogStore {
            load: Some("malformed backend must not be loaded".into()),
            fail_save: false,
            writes: std::sync::Arc::clone(&writes),
        },
        std::sync::Arc::clone(&loads),
    ));
    let loads_before = loads.load(std::sync::atomic::Ordering::Relaxed);
    let mut replacement = Journal::from_snapshot(
        &asset,
        &candidate_preferences,
        serde_json::from_str::<JournalSnapshot>(&encoded).unwrap(),
    )
    .unwrap();
    assert_eq!(
        loads.load(std::sync::atomic::Ordering::Relaxed),
        loads_before,
        "replacement must not read the backend"
    );
    assert_eq!(writes.lock().unwrap().as_slice(), before);
    assert_eq!(
        replacement.entries().unwrap(),
        ["Earlier Ω entry.", "A new journal entry."]
    );
    assert!(replacement.take_notification());
    assert!(!replacement.take_notification());
    replacement.add_log(tag).unwrap();
    replacement.add_log(i32::from_le_bytes(*b"NONE")).unwrap();
    assert_eq!(writes.lock().unwrap().as_slice(), before);
    let empty = i32::from_le_bytes(*b"EMPT");
    replacement.add_log(empty).unwrap();
    hud_call(Some(&hud), "AddLog", &[empty]).unwrap();
    assert_eq!(replacement.entries().unwrap(), hud.entries().unwrap());
    assert_eq!(
        serde_json::to_value(replacement.snapshot().unwrap()).unwrap(),
        serde_json::to_value(hud.snapshot_journal().unwrap()).unwrap()
    );
}

#[test]
fn journal_replacement_rejects_incompatible_or_invalid_state_without_touching_live_hud() {
    use cyrus_game::{
        preferences::Preferences,
        script::{Journal, JournalSnapshot, ScriptHud},
    };
    let asset = hud_asset();
    let writes = std::sync::Arc::default();
    let preferences = Preferences::new(LogStore {
        load: None,
        fail_save: false,
        writes: std::sync::Arc::clone(&writes),
    });
    let hud = ScriptHud::default();
    assert!(hud.snapshot_journal().is_err());
    hud.configure(&asset, &preferences);
    hud_call(Some(&hud), "AddLog", &[i32::from_le_bytes(*b"TEST")]).unwrap();
    let saved = serde_json::to_value(hud.snapshot_journal().unwrap()).unwrap();
    let before = writes.lock().unwrap().clone();
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("seen", serde_json::json!([0])),
        ("seen", serde_json::json!([1, 1])),
        ("seen", serde_json::json!((1..=401).collect::<Vec<i32>>())),
        ("labels", serde_json::json!({})),
    ] {
        let mut invalid = saved.clone();
        invalid[field] = value;
        let invalid: JournalSnapshot = serde_json::from_value(invalid).unwrap();
        assert!(
            Journal::from_snapshot(&asset, &preferences, invalid).is_err(),
            "{field}"
        );
        assert_eq!(
            serde_json::to_value(hud.snapshot_journal().unwrap()).unwrap(),
            saved
        );
        assert_eq!(writes.lock().unwrap().as_slice(), before);
    }
    let mut changed = hud_asset();
    changed
        .labels
        .insert("TEST".into(), "changed installed text".into());
    assert!(
        Journal::from_snapshot(
            &changed,
            &preferences,
            serde_json::from_value(saved).unwrap()
        )
        .is_err()
    );
    let broken = ScriptHud::default();
    broken.configure(
        &asset,
        &Preferences::new(LogStore {
            load: Some("malformed".into()),
            fail_save: false,
            writes: std::sync::Arc::clone(&writes),
        }),
    );
    assert!(broken.snapshot_journal().is_err());
    assert_eq!(writes.lock().unwrap().as_slice(), before);
}

#[test]
fn journal_replacement_retains_full_tag_table_and_failed_write_suppression() {
    use cyrus_game::{
        preferences::Preferences,
        script::{Journal, ScriptHud},
    };
    let asset = hud_asset();
    let writes = std::sync::Arc::default();
    let preferences = Preferences::new(LogStore {
        load: Some("[\"Accepted before failure.\"]".into()),
        fail_save: true,
        writes: std::sync::Arc::clone(&writes),
    });
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    let tag = i32::from_le_bytes(*b"TEST");
    assert!(hud_call(Some(&hud), "AddLog", &[tag]).is_err());
    let working = Preferences::new(LogStore {
        load: None,
        fail_save: false,
        writes: std::sync::Arc::clone(&writes),
    });
    let mut replacement =
        Journal::from_snapshot(&asset, &working, hud.snapshot_journal().unwrap()).unwrap();
    replacement.add_log(tag).unwrap();
    assert_eq!(replacement.entries().unwrap(), ["Accepted before failure."]);
    assert!(!replacement.take_notification());
    assert!(writes.lock().unwrap().is_empty());
    for unknown in 1..400 {
        replacement.add_log(unknown).unwrap();
    }
    let empty = i32::from_le_bytes(*b"EMPT");
    let full = replacement.snapshot().unwrap();
    let mut full = Journal::from_snapshot(&asset, &working, full).unwrap();
    full.add_log(empty).unwrap();
    full.add_log(empty).unwrap();
    assert_eq!(
        full.entries().unwrap(),
        ["Accepted before failure.", "", ""]
    );
    assert_eq!(writes.lock().unwrap().len(), 2);
}

#[test]
fn inventory_scripts_share_counts_caps_selection_and_requirements() {
    use cyrus_assets::hud::InventoryItem;
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = hud_asset();
    asset.inventory = [
        (
            0,
            InventoryItem {
                count: 0,
                total: 1,
                player_max: 1,
                hidden: false,
                ..Default::default()
            },
        ),
        (
            2,
            InventoryItem {
                count: 3,
                total: 8,
                player_max: 6,
                hidden: false,
                ..Default::default()
            },
        ),
        (
            3,
            InventoryItem {
                count: 1,
                total: 1,
                player_max: 1,
                hidden: true,
                required_items: vec![3],
                ..Default::default()
            },
        ),
        (
            4,
            InventoryItem {
                count: 0,
                total: 2,
                player_max: 2,
                hidden: false,
                required_items: vec![3],
                ..Default::default()
            },
        ),
    ]
    .into();
    asset.selected = Some(2);
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud_call(Some(&hud), "AddItem", &[0, 9]).unwrap();
    hud_call(Some(&hud), "AddItem", &[2, 9]).unwrap();
    hud_call(Some(&hud), "AddItem", &[2, -5]).unwrap();
    hud_call(Some(&hud), "SelectItem", &[0]).unwrap();
    assert_eq!(hud_call(Some(&hud), "HaveItem", &[0]).unwrap(), 1);
    assert_eq!(hud_call(Some(&hud), "HaveItem", &[2]).unwrap(), 6);
    let view = hud.inventory(None).unwrap();
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.counts, [(0, 1), (2, 6), (3, 1), (4, 0)].into());
    assert_eq!(hud.inspect()["compass_on"], false);
    assert_eq!(hud.inventory(Some(true)).unwrap().selected, Some(2));
    assert_eq!(hud.inventory(Some(true)).unwrap().selected, Some(0));
    assert_eq!(hud.inventory(Some(false)).unwrap().selected, Some(2));
    let error = hud_call(Some(&hud), "AddItem", &[4, 1]).unwrap_err();
    assert!(format!("{error:#}").contains("required_item_list cycle"));
    assert_eq!(hud.inventory(None).unwrap().counts[&4], 0);
    hud_call(Some(&hud), "SelectItem", &[4]).unwrap();
    assert_eq!(hud.inventory(None).unwrap().selected, Some(0));
    assert!(hud_call(Some(&hud), "AddItem", &[90, 1]).is_err());
    assert!(hud_call(Some(&hud), "SelectItem", &[-1]).is_err());
    // Reconfiguration on a new world cannot restore initial counts or selection.
    hud.configure(&asset, &Preferences::default());
    assert_eq!(hud.inventory(None).unwrap().counts[&0], 1);
}

fn inventory_fixture(counts: &[(u16, u32)]) -> cyrus_assets::hud::HudAsset {
    let mut asset = hud_asset();
    asset.inventory = counts
        .iter()
        .map(|&(id, count)| {
            (
                id,
                cyrus_assets::hud::InventoryItem {
                    count,
                    total: 100,
                    player_max: 100,
                    ..Default::default()
                },
            )
        })
        .collect();
    asset
}

#[test]
fn inventory_requires_recursive_owned_prerequisites_before_any_effect() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(1, 0), (2, 1), (3, 0), (4, 2)]);
    asset.inventory.get_mut(&1).unwrap().required_items = vec![2];
    asset.inventory.get_mut(&2).unwrap().required_items = vec![3];
    asset.inventory.get_mut(&1).unwrap().drop_add_items = vec![4];
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud.add_item(1, 3, &mut |_| Ok(())).unwrap();
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(1, 0), (2, 1), (3, 0), (4, 2)].into()
    );
    hud.add_item(3, 1, &mut |_| Ok(())).unwrap();
    hud.add_item(1, 3, &mut |_| Ok(())).unwrap();
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(1, 3), (2, 1), (3, 1), (4, 1)].into()
    );
}

#[test]
fn inventory_effects_run_once_in_authored_order_with_drop_before_recursive_add() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(1, 0), (2, 1), (3, 0), (4, 0), (5, 0), (6, 0)]);
    asset.inventory.get_mut(&1).unwrap().drop_add_items = vec![2];
    asset.inventory.get_mut(&1).unwrap().add_items = vec![3, 4, 5, 5];
    asset.inventory.get_mut(&3).unwrap().required_items = vec![2];
    asset.inventory.get_mut(&4).unwrap().add_items = vec![6];
    asset.inventory.get_mut(&5).unwrap().required_items = vec![6];
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud.add_item(1, 9, &mut |_| Ok(())).unwrap();
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(1, 9), (2, 0), (3, 0), (4, 1), (5, 2), (6, 1)].into()
    );
}

#[test]
fn inventory_retains_admitted_quantity_when_effects_remove_source() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(1, 4), (2, 0)]);
    let source = asset.inventory.get_mut(&1).unwrap();
    source.total = 5;
    source.player_max = 5;
    source.add_items = vec![2];
    asset.inventory.get_mut(&2).unwrap().drop_add_items = vec![1];
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud.add_item(1, 10, &mut |_| Ok(())).unwrap();
    assert_eq!(hud.inventory(None).unwrap().counts, [(1, 4), (2, 1)].into());
}

#[test]
fn inventory_removal_traverses_only_remaining_owned_sources_and_honors_flags() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    for (source_count, flags, expected) in [
        (2, 0, [1, 0, 0]),
        (1, 0, [0, 2, 3]),
        (2, 1, [0, 2, 3]),
        (2, 2, [1, 0, 0]),
        (2, 4, [1, 0, 0]),
    ] {
        let mut asset = inventory_fixture(&[(1, source_count), (2, 2), (3, 3)]);
        asset.inventory.get_mut(&1).unwrap().flags = flags;
        asset.inventory.get_mut(&1).unwrap().remove_drop_items = vec![2];
        asset.inventory.get_mut(&2).unwrap().remove_drop_items = vec![3];
        let hud = ScriptHud::default();
        hud.configure(&asset, &Preferences::default());
        hud_call(Some(&hud), "DropItem", &[1, 0]).unwrap();
        assert_eq!(
            hud_call(Some(&hud), "HaveItem", &[1]).unwrap(),
            source_count as i32
        );
        hud_call(Some(&hud), "DropItem", &[1, 1]).unwrap();
        let view = hud.inventory(None).unwrap();
        assert_eq!(
            [view.counts[&1], view.counts[&2], view.counts[&3]],
            expected,
            "flags {flags}, source {source_count}"
        );
    }
    let mut asset = inventory_fixture(&[(1, 2), (2, 0), (3, 3)]);
    asset.inventory.get_mut(&1).unwrap().remove_drop_items = vec![2];
    asset.inventory.get_mut(&2).unwrap().remove_drop_items = vec![3];
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud_call(Some(&hud), "DropItem", &[1, 1]).unwrap();
    assert_eq!(hud.inventory(None).unwrap().counts[&3], 3);
}

#[test]
fn inventory_startup_uses_ordered_shared_acquisition_and_retains_initial_selection() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(0, 0), (1, 0), (2, 52), (4, 5), (18, 0)]);
    asset.start_items = vec![1, 2, 4, 18];
    asset.selected = Some(1);
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(0, 0), (1, 1), (2, 53), (4, 6), (18, 1)].into()
    );
    let mut asset = inventory_fixture(&[(0, 0), (1, 1), (2, 0), (3, 0)]);
    asset.start_items = vec![2, 1, 2, 2];
    asset.inventory.get_mut(&1).unwrap().add_items = vec![3];
    asset.inventory.get_mut(&2).unwrap().required_items = vec![3];
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    assert_eq!(hud.inspect()["inventory"]["selected"], 0);
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(0, 0), (1, 2), (2, 2), (3, 1)].into()
    );
}

#[test]
fn inventory_startup_reports_recursive_definition_errors_without_partial_inventory() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(1, 0), (2, 0)]);
    asset.start_items = vec![1];
    asset.inventory.get_mut(&1).unwrap().add_items = vec![2];
    asset.inventory.get_mut(&2).unwrap().add_items = vec![1];
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    assert!(hud.inventory(None).is_none());
    assert!(!hud.ready());
    let error = hud_call(Some(&hud), "AddItem", &[1, 1]).unwrap_err();
    assert!(format!("{error:#}").contains("add_item_list cycle"));
    assert!(
        hud.inspect()["inventory_error"]
            .as_str()
            .unwrap()
            .contains("start_item_list")
    );
}

#[test]
fn inventory_installed_startup_exports_seed_counts_and_acquires_through_shared_owner() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.is_dir() {
        eprintln!("skipped: no install ({})", root.display());
        return;
    }
    let install = cyrus_assets::install::GameInstall::locate(Some(&root)).unwrap();
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("inventory-startup-test-{}", std::process::id()));
    cyrus_assets::hud_export::export_hud(&install, &output).unwrap();
    let asset: cyrus_assets::hud::HudAsset =
        serde_json::from_slice(&std::fs::read(output.join("hud/overlay.hud")).unwrap()).unwrap();
    assert_eq!(asset.start_items, [1, 2, 4, 18]);
    assert_eq!(asset.inventory[&2].count, 52);
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    assert!(hud.inspect()["inventory_error"].is_null());
    let view = hud.inventory(None).unwrap();
    let owned: BTreeMap<_, _> = view
        .counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect();
    assert_eq!(owned, [(1, 1), (2, 53), (4, 6), (18, 1)].into());
    assert_eq!(view.selected, Some(1));
    hud_call(Some(&hud), "AddItem", &[2, 3]).unwrap();
    assert_eq!(hud_call(Some(&hud), "HaveItem", &[2]).unwrap(), 56);
    hud.configure(&asset, &Preferences::default());
    assert_eq!(hud.inventory(None).unwrap().counts[&2], 56);
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn inventory_drop_list_preserves_authored_order_and_zero_admission_skips_effects() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(1, 0), (2, 2), (3, 2), (4, 1)]);
    asset.inventory.get_mut(&1).unwrap().drop_add_items = vec![2, 3];
    asset.inventory.get_mut(&1).unwrap().player_max = 1;
    asset.inventory.get_mut(&2).unwrap().remove_drop_items = vec![3];
    asset.inventory.get_mut(&3).unwrap().remove_drop_items = vec![4];
    asset.inventory.get_mut(&3).unwrap().flags = 1;
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud.add_item(1, 1, &mut |_| Ok(())).unwrap();
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(1, 1), (2, 1), (3, 0), (4, 0)].into()
    );
    hud.add_item(1, 9, &mut |_| Ok(())).unwrap();
    assert_eq!(
        hud.inventory(None).unwrap().counts,
        [(1, 1), (2, 1), (3, 0), (4, 0)].into()
    );
}

#[test]
fn active_item_reads_selected_record_without_ownership_filter_and_takes_no_arguments() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    assert!(hud_call(None, "ActiveItem", &[]).is_err());
    let hud = ScriptHud::default();
    assert_eq!(hud_call(Some(&hud), "ActiveItem", &[]).unwrap(), 0);
    assert_eq!(hud_call(Some(&hud), "HaveItem", &[999]).unwrap(), 0);
    let mut asset = inventory_fixture(&[(1, 1), (257, 0)]);
    asset.inventory.get_mut(&257).unwrap().hidden = true;
    asset.selected = Some(257);
    hud.configure(&asset, &Preferences::default());
    assert_eq!(hud_call(Some(&hud), "ActiveItem", &[]).unwrap(), 257);
    assert!(hud_call(Some(&hud), "ActiveItem", &[0]).is_err());
    hud.select_item(1).unwrap();
    assert_eq!(hud_call(Some(&hud), "ActiveItem", &[]).unwrap(), 1);
}

fn hand_item_world(item: i32, holding: u8) -> ScriptWorld {
    let mut data = scripts(vec![
        Operation::Assign {
            destination: Place::Flag { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id: 3,
                        kind: CallKind::Function,
                        target: Object::Named { string: 0 },
                        function: "HandItem".into(),
                        arguments: vec![Value::Integer { value: item }],
                    }),
                },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]);
    data.definition.functions.push(Function {
        name: "HandItem".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    data.actors
        .get_mut("OTHER")
        .unwrap()
        .attributes
        .insert("attr_110".into(), holding);
    data.content_id = data.content_identity().unwrap();
    ScriptWorld::prepare(&data, 7).unwrap()
}

#[test]
fn hand_item_routes_to_receiver_and_preserves_inventory_and_attributes() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let mut asset = inventory_fixture(&[(1, 1), (257, 0)]);
    asset.inventory.get_mut(&257).unwrap().hidden = true;
    asset.selected = Some(1);
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    let mut world = hand_item_world(257, 0);
    let attributes = world.actors()[1].attributes().to_vec();
    world
        .advance(
            world.id(0).unwrap(),
            BODY,
            cyrus_game::script::ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert!(world.actors()[0].failure().is_none());
    assert_eq!(
        world.actors()[1].control_state().unwrap().held_item,
        Some(1)
    );
    assert_eq!(world.actors()[0].control_state().unwrap().held_item, None);
    assert_eq!(world.actors()[1].attributes(), attributes);
    assert_eq!(hud.active_item().unwrap(), 1);
    assert_eq!(hud.inventory(None).unwrap().counts[&257], 0);
    assert_eq!(world.flags(), [0]);
}

#[test]
fn hand_item_rejects_known_gates_before_lookup_and_requires_known_shell_admission() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let hud = ScriptHud::default();
    hud.configure(&inventory_fixture(&[(1, 1)]), &Preferences::default());
    for (surface, holding, shell) in [(2, 0, false), (4, 0, false), (0, 1, false), (0, 0, true)] {
        let mut world = hand_item_world(-1, holding);
        world
            .set_actor_surface(world.id(1).unwrap(), surface)
            .unwrap();
        world
            .advance(
                world.id(0).unwrap(),
                BODY,
                cyrus_game::script::ScriptInputs {
                    hud: Some(&hud),
                    shell_execution: Some(shell),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap();
        assert!(world.actors()[0].failure().is_none());
        assert_eq!(world.actors()[1].control_state().unwrap().held_item, None);
    }
    let mut world = hand_item_world(1, 0);
    let error = world
        .advance(
            world.id(0).unwrap(),
            BODY,
            cyrus_game::script::ScriptInputs {
                hud: Some(&hud),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("shell admission unavailable"));
    assert!(
        world.actors()[0]
            .failure()
            .unwrap()
            .contains("shell admission unavailable")
    );
}

#[test]
fn hand_item_distinguishes_closed_unknown_and_invalid_inventory() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let closed = ScriptHud::default();
    let mut world = hand_item_world(-1, 0);
    world
        .advance(
            world.id(0).unwrap(),
            BODY,
            cyrus_game::script::ScriptInputs {
                hud: Some(&closed),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.actors()[1].control_state().unwrap().held_item, None);
    for configured in [false, true] {
        let hud = ScriptHud::default();
        if configured {
            hud.configure(&inventory_fixture(&[(1, 1)]), &Preferences::default());
        }
        let mut world = hand_item_world(513, 0);
        let error = world
            .advance(
                world.id(0).unwrap(),
                BODY,
                cyrus_game::script::ScriptInputs {
                    hud: configured.then_some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(if configured {
            "definition absent"
        } else {
            "inventory host unavailable"
        }));
        assert_eq!(world.actors()[1].control_state().unwrap().held_item, None);
    }
    let invalid = ScriptHud::default();
    let mut asset = inventory_fixture(&[(1, 1)]);
    asset.selected = Some(999);
    invalid.configure(&asset, &Preferences::default());
    assert!(hud_call(Some(&invalid), "ActiveItem", &[]).is_err());
    let mut world = hand_item_world(1, 0);
    let error = world
        .advance(
            world.id(0).unwrap(),
            BODY,
            cyrus_game::script::ScriptInputs {
                hud: Some(&invalid),
                shell_execution: Some(false),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("inventory initialization"));
}

#[test]
fn acquisition_repairs_empty_selection_before_count_and_assigns_literal_fallback() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    for (has_one, weapon, expected) in [
        (true, true, Some(1)),
        (false, true, Some(4)),
        (false, false, Some(2)),
    ] {
        let mut asset = inventory_fixture(&[(2, 1), (4, 1), (9, 0)]);
        asset.selected = Some(9);
        for item in asset.inventory.values_mut() {
            item.hidden = true;
        }
        asset.inventory.get_mut(&4).unwrap().weapon = weapon;
        if has_one {
            asset.inventory.insert(
                1,
                cyrus_assets::hud::InventoryItem {
                    count: 0,
                    total: 1,
                    player_max: 1,
                    hidden: true,
                    weapon: false,
                    ..Default::default()
                },
            );
        }
        let hud = ScriptHud::default();
        hud.configure(&asset, &Preferences::default());
        assert_eq!(hud.inventory(None).unwrap().selected, None);
        let mut assignments = Vec::new();
        hud.add_item(9, 1, &mut |id| {
            assignments.push(id);
            Ok(())
        })
        .unwrap();
        assert_eq!(assignments, [expected.unwrap()]);
        assert_eq!(hud.inspect()["inventory"]["selected"], expected.unwrap());
        assert_eq!(hud.inspect()["inventory"]["counts"]["9"], 1);
    }
    let mut asset = inventory_fixture(&[(9, 0)]);
    asset.selected = Some(9);
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    assert_eq!(hud.inventory(None).unwrap().selected, None);
    let mut assignments = Vec::new();
    hud.add_item(9, 1, &mut |id| {
        assignments.push(id);
        Ok(())
    })
    .unwrap();
    assert!(assignments.is_empty());
    assert!(hud.inspect()["inventory"]["selected"].is_null());
    hud.add_item(9, 1, &mut |id| {
        assignments.push(id);
        Ok(())
    })
    .unwrap();
    assert_eq!(assignments, [9]);
    assert_eq!(hud.inspect()["inventory"]["selected"], 9);
}

#[test]
fn acquisition_keeps_existing_selection_and_rejects_unavailable_fallback_owner() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    let asset = inventory_fixture(&[(1, 0), (9, 0)]);
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    hud.select_item(9).unwrap();
    hud.add_item(1, 1, &mut |_| anyhow::bail!("unexpected assignment"))
        .unwrap();
    assert_eq!(hud.active_item().unwrap(), 9);
    hud_call(Some(&hud), "DropItem", &[1, 1]).unwrap();
    assert_eq!(hud.inventory(None).unwrap().selected, None);
    let error = hud
        .add_item(9, 1, &mut |_| anyhow::bail!("selected player unavailable"))
        .unwrap_err();
    assert!(format!("{error:#}").contains("selected player unavailable"));
    assert_eq!(hud.inspect()["inventory"]["counts"]["9"], 0);
}

#[test]
fn routed_acquisition_fallback_assigns_selected_player_not_receiver() {
    use cyrus_game::{
        preferences::Preferences,
        script::{ScriptHud, ScriptInputs},
    };
    let mut data = scripts(vec![
        Operation::ObjectCall {
            call: Call {
                function_id: 3,
                kind: CallKind::Function,
                target: Object::Named { string: 0 },
                function: "AddItem".into(),
                arguments: vec![Value::Integer { value: 9 }, Value::Integer { value: 1 }],
            },
        },
        Operation::End { target: 0 },
    ]);
    data.definition.functions.push(Function {
        name: "AddItem".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    let mut player = data.actors["OTHER"].clone();
    player.namespace = Some("CYRUS".into());
    player.code_origin = 200;
    player.attributes.insert("attr_23".into(), 7);
    data.actors.insert("CYRUS".into(), player);
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 7).unwrap();
    let hud = ScriptHud::default();
    hud.configure(
        &inventory_fixture(&[(1, 0), (9, 0)]),
        &Preferences::default(),
    );
    assert_eq!(hud.inventory(None).unwrap().selected, None);
    world
        .advance(
            world.id(0).unwrap(),
            BODY,
            ScriptInputs {
                hud: Some(&hud),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.actors()[0].attributes()[23], 0);
    assert_eq!(world.actors()[1].attributes()[23], 0);
    assert_eq!(world.actors()[2].attributes()[23], 1);
    assert_eq!(hud.active_item().unwrap(), 1);
    assert_eq!(hud.inspect()["inventory"]["counts"]["9"], 1);
}

#[test]
fn selected_player_weapon_validation_uses_type_ownership_animation_and_shell() {
    use cyrus_game::{preferences::Preferences, script::ScriptHud};
    for (bound, shell, defined, weapon, count, assigned, expected) in [
        (true, false, true, true, 1, 1, 1),
        (true, false, true, false, 1, 1, 0),
        (true, false, true, true, 0, 1, 0),
        (true, false, false, true, 1, 1, 0),
        (false, false, true, true, 1, 1, 0),
        (true, true, true, true, 1, 1, 0),
        (true, false, true, true, 1, 0, 0),
    ] {
        let mut data = scripts(vec![Operation::End { target: 0 }]);
        let mut player = data.actors["OTHER"].clone();
        player.namespace = Some("CYRUS".into());
        player.code_origin = 200;
        player.attributes.insert("attr_23".into(), assigned);
        data.actors.insert("CYRUS".into(), player);
        data.actors
            .get_mut("ACTOR")
            .unwrap()
            .attributes
            .insert("attr_23".into(), 7);
        data.content_id = data.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&data, 7).unwrap();
        let selected = world.id(2).unwrap();
        if bound {
            let mut animation = animations(&data, false);
            animation.metadata["actors"][0]["script_name"] = json!("CYRUS");
            world.bind_animation(selected, &animation, 1).unwrap();
        }
        let mut asset = inventory_fixture(&[(1, count)]);
        asset.inventory.get_mut(&1).unwrap().weapon = weapon;
        asset.inventory.get_mut(&1).unwrap().hidden = true;
        if !defined {
            asset.inventory.clear();
            asset.selected = None;
        }
        let hud = ScriptHud::default();
        hud.configure(&asset, &Preferences::default());
        let mut cues = Vec::new();
        world
            .check_player_weapon(selected, &hud, shell, 0, &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            })
            .unwrap();
        assert_eq!(world.actors()[selected.index].attributes()[23], expected);
        assert_eq!(world.actors()[0].attributes()[23], 7);
        assert!(cues.is_empty());
    }
}

#[test]
fn standalone_role_initialization_owns_combat_and_rejects_repeated_or_failed_entry() {
    for combat in [false, true] {
        let mut data = scripts(vec![Operation::End { target: 0 }]);
        for (name, origin) in [("CYRUS", 200), ("GREMLIN", 300)] {
            let mut role = data.actors["OTHER"].clone();
            role.namespace = Some(name.into());
            role.code_origin = origin;
            role.attributes.insert("attr_14".into(), u8::from(combat));
            data.actors.insert(name.into(), role);
        }
        data.content_id = data.content_identity().unwrap();
        let mut animation = animations(&data, false);
        animation.metadata["actors"] = json!(["CYRUS", "GREMLIN"].map(|name| json!({
            "script_name":name,"animation_groups":[{"group_index":0,"anim_type":0,"anim_id":1,
                "commands":[{"opcode":4,"sound_param":9,"volume_shift":0},
                    {"opcode":0,"frame_reference":3},{"opcode":0,"frame_reference":4},{"opcode":2,"value":0}]}]
        })));
        let mut world = ScriptWorld::prepare(&data, 7).unwrap();
        let mut sounds = Vec::new();
        for index in [2, 3] {
            let id = world.id(index).unwrap();
            world
                .initialize_player_animation(id, &animation, 1, &mut |sound| {
                    sounds.push(sound);
                    Ok(())
                })
                .unwrap();
            assert_eq!(world.animation_frame(id).unwrap(), Some(3));
            assert_eq!(world.actor_surface(id).unwrap(), 0);
            let control = world.actors()[index].control_state().unwrap();
            assert_eq!(control.combat.is_some(), combat);
            assert_eq!(control.combat_death_default, combat.then_some(0));
            assert_eq!(control.object_update_excluded, index == 3);
            assert!(
                world
                    .initialize_player_animation(id, &animation, 1, &mut |_| panic!(
                        "replayed entry"
                    ))
                    .is_err()
            );
        }
        assert_eq!(sounds.len(), 2);
        let mut failed = ScriptWorld::prepare(&data, 8).unwrap();
        let id = failed.id(2).unwrap();
        let error = failed
            .initialize_player_animation(id, &animation, 1, &mut |_| {
                anyhow::bail!("sample unavailable")
            })
            .unwrap_err();
        assert!(format!("{error:#}").contains("sample unavailable"));
        assert!(
            failed.actors()[2]
                .animation_failure()
                .unwrap()
                .contains("sample unavailable")
        );
        assert!(
            failed
                .initialize_player_animation(id, &animation, 1, &mut |_| panic!(
                    "retried failed sound"
                ))
                .is_err()
        );
    }
}

#[test]
fn ordinary_weapon_default_uses_shared_inventory_readiness() {
    use cyrus_game::{
        preferences::Preferences,
        script::{OrdinaryInputs, ScriptHud, ScriptInputs},
    };
    for (ready, expected) in [(false, 1), (true, 8)] {
        let mut source = scripts(vec![Operation::End { target: 0 }]);
        for placement in &mut source.instances {
            placement.object_type = 1;
            placement.active = 1;
        }
        for actor in source.actors.values_mut() {
            actor.attributes.insert("attr_44".into(), 1);
        }
        source.content_id = source.content_identity().unwrap();
        let mut data = animations(&source, false);
        for actor in data.metadata["actors"].as_array_mut().unwrap() {
            actor["animation_groups"] = json!([1, 8, 91].map(|group| json!({
                "group_index":group,"anim_type":0,"anim_id":1,
                "commands":[{"opcode":0,"frame_reference":group},{"opcode":2,"value":0}]
            })));
        }
        let mut world = ScriptWorld::prepare(&source, 1).unwrap();
        for index in 0..2 {
            let id = world.id(index).unwrap();
            world.bind_animation(id, &data, 91).unwrap();
            stage(&mut world, id, ScriptStage::Animation);
        }
        let hud = ScriptHud::default();
        if ready {
            hud.configure(&hud_asset(), &Preferences::default());
        }
        world
            .update_ordinary(
                &data,
                [0; 2],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: None,
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        for index in 0..2 {
            assert!(
                world.actors()[index].failure().is_none(),
                "{:?}",
                world.actors()[index].failure()
            );
            assert_eq!(
                world.animation_frame(world.id(index).unwrap()).unwrap(),
                Some(expected)
            );
        }
    }
}
