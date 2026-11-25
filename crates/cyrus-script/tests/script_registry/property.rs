use super::*;

pub(super) fn heading_property_scripts(reference: u16, object: Object) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec![Property::Address; usize::from(reference) + 1];
    scripts.definition.references[usize::from(reference)] = Property::AngleVy;
    scripts.instances[0].angles[1] = 1536;
    let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
    program.strings = vec!["logic".into()];
    program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Read {
                            place: Place::Property { object, reference },
                        },
                        rest: vec![],
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
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn heading_property_reads_installed_name_with_full_reference_and_receiver() {
    for reference in [0, 38, 294] {
        for (object, expected) in [
            (Object::Me, 1536),
            (Object::Player, 0),
            (Object::Named { string: 0 }, 1536),
        ] {
            let mut scripts = heading_property_scripts(reference, object);
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            world.prepare_spatial().unwrap();
            let player = world.id(2).unwrap();
            world
                .initialize_player_spatial(player, [0; 3], [0; 3])
                .unwrap();
            let caller = world.id(0).unwrap();
            let before = world.spatial(caller).unwrap().local;
            world
                .advance(
                    caller,
                    cyrus_script::script::ScriptStage::Body(Thread::Main),
                    cyrus_script::script::ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.spatial(caller).unwrap().local.angles, before.angles);
            assert_eq!(
                world.spatial(caller).unwrap().local.position,
                before.position
            );
        }
    }
}

#[test]
fn heading_property_rejects_missing_owner_unknown_name_and_unrepresentable_writes() {
    let scripts = heading_property_scripts(294, Object::Me);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let caller = world.id(0).unwrap();
    let error = world
        .advance(
            caller,
            ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("spatial owner unavailable"),
        "{error:#}"
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
}

#[test]
fn local_angle_properties_read_and_write_each_axis_without_refreshing_pose() {
    for ((axis, name), value) in [Property::AngleVx, Property::AngleVy, Property::AngleVz]
        .into_iter()
        .enumerate()
        .flat_map(|axis| [i32::MIN, -4097, -1, 512, 2048, i32::MAX].map(|value| (axis, value)))
    {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        scripts.instances[0].angles = [100, 1536, 300];
        let step = scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap();
        step.operation = Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: 294,
            },
            formula: Formula {
                first: Value::Integer { value },
                rest: vec![],
            },
        };
        let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
        program.steps.get_mut(&1).unwrap().operation = Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Read {
                    place: Place::Property {
                        object: Object::Me,
                        reference: 294,
                    },
                },
                rest: vec![],
            },
        };
        program.steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::Yield,
            },
        );
        program.steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::End { target: 0 },
            },
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        let caller = world.id(0).unwrap();
        let before = world.spatial(caller).unwrap().local;
        world
            .advance(
                caller,
                cyrus_script::script::ScriptStage::Body(Thread::Main),
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let after = world.spatial(caller).unwrap().local;
        let mut expected = before.angles;
        expected[axis] = value;
        assert_eq!(after.angles, expected, "{name}");
        assert_eq!(after.position, before.position);
        assert_eq!(
            after.matrix.0.map(|row| row.map(f32::to_bits)),
            before.matrix.0.map(|row| row.map(f32::to_bits))
        );
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [value]);
    }
}

#[test]
fn position_properties_read_local_coordinates_without_refreshing_pose() {
    for (axis, name) in [Property::PosVx, Property::PosVy, Property::PosVz]
        .into_iter()
        .enumerate()
    {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            let mut scripts = heading_property_scripts(294, object);
            scripts.definition.references[294] = name;
            scripts.instances[0].map_position = [-7, 11, -13];
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            world.prepare_spatial().unwrap();
            let caller = world.id(0).unwrap();
            let player = world.id(2).unwrap();
            world
                .initialize_player_spatial(player, [i32::MIN, -1, i32::MAX], [0; 3])
                .unwrap();
            let receiver = if selected { player } else { caller };
            let before = serde_json::to_value(world.spatial(receiver).unwrap()).unwrap();
            world
                .advance(
                    caller,
                    cyrus_script::script::ScriptStage::Body(Thread::Main),
                    cyrus_script::script::ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            let expected = if selected {
                [i32::MIN, -1, i32::MAX][axis]
            } else {
                [-7 * 256, 11 * 256, -13 * 256][axis]
            };
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(
                serde_json::to_value(world.spatial(receiver).unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn position_property_writes_change_only_the_selected_local_coordinate() {
    for (axis, name) in [Property::PosVx, Property::PosVy, Property::PosVz]
        .into_iter()
        .enumerate()
    {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            for value in [i32::MIN, -1, 0, 1, i32::MAX] {
                let mut scripts = heading_property_scripts(294, object.clone());
                scripts.definition.references[294] = name;
                let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
                let read = program.steps[&0].operation.clone();
                program.steps.insert(
                    0,
                    Step {
                        next: 1,
                        operation: Operation::Assign {
                            destination: Place::Property {
                                object: object.clone(),
                                reference: 294,
                            },
                            formula: Formula {
                                first: Value::Integer { value },
                                rest: vec![],
                            },
                        },
                    },
                );
                program.steps.insert(
                    1,
                    Step {
                        next: 2,
                        operation: read,
                    },
                );
                program.steps.insert(
                    2,
                    Step {
                        next: 3,
                        operation: Operation::End { target: 0 },
                    },
                );
                add_standalone_receivers(&mut scripts);
                let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
                world.prepare_spatial().unwrap();
                let caller = world.id(0).unwrap();
                let player = world.id(2).unwrap();
                world
                    .initialize_player_spatial(player, [123, -456, 789], [0; 3])
                    .unwrap();
                let receiver = if selected { player } else { caller };
                let other = if selected { caller } else { player };
                let mut expected = serde_json::to_value(world.spatial(receiver).unwrap()).unwrap();
                expected["local"]["position"][axis] = value.into();
                let other_before = serde_json::to_value(world.spatial(other).unwrap()).unwrap();
                world
                    .advance(
                        caller,
                        cyrus_script::script::ScriptStage::Body(Thread::Main),
                        cyrus_script::script::ScriptInputs {
                            frame_scale: Some(256),
                            nominal_ticks: Some(1),
                            ..Default::default()
                        },
                        None,
                        None,
                        20,
                    )
                    .unwrap();
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [value]);
                assert_eq!(
                    serde_json::to_value(world.spatial(receiver).unwrap()).unwrap(),
                    expected
                );
                assert_eq!(
                    serde_json::to_value(world.spatial(other).unwrap()).unwrap(),
                    other_before
                );
            }
        }
    }
}

#[test]
fn position_property_access_keeps_attached_effective_pose_until_family_refresh() {
    let mut scripts = heading_property_scripts(294, Object::Me);
    scripts.definition.references[294] = Property::PosVx;
    let mut child = scripts.actors["LOGIC"].clone();
    child.namespace = Some("child".into());
    child.attributes[Attribute::Slave] = 1;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Master] = 1;
    scripts.actors.insert("CHILD".into(), child);
    scripts.instances[0].map_position = [100, 0, 0];
    scripts.instances[1].map_position = [103, 4, 0];
    scripts.instances[1].actor = "CHILD".into();
    scripts.instances[0].angles = [0; 3];
    let program = &mut scripts.actors.get_mut("CHILD").unwrap().program;
    program.steps.insert(
        1,
        Step {
            next: 2,
            operation: Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: 294,
                },
                formula: Formula {
                    first: Value::Integer { value: -512 },
                    rest: vec![],
                },
            },
        },
    );
    program.steps.insert(
        2,
        Step {
            next: 3,
            operation: Operation::End { target: 0 },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let child = world.id(1).unwrap();
    assert_eq!(world.spatial(child).unwrap().local.position[0], 768);
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position[0],
        26368
    );
    let mut expected = serde_json::to_value(world.spatial(child).unwrap()).unwrap();
    expected["local"]["position"][0] = (-512).into();
    world
        .advance(
            child,
            cyrus_script::script::ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [768]);
    assert_eq!(
        world.model_position(child).unwrap(),
        Some([103.0, 4.0, 0.0])
    );
    assert_eq!(
        serde_json::to_value(world.spatial(child).unwrap()).unwrap(),
        expected
    );
    world.refresh_families().unwrap();
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position[0],
        25088
    );
    assert_eq!(world.model_position(child).unwrap(), Some([98.0, 4.0, 0.0]));
}

#[test]
fn position_property_rejects_unavailable_owner_and_unknown_reference() {
    for (name, prepare, expected) in [
        (Property::PosVx, false, "spatial owner unavailable"),
        (Property::Address, true, "unsupported"),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: 294,
            },
            formula: Formula {
                first: Value::Integer { value: i32::MIN },
                rest: vec![],
            },
        };
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        if prepare {
            world.prepare_spatial().unwrap();
        }
        let caller = world.id(0).unwrap();
        let before = prepare.then(|| serde_json::to_value(world.spatial(caller).unwrap()).unwrap());
        let error = world
            .advance(
                caller,
                cyrus_script::script::ScriptStage::Body(Thread::Main),
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        if let Some(before) = before {
            assert_eq!(
                serde_json::to_value(world.spatial(caller).unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn current_animation_property_reads_signed_group_without_advancing_playback() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (group, start, expected) in [
        (None, false, 0),
        (Some(7), false, -1),
        (Some(7), true, 7),
        (Some(32768), true, -32768),
    ] {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            let mut scripts = heading_property_scripts(294, object);
            scripts.definition.references[294] = Property::AnimCurrent;
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let caller = world.id(0).unwrap();
            let receiver = world.id(if selected { 2 } else { 0 }).unwrap();
            let data = Animations {
                version: 2,
                world: scripts.world,
                metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
                    "script_name":world.actors()[receiver.index].name(),
                    "animation_groups":[{"group_index":group,"anim_id":1,"anim_type":0,
                        "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]}]
                }]}),
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            };
            if let Some(group) = group {
                world
                    .bind_animation(receiver, &data, AnimationGroup::from_index(group))
                    .unwrap();
            }
            if start {
                world
                    .advance(
                        receiver,
                        ScriptStage::Animation,
                        ScriptInputs {
                            nominal_ticks: Some(1),
                            ..Default::default()
                        },
                        None,
                        None,
                        0,
                    )
                    .unwrap();
            }
            let before = world.inspect(receiver.index, 1)["actors"][0]["animation"].clone();
            world
                .advance(
                    caller,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(
                world.inspect(receiver.index, 1)["actors"][0]["animation"],
                before
            );
        }
    }
}

#[test]
fn torch_property_wraps_writes_and_reads_the_shared_counter_without_other_changes() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (value, expected) in [
        (-1, 255),
        (0, 0),
        (1, 1),
        (255, 255),
        (256, 0),
        (511, 255),
        (i32::MIN, 0),
        (i32::MAX, 255),
    ] {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            let mut scripts = heading_property_scripts(294, object.clone());
            scripts.definition.references[294] = Property::TorchTimer;
            let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
            let read = program.steps[&0].operation.clone();
            program.steps.insert(
                0,
                Step {
                    next: 1,
                    operation: Operation::Assign {
                        destination: Place::Property {
                            object,
                            reference: 294,
                        },
                        formula: Formula {
                            first: Value::Integer { value },
                            rest: vec![],
                        },
                    },
                },
            );
            program.steps.insert(
                1,
                Step {
                    next: 2,
                    operation: read,
                },
            );
            program.steps.insert(
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            );
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let caller = world.id(0).unwrap();
            let receiver = if selected { 2 } else { 0 };
            let other = if selected { 0 } else { 2 };
            let mut expected_control =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            expected_control["torch_burning"] = expected.into();
            let other_before =
                serde_json::to_value(world.actors()[other].control_state().unwrap()).unwrap();
            world
                .advance(
                    caller,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                expected_control
            );
            assert_eq!(
                serde_json::to_value(world.actors()[other].control_state().unwrap()).unwrap(),
                other_before
            );
        }
    }
}

#[test]
fn motion_byte_properties_read_and_write_the_actor_motion_bytes() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (name, field) in [
        (Property::RopeWait, "jump_selector"),
        (Property::SlideFlag, "sliding_state"),
        (Property::FallFlag, "falling_state"),
        (Property::JumpFlag, "queued_motion"),
        (Property::ExtraFlag, "living_state"),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
        let read = program.steps[&0].operation.clone();
        program.steps.insert(
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: 294,
                    },
                    formula: Formula {
                        first: Value::Integer { value: 7 },
                        rest: vec![],
                    },
                },
            },
        );
        program.steps.insert(
            1,
            Step {
                next: 2,
                operation: read,
            },
        );
        program.steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        );
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [7],
            "{name}"
        );
        let control = serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap();
        assert_eq!(control[field], 7, "{name}");
    }
}

#[test]
fn hide_rope_scabbard_and_grip_properties_use_the_installed_descriptors() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // (property, value written first, then read back)
    for (name, write, read) in [
        (Property::Hide, Some(1), true),
        (Property::Rope, None, true),
        (Property::Scabbard0, Some(42), true),
        (Property::Scabbard1, Some(-3), true),
        (Property::Grip0, Some(-77), true),
        (Property::Grip1, Some(70000), true),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
        let read_step = program.steps[&0].operation.clone();
        let mut steps = vec![];
        if let Some(value) = write {
            steps.push(Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: 294,
                },
                formula: Formula {
                    first: Value::Integer { value },
                    rest: vec![],
                },
            });
        }
        if read {
            steps.push(read_step);
        }
        steps.push(Operation::End { target: 0 });
        program.steps.clear();
        for (index, operation) in steps.into_iter().enumerate() {
            program.steps.insert(
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            );
        }
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_or_else(|error| panic!("{name}: {error:#}"));
        let control = serde_json::to_value(world.actors()[0].control_state().unwrap()).unwrap();
        match name {
            Property::Hide => {
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [1]);
                assert_eq!(control["hidden"], true);
            }
            Property::Rope => {
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
            }
            Property::Scabbard0 => {
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [42]);
                assert_eq!(control["scabbard_override"], serde_json::json!([42, null]));
            }
            Property::Grip0 => {
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [-77]);
                assert_eq!(control["grip_override"], serde_json::json!([-77, null]));
            }
            Property::Grip1 => {
                // The word keeps the low sixteen bits of the stored value.
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4464]);
                assert_eq!(control["grip_override"], serde_json::json!([null, 4464]));
            }
            _ => {
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [-3]);
                assert_eq!(control["scabbard_override"], serde_json::json!([null, -3]));
            }
        }
    }
}

#[test]
fn scabbard_properties_read_signed_installed_raex_words() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{AnimationData, ScriptInputs, ScriptStage};
    for (name, expected) in [(Property::Scabbard0, -32768), (Property::Scabbard1, 32767)] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        add_standalone_receivers(&mut scripts);
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
                "actors": [{"script_name": "LOGIC",
                    "raex": {"scabbard0": -32768, "scabbard1": 32767}}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    animations: Some(AnimationData(&data)),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
        assert_eq!(
            world.actors()[0].control_state().unwrap().scabbard_override,
            [None; 2]
        );
    }
}

#[test]
fn grip_properties_read_signed_installed_raex_words() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{AnimationData, ScriptInputs, ScriptStage};
    for (name, expected) in [(Property::Grip0, -32768), (Property::Grip1, 32767)] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        add_standalone_receivers(&mut scripts);
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
                "actors": [{"script_name": "LOGIC",
                    "raex": {"grip0": -32768, "grip1": 32767}}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    animations: Some(AnimationData(&data)),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
    }
}

#[test]
fn current_animation_property_without_an_association_returns_zero() {
    for object in [Object::Me, Object::Player, Object::Named { string: 0 }] {
        let mut scripts = heading_property_scripts(294, object);
        scripts.definition.references[294] = Property::AnimCurrent;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        world
            .advance(
                caller,
                cyrus_script::script::ScriptStage::Body(Thread::Main),
                cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    }
}

#[test]
fn animation_and_torch_properties_reject_unprepared_owners_and_unanimated_writes() {
    for (name, write, separate, expected) in [
        (Property::AnimCurrent, true, false, "has no animation"),
        (
            Property::TorchTimer,
            false,
            true,
            "control owner unavailable",
        ),
        (
            Property::TorchTimer,
            true,
            true,
            "control owner unavailable",
        ),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name;
        if separate {
            scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
        }
        if write {
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .program
                .steps
                .get_mut(&0)
                .unwrap()
                .operation = Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: 294,
                },
                formula: Formula {
                    first: Value::Integer { value: 255 },
                    rest: vec![],
                },
            };
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let error = world
            .advance(
                caller,
                cyrus_script::script::ScriptStage::Body(Thread::Main),
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
    }
}

#[test]
fn action_statement_resets_preserve_high_words_through_snapshot_restore() {
    use cyrus_script::script::ScriptInputs;
    for (name, arguments, expected) in [
        (Native::ResetAction, vec![-2147483647], [258, 514]),
        (Native::ResetAllAction, vec![], [0, 0]),
        (Native::SetAction, vec![1], [2147483907, 2147484162]),
    ] {
        let (mut world, _, _) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.definition.functions.push(Function {
                    name,
                    param_count: arguments.len() as u8,
                    kind: CallKind::Function,
                });
                let function_id = scripts.definition.functions.len() as u16 - 1;
                scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
                    (
                        0,
                        Step {
                            next: 1,
                            operation: wait(
                                function_id,
                                name,
                                CallKind::Function,
                                arguments
                                    .iter()
                                    .map(|&value| Value::Integer { value })
                                    .collect(),
                            ),
                        },
                    ),
                    (
                        1,
                        Step {
                            next: 2,
                            operation: Operation::End { target: 0 },
                        },
                    ),
                ]);
            },
            |saved| {
                saved["actors"][0]["control"]["directional_state"] =
                    serde_json::json!([2147483907u32, 2147484162u32]);
                saved["actors"][0]["control"]["jump_window"] = 27.into();
            },
        );
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.actions, expected, "{name}");
        assert_eq!(
            control.jump_window, 27,
            "duplicate SetAction and reset preserve timer"
        );
    }
}

#[test]
fn last_direction_reset_preserves_refusal_and_reads_the_taken_bypass_through_script() {
    use cyrus_game::cheats::{CheatStore, Cheats};
    use cyrus_script::script::{OrdinaryForwardAmountInputs, ScriptKeys};
    struct Memory;
    impl CheatStore for Memory {
        fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
            {
                let mut state = cyrus_assets::cheats::empty();
                // SYSTEM.INI seeds these slots at startup in every host.
                cyrus_assets::cheats::CheatDefaults {
                    task: true,
                    animation: true,
                    nodemarker: false,
                }
                .apply(&mut state)?;
                Ok(state)
            }
        }
        fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
            Ok(())
        }
    }
    for (enabled, known_keys, expected) in [
        (false, true, MotionDirection::FORWARD),
        (true, true, MotionDirection::NONE),
        (true, false, MotionDirection::FORWARD),
    ] {
        let (_, mut world, data, scene) = player_walking_fixture_with(|scripts| {
            scripts.updates.final_version = true;
            scripts.definition.references = vec![cyrus_soup::Property::LastDirection];
            let player = scripts.actors.get_mut("CYRUS").unwrap();
            player.attributes[Attribute::Collide] = 4;
            player.program.steps = BTreeMap::from([
                (
                    0,
                    Step {
                        next: 1,
                        operation: Operation::Assign {
                            destination: Place::Flag { index: 0 },
                            formula: Formula {
                                first: Value::Read {
                                    place: Place::Property {
                                        object: Object::Me,
                                        reference: 0,
                                    },
                                },
                                rest: vec![],
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
            ]);
        });
        let player = world.selected_player().unwrap();
        // Running is the production writer; the prepared script only reads its retained byte.
        let amount = world
            .ordinary_forward_amount(
                player,
                OrdinaryForwardAmountInputs {
                    selected_player: Some(player),
                    holding_weapon: false,
                    frame_scale: 256,
                    ticks: 1,
                    duration: 4,
                    elapsed: 0,
                },
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert!(amount > 0);
        assert_eq!(
            world.actors()[player.index]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::FORWARD
        );
        let mut cheats = Cheats::new(Memory);
        cheats
            .execute("yeahbaby", Some(u32::from(enabled)))
            .unwrap();
        let hud = cyrus_script::script::ScriptHud::default();
        hud.configure(
            &inventory_asset(&[]),
            &cyrus_game::preferences::Preferences::default(),
        );
        let held = cyrus_script::player::input::PlayerInput::default();
        world
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_script::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        frame_scale: Some(256),
                        fine_ticks: Some(1),
                        step_fps: Some(30),
                        shell_execution: Some(false),
                        player_controller: Some(cyrus_script::script::PlayerControllerInput {
                            held: &held,
                            walk_mode: true,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        ground_bypass: Some(cheats.ground_bypass().unwrap()),
                        script_keys: known_keys.then_some(ScriptKeys {
                            page_up: enabled,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        let actor = &world.actors()[player.index];
        if known_keys {
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
        }
        assert_eq!(
            actor.control_state().unwrap().motion_classification,
            expected,
            "enabled={enabled}, known_keys={known_keys}, failure={:?}",
            actor.failure()
        );
        if known_keys {
            world
                .advance(
                    player,
                    ScriptStage::Body(Thread::Main),
                    cyrus_script::script::ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap();
            assert_eq!(world.flags()[0], i32::from(expected.byte()));
        } else {
            assert!(actor.failure().unwrap().contains("debug navigation keys"));
        }
    }
}
