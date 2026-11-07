//! `last_pos`, `start_pos` and `last_angle` are the previous position, the saved start and the angle history.
use super::*;
use cyrus_script::script::{ScriptInputs, ScriptStage};

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
    try_pose_world(steps, |_| {}, false).unwrap()
}

/// `edit` changes the scripts before the world is prepared; `refresh` runs one family pass before
/// the script does.
fn try_pose_world(
    steps: &[Step3],
    edit: impl FnOnce(&mut Scripts),
    refresh: bool,
) -> anyhow::Result<ScriptWorld> {
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
    edit(&mut scripts);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    if refresh {
        world.refresh_families().unwrap();
    }
    let id = world.id(0).unwrap();
    world.advance(
        id,
        ScriptStage::OrdinaryMain,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            player_query: Some(cyrus_script::script::QueryInputs {
                player: None,
                distance_override: None,
                fallback_position: None,
                linked_position: None,
            }),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    Ok(world)
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

#[test]
fn start_angle_is_the_creation_angles_reset_rotation_restores() {
    let before = pose_world(&[
        Step3::Read(Property::StartAngleVx),
        Step3::Read(Property::AngleVx),
    ]);
    let [start, angle] = locals(&before)[..] else {
        unreachable!()
    };
    assert_eq!(start, angle, "an actor is created with its start angles");
    // SetRotation moves the angle and the history but not the creation angles.
    let world = pose_world(&[
        Step3::Call(Native::SetRotation, vec![100, 200, 300]),
        Step3::Read(Property::StartAngleVx),
        Step3::Read(Property::StartAngleVy),
        Step3::Read(Property::StartAngleVz),
    ]);
    assert_eq!(locals(&world)[1..], [start, 0, 0]);
    // A raw write replaces the creation angles, and ResetRotation then restores that component.
    let world = pose_world(&[
        Step3::Write(Property::StartAngleVy, 1234),
        Step3::Call(Native::SetRotation, vec![10, 20, 30]),
        Step3::Call(Native::ResetRotation, vec![0, 1, 0]),
        Step3::Read(Property::StartAngleVy),
        Step3::Read(Property::AngleVx),
        Step3::Read(Property::AngleVy),
        Step3::Read(Property::AngleVz),
    ]);
    assert_eq!(locals(&world)[2..], [1234, 10, 1234, 30]);
}

#[test]
fn last_valid_record_is_the_stable_position_the_recovery_restores() {
    // The flag is a byte: a read gives zero or one here, a nonzero write sets it, and a write whose
    // low byte is zero clears it. The three dwords are the position the recovery restores.
    let world = pose_world(&[
        Step3::Read(Property::LastValidFlag),
        Step3::Write(Property::LastValidPosVx, -512),
        Step3::Write(Property::LastValidPosVy, 4096),
        Step3::Write(Property::LastValidPosVz, i32::MAX),
        Step3::Write(Property::LastValidFlag, 7),
        Step3::Read(Property::LastValidFlag),
        Step3::Read(Property::LastValidPosVx),
        Step3::Read(Property::LastValidPosVy),
        Step3::Read(Property::LastValidPosVz),
    ]);
    assert_eq!(locals(&world), [0, 1, -512, 4096, i32::MAX]);
    let control = world.actors()[0].control_state().unwrap();
    assert!(control.stable_position_valid);
    assert_eq!(control.stable_local_position, [-512, 4096, i32::MAX]);
    let world = pose_world(&[
        Step3::Write(Property::LastValidFlag, 1),
        Step3::Write(Property::LastValidFlag, 256),
        Step3::Read(Property::LastValidFlag),
    ]);
    assert_eq!(locals(&world), [0]);
    assert!(
        !world.actors()[0]
            .control_state()
            .unwrap()
            .stable_position_valid
    );
}

#[test]
fn move_properties_are_the_retained_motion_triple_the_rope_release_speed_shares() {
    let world = pose_world(&[
        Step3::Write(Property::MoveVx, -3),
        Step3::Write(Property::MoveVy, 70_000),
        Step3::Write(Property::MoveVz, i32::MIN),
        Step3::Read(Property::MoveVx),
        Step3::Read(Property::MoveVy),
        Step3::Read(Property::MoveVz),
    ]);
    assert_eq!(locals(&world), [-3, 70_000, i32::MIN]);
    let control = world.actors()[0].control_state().unwrap();
    // The rope release's jump-off speed is the last component of the same triple.
    assert_eq!(control.motion_vector, [-3, 70_000, i32::MIN]);
}

#[test]
fn ov_is_the_held_ledge_point_and_shares_the_heading_word_with_move() {
    // The ledge entry stores the point in ov and the heading in the second word of move; the
    // two properties are plain dwords of one actor.
    let world = pose_world(&[
        Step3::Write(Property::OvVx, 5120),
        Step3::Write(Property::OvVy, -768),
        Step3::Write(Property::OvVz, 9),
        Step3::Write(Property::MoveVy, 1500),
        Step3::Read(Property::OvVx),
        Step3::Read(Property::OvVy),
        Step3::Read(Property::OvVz),
        Step3::Read(Property::MoveVy),
    ]);
    assert_eq!(locals(&world), [5120, -768, 9, 1500]);
    let world = pose_world(&[Step3::Read(Property::OvVy)]);
    assert_eq!(
        locals(&world),
        [0],
        "an actor holds no ledge point at creation"
    );
}

/// Actor 0 is a slave of actor 1 (rotated a quarter turn), as `AttachMe` leaves a family.
fn attach_slave(scripts: &mut Scripts) {
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.attributes[Attribute::Slave] = 1;
    let mut master = slave.clone();
    master.attributes[Attribute::Slave] = 0;
    master.attributes[Attribute::Master] = 1;
    master.header_index = 1;
    master.instances = 1;
    master.namespace = Some("master".into());
    master.instance_variables = vec![vec![0; master.program.variables.len()]];
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.instances = 1;
    slave.instance_variables.truncate(1);
    scripts.instances[1].actor = "MASTER".into();
    scripts.actors.insert("MASTER".into(), master);
    scripts.instances[0].angles = [10, 20, 30];
    scripts.instances[1].angles = [0, 512, 0];
}

#[test]
fn slave_properties_are_the_family_composed_pose_and_the_one_before_it() {
    let reads = [
        Step3::Read(Property::SlavePosVx),
        Step3::Read(Property::SlavePosVy),
        Step3::Read(Property::SlavePosVz),
        Step3::Read(Property::SlaveAngleVx),
        Step3::Read(Property::SlaveAngleVy),
        Step3::Read(Property::SlaveAngleVz),
        Step3::Read(Property::LastSlavePosVx),
        Step3::Read(Property::LastSlaveAngleVy),
    ];
    let world = try_pose_world(&reads, attach_slave, true).unwrap();
    let id = world.id(0).unwrap();
    let spatial = world.spatial(id).unwrap();
    let effective = spatial.effective.unwrap();
    assert_eq!(locals(&world)[..3], effective.position);
    assert_eq!(locals(&world)[3..6], effective.angles);
    assert_eq!(
        locals(&world)[6],
        spatial.previous_effective_position.unwrap()[0]
    );
    assert_eq!(
        locals(&world)[7],
        spatial.previous_effective_angles.unwrap()[1]
    );
    assert_eq!(spatial.parent, Some(1));
    // Before any family pass the previous pose does not exist yet, and the error names the property.
    let error = try_pose_world(&reads[6..7], attach_slave, false)
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("last_slave_pos.vx"),
        "{error:#}"
    );
}

#[test]
fn slave_position_writes_reach_the_effective_position_distance_queries_use() {
    let distance = |write: Option<i32>| {
        let mut steps = vec![Step3::Read(Property::SlavePosVx)];
        steps.extend(write.map(|value| Step3::Write(Property::SlavePosVx, value)));
        steps.push(Step3::Call(Native::DistanceToLocation, vec![0]));
        let world = try_pose_world(&steps, attach_slave, true).unwrap();
        (locals(&world), world)
    };
    let (before, _) = distance(None);
    assert_eq!(before[1], 0, "the composed pose is at the home location");
    let (after, world) = distance(Some(before[0] + 5 * 256));
    assert_eq!(after[1], 5 * 256);
    let id = world.id(0).unwrap();
    assert_eq!(
        world.spatial(id).unwrap().query_pose().unwrap().position[0],
        before[0] + 5 * 256
    );
    // The previous pose and the angles are plain whole-word fields too.
    let world = try_pose_world(
        &[
            Step3::Write(Property::LastSlavePosVz, -9),
            Step3::Write(Property::SlaveAngleVy, 1000),
            Step3::Write(Property::LastSlaveAngleVx, 77),
            Step3::Read(Property::LastSlavePosVz),
            Step3::Read(Property::SlaveAngleVy),
            Step3::Read(Property::LastSlaveAngleVx),
        ],
        attach_slave,
        true,
    )
    .unwrap();
    assert_eq!(locals(&world), [-9, 1000, 77]);
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(spatial.effective.unwrap().angles[1], 1000);
}
/// The selected player restored in `living_state` with its saved `ov` point and `move.vy` heading,
/// plus an optional legacy global anchor; returns the restored world and the player's `control`.
fn restored_ledge_player(
    living_state: u8,
    saved: Option<([i32; 3], i32)>,
    legacy: Option<([i32; 3], u16)>,
) -> (serde_json::Value, usize) {
    let (scripts, world, mut data, _) = player_health_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| {
            matches!(
                actor.source,
                cyrus_script::script::ScriptActorSource::Player
            )
        })
        .unwrap();
    let mut world =
        replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved_world| {
            let control = &mut saved_world["actors"][index]["control"];
            control["living_state"] = living_state.into();
            match saved {
                Some((point, heading)) => {
                    control["ledge_point"] = serde_json::json!(point);
                    control["motion_vector"][1] = heading.into();
                }
                // A save from before the field existed has no such key.
                None => {
                    control.as_object_mut().unwrap().remove("ledge_point");
                }
            }
        });
    if let Some((point, heading)) = legacy {
        world
            .globals_mut()
            .set_legacy_ledge_anchor_for_test(point, heading);
    }
    let again = replace_health_test_animation(world, &scripts, &mut data, index, 0, |_| {});
    let control = movement_snapshot(&again, &scripts, &data)["actors"][index]["control"].clone();
    (control, index)
}

#[test]
fn legacy_saved_ledge_point_moves_into_the_players_ov_and_move_heading() {
    // Hanging state (12) with the point and heading in the old global.
    let (control, _) = restored_ledge_player(12, None, Some(([5120, -768, 9], 1500)));
    assert_eq!(control["ledge_point"], serde_json::json!([5120, -768, 9]));
    assert_eq!(control["motion_vector"][1], 1500);
    // Outside the ledge states the stale global is ignored.
    let (control, _) = restored_ledge_player(0, None, Some(([5120, -768, 9], 1500)));
    assert_eq!(control["ledge_point"], serde_json::json!([0, 0, 0]));
    assert_ne!(control["motion_vector"][1], 1500);
}

#[test]
fn actor_held_ledge_point_survives_restores_and_wins_over_a_legacy_global() {
    // Saved in the actor: survives a second restore into a new generation, and the global is
    // ignored because the actor already holds a point.
    let (control, _) = restored_ledge_player(10, Some(([1, 2, 3], 700)), Some(([9, 9, 9], 1)));
    assert_eq!(control["ledge_point"], serde_json::json!([1, 2, 3]));
    assert_eq!(control["motion_vector"][1], 700);
}

#[test]
fn dead_and_weapon_drawn_are_the_killed_marker_and_the_drawn_byte() {
    let world = pose_world(&[
        Step3::Read(Property::Dead),
        Step3::Read(Property::WeaponDrawn),
        Step3::Write(Property::Dead, 0x100 + 1),
        Step3::Write(Property::WeaponDrawn, 2),
        Step3::Read(Property::Dead),
        Step3::Read(Property::WeaponDrawn),
    ]);
    // Bytes read zero or one here, any nonzero low byte sets them.
    assert_eq!(locals(&world), [0, 0, 1, 1]);
    let control = world.actors()[0].control_state().unwrap();
    assert!(control.killed && control.weapon_drawn);
    let world = pose_world(&[
        Step3::Write(Property::Dead, 1),
        Step3::Write(Property::Dead, 0x200),
        Step3::Read(Property::Dead),
    ]);
    assert_eq!(locals(&world), [0]);
    assert!(!world.actors()[0].control_state().unwrap().killed);
}

#[test]
fn a_script_that_marks_itself_dead_is_skipped_by_later_ordinary_updates() {
    // Ordinary script updates skip a receiver whose killed marker is set, so the counter the
    // script increments before marking itself dead stops at one.
    let mut scripts = cache();
    scripts.definition.references = vec![Property::Dead];
    let logic = scripts.actors.get_mut("LOGIC").unwrap();
    logic.program.variables = vec![0];
    logic.instance_variables = vec![vec![0]; 2];
    logic.program.steps = [
        Operation::Increment {
            destination: Place::Local { index: 0 },
            delta: 1,
        },
        Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: 0,
            },
            formula: Formula {
                first: int(1),
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]
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
    for _ in 0..3 {
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
    }
    assert_eq!(locals(&world), [1]);
    assert!(world.actors()[0].control_state().unwrap().killed);
}

#[test]
fn invisible_is_a_draw_only_flag_the_script_keeps_running_under() {
    let world = pose_world(&[
        Step3::Read(Property::Invisible),
        Step3::Write(Property::Invisible, 0x103),
        Step3::Read(Property::Invisible),
        Step3::Write(Property::Dead, 0),
    ]);
    assert_eq!(locals(&world), [0, 1]);
    // It is saved with the actor and, unlike `dead`, does not stop script updates.
    assert!(world.actors()[0].control_state().unwrap().invisible);
    assert!(!world.actors()[0].control_state().unwrap().killed);
}
