//! Public invocation contracts use independently authored portable programs.
use cyrus_assets::script::*;
use cyrus_script::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct Service {
    calls: Vec<String>,
    ready: bool,
    ends: usize,
    property: i32,
}
impl Host for Service {
    fn call(
        &mut self,
        target: &Target,
        _: CallKind,
        function: &str,
        _: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        assert_eq!(target, &Target::Me);
        self.calls.push(function.into());
        match function {
            "Wait" => Ok(CallResult::Pending(9)),
            "Query" => Ok(CallResult::Complete(17)),
            "Fail" => anyhow::bail!("cue failed"),
            _ => Ok(CallResult::Complete(0)),
        }
    }
    fn poll(
        &mut self,
        task: u64,
        _: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        assert_eq!(task, 9);
        Ok(self.ready)
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        self.ends += 1;
        Ok(())
    }
    fn read_property(&mut self, _: &Target, _: u8) -> anyhow::Result<i32> {
        Ok(self.property)
    }
    fn write_property(&mut self, _: &Target, _: u8, value: i32) -> anyhow::Result<()> {
        self.property = value;
        Ok(())
    }
}
fn integer(value: i32) -> Value {
    Value::Integer { value }
}
fn assign(destination: Place, value: i32) -> Operation {
    Operation::Assign {
        destination,
        formula: Formula {
            first: integer(value),
            rest: vec![],
        },
    }
}
fn call(name: &str, kind: CallKind) -> Operation {
    Operation::Call {
        call: Call {
            function_id: 0,
            target: Object::Me,
            kind,
            function: name.into(),
            arguments: vec![],
        },
    }
}
fn program(entry: u32, ops: Vec<Operation>) -> Program {
    Program {
        version: 2,
        entry,
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
            .collect::<BTreeMap<_, _>>(),
    }
}
fn runtime(ops: Vec<Operation>) -> Runtime {
    Runtime::new(program(0, ops), "world 17 actor TEST script".into()).unwrap()
}

#[test]
fn native_arguments_evaluate_backwards_but_retain_authored_positions() {
    #[derive(Default)]
    struct Ordered {
        observed: Vec<Vec<Argument>>,
        reads: i32,
    }
    impl Host for Ordered {
        fn call(
            &mut self,
            _: &Target,
            kind: CallKind,
            _: &str,
            arguments: &[Argument],
            _: cyrus_script::Invocation,
            _: &mut [i32],
        ) -> anyhow::Result<CallResult> {
            self.observed.push(arguments.to_vec());
            Ok(if kind == CallKind::Function {
                CallResult::Complete(9)
            } else {
                CallResult::Pending(1)
            })
        }
        fn poll(
            &mut self,
            _: u64,
            arguments: &[Argument],
            _: cyrus_script::Invocation,
            _: &mut [i32],
        ) -> anyhow::Result<bool> {
            self.observed.push(arguments.to_vec());
            Ok(true)
        }
        fn end(&mut self, _: Thread) -> anyhow::Result<()> {
            Ok(())
        }
        fn read_property(&mut self, _: &Target, _: u8) -> anyhow::Result<i32> {
            self.reads += 1;
            Ok(self.reads)
        }
    }
    for kind in [CallKind::Function, CallKind::Task] {
        let Operation::Call { call: mut outer } = call("Observe", kind) else {
            unreachable!()
        };
        outer.arguments = vec![
            Value::Read {
                place: Place::Property {
                    object: Object::Me,
                    reference: 0,
                },
            },
            Value::Read {
                place: Place::Property {
                    object: Object::Me,
                    reference: 0,
                },
            },
        ];
        let operation = if kind == CallKind::Function {
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(outer),
                    },
                    rest: vec![],
                },
            }
        } else {
            Operation::Call { call: outer }
        };
        let mut vm = runtime(vec![operation, Operation::End { target: 0 }]);
        let mut host = Ordered::default();
        let mut flags = [0];
        vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
        assert_eq!(
            host.observed,
            [vec![Argument::Integer(2), Argument::Integer(1)]]
        );
        if kind == CallKind::Task {
            vm.invoke_tasks(&mut host, &mut flags).unwrap();
            assert_eq!(
                host.observed[1],
                [Argument::Integer(4), Argument::Integer(3)]
            );
            assert_eq!(vm.active_tasks(), 0);
        } else {
            assert_eq!(vm.variables(), [9]);
        }
    }
}

#[test]
fn immediate_statements_run_through_end_and_restart_only_on_a_later_stage() {
    let mut vm = runtime(vec![
        assign(Place::Flag { index: 0 }, 8),
        call("Observe", CallKind::Function),
        Operation::End { target: 0 },
    ]);
    let mut host = Service::default();
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(flags, [8]);
    assert_eq!(host.calls, ["Observe"]);
    assert_eq!(host.ends, 1);
    assert_eq!(vm.program_counter(Thread::Main), Some(0));
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(host.calls, ["Observe", "Observe"]);
    assert_eq!(host.ends, 2);
}
#[test]
fn blocking_completion_resumes_the_same_stage_without_restarting_the_task() {
    let mut vm = runtime(vec![
        call("Wait", CallKind::Task),
        assign(Place::Flag { index: 0 }, 4),
        Operation::End { target: 0 },
    ]);
    let mut host = Service::default();
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(vm.waiting(Thread::Main), Some(9));
    assert_eq!(flags, [0]);
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(host.calls, ["Wait"]);
    assert_eq!(flags, [0]);
    host.ready = true;
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(host.calls, ["Wait"]);
    assert_eq!(flags, [4]);
    assert_eq!(vm.waiting(Thread::Main), None);
}
#[test]
fn subroutines_share_locals_and_resume_the_actor_return_continuation() {
    let mut vm = runtime(vec![
        Operation::Gosub { target: 3 },
        assign(Place::Flag { index: 0 }, 6),
        Operation::End { target: 0 },
        assign(Place::Local { index: 0 }, 9),
        call("Observe", CallKind::Function),
        Operation::Return,
    ]);
    let mut host = Service::default();
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(vm.variables(), [9]);
    assert_eq!(flags, [6]);
    assert_eq!(host.calls, ["Observe"]);
}
#[test]
fn interrupt_and_main_have_independent_continuations_and_shared_locals() {
    let p = program(
        2,
        vec![
            assign(Place::Local { index: 0 }, 7),
            Operation::Endint,
            assign(Place::Local { index: 0 }, 3),
            Operation::End { target: 2 },
        ],
    );
    let mut vm = Runtime::new(p, "actor BOTH".into()).unwrap();
    let mut host = Service::default();
    vm.invoke(Thread::Main, &mut host, &mut [], 20).unwrap();
    assert_eq!(vm.variables(), [3]);
    vm.invoke(Thread::Interrupt, &mut host, &mut [], 20)
        .unwrap();
    assert_eq!(vm.variables(), [7]);
    assert_eq!(vm.program_counter(Thread::Main), Some(2));
    assert_eq!(vm.program_counter(Thread::Interrupt), Some(0));
}
#[test]
fn formula_wraps_uses_unsigned_division_and_signed_masked_shifts() {
    let formula = Formula {
        first: integer(-1),
        rest: vec![(4, integer(2)), (1, integer(1)), (6, integer(33))],
    };
    let mut vm = runtime(vec![
        Operation::Assign {
            destination: Place::Flag { index: 0 },
            formula,
        },
        Operation::End { target: 0 },
    ]);
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut Service::default(), &mut flags, 20)
        .unwrap();
    assert_eq!(flags, [-1073741824]);
}
#[test]
fn failed_callbacks_and_runaway_scripts_are_sticky_and_contextual() {
    let mut vm = runtime(vec![
        call("Fail", CallKind::Function),
        Operation::End { target: 0 },
    ]);
    let mut host = Service::default();
    let first = vm
        .invoke(Thread::Main, &mut host, &mut [], 20)
        .unwrap_err()
        .to_string();
    assert!(first.contains("world 17 actor TEST script instruction 0"));
    assert_eq!(
        first,
        vm.invoke(Thread::Main, &mut host, &mut [], 20)
            .unwrap_err()
            .to_string()
    );
    assert_eq!(host.calls, ["Fail"]);
    let mut vm = runtime(vec![Operation::Goto { target: 0 }]);
    assert!(
        vm.invoke(Thread::Main, &mut host, &mut [], 3)
            .unwrap_err()
            .to_string()
            .contains("execution budget 3")
    );
}
#[test]
fn branches_fold_left_to_right_without_boolean_precedence() {
    let condition = |lhs, rhs, conjunction| Condition {
        lhs: integer(lhs),
        comparison: 0,
        rhs: integer(rhs),
        conjunction,
    };
    let mut vm = runtime(vec![
        Operation::If {
            conditions: vec![condition(1, 1, 2), condition(1, 0, 1), condition(1, 0, 0)],
            target: 2,
        },
        call("Wrong", CallKind::Function),
        Operation::End { target: 0 },
    ]);
    let mut host = Service::default();
    vm.invoke(Thread::Main, &mut host, &mut [], 20).unwrap();
    assert!(host.calls.is_empty());
}
#[test]
fn malformed_targets_are_rejected_before_effects() {
    let error = Runtime::new(
        program(0, vec![Operation::End { target: 4 }]),
        "actor BAD".into(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("actor BAD"));
}

#[test]
fn nested_gosub_overwrites_the_actor_return_continuation() {
    let mut vm = runtime(vec![
        Operation::Gosub { target: 3 },
        call("OuterReturn", CallKind::Function),
        Operation::End { target: 0 },
        Operation::Gosub { target: 6 },
        call("InnerReturn", CallKind::Function),
        Operation::Return,
        Operation::Return,
    ]);
    let mut host = Service::default();
    let error = vm
        .invoke(Thread::Main, &mut host, &mut [], 12)
        .unwrap_err()
        .to_string();
    assert!(error.contains("execution budget"));
    assert!(!host.calls.iter().any(|call| call == "OuterReturn"));
    assert!(host.calls.iter().all(|call| call == "InnerReturn"));
}

#[test]
fn conditional_updates_happen_before_signed_comparison() {
    let mut vm = runtime(vec![
        Operation::If {
            conditions: vec![Condition {
                lhs: Value::Update {
                    place: Place::Local { index: 0 },
                    delta: -1,
                },
                comparison: 2,
                rhs: integer(0),
                conjunction: 0,
            }],
            target: 2,
        },
        assign(Place::Flag { index: 0 }, 7),
        Operation::End { target: 0 },
    ]);
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut Service::default(), &mut flags, 20)
        .unwrap();
    assert_eq!(vm.variables(), [-1]);
    assert_eq!(flags, [7]);
}

#[test]
fn yield_anchor_and_script_return_restore_do_not_issue_end_notifications() {
    let mut vm = runtime(vec![
        Operation::Anchor { value: 7 },
        Operation::ScriptRv {
            expected: 3,
            target: 5,
            save_return: true,
        },
        Operation::Gosub { target: 6 },
        Operation::Yield,
        Operation::End { target: 0 },
        Operation::End { target: 0 },
        Operation::Yield,
        Operation::Return,
    ]);
    let mut host = Service::default();
    vm.set_script_return(3);
    vm.invoke(Thread::Main, &mut host, &mut [], 20).unwrap();
    assert_eq!(vm.anchor(), 7);
    assert_eq!(vm.program_counter(Thread::Main), Some(7));
    assert_eq!(host.ends, 0);
    vm.set_script_return(9);
    vm.invoke(Thread::Main, &mut host, &mut [], 20).unwrap();
    assert_eq!(vm.script_return(), 3);
    assert_eq!(vm.program_counter(Thread::Main), Some(4));
}

#[derive(Default)]
struct Tasks {
    next: u64,
    arguments: Vec<(u64, Vec<Argument>)>,
    cancelled: Vec<u64>,
    ready: bool,
    invocations: Vec<Invocation>,
}
impl Host for Tasks {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: &str,
        args: &[Argument],
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        self.next += 1;
        self.invocations.push(invocation);
        self.arguments.push((self.next, args.to_vec()));
        Ok(CallResult::Pending(self.next))
    }
    fn poll(
        &mut self,
        token: u64,
        args: &[Argument],
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        self.arguments.push((token, args.to_vec()));
        self.invocations.push(invocation);
        Ok(self.ready)
    }
    fn cancel(&mut self, token: u64) -> anyhow::Result<()> {
        self.cancelled.push(token);
        Ok(())
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}
#[test]
fn multitask_operands_are_reevaluated_pause_resume_and_stop_share_actor_slots() {
    let mut task = call("Wait", CallKind::Multitask);
    let Operation::Call { call } = &mut task else {
        unreachable!()
    };
    call.arguments.push(Value::Read {
        place: Place::Flag { index: 0 },
    });
    let mut vm = runtime(vec![
        task,
        Operation::Yield,
        Operation::TaskPause { label: 0 },
        Operation::Yield,
        Operation::TaskResume { label: 0 },
        Operation::Yield,
        Operation::TaskStop { label: 0 },
        Operation::End { target: 0 },
    ]);
    let mut host = Tasks::default();
    let mut flags = [4];
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(vm.active_tasks(), 1);
    assert_eq!(vm.waiting(Thread::Main), None);
    flags[0] = 8;
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(
        host.arguments,
        [
            (1, vec![Argument::Integer(4)]),
            (1, vec![Argument::Integer(8)])
        ]
    );
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(host.arguments.len(), 2);
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(host.arguments.len(), 3);
    assert_eq!(host.cancelled, [1]);
    assert_eq!(vm.active_tasks(), 0);
}
#[test]
fn actor_task_capacity_fails_before_a_seventh_effect_and_teardown_cancels_all() {
    let mut ops = vec![call("Wait", CallKind::Multitask); 7];
    ops.push(Operation::End { target: 0 });
    let mut vm = runtime(ops);
    let mut host = Tasks::default();
    let error = vm
        .invoke(Thread::Main, &mut host, &mut [], 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("task capacity"));
    assert_eq!(host.next, 6);
    vm.cancel_tasks(&mut host).unwrap();
    assert_eq!(host.cancelled, [1, 2, 3, 4, 5, 6]);
    assert_eq!(vm.active_tasks(), 0);
}

#[test]
fn an_or_condition_still_evaluates_the_other_mutating_operand() {
    let mut vm = runtime(vec![
        Operation::If {
            conditions: vec![
                Condition {
                    lhs: integer(1),
                    comparison: 0,
                    rhs: integer(1),
                    conjunction: 2,
                },
                Condition {
                    lhs: Value::Update {
                        place: Place::Property {
                            object: Object::Me,
                            reference: 0,
                        },
                        delta: -1,
                    },
                    comparison: 0,
                    rhs: integer(0),
                    conjunction: 0,
                },
            ],
            target: 2,
        },
        assign(Place::Flag { index: 0 }, 7),
        Operation::End { target: 0 },
    ]);
    let mut host = Service {
        property: 1,
        ..Default::default()
    };
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(flags, [7]);
    assert_eq!(host.property, 0);
}

#[test]
fn interrupt_controls_pause_the_shared_main_task_without_polling_it() {
    let mut vm = Runtime::new(
        program(
            2,
            vec![
                Operation::TaskPause { label: 2 },
                Operation::Endint,
                call("Wait", CallKind::Task),
                assign(Place::Flag { index: 0 }, 7),
                Operation::End { target: 2 },
            ],
        ),
        "actor SHARED".into(),
    )
    .unwrap();
    let mut host = Tasks::default();
    let mut flags = [0];
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(flags, [0]);
    assert!(vm.has_blocking_task());
    vm.invoke_body(Thread::Interrupt, &mut host, &mut flags, 20)
        .unwrap();
    assert_eq!(host.arguments.len(), 1);
    assert!(!vm.has_blocking_task());
    vm.invoke(Thread::Main, &mut host, &mut flags, 20).unwrap();
    assert_eq!(flags, [7]);
    assert_eq!(host.arguments.len(), 1);
    assert_eq!(vm.active_tasks(), 1);
}

#[test]
fn object_tasks_fail_before_effects_instead_of_using_the_callers_slots() {
    let Operation::Call { mut call } = call("Wait", CallKind::Task) else {
        unreachable!()
    };
    call.target = Object::Player;
    let mut vm = runtime(vec![
        Operation::ObjectCall { call },
        Operation::End { target: 0 },
    ]);
    let mut host = Service::default();
    let error = vm
        .invoke(Thread::Main, &mut host, &mut [], 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("object target Player routing unsupported"));
    assert!(host.calls.is_empty());
    assert_eq!(vm.active_tasks(), 0);
}

#[test]
fn task_elapsed_uses_current_scale_and_wrapping_counters() {
    let mut vm = runtime(vec![
        call("Observe", CallKind::Multitask),
        Operation::Yield,
        Operation::End { target: 1 },
    ]);
    let mut host = Tasks::default();
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    assert_eq!(host.invocations[0].elapsed, Some(0));
    assert_eq!(host.invocations[0].unpaused_tasks, 0);
    for scale in [256, 1024, 0, u32::MAX] {
        vm.invoke_tasks_timed(&mut host, &mut [], scale).unwrap();
    }
    assert_eq!(
        host.invocations
            .iter()
            .map(|input| input.elapsed)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(8), Some(0), Some(16777215)]
    );
    // The fourth pass left the counter at five. Advance to its last u16 value.
    for _ in 5..65535 {
        vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    }
    for _ in 0..3 {
        vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    }
    assert_eq!(
        host.invocations
            .iter()
            .rev()
            .take(3)
            .map(|input| input.elapsed)
            .collect::<Vec<_>>(),
        [Some(1), Some(0), Some(65535)]
    );
}

#[test]
fn paused_tasks_keep_their_counter_and_later_slots_see_earlier_completions() {
    let mut vm = runtime(vec![
        call("Observe", CallKind::Multitask),
        call("Observe", CallKind::Task),
        Operation::Yield,
        Operation::TaskPause { label: 0 },
        Operation::Yield,
        Operation::TaskResume { label: 0 },
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    let mut host = Tasks::default();
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    assert_eq!(host.invocations[1].unpaused_tasks, 1);
    host.ready = true;
    vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    assert_eq!(host.invocations[2].unpaused_tasks, 2);
    assert_eq!(host.invocations[2].unpaused_blocking_tasks, 1);
    assert_eq!(host.invocations[3].unpaused_tasks, 1);
    assert_eq!(host.invocations[3].unpaused_blocking_tasks, 1);

    let mut vm = runtime(vec![
        call("Observe", CallKind::Multitask),
        Operation::TaskPause { label: 0 },
        Operation::Yield,
        Operation::TaskResume { label: 0 },
        Operation::Yield,
        Operation::End { target: 0 },
    ]);
    let mut host = Tasks::default();
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    assert_eq!(host.invocations.len(), 1);
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    vm.invoke_tasks_timed(&mut host, &mut [], 512).unwrap();
    assert_eq!(host.invocations[1].elapsed, Some(2));
}

struct ForeignTasks {
    tasks: Tasks,
    call: Call,
}
impl Host for ForeignTasks {
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        invocation: Invocation,
        flags: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        self.tasks
            .call(target, kind, function, arguments, invocation, flags)
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
        flags: &mut [i32],
    ) -> anyhow::Result<bool> {
        self.tasks.poll(token, arguments, invocation, flags)
    }
    fn task_call(&mut self, origin: CodeOrigin, instruction: u32) -> anyhow::Result<Call> {
        assert_eq!((origin, instruction), (CodeOrigin(100), 0));
        Ok(self.call.clone())
    }
    fn end(&mut self, thread: Thread) -> anyhow::Result<()> {
        self.tasks.end(thread)
    }
}
fn foreign_call() -> Call {
    Call {
        function_id: 0,
        kind: CallKind::Task,
        target: Object::Player,
        function: "Observe".into(),
        arguments: vec![
            Value::String { index: 0 },
            Value::Read {
                place: Place::Local { index: 0 },
            },
        ],
    }
}
fn foreign_source() -> TaskSource {
    TaskSource {
        origin: CodeOrigin(100),
        instruction: 0,
        object_call: true,
        thread: Thread::Main,
    }
}
#[test]
fn routed_tasks_preserve_initial_strings_then_use_receiver_strings_and_locals() {
    let mut recipient = program(0, vec![Operation::Yield, Operation::End { target: 0 }]);
    recipient.variables[0] = 37;
    recipient.strings = vec!["receiver strings".into()];
    let mut vm = Runtime::new(recipient, "receiver".into()).unwrap();
    let call = foreign_call();
    let mut host = ForeignTasks {
        tasks: Tasks::default(),
        call: call.clone(),
    };
    vm.receive(
        &call,
        &["caller strings".into()],
        Some(foreign_source()),
        &mut host,
        &mut [],
    )
    .unwrap();
    assert_eq!(vm.program_counter(Thread::Main), Some(0));
    assert_eq!(
        host.tasks.arguments,
        [(
            1,
            vec![
                Argument::String("caller strings".into()),
                Argument::Integer(37)
            ]
        )]
    );
    vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    assert_eq!(
        host.tasks.arguments[1],
        (
            1,
            vec![
                Argument::String("receiver strings".into()),
                Argument::Integer(37)
            ]
        )
    );
    assert_eq!(vm.active_tasks(), 1);
    host.tasks.ready = true;
    vm.invoke_tasks_timed(&mut host, &mut [], 256).unwrap();
    assert_eq!(vm.active_tasks(), 0);
}
#[test]
fn routed_operand_aliases_share_one_slot_and_receiver_labels_control_that_operand() {
    let mut vm = runtime(vec![
        Operation::TaskStop { label: 103 },
        Operation::End { target: 0 },
    ]);
    let mut call = foreign_call();
    call.kind = CallKind::Multitask;
    call.arguments.clear();
    let mut host = Tasks::default();
    vm.receive(&call, &[], Some(foreign_source()), &mut host, &mut [])
        .unwrap();
    let alias = TaskSource {
        origin: CodeOrigin(103),
        instruction: 0,
        object_call: false,
        thread: Thread::Interrupt,
    };
    vm.receive(&call, &[], Some(alias), &mut host, &mut [])
        .unwrap();
    assert_eq!(host.next, 1);
    assert_eq!(vm.active_tasks(), 1);
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
        .unwrap();
    assert_eq!(host.cancelled, [1]);
    assert_eq!(vm.active_tasks(), 0);
}
#[test]
fn missing_foreign_task_source_fails_before_polling_and_is_sticky() {
    let mut vm = runtime(vec![Operation::Yield, Operation::End { target: 0 }]);
    let mut call = foreign_call();
    call.arguments.clear();
    let mut host = Tasks::default();
    vm.receive(&call, &[], Some(foreign_source()), &mut host, &mut [])
        .unwrap();
    let error = vm
        .invoke_tasks_timed(&mut host, &mut [], 256)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("task source 100 instruction 0 unavailable"),
        "{error}"
    );
    assert_eq!(host.arguments.len(), 1);
    assert_eq!(vm.failure(), Some(error.as_str()));
    assert_eq!(
        vm.invoke_tasks_timed(&mut host, &mut [], 256)
            .unwrap_err()
            .to_string(),
        error
    );
}
#[test]
fn routed_capacity_and_identity_overflow_fail_before_native_effects() {
    let mut vm = runtime(vec![Operation::Yield, Operation::End { target: 0 }]);
    let mut call = foreign_call();
    call.arguments.clear();
    let mut host = Tasks::default();
    for instruction in 0..6 {
        let source = TaskSource {
            instruction,
            ..foreign_source()
        };
        vm.receive(&call, &[], Some(source), &mut host, &mut [])
            .unwrap();
    }
    let source = TaskSource {
        instruction: 6,
        ..foreign_source()
    };
    let error = vm
        .receive(&call, &[], Some(source), &mut host, &mut [])
        .unwrap_err()
        .to_string();
    assert!(error.contains("actor task capacity exhausted"), "{error}");
    assert_eq!(host.next, 6);
    assert_eq!(vm.active_tasks(), 6);
    let mut vm = runtime(vec![Operation::Yield, Operation::End { target: 0 }]);
    let source = TaskSource {
        origin: CodeOrigin(u32::MAX),
        ..foreign_source()
    };
    let error = vm
        .receive(&call, &[], Some(source), &mut host, &mut [])
        .unwrap_err()
        .to_string();
    assert!(error.contains("task operand identity overflow"), "{error}");
    assert_eq!(host.next, 6);
}

#[test]
fn ordinary_entry_respects_blocking_operand_but_direct_interrupt_does_not() {
    let mut runtime = Runtime::new(
        program(
            3,
            vec![
                call("Wait", CallKind::Task),
                Operation::Increment {
                    destination: Place::Local { index: 0 },
                    delta: 1,
                },
                Operation::Endint,
                Operation::End { target: 3 },
            ],
        ),
        "entry test".into(),
    )
    .unwrap();
    let mut host = Service::default();
    runtime.invoke_entry(&mut host, &mut [], 10).unwrap();
    assert_eq!(runtime.program_counter(Thread::Interrupt), Some(1));
    runtime.invoke_entry(&mut host, &mut [], 10).unwrap();
    assert_eq!(runtime.variables(), [0]);
    assert_eq!(host.calls, ["Wait"]);
    runtime
        .invoke_body(Thread::Interrupt, &mut host, &mut [], 10)
        .unwrap();
    assert_eq!(runtime.variables(), [1]);
    assert_eq!(runtime.program_counter(Thread::Interrupt), Some(0));
    assert_eq!(runtime.active_tasks(), 1);
}

#[test]
fn entry_compares_foreign_task_operands_in_shared_code_identity() {
    for (origin, expected) in [(10, 0), (200, 1)] {
        let mut runtime = Runtime::from_shared(
            std::sync::Arc::new(program(
                2,
                vec![
                    Operation::Increment {
                        destination: Place::Local { index: 0 },
                        delta: 1,
                    },
                    Operation::Endint,
                    Operation::End { target: 2 },
                ],
            )),
            CodeOrigin(100),
            vec![0],
            "foreign entry".into(),
        )
        .unwrap();
        let mut host = Service::default();
        let Operation::Call { call } = call("Wait", CallKind::Task) else {
            unreachable!()
        };
        runtime
            .receive(
                &call,
                &[],
                Some(TaskSource {
                    origin: CodeOrigin(origin),
                    instruction: 0,
                    object_call: false,
                    thread: Thread::Main,
                }),
                &mut host,
                &mut [],
            )
            .unwrap();
        runtime.invoke_entry(&mut host, &mut [], 10).unwrap();
        assert_eq!(runtime.variables(), [expected]);
        assert_eq!(runtime.active_tasks(), 1);
    }
}
