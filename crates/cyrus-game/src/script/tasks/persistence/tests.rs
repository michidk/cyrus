//! Native continuations resume through VM calls and explicit restored service bindings.
use super::*;
use crate::script::tasks::{Lookup, NativeServices, SpatialCall, Speech};
use cyrus_script::{
    Argument, CallResult, CodeOrigin, Host, Invocation, Runtime, Target, Thread, program::*,
};
use cyrus_world::transform::Matrix;
use serde_json::json;
use std::sync::Arc;

#[derive(Default)]
struct NativeHost {
    native: NativeTasks,
    position: [i32; 3],
    angles: [u16; 3],
    updates: Vec<usize>,
    calls: usize,
    polls: usize,
    frame_progress: u8,
}
impl Host for NativeHost {
    fn call(
        &mut self,
        _: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        invocation: Invocation,
        _: &mut [i32],
    ) -> Result<CallResult> {
        self.calls += 1;
        let spatial = SpatialCall {
            position: &mut self.position,
            angles: &mut self.angles,
            frame_scale: 256,
            nominal_ticks: 1,
            matrix: Matrix::IDENTITY,
            slave: false,
            locations: &[[100, 200, 300]],
            effective_position: None,
            player_position: None,
            actor_index: 0,
            map_updates: &mut self.updates,
        };
        self.native
            .call(
                (function, kind),
                arguments,
                invocation,
                &mut None,
                &mut Some(spatial),
                NativeServices {
                    frame_wait: Some(crate::script::tasks::FrameWaitCall {
                        receiver: Ok((7, -1, self.frame_progress)),
                        player: Ok(Some((7, -1, self.frame_progress))),
                    }),
                    ..Default::default()
                },
            )?
            .context("test native call absent")
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        self.polls += 1;
        let spatial = SpatialCall {
            position: &mut self.position,
            angles: &mut self.angles,
            frame_scale: 256,
            nominal_ticks: 1,
            matrix: Matrix::IDENTITY,
            slave: false,
            locations: &[[100, 200, 300]],
            effective_position: None,
            player_position: None,
            actor_index: 0,
            map_updates: &mut self.updates,
        };
        self.native.poll(
            token,
            arguments,
            invocation,
            &mut None,
            &mut Some(spatial),
            NativeServices {
                frame_wait: Some(crate::script::tasks::FrameWaitCall {
                    receiver: Ok((7, -1, self.frame_progress)),
                    player: Ok(Some((7, -1, self.frame_progress))),
                }),
                ..Default::default()
            },
        )
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        Ok(())
    }
}

fn code(name: &str, kind: CallKind, arguments: &[i32]) -> Arc<Program> {
    Arc::new(Program {
        version: 4,
        entry: 0,
        variables: vec![],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: 0,
                            function: name.into(),
                            kind,
                            target: Object::Me,
                            arguments: arguments
                                .iter()
                                .map(|&value| Value::Integer { value })
                                .collect(),
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
        ]),
    })
}
fn begin(
    name: &str,
    arguments: &[i32],
) -> (Runtime, NativeHost, BTreeMap<CodeOrigin, Arc<Program>>) {
    let program = code(name, CallKind::Task, arguments);
    let sources = BTreeMap::from([(CodeOrigin(10), Arc::clone(&program))]);
    let mut runtime =
        Runtime::from_shared(program, CodeOrigin(10), vec![], "task TEST".into()).unwrap();
    let mut host = NativeHost {
        position: [1000, 2000, 3000],
        ..Default::default()
    };
    runtime
        .invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    (runtime, host, sources)
}
fn resume(
    vm: &Snapshot,
    native: NativeTasksSnapshot,
    sources: &BTreeMap<CodeOrigin, Arc<Program>>,
    host: &NativeHost,
) -> (Runtime, NativeHost) {
    let tokens = BTreeMap::from([(1, 100)]);
    let native = NativeTasks::from_snapshot(
        serde_json::from_slice(&serde_json::to_vec(&native).unwrap()).unwrap(),
        vm,
        &tokens,
        NativeTaskBindings::default(),
    )
    .unwrap();
    let runtime = Runtime::from_snapshot(
        CodeOrigin(10),
        vm.clone(),
        sources,
        &tokens,
        "restored task TEST".into(),
    )
    .unwrap();
    (
        runtime,
        NativeHost {
            native,
            position: host.position,
            angles: host.angles,
            updates: host.updates.clone(),
            ..Default::default()
        },
    )
}

#[test]
fn wait_resumes_elapsed_without_reissuing_and_allocates_after_rebound_tokens() {
    let (mut vm, mut host, sources) = begin("Wait", &[4]);
    vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    let saved = vm.snapshot(&sources).unwrap();
    let (mut restored, mut restored_host) = resume(&saved, host.native.snapshot(), &sources, &host);
    assert_eq!(restored_host.calls, 0);
    for active in [1, 0, 0] {
        restored
            .invoke_tasks_timed(&mut restored_host, &mut [], 256)
            .unwrap();
        vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
        assert_eq!(restored.active_tasks(), active);
        assert_eq!(vm.active_tasks(), active);
    }
    assert_eq!(restored_host.calls, 0);
    restored
        .invoke_body(Thread::Main, &mut restored_host, &mut [], 20)
        .unwrap();
    assert_eq!(restored_host.calls, 0);
    restored
        .invoke_body(Thread::Main, &mut restored_host, &mut [], 20)
        .unwrap();
    assert_eq!(
        restored
            .snapshot(&sources)
            .unwrap()
            .task_tokens()
            .collect::<Vec<_>>(),
        [101]
    );
    assert_eq!(restored_host.calls, 1);
}

#[test]
fn motion_and_rotation_continuations_resume_at_the_same_pose() {
    for (name, args) in [
        ("MoveByAxis", [0, 2560, 10]),
        ("MoveObjectAxis", [0, 2560, 10]),
        ("sMoveByAxis", [0, 2560, 10]),
        ("RotateByAxis", [1, 80, 10]),
        ("RotateToAxis", [1, 80, 10]),
        ("sRotateToAxis", [1, 80, 10]),
        ("sRotateByAxis", [1, 80, 10]),
    ] {
        let (mut vm, mut host, sources) = begin(name, &args);
        for _ in 0..3 {
            vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
        }
        let (mut restored, mut restored_host) = resume(
            &vm.snapshot(&sources).unwrap(),
            host.native.snapshot(),
            &sources,
            &host,
        );
        assert_eq!(restored_host.calls, 0);
        for _ in 0..12 {
            vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
            restored
                .invoke_tasks_timed(&mut restored_host, &mut [], 256)
                .unwrap();
            assert_eq!(
                (
                    restored_host.position,
                    restored_host.angles,
                    restored.active_tasks()
                ),
                (host.position, host.angles, vm.active_tasks()),
                "{name}"
            );
        }
        assert_eq!(restored_host.calls, 0);
    }
}

#[test]
fn restored_relative_motion_keeps_its_literal_destination() {
    let (mut vm, mut host, sources) = begin("MoveByAxis", &[0, 2560, 10]);
    for _ in 0..3 {
        vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    }
    assert_eq!(host.position, [2024, 2000, 3000]);
    let (mut restored, mut restored_host) = resume(
        &vm.snapshot(&sources).unwrap(),
        host.native.snapshot(),
        &sources,
        &host,
    );
    for _ in 0..12 {
        restored
            .invoke_tasks_timed(&mut restored_host, &mut [], 256)
            .unwrap();
    }
    assert_eq!(restored_host.position, [3560, 2000, 3000]);
    assert_eq!(restored.active_tasks(), 0);
}

#[test]
fn invalid_census_tokens_allocator_and_variant_are_rejected_before_replacement() {
    let (vm, host, sources) = begin("Wait", &[4]);
    let saved = vm.snapshot(&sources).unwrap();
    let value = serde_json::to_value(host.native.snapshot()).unwrap();
    let mut cases = vec![];
    let mut changed = value.clone();
    changed["version"] = json!(2);
    cases.push(changed);
    let mut changed = value.clone();
    changed["next"] = json!(0);
    cases.push(changed);
    let mut changed = value.clone();
    changed["tasks"][0]["token"] = json!(0);
    cases.push(changed);
    let mut changed = value.clone();
    changed["tasks"][0]["token"] = json!(2);
    cases.push(changed);
    let mut changed = value.clone();
    changed["tasks"][0]["token"] = json!(2);
    changed["next"] = json!(2);
    cases.push(changed);
    let mut changed = value.clone();
    changed["tasks"][0]["state"] = json!("Tasks");
    cases.push(changed);
    let mut changed = value.clone();
    changed["tasks"].as_array_mut().unwrap().clear();
    cases.push(changed);
    let mut changed = value.clone();
    changed["tasks"]
        .as_array_mut()
        .unwrap()
        .push(value["tasks"][0].clone());
    cases.push(changed);
    for corrupt in cases {
        assert!(
            NativeTasks::from_snapshot(
                serde_json::from_value(corrupt).unwrap(),
                &saved,
                &BTreeMap::from([(1, 100)]),
                NativeTaskBindings::default()
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(host.native.snapshot()).unwrap(), value);
    }
    for tokens in [
        BTreeMap::new(),
        BTreeMap::from([(2, 100)]),
        BTreeMap::from([(1, 0)]),
        BTreeMap::from([(1, 100), (2, 200)]),
    ] {
        assert!(
            NativeTasks::from_snapshot(
                host.native.snapshot(),
                &saved,
                &tokens,
                NativeTaskBindings::default()
            )
            .is_err()
        );
    }
}

struct DescriptorHost;
impl Host for DescriptorHost {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: &str,
        _: &[Argument],
        _: Invocation,
        _: &mut [i32],
    ) -> Result<CallResult> {
        Ok(CallResult::Pending(1))
    }
    fn poll(&mut self, _: u64, _: &[Argument], _: Invocation, _: &mut [i32]) -> Result<bool> {
        Ok(false)
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        Ok(())
    }
}
fn contract_vm(name: &str, kind: CallKind, args: &[i32]) -> Snapshot {
    let program = code(name, kind, args);
    let sources = BTreeMap::from([(CodeOrigin(10), Arc::clone(&program))]);
    let mut vm =
        Runtime::from_shared(program, CodeOrigin(10), vec![], "contract TEST".into()).unwrap();
    vm.invoke_body(Thread::Main, &mut DescriptorHost, &mut [], 20)
        .unwrap();
    vm.snapshot(&sources).unwrap()
}
fn one(state: Task) -> NativeTasksSnapshot {
    NativeTasksSnapshot {
        version: 1,
        next: 1,
        tasks: vec![SavedTask { token: 1, state }],
    }
}
fn dialogue() -> crate::script::DialogueState {
    crate::script::DialogueState::new(&cyrus_assets::script::DialogueSettings {
        menu_start: [35, 25],
        max_menu_items: 300,
        max_dialog_lines: 20,
        max_text_width: 500,
        menu_traverse_delay: 4,
        print_text: true,
        use_speech: true,
        max_distance: 1600,
        rtx_filename: "ENGLISH.RTX".into(),
    })
    .unwrap()
}

#[test]
fn external_service_bindings_are_explicit_and_do_not_reset_continuations() {
    let tokens = BTreeMap::from([(1, 100)]);
    let ambient = contract_vm("AmbientSound", CallKind::Multitask, &[3, 40, 0]);
    let state = one(Task::Ambient { channel: Some(3) });
    assert!(
        NativeTasks::from_snapshot(
            state.clone(),
            &ambient,
            &tokens,
            NativeTaskBindings::default()
        )
        .is_err()
    );
    for channels in [
        BTreeMap::new(),
        BTreeMap::from([(3, 16)]),
        BTreeMap::from([(3, 9), (4, 9)]),
    ] {
        assert!(
            NativeTasks::from_snapshot(
                state.clone(),
                &ambient,
                &tokens,
                NativeTaskBindings {
                    ambient_channels: Some(&channels),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    let channels = BTreeMap::from([(3, 9)]);
    let restored = NativeTasks::from_snapshot(
        state,
        &ambient,
        &tokens,
        NativeTaskBindings {
            ambient_channels: Some(&channels),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(restored.snapshot()).unwrap()["tasks"][0]["state"],
        json!({"Ambient":{"channel":9}})
    );
    assert!(
        NativeTasks::from_snapshot(
            one(Task::Ambient { channel: Some(16) }),
            &ambient,
            &tokens,
            NativeTaskBindings {
                ambient_channels: Some(&channels),
                ..Default::default()
            }
        )
        .is_err()
    );
    let owner = dialogue();
    for (name, args, state) in [
        ("WaitOnDialog", vec![], Task::Dialogue),
        ("RTXp", vec![123], Task::PlayerDialogue { started: true }),
        ("RTXp", vec![123], Task::PlayerDialogue { started: false }),
        ("RTX", vec![123], Task::SpeakerDialogue(Speech::default())),
        (
            "RTX",
            vec![123],
            Task::SpeakerDialogue(Speech {
                record: 4,
                sample: Some(3),
                lookup: Lookup::Done,
            }),
        ),
    ] {
        let vm = contract_vm(name, CallKind::Task, &args);
        let snapshot = one(state);
        assert!(
            NativeTasks::from_snapshot(
                snapshot.clone(),
                &vm,
                &tokens,
                NativeTaskBindings::default()
            )
            .is_err()
        );
        let expected = serde_json::to_value(&snapshot).unwrap()["tasks"][0]["state"].clone();
        let restored = NativeTasks::from_snapshot(
            snapshot,
            &vm,
            &tokens,
            NativeTaskBindings {
                dialogue: Some(&owner),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(restored.snapshot()).unwrap()["tasks"][0]["state"],
            expected
        );
        assert!(!owner.line_active(Some(0)));
    }
}

/// `RTX` and `rtxAnim` share one lookup/record/distance path; `rtxAnim` cycles its operands
/// third, fourth, second while the line is active and ends once the line is over and the last
/// requested group is no longer current.
#[test]
fn speaker_tasks_retry_pending_lookups_and_rtx_anim_cycles_its_groups() {
    use crate::script::tasks::{AnimationCall, PlayerDialogueCall};
    use crate::script::{DialogueLine, DialogueSource, ScriptActorId};
    use cyrus_assets::animation::Animations;
    use std::cell::Cell;
    #[derive(Debug)]
    struct Source(Cell<u32>);
    impl DialogueSource for Source {
        fn player_line(
            &self,
            _: [u8; 4],
            _: ScriptActorId,
            _: u32,
        ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
            if self.0.get() != 0 {
                self.0.set(self.0.get() - 1);
                return Ok(std::task::Poll::Pending);
            }
            Ok(std::task::Poll::Ready(Some(DialogueLine {
                text: "line".into(),
                sample: Some(2),
                lifetime: 100,
            })))
        }
        fn stop_sample(&self, _: u8) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let group = |index: u32| {
        json!({"group_index":index,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]})
    };
    let data = Animations {
        version: 2,
        world: 0,
        metadata: json!({"actors":[{"script_name":"ACTOR",
            "animation_groups":[group(0), group(11), group(12), group(13),
                {"group_index":5,"anim_id":1,"anim_type":1,"commands":[
                    {"opcode":0,"frame_reference":70},{"opcode":0,"frame_reference":71},{"opcode":1}]}]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    fn speaking<'a>(
        source: &'a Source,
        records: &'a mut crate::script::DialogueState,
    ) -> NativeServices<'a> {
        NativeServices {
            dialogue: Some(records),
            player_dialogue: Some(PlayerDialogueCall {
                source: Some(source),
                player: None,
                reset: Some(false),
                speaker: ScriptActorId {
                    generation: 1,
                    index: 0,
                },
                speaker_distance: Ok(0),
                player_distance: Ok(0),
                player_is_receiver: false,
            }),
            ..Default::default()
        }
    }
    let invocation = |elapsed| Invocation {
        elapsed: Some(elapsed),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };

    // A pending lookup keeps retrying on later invocations, which no longer have zero elapsed.
    let mut tasks = NativeTasks::default();
    let mut records = dialogue();
    let source = Source(Cell::new(2));
    let args = [Argument::Integer(i32::from_le_bytes(*b"TEST"))];
    assert_eq!(
        tasks
            .call(
                ("RTX", CallKind::Task),
                &args,
                invocation(0),
                &mut None,
                &mut None,
                speaking(&source, &mut records)
            )
            .unwrap(),
        Some(CallResult::Pending(1))
    );
    for elapsed in 1..=2 {
        assert!(
            !tasks
                .poll(
                    1,
                    &args,
                    invocation(elapsed),
                    &mut None,
                    &mut None,
                    speaking(&source, &mut records)
                )
                .unwrap()
        );
    }
    assert!(records.line_active(Some(1)) && !records.line_active(Some(0)));

    // rtxAnim(label, 13, 11, 12): the operands are used third, fourth, second = 11, 12, 13. An
    // unrelated group is current, so each request stays pending and the ascending order replaces it.
    let mut animation = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    animation.request(5, 0, 1).unwrap();
    let mut frame = None;
    let mut tasks = NativeTasks::default();
    let mut records = dialogue();
    let source = Source(Cell::new(0));
    let args = [
        Argument::Integer(i32::from_le_bytes(*b"TEST")),
        Argument::Integer(13),
        Argument::Integer(11),
        Argument::Integer(12),
    ];
    let mut input = Some(AnimationCall {
        animation: &mut animation,
        ticks: 1,
        frame: &mut frame,
        sounds: None,
    });
    let state = |input: &Option<AnimationCall<'_, '_>>| {
        let animation = &input.as_ref().unwrap().animation;
        (animation.current_group(), animation.pending_group())
    };
    assert_eq!(
        tasks
            .call(
                ("rtxAnim", CallKind::Task),
                &args,
                invocation(0),
                &mut input,
                &mut None,
                speaking(&source, &mut records)
            )
            .unwrap(),
        Some(CallResult::Pending(1))
    );
    // The first request (11) is followed in the same invocation by the cycle step (12).
    assert_eq!(state(&input), (Some(5), Some(12)));
    assert!(records.line_active(Some(1)));
    // While the line is active and the requested group is not current, the next operand is requested;
    // the wrap back to 11 cannot replace the higher pending 13.
    for (elapsed, pending) in [(1, 13), (2, 13)] {
        assert!(
            !tasks
                .poll(
                    1,
                    &args,
                    invocation(elapsed),
                    &mut input,
                    &mut None,
                    speaking(&source, &mut records)
                )
                .unwrap()
        );
        assert_eq!(state(&input), (Some(5), Some(pending)));
    }
    // Once the line is over and the last requested group (11, after the wrap) is not current, it completes.
    records.clear_line(Some(1));
    assert!(
        tasks
            .poll(
                1,
                &args,
                invocation(3),
                &mut input,
                &mut None,
                speaking(&source, &mut records)
            )
            .unwrap()
    );
}

/// `RTXpAnim` speaks and animates as the selected player: the record is theirs, their animation
/// receives the requests, and only their own distance gates it, however far the receiver is.
#[test]
fn rtxp_anim_speaks_and_animates_as_the_selected_player() {
    use crate::script::tasks::{AnimationCall, PlayerDialogueCall};
    use crate::script::{DialogueLine, DialogueSource, ScriptActorId};
    use cyrus_assets::animation::Animations;
    #[derive(Debug)]
    struct Source;
    impl DialogueSource for Source {
        fn player_line(
            &self,
            _: [u8; 4],
            _: ScriptActorId,
            _: u32,
        ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
            Ok(std::task::Poll::Ready(Some(DialogueLine {
                text: "line".into(),
                sample: Some(2),
                lifetime: 100,
            })))
        }
        fn stop_sample(&self, _: u8) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let group = |index: u32| {
        json!({"group_index":index,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]})
    };
    let data = Animations {
        version: 2,
        world: 0,
        metadata: json!({"actors":[{"script_name":"ACTOR",
            "animation_groups":[group(0), group(11), group(12), group(13)]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let receiver = ScriptActorId {
        generation: 1,
        index: 0,
    };
    let player = ScriptActorId {
        generation: 1,
        index: 2,
    };
    fn speaking<'a>(
        records: &'a mut crate::script::DialogueState,
        player_distance: i32,
        players: ScriptActorId,
        receiver: ScriptActorId,
        animation: Option<AnimationCall<'a, 'a>>,
    ) -> NativeServices<'a> {
        NativeServices {
            dialogue: Some(records),
            player_animation: animation,
            player_dialogue: Some(PlayerDialogueCall {
                source: Some(&Source),
                player: Some(players),
                reset: Some(false),
                speaker: receiver,
                // The receiver is far away, which must not matter for RTXpAnim.
                speaker_distance: Ok(99_999),
                player_distance: Ok(player_distance),
                player_is_receiver: false,
            }),
            ..Default::default()
        }
    }
    let invocation = |elapsed| Invocation {
        elapsed: Some(elapsed),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };
    let args = [
        Argument::Integer(i32::from_le_bytes(*b"TEST")),
        Argument::Integer(13),
        Argument::Integer(11),
        Argument::Integer(12),
    ];
    let mut animation = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut frame = None;
    let mut records = dialogue();
    let mut tasks = NativeTasks::default();
    let result = tasks
        .call(
            ("RTXpAnim", CallKind::Task),
            &args,
            invocation(0),
            // The receiver has no animation of its own; the player's serves.
            &mut None,
            &mut None,
            speaking(
                &mut records,
                0,
                player,
                receiver,
                Some(AnimationCall {
                    animation: &mut animation,
                    ticks: 1,
                    frame: &mut frame,
                    sounds: None,
                }),
            ),
        )
        .unwrap();
    assert_eq!(result, Some(CallResult::Pending(1)));
    assert_eq!(animation.current_group(), Some(11));
    let snapshot = serde_json::to_value(&records).unwrap();
    assert_eq!(
        snapshot["records"][1]["owner"],
        serde_json::to_value(player).unwrap()
    );
    // Beyond the limit for the player's own distance, the task completes and cuts the line.
    assert!(
        tasks
            .poll(
                1,
                &args,
                invocation(1),
                &mut None,
                &mut None,
                speaking(&mut records, 1601, player, receiver, None)
            )
            .unwrap()
    );
    assert!(!records.line_active(Some(1)));
}

#[test]
fn animation_restore_keeps_started_state_without_repeating_requests_or_cues() {
    use crate::script::tasks::AnimationCall;
    use cyrus_assets::animation::Animations;
    let data = Animations {
        version: 2,
        world: 0,
        metadata: json!({"actors":[{
        "script_name":"ACTOR", "animation_groups":[
            {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]},
            {"group_index":7,"anim_id":1,"anim_type":1,"commands":[{"opcode":4,"sound_param":4,"volume_shift":0},{"opcode":0,"frame_reference":70},{"opcode":0,"frame_reference":71},{"opcode":1}]}
        ]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut animation = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut frame = None;
    let mut sounds = vec![];
    let mut tasks = NativeTasks::default();
    let args = [Argument::Integer(7), Argument::Integer(2)];
    let invocation = Invocation {
        elapsed: Some(0),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };
    {
        let mut emit = |sound| {
            sounds.push(sound);
            Ok(())
        };
        let mut input = Some(AnimationCall {
            animation: &mut animation,
            ticks: 1,
            frame: &mut frame,
            sounds: Some(&mut emit),
        });
        assert_eq!(
            tasks
                .call(
                    ("PushAnimation", CallKind::Task),
                    &args,
                    invocation,
                    &mut input,
                    &mut None,
                    NativeServices::default()
                )
                .unwrap(),
            Some(CallResult::Pending(1))
        );
        assert!(
            !tasks
                .poll(
                    1,
                    &args,
                    Invocation {
                        elapsed: Some(1),
                        ..invocation
                    },
                    &mut input,
                    &mut None,
                    NativeServices::default()
                )
                .unwrap()
        );
    }
    assert_eq!(sounds.len(), 1);
    animation.set_loop_count(1).unwrap();
    let vm = contract_vm("PushAnimation", CallKind::Task, &[7, 2]);
    let tokens = BTreeMap::from([(1, 100)]);
    assert!(
        NativeTasks::from_snapshot(
            tasks.snapshot(),
            &vm,
            &tokens,
            NativeTaskBindings::default()
        )
        .is_err()
    );
    let mut invalid_group = tasks.snapshot();
    invalid_group.tasks[0].state = Task::Animation {
        group: u16::MAX,
        started: true,
    };
    assert!(
        NativeTasks::from_snapshot(
            invalid_group,
            &vm,
            &tokens,
            NativeTaskBindings {
                animation: Some(&animation),
                ..Default::default()
            },
        )
        .is_err()
    );
    let mut restored = NativeTasks::from_snapshot(
        tasks.snapshot(),
        &vm,
        &tokens,
        NativeTaskBindings {
            animation: Some(&animation),
            ..Default::default()
        },
    )
    .unwrap();
    let before = serde_json::to_value(animation.snapshot()).unwrap();
    sounds.clear();
    let mut emit = |sound| {
        sounds.push(sound);
        Ok(())
    };
    assert!(
        !restored
            .poll(
                100,
                &args,
                Invocation {
                    elapsed: Some(2),
                    ..invocation
                },
                &mut Some(AnimationCall {
                    animation: &mut animation,
                    ticks: 1,
                    frame: &mut frame,
                    sounds: Some(&mut emit)
                }),
                &mut None,
                NativeServices::default()
            )
            .unwrap()
    );
    assert_eq!(serde_json::to_value(animation.snapshot()).unwrap(), before);
    assert!(sounds.is_empty());

    let replacement_data = Animations {
        metadata: json!({"actors":[{
            "script_name":"ACTOR", "animation_groups":[
                {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}
            ]
        }]}),
        ..data
    };
    let mut replacement = ActorAnimation::from_asset(&replacement_data, "ACTOR", 0).unwrap();
    assert!(!replacement.has_group(7));
    let before = serde_json::to_value(replacement.snapshot()).unwrap();
    let mut rebound = NativeTasks::from_snapshot(
        tasks.snapshot(),
        &vm,
        &tokens,
        NativeTaskBindings {
            animation: Some(&replacement),
            ..Default::default()
        },
    )
    .unwrap();
    let mut emit = |sound| {
        sounds.push(sound);
        Ok(())
    };
    for (native, token) in [(&mut tasks, 1), (&mut rebound, 100)] {
        assert!(
            native
                .poll(
                    token,
                    &args,
                    Invocation {
                        elapsed: Some(2),
                        ..invocation
                    },
                    &mut Some(AnimationCall {
                        animation: &mut replacement,
                        ticks: 1,
                        frame: &mut frame,
                        sounds: Some(&mut emit)
                    }),
                    &mut None,
                    NativeServices::default(),
                )
                .unwrap()
        );
        assert!(native.snapshot().tasks.is_empty());
        assert_eq!(
            serde_json::to_value(replacement.snapshot()).unwrap(),
            before
        );
    }
    assert!(sounds.is_empty());
}

#[test]
fn duplicate_tokens_and_paused_vm_slots_are_checked_with_two_native_tasks() {
    let mut program = (*code("Wait", CallKind::Multitask, &[4])).clone();
    let call = program.steps[&0].operation.clone();
    program.steps.insert(
        1,
        Step {
            next: 2,
            operation: call,
        },
    );
    program.steps.insert(
        2,
        Step {
            next: 3,
            operation: Operation::End { target: 0 },
        },
    );
    let program = Arc::new(program);
    let sources = BTreeMap::from([(CodeOrigin(10), Arc::clone(&program))]);
    let mut vm = Runtime::from_shared(program, CodeOrigin(10), vec![], "two TEST".into()).unwrap();
    let mut host = NativeHost::default();
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    let mut document = serde_json::to_value(vm.snapshot(&sources).unwrap()).unwrap();
    document["state"]["tasks"][0]["paused"] = json!(true);
    let saved: Snapshot = serde_json::from_value(document).unwrap();
    let snapshot = host.native.snapshot();
    assert_eq!(snapshot.tasks.len(), 2);
    assert!(
        NativeTasks::from_snapshot(
            snapshot.clone(),
            &saved,
            &BTreeMap::from([(1, 100), (2, 100)]),
            NativeTaskBindings::default()
        )
        .is_err()
    );
    let mut duplicate = snapshot.clone();
    duplicate.tasks[1].token = 1;
    assert!(
        NativeTasks::from_snapshot(
            duplicate,
            &saved,
            &BTreeMap::from([(1, 100), (2, 200)]),
            NativeTaskBindings::default()
        )
        .is_err()
    );
    let tokens = BTreeMap::from([(1, 100), (2, 200)]);
    let native =
        NativeTasks::from_snapshot(snapshot, &saved, &tokens, NativeTaskBindings::default())
            .unwrap();
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(10),
        saved,
        &sources,
        &tokens,
        "two restored TEST".into(),
    )
    .unwrap();
    let mut host = NativeHost {
        native,
        ..Default::default()
    };
    restored
        .invoke_tasks_timed(&mut host, &mut [], 256)
        .unwrap();
    assert_eq!(host.polls, 1);
    assert_eq!(host.calls, 0);
    assert_eq!(restored.active_tasks(), 2);
}

#[test]
fn same_shape_wrong_native_variant_is_rejected() {
    let (vm, host, sources) = begin("WaitOnTasks", &[]);
    let snapshot = one(Task::Blocking);
    let error = NativeTasks::from_snapshot(
        snapshot,
        &vm.snapshot(&sources).unwrap(),
        &BTreeMap::from([(1, 100)]),
        NativeTaskBindings::default(),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("variant WaitNonMulti differs from VM call WaitOnTasks"));
    assert_eq!(host.native.tasks.len(), 1);
}

#[test]
fn empty_restore_preserves_allocator_history_and_exhaustion() {
    let program = code("Wait", CallKind::Task, &[4]);
    let sources = BTreeMap::from([(CodeOrigin(10), Arc::clone(&program))]);
    let vm = Runtime::from_shared(program, CodeOrigin(10), vec![], "empty TEST".into()).unwrap();
    let saved = vm.snapshot(&sources).unwrap();
    for (next, expected) in [(40, Some(41)), (u64::MAX, None)] {
        let native = NativeTasks::from_snapshot(
            NativeTasksSnapshot {
                version: 1,
                next,
                tasks: vec![],
            },
            &saved,
            &BTreeMap::new(),
            NativeTaskBindings::default(),
        )
        .unwrap();
        let mut host = NativeHost {
            native,
            ..Default::default()
        };
        let result = host.call(
            &Target::Me,
            CallKind::Task,
            "Wait",
            &[Argument::Integer(4)],
            Invocation {
                elapsed: Some(0),
                unpaused_tasks: 0,
                unpaused_blocking_tasks: 0,
            },
            &mut [],
        );
        if let Some(token) = expected {
            assert_eq!(result.unwrap(), CallResult::Pending(token));
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains("identity exhausted"));
            assert!(host.native.tasks.is_empty());
        }
    }
}

#[test]
fn frame_wait_restore_preserves_variant_and_reevaluates_operands() {
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 17,
        metadata: json!({"actors":[{"script_name":"ACTOR","animation_groups":[
            {"group_index":7,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}
        ]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let animation = ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
    for (name, player) in [("WaitAnimFrame", false), ("WaitPlayerAnimFrame", true)] {
        let (vm, host, sources) = begin(name, &[7, 10]);
        let saved = vm.snapshot(&sources).unwrap();
        let tokens = BTreeMap::from([(1, 100)]);
        let bindings = || NativeTaskBindings {
            animation: Some(&animation),
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&host.native.snapshot()).unwrap();
        let mut native = NativeTasks::from_snapshot(
            serde_json::from_slice(&bytes).unwrap(),
            &saved,
            &tokens,
            bindings(),
        )
        .unwrap();
        let wrong = one(Task::AnimationFrame { player: !player });
        assert!(NativeTasks::from_snapshot(wrong, &saved, &tokens, bindings()).is_err());
        if !player {
            assert!(
                NativeTasks::from_snapshot(
                    host.native.snapshot(),
                    &saved,
                    &tokens,
                    NativeTaskBindings::default()
                )
                .is_err()
            );
        }
        let inputs = || NativeServices {
            frame_wait: Some(crate::script::tasks::FrameWaitCall {
                receiver: Ok((7, -1, 5)),
                player: Ok(Some((7, -1, 5))),
            }),
            ..Default::default()
        };
        let invocation = Invocation {
            elapsed: Some(1),
            unpaused_tasks: 1,
            unpaused_blocking_tasks: 1,
        };
        assert!(
            !native
                .poll(
                    100,
                    &[Argument::Integer(7), Argument::Integer(10)],
                    invocation,
                    &mut None,
                    &mut None,
                    inputs()
                )
                .unwrap()
        );
        assert!(
            native
                .poll(
                    100,
                    &[Argument::Integer(7), Argument::Integer(5)],
                    invocation,
                    &mut None,
                    &mut None,
                    inputs()
                )
                .unwrap()
        );
    }
}

#[test]
fn walk_forward_task_selects_groups_and_scales_the_zero_step_fallback_before_frame_scaling() {
    use crate::script::{
        locomotion::BackwardState,
        tasks::{AnimationCall, LocomotionAdmission, LocomotionCall, MotionCall},
    };
    use cyrus_assets::animation::Animations;
    use cyrus_world::animation::ActorAnimation;
    for (combat, torch, step, group, available, expected) in [
        (false, false, 8, 20, true, [512, 1194, 2218, 4266]),
        (false, true, 8, 21, true, [512, 1194, 2218, 4266]),
        (true, false, 8, 31, true, [512, 1194, 2218, 4266]),
        (false, false, 0, 20, true, [2, 4, 8, 16]),
        (false, false, 8, 20, false, [0; 4]),
    ] {
        let data = Animations {
            version: 2,
            world: 0,
            metadata: json!({"actors":[{"script_name":"ACTOR", "animation_groups":[
                {"group_index":if available { group } else { 0 },"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}
            ]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut animation =
            ActorAnimation::from_asset(&data, "ACTOR", if available { group } else { 0 }).unwrap();
        let mut task = Task::WalkForward;
        let mut position = [0; 3];
        let mut angles = [0; 3];
        let mut counters = 0;
        let mut direction = 0;
        let mut called = 0;
        let mut frame = None;
        let mut updates = vec![];
        for (elapsed, z) in expected.into_iter().enumerate() {
            let done = task
                .complete(
                    &[Argument::Integer(4)],
                    Invocation {
                        elapsed: Some(elapsed as i32),
                        unpaused_tasks: 0,
                        unpaused_blocking_tasks: 0,
                    },
                    &mut Some(AnimationCall {
                        animation: &mut animation,
                        ticks: 1,
                        frame: &mut frame,
                        sounds: None,
                    }),
                    &mut Some(SpatialCall {
                        position: &mut position,
                        angles: &mut angles,
                        frame_scale: 256,
                        nominal_ticks: 1,
                        matrix: Matrix::IDENTITY,
                        slave: false,
                        locations: &[],
                        effective_position: None,
                        player_position: None,
                        actor_index: 0,
                        map_updates: &mut updates,
                    }),
                    NativeServices {
                        locomotion: Some(LocomotionCall {
                            state: Ok(BackwardState {
                                combat_mode: combat,
                                torch_burning: torch,
                                walk_step_attribute: step,
                                walk_combat_step_attribute: step,
                                forward_step_attribute: 17,
                                step_adjustment: 1,
                                ..Default::default()
                            }),
                            admission: LocomotionAdmission {
                                forward_step_clear: Some(true),
                                walk_forward_mode: true,
                                forward_admitted: Some(true),
                                ..Default::default()
                            },
                            animation_system: false,
                            motion: Some(MotionCall {
                                direction: &mut direction,
                                counters: &mut counters,
                            }),
                            called: &mut called,
                        }),
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(done, elapsed == 3);
            assert_eq!(position, [0, 0, z]);
            assert_eq!(direction, 6);
            assert_eq!(called, if available { 4 } else { 0 });
            assert_eq!(
                counters,
                if available {
                    ((elapsed + 1).min(3) as u8) << 4
                } else {
                    0
                }
            );
            assert_eq!(
                animation.current_group(),
                if available { Some(group) } else { None }
            );
        }
    }
}

#[test]
fn walk_forward_native_snapshot_preserves_variant_and_rebinds_the_task_token() {
    let vm = contract_vm("WalkForward", CallKind::Task, &[4]);
    let restored = NativeTasks::from_snapshot(
        one(Task::WalkForward),
        &vm,
        &BTreeMap::from([(1, 9)]),
        NativeTaskBindings::default(),
    )
    .unwrap();
    assert!(matches!(
        restored.locomotion_operation(9),
        Some(crate::script::tasks::LocomotionOperation::WalkForward)
    ));
    assert!(restored.locomotion_operation(1).is_none());
    assert!(
        NativeTasks::from_snapshot(
            one(Task::WalkForward),
            &contract_vm("MoveForward", CallKind::Task, &[4]),
            &BTreeMap::from([(1, 9)]),
            NativeTaskBindings::default()
        )
        .is_err()
    );
}

#[test]
fn walk_forward_rechecks_step_admission_after_the_startup_counter_saturates() {
    use crate::script::tasks::{LocomotionAdmission, LocomotionCall, MotionCall};
    let mut counters = 48;
    let mut direction = 0;
    let mut called = 0;
    assert!(
        Task::WalkForward
            .complete(
                &[Argument::Integer(4)],
                Invocation {
                    elapsed: Some(0),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
                &mut None,
                &mut None,
                NativeServices {
                    locomotion: Some(LocomotionCall {
                        state: Ok(Default::default()),
                        admission: LocomotionAdmission {
                            forward_step_clear: Some(false),
                            walk_forward_mode: true,
                            forward_admitted: Some(true),
                            ..Default::default()
                        },
                        animation_system: false,
                        motion: Some(MotionCall {
                            direction: &mut direction,
                            counters: &mut counters
                        }),
                        called: &mut called,
                    }),
                    ..Default::default()
                }
            )
            .unwrap()
    );
    assert_eq!((direction, counters, called), (0, 48, 0));
}

#[test]
fn synchronous_movement_ignores_task_identity_and_script_calls_keep_their_slots() {
    use crate::script::tasks::{LocomotionAdmission, LocomotionCall};
    let mut called = 0;
    let mut tasks = NativeTasks {
        next: u64::MAX,
        ..Default::default()
    };
    let invocation = Invocation {
        elapsed: Some(0),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };
    let refused = || crate::script::locomotion::BackwardState {
        rope_attached: true,
        ..Default::default()
    };
    assert!(matches!(
        tasks
            .call_synchronous_movement(
                "MoveForward",
                &[Argument::Integer(1)],
                invocation,
                &mut None,
                &mut None,
                NativeServices {
                    locomotion: Some(LocomotionCall {
                        state: Ok(refused()),
                        admission: LocomotionAdmission::default(),
                        animation_system: true,
                        motion: None,
                        called: &mut called
                    }),
                    ..Default::default()
                }
            )
            .unwrap(),
        Some(CallResult::Complete(0))
    ));
    assert_eq!(tasks.next, u64::MAX);
    assert!(tasks.tasks.is_empty());
    assert_eq!(called, 0);
    let error = tasks
        .call_synchronous_movement(
            "Wait",
            &[Argument::Integer(1)],
            invocation,
            &mut None,
            &mut None,
            NativeServices::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("not a movement operation"));
    let error = tasks
        .call(
            ("MoveForward", CallKind::Function),
            &[Argument::Integer(1)],
            invocation,
            &mut None,
            &mut None,
            NativeServices::default(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("function-encoded MoveForward timing unsupported")
    );
    let error = tasks
        .call(
            ("MoveForward", CallKind::Task),
            &[Argument::Integer(1)],
            invocation,
            &mut None,
            &mut None,
            NativeServices::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("identity exhausted"));
    assert_eq!(tasks.next, u64::MAX);
    assert!(tasks.tasks.is_empty());

    let mut tasks = NativeTasks::default();
    assert!(matches!(
        tasks
            .call(
                ("MoveForward", CallKind::Task),
                &[Argument::Integer(1)],
                invocation,
                &mut None,
                &mut None,
                NativeServices {
                    locomotion: Some(LocomotionCall {
                        state: Ok(refused()),
                        admission: LocomotionAdmission::default(),
                        animation_system: true,
                        motion: None,
                        called: &mut called
                    }),
                    ..Default::default()
                }
            )
            .unwrap(),
        Some(CallResult::Pending(1))
    ));
    assert_eq!(tasks.next, 1);
    assert_eq!(tasks.tasks.len(), 1);
    assert!(matches!(
        tasks
            .call_synchronous_movement(
                "MoveForward",
                &[Argument::Integer(1)],
                invocation,
                &mut None,
                &mut None,
                NativeServices {
                    locomotion: Some(LocomotionCall {
                        state: Ok(refused()),
                        admission: LocomotionAdmission::default(),
                        animation_system: true,
                        motion: None,
                        called: &mut called
                    }),
                    ..Default::default()
                }
            )
            .unwrap(),
        Some(CallResult::Complete(0))
    ));
    assert_eq!(tasks.next, 1);
    assert_eq!(tasks.tasks.len(), 1);
}
