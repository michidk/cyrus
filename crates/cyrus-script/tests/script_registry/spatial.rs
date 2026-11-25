use super::*;

#[test]
fn spatial_axis_locks_restore_independent_saved_axes_and_leave_the_matrix_unchanged() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let template = world.spatial(world.id(0).unwrap()).unwrap();
    for (attribute, position, angles) in [
        (Attribute::StopXMove, [100, 500, 600], [400, 500, 600]),
        (Attribute::StopYMove, [400, 200, 600], [400, 500, 600]),
        (Attribute::StopZMove, [400, 500, 300], [400, 500, 600]),
        (Attribute::StopXRotate, [400, 500, 600], [100, 500, 600]),
        (Attribute::StopYRotate, [400, 500, 600], [400, 200, 600]),
        (Attribute::StopZRotate, [400, 500, 600], [400, 500, 300]),
    ] {
        for slave in [0, 1] {
            let mut pose = template.clone();
            pose.local.position = [100, 200, 300];
            pose.local.angles = [100, 200, 300];
            pose.snapshot();
            pose.local.position = [400, 500, 600];
            pose.local.angles = [400, 500, 600];
            let matrix = pose.local.matrix;
            let mut attributes = Attributes::default();
            attributes[attribute] = 255;
            attributes[Attribute::Slave] = slave;
            pose.restore_axis_locks(&attributes).unwrap();
            assert_eq!(
                pose.local.position,
                if slave == 0 {
                    position
                } else {
                    [400, 500, 600]
                }
            );
            assert_eq!(
                pose.local.angles,
                if slave == 0 { angles } else { [400, 500, 600] }
            );
            assert_eq!(pose.local.matrix, matrix);
        }
    }
}

#[test]
fn in_rectangle_tests_the_receiver_position_with_normalized_inclusive_corners() {
    let origin = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    let (x, z) = (origin[0], origin[2]);
    let call = |x1: i32, z1: i32, x2: i32, z2: i32| {
        let world =
            run_native_world_with(&[(Native::InRectangle, &[x1, z1, x2, z2])], true).unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    assert_eq!(call(x - 1, z - 1, x + 1, z + 1), 1);
    assert_eq!(
        call(x + 1, z + 1, x - 1, z - 1),
        1,
        "corners are normalized"
    );
    assert_eq!(call(x, z, x, z), 1, "edges are inclusive");
    assert_eq!(call(x + 1, z - 1, x + 5, z + 1), 0);
    assert_eq!(call(x - 1, z + 1, x + 1, z + 5), 0);
    assert!(run_native_world(&[(Native::InRectangle, &[0, 0, 1, 1])]).is_err());
}

#[test]
fn distance_from_start_returns_zero_at_the_start_and_for_unsupported_axes() {
    let call = |axis: i32| {
        let world = run_native_world_with(&[(Native::DistanceFromStart, &[axis])], true).unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    for axis in [0, 1, 2, 3, -1, i32::MAX] {
        assert_eq!(call(axis), 0, "axis {axis}");
    }
    assert!(run_native_world(&[(Native::DistanceFromStart, &[0])]).is_err());
}

#[test]
fn start_displacement_shifts_the_wrapped_difference_toward_negative_infinity() {
    let world = run_native_world_with(&[], true).unwrap();
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    let home = spatial.local.position;
    // (offset of the local position from the start, expected whole-unit result)
    for (offset, expected) in [
        (256, -1),
        (1, -1),
        (0, 0),
        (-1, 0),
        (-255, 0),
        (-256, 1),
        (-257, 1),
        (-512, 2),
    ] {
        for (axis, start) in home.into_iter().enumerate() {
            let mut moved = spatial.clone();
            moved.local.position[axis] = start.wrapping_add(offset);
            assert_eq!(
                moved.start_displacement(axis).unwrap(),
                expected,
                "axis {axis}, offset {offset}"
            );
        }
    }
    let mut wrapped = spatial.clone();
    wrapped.local.position[0] = home[0].wrapping_add(i32::MIN);
    assert_eq!(wrapped.start_displacement(0).unwrap(), i32::MIN >> 8);
    assert!(spatial.start_displacement(3).is_err());
}

#[test]
fn set_start_position_restores_only_the_flagged_axes_and_the_position_only() {
    let world = run_native_world_with(&[], true).unwrap();
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    let home = spatial.local.position;
    let history = spatial.previous_local_position;
    let mut moved = spatial.clone();
    for (axis, start) in home.into_iter().enumerate() {
        moved.local.position[axis] = start.wrapping_add(1000 + axis as i32);
    }
    let shifted = moved.local.position;
    moved.restore_start_position([false; 3]).unwrap();
    assert_eq!(moved.local.position, shifted, "no flag changes nothing");
    moved.restore_start_position([true, false, true]).unwrap();
    assert_eq!(moved.local.position, [home[0], shifted[1], home[2]]);
    assert_eq!(moved.previous_local_position, history, "history untouched");
    // A second restore of an already restored axis is a no-op, so the saved start is unchanged.
    moved.restore_start_position([true, true, true]).unwrap();
    assert_eq!(moved.local.position, home);
}

#[test]
fn set_start_position_dispatch_needs_three_flags_and_spatial_state_only_when_flagged() {
    let call = |arguments: &[i32], spatial: bool| {
        run_native_world_with(&[(Native::SetStartPosition, arguments)], spatial)
    };
    for flags in [[0, 0, 0], [1, 0, 0], [0, 7, 0], [-1, 1, 1]] {
        let world = call(&flags, true).unwrap();
        assert!(world.actors()[0].failure().is_none(), "{flags:?}");
    }
    assert!(call(&[0, 0, 0], false).is_ok(), "all-clear reads nothing");
    let error = call(&[1, 0, 0], false).err().unwrap();
    assert!(
        format!("{error:#}").contains("SetStartPosition: spatial state not prepared"),
        "{error:#}"
    );
    assert!(call(&[1, 1], true).is_err());
}

#[test]
fn in_circle_compares_the_rounded_horizontal_distance_inclusively_and_ignores_height() {
    let origin = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    let (x, z) = (origin[0], origin[2]);
    let call = |cx: i32, cz: i32, radius: i32| {
        let world = run_native_world_with(&[(Native::InCircle, &[cx, cz, radius])], true).unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    // A 3-4-5 offset: the boundary is inclusive and one less excludes it.
    assert_eq!(call(x - 3, z - 4, 5), 1, "distance equals radius");
    assert_eq!(call(x - 3, z - 4, 4), 0, "distance exceeds radius");
    assert_eq!(call(x + 3, z + 4, 5), 1, "direction does not matter");
    assert_eq!(
        call(x, z, 0),
        1,
        "the receiver's own point is inside radius zero"
    );
    assert_eq!(call(x, z, -1), 0, "a negative radius excludes everything");
    assert_eq!(call(x - 3, z - 4, i32::MAX), 1);
    assert!(run_native_world(&[(Native::InCircle, &[0, 0, 1])]).is_err());
}

/// One call of `function` with integer `arguments` by actor 0, whose `location_offsets` and
/// rotation mode are set, on a world with spatial state.
fn spatial_setter_world(
    function: Native,
    arguments: &[i32],
    offsets: Vec<[i32; 3]>,
    rotation_mode: u8,
) -> anyhow::Result<ScriptWorld> {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(
        function,
        arguments
            .iter()
            .map(|&value| Value::Integer { value })
            .collect(),
        Object::Me,
    );
    scripts.definition.functions[1].param_count = arguments.len() as u8;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.location_offsets = offsets;
    actor.attributes[Attribute::Rotation] = rotation_mode;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world.advance(
        id,
        ScriptStage::OrdinaryMain,
        ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    Ok(world)
}

#[test]
fn set_position_writes_unquantized_coordinates_and_the_history() {
    let world =
        run_native_world_with(&[(Native::SetPosition, &[1000, -2000, i32::MAX])], true).unwrap();
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(spatial.local.position, [1000, -2000, i32::MAX]);
    assert_eq!(spatial.previous_local_position, [1000, -2000, i32::MAX]);
    let error = run_native_world(&[(Native::SetPosition, &[1, 2, 3])])
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("SetPosition: spatial state not prepared"),
        "{error:#}"
    );
}

#[test]
fn set_location_selects_a_table_entry_or_the_home_and_rejects_the_count() {
    let offsets = vec![[256, 0, 0], [0, 512, 0]];
    let home = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    let at = |location: i32| {
        spatial_setter_world(Native::SetLocation, &[location], offsets.clone(), 0).map(|world| {
            let spatial = world.spatial(world.id(0).unwrap()).unwrap();
            (spatial.local.position, spatial.previous_local_position)
        })
    };
    let shifted = |delta: [i32; 3]| std::array::from_fn(|axis| home[axis] + delta[axis]);
    assert_eq!(at(0).unwrap().0, home, "selector zero is the home entry");
    assert_eq!(at(1).unwrap().0, shifted([256, 0, 0]));
    assert_eq!(at(2).unwrap().0, shifted([0, 512, 0]));
    let (position, history) = at(2).unwrap();
    assert_eq!(history, position, "the history takes the same coordinates");
    assert_eq!(at(-1).unwrap().0, home, "below zero selects the home");
    assert_eq!(at(4).unwrap().0, home, "above the count selects the home");
    let error = at(3).unwrap_err();
    assert!(
        format!("{error:#}").contains("SetLocation: location selector equals the location count"),
        "{error:#}"
    );
}

#[test]
fn set_object_location_copies_the_selected_coordinates_on_the_flagged_axes_only() {
    let offsets = vec![[256, 0, 0], [0, 512, 0]];
    let home = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    let at = |location: i32, flags: [i32; 3]| {
        let arguments = [location, flags[0], flags[1], flags[2]];
        spatial_setter_world(Native::SetObjectLocation, &arguments, offsets.clone(), 0).map(
            |world| {
                let spatial = world.spatial(world.id(0).unwrap()).unwrap();
                (spatial.local.position, spatial.previous_local_position)
            },
        )
    };
    let target =
        |delta: [i32; 3]| -> [i32; 3] { std::array::from_fn(|axis| home[axis] + delta[axis]) };
    // The entry's coordinate lands only where the flag is nonzero; the rest stay.
    assert_eq!(at(1, [1, 1, 1]).unwrap().0, target([256, 0, 0]));
    assert_eq!(at(2, [1, 0, 0]).unwrap().0[0], home[0]);
    assert_eq!(at(2, [0, 7, 0]).unwrap().0, target([0, 512, 0]));
    assert_eq!(
        at(2, [0, 0, 0]).unwrap().0,
        home,
        "no flag leaves the position"
    );
    assert_eq!(at(2, [-1, 0, 5]).unwrap().0, [home[0], home[1], home[2]]);
    // An out-of-range selector uses the home entry, and the history is not rewritten.
    assert_eq!(at(-1, [1, 1, 1]).unwrap().0, home);
    assert_eq!(at(9, [1, 1, 1]).unwrap().0, home);
    let (position, history) = at(2, [0, 1, 0]).unwrap();
    assert_eq!(position, target([0, 512, 0]));
    assert_eq!(history, home, "only the current position is written");
    // The count itself reads past the table in the original: an explicit error, nothing written.
    let error = at(3, [1, 1, 1]).unwrap_err();
    assert!(
        format!("{error:#}")
            .contains("SetObjectLocation: location selector equals the location count"),
        "{error:#}"
    );
}

#[test]
fn set_rotation_masks_the_angles_and_rebuilds_the_matrix_from_them() {
    let rotated =
        |angles: &[i32], mode: u8| spatial_setter_world(Native::SetRotation, angles, vec![], mode);
    let world = rotated(&[2048 + 5, -1, 4096 + 3], 0).unwrap();
    let spatial = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(spatial.local.angles, [5, 2047, 3]);
    assert_eq!(
        spatial.angle_history,
        [5, 2047, 3],
        "the history takes the same angles"
    );
    let masked = rotated(&[5, 2047, 3], 0).unwrap();
    let masked = masked.spatial(masked.id(0).unwrap()).unwrap();
    assert_eq!(
        format!("{:?}", spatial.local.matrix),
        format!("{:?}", masked.local.matrix),
        "the matrix follows the masked angles"
    );
    let zero = rotated(&[0, 0, 0], 0).unwrap();
    let zero = zero.spatial(zero.id(0).unwrap()).unwrap();
    assert_ne!(
        format!("{:?}", spatial.local.matrix),
        format!("{:?}", zero.local.matrix),
        "different angles give a different matrix"
    );
    let alternate = rotated(&[1, 2, 3], 1).unwrap();
    let pose = alternate.spatial(alternate.id(0).unwrap()).unwrap().local;
    assert_eq!(pose.angles, [1, 2, 3]);
    assert_eq!(
        format!("{:?}", pose.matrix),
        format!("{:?}", cyrus_world::transform::Matrix::euler([1, 2, 3]))
    );
}

#[test]
fn at_node_position_compares_the_unshifted_distance_and_treats_overflow_as_indefinite() {
    let origin = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    let call = |offset: [i32; 3], distance: i32| {
        let [x, y, z] = std::array::from_fn(|axis| origin[axis].wrapping_add(offset[axis]));
        let world =
            run_native_world_with(&[(Native::AtNodePosition, &[x, y, z, distance])], true).unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    // A 3-4-5 offset in raw units: nothing is shifted and the bound is inclusive.
    assert_eq!(call([3, 4, 0], 5), 1, "distance equals the limit");
    assert_eq!(call([3, 4, 0], 4), 0, "distance exceeds the limit");
    // The vertical difference counts here: 2-3-6 has length 7.
    assert_eq!(call([2, 6, 3], 7), 1);
    assert_eq!(call([2, 6, 3], 6), 0, "the vertical axis is not dropped");
    assert_eq!(call([0, 0, 0], 0), 1);
    assert_eq!(
        call([3, 4, 0], -1),
        0,
        "a negative limit is compared as signed"
    );
    // A length beyond the signed range converts to the integer-indefinite minimum.
    let far = [i32::MIN; 3];
    assert_eq!(
        call(far, i32::MIN),
        1,
        "indefinite compares as the signed minimum"
    );
    assert_eq!(call(far, 0), 1);
    let error = run_native_world(&[(Native::AtNodePosition, &[0, 0, 0, 1])])
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("AtNodePosition: spatial state not prepared"),
        "{error:#}"
    );
}

#[test]
fn reload_position_copies_the_local_position_into_the_history_and_changes_nothing_else() {
    // SetPosition writes both; SetStartPosition then moves only the local x back to the start, so
    // the history lags until ReloadPosition.
    let prefix = [
        (Native::SetPosition, &[1000, 2000, 3000][..]),
        (Native::SetStartPosition, &[1, 0, 0][..]),
    ];
    let before = run_native_world_with(&prefix, true).unwrap();
    let before = before.spatial(before.id(0).unwrap()).unwrap();
    assert_ne!(
        before.local.position, before.previous_local_position,
        "the history lags the local position before the reload"
    );
    let mut calls = prefix.to_vec();
    calls.push((Native::ReloadPosition, &[][..]));
    let world = run_native_world_with(&calls, true).unwrap();
    let after = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(
        after.local.position, before.local.position,
        "the local position is unchanged"
    );
    assert_eq!(after.previous_local_position, after.local.position);
    assert_eq!(after.local.angles, before.local.angles);
    let error = run_native_world(&[(Native::ReloadPosition, &[][..])])
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("ReloadPosition: spatial state not prepared"),
        "{error:#}"
    );
}

#[test]
fn placed_distance_queries_use_receiver_owned_initial_link_state() {
    use cyrus_script::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.instances[1].map_position = [10, 0, 0];
    scripts.definition.functions.push(Function {
        name: Native::PlayerDistance,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    function: Native::PlayerDistance,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    for (index, expected) in [(0, 256), (1, 2304)] {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [256, 0, 0],
                            heading: 0,
                        }),
                        distance_override: Some(false),
                        fallback_position: None,
                        linked_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [expected]);
    }
}

#[test]
fn reset_rotation_restores_only_the_selected_axes_and_rebuilds_the_matrix() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    use cyrus_world::transform::Matrix;
    for kind in [CallKind::Function, CallKind::Task, CallKind::Multitask] {
        let mut scripts = cache();
        for (name, kind) in [
            (Native::RotateByAxis, CallKind::Multitask),
            (Native::ResetRotation, CallKind::Function),
        ] {
            scripts.definition.functions.push(Function {
                name,
                param_count: 3,
                kind,
            });
        }
        let reset = [1, 0, 1].map(|value| Value::Integer { value });
        let rotate = [1, 512, 1].map(|value| Value::Integer { value });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            wait(
                1,
                Native::RotateByAxis,
                CallKind::Multitask,
                rotate.to_vec(),
            ),
            Operation::Yield,
            wait(2, Native::ResetRotation, kind, reset.to_vec()),
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
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
        }
        scripts.instances[0].angles = [128, 2000, 64];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let inputs = ScriptInputs {
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            ..Default::default()
        };
        let run = |world: &mut ScriptWorld, stage| {
            world.advance(id, stage, inputs, None, None, 20).unwrap();
        };
        run(&mut world, ScriptStage::Body(Thread::Main));
        run(&mut world, ScriptStage::Tasks);
        let rotated = world.spatial(id).unwrap().local.angles;
        assert_ne!(rotated[1], 2000, "the rotation task must have changed yaw");
        run(&mut world, ScriptStage::Body(Thread::Main));
        let state = world.spatial(id).unwrap();
        let expected = [128, rotated[1], 64];
        assert_eq!(state.local.angles, expected, "axis one keeps its rotation");
        assert_eq!(state.angle_history, expected);
        assert_eq!(
            state.local.matrix,
            Matrix::orientation(expected.map(|angle| (angle & 2047) as u16))
        );
    }
}

#[test]
fn reload_rotation_keeps_the_angles_and_rebuilds_the_matrix_and_history() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    use cyrus_world::transform::Matrix;
    let mut scripts = cache();
    for (name, kind, count) in [
        (Native::RotateByAxis, CallKind::Multitask, 3),
        (Native::ReloadRotation, CallKind::Function, 0),
    ] {
        scripts.definition.functions.push(Function {
            name,
            param_count: count,
            kind,
        });
    }
    let rotate = [1, 512, 1].map(|value| Value::Integer { value });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
        wait(
            1,
            Native::RotateByAxis,
            CallKind::Multitask,
            rotate.to_vec(),
        ),
        Operation::Yield,
        wait(2, Native::ReloadRotation, CallKind::Function, vec![]),
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
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.instances[0].angles = [128, 2000, 64];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    let run = |world: &mut ScriptWorld, stage| {
        world.advance(id, stage, inputs, None, None, 20).unwrap();
    };
    run(&mut world, ScriptStage::Body(Thread::Main));
    run(&mut world, ScriptStage::Tasks);
    let rotated = world.spatial(id).unwrap().local.angles;
    assert_ne!(rotated[1], 2000, "the rotation task must have changed yaw");
    run(&mut world, ScriptStage::Body(Thread::Main));
    let state = world.spatial(id).unwrap();
    let expected = [128, rotated[1], 64];
    assert_eq!(state.local.angles, expected, "no axis is restored");
    assert_eq!(state.angle_history, expected);
    assert_eq!(
        state.local.matrix,
        Matrix::orientation(expected.map(|angle| (angle & 2047) as u16))
    );
}

#[test]
fn distance_from_location_measures_from_the_current_local_position_through_public_calls() {
    let home = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    // Location zero is the start: a receiver still at its start is zero away, and one moved by
    // a (3, 4, 0) offset is five away (original rounding), whichever selector names the start.
    let call = |offset: [i32; 3], selector: i32| {
        let position = [0, 1, 2].map(|axis| home[axis] + offset[axis]);
        let world = run_native_world_with(
            &[
                (Native::SetPosition, &position),
                (Native::DistanceFromLocation, &[selector]),
            ],
            true,
        )
        .unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    for selector in [0, -1, i32::MAX] {
        assert_eq!(call([0; 3], selector), 0, "selector {selector}");
        assert_eq!(call([3, 4, 0], selector), 5, "selector {selector}");
        assert_eq!(call([1, 1, 0], selector), 1, "selector {selector}");
    }
    // Exactly one selector, the location count, reads past the table: an explicit error.
    let failing: Vec<i32> = (0..16)
        .filter(|&selector| {
            run_native_world_with(&[(Native::DistanceFromLocation, &[selector])], true).is_err()
        })
        .collect();
    assert_eq!(failing.len(), 1, "{failing:?}");
    // Without prepared spatial state the call cannot run.
    assert!(run_native_world(&[(Native::DistanceFromLocation, &[0])]).is_err());
}
