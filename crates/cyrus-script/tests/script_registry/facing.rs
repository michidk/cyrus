use super::*;

fn face_object_world(turn_rate: u8, start: u32) -> ScriptWorld {
    let mut scripts = actor_reference_scripts(Object::Named { string: 0 });
    let function_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::FaceObject,
        kind: CallKind::Task,
        param_count: 1,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::TurnSpeed] = turn_rate;
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::Call {
        call: Call {
            function_id,
            function: Native::FaceObject,
            kind: CallKind::Task,
            target: Object::Me,
            arguments: vec![Value::Read {
                place: Place::Local { index: 0 },
            }],
        },
    };
    // Actor 1 starts facing `start`; actor 0 lies far along +Z of it, so the bearing is zero.
    scripts.instances[0].map_position = [0, 0, 2000];
    scripts.instances[1].map_position = [0, 0, 0];
    scripts.instances[1].angles[1] = start;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    world.prepare_spatial().unwrap();
    world
}

fn face_object_step(world: &mut ScriptWorld, stage: ScriptStage) {
    let hud = cyrus_script::script::ScriptHud::default();
    let mut sounds =
        |_: cyrus_script::script::ScriptActorId, _: cyrus_script::script::ScriptSound| Ok(());
    let id = world.id(1).unwrap();
    world
        .advance(
            id,
            stage,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                hud: Some(&hud),
                shell_execution: Some(false),
                ..Default::default()
            },
            Some(&mut sounds),
            None,
            20,
        )
        .unwrap();
}

#[test]
fn face_object_turns_one_step_per_pass_toward_the_target_actor_and_a_zero_rate_ends_it() {
    // The first body pass captures the reference; the second starts the task and takes the first
    // step; each later task pass takes one more.
    let mut world = face_object_world(16, 1000);
    let id = world.id(1).unwrap();
    face_object_step(&mut world, ScriptStage::Body(Thread::Main));
    face_object_step(&mut world, ScriptStage::Body(Thread::Main));
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 984);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    face_object_step(&mut world, ScriptStage::Tasks);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 968);
    // A zero turn rate ends the task on its first pass without reading the target.
    let mut world = face_object_world(0, 1000);
    let id = world.id(1).unwrap();
    face_object_step(&mut world, ScriptStage::Body(Thread::Main));
    face_object_step(&mut world, ScriptStage::Body(Thread::Main));
    face_object_step(&mut world, ScriptStage::Tasks);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 1000);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
}

pub(super) fn facing_world(
    heading: u32,
    destination: [i64; 3],
    group: Option<u16>,
) -> (
    ScriptWorld,
    cyrus_script::script::ScriptActorId,
    cyrus_script::script::ScriptActorId,
) {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.instances[0].map_position = [0, 5, 0];
    scripts.instances[0].angles[1] = heading;
    scripts.instances[1].map_position = destination;
    for (attribute, value) in [
        (Attribute::ForwardSpeed, 4),
        (Attribute::BackwardSpeed, 5),
        (Attribute::SideSpeed, 6),
        (Attribute::ForwardWalk, 2),
        (Attribute::BackwardWalk, 3),
        (Attribute::TurnSpeed, 16),
        (Attribute::FightForwardSpeed, 8),
        (Attribute::FightForwardWalk, 7),
        (Attribute::FightTurnSpeed, 32),
    ] {
        scripts.actors.get_mut("LOGIC").unwrap().attributes[attribute] = value;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let target = world.id(1).unwrap();
    if let Some(group) = group {
        let mut groups = vec![group, 14, 15, 16, 17, 96, 97];
        groups.sort_unstable();
        groups.dedup();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
                "actors":[{"script_name":"LOGIC","animation_groups":groups.into_iter().map(|group| serde_json::json!({
                    "group_index":group,"anim_id":group+1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":group},{"opcode":2,"value":0}]
                })).collect::<Vec<_>>()}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        world
            .bind_animation(id, &data, AnimationGroup::from_index(group))
            .unwrap();
        world
            .advance(
                id,
                ScriptStage::Animation,
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
    }
    (world, id, target)
}

#[test]
fn ordinary_facing_snaps_at_each_current_group_speed_boundary_and_preserves_y_and_heading() {
    use cyrus_script::script::OrdinarySlopeInputs;
    for (group, units) in [
        (None, 1),
        (Some(0), 1),
        (Some(18), 5),
        (Some(30), 5),
        (Some(142), 5),
        (Some(19), 6),
        (Some(143), 6),
        (Some(20), 3),
        (Some(21), 3),
        (Some(22), 4),
        (Some(23), 4),
        (Some(24), 7),
        (Some(25), 7),
        (Some(26), 7),
        (Some(27), 7),
        (Some(31), 8),
        (Some(92), 9),
        (Some(93), 9),
    ] {
        let (mut world, id, target) = facing_world(400, [units, -3, 0], group);
        let previous_frame = world.animation_frame(id).unwrap();
        let angle = world
            .face_ordinary_target(
                id,
                target,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0,
                },
                &mut |_, _| panic!("arrival must not request animation sounds"),
            )
            .unwrap();
        assert_eq!(angle, 0, "{group:?}");
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [units as i32 * 256, 1280, 0],
            "{group:?}"
        );
        assert_eq!(world.spatial(id).unwrap().local.angles[1], 400);
        assert_eq!(world.animation_frame(id).unwrap(), previous_frame);
    }
}

#[test]
fn ordinary_facing_returns_pre_turn_angle_and_uses_independent_weapon_rate_and_groups() {
    use cyrus_script::script::OrdinarySlopeInputs;
    for (heading, weapon, combat, expected_heading, expected_angle, expected_group) in [
        (1408, false, false, 1424, 128, 14),
        (1664, false, false, 1648, -128, 15),
        (1408, false, true, 1440, 128, 14),
        (1408, true, false, 1440, 128, 96),
        (1664, true, true, 1632, -128, 97),
        (1520, false, false, 1536, 0, 0),
    ] {
        let (mut world, id, target) = facing_world(heading, [10, -3, 0], Some(0));
        assert_eq!(
            world
                .face_ordinary_target(
                    id,
                    target,
                    OrdinarySlopeInputs {
                        selected_player: None,
                        holding_weapon: weapon,
                        combat_active: combat,
                        frame_scale: 256,
                        ticks: 0,
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            expected_angle
        );
        assert_eq!(world.spatial(id).unwrap().local.angles[1], expected_heading);
        assert_eq!(world.animation_frame(id).unwrap(), Some(expected_group));
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 1280, 0]);
    }
}

#[test]
fn ordinary_facing_handles_absent_animation_wrapping_scale_and_the_selected_player() {
    use cyrus_script::script::OrdinarySlopeInputs;
    let (mut world, id, target) = facing_world(1424, [2, -3, 0], None);
    assert_eq!(
        world
            .face_ordinary_target(
                id,
                target,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0,
                },
                &mut |_, _| panic!("absent animation requested sound")
            )
            .unwrap(),
        112
    );
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 1440);
    // The selected player takes the same turn: the original's facing wrappers share one helper,
    // whose only selected-player step rebuilds the matrix and copies the angles to history.
    let (mut world, selected, target) = facing_world(1424, [2, -3, 0], None);
    assert_eq!(
        world
            .face_ordinary_target(
                selected,
                target,
                OrdinarySlopeInputs {
                    selected_player: Some(selected),
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0,
                },
                &mut |_, _| Ok(()),
            )
            .unwrap(),
        112
    );
    let turned = world.spatial(selected).unwrap().local;
    assert_eq!(turned.angles[1], 1440);
    assert_eq!(world.spatial(selected).unwrap().angle_history[1], 1440);
    let (mut world, id, target) = facing_world(400, [0, -3, 0], None);
    assert_eq!(
        world
            .face_ordinary_target(
                id,
                target,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: u32::MAX,
                    ticks: 0,
                },
                &mut |_, _| Ok(())
            )
            .unwrap(),
        -400
    );
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 392);
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 1280, 0]);
}

#[test]
fn ordinary_facing_arrival_registers_cell_movement_until_the_deferred_pass() {
    use cyrus_script::script::OrdinarySlopeInputs;
    for excluded in [false, true] {
        let mut scripts = cache();
        scripts.instances[0].map_position = [512, 5, 512];
        scripts.instances[1].map_position = [511, -3, 512];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let target = world.id(1).unwrap();
        world.ordinary_actors([0, 0]).unwrap();
        for _ in 0..2 {
            assert_eq!(
                world
                    .face_ordinary_target(
                        id,
                        target,
                        OrdinarySlopeInputs {
                            selected_player: None,
                            holding_weapon: false,
                            combat_active: false,
                            frame_scale: 256,
                            ticks: 0,
                        },
                        &mut |_, _| Ok(())
                    )
                    .unwrap(),
                0
            );
        }
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [130816, 1280, 131072]
        );
        world.flush_map_updates([0; 3], excluded).unwrap();
        world.ordinary_actors([6000, 6000]).unwrap();
        assert_eq!(
            world
                .ordinary_actors([0, 0])
                .unwrap()
                .iter()
                .map(|id| id.index)
                .collect::<Vec<_>>(),
            if excluded { vec![1, 0] } else { vec![0, 1] }
        );
    }
}
