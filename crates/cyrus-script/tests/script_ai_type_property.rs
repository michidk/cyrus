//! Receiver-owned AI lifecycle selector reaches real public native producers and consumers.
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::*;
use cyrus_script::script::ScriptWorld;
use cyrus_soup::Attribute;
use cyrus_soup::program::*;
use std::collections::BTreeMap;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn ai_type_routes_unsigned_low_bytes_without_changing_adjacent_or_other_control() {
    for (value, expected) in [
        (0, 0),
        (6, 6),
        (8, 8),
        (128, 128),
        (207, 207),
        (-1, 255),
        (264, 8),
        (256, 0),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::AiType, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["ai_type"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .ai_type
                    .byte(),
                0
            );
        }
    }
}

#[test]
fn ai_type_allocation_zero_and_missing_owner_receiver_failures_are_distinct() {
    let scripts = counter_scripts(cyrus_soup::Property::AiType, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for missing_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::AiType,
                if missing_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                207,
                write,
            );
            if !missing_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if missing_receiver {
                "selected player unavailable"
            } else {
                "ai_type control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn native_fixture(object: Object, value: i32, health: u8, native_producer: bool) -> Scripts {
    use cyrus_soup::Native;
    let mut scripts = counter_scripts(cyrus_soup::Property::AiType, object.clone(), value, true);
    for (name, param_count) in [
        (Native::SetAiType, 1),
        (Native::MyAiType, 0),
        (Native::IsDead, 0),
    ] {
        scripts.definition.functions.push(Function {
            name,
            param_count,
            kind: CallKind::Function,
        });
    }
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::Health] = health;
    actor.program.variables = vec![4, 5, 6];
    actor.instance_variables = vec![vec![4, 5, 6], vec![9, 10, 11]];
    let mut old = std::mem::take(&mut actor.program.steps);
    let first = if native_producer {
        Operation::ObjectCall {
            call: Call {
                function_id: 1,
                function: Native::SetAiType,
                target: object.clone(),
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value }],
            },
        }
    } else {
        old.remove(&0).unwrap().operation
    };
    let assign_call = |index, function_id, function| Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first: Value::ObjectCall {
                call: Box::new(Call {
                    function_id,
                    function,
                    target: object.clone(),
                    kind: CallKind::Function,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: first,
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: old.remove(&1).unwrap().operation,
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: assign_call(1, 2, Native::MyAiType),
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: assign_call(2, 3, Native::IsDead),
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: old.remove(&2).unwrap().operation,
            },
        ),
        (
            5,
            Step {
                next: 6,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn public_property_and_setter_share_the_real_ai_and_death_consumers() {
    for native_producer in [false, true] {
        for (value, selector, dead) in [
            (6, 6, 0),
            (8, 8, 1),
            (264, 8, 1),
            (300, 44, 0),
            (-1, 255, 0),
        ] {
            for health in [0, 123] {
                for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
                    let scripts = native_fixture(object, value, health, native_producer);
                    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
                    for _ in 0..2 {
                        advance(&mut world, 1).unwrap();
                        assert_eq!(
                            world.actors()[1].runtime().unwrap().variables(),
                            [selector, selector, dead]
                        );
                        assert_eq!(world.flags(), [73]);
                        let control = world.actors()[receiver].control_state().unwrap();
                        assert_eq!(control.ai_type.byte(), selector as u8);
                        assert_eq!(control.ai_mode, 0);
                        assert!(!control.killed);
                        assert_eq!(control.script_control, 0);
                        assert_eq!(
                            world.actors()[receiver].attributes()[Attribute::Health],
                            health
                        );
                        assert_eq!(
                            world.actors()[1 - receiver]
                                .control_state()
                                .unwrap()
                                .ai_type
                                .byte(),
                            0
                        );
                    }
                }
            }
        }
    }
}
