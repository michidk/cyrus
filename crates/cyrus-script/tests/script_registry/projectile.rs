use super::*;

#[test]
fn shoot_creates_the_named_object_at_the_creator_with_the_given_orientation() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs, ScriptStage};
    let shoot = |name_known: bool| {
        let mut scripts = cache();
        let mut projectile = scripts.actors["LOGIC"].clone();
        projectile.code_origin += 1000;
        projectile.header_index += 100;
        scripts.actors.insert("BALL".into(), projectile);
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::Shoot,
            param_count: 7,
            kind: CallKind::Function,
        });
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.program.strings = vec![if name_known { "ball" } else { "nothing" }.into()];
        let mut arguments = vec![Value::String { index: 0 }];
        arguments.extend([0, 0, 0, 0, 512, 0].map(|value| Value::Integer { value }));
        logic.program.steps = [
            Operation::Call {
                call: Call {
                    function_id: id,
                    function: Native::Shoot,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments,
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
        for placement in &mut scripts.instances {
            placement.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let creator = world.id(0).unwrap();
        world
            .advance(
                creator,
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
            .map(|()| world)
    };
    let world = shoot(true).unwrap();
    let created = world
        .actors()
        .iter()
        .position(
            |actor| matches!(&actor.source, ScriptActorSource::Created(name) if name == "BALL"),
        )
        .expect("the object exists");
    let id = world.id(created).unwrap();
    let pose = world.spatial(id).unwrap().local;
    assert_eq!(pose.angles, [0, 512, 0]);
    let creator = world.spatial(world.id(0).unwrap()).unwrap().local.position;
    assert_eq!(
        pose.position, creator,
        "a zero offset puts the object on its creator"
    );
    assert!(shoot(false).is_err(), "an unknown script name is an error");
}

#[test]
fn shoot_player_aims_the_created_object_at_the_selected_player() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let mut projectile = scripts.actors["LOGIC"].clone();
        projectile.code_origin += 1000;
        projectile.header_index += 100;
        scripts.actors.insert("BALL".into(), projectile);
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::ShootPlayer,
            param_count: 4,
            kind: CallKind::Function,
        });
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.program.strings = vec!["ball".into()];
        logic.program.steps = [
            Operation::Call {
                call: Call {
                    function_id: id,
                    function: Native::ShootPlayer,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![
                        Value::String { index: 0 },
                        Value::Integer { value: 0 },
                        Value::Integer { value: 0 },
                        Value::Integer { value: 256 },
                    ],
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
    });
    let map = world.id(0).unwrap();
    world
        .advance(
            map,
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
    let created = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Created(_)))
        .expect("the object exists");
    let pose = world.spatial(world.id(created).unwrap()).unwrap().local;
    let player = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let target = world
        .spatial(world.id(player).unwrap())
        .unwrap()
        .local
        .position;
    let (dx, dz) = (
        f64::from(target[0].wrapping_sub(pose.position[0])),
        f64::from(target[2].wrapping_sub(pose.position[2])),
    );
    assert!(
        dx != 0.0 || dz != 0.0,
        "the fixture separates the player from the shot"
    );
    let expected = (-dx.atan2(dz) * 2048.0 / std::f64::consts::TAU).rem_euclid(2048.0);
    let yaw = f64::from(pose.angles[1]);
    let distance = (yaw - expected).abs().min(2048.0 - (yaw - expected).abs());
    assert!(distance <= 2.0, "yaw {yaw} against {expected}");
    assert_eq!(pose.angles[2], 0, "roll is zero");
}

#[test]
fn projectile_ai_flies_along_its_heading_until_its_parent_range_then_continues_the_script() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: Native::ProjectileAI,
            param_count: 0,
            kind: CallKind::Task,
        },
        Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        },
    ]);
    let mut projectile = scripts.actors["LOGIC"].clone();
    projectile.code_origin += 1000;
    projectile.header_index += 100;
    projectile.instances = 1;
    projectile.namespace = None;
    projectile.instance_variables = vec![vec![]];
    projectile.program.variables = vec![];
    projectile.attributes = Attributes::from([(Attribute::ForwardSpeed, 200)]);
    projectile.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(1, Native::ProjectileAI, CallKind::Task, vec![]),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: wait(2, Native::KillMe, CallKind::Function, vec![]),
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.actors.insert("BALL".into(), projectile);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.active = 1;
        placement.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, models) = static_model_fixture_at(scripts, None, [600, 20, 600]);
    let creator = world.id(0).unwrap();
    let origin = world.spatial(creator).unwrap().local.position;
    // Yaw 0 flies toward +Z in the shoot rotation; attribute 8 = 200 map units per nominal tick.
    let ball = world
        .create_object(creator, "ball", origin, [0, 0, 0])
        .unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let mut random = cyrus_game::random::EngineRandom::default();
    let mut positions = Vec::new();
    for _ in 0..24 {
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut random,
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        let actor = &world.actors()[ball.index];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        // The final killed pose is retained for diagnostics, not live access.
        positions.push(
            serde_json::from_value::<[i32; 3]>(
                world.inspect(ball.index, 1)["actors"][0]["spatial"]["local"]["position"].clone(),
            )
            .unwrap(),
        );
        if actor.control_state().unwrap().killed {
            break;
        }
    }
    let step = positions[1][2] - positions[0][2];
    assert_eq!(step.abs(), 200 << 8, "{positions:?}");
    assert!(
        positions
            .iter()
            .all(|p| p[0] == origin[0] && p[1] == origin[1])
    );
    // The range ends at 0xbb800 Q8 = 3000 map units from the parent: the step that reaches it
    // ends the task (keeping the moved position) and the script proceeds to KillMe.
    let last = *positions.last().unwrap();
    assert!(
        world.actors()[ball.index].control_state().unwrap().killed,
        "{positions:?}"
    );
    // 3000 map units at 200 per step: the fifteenth step reaches the range.
    assert_eq!(positions.len(), 15, "{positions:?}");
    assert_eq!((last[2] - origin[2]).abs(), 0xbb800);
}

#[test]
fn projectile_ai_stops_at_a_wall_contact() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for yaw in [512u16, 1536] {
        let mut scripts = cache();
        scripts.definition.functions.extend([
            Function {
                name: Native::ProjectileAI,
                param_count: 0,
                kind: CallKind::Task,
            },
            Function {
                name: Native::KillMe,
                param_count: 0,
                kind: CallKind::Function,
            },
        ]);
        let mut projectile = scripts.actors["LOGIC"].clone();
        projectile.code_origin += 1000;
        projectile.header_index += 100;
        projectile.instances = 1;
        projectile.namespace = None;
        projectile.instance_variables = vec![vec![]];
        projectile.program.variables = vec![];
        projectile.attributes = Attributes::from([(Attribute::ForwardSpeed, 200)]);
        projectile.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(1, Native::ProjectileAI, CallKind::Task, vec![]),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: wait(2, Native::KillMe, CallKind::Function, vec![]),
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        scripts.actors.insert("BALL".into(), projectile);
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.active = 1;
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let (mut world, data, models) = static_model_fixture_at(scripts, None, [2000, 20, 600]);
        let creator = world.id(0).unwrap();
        let origin = world.spatial(creator).unwrap().local.position;
        let ball = world
            .create_object(creator, "ball", origin, [0, yaw, 0])
            .unwrap();
        let mut scene = ordinary_collision_scene(world.world, models);
        // Walls 1050 map units to either side along X (off the 200-unit step grid, since a step
        // ending exactly on a plane is not a crossing); only the one in the flight path is hit.
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: [3050, 950]
                .into_iter()
                .enumerate()
                .map(|(id, x)| StaticPlacement {
                    id: id as u32 + 1,
                    model: "WALL".into(),
                    position: [x, 20, 600],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .collect(),
            models: BTreeMap::from([("WALL".into(), ordinary_forward_wall_model())]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let mut random = cyrus_game::random::EngineRandom::default();
        let mut path = Vec::new();
        for _ in 0..12 {
            world
                .update_ordinary(
                    &data,
                    [2000, 600],
                    OrdinaryInputs {
                        scripts: ScriptInputs {
                            nominal_ticks: Some(1),
                            frame_scale: Some(256),
                            ..Default::default()
                        },
                        scene: Some(&scene),
                    },
                    &mut random,
                    &mut |_, _| Ok(()),
                    100,
                )
                .unwrap();
            let actor = &world.actors()[ball.index];
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
            // The contact pose remains in diagnostic storage after cleanup.
            path.push(
                serde_json::from_value::<[i32; 3]>(
                    world.inspect(ball.index, 1)["actors"][0]["spatial"]["local"]["position"]
                        .clone(),
                )
                .unwrap(),
            );
            if actor.control_state().unwrap().killed {
                break;
            }
        }
        let last = *path.last().unwrap();
        assert!(
            world.actors()[ball.index].control_state().unwrap().killed,
            "yaw {yaw}: {path:?}"
        );
        // The flight ends at the contact on the wall plane, 1050 map units from the start.
        assert_eq!(
            (last[0] - origin[0]).abs(),
            1050 << 8,
            "yaw {yaw}: {path:?}"
        );
        assert!(path.len() < 15, "yaw {yaw}: {path:?}");
    }
}

#[test]
fn projectile_contact_uses_body_shape_despite_separate_vertex_collision_volumes() {
    use cyrus_assets::{
        animation::{AnimationModel, Animations},
        collision::ActorCollisionModels,
    };
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_script::script::{PlayerCollisionSource, ScriptInputs};
    for (exclude_body, target_x, group, combat, alternate, expected_hit) in [
        (false, 600, 0, false, false, true),
        (true, 600, 0, false, false, false),
        (false, 620, 121, true, true, true),
        (false, 620, 8, true, true, false),
        (false, 620, 121, false, true, false),
        (false, 620, 121, true, false, false),
        (true, 620, 121, true, true, false),
    ] {
        let mut scripts = cache();
        scripts.definition.functions.extend([
            Function {
                name: Native::ProjectileAI,
                param_count: 0,
                kind: CallKind::Task,
            },
            Function {
                name: Native::KillMe,
                param_count: 0,
                kind: CallKind::Function,
            },
        ]);
        let mut projectile = scripts.actors["LOGIC"].clone();
        projectile.code_origin += 1000;
        projectile.header_index += 100;
        projectile.instances = 1;
        projectile.namespace = None;
        projectile.instance_variables = vec![vec![]];
        projectile.program.variables = vec![];
        projectile.attributes = Attributes::from([(Attribute::ForwardSpeed, 200)]);
        projectile.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(1, Native::ProjectileAI, CallKind::Task, vec![]),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: wait(2, Native::KillMe, CallKind::Function, vec![]),
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        scripts.actors.insert("BALL".into(), projectile);
        for (placement, z) in scripts.instances.iter_mut().zip([600, 800]) {
            placement.object_type = PlacementType::MODEL;
            placement.active = 1;
            placement.map_position = [600, 20, z];
        }
        scripts.definition.functions.push(Function {
            name: Native::PlayAnimation,
            param_count: 2,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: u16::try_from(scripts.definition.functions.len() - 1)
                                .unwrap(),
                            kind: CallKind::Function,
                            target: Object::Me,
                            function: Native::PlayAnimation,
                            arguments: vec![
                                Value::Integer { value: group },
                                Value::Integer { value: 0 },
                            ],
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
        scripts.instances[1].map_position[0] = target_x;
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Combat] = u8::from(combat);
        scripts.content_id = scripts.content_identity().unwrap();
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
                "mpob_objects": scripts.instances.iter().map(|p| serde_json::json!({"script_name":p.actor,"object_type":1})).collect::<Vec<_>>(),
                "mpsz_entries":[{"total_extent":[48,48,48],"pos_extent":[24,24,24],
                    "neg_extent":[24,24,24],"center_offset":[0,0,0]}],
                "actors":[{"script_name":"LOGIC", "mpsz_bounds_1": if alternate {0} else {-1}, "model_load_path":"animation_table",
                    "ravc_count":2,"raex":{"unknown_12":0,"v_vertex":0,"v_size":0},
                    "raan_entries":[{"file_path":"BODY.3D","frame_count":3}],
                    "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                        "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                        {"group_index":8,"anim_id":1,"anim_type":0,
                         "commands":[{"opcode":0,"frame_reference":1},{"opcode":2,"value":0}]},
                        {"group_index":121,"anim_id":1,"anim_type":0,
                         "commands":[{"opcode":0,"frame_reference":2},{"opcode":2,"value":0}]}]}]}),
            models: BTreeMap::from([(
                "BODY".into(),
                AnimationModel {
                    radius: Some(42.0),
                    base_positions: vec![],
                    base_normals: vec![],
                    faces: vec![],
                    corner_normals: vec![],
                    encoding: "animated_i16".into(),
                    positions: vec![vec![], vec![], vec![]],
                    normals: vec![vec![], vec![], vec![]],
                },
            )]),
            sections: Default::default(),
            unsupported: Default::default(),
        };
        let models = ActorModelCollision::new(&ActorCollisionModels {
            object_count: 2,
            collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
            ..Default::default()
        })
        .unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.prepare_model_resources(&data, &models).unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        for id in ids {
            world
                .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
                .unwrap();
            world
                .prepare_ordinary_collision_shape(id, &data, &scene)
                .unwrap();
        }
        world
            .advance(
                ids[1],
                ScriptStage::Body(Thread::Main),
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
        assert_eq!(
            world.animation_frame(ids[1]).unwrap(),
            Some(match group {
                8 => 1,
                121 => 2,
                _ => 0,
            })
        );
        world.refresh_model_presentations(&data, &ids).unwrap();
        world
            .set_actor_attribute(1, 56, u8::from(exclude_body))
            .unwrap();
        let origin = world.spatial(ids[0]).unwrap().local.position;
        let ball = world.create_object(ids[0], "BALL", origin, [0; 3]).unwrap();
        // Exercise initial task flight and its retained poll through ordinary created-object updates.
        for _ in 0..2 {
            world
                .update_created(
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        player_collision: Some(PlayerCollisionSource {
                            scene: &scene,
                            animations: &data,
                            view: [600, 600],
                            actors: &ids,
                        }),
                        ..Default::default()
                    },
                    &mut Default::default(),
                    &mut |_, _| Ok(()),
                    100,
                )
                .unwrap();
        }
        let actor = &world.actors()[ball.index];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());

        assert_eq!(
            serde_json::from_value::<[i32; 3]>(
                world.inspect(ball.index, 1)["actors"][0]["spatial"]["local"]["position"].clone(),
            )
            .unwrap(),
            if !expected_hit {
                [600 << 8, 20 << 8, 1000 << 8]
            } else {
                origin
            }
        );
        assert_eq!(actor.control_state().unwrap().killed, expected_hit);
    }
}
