//! `last_pos`, `start_pos` and `last_angle` are the previous position, the saved start and the angle history.
use super::*;
use cyrus_game::script::{ScriptInputs, ScriptStage};

fn int(value: i32) -> Value {
    Value::Integer { value }
}

/// One step of a script on actor 0: a property write, a property read into a local, or a native
/// call whose result goes to a local.
enum Step3 {
    Write(Property, i32),
    Read(Property),
    Call(Native, Vec<i32>),
}

fn pose_world(steps: &[Step3]) -> ScriptWorld {
    let mut scripts = cache();
    let mut properties: Vec<Property> = vec![];
    let mut natives: Vec<Native> = vec![];
    for step in steps {
        match step {
            Step3::Write(property, _) | Step3::Read(property) => {
                if !properties.contains(property) {
                    properties.push(*property)
                }
            }
            Step3::Call(native, _) => {
                if !natives.contains(native) {
                    natives.push(*native)
                }
            }
        }
    }
    let first_function = scripts.definition.functions.len();
    for native in &natives {
        let arguments = steps
            .iter()
            .find_map(|step| match step {
                Step3::Call(n, arguments) if n == native => Some(arguments.len()),
                _ => None,
            })
            .unwrap();
        scripts.definition.functions.push(Function {
            name: *native,
            param_count: arguments as u8,
            kind: CallKind::Function,
        });
    }
    scripts.definition.references = properties.clone();
    let reference =
        |property: Property| properties.iter().position(|p| *p == property).unwrap() as u16;
    let reads = steps
        .iter()
        .filter(|step| !matches!(step, Step3::Write(..)))
        .count();
    let mut operations = vec![];
    let mut slot = 0u8;
    for step in steps {
        match step {
            Step3::Write(property, value) => operations.push(Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: reference(*property),
                },
                formula: Formula {
                    first: int(*value),
                    rest: vec![],
                },
            }),
            Step3::Read(property) => {
                operations.push(Operation::Assign {
                    destination: Place::Local { index: slot },
                    formula: Formula {
                        first: Value::Read {
                            place: Place::Property {
                                object: Object::Me,
                                reference: reference(*property),
                            },
                        },
                        rest: vec![],
                    },
                });
                slot += 1;
            }
            Step3::Call(native, arguments) => {
                let function_id =
                    (first_function + natives.iter().position(|n| n == native).unwrap()) as u16;
                operations.push(Operation::Assign {
                    destination: Place::Local { index: slot },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id,
                                function: *native,
                                target: Object::Me,
                                kind: CallKind::Function,
                                arguments: arguments.iter().map(|v| int(*v)).collect(),
                            }),
                        },
                        rest: vec![],
                    },
                });
                slot += 1;
            }
        }
    }
    operations.push(Operation::End { target: 0 });
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.program.variables = vec![0; reads.max(1)];
    logic.instance_variables = vec![vec![0; reads.max(1)]; 2];
    logic.program.steps = operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
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
            100,
        )
        .unwrap();
    world
}

fn locals(world: &ScriptWorld) -> Vec<i32> {
    world.actors()[0]
        .runtime()
        .unwrap()
        .variables()
        .iter()
        .map(|value| value.integer().unwrap())
        .collect()
}

#[test]
fn last_pos_is_the_previous_position_set_position_stores() {
    // SetPosition copies its arguments to the local position and to the previous position.
    let world = pose_world(&[
        Step3::Call(Native::SetPosition, vec![1000, -2000, 3000]),
        Step3::Read(Property::LastPosVx),
        Step3::Read(Property::LastPosVy),
        Step3::Read(Property::LastPosVz),
        Step3::Read(Property::PosVx),
    ]);
    assert_eq!(locals(&world), [0, 1000, -2000, 3000, 1000]);
    // A raw write moves only the previous position.
    let world = pose_world(&[
        Step3::Write(Property::LastPosVx, 77),
        Step3::Write(Property::LastPosVz, -5),
        Step3::Read(Property::LastPosVx),
        Step3::Read(Property::LastPosVz),
        Step3::Read(Property::PosVx),
    ]);
    let id = world.id(0).unwrap();
    let spatial = world.spatial(id).unwrap();
    assert_eq!(spatial.previous_local_position[0], 77);
    assert_eq!(spatial.previous_local_position[2], -5);
    assert_eq!(spatial.local.position[0], locals(&world)[2]);
    assert_eq!(locals(&world)[..2], [77, -5]);
}

#[test]
fn start_pos_is_the_saved_start_distance_from_start_measures() {
    // DistanceFromStart(axis) is (start - local) >> 8: moving the saved start by whole units
    // shows through it, and the property reads back the same triple.
    let before = pose_world(&[
        Step3::Read(Property::StartPosVx),
        Step3::Read(Property::PosVx),
    ]);
    let [start, local] = locals(&before)[..] else {
        unreachable!()
    };
    assert_eq!(start, local, "an actor starts at its saved start");
    let world = pose_world(&[
        Step3::Write(Property::StartPosVx, local + 3 * 256 + 17),
        Step3::Read(Property::StartPosVx),
        Step3::Call(Native::DistanceFromStart, vec![0]),
    ]);
    assert_eq!(locals(&world), [local + 3 * 256 + 17, 3]);
}

#[test]
fn last_angle_is_the_angle_history_set_rotation_stores() {
    let world = pose_world(&[
        Step3::Call(Native::SetRotation, vec![100, 2047 + 5, 300]),
        Step3::Read(Property::LastAngleVx),
        Step3::Read(Property::LastAngleVy),
        Step3::Read(Property::LastAngleVz),
    ]);
    // SetRotation masks each angle to eleven bits and stores it as the local angle and the history.
    assert_eq!(locals(&world), [0, 100, 4, 300]);
    let world = pose_world(&[
        Step3::Write(Property::LastAngleVy, -9),
        Step3::Read(Property::LastAngleVy),
        Step3::Read(Property::AngleVy),
    ]);
    assert_eq!(locals(&world)[0], -9);
    assert_ne!(locals(&world)[1], -9, "the local angle is not the history");
}
