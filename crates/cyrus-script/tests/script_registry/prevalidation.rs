use super::*;

#[test]
fn ordinary_ground_prevalidation_preserves_slope_direction_and_animation_exclusions() {
    use cyrus_collision::ground::GroundSupport;
    use cyrus_script::script::{OrdinaryGroundPrevalidation, ordinary_ground_prevalidation};
    let flat = GroundSupport {
        height: 0.0,
        normal: [0, -256, 0],
    };
    let steep = GroundSupport {
        height: 0.0,
        normal: [0, -100, 256],
    };
    let clear = OrdinaryGroundPrevalidation {
        rollback: false,
        sliding: false,
    };
    let slide = OrdinaryGroundPrevalidation {
        rollback: false,
        sliding: true,
    };
    let veto = OrdinaryGroundPrevalidation {
        rollback: true,
        sliding: false,
    };
    assert_eq!(
        ordinary_ground_prevalidation(None, None, [0; 3], 0, false).unwrap(),
        clear
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(flat), Some(flat), [0; 3], 0, false).unwrap(),
        clear
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(steep), [0; 3], 1, true).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [-256, 0, 0], 0, false).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [256, 0, 0], 0, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, 256], 0, false).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, -256], 0, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, 256], 1, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, 256], 0, true).unwrap(),
        veto
    );
    // A missing side is neither class 2 nor class 4 (the original reads uninitialised data).
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), None, [-256, 0, 0], 0, false).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), None, [256, 0, 0], 0, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(flat), None, [0; 3], 0, false).unwrap(),
        clear
    );
    assert_eq!(
        ordinary_ground_prevalidation(None, Some(steep), [0; 3], 0, false).unwrap(),
        clear
    );
}

#[test]
fn ordinary_ground_prevalidation_queries_retained_support_and_restores_local_motion() {
    use cyrus_script::script::{
        ObstacleEnvironment, OrdinaryGroundPrevalidation, OrdinaryGroundQuery, ScriptInputs,
        ScriptStage,
    };
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 2;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 1;
    let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    world.snapshot_spatial(id).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 1,
        sidedness: false,
    };
    assert_eq!(
        world
            .query_ordinary_ground_prevalidation(id, query(), false)
            .unwrap(),
        OrdinaryGroundPrevalidation::default()
    );
    assert_eq!(
        world
            .query_ordinary_ground_prevalidation(id, query(), true)
            .unwrap(),
        OrdinaryGroundPrevalidation::default()
    );

    world
        .apply_ordinary_falling_vertical(
            other,
            cyrus_script::script::OrdinaryVerticalInputs::for_falling(0, 14, 0, 50, false).unwrap(),
        )
        .unwrap();
    world.refresh_model_presentations(&data, &[other]).unwrap();
    assert_eq!(
        world.query_ordinary_queued_clearance(id, query()).unwrap(),
        256
    );
    // The clearance segment starts at the query-position selector's position: a link position far
    // from every model finds none.
    world.set_link_position_for_test(id, [4000 * 256, 20 * 256, 4000 * 256]);
    assert_eq!(
        world.query_ordinary_queued_clearance(id, query()).unwrap(),
        256000
    );
    let empty = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
        .unwrap();
    assert_eq!(
        world
            .query_ordinary_queued_clearance(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &empty,
                    distance: 1,
                    sidedness: false,
                }
            )
            .unwrap(),
        256000
    );

    let (mut world, data) = overlap_response_fixture(128, 0, 1024);
    let id = world.id(0).unwrap();
    world.snapshot_spatial(id).unwrap();
    let before = world.spatial(id).unwrap().local.position;
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
    assert_ne!(world.spatial(id).unwrap().local.position, before);
    world
        .apply_ordinary_ground_prevalidation(
            id,
            OrdinaryGroundPrevalidation {
                rollback: true,
                sliding: false,
            },
            &data,
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
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::NONE
    );
}

fn tilted_surface_model(
    normal: [f32; 3],
    vertices: Vec<[f32; 3]>,
) -> cyrus_assets::collision::CollisionModel {
    use cyrus_assets::collision::{CollisionGeometry, CollisionModel, CollisionSubobject};
    CollisionModel {
        flags: 0,
        geometry: CollisionGeometry {
            version: Some(*b"v5.0"),
            radius: 1000.0,
            vertices,
            normals: vec![normal],
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
fn step_hazard_is_a_steep_destination_surface_unless_both_points_are_floors() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::OrdinaryObstacleQuery;
    let model = |kind: &str| match kind {
        "floor" => tilted_surface_model(
            [0.0, -1.0, 0.0],
            vec![
                [-10.0, 0.0, -10.0],
                [10.0, 0.0, -10.0],
                [10.0, 0.0, 10.0],
                [-10.0, 0.0, 10.0],
            ],
        ),
        _ => tilted_surface_model(
            [-0.866, -0.5, 0.0],
            vec![
                [-5.0, 8.66, -10.0],
                [5.0, -8.66, -10.0],
                [5.0, -8.66, 10.0],
                [-5.0, 8.66, 10.0],
            ],
        ),
    };
    for (surface, collide, expected) in [
        (Some("slope"), 10, true),
        (Some("floor"), 10, false),
        (Some("slope"), 8, false),
        (None, 10, false),
    ] {
        let (mut world, data) = overlap_response_fixture_with(128, 0, 1024, collide);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        world.snapshot_spatial(ids[0]).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        let current = world.spatial(ids[0]).unwrap().local.position;
        let destination = [current[0] + 594 * 256, current[1], current[2]];
        // The body shape is prepared with the first admission, so read its extent from an
        // empty scene before placing the surface.
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &[ids[0], ids[1]])
            .unwrap();
        let centre = (destination[1] >> 8)
            + world
                .ordinary_collision_shape(ids[0])
                .unwrap()
                .positive_extent[1];
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: surface
                .map(|_| StaticPlacement {
                    id: 1,
                    model: "SURFACE".into(),
                    position: [destination[0] >> 8, centre, destination[2] >> 8],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .into_iter()
                .collect(),
            models: surface
                .map(|kind| BTreeMap::from([("SURFACE".into(), model(kind))]))
                .unwrap_or_default(),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &[ids[0], ids[1]])
            .unwrap();
        let query = OrdinaryObstacleQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            vertical_offset: 0,
            respond: false,
        };
        assert_eq!(
            world.step_hazard(ids[0], None, destination, query).unwrap(),
            expected,
            "surface {} collide {collide}",
            surface.is_some()
        );
    }
}

#[test]
fn ground_prevalidation_retains_shared_results_after_destination_and_receiver_changes() {
    use cyrus_collision::ground::TerrainGround;
    use cyrus_script::script::{OrdinaryGroundPrevalidation, OrdinaryGroundQuery};
    for bypass_first in [false, true] {
        let mut scripts = cache();
        for placement in &mut scripts.instances {
            placement.active = 1;
        }
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 2;
        let (mut world, data, models) =
            static_model_fixture_at(scripts, Some(20.0), [600, 2400, 600]);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, other])
            .unwrap();
        world.snapshot_spatial(id).unwrap();
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.settings.post_collide_height = 5000;
        let heights = (0_usize..256 * 256)
            .map(|index| ((index % 256).min(16) * 1024) as i16)
            .collect();
        scene.terrain = Some(TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap());
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
            .unwrap();
        let result = world
            .query_ordinary_ground_prevalidation(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 1,
                    sidedness: false,
                },
                bypass_first,
            )
            .unwrap();
        assert_eq!(
            result,
            OrdinaryGroundPrevalidation {
                rollback: false,
                sliding: !bypass_first
            }
        );
        let models = scene.actor_collision.take();
        scene.terrain = None;
        assert_eq!(
            world
                .query_ordinary_ground_prevalidation(
                    other,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    },
                    false
                )
                .unwrap(),
            result
        );
        scene.actor_collision = models;
        scene.terrain =
            Some(TerrainGround::new(vec![2400; 256 * 256], vec![false; 256 * 256], None).unwrap());
        world
            .update_ordinary(
                &data,
                [600, 600],
                cyrus_script::script::OrdinaryInputs {
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        for actor in world.actors() {
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
        }
        assert_eq!(
            world
                .query_ordinary_ground_prevalidation(
                    id,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    },
                    false
                )
                .unwrap(),
            OrdinaryGroundPrevalidation::default()
        );
    }
}

#[test]
fn ground_prevalidation_does_not_turn_an_unsupported_first_query_into_success() {
    use cyrus_script::script::OrdinaryGroundQuery;
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 2;
    let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    world.snapshot_spatial(id).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
        .unwrap();
    scene.actor_collision = None;
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 1,
        sidedness: false,
    };
    let error = world
        .query_ordinary_ground_prevalidation(id, query(), false)
        .unwrap_err();
    assert!(format!("{error:#}").contains("live geometry unavailable"));
    let error = world
        .query_ordinary_ground_prevalidation(id, query(), true)
        .unwrap_err();
    assert!(format!("{error:#}").contains("earlier unsupported query"));
}
