//! Receiver-owned runtime script-control byte uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use std::collections::BTreeMap;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn disable_script_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::DisableScript, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["script_control"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .script_control,
                0
            );
        }
    }
}

#[test]
fn disable_script_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::DisableScript, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::DisableScript,
                if absent_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                48,
                write,
            );
            if !absent_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if absent_receiver {
                "Player property selected player unavailable"
            } else {
                "disable_script control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn gate_fixture(flags: i32) -> ScriptWorld {
    let mut scripts = counter_scripts(cyrus_soup::Property::DisableScript, Object::Me, flags, true);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 4 };
    actor.program.steps.insert(
        4,
        Step {
            next: 5,
            operation: Operation::Assign {
                destination: Place::Flag { index: 0 },
                formula: Formula {
                    first: Value::Integer { value: 99 },
                    rest: vec![],
                },
            },
        },
    );
    actor.program.steps.insert(
        5,
        Step {
            next: 6,
            operation: Operation::End { target: 4 },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.flags(), [73]);
    world
}

#[test]
fn property_controls_real_ordinary_main_without_stopping_explicit_body() {
    for (flags, expected) in [
        (0, 99),
        (1, 73),
        (2, 99),
        (4, 99),
        (5, 73),
        (128, 99),
        (129, 73),
    ] {
        let mut world = gate_fixture(flags);
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap();
        assert_eq!(world.flags(), [expected]);
        assert_eq!(
            world.actors()[0].control_state().unwrap().script_control,
            flags as u8
        );
        assert!(world.actors()[0].failure().is_none());
    }
}

#[test]
fn native_enable_script_changes_only_its_mask_in_property_seeded_byte() {
    let mut scripts = counter_scripts(cyrus_soup::Property::DisableScript, Object::Me, 133, true);
    scripts.definition.functions.push(Function {
        name: cyrus_soup::Native::EnableScript,
        param_count: 0,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let mut old = std::mem::take(&mut actor.program.steps);
    actor.program.steps = BTreeMap::from([
        (0, old.remove(&0).unwrap()),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Call {
                    call: Call {
                        function_id: 1,
                        function: cyrus_soup::Native::EnableScript,
                        target: Object::Me,
                        kind: CallKind::Function,
                        arguments: vec![],
                    },
                },
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: old.remove(&1).unwrap().operation,
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: old.remove(&2).unwrap().operation,
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: Operation::End { target: 2 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [132]);
    assert_eq!(
        world.actors()[0].control_state().unwrap().script_control,
        132
    );
    assert_eq!(world.actors()[1].control_state().unwrap().script_control, 0);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn missing_task_timing_fails_before_ordinary_continuation_then_valid_retry_runs() {
    let mut world = gate_fixture(0);
    let id = world.id(0).unwrap();
    let error = world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs::default(),
            None,
            None,
            30,
        )
        .unwrap_err();
    assert!(error.to_string().contains("requires frame_scale"));
    assert_eq!(world.flags(), [73]);
    assert_eq!(world.actors()[0].control_state().unwrap().script_control, 0);
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.flags(), [99]);
}
