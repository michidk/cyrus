//! Saved VMs resume through public invocations without replaying native effects.
use cyrus_script::{persistence::Snapshot, program::*, *};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
struct Service {
    next: u64,
    calls: usize,
    ready: bool,
    polls: Vec<(u64, Option<i32>, Vec<Argument>)>,
    foreign: Option<Call>,
}
impl Host for Service {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: &str,
        _: &[Argument],
        _: Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        self.calls += 1;
        let token = self.next;
        self.next += 1;
        Ok(CallResult::Pending(token))
    }
    fn poll(
        &mut self,
        token: u64,
        args: &[Argument],
        invocation: Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        self.polls.push((token, invocation.elapsed, args.to_vec()));
        Ok(self.ready)
    }
    fn task_call(&mut self, _: CodeOrigin, _: u32) -> anyhow::Result<Call> {
        Ok(self.foreign.clone().unwrap())
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}
fn call(kind: CallKind) -> Call {
    Call {
        function_id: 3,
        kind,
        target: Object::Me,
        function: "Wait".into(),
        arguments: vec![Value::Read {
            place: Place::Local { index: 0 },
        }],
    }
}
fn program(ops: Vec<Operation>) -> Arc<Program> {
    Arc::new(Program {
        version: 2,
        entry: 0,
        variables: vec![0],
        strings: vec![],
        steps: ops
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
            .collect(),
    })
}
fn source() -> Arc<Program> {
    program(vec![
        Operation::Anchor { value: 9 },
        Operation::Gosub { target: 5 },
        Operation::Call {
            call: call(CallKind::Task),
        },
        Operation::Increment {
            destination: Place::Flag { index: 0 },
            delta: 1,
        },
        Operation::End { target: 0 },
        Operation::Increment {
            destination: Place::Local { index: 0 },
            delta: 1,
        },
        Operation::Call {
            call: call(CallKind::Task),
        },
        Operation::Return,
    ])
}
fn fresh(program: Arc<Program>, origin: u32) -> Runtime {
    Runtime::from_shared(
        program,
        CodeOrigin(origin),
        vec![0],
        "world TEST actor A".into(),
    )
    .unwrap()
}
fn waiting() -> (Runtime, Service, BTreeMap<CodeOrigin, Arc<Program>>) {
    let code = source();
    let mut runtime = fresh(Arc::clone(&code), 100);
    let mut host = Service {
        next: 10,
        ..Default::default()
    };
    runtime
        .invoke_body(Thread::Main, &mut host, &mut [0], 100)
        .unwrap();
    runtime
        .invoke_tasks_timed(&mut host, &mut [0], 256)
        .unwrap();
    (runtime, host, BTreeMap::from([(CodeOrigin(100), code)]))
}

#[test]
fn restored_subroutine_wait_keeps_locals_returns_and_elapsed_without_reissuing_calls() {
    let (mut original, mut original_host, sources) = waiting();
    let bytes = serde_json::to_vec(&original.snapshot(&sources).unwrap()).unwrap();
    let saved: Snapshot = serde_json::from_slice(&bytes).unwrap();
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(100),
        saved,
        &sources,
        &BTreeMap::from([(10, 1000)]),
        "restored actor A".into(),
    )
    .unwrap();
    let mut restored_host = Service {
        next: 1001,
        ..Default::default()
    };
    assert_eq!(restored_host.calls, 0);
    assert_eq!(restored.program_counter(Thread::Main), Some(7));
    assert_eq!(restored.variables(), &[1]);
    assert_eq!(restored.anchor(), 9);
    assert_eq!(restored.waiting(Thread::Main), Some(1000));
    let mut flags = [0];
    restored
        .invoke_tasks_timed(&mut restored_host, &mut flags, 256)
        .unwrap();
    assert_eq!(
        restored_host.polls,
        vec![(1000, Some(2), vec![Argument::Integer(1)])]
    );
    restored_host.ready = true;
    restored
        .invoke_tasks_timed(&mut restored_host, &mut flags, 256)
        .unwrap();
    restored
        .invoke_body(Thread::Main, &mut restored_host, &mut flags, 100)
        .unwrap();
    assert_eq!(restored_host.calls, 1);
    assert_eq!(restored.waiting(Thread::Main), Some(1001));
    restored
        .invoke_tasks_timed(&mut restored_host, &mut flags, 256)
        .unwrap();
    restored
        .invoke_body(Thread::Main, &mut restored_host, &mut flags, 100)
        .unwrap();
    assert_eq!(flags, [1]);
    assert_eq!(restored.variables(), &[1]);
    assert_eq!(restored.program_counter(Thread::Main), Some(0));
    original
        .invoke_tasks_timed(&mut original_host, &mut [0], 256)
        .unwrap();
    original_host.ready = true;
    let mut uninterrupted_flags = [0];
    original
        .invoke_tasks_timed(&mut original_host, &mut uninterrupted_flags, 256)
        .unwrap();
    original
        .invoke_body(
            Thread::Main,
            &mut original_host,
            &mut uninterrupted_flags,
            100,
        )
        .unwrap();
    original
        .invoke_tasks_timed(&mut original_host, &mut uninterrupted_flags, 256)
        .unwrap();
    original
        .invoke_body(
            Thread::Main,
            &mut original_host,
            &mut uninterrupted_flags,
            100,
        )
        .unwrap();
    assert_eq!(uninterrupted_flags, flags);
    assert_eq!(original.variables(), restored.variables());
    assert_eq!(original_host.calls, 2);
}

#[test]
fn malformed_state_content_and_missing_bindings_reject_without_changing_the_live_vm() {
    let (runtime, host, sources) = waiting();
    let snapshot = serde_json::to_value(runtime.snapshot(&sources).unwrap()).unwrap();
    let before = serde_json::to_value(runtime.state()).unwrap();
    for (pointer, value) in [
        ("/version", json!(2)),
        ("/state/variables", json!([])),
        ("/state/threads/0", json!(null)),
        ("/state/threads/1", json!({"cursor": 0})),
        ("/state/threads/0/cursor", json!(999)),
        ("/state/return_to", json!(999)),
        ("/state/tasks/0/operand", json!(999)),
        ("/state/tasks/0/blocking", json!(false)),
        ("/state/tasks/0/source/instruction", json!(0)),
        ("/state/tasks/0/source/object_call", json!(true)),
        ("/programs/100/strings", json!(["changed"])),
    ] {
        let mut document = snapshot.clone();
        *document.pointer_mut(pointer).unwrap() = value;
        let saved: Snapshot = serde_json::from_value(document).unwrap();
        let error = Runtime::from_snapshot(
            CodeOrigin(100),
            saved,
            &sources,
            &BTreeMap::from([(10, 1000)]),
            "restore actor A".into(),
        )
        .err()
        .unwrap();
        assert!(
            format!("{error:#}").contains("restore actor A restore"),
            "{error:#}"
        );
    }
    let saved: Snapshot = serde_json::from_value(snapshot.clone()).unwrap();
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(101),
            saved.clone(),
            &sources,
            &BTreeMap::from([(10, 1000)]),
            "actor".into()
        )
        .is_err()
    );
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            saved.clone(),
            &sources,
            &BTreeMap::new(),
            "actor".into()
        )
        .is_err()
    );
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            saved.clone(),
            &sources,
            &BTreeMap::from([(11, 1000)]),
            "actor".into()
        )
        .is_err()
    );
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            saved,
            &BTreeMap::new(),
            &BTreeMap::from([(10, 1000)]),
            "actor".into()
        )
        .is_err()
    );
    assert_eq!(serde_json::to_value(runtime.state()).unwrap(), before);
    assert_eq!(host.calls, 1);
}

#[test]
fn foreign_task_slots_keep_order_pause_and_wrapping_counters_across_restore() {
    let receiver_code = program(vec![
        Operation::TaskResume { label: 4 },
        Operation::End { target: 0 },
    ]);
    let foreign_code = program(vec![
        Operation::ObjectCall {
            call: call(CallKind::Multitask),
        },
        Operation::End { target: 0 },
    ]);
    let mut receiver = fresh(Arc::clone(&receiver_code), 100);
    let mut host = Service {
        next: 10,
        ..Default::default()
    };
    receiver
        .receive(
            &call(CallKind::Multitask),
            &[],
            Some(TaskSource {
                origin: CodeOrigin(300),
                instruction: 0,
                object_call: true,
                thread: Thread::Interrupt,
            }),
            &mut host,
            &mut [0],
        )
        .unwrap();
    let sources = BTreeMap::from([
        (CodeOrigin(100), receiver_code),
        (CodeOrigin(300), foreign_code),
    ]);
    assert!(receiver.snapshot(&BTreeMap::new()).is_err());
    let mut document = serde_json::to_value(receiver.snapshot(&sources).unwrap()).unwrap();
    let mut task = document["state"]["tasks"][0].take();
    task["paused"] = json!(true);
    task["counter"] = json!(65535);
    document["state"]["tasks"][4] = task;
    let saved: Snapshot = serde_json::from_value(document).unwrap();
    let descriptors = saved.task_calls().unwrap();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].token, 10);
    assert_eq!(descriptors[0].source.origin, CodeOrigin(300));
    assert!(descriptors[0].source.object_call);
    assert_eq!(descriptors[0].call.kind, CallKind::Multitask);
    assert_eq!(descriptors[0].call.function, "Wait");
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(100),
        saved.clone(),
        &sources,
        &BTreeMap::from([(10, 1000)]),
        "receiver".into(),
    )
    .unwrap();
    let mut restored_host = Service {
        ready: true,
        foreign: Some(call(CallKind::Multitask)),
        ..Default::default()
    };
    restored
        .invoke_tasks_timed(&mut restored_host, &mut [0], 256)
        .unwrap();
    assert!(restored_host.polls.is_empty());
    let mut resumed = serde_json::to_value(restored.snapshot(&sources).unwrap()).unwrap();
    assert!(resumed["state"]["tasks"][0].is_null());
    assert_eq!(resumed["state"]["tasks"][4]["counter"], 65535);
    resumed["state"]["tasks"][4]["paused"] = json!(false);
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(100),
        serde_json::from_value(resumed).unwrap(),
        &sources,
        &BTreeMap::from([(1000, 2000)]),
        "receiver".into(),
    )
    .unwrap();
    restored_host.ready = false;
    restored
        .invoke_tasks_timed(&mut restored_host, &mut [0], 256)
        .unwrap();
    restored
        .invoke_tasks_timed(&mut restored_host, &mut [0], 256)
        .unwrap();
    assert_eq!(
        restored_host.polls,
        vec![
            (2000, Some(65535), vec![Argument::Integer(0)]),
            (2000, Some(0), vec![Argument::Integer(0)]),
        ]
    );
    let mut changed = sources.clone();
    Arc::make_mut(changed.get_mut(&CodeOrigin(300)).unwrap())
        .strings
        .push("different".into());
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            saved,
            &changed,
            &BTreeMap::from([(10, 1000)]),
            "receiver".into()
        )
        .is_err()
    );
}

#[test]
fn restore_preserves_sticky_failure_without_retrying_a_native_effect() {
    let code = program(vec![Operation::Unsupported {
        reason: "unsupported native state".into(),
    }]);
    let mut runtime = fresh(Arc::clone(&code), 100);
    let mut host = Service::default();
    assert!(
        runtime
            .invoke_body(Thread::Main, &mut host, &mut [0], 100)
            .is_err()
    );
    let sources = BTreeMap::from([(CodeOrigin(100), code)]);
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(100),
        runtime.snapshot(&sources).unwrap(),
        &sources,
        &BTreeMap::new(),
        "actor".into(),
    )
    .unwrap();
    assert_eq!(restored.failure(), runtime.failure());
    assert!(
        restored
            .invoke_body(Thread::Main, &mut host, &mut [0], 100)
            .is_err()
    );
    assert_eq!(host.calls, 0);
}

#[test]
fn both_continuations_and_saved_return_byte_resume_after_a_yield() {
    let mut code = program(vec![
        Operation::ScriptRv {
            expected: 17,
            target: 0,
            save_return: true,
        },
        Operation::Gosub { target: 4 },
        Operation::End { target: 2 },
        Operation::End { target: 2 },
        Operation::Yield,
        Operation::Return,
    ]);
    Arc::make_mut(&mut code).entry = 2;
    let mut runtime = fresh(Arc::clone(&code), 100);
    runtime.set_script_return(17);
    let mut host = Service::default();
    runtime
        .invoke_body(Thread::Interrupt, &mut host, &mut [0], 100)
        .unwrap();
    let sources = BTreeMap::from([(CodeOrigin(100), code)]);
    let mut missing_interrupt = serde_json::to_value(runtime.snapshot(&sources).unwrap()).unwrap();
    missing_interrupt["state"]["threads"][1] = json!(null);
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            serde_json::from_value(missing_interrupt).unwrap(),
            &sources,
            &BTreeMap::new(),
            "actor".into(),
        )
        .is_err()
    );
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(100),
        runtime.snapshot(&sources).unwrap(),
        &sources,
        &BTreeMap::new(),
        "actor".into(),
    )
    .unwrap();
    assert_eq!(restored.program_counter(Thread::Main), Some(2));
    assert_eq!(restored.program_counter(Thread::Interrupt), Some(5));
    assert_eq!(restored.script_return(), 17);
    restored.set_script_return(3);
    restored
        .invoke_body(Thread::Interrupt, &mut host, &mut [0], 100)
        .unwrap();
    assert_eq!(restored.script_return(), 17);
    assert_eq!(restored.program_counter(Thread::Interrupt), Some(2));
    assert_eq!(host.calls, 0);
}

#[test]
fn task_rebinding_is_complete_and_injective_and_retains_poll_order() {
    let code = program(vec![
        Operation::Call {
            call: call(CallKind::Multitask),
        },
        Operation::Call {
            call: call(CallKind::Multitask),
        },
        Operation::End { target: 0 },
    ]);
    let mut runtime = fresh(Arc::clone(&code), 100);
    let mut host = Service {
        next: 10,
        ..Default::default()
    };
    runtime
        .invoke_body(Thread::Main, &mut host, &mut [0], 100)
        .unwrap();
    let sources = BTreeMap::from([(CodeOrigin(100), code)]);
    let snapshot = runtime.snapshot(&sources).unwrap();
    assert_eq!(snapshot.task_tokens().collect::<Vec<_>>(), vec![10, 11]);
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            snapshot.clone(),
            &sources,
            &BTreeMap::from([(10, 1000), (11, 1000)]),
            "actor".into()
        )
        .is_err()
    );
    let mut restored = Runtime::from_snapshot(
        CodeOrigin(100),
        snapshot.clone(),
        &sources,
        &BTreeMap::from([(10, 1001), (11, 1000)]),
        "actor".into(),
    )
    .unwrap();
    let mut restored_host = Service::default();
    restored
        .invoke_tasks_timed(&mut restored_host, &mut [0], 256)
        .unwrap();
    assert_eq!(
        restored_host.polls,
        vec![
            (1001, Some(1), vec![Argument::Integer(0)]),
            (1000, Some(1), vec![Argument::Integer(0)]),
        ]
    );
    let mut duplicated = serde_json::to_value(snapshot).unwrap();
    duplicated["state"]["tasks"][1] = duplicated["state"]["tasks"][0].clone();
    assert!(
        Runtime::from_snapshot(
            CodeOrigin(100),
            serde_json::from_value(duplicated).unwrap(),
            &sources,
            &BTreeMap::from([(10, 1001), (11, 1000)]),
            "actor".into()
        )
        .is_err()
    );
}

#[test]
fn saved_task_calls_are_validated_unevaluated_and_keep_the_source_operand() {
    let (runtime, host, sources) = waiting();
    let snapshot = runtime.snapshot(&sources).unwrap();
    let calls = snapshot.task_calls().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].token, 10);
    assert_eq!(calls[0].source.origin, CodeOrigin(100));
    assert_eq!(calls[0].source.instruction, 6);
    assert!(!calls[0].source.object_call);
    assert_eq!(calls[0].call.function, "Wait");
    assert_eq!(calls[0].call.kind, CallKind::Task);
    assert_eq!(
        calls[0].call.arguments,
        [Value::Read {
            place: Place::Local { index: 0 }
        }]
    );
    assert_eq!(host.calls, 1);
    let mut malformed = serde_json::to_value(snapshot).unwrap();
    malformed["state"]["tasks"][0]["source"]["instruction"] = json!(0);
    let malformed: Snapshot = serde_json::from_value(malformed).unwrap();
    assert!(malformed.task_calls().is_err());
}
