use super::*;

#[test]
fn researched_attribute_query_reads_unsigned_bytes_and_checks_bounds() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::GetMyAttr,
        param_count: 1,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::Active] = 255;
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: Native::GetMyAttr,
                    arguments: vec![Value::Integer { value: 0 }],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [255]);
    for index in [-1, 256] {
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        let Operation::Assign { formula, .. } =
            &mut actor.program.steps.get_mut(&0).unwrap().operation
        else {
            unreachable!()
        };
        let Value::Call { call } = &mut formula.first else {
            unreachable!()
        };
        call.arguments = vec![Value::Integer { value: index }];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [0]);
    }
}

pub(super) fn attribute_call(id: u16, name: Native, index: i32, value: Option<i32>) -> Call {
    let mut arguments = vec![Value::Integer { value: index }];
    if let Some(value) = value {
        arguments.push(Value::Integer { value });
    }
    Call {
        function_id: id,
        target: Object::Me,
        kind: CallKind::Function,
        function: name,
        arguments,
    }
}

pub(super) fn attribute_cache(operations: Vec<Operation>) -> Scripts {
    let mut scripts = wait_cache(operations);
    scripts.definition.functions.truncate(1);
    for (name, param_count) in [
        (Native::SetAttribute, 2),
        (Native::SetMyAttr, 2),
        (Native::GetAttribute, 1),
        (Native::GetMyAttr, 1),
    ] {
        scripts.definition.functions.push(Function {
            name,
            kind: CallKind::Function,
            param_count,
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn attribute_writes_truncate_and_only_set_attribute_updates_backup_bytes() {
    let scripts = attribute_cache(vec![
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(attribute_call(1, Native::SetAttribute, 1, Some(-2))),
                },
                rest: vec![],
            },
        },
        Operation::Yield,
        Operation::Call {
            call: attribute_call(1, Native::SetAttribute, 6, Some(0x123)),
        },
        Operation::Call {
            call: attribute_call(2, Native::SetMyAttr, 1, Some(511)),
        },
        Operation::Call {
            call: attribute_call(2, Native::SetMyAttr, 6, Some(-1)),
        },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(attribute_call(3, Native::GetAttribute, 115, None)),
                },
                rest: vec![(
                    1,
                    Value::Call {
                        call: Box::new(attribute_call(4, Native::GetMyAttr, 119, None)),
                    },
                )],
            },
        },
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [289]);
    let mut expected = [0; 256];
    expected[1] = 255;
    expected[6] = 255;
    expected[115] = 35;
    expected[119] = 254;
    assert_eq!(world.actors()[0].attributes().bytes(), &expected);
    assert_eq!(world.actors()[1].attributes().bytes(), &[0; 256]);
}

#[test]
fn set_my_attribute_ignores_out_of_range_indices_without_mutation() {
    for index in [i32::MIN, -1, 256, i32::MAX] {
        let scripts = attribute_cache(vec![
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(attribute_call(2, Native::SetMyAttr, index, Some(5))),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
        assert_eq!(world.actors()[0].attributes().bytes(), &[0; 256]);
    }
}

#[test]
fn routed_attribute_writes_use_receiver_locals_and_are_visible_in_the_same_stage() {
    let mut write = attribute_call(1, Native::SetAttribute, 1, Some(0));
    write.target = Object::Named { string: 0 };
    write.arguments[1] = Value::Read {
        place: Place::Local { index: 0 },
    };
    let mut read = attribute_call(3, Native::GetAttribute, 119, None);
    read.target = Object::Named { string: 0 };
    let mut scripts = attribute_cache(vec![
        Operation::ObjectCall { call: write },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(read),
                },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["LoGiC".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.step(world.id(1).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 4);
    assert_eq!(
        world.actors()[0].attributes()[Attribute::ReservedCollide],
        4
    );
    assert_eq!(world.actors()[1].attributes().bytes(), &[0; 256]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [4]);
}

#[test]
fn unsupported_attribute_access_fails_before_mutation_and_remains_sticky() {
    for (id, name, index, value, reason) in [
        (1, Native::SetAttribute, -1, Some(9), "index outside"),
        (1, Native::SetAttribute, 256, Some(9), "index outside"),
        (3, Native::GetAttribute, 256, None, "index outside"),
    ] {
        let scripts = attribute_cache(vec![
            Operation::Call {
                call: attribute_call(id, name, index, value),
            },
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let actor = world.id(0).unwrap();
        let error = world.step(actor, Thread::Main, 20).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
        assert!(
            error.contains("world 7") && error.contains("instruction 0"),
            "{error}"
        );
        assert_eq!(world.actors()[0].attributes().bytes(), &[0; 256]);
        assert_eq!(world.flags(), [12]);
        assert_eq!(
            world.step(actor, Thread::Main, 20).unwrap_err().to_string(),
            error
        );
    }
}

#[test]
fn set_attribute_forty_four_is_refused_for_a_dead_actor_without_a_combat_record() {
    for dead in [false, true] {
        let mut ops = Vec::new();
        if dead {
            ops.push(Operation::Call {
                call: Call {
                    function_id: 5,
                    function: Native::KillMe,
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![],
                },
            });
        }
        ops.push(Operation::Call {
            call: attribute_call(1, Native::SetAttribute, 44, Some(0x123)),
        });
        ops.push(Operation::End { target: 0 });
        let mut scripts = attribute_cache(ops);
        scripts.definition.functions.push(Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Combat] = 1;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let mut sink = |_, _| Ok(());
        let _ = world.advance(
            id,
            cyrus_script::script::ScriptStage::OrdinaryMain,
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut sink),
            None,
            100,
        );
        let actor = &world.actors()[0];
        if dead {
            // A dead actor releases its nodes and stores the default in its combat record, which
            // this fixture never initialized.
            assert!(
                format!("{:?}", actor.failure()).contains("without a combat record"),
                "{:?}",
                actor.failure()
            );
        } else {
            assert_eq!(actor.attributes()[Attribute::PanicGroup], 0x23);
            assert_eq!(actor.control_state().unwrap().combat_death_default, None);
        }
    }
}
