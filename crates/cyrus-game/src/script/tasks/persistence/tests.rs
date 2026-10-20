//! Native continuations resume through VM calls and explicit restored service bindings.
use super::*;
use crate::script::tasks::{
    FaceTarget, Lookup, MoveTarget, NativeServices, PlayerFaceCall, PlayerFaceOutcome,
    PlayerFaceState, PointHost, SpatialCall, Speech,
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
            markers: None,
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
            markers: None,
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
        markers: None,
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
            markers: None,
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
fn move_to_marker_matches_the_original_routine() {
    // Rows run through the original MoveToMarker in an emulator (research oracle), one per line:
    // slave, position, attached position, marker position (map units), duration, elapsed, frame
    // scale, then the original's result (0 done, 1 running), the position after and the number of
    // map updates raised.
    const ROWS: &str = "\
1,256,-125898,-2560,102175,-290219,-38831,-1025,4,4,12,0,128,1,-16373,-112929,-898,1
0,-2560,0,77703,-168925,282211,-280562,0,4,-16,40,2,300,1,-2478,0,75063,1
0,-109065,-2560,256,-188105,-201938,126970,-1012,4,0,2,0,512,1,-109065,-2560,256,0
1,-2560,0,-285060,37505,-115106,-154446,0,4,0,200,0,256,1,-2761,571,-284289,1
0,-2560,181979,0,-10654,268463,-78193,-16,-16,-16,3,0,256,1,-4015,88919,-2909,1
0,0,256,256,-232508,-51270,274247,-465,0,-165,1,199,300,0,-119040,0,-42240,1
0,279395,-2560,-2560,-268707,-112744,-138258,4,0,-16,5,39,256,0,1024,0,-4096,1
1,-2560,256,-2560,136856,17955,122754,4,-701,4,3,1,512,1,-2560,256,-2560,0
1,256,219213,296892,-243045,261094,-271897,-300,-16,4,200,0,256,1,1070,217877,298259,1
0,238768,0,256633,-271651,-55239,-15921,4,4,-16,2,199,128,0,1024,1024,-4096,1
0,-2560,-2560,0,160265,-282415,25470,-16,0,205,40,250,1000,0,-4096,0,52480,1
0,0,88625,256,179477,-118613,-22901,0,0,-16,40,11,1000,1,0,75949,-537,1
0,0,0,256,-11202,251690,-12959,4,834,-16,12,1,256,1,0,21018,-412,1
0,0,-2560,-2560,-37041,6457,-16260,0,-16,4,200,250,300,0,0,-4096,1024,1
0,256,91785,172019,-68071,-260277,-87470,-16,0,-392,200,0,300,1,229,91227,170398,1
1,0,124363,0,-282938,-208899,295302,23,665,0,2,0,1000,1,0,124363,0,0
1,-2560,256,0,245030,235833,-133266,4,-16,6,3,0,256,1,-126116,-120426,66087,1
0,0,0,-2560,-41460,-116958,-164959,4,-16,-1070,1,0,128,0,1024,-4096,-273920,1
0,256,256,-2560,-45299,-220530,-147140,0,0,-337,5,250,256,0,0,0,-86272,1
1,256,0,64785,221324,66937,167213,-16,-16,0,0,199,256,0,-4096,-4096,0,1
1,154584,256,-2560,-58157,-206838,63081,0,-16,4,40,2,128,1,155326,2992,-3442,1
1,0,227979,0,149691,-137820,125925,0,-1028,4,200,11,1000,1,-3169,225338,-2641,1
1,-53080,-2560,-9455,-264602,154469,17956,-16,4,-16,200,0,1000,1,-47967,-5685,-9929,1
1,-247933,-2560,-123896,-154344,168552,152085,4,0,4,2,4,128,0,1024,0,1024,1
0,0,81555,-2560,39092,89498,-61570,4,0,675,0,0,77,0,1024,0,172800,1
1,0,256,215725,-247917,260254,-190164,4,4,4,1,39,1000,0,1024,1024,1024,1
0,0,256,0,-173677,121390,48892,-16,0,-16,12,199,128,0,-4096,0,-4096,1
1,-2560,-2560,0,-228881,-260994,158268,844,4,0,12,0,1000,1,216844,128238,-80167,1
1,-2560,256,256,229667,-120782,205499,0,4,4,5,199,1000,0,0,1024,1024,1
0,-2560,256,-267190,-119314,48497,165561,141,0,4,12,0,300,1,1673,-215,-237553,1
0,-2560,-2560,-2560,239419,39394,-100575,-16,-16,4,0,1,1000,0,-4096,-4096,1024,1
0,-2560,256,256,116719,242111,-212167,91,-16,-1028,0,11,300,0,23296,-4096,-263168,1
0,162174,0,0,281683,120052,274058,0,0,0,12,2,77,1,156582,0,0,1
0,-80342,196809,-2560,-184254,-5961,-61292,4,-16,4,200,1,300,1,-79859,195601,-2540,1
0,256,0,-2560,-270199,110626,-75586,0,0,-942,5,199,256,0,0,0,-241152,1
0,256,191567,256,143259,-203867,13902,-16,4,-251,40,39,77,0,-4096,1024,-64256,1
0,233358,-64166,0,98327,182471,239701,0,4,0,5,11,256,0,0,1024,0,1
1,255760,-2560,256,174211,-277309,139040,-16,-16,0,12,39,256,0,-4096,-4096,0,1
1,-2560,256,256,-163972,-79986,109743,4,4,4,1,11,512,0,1024,1024,1024,1
0,196715,0,0,3733,47736,38491,4,903,4,3,39,512,0,1024,231168,1024,1
1,-2560,-2560,256,225870,239815,180916,0,-787,4,5,0,1000,1,-233282,-447524,-181026,1
1,0,0,128032,-18318,-202208,-517,4,0,-16,1,1,1000,0,1024,0,-4096,1
0,-2560,256,256,1059,203761,206127,59,-16,4,200,11,77,1,-2533,249,256,1
1,-2560,-216858,-277494,-232104,-31246,-208706,0,0,4,0,2,300,0,0,0,1024,1
0,-2560,0,98023,79097,-84644,-177223,0,-16,-16,3,4,128,0,0,-4096,-4096,1
0,8754,0,-2560,40903,83809,-214873,-684,82,0,0,199,256,0,-175104,20992,0,1
0,256,-2560,-2560,-198121,137460,107700,4,-16,0,0,4,256,0,1024,-4096,0,1
0,0,256,0,-218660,54433,-240453,4,4,-649,2,250,300,0,1024,1024,-166144,1
1,256,0,256,-209782,266039,253455,779,4,916,3,39,128,0,199424,1024,234496,1
0,0,-96481,-109856,172277,183225,92978,0,4,-376,12,199,77,0,0,1024,-96256,1
0,126971,0,0,-33727,253619,-13550,-16,4,-16,1,0,512,0,-4096,1024,-4096,1
0,47245,256,-2560,76598,25066,-49423,0,92,-16,12,39,256,0,0,23552,-4096,1
0,295730,256,-2560,146017,200903,-142868,-16,0,4,1,0,256,0,-4096,0,1024,1
1,0,-2560,256,171969,-103197,177158,-16,-16,-16,2,250,128,0,-4096,-4096,-4096,1
1,128162,-125866,-2560,295827,-215184,-241300,4,0,-877,1,0,512,0,1024,0,-224512,1
1,0,256,0,-26456,-296355,-179003,4,4,0,5,0,256,1,6800,73699,43522,1
0,256,-191090,-160976,20367,-75374,-25363,931,-16,-16,3,250,300,0,238336,-4096,-4096,1
0,127617,-2560,-177860,-24961,111734,18507,4,4,-13,1,4,128,0,1024,1024,-3328,1
0,-34881,256,-2560,222852,122568,230666,-16,-16,-16,200,199,1000,0,-4096,-4096,-4096,1
1,256,-19504,-2560,190,173442,-169319,-16,0,-255,200,199,256,0,-4096,0,-65280,1
0,254044,0,-2560,-269521,-89734,99404,-16,-16,0,200,39,1000,1,247592,-202,-2560,1
0,-46049,-211536,-2560,-154340,10200,-243331,0,0,-16,200,2,256,1,-45827,-210475,-2578,1
0,-2560,84299,256,220959,250645,221981,-16,4,-16,40,0,512,1,-2698,79911,-19,1
1,0,-85137,123152,162087,-259683,-243052,-16,-505,4,200,4,256,1,-852,-84492,124391,1
1,-2560,-2560,-2560,-278899,33889,-96779,0,4,4,40,0,128,1,1013,-3037,-1310,1
0,256,256,-2560,-166888,57996,250535,-16,4,0,0,250,77,0,-4096,1024,0,1
1,36042,125043,-226751,-144294,209018,174134,0,0,4,5,11,256,0,0,0,1024,1
0,0,-2560,0,-165110,-234446,-75211,4,-16,0,12,0,256,1,89,-2699,0,1
0,256,0,256,-282831,-115841,-155255,4,-734,4,5,1,77,1,256,-20876,256,1
1,0,-22879,256,-66164,246843,279006,4,442,4,40,199,128,0,1024,113152,1024,1
0,-32138,134742,0,153871,247768,206528,0,-16,0,2,4,256,0,0,-4096,0,1
1,-2560,0,0,-48884,-70785,-156746,4,-16,4,40,1,300,1,-1080,2002,4875,1
1,-268300,256,0,-51536,-52889,225348,-16,467,-16,2,4,1000,0,-4096,119552,-4096,1
0,0,256,-2560,-57020,-129238,102418,-740,0,-524,5,39,77,0,-189440,0,-134144,1
0,0,256,-2560,-1274,-180194,92324,4,-16,4,3,4,77,0,1024,-4096,1024,1
1,0,256,-40965,292760,-175766,206268,-16,-1045,0,0,39,512,0,-4096,-267520,0,1
1,-2560,-2560,-67488,37754,236803,225035,4,-16,-226,1,199,512,0,1024,-4096,-57856,1
1,0,0,256,186471,-22828,-282777,0,0,0,2,0,1000,1,0,0,256,0
1,-2560,5505,0,13251,-140189,294560,-16,4,4,0,39,512,0,-4096,1024,1024,1
0,0,-2560,-2560,-282408,-126617,-213665,4,113,4,3,0,128,1,247,5240,-1694,1";
    for row in ROWS.lines() {
        let v: Vec<i32> = row.split(',').map(|value| value.parse().unwrap()).collect();
        let metadata = serde_json::json!({
            "markers": (0..8)
                .map(|index| serde_json::json!({
                    "map_position": if index == 7 { [v[7], v[8], v[9]] } else { [0, 0, 0] },
                    "reserved": u8::from(index == 7),
                }))
                .collect::<Vec<_>>(),
        });
        let markers = crate::script::markers::Markers::prepare(&metadata).unwrap();
        let mut position = [v[1], v[2], v[3]];
        let mut angles = [0; 3];
        let mut updates = vec![];
        let mut spatial = Some(SpatialCall {
            position: &mut position,
            angles: &mut angles,
            frame_scale: v[12] as u32,
            turn: 0,
            nominal_ticks: 1,
            matrix: Matrix::IDENTITY,
            slave: v[0] != 0,
            locations: &[],
            markers: Some(&markers),
            effective_position: Some([v[4], v[5], v[6]]),
            player_position: None,
            actor_index: 5,
            map_updates: &mut updates,
        });
        let done = super::super::Task::Marker
            .complete(
                &[Argument::Integer(7), Argument::Integer(v[10])],
                Invocation {
                    elapsed: Some(v[11]),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
                &mut None,
                &mut spatial,
                NativeServices::default(),
            )
            .unwrap();
        assert_eq!(i32::from(!done), v[13], "{row}");
        assert_eq!(position, [v[14], v[15], v[16]], "{row}");
        assert_eq!(updates.len() as i32, v[17], "{row}");
    }
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
                markers: None,
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
            markers: None,
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
                        markers: None,
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
fn face_player_matches_the_original_routine() {
    // Rows from the original routine in an emulator: heading, player offset X and Z, duration,
    // elapsed input, heading afterwards and the routine's continue flag.
    let rows: Vec<[i64; 7]> = serde_json::from_str(
        r#"[
[0,0,5000,10,0,0,0],
[0,0,5000,10,5,0,0],
[0,0,5000,10,9,0,0],
[0,0,5000,4,1,0,0],
[0,5000,0,10,0,1997,1],
[0,5000,0,10,5,1946,1],
[0,5000,0,10,9,1536,0],
[0,5000,0,4,1,1878,1],
[0,0,-5000,10,0,102,1],
[0,0,-5000,10,5,204,1],
[0,0,-5000,10,9,1024,0],
[0,0,-5000,4,1,341,1],
[0,-5000,0,10,0,51,1],
[0,-5000,0,10,5,102,1],
[0,-5000,0,10,9,512,0],
[0,-5000,0,4,1,170,1],
[0,3000,4000,10,0,2028,1],
[0,3000,4000,10,5,2007,1],
[0,3000,4000,10,9,1839,0],
[0,3000,4000,4,1,1979,1],
[0,-3000,4000,10,0,20,1],
[0,-3000,4000,10,5,41,1],
[0,-3000,4000,10,9,209,0],
[0,-3000,4000,4,1,69,1],
[0,4000,-3000,10,0,1976,1],
[0,4000,-3000,10,5,1904,1],
[0,4000,-3000,10,9,1327,0],
[0,4000,-3000,4,1,1808,1],
[0,-2500,-2500,10,0,76,1],
[0,-2500,-2500,10,5,153,1],
[0,-2500,-2500,10,9,768,0],
[0,-2500,-2500,4,1,256,1],
[300,0,5000,10,0,270,1],
[300,0,5000,10,5,240,1],
[300,0,5000,10,9,0,0],
[300,0,5000,4,1,200,1],
[300,5000,0,10,0,219,1],
[300,5000,0,10,5,138,1],
[300,5000,0,10,9,1536,0],
[300,5000,0,4,1,30,1],
[300,0,-5000,10,0,372,1],
[300,0,-5000,10,5,444,1],
[300,0,-5000,10,9,1024,0],
[300,0,-5000,4,1,541,1],
[300,-5000,0,10,0,321,1],
[300,-5000,0,10,5,342,1],
[300,-5000,0,10,9,512,0],
[300,-5000,0,4,1,370,1],
[300,3000,4000,10,0,250,1],
[300,3000,4000,10,5,199,1],
[300,3000,4000,10,9,1839,0],
[300,3000,4000,4,1,131,1],
[300,-3000,4000,10,0,291,1],
[300,-3000,4000,10,5,282,1],
[300,-3000,4000,10,9,209,0],
[300,-3000,4000,4,1,270,1],
[300,4000,-3000,10,0,198,1],
[300,4000,-3000,10,5,96,1],
[300,4000,-3000,10,9,1327,0],
[300,4000,-3000,4,1,2008,1],
[300,-2500,-2500,10,0,346,1],
[300,-2500,-2500,10,5,393,1],
[300,-2500,-2500,10,9,768,0],
[300,-2500,-2500,4,1,456,1],
[1024,0,5000,10,0,1126,1],
[1024,0,5000,10,5,1228,1],
[1024,0,5000,10,9,0,0],
[1024,0,5000,4,1,1365,1],
[1024,5000,0,10,0,1075,1],
[1024,5000,0,10,5,1126,1],
[1024,5000,0,10,9,1536,0],
[1024,5000,0,4,1,1194,1],
[1024,0,-5000,10,0,1024,0],
[1024,0,-5000,10,5,1024,0],
[1024,0,-5000,10,9,1024,0],
[1024,0,-5000,4,1,1024,0],
[1024,-5000,0,10,0,973,1],
[1024,-5000,0,10,5,922,1],
[1024,-5000,0,10,9,512,0],
[1024,-5000,0,4,1,854,1],
[1024,3000,4000,10,0,1105,1],
[1024,3000,4000,10,5,1187,1],
[1024,3000,4000,10,9,1839,0],
[1024,3000,4000,4,1,1295,1],
[1024,-3000,4000,10,0,943,1],
[1024,-3000,4000,10,5,861,1],
[1024,-3000,4000,10,9,209,0],
[1024,-3000,4000,4,1,753,1],
[1024,4000,-3000,10,0,1054,1],
[1024,4000,-3000,10,5,1084,1],
[1024,4000,-3000,10,9,1327,0],
[1024,4000,-3000,4,1,1125,1],
[1024,-2500,-2500,10,0,999,1],
[1024,-2500,-2500,10,5,973,1],
[1024,-2500,-2500,10,9,768,0],
[1024,-2500,-2500,4,1,939,1],
[1700,0,5000,10,0,1734,1],
[1700,0,5000,10,5,1769,1],
[1700,0,5000,10,9,0,0],
[1700,0,5000,4,1,1816,1],
[1700,5000,0,10,0,1684,1],
[1700,5000,0,10,5,1668,1],
[1700,5000,0,10,9,1536,0],
[1700,5000,0,4,1,1646,1],
[1700,0,-5000,10,0,1633,1],
[1700,0,-5000,10,5,1565,1],
[1700,0,-5000,10,9,1024,0],
[1700,0,-5000,4,1,1475,1],
[1700,-5000,0,10,0,1786,1],
[1700,-5000,0,10,5,1872,1],
[1700,-5000,0,10,9,512,0],
[1700,-5000,0,4,1,1986,1],
[1700,3000,4000,10,0,1713,1],
[1700,3000,4000,10,5,1727,1],
[1700,3000,4000,10,9,1839,0],
[1700,3000,4000,4,1,1746,1],
[1700,-3000,4000,10,0,1755,1],
[1700,-3000,4000,10,5,1811,1],
[1700,-3000,4000,10,9,209,0],
[1700,-3000,4000,4,1,1885,1],
[1700,4000,-3000,10,0,1663,1],
[1700,4000,-3000,10,5,1626,1],
[1700,4000,-3000,10,9,1327,0],
[1700,4000,-3000,4,1,1576,1],
[1700,-2500,-2500,10,0,1607,1],
[1700,-2500,-2500,10,5,1514,1],
[1700,-2500,-2500,10,9,768,0],
[1700,-2500,-2500,4,1,1390,1]
]"#,
    )
    .unwrap();
    for [heading, dx, dz, duration, elapsed, after, report] in rows {
        let mut task = Task::FacePlayer;
        let mut position = [0; 3];
        let mut angles = [0, heading as u16, 0];
        let mut updates = vec![];
        let done = task
            .complete(
                &[Argument::Integer(duration as i32)],
                Invocation {
                    elapsed: Some(elapsed as i32),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
                &mut None,
                &mut Some(SpatialCall {
                    position: &mut position,
                    angles: &mut angles,
                    frame_scale: 256,
                    turn: 0,
                    nominal_ticks: 1,
                    matrix: Matrix::IDENTITY,
                    slave: false,
                    locations: &[],
                    markers: None,
                    effective_position: None,
                    player_position: Some([dx as i32, 0, dz as i32]),
                    actor_index: 0,
                    map_updates: &mut updates,
                }),
                NativeServices::default(),
            )
            .unwrap();
        let label =
            format!("heading {heading} player ({dx},{dz}) duration {duration} elapsed {elapsed}");
        assert_eq!(i64::from(angles[1]), after, "{label}");
        assert_eq!(!done, report != 0, "{label}");
    }
}

#[test]
fn face_player_inertia_matches_the_original_routine() {
    // Rows from the original routine in an emulator: heading, player offset X and Z, duration,
    // elapsed input, heading afterwards and the routine's continue flag.
    let rows: Vec<[i64; 7]> = serde_json::from_str(
        r#"[
[0,0,5000,10,0,0,0],
[0,0,5000,10,5,0,0],
[0,0,5000,10,9,0,0],
[0,0,5000,4,1,0,0],
[0,5000,0,10,0,1997,1],
[0,5000,0,10,5,1997,1],
[0,5000,0,10,9,1997,0],
[0,5000,0,4,1,1921,1],
[0,0,-5000,10,0,102,1],
[0,0,-5000,10,5,102,1],
[0,0,-5000,10,9,102,0],
[0,0,-5000,4,1,255,1],
[0,-5000,0,10,0,51,1],
[0,-5000,0,10,5,51,1],
[0,-5000,0,10,9,51,0],
[0,-5000,0,4,1,127,1],
[0,3000,4000,10,0,2028,1],
[0,3000,4000,10,5,2028,1],
[0,3000,4000,10,9,2028,0],
[0,3000,4000,4,1,1996,1],
[0,-3000,4000,10,0,20,1],
[0,-3000,4000,10,5,20,1],
[0,-3000,4000,10,9,20,0],
[0,-3000,4000,4,1,52,1],
[0,4000,-3000,10,0,1976,1],
[0,4000,-3000,10,5,1976,1],
[0,4000,-3000,10,9,1976,0],
[0,4000,-3000,4,1,1868,1],
[0,-2500,-2500,10,0,76,1],
[0,-2500,-2500,10,5,76,1],
[0,-2500,-2500,10,9,76,0],
[0,-2500,-2500,4,1,191,1],
[300,0,5000,10,0,270,1],
[300,0,5000,10,5,271,1],
[300,0,5000,10,9,271,0],
[300,0,5000,4,1,226,1],
[300,5000,0,10,0,219,1],
[300,5000,0,10,5,219,1],
[300,5000,0,10,9,219,0],
[300,5000,0,4,1,98,1],
[300,0,-5000,10,0,372,1],
[300,0,-5000,10,5,372,1],
[300,0,-5000,10,9,372,0],
[300,0,-5000,4,1,480,1],
[300,-5000,0,10,0,321,1],
[300,-5000,0,10,5,321,1],
[300,-5000,0,10,9,321,0],
[300,-5000,0,4,1,352,1],
[300,3000,4000,10,0,250,1],
[300,3000,4000,10,5,250,1],
[300,3000,4000,10,9,250,0],
[300,3000,4000,4,1,173,1],
[300,-3000,4000,10,0,291,1],
[300,-3000,4000,10,5,291,1],
[300,-3000,4000,10,9,291,0],
[300,-3000,4000,4,1,278,1],
[300,4000,-3000,10,0,198,1],
[300,4000,-3000,10,5,198,1],
[300,4000,-3000,10,9,198,0],
[300,4000,-3000,4,1,45,1],
[300,-2500,-2500,10,0,346,1],
[300,-2500,-2500,10,5,346,1],
[300,-2500,-2500,10,9,346,0],
[300,-2500,-2500,4,1,416,1],
[1024,0,5000,10,0,1126,1],
[1024,0,5000,10,5,1126,1],
[1024,0,5000,10,9,1126,0],
[1024,0,5000,4,1,1279,1],
[1024,5000,0,10,0,1075,1],
[1024,5000,0,10,5,1075,1],
[1024,5000,0,10,9,1075,0],
[1024,5000,0,4,1,1151,1],
[1024,0,-5000,10,0,1024,0],
[1024,0,-5000,10,5,1024,0],
[1024,0,-5000,10,9,1024,0],
[1024,0,-5000,4,1,1024,0],
[1024,-5000,0,10,0,973,1],
[1024,-5000,0,10,5,973,1],
[1024,-5000,0,10,9,973,0],
[1024,-5000,0,4,1,897,1],
[1024,3000,4000,10,0,1105,1],
[1024,3000,4000,10,5,1105,1],
[1024,3000,4000,10,9,1105,0],
[1024,3000,4000,4,1,1227,1],
[1024,-3000,4000,10,0,943,1],
[1024,-3000,4000,10,5,943,1],
[1024,-3000,4000,10,9,943,0],
[1024,-3000,4000,4,1,821,1],
[1024,4000,-3000,10,0,1054,1],
[1024,4000,-3000,10,5,1054,1],
[1024,4000,-3000,10,9,1054,0],
[1024,4000,-3000,4,1,1099,1],
[1024,-2500,-2500,10,0,999,1],
[1024,-2500,-2500,10,5,999,1],
[1024,-2500,-2500,10,9,999,0],
[1024,-2500,-2500,4,1,961,1],
[1700,0,5000,10,0,1734,1],
[1700,0,5000,10,5,1734,1],
[1700,0,5000,10,9,1734,0],
[1700,0,5000,4,1,1786,1],
[1700,5000,0,10,0,1684,1],
[1700,5000,0,10,5,1684,1],
[1700,5000,0,10,9,1684,0],
[1700,5000,0,4,1,1660,1],
[1700,0,-5000,10,0,1633,1],
[1700,0,-5000,10,5,1633,1],
[1700,0,-5000,10,9,1633,0],
[1700,0,-5000,4,1,1532,1],
[1700,-5000,0,10,0,1786,1],
[1700,-5000,0,10,5,1785,1],
[1700,-5000,0,10,9,1785,0],
[1700,-5000,0,4,1,1914,1],
[1700,3000,4000,10,0,1713,1],
[1700,3000,4000,10,5,1713,1],
[1700,3000,4000,10,9,1713,0],
[1700,3000,4000,4,1,1734,1],
[1700,-3000,4000,10,0,1755,1],
[1700,-3000,4000,10,5,1755,1],
[1700,-3000,4000,10,9,1755,0],
[1700,-3000,4000,4,1,1839,1],
[1700,4000,-3000,10,0,1663,1],
[1700,4000,-3000,10,5,1663,1],
[1700,4000,-3000,10,9,1663,0],
[1700,4000,-3000,4,1,1607,1],
[1700,-2500,-2500,10,0,1607,1],
[1700,-2500,-2500,10,5,1607,1],
[1700,-2500,-2500,10,9,1607,0],
[1700,-2500,-2500,4,1,1468,1]
]"#,
    )
    .unwrap();
    for [heading, dx, dz, duration, elapsed, after, report] in rows {
        let mut task = Task::FacePlayerInertia;
        let mut position = [0; 3];
        let mut angles = [0, heading as u16, 0];
        let mut updates = vec![];
        let done = task
            .complete(
                &[Argument::Integer(duration as i32)],
                Invocation {
                    elapsed: Some(elapsed as i32),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
                &mut None,
                &mut Some(SpatialCall {
                    position: &mut position,
                    angles: &mut angles,
                    frame_scale: 256,
                    turn: 0,
                    nominal_ticks: 1,
                    matrix: Matrix::IDENTITY,
                    slave: false,
                    locations: &[],
                    markers: None,
                    effective_position: None,
                    player_position: Some([dx as i32, 0, dz as i32]),
                    actor_index: 0,
                    map_updates: &mut updates,
                }),
                NativeServices::default(),
            )
            .unwrap();
        let label =
            format!("heading {heading} player ({dx},{dz}) duration {duration} elapsed {elapsed}");
        assert_eq!(i64::from(angles[1]), after, "{label}");
        assert_eq!(!done, report != 0, "{label}");
    }
}

#[test]
fn axis_movement_and_rotation_match_the_original_routines() {
    // Rows from the original routines in an emulator, starting at position (1000, 2000, 3000) and
    // angles (100, 300, 50): axis, delta or target, duration, frame scale, then per call the
    // position, the angles and the routine's continue flag, with the elapsed input counting from 0.
    type Step = ([i32; 3], [i32; 3], u32);
    type Row = (i32, i32, i32, u32, Vec<Step>);
    let rows = |text: &str| serde_json::from_str::<Vec<Row>>(text).unwrap();
    let cases = [
        (
            "MoveByAxis",
            rows(
                r#"[
[0,1024,4,256,[[[1256,2000,3000],[100,300,50],1],[[1512,2000,3000],[100,300,50],1],[[1768,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,1024,4,100,[[[1100,2000,3000],[100,300,50],1],[[1200,2000,3000],[100,300,50],1],[[1300,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,-1536,4,256,[[[488,2000,3000],[100,300,50],1],[[-24,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[0,-1536,4,100,[[[800,2000,3000],[100,300,50],1],[[600,2000,3000],[100,300,50],1],[[400,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[0,-1536,7,256,[[[744,2000,3000],[100,300,50],1],[[488,2000,3000],[100,300,50],1],[[232,2000,3000],[100,300,50],1],[[-24,2000,3000],[100,300,50],1],[[-280,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[0,-1536,7,100,[[[900,2000,3000],[100,300,50],1],[[800,2000,3000],[100,300,50],1],[[700,2000,3000],[100,300,50],1],[[600,2000,3000],[100,300,50],1],[[500,2000,3000],[100,300,50],1],[[400,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[1,1024,4,256,[[[1000,2256,3000],[100,300,50],1],[[1000,2512,3000],[100,300,50],1],[[1000,2768,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,1024,4,100,[[[1000,2100,3000],[100,300,50],1],[[1000,2200,3000],[100,300,50],1],[[1000,2300,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,-1536,4,256,[[[1000,1488,3000],[100,300,50],1],[[1000,976,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[1,-1536,4,100,[[[1000,1800,3000],[100,300,50],1],[[1000,1600,3000],[100,300,50],1],[[1000,1400,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[1,-1536,7,256,[[[1000,1744,3000],[100,300,50],1],[[1000,1488,3000],[100,300,50],1],[[1000,1232,3000],[100,300,50],1],[[1000,976,3000],[100,300,50],1],[[1000,720,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[1,-1536,7,100,[[[1000,1900,3000],[100,300,50],1],[[1000,1800,3000],[100,300,50],1],[[1000,1700,3000],[100,300,50],1],[[1000,1600,3000],[100,300,50],1],[[1000,1500,3000],[100,300,50],1],[[1000,1400,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[2,1024,4,256,[[[1000,2000,3256],[100,300,50],1],[[1000,2000,3512],[100,300,50],1],[[1000,2000,3768],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,1024,4,100,[[[1000,2000,3100],[100,300,50],1],[[1000,2000,3200],[100,300,50],1],[[1000,2000,3300],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,-1536,4,256,[[[1000,2000,2488],[100,300,50],1],[[1000,2000,1976],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]],
[2,-1536,4,100,[[[1000,2000,2800],[100,300,50],1],[[1000,2000,2600],[100,300,50],1],[[1000,2000,2400],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]],
[2,-1536,7,256,[[[1000,2000,2744],[100,300,50],1],[[1000,2000,2488],[100,300,50],1],[[1000,2000,2232],[100,300,50],1],[[1000,2000,1976],[100,300,50],1],[[1000,2000,1720],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]],
[2,-1536,7,100,[[[1000,2000,2900],[100,300,50],1],[[1000,2000,2800],[100,300,50],1],[[1000,2000,2700],[100,300,50],1],[[1000,2000,2600],[100,300,50],1],[[1000,2000,2500],[100,300,50],1],[[1000,2000,2400],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]]
]"#,
            ),
        ),
        (
            "RotateByAxis",
            rows(
                r#"[
[0,1024,4,256,[[[1000,2000,3000],[356,300,50],1],[[1000,2000,3000],[612,300,50],1],[[1000,2000,3000],[868,300,50],1],[[1000,2000,3000],[1124,300,50],0]]],
[0,1024,4,100,[[[1000,2000,3000],[200,300,50],1],[[1000,2000,3000],[300,300,50],1],[[1000,2000,3000],[400,300,50],1],[[1000,2000,3000],[500,300,50],1],[[1000,2000,3000],[600,300,50],1],[[1000,2000,3000],[700,300,50],1],[[1000,2000,3000],[800,300,50],1]]],
[0,1024,7,256,[[[1000,2000,3000],[246,300,50],1],[[1000,2000,3000],[392,300,50],1],[[1000,2000,3000],[538,300,50],1],[[1000,2000,3000],[685,300,50],1],[[1000,2000,3000],[831,300,50],1],[[1000,2000,3000],[977,300,50],1],[[1000,2000,3000],[1123,300,50],1],[[1000,2000,3000],[1124,300,50],0]]],
[0,1024,7,100,[[[1000,2000,3000],[157,300,50],1],[[1000,2000,3000],[214,300,50],1],[[1000,2000,3000],[271,300,50],1],[[1000,2000,3000],[328,300,50],1],[[1000,2000,3000],[385,300,50],1],[[1000,2000,3000],[442,300,50],1],[[1000,2000,3000],[499,300,50],1],[[1000,2000,3000],[557,300,50],1],[[1000,2000,3000],[614,300,50],1],[[1000,2000,3000],[671,300,50],1]]],
[0,-1536,4,256,[[[1000,2000,3000],[1764,300,50],1],[[1000,2000,3000],[1380,300,50],1],[[1000,2000,3000],[996,300,50],1],[[1000,2000,3000],[612,300,50],0]]],
[0,-1536,4,100,[[[1000,2000,3000],[1998,300,50],1],[[1000,2000,3000],[1848,300,50],1],[[1000,2000,3000],[1698,300,50],1],[[1000,2000,3000],[1548,300,50],1],[[1000,2000,3000],[1398,300,50],1],[[1000,2000,3000],[1248,300,50],1],[[1000,2000,3000],[1098,300,50],1]]],
[0,-1536,7,256,[[[1000,2000,3000],[1928,300,50],1],[[1000,2000,3000],[1709,300,50],1],[[1000,2000,3000],[1489,300,50],1],[[1000,2000,3000],[1270,300,50],1],[[1000,2000,3000],[1050,300,50],1],[[1000,2000,3000],[831,300,50],1],[[1000,2000,3000],[612,300,50],0]]],
[0,-1536,7,100,[[[1000,2000,3000],[14,300,50],1],[[1000,2000,3000],[1976,300,50],1],[[1000,2000,3000],[1890,300,50],1],[[1000,2000,3000],[1805,300,50],1],[[1000,2000,3000],[1719,300,50],1],[[1000,2000,3000],[1633,300,50],1],[[1000,2000,3000],[1547,300,50],1],[[1000,2000,3000],[1462,300,50],1],[[1000,2000,3000],[1376,300,50],1],[[1000,2000,3000],[1290,300,50],1]]],
[1,1024,4,256,[[[1000,2000,3000],[100,556,50],1],[[1000,2000,3000],[100,812,50],1],[[1000,2000,3000],[100,1068,50],1],[[1000,2000,3000],[100,1324,50],0]]],
[1,1024,4,100,[[[1000,2000,3000],[100,400,50],1],[[1000,2000,3000],[100,500,50],1],[[1000,2000,3000],[100,600,50],1],[[1000,2000,3000],[100,700,50],1],[[1000,2000,3000],[100,800,50],1],[[1000,2000,3000],[100,900,50],1],[[1000,2000,3000],[100,1000,50],1]]],
[1,1024,7,256,[[[1000,2000,3000],[100,446,50],1],[[1000,2000,3000],[100,592,50],1],[[1000,2000,3000],[100,738,50],1],[[1000,2000,3000],[100,885,50],1],[[1000,2000,3000],[100,1031,50],1],[[1000,2000,3000],[100,1177,50],1],[[1000,2000,3000],[100,1323,50],1],[[1000,2000,3000],[100,1324,50],0]]],
[1,1024,7,100,[[[1000,2000,3000],[100,357,50],1],[[1000,2000,3000],[100,414,50],1],[[1000,2000,3000],[100,471,50],1],[[1000,2000,3000],[100,528,50],1],[[1000,2000,3000],[100,585,50],1],[[1000,2000,3000],[100,642,50],1],[[1000,2000,3000],[100,699,50],1],[[1000,2000,3000],[100,757,50],1],[[1000,2000,3000],[100,814,50],1],[[1000,2000,3000],[100,871,50],1]]],
[1,-1536,4,256,[[[1000,2000,3000],[100,1964,50],1],[[1000,2000,3000],[100,1580,50],1],[[1000,2000,3000],[100,1196,50],1],[[1000,2000,3000],[100,812,50],0]]],
[1,-1536,4,100,[[[1000,2000,3000],[100,150,50],1],[[1000,2000,3000],[100,0,50],1],[[1000,2000,3000],[100,1898,50],1],[[1000,2000,3000],[100,1748,50],1],[[1000,2000,3000],[100,1598,50],1],[[1000,2000,3000],[100,1448,50],1],[[1000,2000,3000],[100,1298,50],1]]],
[1,-1536,7,256,[[[1000,2000,3000],[100,80,50],1],[[1000,2000,3000],[100,1909,50],1],[[1000,2000,3000],[100,1689,50],1],[[1000,2000,3000],[100,1470,50],1],[[1000,2000,3000],[100,1250,50],1],[[1000,2000,3000],[100,1031,50],1],[[1000,2000,3000],[100,812,50],0]]],
[1,-1536,7,100,[[[1000,2000,3000],[100,214,50],1],[[1000,2000,3000],[100,128,50],1],[[1000,2000,3000],[100,42,50],1],[[1000,2000,3000],[100,2005,50],1],[[1000,2000,3000],[100,1919,50],1],[[1000,2000,3000],[100,1833,50],1],[[1000,2000,3000],[100,1747,50],1],[[1000,2000,3000],[100,1662,50],1],[[1000,2000,3000],[100,1576,50],1],[[1000,2000,3000],[100,1490,50],1]]],
[2,1024,4,256,[[[1000,2000,3000],[100,300,306],1],[[1000,2000,3000],[100,300,562],1],[[1000,2000,3000],[100,300,818],1],[[1000,2000,3000],[100,300,1074],0]]],
[2,1024,4,100,[[[1000,2000,3000],[100,300,150],1],[[1000,2000,3000],[100,300,250],1],[[1000,2000,3000],[100,300,350],1],[[1000,2000,3000],[100,300,450],1],[[1000,2000,3000],[100,300,550],1],[[1000,2000,3000],[100,300,650],1],[[1000,2000,3000],[100,300,750],1]]],
[2,1024,7,256,[[[1000,2000,3000],[100,300,196],1],[[1000,2000,3000],[100,300,342],1],[[1000,2000,3000],[100,300,488],1],[[1000,2000,3000],[100,300,635],1],[[1000,2000,3000],[100,300,781],1],[[1000,2000,3000],[100,300,927],1],[[1000,2000,3000],[100,300,1073],1],[[1000,2000,3000],[100,300,1074],0]]],
[2,1024,7,100,[[[1000,2000,3000],[100,300,107],1],[[1000,2000,3000],[100,300,164],1],[[1000,2000,3000],[100,300,221],1],[[1000,2000,3000],[100,300,278],1],[[1000,2000,3000],[100,300,335],1],[[1000,2000,3000],[100,300,392],1],[[1000,2000,3000],[100,300,449],1],[[1000,2000,3000],[100,300,507],1],[[1000,2000,3000],[100,300,564],1],[[1000,2000,3000],[100,300,621],1]]],
[2,-1536,4,256,[[[1000,2000,3000],[100,300,1714],1],[[1000,2000,3000],[100,300,1330],1],[[1000,2000,3000],[100,300,946],1],[[1000,2000,3000],[100,300,562],0]]],
[2,-1536,4,100,[[[1000,2000,3000],[100,300,1948],1],[[1000,2000,3000],[100,300,1798],1],[[1000,2000,3000],[100,300,1648],1],[[1000,2000,3000],[100,300,1498],1],[[1000,2000,3000],[100,300,1348],1],[[1000,2000,3000],[100,300,1198],1],[[1000,2000,3000],[100,300,1048],1]]],
[2,-1536,7,256,[[[1000,2000,3000],[100,300,1878],1],[[1000,2000,3000],[100,300,1659],1],[[1000,2000,3000],[100,300,1439],1],[[1000,2000,3000],[100,300,1220],1],[[1000,2000,3000],[100,300,1000],1],[[1000,2000,3000],[100,300,781],1],[[1000,2000,3000],[100,300,562],0]]],
[2,-1536,7,100,[[[1000,2000,3000],[100,300,2012],1],[[1000,2000,3000],[100,300,1926],1],[[1000,2000,3000],[100,300,1840],1],[[1000,2000,3000],[100,300,1755],1],[[1000,2000,3000],[100,300,1669],1],[[1000,2000,3000],[100,300,1583],1],[[1000,2000,3000],[100,300,1497],1],[[1000,2000,3000],[100,300,1412],1],[[1000,2000,3000],[100,300,1326],1],[[1000,2000,3000],[100,300,1240],1]]]
]"#,
            ),
        ),
        (
            "RotateToAxis",
            rows(
                r#"[
[0,0,4,256,[[[1000,2000,3000],[75,300,50],1],[[1000,2000,3000],[50,300,50],1],[[1000,2000,3000],[25,300,50],1],[[1000,2000,3000],[0,300,50],0]]],
[0,0,4,100,[[[1000,2000,3000],[90,300,50],1],[[1000,2000,3000],[80,300,50],1],[[1000,2000,3000],[70,300,50],1],[[1000,2000,3000],[60,300,50],1],[[1000,2000,3000],[51,300,50],1],[[1000,2000,3000],[41,300,50],1],[[1000,2000,3000],[31,300,50],1]]],
[0,0,7,256,[[[1000,2000,3000],[85,300,50],1],[[1000,2000,3000],[71,300,50],1],[[1000,2000,3000],[57,300,50],1],[[1000,2000,3000],[42,300,50],1],[[1000,2000,3000],[28,300,50],1],[[1000,2000,3000],[14,300,50],1],[[1000,2000,3000],[0,300,50],0]]],
[0,0,7,100,[[[1000,2000,3000],[94,300,50],1],[[1000,2000,3000],[88,300,50],1],[[1000,2000,3000],[83,300,50],1],[[1000,2000,3000],[77,300,50],1],[[1000,2000,3000],[72,300,50],1],[[1000,2000,3000],[66,300,50],1],[[1000,2000,3000],[60,300,50],1],[[1000,2000,3000],[55,300,50],1],[[1000,2000,3000],[49,300,50],1],[[1000,2000,3000],[44,300,50],1]]],
[0,700,4,256,[[[1000,2000,3000],[250,300,50],1],[[1000,2000,3000],[400,300,50],1],[[1000,2000,3000],[550,300,50],1],[[1000,2000,3000],[700,300,50],0]]],
[0,700,4,100,[[[1000,2000,3000],[158,300,50],1],[[1000,2000,3000],[217,300,50],1],[[1000,2000,3000],[275,300,50],1],[[1000,2000,3000],[334,300,50],1],[[1000,2000,3000],[392,300,50],1],[[1000,2000,3000],[451,300,50],1],[[1000,2000,3000],[510,300,50],1]]],
[0,700,7,256,[[[1000,2000,3000],[185,300,50],1],[[1000,2000,3000],[271,300,50],1],[[1000,2000,3000],[357,300,50],1],[[1000,2000,3000],[442,300,50],1],[[1000,2000,3000],[528,300,50],1],[[1000,2000,3000],[614,300,50],1],[[1000,2000,3000],[699,300,50],1],[[1000,2000,3000],[700,300,50],0]]],
[0,700,7,100,[[[1000,2000,3000],[133,300,50],1],[[1000,2000,3000],[166,300,50],1],[[1000,2000,3000],[200,300,50],1],[[1000,2000,3000],[233,300,50],1],[[1000,2000,3000],[267,300,50],1],[[1000,2000,3000],[300,300,50],1],[[1000,2000,3000],[334,300,50],1],[[1000,2000,3000],[367,300,50],1],[[1000,2000,3000],[401,300,50],1],[[1000,2000,3000],[434,300,50],1]]],
[1,0,4,256,[[[1000,2000,3000],[100,225,50],1],[[1000,2000,3000],[100,150,50],1],[[1000,2000,3000],[100,75,50],1],[[1000,2000,3000],[100,0,50],0]]],
[1,0,4,100,[[[1000,2000,3000],[100,270,50],1],[[1000,2000,3000],[100,241,50],1],[[1000,2000,3000],[100,212,50],1],[[1000,2000,3000],[100,182,50],1],[[1000,2000,3000],[100,153,50],1],[[1000,2000,3000],[100,124,50],1],[[1000,2000,3000],[100,94,50],1]]],
[1,0,7,256,[[[1000,2000,3000],[100,257,50],1],[[1000,2000,3000],[100,214,50],1],[[1000,2000,3000],[100,171,50],1],[[1000,2000,3000],[100,128,50],1],[[1000,2000,3000],[100,85,50],1],[[1000,2000,3000],[100,42,50],1],[[1000,2000,3000],[100,0,50],0]]],
[1,0,7,100,[[[1000,2000,3000],[100,283,50],1],[[1000,2000,3000],[100,266,50],1],[[1000,2000,3000],[100,249,50],1],[[1000,2000,3000],[100,233,50],1],[[1000,2000,3000],[100,216,50],1],[[1000,2000,3000],[100,199,50],1],[[1000,2000,3000],[100,182,50],1],[[1000,2000,3000],[100,166,50],1],[[1000,2000,3000],[100,149,50],1],[[1000,2000,3000],[100,132,50],1]]],
[1,700,4,256,[[[1000,2000,3000],[100,400,50],1],[[1000,2000,3000],[100,500,50],1],[[1000,2000,3000],[100,600,50],1],[[1000,2000,3000],[100,700,50],0]]],
[1,700,4,100,[[[1000,2000,3000],[100,339,50],1],[[1000,2000,3000],[100,378,50],1],[[1000,2000,3000],[100,417,50],1],[[1000,2000,3000],[100,456,50],1],[[1000,2000,3000],[100,495,50],1],[[1000,2000,3000],[100,534,50],1],[[1000,2000,3000],[100,573,50],1]]],
[1,700,7,256,[[[1000,2000,3000],[100,357,50],1],[[1000,2000,3000],[100,414,50],1],[[1000,2000,3000],[100,471,50],1],[[1000,2000,3000],[100,528,50],1],[[1000,2000,3000],[100,585,50],1],[[1000,2000,3000],[100,642,50],1],[[1000,2000,3000],[100,699,50],1],[[1000,2000,3000],[100,700,50],0]]],
[1,700,7,100,[[[1000,2000,3000],[100,322,50],1],[[1000,2000,3000],[100,344,50],1],[[1000,2000,3000],[100,366,50],1],[[1000,2000,3000],[100,389,50],1],[[1000,2000,3000],[100,411,50],1],[[1000,2000,3000],[100,433,50],1],[[1000,2000,3000],[100,456,50],1],[[1000,2000,3000],[100,478,50],1],[[1000,2000,3000],[100,500,50],1],[[1000,2000,3000],[100,523,50],1]]],
[2,0,4,256,[[[1000,2000,3000],[100,300,37],1],[[1000,2000,3000],[100,300,25],1],[[1000,2000,3000],[100,300,12],1],[[1000,2000,3000],[100,300,0],0]]],
[2,0,4,100,[[[1000,2000,3000],[100,300,45],1],[[1000,2000,3000],[100,300,40],1],[[1000,2000,3000],[100,300,35],1],[[1000,2000,3000],[100,300,30],1],[[1000,2000,3000],[100,300,25],1],[[1000,2000,3000],[100,300,20],1],[[1000,2000,3000],[100,300,15],1]]],
[2,0,7,256,[[[1000,2000,3000],[100,300,42],1],[[1000,2000,3000],[100,300,35],1],[[1000,2000,3000],[100,300,28],1],[[1000,2000,3000],[100,300,21],1],[[1000,2000,3000],[100,300,14],1],[[1000,2000,3000],[100,300,7],1],[[1000,2000,3000],[100,300,0],0]]],
[2,0,7,100,[[[1000,2000,3000],[100,300,47],1],[[1000,2000,3000],[100,300,44],1],[[1000,2000,3000],[100,300,41],1],[[1000,2000,3000],[100,300,38],1],[[1000,2000,3000],[100,300,36],1],[[1000,2000,3000],[100,300,33],1],[[1000,2000,3000],[100,300,30],1],[[1000,2000,3000],[100,300,27],1],[[1000,2000,3000],[100,300,24],1],[[1000,2000,3000],[100,300,22],1]]],
[2,700,4,256,[[[1000,2000,3000],[100,300,212],1],[[1000,2000,3000],[100,300,375],1],[[1000,2000,3000],[100,300,537],1],[[1000,2000,3000],[100,300,700],1],[[1000,2000,3000],[100,300,700],0]]],
[2,700,4,100,[[[1000,2000,3000],[100,300,113],1],[[1000,2000,3000],[100,300,176],1],[[1000,2000,3000],[100,300,240],1],[[1000,2000,3000],[100,300,303],1],[[1000,2000,3000],[100,300,367],1],[[1000,2000,3000],[100,300,430],1],[[1000,2000,3000],[100,300,494],1]]],
[2,700,7,256,[[[1000,2000,3000],[100,300,142],1],[[1000,2000,3000],[100,300,235],1],[[1000,2000,3000],[100,300,328],1],[[1000,2000,3000],[100,300,421],1],[[1000,2000,3000],[100,300,514],1],[[1000,2000,3000],[100,300,607],1],[[1000,2000,3000],[100,300,699],1],[[1000,2000,3000],[100,300,700],0]]],
[2,700,7,100,[[[1000,2000,3000],[100,300,86],1],[[1000,2000,3000],[100,300,122],1],[[1000,2000,3000],[100,300,158],1],[[1000,2000,3000],[100,300,195],1],[[1000,2000,3000],[100,300,231],1],[[1000,2000,3000],[100,300,267],1],[[1000,2000,3000],[100,300,303],1],[[1000,2000,3000],[100,300,340],1],[[1000,2000,3000],[100,300,376],1],[[1000,2000,3000],[100,300,412],1]]]
]"#,
            ),
        ),
    ];
    for (name, rows) in cases {
        for (axis, delta, duration, scale, steps) in rows {
            let mut task = match name {
                "MoveByAxis" => Task::Movement { target: 0 },
                "RotateByAxis" => Task::Rotation {
                    target: 0,
                    accumulator: 0,
                },
                _ => Task::AbsoluteRotation {
                    movement: 0,
                    accumulator: 0,
                },
            };
            let mut position = [1000, 2000, 3000];
            let mut angles = [100, 300, 50];
            let mut updates = vec![];
            for (elapsed, (expected_position, expected_angles, report)) in
                steps.into_iter().enumerate()
            {
                let done = task
                    .complete(
                        &[
                            Argument::Integer(axis),
                            Argument::Integer(delta),
                            Argument::Integer(duration),
                        ],
                        Invocation {
                            elapsed: Some(elapsed as i32),
                            unpaused_tasks: 0,
                            unpaused_blocking_tasks: 0,
                        },
                        &mut None,
                        &mut Some(SpatialCall {
                            position: &mut position,
                            angles: &mut angles,
                            frame_scale: scale,
                            turn: 0,
                            nominal_ticks: 0,
                            matrix: Matrix::IDENTITY,
                            slave: false,
                            locations: &[],
                            markers: None,
                            effective_position: None,
                            player_position: None,
                            actor_index: 0,
                            map_updates: &mut updates,
                        }),
                        NativeServices::default(),
                    )
                    .unwrap();
                let label = format!(
                    "{name} axis {axis} delta {delta} duration {duration} scale {scale} elapsed {elapsed}"
                );
                assert_eq!(position, expected_position, "{label}");
                assert_eq!(angles.map(i32::from), expected_angles, "{label}");
                assert_eq!(!done, report != 0, "{label}");
            }
        }
    }
}

#[derive(Default)]
struct RecordingPointHost {
    moves: Vec<(MoveTarget, i32)>,
    faces: Vec<FaceTarget>,
    result: i32,
}
impl PointHost for RecordingPointHost {
    fn move_to(&mut self, target: MoveTarget, tolerance: i32) -> Result<i32> {
        self.moves.push((target, tolerance));
        Ok(self.result)
    }
    fn face(&mut self, target: FaceTarget) -> Result<i32> {
        self.faces.push(target);
        Ok(self.result)
    }
}

#[test]
fn point_tasks_decode_their_operands_and_complete_when_the_host_step_returns_zero() {
    let invocation = Invocation {
        elapsed: Some(0),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };
    let integers = |values: &[i32]| -> Vec<Argument> {
        values
            .iter()
            .map(|&value| Argument::Integer(value))
            .collect()
    };
    for (task, arguments, expected) in [
        (
            Task::MoveNodeMarker,
            integers(&[7, 300]),
            (MoveTarget::Marker(7), 300),
        ),
        (
            Task::MoveNodeLocation,
            integers(&[2, -5]),
            (MoveTarget::Location(2), -5),
        ),
        (
            Task::MoveNodePosition,
            integers(&[256, -512, 1024, 0]),
            (MoveTarget::Position([256, -512, 1024]), 0),
        ),
    ] {
        for (result, done) in [(1, false), (-3, false), (0, true)] {
            let mut host = RecordingPointHost {
                result,
                ..Default::default()
            };
            let mut task = task;
            let completed = task
                .complete(
                    &arguments,
                    invocation,
                    &mut None,
                    &mut None,
                    NativeServices {
                        point: Some(&mut host),
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(completed, done, "{task:?} result {result}");
            assert_eq!(host.moves, [expected], "{task:?}");
            assert!(host.faces.is_empty());
        }
        let mut wrong = task;
        let error = wrong
            .complete(
                &arguments[1..],
                invocation,
                &mut None,
                &mut None,
                NativeServices {
                    point: Some(&mut RecordingPointHost::default()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("argument"), "{task:?}");
        let mut absent = task;
        assert!(
            absent
                .complete(
                    &arguments,
                    invocation,
                    &mut None,
                    &mut None,
                    NativeServices::default()
                )
                .is_err()
        );
    }
}

#[test]
fn move_and_rotate_add_the_per_invocation_step_until_the_elapsed_input_reaches_the_duration() {
    // Move adds each operand times the nominal tick count; Rotate adds the operand magnitude
    // shifted by eight, divided by the duration, times the elapsed input, shifted back, signed
    // like the operand. Both report "continue" while duration - 1 exceeds the elapsed input.
    for (task, name, operands, ticks, rows) in [
        (
            Task::Move,
            "Move",
            [10, 0, -5],
            3,
            vec![
                (0, [1030, 2000, 2985], [100, 300, 50], true),
                (1, [1060, 2000, 2970], [100, 300, 50], true),
                (2, [1090, 2000, 2955], [100, 300, 50], false),
            ],
        ),
        (
            Task::ScaledMove,
            "sMove",
            [10, 0, -5],
            3,
            // (10 << 8) / 3 >> 8 is 3 and (-5 << 8) / 3 >> 8 is -2 (truncating division, then
            // an arithmetic shift), added on every invocation whatever the tick count.
            vec![
                (0, [1003, 2000, 2998], [100, 300, 50], true),
                (1, [1006, 2000, 2996], [100, 300, 50], true),
                (2, [1009, 2000, 2994], [100, 300, 50], false),
            ],
        ),
        (
            Task::ScaledRotate,
            "sRotate",
            [80, 0, -40],
            3,
            vec![
                (0, [1000, 2000, 3000], [100, 300, 50], true),
                (1, [1000, 2000, 3000], [126, 300, 37], true),
                (2, [1000, 2000, 3000], [179, 300, 11], false),
            ],
        ),
        (
            Task::Rotate,
            "Rotate",
            [80, 0, -40],
            3,
            vec![
                (0, [1000, 2000, 3000], [100, 300, 50], true),
                (1, [1000, 2000, 3000], [126, 300, 37], true),
                (2, [1000, 2000, 3000], [179, 300, 11], false),
            ],
        ),
    ] {
        let mut task = task;
        let mut position = [1000, 2000, 3000];
        let mut angles = [100, 300, 50];
        let mut updates = vec![];
        for (elapsed, expected_position, expected_angles, report) in rows {
            let done = task
                .complete(
                    &[
                        Argument::Integer(operands[0]),
                        Argument::Integer(operands[1]),
                        Argument::Integer(operands[2]),
                        Argument::Integer(3),
                    ],
                    Invocation {
                        elapsed: Some(elapsed),
                        unpaused_tasks: 0,
                        unpaused_blocking_tasks: 0,
                    },
                    &mut None,
                    &mut Some(SpatialCall {
                        position: &mut position,
                        angles: &mut angles,
                        frame_scale: 256,
                        turn: 0,
                        nominal_ticks: ticks,
                        matrix: Matrix::IDENTITY,
                        slave: false,
                        locations: &[],
                        markers: None,
                        effective_position: None,
                        player_position: None,
                        actor_index: 0,
                        map_updates: &mut updates,
                    }),
                    NativeServices::default(),
                )
                .unwrap();
            assert_eq!(position, expected_position, "{name} elapsed {elapsed}");
            assert_eq!(angles, expected_angles, "{name} elapsed {elapsed}");
            assert_eq!(!done, report, "{name} elapsed {elapsed}");
        }
    }
}

#[test]
fn scaled_axis_movement_rotates_the_step_like_the_original_routine() {
    // Rows from the original routine in an emulator at extended precision: axis, delta, duration,
    // frame scale, elapsed input, angles, starting position, resulting position and the
    // routine's continue flag.
    type Row = (i32, i32, i32, u32, i32, [u16; 3], [i32; 3], [i32; 3], u32);
    let rows: Vec<Row> = serde_json::from_str(
        r#"[
[3,-256,40,300,0,[1385,1899,1024],[281194,-257118,97430],[281196,-257111,97433],1],
[1,-256,1000,77,2,[0,542,2047],[-193392,5322,-85084],[-193392,5321,-85084],1],
[1,-256,200,77,39,[1024,512,2047],[-242550,97217,215597],[-242550,97218,215597],1],
[1,100000,5,128,0,[496,512,0],[-212441,-160348,265989],[-202453,-159857,265989],1],
[0,-256,2,300,39,[2047,0,512],[-256799,-243196,-16891],[-256799,-242940,-16890],0],
[1,-3013,12,77,39,[512,1024,1024],[-184693,83326,173222],[-184693,83326,173300],0],
[1,1208,5,512,11,[2047,512,2047],[-17265,40482,-206870],[-17267,41086,-206872],0],
[2,3478,12,77,0,[2047,2047,512],[293521,-230501,145652],[293521,-230501,145741],1],
[1,1110,200,128,250,[512,512,0],[-247043,-118853,-24266],[-247041,-118853,-24266],0],
[1,256,3,128,199,[2047,1205,1024],[-1145,-152524,204277],[-1145,-152566,204277],0],
[2,282966,2,256,0,[2047,512,1568],[-122707,-12528,218453],[-264189,-12962,218453],1],
[2,-256,1,128,1,[512,0,512],[196643,178882,286411],[196643,178754,286411],0],
[3,100000,3,128,4,[0,1784,1264],[252590,263215,-231236],[264660,263215,-219744],0],
[2,256,5,128,199,[0,0,1024],[-44524,180920,-57669],[-44524,180920,-57644],0],
[2,100000,1000,1000,0,[2047,1665,0],[-101725,-264233,-109880],[-101365,-264234,-109730],1],
[0,100000,1,128,250,[0,1024,1024],[-291014,186381,170975],[-241014,186381,170975],0],
[1,2560,1000,512,2,[512,512,0],[-244708,-174076,-221742],[-244703,-174076,-221742],1],
[0,-275407,2,256,250,[1024,512,1024],[242981,-93056,-283767],[242981,-93056,-146063],0],
[3,3510,1,256,39,[1024,1024,0],[-11450,-161347,14728],[-11450,-161347,18238],0],
[2,256,200,1000,4,[2047,1911,2047],[103725,-104614,45480],[103727,-104614,45485],1],
[3,2560,40,128,250,[512,1024,0],[54257,168793,103557],[54257,168825,103557],0],
[1,-220452,40,77,250,[2047,1024,2047],[-296648,-1515,259416],[-296653,-3186,259421],0],
[1,256,5,128,0,[1024,0,2047],[108334,230796,-158607],[108334,230771,-158607],1],
[2,-7680,12,256,1,[729,0,0],[-7953,202045,247927],[-7953,201542,248322],1],
[2,3810,1,256,0,[512,512,2047],[-72898,240396,-87330],[-72898,244206,-87330],0],
[3,2560,1,128,0,[2047,2047,781],[-196453,-125880,-210990],[-196449,-125884,-209710],0],
[2,-256,5,256,250,[49,512,0],[-240764,-89913,55259],[-240713,-89921,55259],0],
[2,-7680,3,77,0,[2047,2047,0],[29644,-118520,-149190],[29641,-118517,-150044],1],
[1,-2509,200,77,39,[2047,1024,2047],[-110333,-200841,237238],[-110333,-200845,237238],1],
[3,100000,12,300,11,[512,240,1024],[-191721,-83768,-284179],[-191721,-73768,-284179],0],
[1,2560,2,512,2,[512,1024,1024],[38822,286739,-131733],[38822,286739,-134293],0],
[0,-256,5,256,0,[2047,1024,0],[128034,146945,-115059],[128086,146945,-115059],1],
[0,4388,3,256,39,[512,1428,0],[-130976,272146,28197],[-131452,272146,26815],0],
[2,2146,12,512,250,[0,1024,2047],[-27769,82633,174014],[-27769,82633,173657],0],
[2,256,200,128,0,[2047,0,2047],[-217369,109063,-28640],[-217369,109063,-28640],1],
[1,256,1000,300,2,[0,486,512],[161976,-192973,99127],[161976,-192973,99127],1],
[2,100000,3,77,4,[2047,512,2047],[-1988,197758,230382],[-13099,197724,230382],0],
[2,100000,1000,77,199,[0,1024,2047],[-129458,258811,-142946],[-129458,258811,-142976],1],
[3,256,200,77,250,[1024,1793,326],[-22491,283846,-121826],[-22491,283846,-121826],0],
[2,100000,40,128,11,[512,1024,1024],[200263,-214039,41413],[200263,-212789,41413],1],
[2,100000,12,128,39,[0,2047,2047],[8506,94717,174014],[8519,94717,178180],0],
[0,256,3,256,0,[1024,2047,0],[168583,-120811,-133304],[168668,-120811,-133304],1],
[0,2560,2,256,11,[512,512,2019],[263493,-96643,127837],[263607,-96643,129112],0],
[2,2560,5,512,4,[512,87,512],[177417,224241,-171587],[177417,225521,-171587],0],
[0,3946,200,1000,11,[512,512,1466],[-211935,-186958,-210917],[-211860,-186958,-210933],1],
[0,85445,1000,256,199,[2047,1051,512],[86011,-234134,7537],[86011,-234219,7537],1],
[1,256,3,256,2,[0,741,2047],[-165317,139336,-5557],[-165317,139421,-5557],0],
[3,256,200,128,250,[512,1953,0],[85609,-197434,294884],[85609,-197434,294884],0],
[2,-1235,2,77,199,[676,1024,512],[53942,-280930,-53438],[53942,-281110,-53537],0],
[0,119235,1,256,199,[0,1024,1244],[-224441,157192,277192],[-131350,231697,277192],0],
[1,256,5,77,2,[0,512,1123],[75870,-196797,276047],[75870,-196812,276042],1],
[2,256,12,1000,11,[512,0,1078],[-50893,-114130,-273029],[-50893,-114045,-273029],0],
[1,-7680,12,512,2,[2047,2047,2047],[92074,-103903,51236],[92078,-105183,51232],1],
[3,256,2,300,11,[1024,0,2047],[132879,-76668,116519],[132879,-76668,116263],0],
[1,2560,200,256,1,[0,0,1360],[-229474,175600,134819],[-229484,175594,134819],1],
[2,-2011,1,77,199,[0,1024,512],[-272330,-18487,116648],[-272330,-18487,117319],0],
[0,256,200,256,39,[2047,324,1024],[-40323,-279989,196909],[-40324,-279989,196908],1],
[1,-7680,3,512,39,[1024,2047,2047],[-283554,29411,-102827],[-283530,37091,-102827],0],
[1,100000,200,128,0,[2047,512,2047],[-42813,293687,84613],[-42814,293937,84612],1],
[0,2560,2,256,4,[1898,2047,841],[39386,151121,88221],[38302,150510,87922],0],
[2,-4050,1,256,199,[512,415,906],[-155228,61475,-53096],[-155228,57425,-53096],0],
[2,100000,3,128,1,[0,858,2047],[-67554,-235710,22452],[-75680,-235710,7901],1],
[2,-256,1,128,4,[898,2047,2047],[-191005,101849,-41322],[-191005,101801,-41203],0],
[1,256,40,300,0,[20,0,0],[47178,-251160,-177394],[47178,-251153,-177394],1],
[0,-7680,3,77,2,[1086,512,512],[-51443,-99487,173946],[-51604,-100326,173946],0],
[2,-42016,200,300,0,[72,2047,0],[-197239,-132091,-231401],[-197240,-132145,-231643],1],
[1,256,5,300,2,[1931,512,1024],[48785,218936,138437],[48807,218876,138437],1],
[2,2560,2,77,11,[1024,2047,2047],[176669,237955,61253],[176668,237955,60827],0],
[1,256,2,256,1,[2047,2047,306],[-46766,-73613,239976],[-46663,-73537,239976],0],
[0,-7680,2,77,0,[61,1024,0],[-252063,-233698,218783],[-250783,-233698,218783],1],
[3,2560,40,512,4,[543,2047,512],[251225,130746,-260013],[251225,130873,-260025],1],
[0,-140028,3,77,199,[512,1024,22],[147271,-117145,-141402],[162795,-117145,-140353],0],
[3,100000,200,256,0,[32,512,1369],[174570,-264046,-265515],[174072,-263997,-265515],1],
[1,12043,2,256,39,[0,1024,2047],[-103263,-43897,94497],[-103245,-37876,94497],0],
[1,2560,2,256,39,[1392,2047,914],[45281,-15462,-147311],[45701,-14946,-148404],0],
[3,-2214,5,77,11,[2047,2046,1640],[229696,124868,113780],[229695,124868,113641],0],
[3,285382,2,77,250,[512,0,512],[-95774,158112,229424],[-95774,205675,229424],0],
[2,-7680,40,128,2,[1245,1024,512],[-191467,171613,266091],[-191467,171673,266016],1],
[3,2560,200,512,4,[42,0,1024],[184735,-109441,91250],[184735,-109438,91275],1],
[2,100000,5,256,250,[1024,0,1305],[-247485,211178,276113],[-247485,211178,256113],0]
]"#,
    )
    .unwrap();
    for (axis, delta, duration, scale, elapsed, mut angles, mut position, expected, report) in rows
    {
        let mut updates = vec![];
        let done = Task::ScaledAxisMovement
            .complete(
                &[
                    Argument::Integer(axis),
                    Argument::Integer(delta),
                    Argument::Integer(duration),
                ],
                Invocation {
                    elapsed: Some(elapsed),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
                &mut None,
                &mut Some(SpatialCall {
                    position: &mut position,
                    angles: &mut angles,
                    frame_scale: scale,
                    turn: 0,
                    nominal_ticks: 0,
                    matrix: Matrix::IDENTITY,
                    slave: false,
                    locations: &[],
                    markers: None,
                    effective_position: None,
                    player_position: None,
                    actor_index: 0,
                    map_updates: &mut updates,
                }),
                NativeServices::default(),
            )
            .unwrap();
        let label = format!(
            "axis {axis} delta {delta} duration {duration} scale {scale} elapsed {elapsed} angles {angles:?}"
        );
        assert_eq!(position, expected, "{label}");
        assert_eq!(!done, report != 0, "{label}");
    }
}

#[test]
fn scaled_axis_movement_and_rotation_match_the_original_routines() {
    // Rows from the original scaled routines in an emulator, starting at position (1000, 2000,
    // 3000) and angles (100, 300, 50): axis, delta or target, duration, frame scale, then per call
    // the position, the angles and the routine's continue flag, with the elapsed input from 0.
    type Step = ([i32; 3], [i32; 3], u32);
    type Row = (i32, i32, i32, u32, Vec<Step>);
    let rows = |text: &str| serde_json::from_str::<Vec<Row>>(text).unwrap();
    let cases = [
        (
            "sMoveByAxis",
            rows(
                r#"[
[0,1024,4,256,[[[1000,2000,3000],[100,300,50],1],[[1256,2000,3000],[100,300,50],1],[[1512,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,1024,4,100,[[[1000,2000,3000],[100,300,50],1],[[1256,2000,3000],[100,300,50],1],[[1512,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1146,2000,3000],[100,300,50],1],[[1292,2000,3000],[100,300,50],1],[[1438,2000,3000],[100,300,50],1],[[1585,2000,3000],[100,300,50],1],[[1731,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1146,2000,3000],[100,300,50],1],[[1292,2000,3000],[100,300,50],1],[[1438,2000,3000],[100,300,50],1],[[1585,2000,3000],[100,300,50],1],[[1731,2000,3000],[100,300,50],1],[[2024,2000,3000],[100,300,50],0]]],
[0,-1536,4,256,[[[1000,2000,3000],[100,300,50],1],[[616,2000,3000],[100,300,50],1],[[232,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[0,-1536,4,100,[[[1000,2000,3000],[100,300,50],1],[[616,2000,3000],[100,300,50],1],[[232,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[0,-1536,7,256,[[[1000,2000,3000],[100,300,50],1],[[780,2000,3000],[100,300,50],1],[[561,2000,3000],[100,300,50],1],[[341,2000,3000],[100,300,50],1],[[122,2000,3000],[100,300,50],1],[[-98,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[0,-1536,7,100,[[[1000,2000,3000],[100,300,50],1],[[780,2000,3000],[100,300,50],1],[[561,2000,3000],[100,300,50],1],[[341,2000,3000],[100,300,50],1],[[122,2000,3000],[100,300,50],1],[[-98,2000,3000],[100,300,50],1],[[-536,2000,3000],[100,300,50],0]]],
[1,1024,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2256,3000],[100,300,50],1],[[1000,2512,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,1024,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2256,3000],[100,300,50],1],[[1000,2512,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2146,3000],[100,300,50],1],[[1000,2292,3000],[100,300,50],1],[[1000,2438,3000],[100,300,50],1],[[1000,2585,3000],[100,300,50],1],[[1000,2731,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2146,3000],[100,300,50],1],[[1000,2292,3000],[100,300,50],1],[[1000,2438,3000],[100,300,50],1],[[1000,2585,3000],[100,300,50],1],[[1000,2731,3000],[100,300,50],1],[[1000,3024,3000],[100,300,50],0]]],
[1,-1536,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,1616,3000],[100,300,50],1],[[1000,1232,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[1,-1536,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,1616,3000],[100,300,50],1],[[1000,1232,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[1,-1536,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,1780,3000],[100,300,50],1],[[1000,1561,3000],[100,300,50],1],[[1000,1341,3000],[100,300,50],1],[[1000,1122,3000],[100,300,50],1],[[1000,902,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[1,-1536,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,1780,3000],[100,300,50],1],[[1000,1561,3000],[100,300,50],1],[[1000,1341,3000],[100,300,50],1],[[1000,1122,3000],[100,300,50],1],[[1000,902,3000],[100,300,50],1],[[1000,464,3000],[100,300,50],0]]],
[2,1024,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3256],[100,300,50],1],[[1000,2000,3512],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,1024,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3256],[100,300,50],1],[[1000,2000,3512],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3146],[100,300,50],1],[[1000,2000,3292],[100,300,50],1],[[1000,2000,3438],[100,300,50],1],[[1000,2000,3585],[100,300,50],1],[[1000,2000,3731],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3146],[100,300,50],1],[[1000,2000,3292],[100,300,50],1],[[1000,2000,3438],[100,300,50],1],[[1000,2000,3585],[100,300,50],1],[[1000,2000,3731],[100,300,50],1],[[1000,2000,4024],[100,300,50],0]]],
[2,-1536,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,2616],[100,300,50],1],[[1000,2000,2232],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]],
[2,-1536,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,2616],[100,300,50],1],[[1000,2000,2232],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]],
[2,-1536,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,2780],[100,300,50],1],[[1000,2000,2561],[100,300,50],1],[[1000,2000,2341],[100,300,50],1],[[1000,2000,2122],[100,300,50],1],[[1000,2000,1902],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]],
[2,-1536,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,2780],[100,300,50],1],[[1000,2000,2561],[100,300,50],1],[[1000,2000,2341],[100,300,50],1],[[1000,2000,2122],[100,300,50],1],[[1000,2000,1902],[100,300,50],1],[[1000,2000,1464],[100,300,50],0]]]
]"#,
            ),
        ),
        (
            "sRotateByAxis",
            rows(
                r#"[
[0,1024,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[356,300,50],1],[[1000,2000,3000],[612,300,50],1],[[1000,2000,3000],[1124,300,50],0]]],
[0,1024,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[356,300,50],1],[[1000,2000,3000],[612,300,50],1],[[1000,2000,3000],[1124,300,50],0]]],
[0,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[246,300,50],1],[[1000,2000,3000],[392,300,50],1],[[1000,2000,3000],[538,300,50],1],[[1000,2000,3000],[685,300,50],1],[[1000,2000,3000],[831,300,50],1],[[1000,2000,3000],[1124,300,50],0]]],
[0,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[246,300,50],1],[[1000,2000,3000],[392,300,50],1],[[1000,2000,3000],[538,300,50],1],[[1000,2000,3000],[685,300,50],1],[[1000,2000,3000],[831,300,50],1],[[1000,2000,3000],[1124,300,50],0]]],
[0,-1536,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[1764,300,50],1],[[1000,2000,3000],[1380,300,50],1],[[1000,2000,3000],[612,300,50],0]]],
[0,-1536,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[1764,300,50],1],[[1000,2000,3000],[1380,300,50],1],[[1000,2000,3000],[612,300,50],0]]],
[0,-1536,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[1929,300,50],1],[[1000,2000,3000],[1710,300,50],1],[[1000,2000,3000],[1490,300,50],1],[[1000,2000,3000],[1271,300,50],1],[[1000,2000,3000],[1051,300,50],1],[[1000,2000,3000],[612,300,50],0]]],
[0,-1536,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[1929,300,50],1],[[1000,2000,3000],[1710,300,50],1],[[1000,2000,3000],[1490,300,50],1],[[1000,2000,3000],[1271,300,50],1],[[1000,2000,3000],[1051,300,50],1],[[1000,2000,3000],[612,300,50],0]]],
[1,1024,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,556,50],1],[[1000,2000,3000],[100,812,50],1],[[1000,2000,3000],[100,1324,50],0]]],
[1,1024,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,556,50],1],[[1000,2000,3000],[100,812,50],1],[[1000,2000,3000],[100,1324,50],0]]],
[1,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,446,50],1],[[1000,2000,3000],[100,592,50],1],[[1000,2000,3000],[100,738,50],1],[[1000,2000,3000],[100,885,50],1],[[1000,2000,3000],[100,1031,50],1],[[1000,2000,3000],[100,1324,50],0]]],
[1,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,446,50],1],[[1000,2000,3000],[100,592,50],1],[[1000,2000,3000],[100,738,50],1],[[1000,2000,3000],[100,885,50],1],[[1000,2000,3000],[100,1031,50],1],[[1000,2000,3000],[100,1324,50],0]]],
[1,-1536,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,1964,50],1],[[1000,2000,3000],[100,1580,50],1],[[1000,2000,3000],[100,812,50],0]]],
[1,-1536,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,1964,50],1],[[1000,2000,3000],[100,1580,50],1],[[1000,2000,3000],[100,812,50],0]]],
[1,-1536,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,81,50],1],[[1000,2000,3000],[100,1910,50],1],[[1000,2000,3000],[100,1690,50],1],[[1000,2000,3000],[100,1471,50],1],[[1000,2000,3000],[100,1251,50],1],[[1000,2000,3000],[100,812,50],0]]],
[1,-1536,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,81,50],1],[[1000,2000,3000],[100,1910,50],1],[[1000,2000,3000],[100,1690,50],1],[[1000,2000,3000],[100,1471,50],1],[[1000,2000,3000],[100,1251,50],1],[[1000,2000,3000],[100,812,50],0]]],
[2,1024,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,306],1],[[1000,2000,3000],[100,300,562],1],[[1000,2000,3000],[100,300,1074],0]]],
[2,1024,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,306],1],[[1000,2000,3000],[100,300,562],1],[[1000,2000,3000],[100,300,1074],0]]],
[2,1024,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,196],1],[[1000,2000,3000],[100,300,342],1],[[1000,2000,3000],[100,300,488],1],[[1000,2000,3000],[100,300,635],1],[[1000,2000,3000],[100,300,781],1],[[1000,2000,3000],[100,300,1074],0]]],
[2,1024,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,196],1],[[1000,2000,3000],[100,300,342],1],[[1000,2000,3000],[100,300,488],1],[[1000,2000,3000],[100,300,635],1],[[1000,2000,3000],[100,300,781],1],[[1000,2000,3000],[100,300,1074],0]]],
[2,-1536,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,1714],1],[[1000,2000,3000],[100,300,1330],1],[[1000,2000,3000],[100,300,562],0]]],
[2,-1536,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,1714],1],[[1000,2000,3000],[100,300,1330],1],[[1000,2000,3000],[100,300,562],0]]],
[2,-1536,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,1879],1],[[1000,2000,3000],[100,300,1660],1],[[1000,2000,3000],[100,300,1440],1],[[1000,2000,3000],[100,300,1221],1],[[1000,2000,3000],[100,300,1001],1],[[1000,2000,3000],[100,300,562],0]]],
[2,-1536,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,1879],1],[[1000,2000,3000],[100,300,1660],1],[[1000,2000,3000],[100,300,1440],1],[[1000,2000,3000],[100,300,1221],1],[[1000,2000,3000],[100,300,1001],1],[[1000,2000,3000],[100,300,562],0]]]
]"#,
            ),
        ),
        (
            "sRotateToAxis",
            rows(
                r#"[
[0,0,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[66,300,50],1],[[1000,2000,3000],[33,300,50],1],[[1000,2000,3000],[0,300,50],0]]],
[0,0,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[66,300,50],1],[[1000,2000,3000],[33,300,50],1],[[1000,2000,3000],[0,300,50],0]]],
[0,0,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[83,300,50],1],[[1000,2000,3000],[66,300,50],1],[[1000,2000,3000],[50,300,50],1],[[1000,2000,3000],[33,300,50],1],[[1000,2000,3000],[16,300,50],1],[[1000,2000,3000],[0,300,50],0]]],
[0,0,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[83,300,50],1],[[1000,2000,3000],[66,300,50],1],[[1000,2000,3000],[50,300,50],1],[[1000,2000,3000],[33,300,50],1],[[1000,2000,3000],[16,300,50],1],[[1000,2000,3000],[0,300,50],0]]],
[0,700,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[300,300,50],1],[[1000,2000,3000],[500,300,50],1],[[1000,2000,3000],[700,300,50],0]]],
[0,700,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[300,300,50],1],[[1000,2000,3000],[500,300,50],1],[[1000,2000,3000],[700,300,50],0]]],
[0,700,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[200,300,50],1],[[1000,2000,3000],[300,300,50],1],[[1000,2000,3000],[400,300,50],1],[[1000,2000,3000],[500,300,50],1],[[1000,2000,3000],[600,300,50],1],[[1000,2000,3000],[700,300,50],0]]],
[0,700,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[200,300,50],1],[[1000,2000,3000],[300,300,50],1],[[1000,2000,3000],[400,300,50],1],[[1000,2000,3000],[500,300,50],1],[[1000,2000,3000],[600,300,50],1],[[1000,2000,3000],[700,300,50],0]]],
[1,0,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,200,50],1],[[1000,2000,3000],[100,100,50],1],[[1000,2000,3000],[100,0,50],0]]],
[1,0,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,200,50],1],[[1000,2000,3000],[100,100,50],1],[[1000,2000,3000],[100,0,50],0]]],
[1,0,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,250,50],1],[[1000,2000,3000],[100,200,50],1],[[1000,2000,3000],[100,150,50],1],[[1000,2000,3000],[100,100,50],1],[[1000,2000,3000],[100,50,50],1],[[1000,2000,3000],[100,0,50],0]]],
[1,0,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,250,50],1],[[1000,2000,3000],[100,200,50],1],[[1000,2000,3000],[100,150,50],1],[[1000,2000,3000],[100,100,50],1],[[1000,2000,3000],[100,50,50],1],[[1000,2000,3000],[100,0,50],0]]],
[1,700,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,433,50],1],[[1000,2000,3000],[100,566,50],1],[[1000,2000,3000],[100,700,50],0]]],
[1,700,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,433,50],1],[[1000,2000,3000],[100,566,50],1],[[1000,2000,3000],[100,700,50],0]]],
[1,700,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,366,50],1],[[1000,2000,3000],[100,433,50],1],[[1000,2000,3000],[100,499,50],1],[[1000,2000,3000],[100,566,50],1],[[1000,2000,3000],[100,633,50],1],[[1000,2000,3000],[100,700,50],0]]],
[1,700,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,366,50],1],[[1000,2000,3000],[100,433,50],1],[[1000,2000,3000],[100,499,50],1],[[1000,2000,3000],[100,566,50],1],[[1000,2000,3000],[100,633,50],1],[[1000,2000,3000],[100,700,50],0]]],
[2,0,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,33],1],[[1000,2000,3000],[100,300,16],1],[[1000,2000,3000],[100,300,0],0]]],
[2,0,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,33],1],[[1000,2000,3000],[100,300,16],1],[[1000,2000,3000],[100,300,0],0]]],
[2,0,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,41],1],[[1000,2000,3000],[100,300,33],1],[[1000,2000,3000],[100,300,25],1],[[1000,2000,3000],[100,300,16],1],[[1000,2000,3000],[100,300,8],1],[[1000,2000,3000],[100,300,0],0]]],
[2,0,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,41],1],[[1000,2000,3000],[100,300,33],1],[[1000,2000,3000],[100,300,25],1],[[1000,2000,3000],[100,300,16],1],[[1000,2000,3000],[100,300,8],1],[[1000,2000,3000],[100,300,0],0]]],
[2,700,4,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,266],1],[[1000,2000,3000],[100,300,483],1],[[1000,2000,3000],[100,300,700],0]]],
[2,700,4,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,266],1],[[1000,2000,3000],[100,300,483],1],[[1000,2000,3000],[100,300,700],0]]],
[2,700,7,256,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,158],1],[[1000,2000,3000],[100,300,266],1],[[1000,2000,3000],[100,300,374],1],[[1000,2000,3000],[100,300,483],1],[[1000,2000,3000],[100,300,591],1],[[1000,2000,3000],[100,300,700],0]]],
[2,700,7,100,[[[1000,2000,3000],[100,300,50],1],[[1000,2000,3000],[100,300,158],1],[[1000,2000,3000],[100,300,266],1],[[1000,2000,3000],[100,300,374],1],[[1000,2000,3000],[100,300,483],1],[[1000,2000,3000],[100,300,591],1],[[1000,2000,3000],[100,300,700],0]]]
]"#,
            ),
        ),
    ];
    for (name, rows) in cases {
        for (axis, delta, duration, scale, steps) in rows {
            let mut task = match name {
                "sMoveByAxis" => Task::ScaledMovement { origin: 0 },
                "sRotateByAxis" => Task::ScaledRelativeRotation { origin: 0 },
                _ => Task::ScaledRotation { origin: 0 },
            };
            let mut position = [1000, 2000, 3000];
            let mut angles = [100, 300, 50];
            let mut updates = vec![];
            for (elapsed, (expected_position, expected_angles, report)) in
                steps.into_iter().enumerate()
            {
                let done = task
                    .complete(
                        &[
                            Argument::Integer(axis),
                            Argument::Integer(delta),
                            Argument::Integer(duration),
                        ],
                        Invocation {
                            elapsed: Some(elapsed as i32),
                            unpaused_tasks: 0,
                            unpaused_blocking_tasks: 0,
                        },
                        &mut None,
                        &mut Some(SpatialCall {
                            position: &mut position,
                            angles: &mut angles,
                            frame_scale: scale,
                            turn: 0,
                            nominal_ticks: 0,
                            matrix: Matrix::IDENTITY,
                            slave: false,
                            locations: &[],
                            markers: None,
                            effective_position: None,
                            player_position: None,
                            actor_index: 0,
                            map_updates: &mut updates,
                        }),
                        NativeServices::default(),
                    )
                    .unwrap();
                let label = format!(
                    "{name} axis {axis} delta {delta} duration {duration} scale {scale} elapsed {elapsed}"
                );
                assert_eq!(position, expected_position, "{label}");
                assert_eq!(angles.map(i32::from), expected_angles, "{label}");
                assert_eq!(!done, report != 0, "{label}");
            }
        }
    }
}

#[test]
fn move_to_location_matches_the_original_routine() {
    // Rows from the original routine in an emulator: start, location, duration, frame scale, then
    // per call the position and the routine's continue flag, with the elapsed input from 0.
    type Step = ([i32; 3], u32);
    type Row = ([i32; 3], [i32; 3], i32, u32, Vec<Step>);
    let rows: Vec<Row> = serde_json::from_str(
        r#"[
[[1000,2000,3000],[1000,2000,3000],2,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[1000,2000,3000],[1000,2000,3000],2,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[1000,2000,3000],[1000,2000,3000],9,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[1000,2000,3000],[1000,2000,3000],9,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[1000,2000,3000],[5000,6000,7000],2,256,[[[2325,3325,4325],1],[[3654,4654,5654],1],[[4992,5992,6992],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[5000,6000,7000],2,100,[[[1517,2517,3517],1],[[2193,3193,4193],1],[[3283,4283,5283],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[5000,6000,7000],9,256,[[[1397,2397,3397],1],[[1794,2794,3794],1],[[2192,3192,4192],1],[[2590,3590,4590],1],[[2989,3989,4989],1],[[3388,4388,5388],1],[[3788,4788,5788],1],[[4189,5189,6189],1],[[4592,5592,6592],1],[[4997,5997,6997],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[5000,6000,7000],9,100,[[[1155,2155,3155],1],[[1320,2320,3320],1],[[1498,2498,3498],1],[[1692,2692,3692],1],[[1906,2906,3906],1],[[2146,3146,4146],1],[[2422,3422,4422],1],[[2755,3755,4755],1],[[3190,4190,5190],1],[[3892,4892,5892],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[-2560,0,256],2,256,[[[-192,1327,2083],1],[[-1381,662,1169],1],[[-2561,-1,251],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[-2560,0,256],2,100,[[[535,1737,2642],1],[[-72,1397,2175],1],[[-1048,846,1424],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[-2560,0,256],9,256,[[[642,1798,2725],1],[[285,1596,2450],1],[[-72,1396,2175],1],[[-429,1196,1900],1],[[-786,996,1626],1],[[-1143,796,1352],1],[[-1500,596,1076],1],[[-1856,395,800],1],[[-2210,197,526],1],[[-2562,-2,255],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[-2560,0,256],9,100,[[[860,1921,2892],1],[[711,1836,2777],1],[[551,1746,2654],1],[[377,1648,2520],1],[[185,1540,2372],1],[[-31,1419,2206],1],[[-279,1280,2015],1],[[-577,1113,1785],1],[[-966,895,1486],1],[[-1591,545,1005],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[100000,-20000,65536],2,256,[[[33888,-5447,23788],1],[[66819,-12748,44603],1],[[99858,-20073,65486],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[100000,-20000,65536],2,100,[[[13846,-909,11120],1],[[30613,-4706,21718],1],[[57616,-10693,38786],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[100000,-20000,65536],9,256,[[[10866,-234,9236],1],[[20735,-2469,15474],1],[[30607,-4705,21713],1],[[40483,-6895,27955],1],[[50365,-9086,34201],1],[[60253,-11279,40451],1],[[70149,-13473,46706],1],[[80056,-15670,52968],1],[[90027,-17871,59240],1],[[99998,-20025,65513],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[100000,-20000,65536],9,100,[[[4853,1127,5435],1],[[8967,195,8035],1],[[13395,-808,10834],1],[[18210,-1899,13877],1],[[23515,-3101,17230],1],[[29468,-4449,20992],1],[[36329,-5971,25329],1],[[44587,-7802,30549],1],[[55368,-10193,37363],1],[[72732,-14043,48338],1],[[100000,-20000,65536],0]]],
[[0,0,0],[1000,2000,3000],2,256,[[[331,662,998],1],[[662,1330,1998],1],[[999,2000,2995],1],[[1000,2000,3000],0]]],
[[0,0,0],[1000,2000,3000],2,100,[[[129,258,389],1],[[297,595,897],1],[[569,1143,1717],1],[[1000,2000,3000],0]]],
[[0,0,0],[1000,2000,3000],9,256,[[[99,198,299],1],[[198,396,598],1],[[297,594,897],1],[[396,794,1197],1],[[495,994,1495],1],[[594,1194,1795],1],[[695,1395,2094],1],[[796,1595,2395],1],[[897,1797,2696],1],[[999,1998,2998],1],[[1000,2000,3000],0]]],
[[0,0,0],[1000,2000,3000],9,100,[[[38,77,116],1],[[79,159,240],1],[[123,247,373],1],[[171,343,518],1],[[224,449,678],1],[[284,569,858],1],[[353,708,1066],1],[[436,876,1317],1],[[544,1093,1645],1],[[719,1446,2171],1],[[1000,2000,3000],0]]],
[[0,0,0],[5000,6000,7000],2,256,[[[1666,1993,2321],1],[[3321,3990,4660],1],[[4994,5992,6992],1],[[5000,6000,7000],0]]],
[[0,0,0],[5000,6000,7000],2,100,[[[650,778,906],1],[[1492,1794,2089],1],[[2861,3432,3996],1],[[5000,6000,7000],0]]],
[[0,0,0],[5000,6000,7000],9,256,[[[499,597,695],1],[[998,1195,1391],1],[[1494,1793,2088],1],[[1994,2392,2785],1],[[2490,2991,3482],1],[[2991,3591,4185],1],[[3492,4191,4884],1],[[3990,4792,5588],1],[[4493,5394,6289],1],[[4999,5996,6999],1],[[5000,6000,7000],0]]],
[[0,0,0],[5000,6000,7000],9,100,[[[194,233,271],1],[[402,482,561],1],[[625,750,873],1],[[869,1042,1213],1],[[1137,1363,1587],1],[[1436,1724,2007],1],[[1783,2140,2491],1],[[2197,2640,3074],1],[[2744,3294,3836],1],[[3617,4348,5071],1],[[5000,6000,7000],0]]],
[[0,0,0],[-2560,0,256],2,256,[[[-854,0,83],1],[[-1708,0,166],1],[[-2562,0,253],1],[[-2560,0,256],0]]],
[[0,0,0],[-2560,0,256],2,100,[[[-333,0,32],1],[[-769,0,74],1],[[-1470,0,142],1],[[-2560,0,256],0]]],
[[0,0,0],[-2560,0,256],9,256,[[[-256,0,25],1],[[-512,0,50],1],[[-768,0,75],1],[[-1024,0,100],1],[[-1280,0,125],1],[[-1536,0,151],1],[[-1792,0,177],1],[[-2048,0,203],1],[[-2304,0,229],1],[[-2560,0,255],1],[[-2560,0,256],0]]],
[[0,0,0],[-2560,0,256],9,100,[[[-100,0,9],1],[[-207,0,19],1],[[-322,0,30],1],[[-447,0,42],1],[[-585,0,55],1],[[-740,0,70],1],[[-918,0,88],1],[[-1132,0,109],1],[[-1411,0,137],1],[[-1861,0,182],1],[[-2560,0,256],0]]],
[[0,0,0],[100000,-20000,65536],2,256,[[[33304,-6788,21781],1],[[66624,-13421,43573],1],[[99998,-20065,65400],1],[[100000,-20000,65536],0]]],
[[0,0,0],[100000,-20000,65536],2,100,[[[13008,-2652,8508],1],[[29983,-6112,19610],1],[[57309,-11552,37482],1],[[100000,-20000,65536],0]]],
[[0,0,0],[100000,-20000,65536],9,256,[[[9991,-2037,6534],1],[[19983,-4074,13069],1],[[29976,-6111,19605],1],[[39970,-8101,26141],1],[[49967,-10092,32679],1],[[59968,-12083,39219],1],[[69973,-14075,45762],1],[[79936,-16068,52309],1],[[89923,-18066,58919],1],[[99985,-20022,65500],1],[[100000,-20000,65536],0]]],
[[0,0,0],[100000,-20000,65536],9,100,[[[3902,-796,2552],1],[[8068,-1646,5276],1],[[12552,-2560,8209],1],[[17427,-3554,11397],1],[[22797,-4649,14909],1],[[28822,-5849,18850],1],[[35766,-7232,23392],1],[[44123,-8896,28858],1],[[55029,-11068,35991],1],[[72589,-14564,47476],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[1000,2000,3000],2,256,[[[-3006,1122,9229],1],[[-1011,1558,6111],1],[[985,1995,2992],1],[[1000,2000,3000],0]]],
[[-5000,700,12345],[1000,2000,3000],2,100,[[[-4222,864,11128],1],[[-3206,1079,9540],1],[[-1570,1437,6983],1],[[1000,2000,3000],0]]],
[[-5000,700,12345],[1000,2000,3000],9,256,[[[-4402,826,11410],1],[[-3804,952,10475],1],[[-3206,1078,9540],1],[[-2608,1209,8605],1],[[-2010,1340,7670],1],[[-1412,1471,6734],1],[[-814,1602,5798],1],[[-211,1733,4862],1],[[391,1864,3927],1],[[997,1999,2997],1],[[1000,2000,3000],0]]],
[[-5000,700,12345],[1000,2000,3000],9,100,[[[-4767,749,11980],1],[[-4518,801,11590],1],[[-4250,857,11171],1],[[-3959,918,10715],1],[[-3638,986,10212],1],[[-3278,1064,9648],1],[[-2863,1155,8998],1],[[-2363,1264,8216],1],[[-1711,1406,7196],1],[[-660,1636,5553],1],[[1000,2000,3000],0]]],
[[-5000,700,12345],[5000,6000,7000],2,256,[[[-1676,2460,10552],1],[[1650,4221,8774],1],[[4986,5987,6991],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[5000,6000,7000],2,100,[[[-3702,1387,11645],1],[[-2008,2283,10731],1],[[720,3727,9273],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[5000,6000,7000],9,256,[[[-4004,1227,11807],1],[[-3007,1755,11269],1],[[-2010,2283,10731],1],[[-1013,2811,10197],1],[[-15,3339,9663],1],[[984,3868,9129],1],[[1984,4397,8594],1],[[2984,4926,8059],1],[[3987,5462,7527],1],[[4997,5999,6999],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[5000,6000,7000],9,100,[[[-4612,905,12135],1],[[-4197,1124,11911],1],[[-3750,1360,11669],1],[[-3264,1617,11406],1],[[-2728,1900,11116],1],[[-2127,2218,10794],1],[[-1434,2585,10423],1],[[-600,3026,9977],1],[[489,3602,9394],1],[[2244,4531,8456],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[-2560,0,256],2,256,[[[-4196,458,8308],1],[[-3393,216,4274],1],[[-2575,-9,245],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[-2560,0,256],2,100,[[[-4686,605,10768],1],[[-4277,482,8710],1],[[-3618,284,5400],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[-2560,0,256],9,256,[[[-4759,627,11134],1],[[-4518,554,9923],1],[[-4277,481,8713],1],[[-4036,408,7503],1],[[-3791,335,6293],1],[[-3546,267,5083],1],[[-3301,199,3874],1],[[-3056,131,2666],1],[[-2811,63,1458],1],[[-2562,-5,252],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[-2560,0,256],9,100,[[[-4906,671,11872],1],[[-4806,640,11368],1],[[-4698,607,10824],1],[[-4581,571,10233],1],[[-4452,532,9582],1],[[-4307,488,8852],1],[[-4140,437,8011],1],[[-3935,376,6999],1],[[-3667,302,5679],1],[[-3236,183,3556],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[100000,-20000,65536],2,256,[[[29857,-6303,29929],1],[[64789,-13165,47707],1],[[99993,-20050,65387],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[100000,-20000,65536],2,100,[[[8615,-2036,19213],1],[[26391,-5608,28259],1],[[55018,-11232,42700],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[100000,-20000,65536],9,256,[[[5457,-1401,17620],1],[[15918,-3503,22897],1],[[26385,-5606,28224],1],[[36857,-7663,33553],1],[[47334,-9722,38838],1],[[57821,-11783,44175],1],[[68365,-13845,49470],1],[[78867,-15909,54815],1],[[89387,-17976,60168],1],[[99990,-20003,65493],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[100000,-20000,65536],9,100,[[[-916,-121,14405],1],[[3445,-998,16605],1],[[8140,-1942,18973],1],[[13244,-2968,21570],1],[[18868,-4098,24407],1],[[25180,-5367,27619],1],[[32454,-6796,31321],1],[[41210,-8516,35738],1],[[52642,-10762,41556],1],[[71140,-14380,50846],1],[[100000,-20000,65536],0]]]
]"#,
    )
    .unwrap();
    for (start, location, duration, scale, steps) in rows {
        let mut task = Task::Location;
        let mut position = start;
        let mut angles = [0; 3];
        let mut updates = vec![];
        for (elapsed, (expected, report)) in steps.into_iter().enumerate() {
            let done = task
                .complete(
                    &[Argument::Integer(0), Argument::Integer(duration)],
                    Invocation {
                        elapsed: Some(elapsed as i32),
                        unpaused_tasks: 0,
                        unpaused_blocking_tasks: 0,
                    },
                    &mut None,
                    &mut Some(SpatialCall {
                        position: &mut position,
                        angles: &mut angles,
                        frame_scale: scale,
                        turn: 0,
                        nominal_ticks: 0,
                        matrix: Matrix::IDENTITY,
                        slave: false,
                        locations: &[location],
                        markers: None,
                        effective_position: None,
                        player_position: None,
                        actor_index: 0,
                        map_updates: &mut updates,
                    }),
                    NativeServices::default(),
                )
                .unwrap();
            let label = format!(
                "start {start:?} location {location:?} duration {duration} scale {scale} elapsed {elapsed}"
            );
            assert_eq!(position, expected, "{label}");
            assert_eq!(!done, report != 0, "{label}");
        }
    }
}

#[test]
fn wander_to_location_matches_the_original_routine() {
    // Rows from the original routine in an emulator: start, location, duration, frame scale, then
    // per call the position and the routine's continue flag. The routine keeps its speed in the
    // native command words, so the task state carries over between calls.
    type Step = ([i32; 3], u32);
    type Row = ([i32; 3], [i32; 3], i32, u32, Vec<Step>);
    let rows: Vec<Row> = serde_json::from_str(
        r#"[
[[1000,2000,3000],[5000,6000,7000],2,256,[[[2989,3989,4989],1],[[4978,5978,6978],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[5000,6000,7000],2,100,[[[1776,2776,3776],1],[[2552,3552,4552],1],[[3328,4328,5328],1],[[4104,5104,6104],1],[[4880,5880,6880],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[5000,6000,7000],9,256,[[[1441,2441,3441],1],[[1882,2882,3882],1],[[2323,3323,4323],1],[[2764,3764,4764],1],[[3205,4205,5205],1],[[3646,4646,5646],1],[[4087,5087,6087],1],[[4528,5528,6528],1],[[4969,5969,6969],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[5000,6000,7000],9,100,[[[1172,2172,3172],1],[[1344,2344,3344],1],[[1516,2516,3516],1],[[1688,2688,3688],1],[[1860,2860,3860],1],[[2032,3032,4032],1],[[2204,3204,4204],1],[[2376,3376,4376],1],[[2548,3548,4548],1],[[2720,3720,4720],1],[[2892,3892,4892],1],[[3064,4064,5064],1],[[3236,4236,5236],1],[[3408,4408,5408],1],[[3580,4580,5580],1],[[3752,4752,5752],1],[[3924,4924,5924],1],[[4096,5096,6096],1],[[4268,5268,6268],1],[[4440,5440,6440],1],[[4612,5612,6612],1],[[4784,5784,6784],1],[[4956,5956,6956],1],[[5000,6000,7000],0]]],
[[1000,2000,3000],[-2560,0,256],2,256,[[[-788,991,1625],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[-2560,0,256],2,100,[[[302,1606,2463],1],[[-396,1216,1926],1],[[-1094,822,1389],1],[[-1792,432,849],1],[[-2490,38,312],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[-2560,0,256],9,256,[[[603,1776,2695],1],[[206,1552,2390],1],[[-191,1330,2085],1],[[-588,1108,1780],1],[[-985,886,1472],1],[[-1382,662,1167],1],[[-1779,440,859],1],[[-2176,216,554],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[-2560,0,256],9,100,[[[845,1912,2881],1],[[690,1824,2762],1],[[535,1737,2643],1],[[380,1650,2524],1],[[225,1563,2405],1],[[70,1476,2286],1],[[-85,1389,2167],1],[[-240,1302,2048],1],[[-395,1215,1928],1],[[-550,1128,1808],1],[[-705,1041,1689],1],[[-860,954,1569],1],[[-1015,867,1449],1],[[-1170,780,1330],1],[[-1325,693,1210],1],[[-1480,606,1090],1],[[-1635,519,971],1],[[-1790,432,851],1],[[-1945,345,731],1],[[-2100,258,611],1],[[-2255,171,492],1],[[-2410,84,372],1],[[-2560,0,256],0]]],
[[1000,2000,3000],[100000,-20000,65536],2,256,[[[50333,-9170,34182],1],[[99666,-20108,65364],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[100000,-20000,65536],2,100,[[[20270,-2364,15180],1],[[39540,-6728,27360],1],[[58810,-11001,39540],1],[[78080,-15274,51720],1],[[97441,-19456,63900],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[100000,-20000,65536],9,256,[[[11962,-483,9929],1],[[22924,-2966,16858],1],[[33886,-5449,23787],1],[[44848,-7880,30716],1],[[55810,-10311,37645],1],[[66772,-12742,44574],1],[[77734,-15173,51503],1],[[88748,-17604,58432],1],[[99762,-19983,65361],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[100000,-20000,65536],9,100,[[[5282,1030,5706],1],[[9564,60,8412],1],[[13846,-910,11118],1],[[18128,-1880,13824],1],[[22410,-2850,16530],1],[[26692,-3820,19236],1],[[30974,-4770,21942],1],[[35256,-5720,24648],1],[[39538,-6670,27354],1],[[43820,-7620,30060],1],[[48102,-8570,32766],1],[[52384,-9520,35472],1],[[56666,-10470,38178],1],[[60948,-11420,40884],1],[[65230,-12370,43590],1],[[69512,-13320,46296],1],[[73794,-14270,49002],1],[[78076,-15220,51708],1],[[82358,-16170,54414],1],[[86640,-17120,57120],1],[[90942,-18050,59826],1],[[95244,-18980,62532],1],[[99526,-19910,65238],1],[[100000,-20000,65536],0]]],
[[1000,2000,3000],[1100,2000,3000],2,256,[[[1050,2000,3000],1],[[1100,2000,3000],0]]],
[[1000,2000,3000],[1100,2000,3000],2,100,[[[1019,2000,3000],1],[[1038,2000,3000],1],[[1057,2000,3000],1],[[1076,2000,3000],1],[[1095,2000,3000],1],[[1100,2000,3000],0]]],
[[1000,2000,3000],[1100,2000,3000],9,256,[[[1011,2000,3000],1],[[1022,2000,3000],1],[[1033,2000,3000],1],[[1044,2000,3000],1],[[1055,2000,3000],1],[[1066,2000,3000],1],[[1077,2000,3000],1],[[1088,2000,3000],1],[[1099,2000,3000],1],[[1100,2000,3000],0]]],
[[1000,2000,3000],[1100,2000,3000],9,100,[[[1004,2000,3000],1],[[1008,2000,3000],1],[[1012,2000,3000],1],[[1016,2000,3000],1],[[1020,2000,3000],1],[[1024,2000,3000],1],[[1028,2000,3000],1],[[1032,2000,3000],1],[[1036,2000,3000],1],[[1040,2000,3000],1],[[1044,2000,3000],1],[[1048,2000,3000],1],[[1052,2000,3000],1],[[1056,2000,3000],1],[[1060,2000,3000],1],[[1064,2000,3000],1],[[1068,2000,3000],1],[[1072,2000,3000],1],[[1076,2000,3000],1],[[1080,2000,3000],1],[[1084,2000,3000],1],[[1088,2000,3000],1],[[1092,2000,3000],1],[[1096,2000,3000],1],[[1100,2000,3000],0]]],
[[0,0,0],[5000,6000,7000],2,256,[[[2499,2990,3482],1],[[4977,5980,6984],1],[[5000,6000,7000],0]]],
[[0,0,0],[5000,6000,7000],2,100,[[[976,1168,1360],1],[[1944,2336,2720],1],[[2920,3504,4080],1],[[3888,4672,5448],1],[[4864,5840,6808],1],[[5000,6000,7000],0]]],
[[0,0,0],[5000,6000,7000],9,256,[[[555,664,773],1],[[1105,1328,1546],1],[[1660,1992,2319],1],[[2210,2656,3092],1],[[2765,3320,3865],1],[[3315,3984,4643],1],[[3870,4648,5416],1],[[4425,5312,6194],1],[[4980,5976,6972],1],[[5000,6000,7000],0]]],
[[0,0,0],[5000,6000,7000],9,100,[[[216,259,302],1],[[432,518,604],1],[[648,777,906],1],[[864,1036,1208],1],[[1080,1295,1510],1],[[1296,1554,1812],1],[[1512,1813,2114],1],[[1728,2072,2416],1],[[1943,2331,2718],1],[[2159,2590,3020],1],[[2375,2849,3322],1],[[2590,3108,3624],1],[[2806,3367,3926],1],[[3021,3626,4228],1],[[3237,3885,4530],1],[[3453,4144,4832],1],[[3668,4403,5134],1],[[3884,4662,5436],1],[[4100,4921,5739],1],[[4316,5180,6041],1],[[4532,5439,6344],1],[[4748,5698,6647],1],[[4964,5957,6949],1],[[5000,6000,7000],0]]],
[[0,0,0],[-2560,0,256],2,256,[[[-1281,0,125],1],[[-2560,0,256],0]]],
[[0,0,0],[-2560,0,256],2,100,[[[-501,0,49],1],[[-1002,0,98],1],[[-1503,0,147],1],[[-2004,0,197],1],[[-2505,0,249],1],[[-2560,0,256],0]]],
[[0,0,0],[-2560,0,256],9,256,[[[-284,0,27],1],[[-568,0,54],1],[[-852,0,81],1],[[-1136,0,109],1],[[-1420,0,137],1],[[-1704,0,165],1],[[-1988,0,195],1],[[-2272,0,225],1],[[-2556,0,255],1],[[-2560,0,256],0]]],
[[0,0,0],[-2560,0,256],9,100,[[[-111,0,10],1],[[-222,0,20],1],[[-333,0,30],1],[[-444,0,40],1],[[-555,0,50],1],[[-666,0,61],1],[[-777,0,72],1],[[-888,0,83],1],[[-999,0,94],1],[[-1110,0,105],1],[[-1221,0,116],1],[[-1332,0,127],1],[[-1443,0,138],1],[[-1554,0,149],1],[[-1665,0,160],1],[[-1776,0,171],1],[[-1887,0,182],1],[[-1998,0,193],1],[[-2109,0,205],1],[[-2220,0,217],1],[[-2331,0,229],1],[[-2442,0,241],1],[[-2553,0,254],1],[[-2560,0,256],0]]],
[[0,0,0],[100000,-20000,65536],2,256,[[[49956,-10181,32673],1],[[99912,-20125,65346],1],[[100000,-20000,65536],0]]],
[[0,0,0],[100000,-20000,65536],2,100,[[[19514,-3977,12762],1],[[39028,-7954,25524],1],[[58542,-11839,38286],1],[[78056,-15724,51048],1],[[97477,-19609,63903],1],[[100000,-20000,65536],0]]],
[[0,0,0],[100000,-20000,65536],9,256,[[[11101,-2263,7260],1],[[22202,-4526,14520],1],[[33303,-6789,21780],1],[[44404,-8999,29040],1],[[55505,-11209,36300],1],[[66606,-13419,43560],1],[[77707,-15629,50820],1],[[88755,-17839,58133],1],[[99856,-19997,65393],1],[[100000,-20000,65536],0]]],
[[0,0,0],[100000,-20000,65536],9,100,[[[4336,-884,2836],1],[[8672,-1768,5672],1],[[13008,-2652,8508],1],[[17344,-3536,11344],1],[[21680,-4420,14180],1],[[26016,-5304,17016],1],[[30352,-6168,19852],1],[[34688,-7032,22688],1],[[39024,-7896,25524],1],[[43360,-8760,28360],1],[[47696,-9624,31196],1],[[52032,-10488,34032],1],[[56368,-11352,36868],1],[[60704,-12216,39704],1],[[65040,-13080,42540],1],[[69376,-13944,45376],1],[[73691,-14808,48212],1],[[78006,-15672,51048],1],[[82321,-16536,53884],1],[[86636,-17400,56740],1],[[90972,-18264,59576],1],[[95287,-19107,62432],1],[[99623,-19950,65288],1],[[100000,-20000,65536],0]]],
[[0,0,0],[1100,2000,3000],2,256,[[[544,994,1494],1],[[1096,1988,2988],1],[[1100,2000,3000],0]]],
[[0,0,0],[1100,2000,3000],2,100,[[[212,388,583],1],[[424,776,1166],1],[[639,1164,1749],1],[[851,1555,2332],1],[[1066,1943,2915],1],[[1100,2000,3000],0]]],
[[0,0,0],[1100,2000,3000],9,256,[[[120,220,331],1],[[240,440,662],1],[[360,660,993],1],[[482,880,1324],1],[[604,1100,1655],1],[[726,1322,1986],1],[[848,1544,2317],1],[[970,1764,2648],1],[[1092,1986,2979],1],[[1100,2000,3000],0]]],
[[0,0,0],[1100,2000,3000],9,100,[[[47,85,129],1],[[94,170,258],1],[[141,255,387],1],[[188,340,516],1],[[235,426,645],1],[[282,512,774],1],[[329,598,903],1],[[376,684,1032],1],[[423,770,1161],1],[[470,856,1290],1],[[517,942,1419],1],[[564,1028,1548],1],[[611,1114,1677],1],[[658,1200,1806],1],[[705,1286,1935],1],[[752,1372,2064],1],[[799,1458,2193],1],[[846,1544,2322],1],[[893,1630,2450],1],[[941,1716,2578],1],[[989,1802,2706],1],[[1037,1888,2834],1],[[1085,1974,2962],1],[[1100,2000,3000],0]]],
[[-5000,700,12345],[5000,6000,7000],2,256,[[[-14,3340,9656],1],[[4972,5980,6991],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[5000,6000,7000],2,100,[[[-3053,1731,11294],1],[[-1106,2762,10243],1],[[841,3793,9202],1],[[2788,4824,8161],1],[[4745,5864,7129],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[5000,6000,7000],9,256,[[[-3893,1286,11747],1],[[-2786,1872,11149],1],[[-1679,2458,10551],1],[[-572,3044,9959],1],[[535,3630,9367],1],[[1642,4216,8775],1],[[2749,4802,8183],1],[[3862,5393,7596],1],[[4975,5984,7009],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[5000,6000,7000],9,100,[[[-4569,928,12112],1],[[-4138,1156,11879],1],[[-3707,1384,11646],1],[[-3276,1612,11413],1],[[-2845,1840,11180],1],[[-2414,2068,10947],1],[[-1983,2296,10716],1],[[-1552,2524,10485],1],[[-1121,2752,10254],1],[[-690,2980,10023],1],[[-259,3208,9792],1],[[172,3436,9561],1],[[603,3664,9330],1],[[1034,3892,9099],1],[[1465,4120,8868],1],[[1896,4348,8637],1],[[2327,4578,8406],1],[[2758,4808,8177],1],[[3192,5038,7948],1],[[3626,5268,7719],1],[[4060,5498,7490],1],[[4494,5728,7263],1],[[4928,5960,7036],1],[[5000,6000,7000],0]]],
[[-5000,700,12345],[-2560,0,256],2,256,[[[-3794,338,6289],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[-2560,0,256],2,100,[[[-4529,558,9980],1],[[-4058,416,7615],1],[[-3587,274,5250],1],[[-3107,142,2885],1],[[-2618,10,520],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[-2560,0,256],9,256,[[[-4733,619,10999],1],[[-4466,538,9653],1],[[-4199,457,8307],1],[[-3932,376,6961],1],[[-3659,295,5615],1],[[-3386,219,4269],1],[[-3113,143,2923],1],[[-2840,67,1577],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[-2560,0,256],9,100,[[[-4896,668,11820],1],[[-4792,636,11295],1],[[-4688,604,10770],1],[[-4584,572,10245],1],[[-4480,540,9720],1],[[-4376,508,9195],1],[[-4272,476,8670],1],[[-4168,444,8145],1],[[-4062,412,7620],1],[[-3956,380,7095],1],[[-3850,350,6570],1],[[-3744,320,6045],1],[[-3638,290,5520],1],[[-3532,260,4995],1],[[-3426,230,4470],1],[[-3320,200,3945],1],[[-3214,170,3420],1],[[-3108,140,2895],1],[[-3002,110,2370],1],[[-2894,80,1845],1],[[-2786,52,1320],1],[[-2676,24,795],1],[[-2566,-2,270],1],[[-2560,0,256],0]]],
[[-5000,700,12345],[100000,-20000,65536],2,256,[[[47285,-9804,38721],1],[[99570,-20075,65330],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[100000,-20000,65536],2,100,[[[15423,-3403,22647],1],[[35846,-7506,33041],1],[[56360,-11518,43343],1],[[76783,-15530,53737],1],[[97206,-19542,64131],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[100000,-20000,65536],9,256,[[[6619,-1635,18206],1],[[18238,-3970,24067],1],[[29857,-6305,29980],1],[[41476,-8588,35893],1],[[53095,-10871,41754],1],[[64714,-13154,47667],1],[[76384,-15437,53528],1],[[88003,-17720,59441],1],[[99673,-19951,65354],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[100000,-20000,65536],9,100,[[[-462,-212,14634],1],[[4076,-1124,16923],1],[[8614,-2036,19212],1],[[13152,-2948,21521],1],[[17690,-3860,23810],1],[[22228,-4772,26119],1],[[26766,-5684,28428],1],[[31304,-6576,30717],1],[[35842,-7468,33026],1],[[40380,-8360,35315],1],[[44918,-9252,37624],1],[[49456,-10144,39933],1],[[53994,-11036,42222],1],[[58532,-11928,44531],1],[[63090,-12820,46820],1],[[67628,-13712,49129],1],[[72166,-14604,51438],1],[[76724,-15496,53727],1],[[81262,-16388,56036],1],[[85820,-17280,58345],1],[[90378,-18172,60654],1],[[94936,-19044,62963],1],[[99494,-19916,65272],1],[[100000,-20000,65536],0]]],
[[-5000,700,12345],[1100,2000,3000],2,256,[[[-1973,1336,7671],1],[[1076,1994,2997],1],[[1100,2000,3000],0]]],
[[-5000,700,12345],[1100,2000,3000],2,100,[[[-3818,948,10519],1],[[-2627,1196,8693],1],[[-1436,1453,6867],1],[[-245,1710,5041],1],[[946,1967,3215],1],[[1100,2000,3000],0]]],
[[-5000,700,12345],[1100,2000,3000],9,256,[[[-4328,841,11306],1],[[-3651,982,10267],1],[[-2974,1123,9228],1],[[-2297,1264,8189],1],[[-1620,1410,7150],1],[[-943,1556,6111],1],[[-266,1702,5072],1],[[411,1848,4033],1],[[1098,1999,2999],1],[[1100,2000,3000],0]]],
[[-5000,700,12345],[1100,2000,3000],9,100,[[[-4738,755,11939],1],[[-4474,810,11533],1],[[-4210,865,11127],1],[[-3946,920,10721],1],[[-3682,975,10315],1],[[-3418,1030,9909],1],[[-3154,1085,9503],1],[[-2890,1140,9097],1],[[-2626,1197,8691],1],[[-2362,1254,8285],1],[[-2098,1311,7879],1],[[-1834,1368,7473],1],[[-1570,1425,7067],1],[[-1306,1482,6661],1],[[-1042,1539,6255],1],[[-778,1596,5849],1],[[-514,1653,5443],1],[[-248,1710,5037],1],[[18,1767,4633],1],[[284,1824,4229],1],[[550,1881,3825],1],[[816,1938,3421],1],[[1086,1996,3019],1],[[1100,2000,3000],0]]]
]"#,
    )
    .unwrap();
    for (start, location, duration, scale, steps) in rows {
        let mut task = Task::WanderLocation {
            started: false,
            speed: 0,
        };
        let mut position = start;
        let mut angles = [0; 3];
        let mut updates = vec![];
        for (call, (expected, report)) in steps.into_iter().enumerate() {
            let done = task
                .complete(
                    &[Argument::Integer(0), Argument::Integer(duration)],
                    Invocation {
                        elapsed: Some(0),
                        unpaused_tasks: 0,
                        unpaused_blocking_tasks: 0,
                    },
                    &mut None,
                    &mut Some(SpatialCall {
                        position: &mut position,
                        angles: &mut angles,
                        frame_scale: scale,
                        turn: 0,
                        nominal_ticks: 0,
                        matrix: Matrix::IDENTITY,
                        slave: false,
                        locations: &[location],
                        markers: None,
                        effective_position: None,
                        player_position: None,
                        actor_index: 0,
                        map_updates: &mut updates,
                    }),
                    NativeServices::default(),
                )
                .unwrap();
            let label = format!(
                "start {start:?} location {location:?} duration {duration} scale {scale} call {call}"
            );
            assert_eq!(position, expected, "{label}");
            assert_eq!(!done, report != 0, "{label}");
        }
    }
}

#[test]
fn move_object_axis_matches_the_original_routine() {
    // Rows from the original routine in an emulator, starting at position (1000, 2000, 3000):
    // angles, axis, delta, duration, frame scale, then per call the position and the routine's
    // continue flag, with the elapsed input from 0.
    type Step = ([i32; 3], u32);
    type Row = ([u16; 3], i32, i32, i32, u32, Vec<Step>);
    let rows: Vec<Row> = serde_json::from_str(
        r#"[
[[0,0,0],0,1024,3,256,[[[1256,2000,3000],1],[[1512,2000,3000],1],[[1768,2000,3000],0]]],
[[0,0,0],0,1024,3,100,[[[1100,2000,3000],1],[[1200,2000,3000],1],[[1300,2000,3000],0]]],
[[0,0,0],0,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,0,0],0,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,0,0],0,-1536,3,256,[[[488,2000,3000],1],[[-24,2000,3000],1],[[-24,2000,3000],0]]],
[[0,0,0],0,-1536,3,100,[[[800,2000,3000],1],[[600,2000,3000],1],[[400,2000,3000],0]]],
[[0,0,0],0,-1536,6,256,[[[744,2000,3000],1],[[488,2000,3000],1],[[232,2000,3000],1],[[-24,2000,3000],1],[[-280,2000,3000],1],[[-280,2000,3000],0]]],
[[0,0,0],0,-1536,6,100,[[[900,2000,3000],1],[[800,2000,3000],1],[[700,2000,3000],1],[[600,2000,3000],1],[[500,2000,3000],1],[[400,2000,3000],0]]],
[[0,0,0],0,4096,3,256,[[[2280,2000,3000],1],[[3560,2000,3000],1],[[4840,2000,3000],0]]],
[[0,0,0],0,4096,3,100,[[[1500,2000,3000],1],[[2000,2000,3000],1],[[2500,2000,3000],0]]],
[[0,0,0],0,4096,6,256,[[[1512,2000,3000],1],[[2024,2000,3000],1],[[2536,2000,3000],1],[[3048,2000,3000],1],[[3560,2000,3000],1],[[4072,2000,3000],0]]],
[[0,0,0],0,4096,6,100,[[[1200,2000,3000],1],[[1400,2000,3000],1],[[1600,2000,3000],1],[[1800,2000,3000],1],[[2000,2000,3000],1],[[2200,2000,3000],0]]],
[[0,0,0],1,1024,3,256,[[[1000,2256,3000],1],[[1000,2512,3000],1],[[1000,2768,3000],0]]],
[[0,0,0],1,1024,3,100,[[[1000,2100,3000],1],[[1000,2200,3000],1],[[1000,2300,3000],0]]],
[[0,0,0],1,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,0,0],1,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,0,0],1,-1536,3,256,[[[1000,1488,3000],1],[[1000,976,3000],1],[[1000,976,3000],0]]],
[[0,0,0],1,-1536,3,100,[[[1000,1800,3000],1],[[1000,1600,3000],1],[[1000,1400,3000],0]]],
[[0,0,0],1,-1536,6,256,[[[1000,1744,3000],1],[[1000,1488,3000],1],[[1000,1232,3000],1],[[1000,976,3000],1],[[1000,720,3000],1],[[1000,720,3000],0]]],
[[0,0,0],1,-1536,6,100,[[[1000,1900,3000],1],[[1000,1800,3000],1],[[1000,1700,3000],1],[[1000,1600,3000],1],[[1000,1500,3000],1],[[1000,1400,3000],0]]],
[[0,0,0],1,4096,3,256,[[[1000,3280,3000],1],[[1000,4560,3000],1],[[1000,5840,3000],0]]],
[[0,0,0],1,4096,3,100,[[[1000,2500,3000],1],[[1000,3000,3000],1],[[1000,3500,3000],0]]],
[[0,0,0],1,4096,6,256,[[[1000,2512,3000],1],[[1000,3024,3000],1],[[1000,3536,3000],1],[[1000,4048,3000],1],[[1000,4560,3000],1],[[1000,5072,3000],0]]],
[[0,0,0],1,4096,6,100,[[[1000,2200,3000],1],[[1000,2400,3000],1],[[1000,2600,3000],1],[[1000,2800,3000],1],[[1000,3000,3000],1],[[1000,3200,3000],0]]],
[[0,0,0],2,1024,3,256,[[[1000,2000,3256],1],[[1000,2000,3512],1],[[1000,2000,3768],0]]],
[[0,0,0],2,1024,3,100,[[[1000,2000,3100],1],[[1000,2000,3200],1],[[1000,2000,3300],0]]],
[[0,0,0],2,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,0,0],2,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,0,0],2,-1536,3,256,[[[1000,2000,2488],1],[[1000,2000,1976],1],[[1000,2000,1976],0]]],
[[0,0,0],2,-1536,3,100,[[[1000,2000,2800],1],[[1000,2000,2600],1],[[1000,2000,2400],0]]],
[[0,0,0],2,-1536,6,256,[[[1000,2000,2744],1],[[1000,2000,2488],1],[[1000,2000,2232],1],[[1000,2000,1976],1],[[1000,2000,1720],1],[[1000,2000,1720],0]]],
[[0,0,0],2,-1536,6,100,[[[1000,2000,2900],1],[[1000,2000,2800],1],[[1000,2000,2700],1],[[1000,2000,2600],1],[[1000,2000,2500],1],[[1000,2000,2400],0]]],
[[0,0,0],2,4096,3,256,[[[1000,2000,4280],1],[[1000,2000,5560],1],[[1000,2000,6840],0]]],
[[0,0,0],2,4096,3,100,[[[1000,2000,3500],1],[[1000,2000,4000],1],[[1000,2000,4500],0]]],
[[0,0,0],2,4096,6,256,[[[1000,2000,3512],1],[[1000,2000,4024],1],[[1000,2000,4536],1],[[1000,2000,5048],1],[[1000,2000,5560],1],[[1000,2000,6072],0]]],
[[0,0,0],2,4096,6,100,[[[1000,2000,3200],1],[[1000,2000,3400],1],[[1000,2000,3600],1],[[1000,2000,3800],1],[[1000,2000,4000],1],[[1000,2000,4200],0]]],
[[0,300,0],0,1024,3,256,[[[1155,2000,3204],1],[[1310,2000,3408],1],[[1465,2000,3612],0]]],
[[0,300,0],0,1024,3,100,[[[1061,2000,3080],1],[[1122,2000,3160],1],[[1183,2000,3240],0]]],
[[0,300,0],0,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,300,0],0,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,300,0],0,-1536,3,256,[[[690,2000,2593],1],[[380,2000,2186],1],[[380,2000,2186],0]]],
[[0,300,0],0,-1536,3,100,[[[879,2000,2841],1],[[758,2000,2682],1],[[637,2000,2523],0]]],
[[0,300,0],0,-1536,6,256,[[[845,2000,2796],1],[[690,2000,2592],1],[[535,2000,2388],1],[[380,2000,2184],1],[[225,2000,1980],1],[[225,2000,1980],0]]],
[[0,300,0],0,-1536,6,100,[[[939,2000,2920],1],[[878,2000,2840],1],[[817,2000,2760],1],[[756,2000,2680],1],[[695,2000,2600],1],[[634,2000,2520],0]]],
[[0,300,0],0,4096,3,256,[[[1775,2000,4019],1],[[2550,2000,5038],1],[[3325,2000,6057],0]]],
[[0,300,0],0,4096,3,100,[[[1303,2000,3398],1],[[1606,2000,3796],1],[[1909,2000,4194],0]]],
[[0,300,0],0,4096,6,256,[[[1310,2000,3407],1],[[1620,2000,3814],1],[[1930,2000,4221],1],[[2240,2000,4628],1],[[2550,2000,5035],1],[[2860,2000,5442],0]]],
[[0,300,0],0,4096,6,100,[[[1121,2000,3159],1],[[1242,2000,3318],1],[[1363,2000,3477],1],[[1484,2000,3636],1],[[1605,2000,3795],1],[[1726,2000,3954],0]]],
[[0,300,0],1,1024,3,256,[[[1000,2256,3000],1],[[1000,2512,3000],1],[[1000,2768,3000],0]]],
[[0,300,0],1,1024,3,100,[[[1000,2100,3000],1],[[1000,2200,3000],1],[[1000,2300,3000],0]]],
[[0,300,0],1,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,300,0],1,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,300,0],1,-1536,3,256,[[[1000,1488,3000],1],[[1000,976,3000],1],[[1000,976,3000],0]]],
[[0,300,0],1,-1536,3,100,[[[1000,1800,3000],1],[[1000,1600,3000],1],[[1000,1400,3000],0]]],
[[0,300,0],1,-1536,6,256,[[[1000,1744,3000],1],[[1000,1488,3000],1],[[1000,1232,3000],1],[[1000,976,3000],1],[[1000,720,3000],1],[[1000,720,3000],0]]],
[[0,300,0],1,-1536,6,100,[[[1000,1900,3000],1],[[1000,1800,3000],1],[[1000,1700,3000],1],[[1000,1600,3000],1],[[1000,1500,3000],1],[[1000,1400,3000],0]]],
[[0,300,0],1,4096,3,256,[[[1000,3280,3000],1],[[1000,4560,3000],1],[[1000,5840,3000],0]]],
[[0,300,0],1,4096,3,100,[[[1000,2500,3000],1],[[1000,3000,3000],1],[[1000,3500,3000],0]]],
[[0,300,0],1,4096,6,256,[[[1000,2512,3000],1],[[1000,3024,3000],1],[[1000,3536,3000],1],[[1000,4048,3000],1],[[1000,4560,3000],1],[[1000,5072,3000],0]]],
[[0,300,0],1,4096,6,100,[[[1000,2200,3000],1],[[1000,2400,3000],1],[[1000,2600,3000],1],[[1000,2800,3000],1],[[1000,3000,3000],1],[[1000,3200,3000],0]]],
[[0,300,0],2,1024,3,256,[[[796,2000,3155],1],[[592,2000,3310],1],[[388,2000,3465],0]]],
[[0,300,0],2,1024,3,100,[[[920,2000,3060],1],[[840,2000,3120],1],[[760,2000,3180],0]]],
[[0,300,0],2,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,300,0],2,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,300,0],2,-1536,3,256,[[[1407,2000,2689],1],[[1814,2000,2378],1],[[1814,2000,2378],0]]],
[[0,300,0],2,-1536,3,100,[[[1159,2000,2878],1],[[1318,2000,2756],1],[[1477,2000,2634],0]]],
[[0,300,0],2,-1536,6,256,[[[1203,2000,2844],1],[[1406,2000,2688],1],[[1609,2000,2532],1],[[1812,2000,2376],1],[[2015,2000,2220],1],[[2015,2000,2220],0]]],
[[0,300,0],2,-1536,6,100,[[[1079,2000,2939],1],[[1158,2000,2878],1],[[1237,2000,2817],1],[[1316,2000,2756],1],[[1395,2000,2695],1],[[1474,2000,2634],0]]],
[[0,300,0],2,4096,3,256,[[[-18,2000,3775],1],[[-1036,2000,4550],1],[[-2054,2000,5325],0]]],
[[0,300,0],2,4096,3,100,[[[602,2000,3302],1],[[204,2000,3604],1],[[-193,2000,3906],0]]],
[[0,300,0],2,4096,6,256,[[[592,2000,3310],1],[[184,2000,3620],1],[[-223,2000,3930],1],[[-630,2000,4240],1],[[-1037,2000,4550],1],[[-1444,2000,4860],0]]],
[[0,300,0],2,4096,6,100,[[[840,2000,3121],1],[[680,2000,3242],1],[[520,2000,3363],1],[[360,2000,3484],1],[[200,2000,3605],1],[[40,2000,3726],0]]],
[[64,700,20],0,1024,3,256,[[[858,1985,3213],1],[[716,1970,3426],1],[[574,1955,3639],0]]],
[[64,700,20],0,1024,3,100,[[[945,1994,3083],1],[[890,1988,3166],1],[[835,1982,3249],0]]],
[[64,700,20],0,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[64,700,20],0,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[64,700,20],0,-1536,3,256,[[[1284,2031,2575],1],[[1568,2062,2150],1],[[1568,2062,2150],0]]],
[[64,700,20],0,-1536,3,100,[[[1111,2012,2834],1],[[1222,2024,2668],1],[[1333,2036,2502],0]]],
[[64,700,20],0,-1536,6,256,[[[1142,2015,2787],1],[[1284,2030,2574],1],[[1426,2045,2361],1],[[1568,2060,2148],1],[[1710,2075,1935],1],[[1710,2075,1935],0]]],
[[64,700,20],0,-1536,6,100,[[[1055,2006,2917],1],[[1110,2012,2834],1],[[1165,2018,2751],1],[[1220,2024,2668],1],[[1275,2030,2585],1],[[1330,2036,2502],0]]],
[[64,700,20],0,4096,3,256,[[[290,1923,4063],1],[[-420,1846,5126],1],[[-1130,1769,6189],0]]],
[[64,700,20],0,4096,3,100,[[[723,1970,3415],1],[[446,1940,3830],1],[[169,1910,4245],0]]],
[[64,700,20],0,4096,6,256,[[[716,1969,3425],1],[[432,1938,3850],1],[[148,1907,4275],1],[[-136,1876,4700],1],[[-420,1845,5125],1],[[-704,1814,5550],0]]],
[[64,700,20],0,4096,6,100,[[[889,1988,3166],1],[[778,1976,3332],1],[[667,1964,3498],1],[[556,1952,3664],1],[[445,1940,3830],1],[[334,1928,3996],0]]],
[[64,700,20],1,1024,3,256,[[[1033,2251,3040],1],[[1066,2502,3080],1],[[1099,2753,3120],0]]],
[[64,700,20],1,1024,3,100,[[[1013,2098,3016],1],[[1026,2196,3032],1],[[1039,2294,3048],0]]],
[[64,700,20],1,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[64,700,20],1,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[64,700,20],1,-1536,3,256,[[[934,1499,2919],1],[[868,998,2838],1],[[868,998,2838],0]]],
[[64,700,20],1,-1536,3,100,[[[974,1804,2968],1],[[948,1608,2936],1],[[922,1412,2904],0]]],
[[64,700,20],1,-1536,6,256,[[[967,1749,2960],1],[[934,1498,2920],1],[[901,1247,2880],1],[[868,996,2840],1],[[835,745,2800],1],[[835,745,2800],0]]],
[[64,700,20],1,-1536,6,100,[[[987,1902,2984],1],[[974,1804,2968],1],[[961,1706,2952],1],[[948,1608,2936],1],[[935,1510,2920],1],[[922,1412,2904],0]]],
[[64,700,20],1,4096,3,256,[[[1166,3253,3202],1],[[1332,4506,3404],1],[[1498,5759,3606],0]]],
[[64,700,20],1,4096,3,100,[[[1065,2489,3079],1],[[1130,2978,3158],1],[[1195,3467,3237],0]]],
[[64,700,20],1,4096,6,256,[[[1066,2501,3081],1],[[1132,3002,3162],1],[[1198,3503,3243],1],[[1264,4004,3324],1],[[1330,4505,3405],1],[[1396,5006,3486],0]]],
[[64,700,20],1,4096,6,100,[[[1026,2196,3032],1],[[1052,2392,3064],1],[[1078,2588,3096],1],[[1104,2784,3128],1],[[1130,2980,3160],1],[[1156,3176,3192],0]]],
[[64,700,20],2,1024,3,256,[[[790,2050,2863],1],[[580,2100,2726],1],[[370,2150,2589],0]]],
[[64,700,20],2,1024,3,100,[[[918,2020,2947],1],[[836,2040,2894],1],[[754,2060,2841],0]]],
[[64,700,20],2,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[64,700,20],2,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[64,700,20],2,-1536,3,256,[[[1421,1900,3274],1],[[1842,1800,3548],1],[[1842,1800,3548],0]]],
[[64,700,20],2,-1536,3,100,[[[1164,1961,3107],1],[[1328,1922,3214],1],[[1492,1883,3321],0]]],
[[64,700,20],2,-1536,6,256,[[[1210,1950,3137],1],[[1420,1900,3274],1],[[1630,1850,3411],1],[[1840,1800,3548],1],[[2050,1750,3685],1],[[2050,1750,3685],0]]],
[[64,700,20],2,-1536,6,100,[[[1082,1980,3053],1],[[1164,1960,3106],1],[[1246,1940,3159],1],[[1328,1920,3212],1],[[1410,1900,3265],1],[[1492,1880,3318],0]]],
[[64,700,20],2,4096,3,256,[[[-52,2250,2315],1],[[-1104,2500,1630],1],[[-2156,2750,945],0]]],
[[64,700,20],2,4096,3,100,[[[589,2098,2733],1],[[178,2196,2466],1],[[-233,2294,2199],0]]],
[[64,700,20],2,4096,6,256,[[[579,2100,2726],1],[[158,2200,2452],1],[[-263,2300,2178],1],[[-684,2400,1904],1],[[-1105,2500,1630],1],[[-1526,2600,1356],0]]],
[[64,700,20],2,4096,6,100,[[[836,2039,2893],1],[[672,2078,2786],1],[[508,2117,2679],1],[[344,2156,2572],1],[[180,2195,2465],1],[[16,2234,2358],0]]],
[[0,1500,100],0,1024,3,256,[[[973,1923,2757],1],[[946,1846,2514],1],[[919,1769,2271],0]]],
[[0,1500,100],0,1024,3,100,[[[989,1970,2905],1],[[978,1940,2810],1],[[967,1910,2715],0]]],
[[0,1500,100],0,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,1500,100],0,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,1500,100],0,-1536,3,256,[[[1054,2155,3485],1],[[1108,2310,3970],1],[[1108,2310,3970],0]]],
[[0,1500,100],0,-1536,3,100,[[[1021,2060,3189],1],[[1042,2120,3378],1],[[1063,2180,3567],0]]],
[[0,1500,100],0,-1536,6,256,[[[1027,2077,3243],1],[[1054,2154,3486],1],[[1081,2231,3729],1],[[1108,2308,3972],1],[[1135,2385,4215],1],[[1135,2385,4215],0]]],
[[0,1500,100],0,-1536,6,100,[[[1011,2030,3095],1],[[1022,2060,3190],1],[[1033,2090,3285],1],[[1044,2120,3380],1],[[1055,2150,3475],1],[[1066,2180,3570],0]]],
[[0,1500,100],0,4096,3,256,[[[866,1613,1787],1],[[732,1226,574],1],[[598,839,-639],0]]],
[[0,1500,100],0,4096,3,100,[[[947,1849,2526],1],[[894,1698,2052],1],[[841,1547,1578],0]]],
[[0,1500,100],0,4096,6,256,[[[946,1845,2515],1],[[892,1690,2030],1],[[838,1535,1545],1],[[784,1380,1060],1],[[730,1225,575],1],[[676,1070,90],0]]],
[[0,1500,100],0,4096,6,100,[[[979,1940,2811],1],[[958,1880,2622],1],[[937,1820,2433],1],[[916,1760,2244],1],[[895,1700,2055],1],[[874,1640,1866],0]]],
[[0,1500,100],1,1024,3,256,[[[991,2244,2923],1],[[982,2488,2846],1],[[973,2732,2769],0]]],
[[0,1500,100],1,1024,3,100,[[[997,2095,2970],1],[[994,2190,2940],1],[[991,2285,2910],0]]],
[[0,1500,100],1,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,1500,100],1,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,1500,100],1,-1536,3,256,[[[1017,1512,3154],1],[[1034,1024,3308],1],[[1034,1024,3308],0]]],
[[0,1500,100],1,-1536,3,100,[[[1007,1809,3060],1],[[1014,1618,3120],1],[[1021,1427,3180],0]]],
[[0,1500,100],1,-1536,6,256,[[[1009,1756,3077],1],[[1018,1512,3154],1],[[1027,1268,3231],1],[[1036,1024,3308],1],[[1045,780,3385],1],[[1045,780,3385],0]]],
[[0,1500,100],1,-1536,6,100,[[[1003,1905,3030],1],[[1006,1810,3060],1],[[1009,1715,3090],1],[[1012,1620,3120],1],[[1015,1525,3150],1],[[1018,1430,3180],0]]],
[[0,1500,100],1,4096,3,256,[[[957,3220,2616],1],[[914,4440,2232],1],[[871,5660,1848],0]]],
[[0,1500,100],1,4096,3,100,[[[983,2477,2850],1],[[966,2954,2700],1],[[949,3431,2550],0]]],
[[0,1500,100],1,4096,6,256,[[[983,2488,2846],1],[[966,2976,2692],1],[[949,3464,2538],1],[[932,3952,2384],1],[[915,4440,2230],1],[[898,4928,2076],0]]],
[[0,1500,100],1,4096,6,100,[[[993,2191,2940],1],[[986,2382,2880],1],[[979,2573,2820],1],[[972,2764,2760],1],[[965,2955,2700],1],[[958,3146,2640],0]]],
[[0,1500,100],2,1024,3,256,[[[1254,2000,2972],1],[[1508,2000,2944],1],[[1762,2000,2916],0]]],
[[0,1500,100],2,1024,3,100,[[[1099,2000,2989],1],[[1198,2000,2978],1],[[1297,2000,2967],0]]],
[[0,1500,100],2,1024,6,256,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,1500,100],2,1024,6,100,[[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],1],[[1000,2000,3000],0]]],
[[0,1500,100],2,-1536,3,256,[[[491,2000,3056],1],[[-18,2000,3112],1],[[-18,2000,3112],0]]],
[[0,1500,100],2,-1536,3,100,[[[801,2000,3022],1],[[602,2000,3044],1],[[403,2000,3066],0]]],
[[0,1500,100],2,-1536,6,256,[[[746,2000,3028],1],[[492,2000,3056],1],[[238,2000,3084],1],[[-16,2000,3112],1],[[-270,2000,3140],1],[[-270,2000,3140],0]]],
[[0,1500,100],2,-1536,6,100,[[[901,2000,3011],1],[[802,2000,3022],1],[[703,2000,3033],1],[[604,2000,3044],1],[[505,2000,3055],1],[[406,2000,3066],0]]],
[[0,1500,100],2,4096,3,256,[[[2272,2000,2859],1],[[3544,2000,2718],1],[[4816,2000,2577],0]]],
[[0,1500,100],2,4096,3,100,[[[1497,2000,2945],1],[[1994,2000,2890],1],[[2491,2000,2835],0]]],
[[0,1500,100],2,4096,6,256,[[[1509,2000,2944],1],[[2018,2000,2888],1],[[2527,2000,2832],1],[[3036,2000,2776],1],[[3545,2000,2720],1],[[4054,2000,2664],0]]],
[[0,1500,100],2,4096,6,100,[[[1199,2000,2978],1],[[1398,2000,2956],1],[[1597,2000,2934],1],[[1796,2000,2912],1],[[1995,2000,2890],1],[[2194,2000,2868],0]]]
]"#,
    )
    .unwrap();
    for (angles, axis, delta, duration, scale, steps) in rows {
        let mut task = Task::ObjectMovement {
            target: 0,
            traveled: 0,
        };
        let mut position = [1000, 2000, 3000];
        let mut current = angles;
        let mut updates = vec![];
        for (elapsed, (expected, report)) in steps.into_iter().enumerate() {
            let done = task
                .complete(
                    &[
                        Argument::Integer(axis),
                        Argument::Integer(delta),
                        Argument::Integer(duration),
                    ],
                    Invocation {
                        elapsed: Some(elapsed as i32),
                        unpaused_tasks: 0,
                        unpaused_blocking_tasks: 0,
                    },
                    &mut None,
                    &mut Some(SpatialCall {
                        position: &mut position,
                        angles: &mut current,
                        frame_scale: scale,
                        turn: 0,
                        nominal_ticks: 0,
                        matrix: Matrix::orientation(angles),
                        slave: false,
                        locations: &[],
                        markers: None,
                        effective_position: None,
                        player_position: None,
                        actor_index: 0,
                        map_updates: &mut updates,
                    }),
                    NativeServices::default(),
                )
                .unwrap();
            let label = format!(
                "angles {angles:?} axis {axis} delta {delta} duration {duration} scale {scale} elapsed {elapsed}"
            );
            assert_eq!(position, expected, "{label}");
            assert_eq!(!done, report != 0, "{label}");
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
                    markers: None,
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

/// A lineup owner that logs the helpers the movers call; the path check, facing and every other
/// answer come from the fields.
struct LineupLog {
    reference: ([i32; 3], u16),
    path_clear: bool,
    log: Vec<String>,
}

impl crate::script::lineup::LineupPort for LineupLog {
    fn stale_response(&self) -> bool {
        false
    }
    fn level_roll(&mut self) {}
    fn path_clear(&mut self, target: [i32; 3]) -> bool {
        self.log.push(format!("path {target:?}"));
        self.path_clear
    }
    fn face_point(&mut self, x: i32, z: i32) -> bool {
        self.log.push(format!("face {x} {z}"));
        true
    }
    fn arrival(&mut self, _: i32, _: i32) -> bool {
        false
    }
    fn permitted(&mut self, _: i32, _: i32) -> bool {
        false
    }
    fn step(&mut self, _: crate::script::alignment::CloseStep, _: i32, _: i32) -> bool {
        true
    }
    fn turn_to(&mut self, heading: u16) -> bool {
        self.log.push(format!("turn {heading}"));
        true
    }
    fn heading(&self) -> u16 {
        0
    }
    fn relative_bearing(&self, _: i32, _: i32) -> u16 {
        0
    }
    fn default_animation(&mut self) -> bool {
        true
    }
    fn set_called_flags(&mut self, _: bool) {}
    fn abandon(&mut self, _: bool) {}
    fn response_masked(&self) -> bool {
        false
    }
    fn response_age_over(&self) -> bool {
        false
    }
    fn set_hook_flag(&mut self) {}
    fn clear_hook_flag(&mut self) {}
}

impl crate::script::lineup::LineupHost for LineupLog {
    fn reference(
        &mut self,
        _: Option<u32>,
    ) -> anyhow::Result<(u32, crate::script::lineup::Reference)> {
        Ok((
            0,
            crate::script::lineup::Reference {
                position: self.reference.0,
                heading: self.reference.1,
            },
        ))
    }
    fn receiver_distance(&mut self) -> anyhow::Result<i32> {
        Ok(0)
    }
    fn lineup_distance(&mut self) -> anyhow::Result<i32> {
        Ok(0)
    }
    fn set_lineup_result(&mut self, _: i32) {}
    fn failure(&mut self) -> Option<anyhow::Error> {
        None
    }
}

#[test]
fn the_lineup_natives_target_the_receiver_with_unrotated_offsets_and_retain_the_fallback() {
    let args = [
        Argument::Integer(100),
        Argument::Integer(-30),
        Argument::Integer(40),
    ];
    let invocation = |elapsed| Invocation {
        elapsed: Some(elapsed),
        unpaused_tasks: 0,
        unpaused_blocking_tasks: 0,
    };
    let mut owner = LineupLog {
        reference: ([1000, 77, 2000], 2000),
        path_clear: false,
        log: vec![],
    };
    let mut tasks = NativeTasks::default();
    // A blocked path at elapsed zero selects the plain mover for the rest of the task: with the
    // player's heading 0 and the final heading (2000 + 100 + 1024) & 2047 = 1076, the turn target
    // is (1076 + 1024) & 2047 = 52.
    fn services(owner: &mut LineupLog) -> NativeServices<'_> {
        NativeServices {
            lineup: Some(owner),
            ..Default::default()
        }
    }
    let Some(CallResult::Pending(token)) = tasks
        .call(
            ("PlayerPickupLineUp", CallKind::Task),
            &args,
            invocation(0),
            &mut None,
            &mut None,
            services(&mut owner),
        )
        .unwrap()
    else {
        panic!("a lineup task allocates a continuation")
    };
    assert_eq!(owner.log, ["path [970, 77, 2040]", "turn 52"]);
    owner.log.clear();
    // The fallback is retained: later polls do not test the path again.
    owner.path_clear = true;
    tasks
        .poll(
            token,
            &args,
            invocation(1),
            &mut None,
            &mut None,
            services(&mut owner),
        )
        .unwrap();
    assert_eq!(owner.log, ["turn 52"]);
    // The retained fallback survives a snapshot; the contract names the native with three arguments.
    let snapshot = serde_json::to_value(tasks.snapshot()).unwrap();
    assert!(snapshot.to_string().contains("\"fallback\":true"));
    // The parallel native always runs the plain mover.
    owner.log.clear();
    let mut parallel = NativeTasks::default();
    parallel
        .call(
            ("PlayerParallelLineUp", CallKind::Task),
            &args,
            invocation(0),
            &mut None,
            &mut None,
            services(&mut owner),
        )
        .unwrap();
    assert_eq!(owner.log, ["turn 52"]);
    // A clear path keeps the pickup mover: it faces the target position.
    owner.log.clear();
    let mut pickup = NativeTasks::default();
    pickup
        .call(
            ("PlayerPickupLineUp", CallKind::Task),
            &args,
            invocation(0),
            &mut None,
            &mut None,
            services(&mut owner),
        )
        .unwrap();
    assert_eq!(owner.log, ["path [970, 77, 2040]", "face 970 2040"]);
}
