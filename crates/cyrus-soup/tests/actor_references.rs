//! References cross the public VM boundary without a numeric pointer encoding.
use anyhow::{Result, ensure};
use cyrus_soup::{program::*, *};
use std::{collections::BTreeMap, sync::Arc};
const REF: ActorReference = ActorReference {
    generation: 7,
    index: 4,
};
#[derive(Default)]
struct Service {
    arguments: Vec<Vec<Argument>>,
    targets: Vec<Target>,
    complete: bool,
}
impl Host for Service {
    fn read_value(&mut self, _: &Target, reference: u16) -> Result<ScriptValue> {
        ensure!(reference == 294, "wrong full property index");
        Ok(ScriptValue::Actor(REF))
    }
    fn route(
        &mut self,
        target: &Target,
        _: &Call,
        _: &[String],
        _: NativeCallSite,
        _: &mut [i32],
    ) -> Result<Route> {
        self.targets.push(target.clone());
        Ok(Route::Here)
    }
    fn call(
        &mut self,
        _: &Target,
        kind: CallKind,
        _: Native,
        arguments: &[Argument],
        _: Invocation,
        _: &mut [i32],
    ) -> Result<CallResult> {
        self.arguments.push(arguments.to_vec());
        Ok(if kind == CallKind::Task {
            CallResult::Pending(5)
        } else {
            CallResult::Complete(0)
        })
    }
    fn poll(
        &mut self,
        _: u64,
        arguments: &[Argument],
        _: Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        self.arguments.push(arguments.to_vec());
        Ok(self.complete)
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        Ok(())
    }
}
fn address() -> Value {
    Value::Read {
        place: Place::Property {
            object: Object::Me,
            reference: 294,
        },
    }
}
fn local(index: u8) -> Value {
    Value::Read {
        place: Place::Local { index },
    }
}
fn assign(index: u8, value: Value) -> Operation {
    Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first: value,
            rest: vec![],
        },
    }
}
fn program(operations: Vec<Operation>) -> Program {
    let mut steps: BTreeMap<_, _> = operations
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
        .collect();
    let end = steps.len() as u32;
    steps.insert(
        end,
        Step {
            next: end + 1,
            operation: Operation::End { target: end },
        },
    );
    Program {
        version: 4,
        entry: 0,
        variables: vec![0, 0],
        strings: vec![],
        steps,
    }
}
fn call(target: Object, argument: Value, kind: CallKind) -> Operation {
    let object = target != Object::Me;
    let call = Call {
        target,
        function_id: 0,
        function: Native::WaitLineUp,
        kind,
        arguments: vec![argument],
    };
    if object {
        Operation::ObjectCall { call }
    } else {
        Operation::Call { call }
    }
}
#[test]
fn references_survive_copy_native_arguments_object_targets_and_pending_continuation() -> Result<()>
{
    let mut vm = Runtime::new(
        program(vec![
            assign(0, address()),
            assign(1, local(0)),
            call(Object::Local { index: 1 }, address(), CallKind::Function),
            call(Object::Me, local(0), CallKind::Task),
            assign(1, Value::Integer { value: 19 }),
        ]),
        "references".into(),
    )?;
    let mut host = Service::default();
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)?;
    assert_eq!(vm.variables(), [ScriptValue::Actor(REF); 2]);
    assert_eq!(host.targets[0], Target::Actor(REF));
    assert_eq!(host.arguments, vec![vec![Argument::Actor(REF)]; 2]);
    assert_eq!(vm.active_tasks(), 1);
    host.complete = true;
    vm.invoke_tasks(&mut host, &mut [])?;
    vm.invoke_body(Thread::Main, &mut host, &mut [], 20)?;
    assert_eq!(host.arguments[2], vec![Argument::Actor(REF)]);
    assert_eq!(vm.variables()[1], 19);
    Ok(())
}
#[test]
fn reference_arithmetic_and_integer_targets_fail_before_continuation_and_stay_failed() -> Result<()>
{
    for operation in [
        Operation::Increment {
            destination: Place::Local { index: 0 },
            delta: 1,
        },
        Operation::Assign {
            destination: Place::Flag { index: 0 },
            formula: Formula {
                first: local(0),
                rest: vec![],
            },
        },
        Operation::Assign {
            destination: Place::Local { index: 1 },
            formula: Formula {
                first: local(0),
                rest: vec![(1, Value::Integer { value: 1 })],
            },
        },
    ] {
        let mut vm = Runtime::new(
            program(vec![
                assign(0, address()),
                operation,
                assign(1, Value::Integer { value: 99 }),
            ]),
            "references".into(),
        )?;
        let mut host = Service::default();
        let mut flags = [13];
        let error = vm
            .invoke_body(Thread::Main, &mut host, &mut flags, 20)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("actor reference cannot be used as an integer"),
            "{error}"
        );
        assert_eq!(vm.variables()[1], 0);
        assert_eq!(flags, [13]);
        assert_eq!(
            vm.invoke_body(Thread::Main, &mut host, &mut flags, 20)
                .unwrap_err()
                .to_string(),
            error
        );
    }
    let mut vm = Runtime::new(
        program(vec![call(
            Object::Local { index: 0 },
            Value::Integer { value: 0 },
            CallKind::Function,
        )]),
        "integer target".into(),
    )?;
    let mut host = Service::default();
    assert!(
        vm.invoke_body(Thread::Main, &mut host, &mut [], 20)
            .unwrap_err()
            .to_string()
            .contains("integer is not an actor identity")
    );
    assert!(host.arguments.is_empty());
    Ok(())
}
#[test]
fn snapshots_rebind_reference_values_atomically_without_executing_code() -> Result<()> {
    let program = Arc::new(program(vec![
        assign(0, address()),
        assign(1, local(0)),
        Operation::Yield,
    ]));
    let sources = BTreeMap::from([(CodeOrigin(0), program.clone())]);
    let mut vm = Runtime::from_shared(program, CodeOrigin(0), vec![0, 0], "snapshot".into())?;
    vm.invoke_body(Thread::Main, &mut Service::default(), &mut [], 20)?;
    let mut snapshot = vm.snapshot(&sources)?;
    let before = serde_json::to_value(&snapshot)?;
    let mut calls = 0;
    assert!(
        snapshot
            .rebind_actor_references(|reference| {
                calls += 1;
                ensure!(calls == 1, "second reference cannot be rebound");
                Ok(ActorReference {
                    generation: 8,
                    ..reference
                })
            })
            .is_err()
    );
    assert_eq!(serde_json::to_value(&snapshot)?, before);
    snapshot.rebind_actor_references(|reference| {
        Ok(ActorReference {
            generation: 8,
            ..reference
        })
    })?;
    let restored = Runtime::from_snapshot(
        CodeOrigin(0),
        snapshot,
        &sources,
        &BTreeMap::new(),
        "restored".into(),
    )?;
    assert_eq!(
        restored.variables(),
        [ScriptValue::Actor(ActorReference {
            generation: 8,
            index: 4
        }); 2]
    );
    Ok(())
}

#[test]
fn reference_conditions_compare_identity_and_null_without_numeric_conversion() -> Result<()> {
    for (comparison, rhs, expected) in [
        (0, local(0), 1),
        (1, local(0), 2),
        (0, Value::Integer { value: 0 }, 2),
        (1, Value::Integer { value: 0 }, 1),
    ] {
        let mut vm = Runtime::new(
            program(vec![
                assign(0, address()),
                Operation::If {
                    conditions: vec![Condition {
                        lhs: local(0),
                        comparison,
                        rhs,
                        conjunction: 0,
                    }],
                    target: 4,
                },
                assign(1, Value::Integer { value: 1 }),
                Operation::Yield,
                assign(1, Value::Integer { value: 2 }),
                Operation::Yield,
            ]),
            "reference comparison".into(),
        )?;
        vm.invoke_body(Thread::Main, &mut Service::default(), &mut [], 20)?;
        assert_eq!(vm.variables()[1], expected);
    }
    Ok(())
}
