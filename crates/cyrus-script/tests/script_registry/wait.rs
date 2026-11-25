use super::*;

pub(super) fn wait_cache(ops: Vec<Operation>) -> Scripts {
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: Native::Wait,
            param_count: 1,
            kind: CallKind::Task,
        },
        Function {
            name: Native::WaitOnTasks,
            param_count: 0,
            kind: CallKind::Task,
        },
        Function {
            name: Native::WaitNonMulti,
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

pub(super) fn wait(id: u16, name: Native, kind: CallKind, args: Vec<Value>) -> Operation {
    Operation::Call {
        call: Call {
            function_id: id,
            function: name,
            target: Object::Me,
            kind,
            arguments: args,
        },
    }
}

pub(super) fn set_flag(value: i32) -> Operation {
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
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 4 }],
        ),
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
fn a_multitask_call_of_a_function_native_runs_it_as_the_function() {
    // The original keeps only the handler per function, so FlatAnimate behaves the same whichever
    // call type a script uses: the texture check of the function form fires for the multitask form.
    for kind in [CallKind::Task, CallKind::Multitask] {
        let mut scripts = wait_cache(vec![
            wait(
                4,
                Native::FlatAnimate,
                kind,
                [(1 << 7) | 2, 0, 0]
                    .map(|value| Value::Integer { value })
                    .to_vec(),
            ),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.push(Function {
            name: Native::FlatAnimate,
            param_count: 3,
            kind: CallKind::Function,
        });
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let error = world.step(id, Thread::Main, 20).unwrap_err();
        assert!(format!("{error:#}").contains("not installed"), "{error:#}");
    }
}

#[test]
fn wait_signed_duration_wrap_and_initial_count_excludes_unallocated_slot() {
    for duration in [i32::MIN, -1, 0, 1, 2] {
        let scripts = wait_cache(vec![
            wait(
                1,
                Native::Wait,
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
    for (id, name) in [(2, Native::WaitOnTasks), (3, Native::WaitNonMulti)] {
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
            Native::Wait,
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(
            1,
            Native::Wait,
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(2, Native::WaitOnTasks, CallKind::Multitask, vec![]),
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
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::TaskResume { label: 6 },
        wait(3, Native::WaitNonMulti, CallKind::Task, vec![]),
        set_flag(7),
        Operation::Endint,
        wait(
            1,
            Native::Wait,
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
            Native::Wait,
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(
            1,
            Native::Wait,
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(3, Native::WaitNonMulti, CallKind::Task, vec![]),
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

#[test]
fn animation_frame_waits_resume_from_the_correct_owner_without_advancing_playback() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (function, selected) in [
        (Native::WaitAnimFrame, false),
        (Native::WaitPlayerAnimFrame, true),
    ] {
        let mut scripts = wait_cache(vec![
            wait(
                1,
                function,
                CallKind::Task,
                vec![Value::Integer { value: 7 }, Value::Integer { value: 1 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions[1] = Function {
            name: function,
            param_count: 2,
            kind: CallKind::Task,
        };
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let receiver = world.id(if selected { 2 } else { 0 }).unwrap();
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
                "script_name":world.actors()[receiver.index].name(),
                "animation_groups":[{"group_index":7,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]}]
            }]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        world
            .bind_animation(receiver, &data, AnimationGroup::IDLE_LEDGE)
            .unwrap();
        world
            .advance(
                receiver,
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
        let before = world.inspect(receiver.index, 1)["actors"][0]["animation"].clone();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world
            .advance(
                caller,
                ScriptStage::Tasks,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(
            world.inspect(receiver.index, 1)["actors"][0]["animation"],
            before
        );
        world
            .advance(
                receiver,
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
        world
            .advance(
                caller,
                ScriptStage::Tasks,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [7]);
    }
}

#[test]
fn animation_frame_wait_rejects_unprepared_owner_but_absent_selected_player_completes() {
    for (function, completes) in [
        (Native::WaitAnimFrame, false),
        (Native::WaitPlayerAnimFrame, true),
    ] {
        let mut scripts = wait_cache(vec![
            wait(
                1,
                function,
                CallKind::Task,
                vec![Value::Integer { value: 7 }, Value::Integer { value: 1 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions[1] = Function {
            name: function,
            param_count: 2,
            kind: CallKind::Task,
        };
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let result = world.step(world.id(0).unwrap(), Thread::Main, 20);
        if completes {
            result.unwrap();
            world.step_tasks(world.id(0).unwrap(), Some(256)).unwrap();
            world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
            assert_eq!(world.flags(), [7]);
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains("requires prepared playback"));
        }
    }
}
