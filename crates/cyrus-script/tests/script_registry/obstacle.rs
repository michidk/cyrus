use super::*;

#[test]
fn ordinary_forward_hit_keeps_rank_ties_and_combines_later_normals_in_order() {
    use cyrus_collision::collision_mesh::IntegerSegmentContact;
    use cyrus_script::script::{OrdinaryForwardHit, ordinary_forward_hit};
    let contacts = [
        IntegerSegmentContact {
            face: 0,
            rank: 0x8002,
            point: [10, 20, 30],
            normal: [256, 0, 0],
        },
        IntegerSegmentContact {
            face: 1,
            rank: 2,
            point: [40, 50, 60],
            normal: [256, 0, 256],
        },
        IntegerSegmentContact {
            face: 2,
            rank: 5,
            point: [70, 80, 90],
            normal: [256, 0, -256],
        },
        IntegerSegmentContact {
            face: 3,
            rank: 0,
            point: [100, 110, 120],
            normal: [-256, 0, 0],
        },
    ];
    assert_eq!(
        ordinary_forward_hit(512, &contacts),
        Some(OrdinaryForwardHit {
            point: [10, 20, 30],
            normal: [245, 0, -73],
        })
    );
    let reordered = [
        contacts[1].clone(),
        contacts[2].clone(),
        contacts[0].clone(),
    ];
    assert_eq!(
        ordinary_forward_hit(512, &reordered),
        Some(OrdinaryForwardHit {
            point: [40, 50, 60],
            normal: [256, 0, 0],
        })
    );
}

#[test]
fn ordinary_forward_facing_retains_strict_boundary_and_2047_wrap() {
    use cyrus_collision::collision_mesh::IntegerSegmentContact;
    use cyrus_script::script::ordinary_forward_hit;
    let contact = IntegerSegmentContact {
        face: 0,
        rank: 1,
        point: [0; 3],
        normal: [256, 0, 0],
    };
    for (heading, hit) in [(0, false), (1, true), (1023, true), (1024, false)] {
        assert_eq!(
            ordinary_forward_hit(heading, std::slice::from_ref(&contact)).is_some(),
            hit
        );
    }
    assert!(
        ordinary_forward_hit(
            2047,
            &[IntegerSegmentContact {
                normal: [0, 0, -256],
                ..contact
            }]
        )
        .is_some()
    );
}

pub(super) fn ordinary_forward_wall_model() -> cyrus_assets::collision::CollisionModel {
    use cyrus_assets::collision::{CollisionGeometry, CollisionModel, CollisionSubobject};
    CollisionModel {
        flags: 0,
        geometry: CollisionGeometry {
            version: Some(*b"v5.0"),
            radius: 1000.0,
            vertices: vec![
                [0.0, -100.0, -100.0],
                [0.0, -100.0, 100.0],
                [0.0, 100.0, 100.0],
                [0.0, 100.0, -100.0],
            ],
            normals: vec![[-1.0, 0.0, 0.0]],
            faces: vec![vec![0, 1, 2, 3]],
            textures: None,
            subobjects: vec![CollisionSubobject {
                center: [0.0; 3],
                radius: 1000.0,
                extents: Some([1.0; 3]),
                faces: vec![0],
            }],
        },
    }
}

#[test]
fn ordinary_forward_obstacle_uses_effective_motion_and_preserves_y_and_model_pose() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{OrdinaryObstacleQuery, ScriptInputs, ScriptStage};
    for (offset, respond, expected_hit, expected_x, wrapper, linked) in [
        (0, false, true, 264704, false, false),
        (0, true, true, 143872, false, false),
        (500, true, false, 264704, false, false),
        (0, true, true, 143872, true, false),
        // A linked actor's motion runs from its previous link position to its link position,
        // which did not move, so it meets no wall (the local motion would).
        (0, false, false, 264704, false, true),
    ] {
        let (mut world, data) = overlap_response_fixture(128, 0, 1024);
        let id = world.id(0).unwrap();
        world.snapshot_spatial(id).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: vec![StaticPlacement {
                id: 1,
                model: "WALL".into(),
                position: [604, 20, 30],
                rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
            }],
            models: BTreeMap::from([("WALL".into(), ordinary_forward_wall_model())]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
            .unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        if linked {
            world.set_link_position_for_test(id, [0, 5120, 0]);
            world.set_link_position_for_test(id, [0, 5120, 0]);
        }
        if wrapper {
            world
                .resolve_ordinary_obstacle_wrapper(
                    id,
                    OrdinaryObstacleQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        vertical_offset: offset,
                        respond,
                    },
                    cyrus_script::script::ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 0,
                    },
                    &mut |_, _| Ok(()),
                )
                .unwrap();
        } else {
            assert_eq!(
                world
                    .resolve_ordinary_forward_obstacle(
                        id,
                        OrdinaryObstacleQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            vertical_offset: offset,
                            respond,
                        }
                    )
                    .unwrap(),
                expected_hit
            );
        }
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [expected_x, 5120, 7680]
        );
        assert_eq!(
            world.model_presentation(id).unwrap().unwrap().pose.position,
            [2560, 5120, 7680]
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            if wrapper { 0x800 } else { 0 }
        );
    }
}

#[test]
fn ordinary_obstacle_transition_preserves_grounded_and_state_fifteen_and_restarts_falls() {
    use cyrus_script::script::{
        ObstacleFallStart, ObstacleTransition, ObstacleTransitionInput,
        ordinary_obstacle_transition,
    };
    let base = ObstacleTransitionInput {
        living_state: LivingState::ORDINARY,
        falling_state: JumpMotion::NONE,
        queued_motion: JumpMotion::NONE,
        logical_y: 2560,
        fall_attribute: 7,
        holding_weapon: false,
        current_group: None,
    };
    assert_eq!(
        ordinary_obstacle_transition(base),
        Some(ObstacleTransition {
            living_state: LivingState::ORDINARY,
            falling_state: JumpMotion::NONE,
            queued_motion: JumpMotion::NONE,
            body_response_flags: CollisionFlags::default(),
            clear_motion_vector: false,
            fall_start: None,
        })
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: LivingState::HANGING,
            falling_state: JumpMotion::from_byte(0x88),
            queued_motion: JumpMotion::ROPE,
            ..base
        }),
        None
    );
    for living in (27..=32).map(LivingState::from_byte) {
        assert_eq!(
            ordinary_obstacle_transition(ObstacleTransitionInput {
                living_state: living,
                ..base
            }),
            Some(ObstacleTransition {
                living_state: LivingState::ORDINARY,
                falling_state: JumpMotion::STANDING,
                queued_motion: JumpMotion::NONE,
                body_response_flags: CollisionFlags::PREMETURE_JUMP_END,
                clear_motion_vector: true,
                fall_start: Some(ObstacleFallStart {
                    origin_y: 4352,
                    timer: 30,
                    force_group: Some(AnimationGroup::FALL)
                })
            })
        );
    }
    for (queued, holding, current, falling, group, clear) in [
        (
            JumpMotion::ROPE,
            false,
            None,
            JumpMotion::STANDING,
            Some(AnimationGroup::FALL),
            true,
        ),
        (
            JumpMotion::from_byte(5),
            false,
            None,
            JumpMotion::STANDING,
            Some(AnimationGroup::FALL),
            false,
        ),
        (
            JumpMotion::from_byte(0x85),
            false,
            None,
            JumpMotion::from_byte(0x81),
            Some(AnimationGroup::FALL_UP_ARMS),
            false,
        ),
        (
            JumpMotion::from_byte(5),
            true,
            None,
            JumpMotion::STANDING,
            Some(AnimationGroup::FIGHT_FALL),
            false,
        ),
        (
            JumpMotion::from_byte(5),
            true,
            Some(AnimationGroup::FIGHT_FALL),
            JumpMotion::STANDING,
            None,
            false,
        ),
    ] {
        assert_eq!(
            ordinary_obstacle_transition(ObstacleTransitionInput {
                queued_motion: queued,
                holding_weapon: holding,
                current_group: current,
                ..base
            }),
            Some(ObstacleTransition {
                living_state: LivingState::ORDINARY,
                falling_state: falling,
                queued_motion: JumpMotion::NONE,
                body_response_flags: CollisionFlags::PREMETURE_JUMP_END,
                clear_motion_vector: clear,
                fall_start: Some(ObstacleFallStart {
                    origin_y: 4352,
                    timer: 30,
                    force_group: group
                }),
            })
        );
    }
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            falling_state: JumpMotion::from_byte(0x8f),
            ..base
        }),
        Some(ObstacleTransition {
            living_state: LivingState::ORDINARY,
            falling_state: JumpMotion::from_byte(0x81),
            queued_motion: JumpMotion::NONE,
            body_response_flags: CollisionFlags::PREMETURE_FALL,
            clear_motion_vector: false,
            fall_start: None
        })
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: LivingState::from_byte(26),
            ..base
        })
        .unwrap()
        .living_state,
        LivingState::from_byte(26)
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: LivingState::LIGHTING_TORCH,
            ..base
        })
        .unwrap()
        .living_state,
        LivingState::LIGHTING_TORCH
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            queued_motion: JumpMotion::STANDING,
            logical_y: i32::MAX,
            fall_attribute: 255,
            ..base
        })
        .unwrap()
        .fall_start
        .unwrap()
        .origin_y,
        -2147418369
    );
}

#[test]
fn ordinary_overlap_stage_repeats_obstacle_wrapper_after_weighted_static_separation() {
    use cyrus_assets::collision::StaticPlacement;
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{
        ObstacleEnvironment, OrdinaryObstacleQuery, ScriptInputs, ScriptStage,
    };
    let (mut world, data) = overlap_response_fixture(128, 0, 512);
    let id = world.id(0).unwrap();
    world.snapshot_spatial(id).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let mut scene = static_overlap_scene(world.world);
    scene
        .collision
        .models
        .insert("WALL".into(), ordinary_forward_wall_model());
    scene.collision.placements.push(StaticPlacement {
        id: 2,
        model: "WALL".into(),
        position: [512, 20, 30],
        rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
    });
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
        .unwrap();
    world
        .resolve_ordinary_overlap_stage(
            id,
            OrdinaryObstacleQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                vertical_offset: 9999,
                respond: false,
            },
            ObstacleEnvironment {
                selected_player: None,
                auto_grab: false,
                item_system_ready: false,
                shell_execution: false,
                ticks: 0,
            },
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [120320, 5120, 7680]
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits(),
        0xc00
    );
    assert!(
        !world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .contains(CollisionFlags::ROLLBACK)
    );
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        [2560, 5120, 7680]
    );
}
