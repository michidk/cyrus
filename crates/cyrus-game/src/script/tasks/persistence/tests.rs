//! Native continuations resume through VM calls and explicit restored service bindings.
use super::*;
use crate::script::tasks::{
    Lookup, NativeServices, PlayerFaceCall, PlayerFaceOutcome, PlayerFaceState, SpatialCall, Speech,
};
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
            turn: 0,
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
            turn: 0,
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
    // AmbientRtx rebinds its saved clip slot through the same channel map; a missing binding fails.
    let rtx = contract_vm("AmbientRtx", CallKind::Multitask, &[3, 40, 0]);
    let saved = one(Task::AmbientRtx {
        slot: Some(3),
        lookup: Lookup::Done,
    });
    assert!(
        NativeTasks::from_snapshot(saved.clone(), &rtx, &tokens, NativeTaskBindings::default())
            .is_err()
    );
    let restored = NativeTasks::from_snapshot(
        saved,
        &rtx,
        &tokens,
        NativeTaskBindings {
            ambient_channels: Some(&channels),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(restored.snapshot()).unwrap()["tasks"][0]["state"],
        json!({"AmbientRtx":{"slot":9,"lookup":"Done"}})
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
        fn update_ambient_rtx(&self, _: u8, _: [i32; 3], _: i32, _: bool) -> anyhow::Result<bool> {
            Ok(false)
        }
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
        fn update_ambient_rtx(&self, _: u8, _: [i32; 3], _: i32, _: bool) -> anyhow::Result<bool> {
            Ok(false)
        }
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

/// `AmbientRtx` starts the label's clip once, creates no dialogue line, updates volume and pan each
/// run with its volume offset and the receiver's killed state, and ends when the clip has finished.
#[test]
fn ambient_rtx_follows_its_clip_without_creating_a_dialogue_line() {
    use crate::script::tasks::{AmbientCall, PlayerDialogueCall};
    use crate::script::{DialogueLine, DialogueSource, ScriptActorId};
    use std::cell::{Cell, RefCell};
    /// Sample slot, receiver position, volume offset and killed state of one run.
    type Update = (u8, [i32; 3], i32, bool);
    #[derive(Debug, Default)]
    struct Source {
        pending: Cell<u32>,
        sample: Cell<Option<u8>>,
        playing: Cell<bool>,
        lookups: Cell<u32>,
        updates: RefCell<Vec<Update>>,
    }
    impl DialogueSource for Source {
        fn player_line(
            &self,
            _: [u8; 4],
            _: ScriptActorId,
            _: u32,
        ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
            self.lookups.set(self.lookups.get() + 1);
            if self.pending.get() != 0 {
                self.pending.set(self.pending.get() - 1);
                return Ok(std::task::Poll::Pending);
            }
            Ok(std::task::Poll::Ready(Some(DialogueLine {
                text: "ignored".into(),
                sample: self.sample.get(),
                lifetime: 100,
            })))
        }
        fn stop_sample(&self, _: u8) -> anyhow::Result<()> {
            Ok(())
        }
        fn update_ambient_rtx(
            &self,
            sample: u8,
            position: [i32; 3],
            offset: i32,
            killed: bool,
        ) -> anyhow::Result<bool> {
            self.updates
                .borrow_mut()
                .push((sample, position, offset, killed));
            Ok(self.playing.get() && !killed)
        }
    }
    fn services<'a>(
        source: &'a Source,
        records: &'a mut crate::script::DialogueState,
        ambient: &'a AmbientCall<'a>,
    ) -> NativeServices<'a> {
        NativeServices {
            ambient: Some(ambient),
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
    let alive = AmbientCall {
        driver: None,
        reset: Some(false),
        position: Some([256, 0, -512]),
        excluded: Some(false),
        killed: Some(false),
        generation: 1,
        actor_index: 0,
    };
    let dead = AmbientCall {
        killed: Some(true),
        ..alive
    };
    let invocation = |elapsed| Invocation {
        elapsed: Some(elapsed),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };
    let args = [
        Argument::Integer(i32::from_le_bytes(*b"TEST")),
        Argument::Integer(-7),
        Argument::Integer(0),
    ];
    let call = ("AmbientRtx", CallKind::Multitask);

    // A pending lookup retries; the started clip is then followed with the task's offset.
    let source = Source {
        pending: Cell::new(1),
        sample: Cell::new(Some(5)),
        playing: Cell::new(true),
        ..Default::default()
    };
    let mut records = dialogue();
    let mut tasks = NativeTasks::default();
    assert_eq!(
        tasks
            .call(
                call,
                &args,
                invocation(0),
                &mut None,
                &mut None,
                services(&source, &mut records, &alive)
            )
            .unwrap(),
        Some(CallResult::Pending(1))
    );
    assert!(source.updates.borrow().is_empty());
    assert!(
        !tasks
            .poll(
                1,
                &args,
                invocation(1),
                &mut None,
                &mut None,
                services(&source, &mut records, &alive)
            )
            .unwrap()
    );
    assert_eq!(*source.updates.borrow(), [(5, [256, 0, -512], -7, false)]);
    assert_eq!(
        source.lookups.get(),
        2,
        "the lookup is not repeated once started"
    );
    assert!(!records.waiting(), "no dialogue line is created");
    // The clip finishing ends the task; the next run does not look it up again.
    source.playing.set(false);
    assert!(
        tasks
            .poll(
                1,
                &args,
                invocation(2),
                &mut None,
                &mut None,
                services(&source, &mut records, &alive)
            )
            .unwrap()
    );
    assert_eq!(source.lookups.get(), 2);

    // A killed receiver is passed through and ends the task.
    let source = Source {
        sample: Cell::new(Some(5)),
        playing: Cell::new(true),
        ..Default::default()
    };
    let mut tasks = NativeTasks::default();
    tasks
        .call(
            call,
            &args,
            invocation(0),
            &mut None,
            &mut None,
            services(&source, &mut records, &alive),
        )
        .unwrap();
    assert!(
        tasks
            .poll(
                1,
                &args,
                invocation(1),
                &mut None,
                &mut None,
                services(&source, &mut records, &dead)
            )
            .unwrap()
    );
    assert!(source.updates.borrow().last().unwrap().3);

    // A text-only entry has no clip: the task completes on its next poll and never updates a channel.
    let source = Source::default();
    let mut tasks = NativeTasks::default();
    assert_eq!(
        tasks
            .call(
                call,
                &args,
                invocation(0),
                &mut None,
                &mut None,
                services(&source, &mut records, &alive)
            )
            .unwrap(),
        Some(CallResult::Pending(1))
    );
    assert!(
        tasks
            .poll(
                1,
                &args,
                invocation(1),
                &mut None,
                &mut None,
                services(&source, &mut records, &alive)
            )
            .unwrap()
    );
    assert!(source.updates.borrow().is_empty());
}

fn player_face(receiver: [i32; 3], player: PlayerFaceState) -> PlayerFaceOutcome {
    let mut tasks = NativeTasks::default();
    let mut position = receiver;
    let mut angles = [0; 3];
    let mut updates = vec![];
    let mut spatial = Some(SpatialCall {
        position: &mut position,
        angles: &mut angles,
        frame_scale: 256,
        turn: 0,
        nominal_ticks: 8,
        matrix: Matrix::IDENTITY,
        slave: false,
        locations: &[],
        effective_position: None,
        player_position: None,
        actor_index: 5,
        map_updates: &mut updates,
    });
    let mut outcome = PlayerFaceOutcome::default();
    let result = tasks
        .call(
            ("PlayerFaceObj", CallKind::Task),
            &[],
            Invocation {
                elapsed: Some(0),
                unpaused_tasks: 0,
                unpaused_blocking_tasks: 0,
            },
            &mut None,
            &mut spatial,
            NativeServices {
                player_face: Some(PlayerFaceCall {
                    state: Ok(player),
                    outcome: &mut outcome,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(result, Some(CallResult::Pending(1)));
    outcome
}

fn face_state() -> PlayerFaceState {
    PlayerFaceState {
        position: [256, 77, 0],
        angles: [0, 1024, 600],
        speed: 0,
        rate_attribute: 12,
        holding_weapon: false,
        torch: false,
        response_present: false,
        response_age: 0,
    }
}

#[test]
fn player_face_obj_snaps_onto_a_near_receiver_and_levels_the_roll_when_facing() {
    // The receiver is 256 away (the speed-zero facing threshold): the player's X/Z snap, Y stays.
    let outcome = player_face([512, 5, 0], face_state());
    assert_eq!(outcome.position, Some([512, 77, 0]));
    assert!(!outcome.aligning);
    assert_eq!(outcome.return_state, None);
    // Far away and already facing it (heading 1024 faces negative Z): the roll levels by the
    // elapsed ticks (600 -> 592) and the alignment is complete.
    let outcome = player_face([256, 0, -25600], face_state());
    assert_eq!(outcome.position, None);
    assert_eq!(outcome.angles, Some([0, 1024, 592]));
    assert!(!outcome.aligning);
}

#[test]
fn player_face_obj_abandons_after_a_stale_collision_response() {
    let mut player = face_state();
    player.response_present = true;
    player.response_age = 9;
    let outcome = player_face([256, 0, -25600], player);
    assert_eq!(outcome.return_state, Some(6));
    assert_eq!(outcome.angles, None);
    assert!(!outcome.aligning);
}

#[test]
fn wander_to_location_fixes_its_speed_on_the_first_call_and_snaps_when_within_a_step() {
    let mut tasks = NativeTasks::default();
    let args = [Argument::Integer(1), Argument::Integer(4)];
    let mut position = [0; 3];
    let mut angles = [0; 3];
    let mut updates = vec![];
    let locations = [[0, 0, 0], [0, 0, 10240]];
    let mut step = |tasks: &mut NativeTasks, position: &mut [i32; 3], token| {
        let invocation = Invocation {
            elapsed: Some(0),
            unpaused_tasks: 0,
            unpaused_blocking_tasks: 0,
        };
        let mut spatial = Some(SpatialCall {
            position,
            angles: &mut angles,
            frame_scale: 256,
            turn: 0,
            nominal_ticks: 1,
            matrix: Matrix::IDENTITY,
            slave: false,
            locations: &locations,
            effective_position: None,
            player_position: None,
            actor_index: 5,
            map_updates: &mut updates,
        });
        match token {
            None => {
                assert_eq!(
                    tasks
                        .call(
                            ("WanderToLocation", CallKind::Task),
                            &args,
                            invocation,
                            &mut None,
                            &mut spatial,
                            NativeServices::default(),
                        )
                        .unwrap(),
                    Some(CallResult::Pending(1))
                );
                false
            }
            Some(token) => tasks
                .poll(
                    token,
                    &args,
                    invocation,
                    &mut None,
                    &mut spatial,
                    NativeServices::default(),
                )
                .unwrap(),
        }
    };
    // 10240 / 4 = 2560 per update at scale 256; the fourth call is within one step and snaps.
    step(&mut tasks, &mut position, None);
    assert_eq!(position, [0, 0, 2560]);
    assert!(!step(&mut tasks, &mut position, Some(1)));
    assert_eq!(position, [0, 0, 5120]);
    assert!(!step(&mut tasks, &mut position, Some(1)));
    assert_eq!(position, [0, 0, 7680]);
    assert!(step(&mut tasks, &mut position, Some(1)));
    assert_eq!(position, [0, 0, 10240]);
    assert_eq!(updates, vec![5]);
}

#[test]
fn move_along_axis_matches_the_original_routine() {
    // Rows run through the original MoveAlongAxis over synthetic actors with the published rotation
    // table (research oracle): [[pitch, yaw, roll], axis, delta, duration, scale, steps] where each
    // step is [[x, y, z] after the call, original return] for elapsed 0, 1, ... from (1000, 2000, 3000).
    // An ordinary actor rotates the step by its angles, the inverse of the stored matrix's sense.
    let rows: Vec<serde_json::Value> = serde_json::from_str(r#"[[[0,512,0],0,1024,4,256,[[[1000,2000,2744],1],[[1000,2000,2488],1],[[1000,2000,2232],1],[[1000,2000,2232],0]]],[[0,512,0],0,1024,4,100,[[[1000,2000,2900],1],[[1000,2000,2800],1],[[1000,2000,2700],1],[[1000,2000,2600],0]]],[[0,512,0],0,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,512,0],0,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,512,0],0,-1536,4,256,[[[1000,2000,3512],1],[[1000,2000,4024],1],[[1000,2000,4024],0]]],[[0,512,0],0,-1536,4,100,[[[1000,2000,3200],1],[[1000,2000,3400],1],[[1000,2000,3600],1],[[1000,2000,3800],0]]],[[0,512,0],0,-1536,7,256,[[[1000,2000,3256],1],[[1000,2000,3512],1],[[1000,2000,3768],1],[[1000,2000,4024],1],[[1000,2000,4280],1],[[1000,2000,4280],0]]],[[0,512,0],0,-1536,7,100,[[[1000,2000,3100],1],[[1000,2000,3200],1],[[1000,2000,3300],1],[[1000,2000,3400],1],[[1000,2000,3500],1],[[1000,2000,3600],1],[[1000,2000,3700],0]]],[[0,512,0],0,4096,4,256,[[[1000,2000,1976],1],[[1000,2000,952],1],[[1000,2000,-72],1],[[1000,2000,-72],0]]],[[0,512,0],0,4096,4,100,[[[1000,2000,2600],1],[[1000,2000,2200],1],[[1000,2000,1800],1],[[1000,2000,1400],0]]],[[0,512,0],0,4096,7,256,[[[1000,2000,2488],1],[[1000,2000,1976],1],[[1000,2000,1464],1],[[1000,2000,952],1],[[1000,2000,440],1],[[1000,2000,-72],1],[[1000,2000,-584],0]]],[[0,512,0],0,4096,7,100,[[[1000,2000,2800],1],[[1000,2000,2600],1],[[1000,2000,2400],1],[[1000,2000,2200],1],[[1000,2000,2000],1],[[1000,2000,1800],1],[[1000,2000,1600],0]]],[[0,512,0],1,1024,4,256,[[[1000,2256,3000],1],[[1000,2512,3000],1],[[1000,2768,3000],1],[[1000,2768,3000],0]]],[[0,512,0],1,1024,4,100,[[[1000,2100,3000],1],[[1000,2200,3000],1],[[1000,2300,3000],1],[[1000,2400,3000],0]]],[[0,512,0],1,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,512,0],1,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,512,0],1,-1536,4,256,[[[1000,1488,3000],1],[[1000,976,3000],1],[[1000,976,3000],0]]],[[0,512,0],1,-1536,4,100,[[[1000,1800,3000],1],[[1000,1600,3000],1],[[1000,1400,3000],1],[[1000,1200,3000],0]]],[[0,512,0],1,-1536,7,256,[[[1000,1744,3000],1],[[1000,1488,3000],1],[[1000,1232,3000],1],[[1000,976,3000],1],[[1000,720,3000],1],[[1000,720,3000],0]]],[[0,512,0],1,-1536,7,100,[[[1000,1900,3000],1],[[1000,1800,3000],1],[[1000,1700,3000],1],[[1000,1600,3000],1],[[1000,1500,3000],1],[[1000,1400,3000],1],[[1000,1300,3000],0]]],[[0,512,0],1,4096,4,256,[[[1000,3024,3000],1],[[1000,4048,3000],1],[[1000,5072,3000],1],[[1000,5072,3000],0]]],[[0,512,0],1,4096,4,100,[[[1000,2400,3000],1],[[1000,2800,3000],1],[[1000,3200,3000],1],[[1000,3600,3000],0]]],[[0,512,0],1,4096,7,256,[[[1000,2512,3000],1],[[1000,3024,3000],1],[[1000,3536,3000],1],[[1000,4048,3000],1],[[1000,4560,3000],1],[[1000,5072,3000],1],[[1000,5584,3000],0]]],[[0,512,0],1,4096,7,100,[[[1000,2200,3000],1],[[1000,2400,3000],1],[[1000,2600,3000],1],[[1000,2800,3000],1],[[1000,3000,3000],1],[[1000,3200,3000],1],[[1000,3400,3000],0]]],[[0,512,0],2,1024,4,256,[[[1256,2000,3000],1],[[1512,2000,3000],1],[[1768,2000,3000],1],[[1768,2000,3000],0]]],[[0,512,0],2,1024,4,100,[[[1100,2000,3000],1],[[1200,2000,3000],1],[[1300,2000,3000],1],[[1400,2000,3000],0]]],[[0,512,0],2,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,512,0],2,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,512,0],2,-1536,4,256,[[[488,2000,3000],1],[[-24,2000,3000],1],[[-24,2000,3000],0]]],[[0,512,0],2,-1536,4,100,[[[800,2000,3000],1],[[600,2000,3000],1],[[400,2000,3000],1],[[200,2000,3000],0]]],[[0,512,0],2,-1536,7,256,[[[744,2000,3000],1],[[488,2000,3000],1],[[232,2000,3000],1],[[-24,2000,3000],1],[[-280,2000,3000],1],[[-280,2000,3000],0]]],[[0,512,0],2,-1536,7,100,[[[900,2000,3000],1],[[800,2000,3000],1],[[700,2000,3000],1],[[600,2000,3000],1],[[500,2000,3000],1],[[400,2000,3000],1],[[300,2000,3000],0]]],[[0,512,0],2,4096,4,256,[[[2024,2000,3000],1],[[3048,2000,3000],1],[[4072,2000,3000],1],[[4072,2000,3000],0]]],[[0,512,0],2,4096,4,100,[[[1400,2000,3000],1],[[1800,2000,3000],1],[[2200,2000,3000],1],[[2600,2000,3000],0]]],[[0,512,0],2,4096,7,256,[[[1512,2000,3000],1],[[2024,2000,3000],1],[[2536,2000,3000],1],[[3048,2000,3000],1],[[3560,2000,3000],1],[[4072,2000,3000],1],[[4584,2000,3000],0]]],[[0,512,0],2,4096,7,100,[[[1200,2000,3000],1],[[1400,2000,3000],1],[[1600,2000,3000],1],[[1800,2000,3000],1],[[2000,2000,3000],1],[[2200,2000,3000],1],[[2400,2000,3000],0]]],[[0,300,0],0,1024,4,256,[[[1155,2000,2796],1],[[1310,2000,2592],1],[[1465,2000,2388],1],[[1465,2000,2388],0]]],[[0,300,0],0,1024,4,100,[[[1061,2000,2920],1],[[1122,2000,2840],1],[[1183,2000,2760],1],[[1244,2000,2680],0]]],[[0,300,0],0,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,300,0],0,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,300,0],0,-1536,4,256,[[[690,2000,3407],1],[[380,2000,3814],1],[[380,2000,3814],0]]],[[0,300,0],0,-1536,4,100,[[[879,2000,3159],1],[[758,2000,3318],1],[[637,2000,3477],1],[[516,2000,3636],0]]],[[0,300,0],0,-1536,7,256,[[[845,2000,3204],1],[[690,2000,3408],1],[[535,2000,3612],1],[[380,2000,3816],1],[[225,2000,4020],1],[[225,2000,4020],0]]],[[0,300,0],0,-1536,7,100,[[[939,2000,3080],1],[[878,2000,3160],1],[[817,2000,3240],1],[[756,2000,3320],1],[[695,2000,3400],1],[[634,2000,3480],1],[[573,2000,3560],0]]],[[0,300,0],0,4096,4,256,[[[1620,2000,2185],1],[[2240,2000,1370],1],[[2860,2000,555],1],[[2860,2000,555],0]]],[[0,300,0],0,4096,4,100,[[[1242,2000,2682],1],[[1484,2000,2364],1],[[1726,2000,2046],1],[[1968,2000,1728],0]]],[[0,300,0],0,4096,7,256,[[[1310,2000,2593],1],[[1620,2000,2186],1],[[1930,2000,1779],1],[[2240,2000,1372],1],[[2550,2000,965],1],[[2860,2000,558],1],[[3170,2000,151],0]]],[[0,300,0],0,4096,7,100,[[[1121,2000,2841],1],[[1242,2000,2682],1],[[1363,2000,2523],1],[[1484,2000,2364],1],[[1605,2000,2205],1],[[1726,2000,2046],1],[[1847,2000,1887],0]]],[[0,300,0],1,1024,4,256,[[[1000,2256,3000],1],[[1000,2512,3000],1],[[1000,2768,3000],1],[[1000,2768,3000],0]]],[[0,300,0],1,1024,4,100,[[[1000,2100,3000],1],[[1000,2200,3000],1],[[1000,2300,3000],1],[[1000,2400,3000],0]]],[[0,300,0],1,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,300,0],1,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,300,0],1,-1536,4,256,[[[1000,1488,3000],1],[[1000,976,3000],1],[[1000,976,3000],0]]],[[0,300,0],1,-1536,4,100,[[[1000,1800,3000],1],[[1000,1600,3000],1],[[1000,1400,3000],1],[[1000,1200,3000],0]]],[[0,300,0],1,-1536,7,256,[[[1000,1744,3000],1],[[1000,1488,3000],1],[[1000,1232,3000],1],[[1000,976,3000],1],[[1000,720,3000],1],[[1000,720,3000],0]]],[[0,300,0],1,-1536,7,100,[[[1000,1900,3000],1],[[1000,1800,3000],1],[[1000,1700,3000],1],[[1000,1600,3000],1],[[1000,1500,3000],1],[[1000,1400,3000],1],[[1000,1300,3000],0]]],[[0,300,0],1,4096,4,256,[[[1000,3024,3000],1],[[1000,4048,3000],1],[[1000,5072,3000],1],[[1000,5072,3000],0]]],[[0,300,0],1,4096,4,100,[[[1000,2400,3000],1],[[1000,2800,3000],1],[[1000,3200,3000],1],[[1000,3600,3000],0]]],[[0,300,0],1,4096,7,256,[[[1000,2512,3000],1],[[1000,3024,3000],1],[[1000,3536,3000],1],[[1000,4048,3000],1],[[1000,4560,3000],1],[[1000,5072,3000],1],[[1000,5584,3000],0]]],[[0,300,0],1,4096,7,100,[[[1000,2200,3000],1],[[1000,2400,3000],1],[[1000,2600,3000],1],[[1000,2800,3000],1],[[1000,3000,3000],1],[[1000,3200,3000],1],[[1000,3400,3000],0]]],[[0,300,0],2,1024,4,256,[[[1204,2000,3155],1],[[1408,2000,3310],1],[[1612,2000,3465],1],[[1612,2000,3465],0]]],[[0,300,0],2,1024,4,100,[[[1080,2000,3061],1],[[1160,2000,3122],1],[[1240,2000,3183],1],[[1320,2000,3244],0]]],[[0,300,0],2,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,300,0],2,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[0,300,0],2,-1536,4,256,[[[593,2000,2690],1],[[186,2000,2380],1],[[186,2000,2380],0]]],[[0,300,0],2,-1536,4,100,[[[841,2000,2879],1],[[682,2000,2758],1],[[523,2000,2637],1],[[364,2000,2516],0]]],[[0,300,0],2,-1536,7,256,[[[796,2000,2845],1],[[592,2000,2690],1],[[388,2000,2535],1],[[184,2000,2380],1],[[-20,2000,2225],1],[[-20,2000,2225],0]]],[[0,300,0],2,-1536,7,100,[[[920,2000,2939],1],[[840,2000,2878],1],[[760,2000,2817],1],[[680,2000,2756],1],[[600,2000,2695],1],[[520,2000,2634],1],[[440,2000,2573],0]]],[[0,300,0],2,4096,4,256,[[[1815,2000,3620],1],[[2630,2000,4240],1],[[3445,2000,4860],1],[[3445,2000,4860],0]]],[[0,300,0],2,4096,4,100,[[[1318,2000,3242],1],[[1636,2000,3484],1],[[1954,2000,3726],1],[[2272,2000,3968],0]]],[[0,300,0],2,4096,7,256,[[[1407,2000,3310],1],[[1814,2000,3620],1],[[2221,2000,3930],1],[[2628,2000,4240],1],[[3035,2000,4550],1],[[3442,2000,4860],1],[[3849,2000,5170],0]]],[[0,300,0],2,4096,7,100,[[[1159,2000,3121],1],[[1318,2000,3242],1],[[1477,2000,3363],1],[[1636,2000,3484],1],[[1795,2000,3605],1],[[1954,2000,3726],1],[[2113,2000,3847],0]]],[[64,700,20],0,1024,4,256,[[[858,2033,2790],1],[[716,2066,2580],1],[[574,2099,2370],1],[[574,2099,2370],0]]],[[64,700,20],0,1024,4,100,[[[945,2013,2918],1],[[890,2026,2836],1],[[835,2039,2754],1],[[780,2052,2672],0]]],[[64,700,20],0,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[64,700,20],0,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[64,700,20],0,-1536,4,256,[[[1284,1934,3421],1],[[1568,1868,3842],1],[[1568,1868,3842],0]]],[[64,700,20],0,-1536,4,100,[[[1111,1974,3164],1],[[1222,1948,3328],1],[[1333,1922,3492],1],[[1444,1896,3656],0]]],[[64,700,20],0,-1536,7,256,[[[1142,1967,3210],1],[[1284,1934,3420],1],[[1426,1901,3630],1],[[1568,1868,3840],1],[[1710,1835,4050],1],[[1710,1835,4050],0]]],[[64,700,20],0,-1536,7,100,[[[1055,1987,3082],1],[[1110,1974,3164],1],[[1165,1961,3246],1],[[1220,1948,3328],1],[[1275,1935,3410],1],[[1330,1922,3492],1],[[1385,1909,3574],0]]],[[64,700,20],0,4096,4,256,[[[432,2133,2158],1],[[-136,2266,1316],1],[[-704,2399,474],1],[[-704,2399,474],0]]],[[64,700,20],0,4096,4,100,[[[778,2052,2671],1],[[556,2104,2342],1],[[334,2156,2013],1],[[112,2208,1684],0]]],[[64,700,20],0,4096,7,256,[[[716,2066,2579],1],[[432,2132,2158],1],[[148,2198,1737],1],[[-136,2264,1316],1],[[-420,2330,895],1],[[-704,2396,474],1],[[-988,2462,53],0]]],[[64,700,20],0,4096,7,100,[[[889,2026,2836],1],[[778,2052,2672],1],[[667,2078,2508],1],[[556,2104,2344],1],[[445,2130,2180],1],[[334,2156,2016],1],[[223,2182,1852],0]]],[[64,700,20],1,1024,4,256,[[[985,2251,3050],1],[[970,2502,3100],1],[[955,2753,3150],1],[[955,2753,3150],0]]],[[64,700,20],1,1024,4,100,[[[994,2098,3020],1],[[988,2196,3040],1],[[982,2294,3060],1],[[976,2392,3080],0]]],[[64,700,20],1,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[64,700,20],1,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[64,700,20],1,-1536,4,256,[[[1031,1499,2900],1],[[1062,998,2800],1],[[1062,998,2800],0]]],[[64,700,20],1,-1536,4,100,[[[1012,1804,2961],1],[[1024,1608,2922],1],[[1036,1412,2883],1],[[1048,1216,2844],0]]],[[64,700,20],1,-1536,7,256,[[[1015,1749,2950],1],[[1030,1498,2900],1],[[1045,1247,2850],1],[[1060,996,2800],1],[[1075,745,2750],1],[[1075,745,2750],0]]],[[64,700,20],1,-1536,7,100,[[[1006,1902,2980],1],[[1012,1804,2960],1],[[1018,1706,2940],1],[[1024,1608,2920],1],[[1030,1510,2900],1],[[1036,1412,2880],1],[[1042,1314,2860],0]]],[[64,700,20],1,4096,4,256,[[[938,3002,3200],1],[[876,4004,3400],1],[[814,5006,3600],1],[[814,5006,3600],0]]],[[64,700,20],1,4096,4,100,[[[976,2392,3078],1],[[952,2784,3156],1],[[928,3176,3234],1],[[904,3568,3312],0]]],[[64,700,20],1,4096,7,256,[[[969,2501,3100],1],[[938,3002,3200],1],[[907,3503,3300],1],[[876,4004,3400],1],[[845,4505,3500],1],[[814,5006,3600],1],[[783,5507,3700],0]]],[[64,700,20],1,4096,7,100,[[[988,2196,3039],1],[[976,2392,3078],1],[[964,2588,3117],1],[[952,2784,3156],1],[[940,2980,3195],1],[[928,3176,3234],1],[[916,3372,3273],0]]],[[64,700,20],2,1024,4,256,[[[1213,2040,2863],1],[[1426,2080,2726],1],[[1639,2120,2589],1],[[1639,2120,2589],0]]],[[64,700,20],2,1024,4,100,[[[1083,2016,2947],1],[[1166,2032,2894],1],[[1249,2048,2841],1],[[1332,2064,2788],0]]],[[64,700,20],2,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[64,700,20],2,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[64,700,20],2,-1536,4,256,[[[575,1919,3274],1],[[150,1838,3548],1],[[150,1838,3548],0]]],[[64,700,20],2,-1536,4,100,[[[834,1968,3107],1],[[668,1936,3214],1],[[502,1904,3321],1],[[336,1872,3428],0]]],[[64,700,20],2,-1536,7,256,[[[787,1960,3137],1],[[574,1920,3274],1],[[361,1880,3411],1],[[148,1840,3548],1],[[-65,1800,3685],1],[[-65,1800,3685],0]]],[[64,700,20],2,-1536,7,100,[[[917,1984,3053],1],[[834,1968,3106],1],[[751,1952,3159],1],[[668,1936,3212],1],[[585,1920,3265],1],[[502,1904,3318],1],[[419,1888,3371],0]]],[[64,700,20],2,4096,4,256,[[[1850,2161,2452],1],[[2700,2322,1904],1],[[3550,2483,1356],1],[[3550,2483,1356],0]]],[[64,700,20],2,4096,4,100,[[[1332,2063,2786],1],[[1664,2126,2572],1],[[1996,2189,2358],1],[[2328,2252,2144],0]]],[[64,700,20],2,4096,7,256,[[[1425,2081,2726],1],[[1850,2162,2452],1],[[2275,2243,2178],1],[[2700,2324,1904],1],[[3125,2405,1630],1],[[3550,2486,1356],1],[[3975,2567,1082],0]]],[[64,700,20],2,4096,7,100,[[[1166,2032,2893],1],[[1332,2064,2786],1],[[1498,2096,2679],1],[[1664,2128,2572],1],[[1830,2160,2465],1],[[1996,2192,2358],1],[[2162,2224,2251],0]]],[[2000,1500,100],0,1024,4,256,[[[962,2027,3252],1],[[924,2054,3504],1],[[886,2081,3756],1],[[886,2081,3756],0]]],[[2000,1500,100],0,1024,4,100,[[[985,2011,3098],1],[[970,2022,3196],1],[[955,2033,3294],1],[[940,2044,3392],0]]],[[2000,1500,100],0,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[2000,1500,100],0,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[2000,1500,100],0,-1536,4,256,[[[1076,1946,2497],1],[[1152,1892,1994],1],[[1152,1892,1994],0]]],[[2000,1500,100],0,-1536,4,100,[[[1030,1979,2803],1],[[1060,1958,2606],1],[[1090,1937,2409],1],[[1120,1916,2212],0]]],[[2000,1500,100],0,-1536,7,256,[[[1038,1973,2748],1],[[1076,1946,2496],1],[[1114,1919,2244],1],[[1152,1892,1992],1],[[1190,1865,1740],1],[[1190,1865,1740],0]]],[[2000,1500,100],0,-1536,7,100,[[[1015,1989,2902],1],[[1030,1978,2804],1],[[1045,1967,2706],1],[[1060,1956,2608],1],[[1075,1945,2510],1],[[1090,1934,2412],1],[[1105,1923,2314],0]]],[[2000,1500,100],0,4096,4,256,[[[847,2108,4007],1],[[694,2216,5014],1],[[541,2324,6021],1],[[541,2324,6021],0]]],[[2000,1500,100],0,4096,4,100,[[[940,2042,3393],1],[[880,2084,3786],1],[[820,2126,4179],1],[[760,2168,4572],0]]],[[2000,1500,100],0,4096,7,256,[[[924,2054,3503],1],[[848,2108,4006],1],[[772,2162,4509],1],[[696,2216,5012],1],[[620,2270,5515],1],[[544,2324,6018],1],[[468,2378,6521],0]]],[[2000,1500,100],0,4096,7,100,[[[970,2021,3197],1],[[940,2042,3394],1],[[910,2063,3591],1],[[880,2084,3788],1],[[850,2105,3985],1],[[820,2126,4182],1],[[790,2147,4379],0]]],[[2000,1500,100],1,1024,4,256,[[[924,2241,2962],1],[[848,2482,2924],1],[[772,2723,2886],1],[[772,2723,2886],0]]],[[2000,1500,100],1,1024,4,100,[[[970,2094,2985],1],[[940,2188,2970],1],[[910,2282,2955],1],[[880,2376,2940],0]]],[[2000,1500,100],1,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[2000,1500,100],1,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[2000,1500,100],1,-1536,4,256,[[[1153,1517,3075],1],[[1306,1034,3150],1],[[1306,1034,3150],0]]],[[2000,1500,100],1,-1536,4,100,[[[1060,1811,3029],1],[[1120,1622,3058],1],[[1180,1433,3087],1],[[1240,1244,3116],0]]],[[2000,1500,100],1,-1536,7,256,[[[1076,1759,3038],1],[[1152,1518,3076],1],[[1228,1277,3114],1],[[1304,1036,3152],1],[[1380,795,3190],1],[[1380,795,3190],0]]],[[2000,1500,100],1,-1536,7,100,[[[1030,1906,3015],1],[[1060,1812,3030],1],[[1090,1718,3045],1],[[1120,1624,3060],1],[[1150,1530,3075],1],[[1180,1436,3090],1],[[1210,1342,3105],0]]],[[2000,1500,100],1,4096,4,256,[[[694,2966,2850],1],[[388,3932,2700],1],[[82,4898,2550],1],[[82,4898,2550],0]]],[[2000,1500,100],1,4096,4,100,[[[881,2377,2941],1],[[762,2754,2882],1],[[643,3131,2823],1],[[524,3508,2764],0]]],[[2000,1500,100],1,4096,7,256,[[[847,2483,2925],1],[[694,2966,2850],1],[[541,3449,2775],1],[[388,3932,2700],1],[[235,4415,2625],1],[[82,4898,2550],1],[[-71,5381,2475],0]]],[[2000,1500,100],1,4096,7,100,[[[940,2189,2971],1],[[880,2378,2942],1],[[820,2567,2913],1],[[760,2756,2884],1],[[700,2945,2855],1],[[640,3134,2826],1],[[580,3323,2797],0]]],[[2000,1500,100],2,1024,4,256,[[[759,1919,2972],1],[[518,1838,2944],1],[[277,1757,2916],1],[[277,1757,2916],0]]],[[2000,1500,100],2,1024,4,100,[[[906,1968,2989],1],[[812,1936,2978],1],[[718,1904,2967],1],[[624,1872,2956],0]]],[[2000,1500,100],2,1024,7,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[2000,1500,100],2,1024,7,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],[[2000,1500,100],2,-1536,4,256,[[[1483,2162,3056],1],[[1966,2324,3112],1],[[1966,2324,3112],0]]],[[2000,1500,100],2,-1536,4,100,[[[1189,2063,3022],1],[[1378,2126,3044],1],[[1567,2189,3066],1],[[1756,2252,3088],0]]],[[2000,1500,100],2,-1536,7,256,[[[1241,2081,3028],1],[[1482,2162,3056],1],[[1723,2243,3084],1],[[1964,2324,3112],1],[[2205,2405,3140],1],[[2205,2405,3140],0]]],[[2000,1500,100],2,-1536,7,100,[[[1094,2032,3011],1],[[1188,2064,3022],1],[[1282,2096,3033],1],[[1376,2128,3044],1],[[1470,2160,3055],1],[[1564,2192,3066],1],[[1658,2224,3077],0]]],[[2000,1500,100],2,4096,4,256,[[[35,1677,2888],1],[[-930,1354,2776],1],[[-1895,1031,2664],1],[[-1895,1031,2664],0]]],[[2000,1500,100],2,4096,4,100,[[[623,1874,2956],1],[[246,1748,2912],1],[[-131,1622,2868],1],[[-508,1496,2824],0]]],[[2000,1500,100],2,4096,7,256,[[[517,1838,2944],1],[[34,1676,2888],1],[[-449,1514,2832],1],[[-932,1352,2776],1],[[-1415,1190,2720],1],[[-1898,1028,2664],1],[[-2381,866,2608],0]]],[[2000,1500,100],2,4096,7,100,[[[811,1937,2978],1],[[622,1874,2956],1],[[433,1811,2934],1],[[244,1748,2912],1],[[55,1685,2890],1],[[-134,1622,2868],1],[[-323,1559,2846],0]]]]"#).unwrap();
    for row in rows {
        let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
        let angles: [u16; 3] = std::array::from_fn(|i| row[0][i].as_u64().unwrap() as u16);
        let (axis, delta, duration) = (int(&row[1]), int(&row[2]), int(&row[3]));
        let scale = row[4].as_u64().unwrap() as u32;
        let args = [
            Argument::Integer(axis),
            Argument::Integer(delta),
            Argument::Integer(duration),
        ];
        let mut tasks = NativeTasks::default();
        let mut position = [1000, 2000, 3000];
        let mut angles_state = angles;
        let mut updates = vec![];
        for (elapsed, step) in row[5].as_array().unwrap().iter().enumerate() {
            let invocation = Invocation {
                elapsed: Some(elapsed as i32),
                unpaused_tasks: 0,
                unpaused_blocking_tasks: 0,
            };
            let mut spatial = Some(SpatialCall {
                position: &mut position,
                angles: &mut angles_state,
                frame_scale: scale,
                turn: 0,
                nominal_ticks: 1,
                matrix: Matrix::orientation(angles),
                slave: false,
                locations: &[],
                effective_position: None,
                player_position: None,
                actor_index: 3,
                map_updates: &mut updates,
            });
            let completed = if elapsed == 0 {
                assert_eq!(
                    tasks
                        .call(
                            ("MoveAlongAxis", CallKind::Task),
                            &args,
                            invocation,
                            &mut None,
                            &mut spatial,
                            NativeServices::default(),
                        )
                        .unwrap(),
                    Some(CallResult::Pending(1))
                );
                false
            } else {
                tasks
                    .poll(
                        1,
                        &args,
                        invocation,
                        &mut None,
                        &mut spatial,
                        NativeServices::default(),
                    )
                    .unwrap()
            };
            let expected: [i32; 3] = std::array::from_fn(|i| int(&step[0][i]));
            assert_eq!(
                (position, completed),
                (expected, step[1].as_u64().unwrap() == 0 && elapsed > 0),
                "angles {angles:?} axis {axis} delta {delta} duration {duration} scale {scale} elapsed {elapsed}"
            );
        }
    }
}

#[test]
fn control_animation_turns_the_receiver_only_while_its_group_plays() {
    use crate::script::tasks::AnimationCall;
    use cyrus_assets::animation::Animations;
    let data = Animations {
        version: 2,
        world: 0,
        metadata: json!({"actors":[{
        "script_name":"ACTOR", "animation_groups":[
            {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]},
            {"group_index":7,"anim_id":1,"anim_type":1,"commands":[{"opcode":0,"frame_reference":70},{"opcode":0,"frame_reference":71},{"opcode":1}]}
        ]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut animation = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut frame = None;
    let mut tasks = NativeTasks::default();
    let args = [Argument::Integer(7), Argument::Integer(2)];
    let mut position = [0; 3];
    let mut angles = [0, 100, 0];
    let mut updates = vec![];
    let mut step = |tasks: &mut NativeTasks,
                    animation: &mut ActorAnimation,
                    angles: &mut [u16; 3],
                    token: Option<u64>| {
        let invocation = Invocation {
            elapsed: Some(0),
            unpaused_tasks: 0,
            unpaused_blocking_tasks: 0,
        };
        let mut input = Some(AnimationCall {
            animation,
            ticks: 1,
            frame: &mut frame,
            sounds: None,
        });
        let mut spatial = Some(SpatialCall {
            position: &mut position,
            angles,
            frame_scale: 256,
            turn: -12,
            nominal_ticks: 1,
            matrix: Matrix::IDENTITY,
            slave: false,
            locations: &[],
            effective_position: None,
            player_position: None,
            actor_index: 0,
            map_updates: &mut updates,
        });
        match token {
            None => {
                assert_eq!(
                    tasks
                        .call(
                            ("PushControlAnimation", CallKind::Task),
                            &args,
                            invocation,
                            &mut input,
                            &mut spatial,
                            NativeServices::default(),
                        )
                        .unwrap(),
                    Some(CallResult::Pending(1))
                );
                false
            }
            Some(token) => tasks
                .poll(
                    token,
                    &args,
                    invocation,
                    &mut input,
                    &mut spatial,
                    NativeServices::default(),
                )
                .unwrap(),
        }
    };
    // Requesting and starting do not turn; the started task then adds the held turn each pass.
    step(&mut tasks, &mut animation, &mut angles, None);
    assert!(!step(&mut tasks, &mut animation, &mut angles, Some(1)));
    assert_eq!(angles[1], 100);
    assert!(!step(&mut tasks, &mut animation, &mut angles, Some(1)));
    assert_eq!(angles[1], 112);
    assert!(!step(&mut tasks, &mut animation, &mut angles, Some(1)));
    assert_eq!(angles[1], 124);
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
                        turn: 0,
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
fn walk_backward_matches_the_original_routine() {
    use crate::script::{
        locomotion::BackwardState,
        tasks::{AnimationCall, LocomotionAdmission, LocomotionCall, MotionCall},
    };
    use cyrus_assets::animation::Animations;
    use cyrus_world::animation::ActorAnimation;
    // Rows from the original routine run in an emulator with duration 1: heading, step counter,
    // attribute 11, attribute 13, attribute 9, frame scale, resulting position, packed counter byte.
    let rows: Vec<[i64; 10]> = serde_json::from_str(
        r#"[
[0,0,0,8,3,256,0,0,-2048,64],
[0,0,0,8,3,100,0,0,-800,64],
[0,0,0,24,3,256,0,0,-6144,64],
[0,0,0,24,3,100,0,0,-2400,64],
[0,0,0,0,6,256,0,0,-3,64],
[0,0,0,0,6,100,0,0,-1,64],
[0,0,0,0,0,256,0,0,0,64],
[0,0,0,0,0,100,0,0,0,64],
[0,0,1,8,3,256,0,0,-512,64],
[0,0,1,8,3,100,0,0,-200,64],
[0,0,1,24,3,256,0,0,-1536,64],
[0,0,1,24,3,100,0,0,-600,64],
[0,0,1,0,6,256,0,0,0,64],
[0,0,1,0,6,100,0,0,0,64],
[0,0,1,0,0,256,0,0,0,64],
[0,0,1,0,0,100,0,0,0,64],
[0,0,2,8,3,256,0,0,0,64],
[0,0,2,8,3,100,0,0,0,64],
[0,0,2,24,3,256,0,0,0,64],
[0,0,2,24,3,100,0,0,0,64],
[0,0,2,0,6,256,0,0,0,64],
[0,0,2,0,6,100,0,0,0,64],
[0,0,2,0,0,256,0,0,0,64],
[0,0,2,0,0,100,0,0,0,64],
[0,0,3,8,3,256,0,0,0,64],
[0,0,3,8,3,100,0,0,0,64],
[0,0,3,24,3,256,0,0,0,64],
[0,0,3,24,3,100,0,0,0,64],
[0,0,3,0,6,256,0,0,0,64],
[0,0,3,0,6,100,0,0,0,64],
[0,0,3,0,0,256,0,0,0,64],
[0,0,3,0,0,100,0,0,0,64],
[0,1,0,8,3,256,0,0,-2048,128],
[0,1,0,8,3,100,0,0,-800,128],
[0,1,0,24,3,256,0,0,-6144,128],
[0,1,0,24,3,100,0,0,-2400,128],
[0,1,0,0,6,256,0,0,-3,128],
[0,1,0,0,6,100,0,0,-1,128],
[0,1,0,0,0,256,0,0,0,128],
[0,1,0,0,0,100,0,0,0,128],
[0,1,1,8,3,256,0,0,-682,128],
[0,1,1,8,3,100,0,0,-266,128],
[0,1,1,24,3,256,0,0,-2048,128],
[0,1,1,24,3,100,0,0,-800,128],
[0,1,1,0,6,256,0,0,-1,128],
[0,1,1,0,6,100,0,0,0,128],
[0,1,1,0,0,256,0,0,0,128],
[0,1,1,0,0,100,0,0,0,128],
[0,1,2,8,3,256,0,0,-680,128],
[0,1,2,8,3,100,0,0,-265,128],
[0,1,2,24,3,256,0,0,-2040,128],
[0,1,2,24,3,100,0,0,-796,128],
[0,1,2,0,6,256,0,0,0,128],
[0,1,2,0,6,100,0,0,0,128],
[0,1,2,0,0,256,0,0,0,128],
[0,1,2,0,0,100,0,0,0,128],
[0,1,3,8,3,256,0,0,0,128],
[0,1,3,8,3,100,0,0,0,128],
[0,1,3,24,3,256,0,0,0,128],
[0,1,3,24,3,100,0,0,0,128],
[0,1,3,0,6,256,0,0,0,128],
[0,1,3,0,6,100,0,0,0,128],
[0,1,3,0,0,256,0,0,0,128],
[0,1,3,0,0,100,0,0,0,128],
[0,2,0,8,3,256,0,0,-2048,192],
[0,2,0,8,3,100,0,0,-800,192],
[0,2,0,24,3,256,0,0,-6144,192],
[0,2,0,24,3,100,0,0,-2400,192],
[0,2,0,0,6,256,0,0,-3,192],
[0,2,0,0,6,100,0,0,-1,192],
[0,2,0,0,0,256,0,0,0,192],
[0,2,0,0,0,100,0,0,0,192],
[0,2,1,8,3,256,0,0,-1024,192],
[0,2,1,8,3,100,0,0,-400,192],
[0,2,1,24,3,256,0,0,-3072,192],
[0,2,1,24,3,100,0,0,-1200,192],
[0,2,1,0,6,256,0,0,-1,192],
[0,2,1,0,6,100,0,0,0,192],
[0,2,1,0,0,256,0,0,0,192],
[0,2,1,0,0,100,0,0,0,192],
[0,2,2,8,3,256,0,0,-1360,192],
[0,2,2,8,3,100,0,0,-531,192],
[0,2,2,24,3,256,0,0,-4080,192],
[0,2,2,24,3,100,0,0,-1593,192],
[0,2,2,0,6,256,0,0,-1,192],
[0,2,2,0,6,100,0,0,0,192],
[0,2,2,0,0,256,0,0,0,192],
[0,2,2,0,0,100,0,0,0,192],
[0,2,3,8,3,256,0,0,-1024,192],
[0,2,3,8,3,100,0,0,-400,192],
[0,2,3,24,3,256,0,0,-3072,192],
[0,2,3,24,3,100,0,0,-1200,192],
[0,2,3,0,6,256,0,0,-1,192],
[0,2,3,0,6,100,0,0,0,192],
[0,2,3,0,0,256,0,0,0,192],
[0,2,3,0,0,100,0,0,0,192],
[0,3,0,8,3,256,0,0,-2048,192],
[0,3,0,8,3,100,0,0,-800,192],
[0,3,0,24,3,256,0,0,-6144,192],
[0,3,0,24,3,100,0,0,-2400,192],
[0,3,0,0,6,256,0,0,-3,192],
[0,3,0,0,6,100,0,0,-1,192],
[0,3,0,0,0,256,0,0,0,192],
[0,3,0,0,0,100,0,0,0,192],
[0,3,1,8,3,256,0,0,-2048,192],
[0,3,1,8,3,100,0,0,-800,192],
[0,3,1,24,3,256,0,0,-6144,192],
[0,3,1,24,3,100,0,0,-2400,192],
[0,3,1,0,6,256,0,0,-3,192],
[0,3,1,0,6,100,0,0,-1,192],
[0,3,1,0,0,256,0,0,0,192],
[0,3,1,0,0,100,0,0,0,192],
[0,3,2,8,3,256,0,0,-2048,192],
[0,3,2,8,3,100,0,0,-800,192],
[0,3,2,24,3,256,0,0,-6144,192],
[0,3,2,24,3,100,0,0,-2400,192],
[0,3,2,0,6,256,0,0,-3,192],
[0,3,2,0,6,100,0,0,-1,192],
[0,3,2,0,0,256,0,0,0,192],
[0,3,2,0,0,100,0,0,0,192],
[0,3,3,8,3,256,0,0,-2048,192],
[0,3,3,8,3,100,0,0,-800,192],
[0,3,3,24,3,256,0,0,-6144,192],
[0,3,3,24,3,100,0,0,-2400,192],
[0,3,3,0,6,256,0,0,-3,192],
[0,3,3,0,6,100,0,0,-1,192],
[0,3,3,0,0,256,0,0,0,192],
[0,3,3,0,0,100,0,0,0,192],
[700,0,0,8,3,256,1716,0,1116,64],
[700,0,0,8,3,100,670,0,436,64],
[700,0,0,24,3,256,5150,0,3350,64],
[700,0,0,24,3,100,2011,0,1308,64],
[700,0,0,0,6,256,2,0,1,64],
[700,0,0,0,6,100,0,0,0,64],
[700,0,0,0,0,256,0,0,0,64],
[700,0,0,0,0,100,0,0,0,64],
[700,0,1,8,3,256,429,0,279,64],
[700,0,1,8,3,100,167,0,109,64],
[700,0,1,24,3,256,1287,0,837,64],
[700,0,1,24,3,100,502,0,327,64],
[700,0,1,0,6,256,0,0,0,64],
[700,0,1,0,6,100,0,0,0,64],
[700,0,1,0,0,256,0,0,0,64],
[700,0,1,0,0,100,0,0,0,64],
[700,0,2,8,3,256,0,0,0,64],
[700,0,2,8,3,100,0,0,0,64],
[700,0,2,24,3,256,0,0,0,64],
[700,0,2,24,3,100,0,0,0,64],
[700,0,2,0,6,256,0,0,0,64],
[700,0,2,0,6,100,0,0,0,64],
[700,0,2,0,0,256,0,0,0,64],
[700,0,2,0,0,100,0,0,0,64],
[700,0,3,8,3,256,0,0,0,64],
[700,0,3,8,3,100,0,0,0,64],
[700,0,3,24,3,256,0,0,0,64],
[700,0,3,24,3,100,0,0,0,64],
[700,0,3,0,6,256,0,0,0,64],
[700,0,3,0,6,100,0,0,0,64],
[700,0,3,0,0,256,0,0,0,64],
[700,0,3,0,0,100,0,0,0,64],
[700,1,0,8,3,256,1716,0,1116,128],
[700,1,0,8,3,100,670,0,436,128],
[700,1,0,24,3,256,5150,0,3350,128],
[700,1,0,24,3,100,2011,0,1308,128],
[700,1,0,0,6,256,2,0,1,128],
[700,1,0,0,6,100,0,0,0,128],
[700,1,0,0,0,256,0,0,0,128],
[700,1,0,0,0,100,0,0,0,128],
[700,1,1,8,3,256,571,0,371,128],
[700,1,1,8,3,100,222,0,145,128],
[700,1,1,24,3,256,1716,0,1116,128],
[700,1,1,24,3,100,670,0,436,128],
[700,1,1,0,6,256,0,0,0,128],
[700,1,1,0,6,100,0,0,0,128],
[700,1,1,0,0,256,0,0,0,128],
[700,1,1,0,0,100,0,0,0,128],
[700,1,2,8,3,256,569,0,370,128],
[700,1,2,8,3,100,222,0,144,128],
[700,1,2,24,3,256,1709,0,1112,128],
[700,1,2,24,3,100,667,0,434,128],
[700,1,2,0,6,256,0,0,0,128],
[700,1,2,0,6,100,0,0,0,128],
[700,1,2,0,0,256,0,0,0,128],
[700,1,2,0,0,100,0,0,0,128],
[700,1,3,8,3,256,0,0,0,128],
[700,1,3,8,3,100,0,0,0,128],
[700,1,3,24,3,256,0,0,0,128],
[700,1,3,24,3,100,0,0,0,128],
[700,1,3,0,6,256,0,0,0,128],
[700,1,3,0,6,100,0,0,0,128],
[700,1,3,0,0,256,0,0,0,128],
[700,1,3,0,0,100,0,0,0,128],
[700,2,0,8,3,256,1716,0,1116,192],
[700,2,0,8,3,100,670,0,436,192],
[700,2,0,24,3,256,5150,0,3350,192],
[700,2,0,24,3,100,2011,0,1308,192],
[700,2,0,0,6,256,2,0,1,192],
[700,2,0,0,6,100,0,0,0,192],
[700,2,0,0,0,256,0,0,0,192],
[700,2,0,0,0,100,0,0,0,192],
[700,2,1,8,3,256,858,0,558,192],
[700,2,1,8,3,100,335,0,218,192],
[700,2,1,24,3,256,2575,0,1675,192],
[700,2,1,24,3,100,1005,0,654,192],
[700,2,1,0,6,256,0,0,0,192],
[700,2,1,0,6,100,0,0,0,192],
[700,2,1,0,0,256,0,0,0,192],
[700,2,1,0,0,100,0,0,0,192],
[700,2,2,8,3,256,1139,0,741,192],
[700,2,2,8,3,100,445,0,289,192],
[700,2,2,24,3,256,3419,0,2224,192],
[700,2,2,24,3,100,1335,0,868,192],
[700,2,2,0,6,256,0,0,0,192],
[700,2,2,0,6,100,0,0,0,192],
[700,2,2,0,0,256,0,0,0,192],
[700,2,2,0,0,100,0,0,0,192],
[700,2,3,8,3,256,858,0,558,192],
[700,2,3,8,3,100,335,0,218,192],
[700,2,3,24,3,256,2575,0,1675,192],
[700,2,3,24,3,100,1005,0,654,192],
[700,2,3,0,6,256,0,0,0,192],
[700,2,3,0,6,100,0,0,0,192],
[700,2,3,0,0,256,0,0,0,192],
[700,2,3,0,0,100,0,0,0,192],
[700,3,0,8,3,256,1716,0,1116,192],
[700,3,0,8,3,100,670,0,436,192],
[700,3,0,24,3,256,5150,0,3350,192],
[700,3,0,24,3,100,2011,0,1308,192],
[700,3,0,0,6,256,2,0,1,192],
[700,3,0,0,6,100,0,0,0,192],
[700,3,0,0,0,256,0,0,0,192],
[700,3,0,0,0,100,0,0,0,192],
[700,3,1,8,3,256,1716,0,1116,192],
[700,3,1,8,3,100,670,0,436,192],
[700,3,1,24,3,256,5150,0,3350,192],
[700,3,1,24,3,100,2011,0,1308,192],
[700,3,1,0,6,256,2,0,1,192],
[700,3,1,0,6,100,0,0,0,192],
[700,3,1,0,0,256,0,0,0,192],
[700,3,1,0,0,100,0,0,0,192],
[700,3,2,8,3,256,1716,0,1116,192],
[700,3,2,8,3,100,670,0,436,192],
[700,3,2,24,3,256,5150,0,3350,192],
[700,3,2,24,3,100,2011,0,1308,192],
[700,3,2,0,6,256,2,0,1,192],
[700,3,2,0,6,100,0,0,0,192],
[700,3,2,0,0,256,0,0,0,192],
[700,3,2,0,0,100,0,0,0,192],
[700,3,3,8,3,256,1716,0,1116,192],
[700,3,3,8,3,100,670,0,436,192],
[700,3,3,24,3,256,5150,0,3350,192],
[700,3,3,24,3,100,2011,0,1308,192],
[700,3,3,0,6,256,2,0,1,192],
[700,3,3,0,6,100,0,0,0,192],
[700,3,3,0,0,256,0,0,0,192],
[700,3,3,0,0,100,0,0,0,192],
[1500,0,0,8,3,256,-2035,0,225,64],
[1500,0,0,8,3,100,-795,0,88,64],
[1500,0,0,24,3,256,-6106,0,677,64],
[1500,0,0,24,3,100,-2385,0,264,64],
[1500,0,0,0,6,256,-2,0,0,64],
[1500,0,0,0,6,100,0,0,0,64],
[1500,0,0,0,0,256,0,0,0,64],
[1500,0,0,0,0,100,0,0,0,64],
[1500,0,1,8,3,256,-508,0,56,64],
[1500,0,1,8,3,100,-198,0,22,64],
[1500,0,1,24,3,256,-1526,0,169,64],
[1500,0,1,24,3,100,-596,0,66,64],
[1500,0,1,0,6,256,0,0,0,64],
[1500,0,1,0,6,100,0,0,0,64],
[1500,0,1,0,0,256,0,0,0,64],
[1500,0,1,0,0,100,0,0,0,64],
[1500,0,2,8,3,256,0,0,0,64],
[1500,0,2,8,3,100,0,0,0,64],
[1500,0,2,24,3,256,0,0,0,64],
[1500,0,2,24,3,100,0,0,0,64],
[1500,0,2,0,6,256,0,0,0,64],
[1500,0,2,0,6,100,0,0,0,64],
[1500,0,2,0,0,256,0,0,0,64],
[1500,0,2,0,0,100,0,0,0,64],
[1500,0,3,8,3,256,0,0,0,64],
[1500,0,3,8,3,100,0,0,0,64],
[1500,0,3,24,3,256,0,0,0,64],
[1500,0,3,24,3,100,0,0,0,64],
[1500,0,3,0,6,256,0,0,0,64],
[1500,0,3,0,6,100,0,0,0,64],
[1500,0,3,0,0,256,0,0,0,64],
[1500,0,3,0,0,100,0,0,0,64],
[1500,1,0,8,3,256,-2035,0,225,128],
[1500,1,0,8,3,100,-795,0,88,128],
[1500,1,0,24,3,256,-6106,0,677,128],
[1500,1,0,24,3,100,-2385,0,264,128],
[1500,1,0,0,6,256,-2,0,0,128],
[1500,1,0,0,6,100,0,0,0,128],
[1500,1,0,0,0,256,0,0,0,128],
[1500,1,0,0,0,100,0,0,0,128],
[1500,1,1,8,3,256,-677,0,75,128],
[1500,1,1,8,3,100,-264,0,29,128],
[1500,1,1,24,3,256,-2035,0,225,128],
[1500,1,1,24,3,100,-795,0,88,128],
[1500,1,1,0,6,256,0,0,0,128],
[1500,1,1,0,6,100,0,0,0,128],
[1500,1,1,0,0,256,0,0,0,128],
[1500,1,1,0,0,100,0,0,0,128],
[1500,1,2,8,3,256,-675,0,74,128],
[1500,1,2,8,3,100,-263,0,29,128],
[1500,1,2,24,3,256,-2027,0,224,128],
[1500,1,2,24,3,100,-791,0,87,128],
[1500,1,2,0,6,256,0,0,0,128],
[1500,1,2,0,6,100,0,0,0,128],
[1500,1,2,0,0,256,0,0,0,128],
[1500,1,2,0,0,100,0,0,0,128],
[1500,1,3,8,3,256,0,0,0,128],
[1500,1,3,8,3,100,0,0,0,128],
[1500,1,3,24,3,256,0,0,0,128],
[1500,1,3,24,3,100,0,0,0,128],
[1500,1,3,0,6,256,0,0,0,128],
[1500,1,3,0,6,100,0,0,0,128],
[1500,1,3,0,0,256,0,0,0,128],
[1500,1,3,0,0,100,0,0,0,128],
[1500,2,0,8,3,256,-2035,0,225,192],
[1500,2,0,8,3,100,-795,0,88,192],
[1500,2,0,24,3,256,-6106,0,677,192],
[1500,2,0,24,3,100,-2385,0,264,192],
[1500,2,0,0,6,256,-2,0,0,192],
[1500,2,0,0,6,100,0,0,0,192],
[1500,2,0,0,0,256,0,0,0,192],
[1500,2,0,0,0,100,0,0,0,192],
[1500,2,1,8,3,256,-1017,0,112,192],
[1500,2,1,8,3,100,-397,0,44,192],
[1500,2,1,24,3,256,-3053,0,338,192],
[1500,2,1,24,3,100,-1192,0,132,192],
[1500,2,1,0,6,256,0,0,0,192],
[1500,2,1,0,6,100,0,0,0,192],
[1500,2,1,0,0,256,0,0,0,192],
[1500,2,1,0,0,100,0,0,0,192],
[1500,2,2,8,3,256,-1351,0,149,192],
[1500,2,2,8,3,100,-527,0,58,192],
[1500,2,2,24,3,256,-4055,0,449,192],
[1500,2,2,24,3,100,-1583,0,175,192],
[1500,2,2,0,6,256,0,0,0,192],
[1500,2,2,0,6,100,0,0,0,192],
[1500,2,2,0,0,256,0,0,0,192],
[1500,2,2,0,0,100,0,0,0,192],
[1500,2,3,8,3,256,-1017,0,112,192],
[1500,2,3,8,3,100,-397,0,44,192],
[1500,2,3,24,3,256,-3053,0,338,192],
[1500,2,3,24,3,100,-1192,0,132,192],
[1500,2,3,0,6,256,0,0,0,192],
[1500,2,3,0,6,100,0,0,0,192],
[1500,2,3,0,0,256,0,0,0,192],
[1500,2,3,0,0,100,0,0,0,192],
[1500,3,0,8,3,256,-2035,0,225,192],
[1500,3,0,8,3,100,-795,0,88,192],
[1500,3,0,24,3,256,-6106,0,677,192],
[1500,3,0,24,3,100,-2385,0,264,192],
[1500,3,0,0,6,256,-2,0,0,192],
[1500,3,0,0,6,100,0,0,0,192],
[1500,3,0,0,0,256,0,0,0,192],
[1500,3,0,0,0,100,0,0,0,192],
[1500,3,1,8,3,256,-2035,0,225,192],
[1500,3,1,8,3,100,-795,0,88,192],
[1500,3,1,24,3,256,-6106,0,677,192],
[1500,3,1,24,3,100,-2385,0,264,192],
[1500,3,1,0,6,256,-2,0,0,192],
[1500,3,1,0,6,100,0,0,0,192],
[1500,3,1,0,0,256,0,0,0,192],
[1500,3,1,0,0,100,0,0,0,192],
[1500,3,2,8,3,256,-2035,0,225,192],
[1500,3,2,8,3,100,-795,0,88,192],
[1500,3,2,24,3,256,-6106,0,677,192],
[1500,3,2,24,3,100,-2385,0,264,192],
[1500,3,2,0,6,256,-2,0,0,192],
[1500,3,2,0,6,100,0,0,0,192],
[1500,3,2,0,0,256,0,0,0,192],
[1500,3,2,0,0,100,0,0,0,192],
[1500,3,3,8,3,256,-2035,0,225,192],
[1500,3,3,8,3,100,-795,0,88,192],
[1500,3,3,24,3,256,-6106,0,677,192],
[1500,3,3,24,3,100,-2385,0,264,192],
[1500,3,3,0,6,256,-2,0,0,192],
[1500,3,3,0,6,100,0,0,0,192],
[1500,3,3,0,0,256,0,0,0,192],
[1500,3,3,0,0,100,0,0,0,192]
]"#,
    )
    .unwrap();
    let data = Animations {
        version: 2,
        world: 0,
        metadata: json!({"actors":[{"script_name":"ACTOR", "animation_groups":[
            {"group_index":22,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}
        ]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let run = |heading: i64,
               counter: i64,
               scale: i64,
               state: BackwardState,
               admission: LocomotionAdmission| {
        let mut animation = ActorAnimation::from_asset(&data, "ACTOR", 22).unwrap();
        let mut task = Task::WalkBackward;
        let mut position = [0; 3];
        let mut angles = [0, heading as u16, 0];
        let mut counters = (counter as u8) << 6;
        let mut direction = 0;
        let mut called = 0;
        let mut frame = None;
        let mut updates = vec![];
        let done = task
            .complete(
                &[Argument::Integer(1)],
                Invocation {
                    elapsed: Some(0),
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
                    frame_scale: scale as u32,
                    turn: 0,
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
                        state: Ok(state),
                        admission,
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
        (done, position, counters, direction, called)
    };
    let admitted = || LocomotionAdmission {
        backward_step_clear: Some(true),
        walk_backward_mode: true,
        ..Default::default()
    };
    for [heading, counter, mode, step, attr9, scale, x, y, z, byte] in rows {
        let state = BackwardState {
            step_adjustment: mode as u8,
            walk_backward_step_attribute: step as u8,
            step_attribute: attr9 as u8,
            ..Default::default()
        };
        let (done, position, counters, direction, called) =
            run(heading, counter, scale, state, admitted());
        let label = format!(
            "heading {heading} counter {counter} mode {mode} step {step} attr9 {attr9} scale {scale}"
        );
        assert!(done, "{label}");
        assert_eq!(position, [x as i32, y as i32, z as i32], "{label}");
        assert_eq!(counters, byte as u8, "{label}");
        assert_eq!((direction, called), (7, 8), "{label}");
    }
    // Refusals leave the receiver untouched: lifecycle byte 15 and a blocked step check.
    for (state, admission) in [
        (
            BackwardState {
                living_state: 15,
                walk_backward_step_attribute: 8,
                ..Default::default()
            },
            admitted(),
        ),
        (
            BackwardState {
                walk_backward_step_attribute: 8,
                ..Default::default()
            },
            LocomotionAdmission {
                backward_step_clear: Some(false),
                ..admitted()
            },
        ),
    ] {
        let (done, position, counters, direction, called) = run(0, 0, 256, state, admission);
        assert_eq!(
            (done, position, counters, direction, called),
            (true, [0; 3], 0, 0, 0)
        );
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
