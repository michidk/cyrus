use super::*;

fn offset_world(caller: Vec<Operation>, slave: bool) -> ScriptWorld {
    let mut scripts = routed_cache(caller, vec![Operation::End { target: 0 }]);
    scripts.definition.functions.extend([
        Function {
            name: Native::Offset,
            param_count: 3,
            kind: CallKind::Function,
        },
        Function {
            name: Native::UnOffset,
            param_count: 0,
            kind: CallKind::Function,
        },
    ]);
    scripts.instances[0].map_position = [10, 0, 5];
    scripts.instances[1].map_position = [4, 0, 1];
    // A quarter turn of the owner makes the stored offset differ from the world delta.
    scripts.instances[1].angles[1] = 512;
    if slave {
        for (actor, attribute) in [("LOGIC", Attribute::Slave), ("RECEIVER", Attribute::Master)] {
            scripts.actors.get_mut(actor).unwrap().attributes[attribute] = 1;
        }
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world
}

fn offset_call(arguments: [i32; 3]) -> Operation {
    Operation::ObjectCall {
        call: Call {
            function_id: 4,
            function: Native::Offset,
            target: Object::Named { string: 0 },
            kind: CallKind::Function,
            arguments: arguments.map(|value| Value::Integer { value }).to_vec(),
        },
    }
}

#[test]
fn offset_attaches_the_caller_to_a_rotated_receiver_and_unoffset_restores_its_position() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let end = || Operation::End { target: 0 };
    let run = |world: &mut ScriptWorld, caller| {
        world.advance(
            caller,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            None,
            20,
        )
    };
    let positions = |world: &ScriptWorld| {
        let (caller, receiver) = (world.id(0).unwrap(), world.id(1).unwrap());
        (
            world.spatial(caller).unwrap().local.position,
            world.spatial(receiver).unwrap().local.position,
        )
    };
    // Offset zeroes the caller's own position; the cache catches up at the next update boundary and
    // then reproduces the original world position through the receiver's rotation.
    let mut world = offset_world(vec![offset_call([0, 0, 0]), end()], false);
    let (original, _) = positions(&world);
    let caller = world.id(0).unwrap();
    run(&mut world, caller).unwrap();
    assert_eq!(world.spatial(caller).unwrap().local.position, [0; 3]);
    assert_eq!(
        world.spatial(caller).unwrap().linked_position(),
        Some([0; 3])
    );
    world.snapshot_spatial(caller).unwrap();
    assert_eq!(
        world.spatial(caller).unwrap().linked_position(),
        Some(original)
    );
    // Offsetting again to the same owner keeps the stored offset.
    let mut again = offset_world(
        vec![offset_call([0, 0, 0]), offset_call([256, 0, 0]), end()],
        false,
    );
    let caller = again.id(0).unwrap();
    run(&mut again, caller).unwrap();
    again.snapshot_spatial(caller).unwrap();
    assert_eq!(
        again.spatial(caller).unwrap().linked_position(),
        Some(original)
    );
    // UnOffset gives the owner-relative position back as local coordinates and clears the owner.
    let mut detached = offset_world(
        vec![
            offset_call([0, 0, 0]),
            wait(5, Native::UnOffset, CallKind::Function, vec![]),
            end(),
        ],
        false,
    );
    let caller = detached.id(0).unwrap();
    run(&mut detached, caller).unwrap();
    assert_eq!(detached.spatial(caller).unwrap().local.position, original);
    assert_eq!(detached.spatial(caller).unwrap().linked_position(), None);
    let expected = original.map(|value| (value >> 8) as f32);
    assert_eq!(detached.model_position(caller).unwrap(), Some(expected));
    // The arguments shift the attachment point in the world frame before the owner's rotation.
    let mut shifted = offset_world(
        vec![
            offset_call([256, -512, 768]),
            wait(5, Native::UnOffset, CallKind::Function, vec![]),
            end(),
        ],
        false,
    );
    let caller = shifted.id(0).unwrap();
    run(&mut shifted, caller).unwrap();
    assert_eq!(
        shifted.spatial(caller).unwrap().local.position,
        [original[0] + 256, original[1] - 512, original[2] + 768]
    );
    // An attached caller is fatal in the original.
    let mut slave = offset_world(vec![offset_call([0, 0, 0]), end()], true);
    let caller = slave.id(0).unwrap();
    assert!(run(&mut slave, caller).is_err());
}

#[test]
fn nested_families_compose_original_positions_and_keep_generation_identity() {
    let mut scripts = cache();
    let template = scripts.actors["LOGIC"].clone();
    scripts.actors.clear();
    scripts.instances.clear();
    for (index, (name, parent, master, position, angles)) in [
        ("ROOT", 0, 20, [100, 200, 300], [0, 0, 0]),
        ("PLATFORM", 20, 24, [100, 500, 300], [0, 0, 0]),
        ("LEVER", 24, 0, [110, 480, 330], [0, 512, 0]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut actor = template.clone();
        actor.instances = 1;
        actor.instance_variables.truncate(1);
        actor.attributes =
            Attributes::from([(Attribute::Master, master), (Attribute::Slave, parent)]);
        scripts.actors.insert(name.into(), actor);
        scripts.instances.push(Instance {
            object_id: index as u32,
            actor: name.into(),
            object_type: Default::default(),
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            light_intensity: 0,
            light_radius: 0,
            light_rgb: [0; 3],
            texture: 0,
            map_position: position,
            angles,
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 12).unwrap();
    world.prepare_spatial().unwrap();
    let lever = world.id(2).unwrap();
    assert_eq!(
        world.spatial(lever).unwrap().local.position,
        [2560, -5120, 7680]
    );
    assert!(world.spatial(lever).unwrap().effective.is_none());
    world.refresh_families().unwrap();
    let pose = world.spatial(lever).unwrap();
    assert_eq!(pose.effective.unwrap().position, [28160, 122880, 84480]);
    assert_eq!(pose.effective.unwrap().angles, [0, 512, 0]);
    assert_eq!(pose.previous_effective_position, Some([2560, -5120, 7680]));
    assert_eq!(
        world.model_position(lever).unwrap(),
        Some([110.0, 480.0, 330.0])
    );
    assert_eq!(
        world.model_position(world.id(1).unwrap()).unwrap(),
        Some([100.0, 500.0, 300.0])
    );
    assert_eq!(
        world.model_position(world.id(0).unwrap()).unwrap(),
        Some([0.0; 3])
    );
    assert!(world.prepare_spatial().is_err());
    let replacement = ScriptWorld::prepare(&scripts, 13).unwrap();
    assert!(
        replacement
            .spatial(lever)
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
}

#[test]
fn spatial_distance_requires_explicit_override_and_fallback_state() {
    use cyrus_script::script::{PlayerQuery, QueryInputs};
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let actor = world.spatial(world.id(0).unwrap()).unwrap();
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [256, -512, 768],
            heading: 2047,
        }),
        distance_override: None,
        fallback_position: None,
        linked_position: Some(None),
    };
    assert!(actor.query(Native::PlayerDistance, &[], inputs).is_err());
    inputs.distance_override = Some(false);
    assert_eq!(
        actor.query(Native::PlayerDistance, &[], inputs).unwrap(),
        Some(1536)
    );
    // The accurate form is the rounded Euclidean length: sqrt(256² + 512² + 768²) is 957.8.
    assert_eq!(
        actor
            .query(Native::AcuratePlayerDistance, &[], inputs)
            .unwrap(),
        Some(958)
    );
    assert_eq!(
        actor
            .query(
                Native::PlayerFacing,
                &[Argument::Integer(0), Argument::Integer(1)],
                inputs
            )
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        actor
            .query(
                Native::PlayerFacing,
                &[Argument::Integer(0), Argument::Integer(-1)],
                inputs
            )
            .unwrap(),
        Some(0)
    );
    inputs.player = None;
    assert!(
        actor
            .query(
                Native::PlayerFacing,
                &[Argument::Integer(0), Argument::Integer(1)],
                inputs
            )
            .is_err()
    );
    assert!(actor.query(Native::PlayerDistance, &[], inputs).is_err());
    inputs.fallback_position = Some([1, 2, 3]);
    assert_eq!(
        actor.query(Native::PlayerDistance, &[], inputs).unwrap(),
        Some(6)
    );
    assert_eq!(
        actor
            .query(Native::AcuratePlayerDistance, &[], inputs)
            .unwrap(),
        Some(4)
    );
    inputs.distance_override = Some(true);
    assert_eq!(
        actor.query(Native::PlayerDistance, &[], inputs).unwrap(),
        Some(9_999_999)
    );
    assert_eq!(
        actor
            .query(Native::AcuratePlayerDistance, &[], inputs)
            .unwrap(),
        Some(9_999_999)
    );
}

#[test]
fn ambiguous_attachment_preparation_does_not_install_partial_state() {
    let mut data = cache();
    data.instances[1].active = 1;
    data.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Master] = 1;
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    let error = world.prepare_spatial().unwrap_err();
    assert!(format!("{error:#}").contains("duplicate masters"));
    assert!(
        world
            .spatial(world.id(0).unwrap())
            .unwrap_err()
            .to_string()
            .contains("not prepared")
    );
    assert_eq!(world.flags(), [12]);
}

#[test]
fn slave_family_controls_change_nothing_and_return_zero_for_any_master_byte() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for function in [
        Native::EnableSlaves,
        Native::DisableSlaves,
        Native::KillSlaves,
        Native::HideSlaves,
        Native::ShowSlaves,
        Native::DetachSlaves,
    ] {
        for master in [0, 1, 255] {
            let mut scripts = random_scripts(function, vec![], Object::Me);
            scripts.definition.functions[1].param_count = 0;
            scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Master] = master;
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
            let id = world.id(0).unwrap();
            let before = world.actors()[0].attributes().clone();
            world
                .advance(
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
                )
                .unwrap();
            let context = format!("{function}, master {master}");
            assert!(world.actors()[0].failure().is_none(), "{context}");
            assert_eq!(
                world.actors()[0].runtime().unwrap().variables()[0],
                0,
                "{context}"
            );
            assert_eq!(world.actors()[0].attributes(), &before, "{context}");
            assert!(
                !world.actors()[0].control_state().unwrap().killed,
                "{context}"
            );
        }
    }
}

/// A slave (actor 0, slave group `slave_group`) and a root master (actor 1, master group 1) with
/// authored placement angles. `call` is `(axis, angle)` for the slave's one `FixSlaveAngle` call;
/// `None` returns the prepared world without running the script.
fn fix_slave_angle_world(
    slave_group: u8,
    call: Option<(i32, i32)>,
    spatial: bool,
    late_slave_group: Option<u8>,
) -> anyhow::Result<ScriptWorld> {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (axis, angle) = call.unwrap_or((0, 0));
    let mut scripts = random_scripts(
        Native::FixSlaveAngle,
        vec![
            Value::Integer { value: axis },
            Value::Integer { value: angle },
        ],
        Object::Me,
    );
    scripts.definition.functions[1].param_count = 2;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Slave] = slave_group;
    let mut master = scripts.actors["LOGIC"].clone();
    master.attributes[Attribute::Slave] = 0;
    master.attributes[Attribute::Master] = 1;
    master.header_index = 1;
    master.instances = 1;
    master.namespace = Some("master".into());
    master.instance_variables = vec![vec![9]];
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.instances = 1;
    slave.instance_variables.truncate(1);
    scripts.instances[1].actor = "MASTER".into();
    scripts.actors.insert("MASTER".into(), master);
    scripts.instances[0].angles = [10, 20, 30];
    scripts.instances[1].angles = [100, 200, 300];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    if spatial {
        world.prepare_spatial().unwrap();
    }
    if let Some(group) = late_slave_group {
        // Preparation rejects a slave group without a master, so that state only arises when the
        // byte is written afterwards.
        let id = world.id(0).unwrap();
        world
            .set_actor_attribute((id).index, (Attribute::Slave).index(), group)
            .unwrap();
    }
    if call.is_some() {
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
    }
    Ok(world)
}

#[test]
fn fix_slave_angle_sets_one_local_component_from_the_master_group_angle() {
    let baseline = fix_slave_angle_world(1, None, true, None).unwrap();
    let before = baseline.spatial(baseline.id(0).unwrap()).unwrap();
    // The master's angles are [100, 200, 300] and the slave's placement angles [10, 20, 30], so the
    // slave's local angles start as their differences modulo 2048.
    assert_eq!(before.local.angles, [1958, 1868, 1778]);
    for (axis, angle, component, expected) in [
        (0, 500, 0, 400),
        (1, 5, 1, 1853),
        (2, 1000, 2, 700),
        (7, -1, 2, 1747),
        (-3, 2048 + 305, 2, 5),
        (1, i32::MAX, 1, (i32::MAX - 200) & 2047),
    ] {
        let world = fix_slave_angle_world(1, Some((axis, angle)), true, None).unwrap();
        let slave = world.spatial(world.id(0).unwrap()).unwrap();
        let mut angles = before.local.angles;
        angles[component] = expected;
        assert_eq!(slave.local.angles, angles, "axis {axis}, angle {angle}");
        assert_eq!(
            format!("{:?}", slave.effective),
            format!("{:?}", before.effective),
            "the effective pose waits for the next family refresh"
        );
        assert_eq!(
            format!("{:?}", slave.local.matrix),
            format!("{:?}", before.local.matrix),
            "the matrix is not rebuilt"
        );
    }
}

#[test]
fn fix_slave_angle_leaves_a_receiver_without_a_slave_group_unchanged_and_reports_missing_state() {
    let baseline = fix_slave_angle_world(0, None, true, None).unwrap();
    let before = baseline
        .spatial(baseline.id(0).unwrap())
        .unwrap()
        .local
        .angles;
    let world = fix_slave_angle_world(0, Some((1, 5)), true, None).unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles,
        before
    );
    // An unvisited freshly allocated group retains its zero angle-table entry.
    let world = fix_slave_angle_world(1, Some((1, 5)), true, Some(3)).unwrap();
    assert_eq!(
        world.spatial(world.id(0).unwrap()).unwrap().local.angles[1],
        5
    );
    let error = fix_slave_angle_world(1, Some((1, 5)), false, None)
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("FixSlaveAngle: spatial state not prepared"),
        "{error:#}"
    );
}

/// A slave (actor 0, slave group `slave_group`, master byte `slave_master`) attached to a root
/// master (actor 1, master group 1), with the slave's one `DetachMe` call run when `call` is set.
/// `refreshed` runs the family refresh first, which gives attached actors their effective pose.
fn detach_me_world(
    slave_group: u8,
    slave_master: u8,
    refreshed: bool,
    call: bool,
    spatial: bool,
) -> anyhow::Result<ScriptWorld> {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::DetachMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.attributes[Attribute::Slave] = slave_group;
    slave.attributes[Attribute::Master] = slave_master;
    let mut master = scripts.actors["LOGIC"].clone();
    master.attributes[Attribute::Slave] = 0;
    master.attributes[Attribute::Master] = 1;
    master.header_index = 1;
    master.instances = 1;
    master.namespace = Some("master".into());
    master.instance_variables = vec![vec![9]];
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.instances = 1;
    slave.instance_variables.truncate(1);
    scripts.instances[1].actor = "MASTER".into();
    scripts.actors.insert("MASTER".into(), master);
    scripts.instances[0].angles = [10, 20, 30];
    scripts.instances[1].angles = [100, 200, 300];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    if spatial {
        world.prepare_spatial().unwrap();
        if refreshed {
            world.refresh_families().unwrap();
        }
    }
    if call {
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
    }
    Ok(world)
}

#[test]
fn detach_me_leaves_the_family_and_keeps_the_effective_pose_as_the_local_pose() {
    let before_world = detach_me_world(1, 0, true, false, true).unwrap();
    let before = before_world.spatial(before_world.id(0).unwrap()).unwrap();
    let effective = before.effective.expect("the family refresh publishes it");
    assert!(before.parent.is_some());
    let world = detach_me_world(1, 0, true, true, true).unwrap();
    let slave = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 0);
    assert_eq!(slave.parent, None, "the family record is unlinked");
    assert_eq!(slave.local.position, effective.position);
    assert_eq!(slave.local.angles, effective.angles);
    assert_eq!(
        format!("{:?}", slave.local.matrix),
        format!("{:?}", effective.matrix)
    );
    assert_eq!(
        format!("{:?}", slave.effective),
        format!("{:?}", before.effective),
        "the effective fields are not changed"
    );
    // The master is untouched.
    let master = world.spatial(world.id(1).unwrap()).unwrap();
    let master_before = before_world.spatial(before_world.id(1).unwrap()).unwrap();
    assert_eq!(master.local.angles, master_before.local.angles);
    assert_eq!(master.local.position, master_before.local.position);
}

#[test]
fn detach_me_changes_less_for_unattached_and_master_receivers_and_reports_missing_state() {
    // An unattached receiver is unchanged.
    let world = detach_me_world(0, 0, true, true, true).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 0);
    assert_eq!(world.spatial(world.id(0).unwrap()).unwrap().parent, None);
    // An attached receiver that is itself a master only loses its slave byte.
    let before_world = detach_me_world(1, 2, true, false, true).unwrap();
    let before = before_world.spatial(before_world.id(0).unwrap()).unwrap();
    let world = detach_me_world(1, 2, true, true, true).unwrap();
    let slave = world.spatial(world.id(0).unwrap()).unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 0);
    assert_eq!(slave.parent, before.parent, "the family record is kept");
    assert_eq!(slave.local.position, before.local.position);
    assert_eq!(slave.local.angles, before.local.angles);
    // Before the first family refresh there is no effective pose to keep; the byte is untouched.
    let error = detach_me_world(1, 0, false, true, true).err().unwrap();
    assert!(
        format!("{error:#}").contains("before the first family refresh"),
        "{error:#}"
    );
    let error = detach_me_world(1, 0, false, true, false).err().unwrap();
    assert!(
        format!("{error:#}").contains("DetachMe: spatial state not prepared"),
        "{error:#}"
    );
}

fn attach_me_world(group: i32, slave_group: u8, no_rotation: bool) -> anyhow::Result<ScriptWorld> {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(
        Native::AttachMe,
        vec![Value::Integer { value: group }],
        Object::Me,
    );
    scripts.definition.functions[1].param_count = 1;
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.attributes[Attribute::Slave] = slave_group;
    slave.attributes[Attribute::Master] = 0;
    slave.attributes[Attribute::NoSlaveRot] = u8::from(no_rotation);
    let mut master = slave.clone();
    master.attributes[Attribute::Slave] = 0;
    master.attributes[Attribute::Master] = 1;
    master.header_index = 1;
    master.instances = 1;
    master.namespace = Some("master".into());
    master.instance_variables = vec![vec![9]];
    let slave = scripts.actors.get_mut("LOGIC").unwrap();
    slave.instances = 1;
    slave.instance_variables.truncate(1);
    scripts.instances[1].actor = "MASTER".into();
    scripts.actors.insert("MASTER".into(), master);
    scripts.instances[0].angles = [10, 20, 30];
    scripts.instances[1].angles = [0, 512, 0];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    world.advance(
        world.id(0).unwrap(),
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
fn attach_me_converts_ordinary_local_pose_and_refreshes_in_parent_order() {
    let mut world = attach_me_world(1, 0, false).unwrap();
    let id = world.id(0).unwrap();
    let actor = world.spatial(id).unwrap();
    let original = actor.effective.unwrap();
    let parent = world.spatial(world.id(1).unwrap()).unwrap().local;
    assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 1);
    assert_eq!(actor.parent, Some(1));
    assert_eq!(actor.local.angles, [10, 1556, 30]);
    assert_eq!(actor.angle_history, [10, 1556, 30]);
    let delta: [i32; 3] =
        std::array::from_fn(|axis| original.position[axis] - parent.position[axis]);
    assert_eq!(actor.local.position, [-delta[2], delta[1], delta[0]]);
    assert_eq!(actor.previous_local_position, original.position);
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
    world.refresh_families().unwrap();
    assert_eq!(
        world.spatial(id).unwrap().effective.unwrap().position,
        original.position
    );
    assert_eq!(
        world.spatial(id).unwrap().effective.unwrap().angles,
        [10, 20, 30]
    );
}

#[test]
fn attach_me_preserves_rotation_when_disabled_and_does_not_truncate_target() {
    let world = attach_me_world(1, 0, true).unwrap();
    let actor = world.spatial(world.id(0).unwrap()).unwrap();
    let original = actor.effective.unwrap();
    assert_eq!(actor.local.angles, original.angles);
    assert_eq!(actor.angle_history, original.angles);
    assert_eq!(actor.local.matrix, original.matrix);
    for group in [0, 1, -1, 256, 257, i32::MAX] {
        let world = attach_me_world(group, 1, false).unwrap();
        let actor = world.spatial(world.id(0).unwrap()).unwrap();
        if group == 0 || group == 1 {
            assert_eq!(actor.parent, Some(1));
            assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 1);
        } else {
            assert_eq!(
                actor.parent, None,
                "missing target must retain detach effects"
            );
            assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 0);
            assert_eq!(actor.local.position, actor.effective.unwrap().position);
            assert_eq!(actor.local.angles, actor.effective.unwrap().angles);
        }
    }
    assert_eq!(run_native_calls(&[(Native::AttachMe, &[0])]).unwrap(), 0);
    let error = run_native_calls(&[(Native::AttachMe, &[1])]).unwrap_err();
    assert!(format!("{error:#}").contains("AttachMe: spatial state not prepared"));
}

#[test]
fn attach_me_guard_uses_current_master_and_slave_without_spatial_state() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (group, succeeds) in [(0, true), (7, true), (9, true), (8, false)] {
        let mut scripts = random_scripts(
            Native::AttachMe,
            vec![Value::Integer { value: group }],
            Object::Me,
        );
        scripts.definition.functions[1].param_count = 1;
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes[Attribute::Master] = 7;
        actor.attributes[Attribute::Slave] = 9;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let result = world.advance(
            world.id(0).unwrap(),
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            100,
        );
        if succeeds {
            result.unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
        } else {
            let error = result.unwrap_err();
            assert!(
                format!("{error:#}")
                    .contains("requires separate ordinary and master family records")
            );
        }
        assert_eq!(world.actors()[0].attributes()[Attribute::Master], 7);
        assert_eq!(world.actors()[0].attributes()[Attribute::Slave], 9);
    }
}

#[test]
fn an_attached_support_carries_riders_by_its_effective_pose_after_three_family_refreshes() {
    use cyrus_script::script::{ModelSupportOwners, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveByAxis,
        param_count: 3,
        kind: CallKind::Multitask,
    });
    let template = scripts.actors["LOGIC"].clone();
    scripts.actors.clear();
    scripts.instances.clear();
    for (index, (name, master, slave, support)) in [
        ("ROOT", 20, 0, 0),
        ("PLATFORM", 0, 20, 1),
        ("RIDER", 0, 0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut actor = template.clone();
        actor.instances = 1;
        actor.instance_variables.truncate(1);
        actor.attributes = Attributes::from([
            (Attribute::PlayerAlign, support),
            (Attribute::Master, master),
            (Attribute::Slave, slave),
        ]);
        if name == "ROOT" {
            actor.program.steps = BTreeMap::from([
                (
                    0,
                    Step {
                        next: 1,
                        operation: wait(
                            1,
                            Native::MoveByAxis,
                            CallKind::Multitask,
                            vec![
                                Value::Integer { value: 0 },
                                Value::Integer { value: 256 },
                                Value::Integer { value: 1 },
                            ],
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
        }
        scripts.actors.insert(name.into(), actor);
        scripts.instances.push(Instance {
            object_id: index as u32,
            actor: name.into(),
            object_type: PlacementType::MODEL,
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            light_intensity: 0,
            light_radius: 0,
            light_rgb: [0; 3],
            texture: 0,
            map_position: [100, 200, 300],
            angles: [0; 3],
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 12).unwrap();
    world.prepare_spatial().unwrap();
    let (root, platform, rider) = (
        world.id(0).unwrap(),
        world.id(1).unwrap(),
        world.id(2).unwrap(),
    );
    world
        .set_model_support(
            rider,
            ModelSupportOwners {
                actor: Some(platform),
                static_placement: None,
            },
        )
        .unwrap();
    for _ in 0..2 {
        world.refresh_families().unwrap();
        assert!(!world.follow_ordinary_support(rider, None, None).unwrap());
    }
    world.snapshot_spatial(root).unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            root,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    world
        .advance(root, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    world.refresh_families().unwrap();
    let before = world.spatial(rider).unwrap().local.position;
    assert!(world.follow_ordinary_support(rider, None, None).unwrap());
    let after = world.spatial(rider).unwrap().local.position;
    assert_eq!(after, [before[0] + 256, before[1], before[2]]);
}

#[test]
fn offset_location_resolves_source_before_receiver_and_uses_composed_source_matrix() {
    for source_is_slave in [true, false] {
        let mut scripts =
            offset_location_scripts(Object::Named { string: 0 }, [256, 0, 512], Object::Me);
        let mut receiver = scripts.actors["LOGIC"].clone();
        receiver.header_index += 1;
        receiver.code_origin += 100;
        receiver.namespace = Some("receiver".into());
        receiver.instances = 1;
        receiver.instance_variables = vec![vec![0; 3]];
        let rotation_id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::SetRotation,
            kind: CallKind::Function,
            param_count: 3,
        });
        for key in (3..=5).rev() {
            let mut step = receiver.program.steps.remove(&key).unwrap();
            step.next += 1;
            if let Operation::End { target } = &mut step.operation {
                *target += 1;
            }
            receiver.program.steps.insert(key + 1, step);
        }
        receiver.program.steps.get_mut(&2).unwrap().operation = wait(
            rotation_id,
            Native::SetRotation,
            CallKind::Function,
            vec![
                Value::Integer { value: 0 },
                Value::Integer { value: 512 },
                Value::Integer { value: 0 },
            ],
        );
        receiver.program.steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::Yield,
            },
        );

        let source = scripts.actors.get_mut("LOGIC").unwrap();
        source.instances = 1;
        source.instance_variables = vec![vec![0; 3]];
        source.attributes[if source_is_slave {
            Attribute::Slave
        } else {
            Attribute::Master
        }] = 1;
        receiver.attributes[if source_is_slave {
            Attribute::Master
        } else {
            Attribute::Slave
        }] = 1;
        scripts.actors.insert("RECEIVER".into(), receiver);
        scripts.instances[1].actor = "RECEIVER".into();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(1).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        let before = world.inspect(0, 10);
        let error = actor_reference_step(&mut world, id)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(if source_is_slave {
                "OffsetLocation source position"
            } else {
                "OffsetLocation receiver position"
            }),
            "{error}"
        );
        let after = world.inspect(0, 10);
        for index in 0..2 {
            assert_eq!(
                after["actors"][index]["spatial"],
                before["actors"][index]["spatial"]
            );
        }
        assert_eq!(world.flags(), [12]);

        // A fresh world with an explicitly refreshed family can execute the same public call.
        let mut world = ScriptWorld::prepare(&scripts, 18).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(1).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        world.refresh_families().unwrap();
        actor_reference_step(&mut world, id).unwrap();
        assert_eq!(world.flags(), [77]);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if source_is_slave {
                [-5632, 5120, 3328]
            } else {
                [2816, 5120, 8192]
            }
        );
    }
}

#[test]
fn offset_location_captured_source_survives_restore_but_not_created_actor_retirement() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    for retire in [false, true] {
        let mut scripts =
            offset_location_scripts(Object::Named { string: 1 }, [256, -3, 7], Object::Me);
        let kill = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        });
        let mut source = scripts.actors["LOGIC"].clone();
        source.header_index += 10;
        source.code_origin += 1000;
        source.namespace = Some("source".into());
        source.instances = 1;
        source.instance_variables = vec![vec![0; 3]];
        source.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(kill, Native::KillMe, CallKind::Function, vec![]),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::End { target: 1 },
                },
            ),
        ]);
        scripts.actors.insert("SOURCE".into(), source);
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let (mut world, data, _) =
            static_model_fixture_session(scripts.clone(), None, [10, 20, 30], false);
        let creator = world.id(0).unwrap();
        let source = world
            .create_object(creator, "SOURCE", [100, 200, 300], [0; 3])
            .unwrap();
        let caller = world.id(1).unwrap();
        actor_reference_step(&mut world, caller).unwrap();
        if retire {
            world
                .advance(
                    source,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    Some(&mut |_, _| Ok(())),
                    None,
                    20,
                )
                .unwrap();
            world
                .update_created(
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    &mut cyrus_game::random::EngineRandom::from_state(42),
                    &mut |_, _| Ok(()),
                    20,
                )
                .unwrap();
            assert!(world.actors()[source.index].runtime().is_none());
        }
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let saved = world.snapshot(&sources).unwrap();
        let mut restored = ScriptWorld::from_snapshot(
            saved,
            &sources,
            WorldRestoreContext {
                generation: 101,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let caller = restored.id(caller.index).unwrap();
        let captured = restored.actors()[caller.index]
            .runtime()
            .unwrap()
            .variables()[1]
            .actor()
            .unwrap();
        assert_eq!(captured.generation, 101);
        assert_eq!(captured.index, source.index);
        let before = restored.inspect(0, 10);
        let result = actor_reference_step(&mut restored, caller);
        if retire {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("OffsetLocation source position")
                    && error.contains("retired created receiver"),
                "{error}"
            );
            let after = restored.inspect(0, 10);
            assert_eq!(
                after["actors"][caller.index]["spatial"],
                before["actors"][caller.index]["spatial"]
            );
            assert_eq!(
                after["actors"][caller.index]["main"],
                before["actors"][caller.index]["main"]
            );
            assert_eq!(
                actor_reference_step(&mut restored, caller)
                    .unwrap_err()
                    .to_string(),
                error
            );
        } else {
            result.unwrap();
            assert_eq!(
                restored.spatial(caller).unwrap().local.position,
                [356, 197, 307]
            );
            assert_eq!(restored.flags(), [77]);
        }
    }
}

#[test]
fn offset_location_reads_link_cache_without_refreshing_it_or_the_receiver() {
    let mut scripts =
        offset_location_scripts(Object::Named { string: 0 }, [256, -3, 7], Object::Me);
    let offset_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::Offset,
        kind: CallKind::Function,
        param_count: 3,
    });
    let mut source = scripts.actors["LOGIC"].clone();
    source.instances = 1;
    source.instance_variables = vec![vec![0; 3]];
    source.program.strings[0] = "receiver".into();
    source.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::ObjectCall {
                    call: Call {
                        function_id: offset_id,
                        function: Native::Offset,
                        kind: CallKind::Function,
                        target: Object::Named { string: 0 },
                        arguments: vec![Value::Integer { value: 0 }; 3],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 1 },
            },
        ),
    ]);
    let mut receiver = scripts.actors["LOGIC"].clone();
    receiver.header_index += 1;
    receiver.code_origin += 100;
    receiver.namespace = Some("receiver".into());
    receiver.instances = 1;
    receiver.instance_variables = vec![vec![0; 3]];
    scripts.actors.insert("LOGIC".into(), source);
    scripts.actors.insert("RECEIVER".into(), receiver);
    scripts.instances[1].actor = "RECEIVER".into();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    world.prepare_spatial().unwrap();
    let source = world.id(0).unwrap();
    actor_reference_step(&mut world, source).unwrap();
    world.snapshot_spatial(source).unwrap();
    assert_eq!(world.spatial(source).unwrap().local.position, [0; 3]);
    assert_eq!(
        world.spatial(source).unwrap().linked_position(),
        Some([2560, 5120, 7680])
    );
    let receiver = world.id(1).unwrap();
    actor_reference_step(&mut world, receiver).unwrap();
    actor_reference_step(&mut world, receiver).unwrap();
    assert_eq!(
        world.spatial(receiver).unwrap().local.position,
        [2816, 5117, 7687]
    );
    assert_eq!(
        world.spatial(source).unwrap().linked_position(),
        Some([2560, 5120, 7680])
    );
}

#[test]
fn offset_location_observes_an_attached_sources_immediate_rotation_rebuild() {
    for function in [Native::SetRotation, Native::ResetRotation] {
        let mut scripts =
            offset_location_scripts(Object::Named { string: 0 }, [256, 0, 512], Object::Me);
        let rotation = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: function,
            kind: CallKind::Function,
            param_count: 3,
        });
        let mut receiver = scripts.actors["LOGIC"].clone();
        receiver.header_index += 1;
        receiver.code_origin += 100;
        receiver.namespace = Some("receiver".into());
        receiver.instances = 1;
        receiver.instance_variables = vec![vec![0; 3]];
        receiver.attributes[Attribute::Master] = 1;
        let master_rotation = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::SetRotation,
            kind: CallKind::Function,
            param_count: 3,
        });
        for key in (3..=5).rev() {
            let mut step = receiver.program.steps.remove(&key).unwrap();
            step.next += 1;
            if let Operation::End { target } = &mut step.operation {
                *target += 1;
            }
            receiver.program.steps.insert(key + 1, step);
        }
        receiver.program.steps.get_mut(&2).unwrap().operation = wait(
            master_rotation,
            Native::SetRotation,
            CallKind::Function,
            vec![
                Value::Integer { value: 0 },
                Value::Integer { value: 1024 },
                Value::Integer { value: 0 },
            ],
        );
        receiver.program.steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::Yield,
            },
        );

        let source = scripts.actors.get_mut("LOGIC").unwrap();
        source.instances = 1;
        source.instance_variables = vec![vec![0; 3]];
        source.attributes[Attribute::Slave] = 1;
        source.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(
                        rotation,
                        function,
                        CallKind::Function,
                        [
                            0,
                            if function == Native::SetRotation {
                                1024
                            } else {
                                1
                            },
                            0,
                        ]
                        .map(|value| Value::Integer { value })
                        .to_vec(),
                    ),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::End { target: 1 },
                },
            ),
        ]);
        scripts.actors.insert("RECEIVER".into(), receiver);
        scripts.instances[0].angles[1] = 512;
        scripts.instances[1].actor = "RECEIVER".into();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        world.prepare_spatial().unwrap();
        let source = world.id(0).unwrap();
        let receiver = world.id(1).unwrap();
        actor_reference_step(&mut world, receiver).unwrap();
        world.refresh_families().unwrap();
        // Change the source matrix through its public native, without another family pass.
        let before = world.spatial(source).unwrap().query_pose().unwrap();
        actor_reference_step(&mut world, source).unwrap();
        assert_eq!(
            world
                .spatial(source)
                .unwrap()
                .query_pose()
                .unwrap()
                .position,
            before.position
        );
        actor_reference_step(&mut world, receiver).unwrap();
        assert_eq!(
            world.spatial(receiver).unwrap().local.position,
            if function == Native::SetRotation {
                [-768, 5120, -5120]
            } else {
                [-1024, 5120, -4352]
            }
        );
    }
}

fn offset_location_scripts(source: Object, offset: [i32; 3], target: Object) -> Scripts {
    let mut scripts = actor_reference_scripts(source);
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::OffsetLocation,
        kind: CallKind::Function,
        param_count: 4,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings.push("source".into());
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 2 },
        formula: Formula {
            first: if target == Object::Me {
                Value::Call {
                    call: Box::new(Call {
                        function_id,
                        function: Native::OffsetLocation,
                        kind: CallKind::Function,
                        target,
                        arguments: std::iter::once(Value::Read {
                            place: Place::Local { index: 1 },
                        })
                        .chain(offset.map(|value| Value::Integer { value }))
                        .collect(),
                    }),
                }
            } else {
                Value::ObjectCall {
                    call: Box::new(Call {
                        function_id,
                        function: Native::OffsetLocation,
                        kind: CallKind::Function,
                        target,
                        arguments: std::iter::once(Value::Read {
                            place: Place::Property {
                                object: Object::Me,
                                reference: 294,
                            },
                        })
                        .chain(offset.map(|value| Value::Integer { value }))
                        .collect(),
                    }),
                }
            },
            rest: vec![],
        },
    };
    actor.program.steps.get_mut(&4).unwrap().operation = set_flag(77);
    actor.program.steps.insert(
        5,
        Step {
            next: 6,
            operation: Operation::End { target: 5 },
        },
    );
    scripts.instances[0].map_position = [10, 20, 30];
    scripts.instances[0].angles = [0; 3];
    scripts.instances[1].map_position = [4, 5, 6];
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn offset_location_public_values_preserve_source_receiver_and_local_only_writes() {
    for (source, target, receiver, expected) in [
        (
            Object::Named { string: 0 },
            Object::Me,
            1,
            [2816, 5117, 7687],
        ),
        (Object::Me, Object::Me, 1, [1280, 1277, 1543]),
        (
            Object::Me,
            Object::Named { string: 0 },
            0,
            [2816, 5117, 7687],
        ),
    ] {
        let scripts = offset_location_scripts(source, [256, -3, 7], target);
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        world.prepare_spatial().unwrap();
        let caller = world.id(1).unwrap();
        actor_reference_step(&mut world, caller).unwrap();
        let before = world.inspect(0, 10);
        let history = world
            .spatial(world.id(receiver).unwrap())
            .unwrap()
            .previous_local_position;
        actor_reference_step(&mut world, caller).unwrap();
        assert_eq!(
            world
                .spatial(world.id(receiver).unwrap())
                .unwrap()
                .local
                .position,
            expected
        );
        assert_eq!(
            world
                .spatial(world.id(receiver).unwrap())
                .unwrap()
                .previous_local_position,
            history
        );
        assert_eq!(world.actors()[1].runtime().unwrap().variables()[2], 0);
        assert_eq!(world.flags(), [77]);
        assert!(
            world
                .actors()
                .iter()
                .all(|actor| actor.runtime().unwrap().active_tasks() == 0)
        );
        let after = world.inspect(0, 10);
        for index in 0..2 {
            let mut expected_pose = before["actors"][index]["spatial"].clone();
            if index == receiver {
                expected_pose["local"]["position"] = serde_json::json!(expected);
            }
            assert_eq!(after["actors"][index]["spatial"], expected_pose);
        }
        // A pending map update would require geometry here. OffsetLocation registers none.
        world.flush_map_updates([0; 3], false).unwrap();
    }
}

#[test]
fn offset_location_rotates_rounds_inputs_and_wraps_position_sums() {
    for (yaw, offset, expected) in [
        (512, [256, -3, 512], [2048, 5117, 7936]),
        (0, [16_777_217, 0, 0], [16_779_776, 5120, 7680]),
        (0, [2_147_483_520, 0, 0], [-2_147_481_216, 5120, 7680]),
    ] {
        let mut scripts = offset_location_scripts(Object::Named { string: 0 }, offset, Object::Me);
        scripts.instances[0].angles[1] = yaw;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(1).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
    }
}

#[test]
fn offset_location_errors_preserve_pose_and_continuation_and_remain_failed() {
    for (integer_source, spatial, offset, expected) in [
        (true, true, [0; 3], "actor reference required"),
        (false, false, [0; 3], "OffsetLocation source position"),
        (
            false,
            true,
            [i32::MAX, 0, 0],
            "OffsetLocation source matrix",
        ),
    ] {
        let scripts = offset_location_scripts(Object::Me, offset, Object::Me);
        let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
        if spatial {
            world.prepare_spatial().unwrap();
        }
        let id = world.id(1).unwrap();
        actor_reference_step(&mut world, id).unwrap();
        if integer_source {
            // Continue the captured reference fixture with an integer assignment, through the VM.
            let mut scripts = scripts.clone();
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.program.steps.get_mut(&1).unwrap().operation = Operation::Assign {
                destination: Place::Local { index: 1 },
                formula: Formula {
                    first: Value::Integer { value: 0 },
                    rest: vec![],
                },
            };
            scripts.content_id = scripts.content_identity().unwrap();
            world = ScriptWorld::prepare(&scripts, 17).unwrap();
            world.prepare_spatial().unwrap();
            actor_reference_step(&mut world, id).unwrap();
        }
        let before = world.inspect(0, 10);
        let error = actor_reference_step(&mut world, id)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert_eq!(world.flags(), [12]);
        let after = world.inspect(0, 10);
        assert_eq!(
            after["actors"][1]["spatial"],
            before["actors"][1]["spatial"]
        );
        assert_eq!(after["actors"][1]["main"], before["actors"][1]["main"]);
        assert_eq!(
            actor_reference_step(&mut world, id)
                .unwrap_err()
                .to_string(),
            error
        );
    }
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_family_orphan_angles_drive_the_first_public_fix_slave_angle_call() -> anyhow::Result<()>
{
    let mut world = fix_slave_angle_world(1, None, true, Some(77))?;
    let mut families = vec![
        rgpre::import::savegame::SaveFamily {
            actor_id: -1,
            position_q8: [0; 3],
            angles: [0; 3],
        };
        256
    ];
    families[77].angles = [321, -654, 987];
    families[77].position_q8 = [-256, 512, 768];
    world.restore_original_families(&[], &families)?;
    let id = world.id(0)?;
    let before = world.spatial(id)?.local.angles;
    world.advance(
        id,
        cyrus_script::script::ScriptStage::OrdinaryMain,
        cyrus_script::script::ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    assert_eq!(
        world.spatial(id)?.local.angles,
        [(-321i32) & 2047, before[1], before[2]]
    );
    Ok(())
}
