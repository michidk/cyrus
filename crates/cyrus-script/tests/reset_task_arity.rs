use cyrus_script::program::*;
use std::collections::BTreeMap;

#[test]
fn only_literal_self_reset_task_can_exceed_its_zero_argument_declaration() {
    let definition = Definition {
        functions: vec![
            Function {
                name: cyrus_script::Native::NullFunction,
                param_count: 0,
                kind: CallKind::Function,
            },
            Function {
                name: cyrus_script::Native::ResetAllAction,
                param_count: 0,
                kind: CallKind::Function,
            },
        ],
        flags: vec![],
        references: vec![],
        attributes: vec![],
    };
    let call = Call {
        function_id: 1,
        function: cyrus_script::Native::ResetAllAction,
        target: Object::Me,
        kind: CallKind::Task,
        arguments: vec![Value::Integer { value: 23 }, Value::Integer { value: -4 }],
    };
    let program = |call: Call, object: bool| Program {
        version: 4,
        entry: 0,
        variables: vec![],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: if object {
                        Operation::ObjectCall { call }
                    } else {
                        Operation::Call { call }
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
    };
    program(call.clone(), false)
        .validate_definition(&definition)
        .unwrap();
    assert!(
        program(call.clone(), true)
            .validate_definition(&definition)
            .is_err()
    );
    for kind in [CallKind::Function, CallKind::Multitask] {
        let mut changed = call.clone();
        changed.kind = kind;
        assert!(
            program(changed, false)
                .validate_definition(&definition)
                .is_err()
        );
    }
    let mut changed = call.clone();
    changed.arguments.pop();
    assert!(
        program(changed, false)
            .validate_definition(&definition)
            .is_err()
    );
    let mut changed = call.clone();
    changed.function_id = 2;
    assert!(
        program(changed, false)
            .validate_definition(&definition)
            .is_err()
    );
    let mut changed_definition = definition.clone();
    changed_definition.functions[1].name = cyrus_script::Native::TrueFunction;
    let mut changed = call.clone();
    changed.function = cyrus_script::Native::TrueFunction;
    assert!(
        program(changed, false)
            .validate_definition(&changed_definition)
            .is_err()
    );
    let mut changed = call.clone();
    changed.arguments[0] = Value::Read {
        place: Place::Flag { index: 0 },
    };
    assert!(
        program(changed, false)
            .validate_definition(&definition)
            .is_err()
    );
    assert!(
        program(call.clone(), false)
            .validate_native_hook(&call, &definition)
            .is_err()
    );
}
