use super::*;

#[test]
fn ordinary_bounds_endpoints_keep_stored_negative_magnitudes_and_positive_y() {
    use cyrus_script::script::{OrdinaryCollisionShape, ordinary_bounds_segments};
    let shape = OrdinaryCollisionShape {
        retained: cyrus_script::script::RetainedBounds::only(cyrus_script::script::BoundsRecord {
            positive_extent: [10, 20, 30],
            negative_extent: [40, 50, 60],
            total_extent: [50, 70, 90],
            center_offset: [0; 3],
        }),
        total_extent: [50, 70, 90],
        center_offset: [0; 3],
        positive_extent: [10, 20, 30],
        negative_extent: [40, 50, 60],
        body: vec![],
        additional: None,
    };
    assert_eq!(
        ordinary_bounds_segments(
            &shape,
            cyrus_world::transform::Matrix::orientation([0; 3]),
            [256, 512, 768]
        )
        .unwrap(),
        [[[11, 24, 33], [41, 20, 63]], [[41, 24, 33], [11, 20, 63]],]
    );
}

#[test]
fn ordinary_bounds_separation_keeps_independent_rank_limits_and_earlier_contributions() {
    use cyrus_collision::collision_mesh::IntegerSegmentContact;
    use cyrus_script::script::{
        BoundsSegment::{First, Second},
        ordinary_bounds_separation,
    };
    let contact = |rank: i32, normal| IntegerSegmentContact {
        face: 0,
        rank: rank as u32,
        point: [0; 3],
        normal,
    };
    assert_eq!(
        ordinary_bounds_separation(&[
            (First, contact(10, [100, -41, 0])),
            (First, contact(5, [0, 160, 100])),
            (First, contact(7, [999, 0, 0])),
            (First, contact(5, [999, 0, 0])),
            (Second, contact(5, [200, 0, 0])),
            (First, contact(0, [999, 0, 0])),
            (First, contact(-999999, [999, 0, 0])),
            (First, contact(-10, [0, 0, 200])),
            (First, contact(-20, [999, 0, 0])),
            (First, contact(-5, [5, 0, 0])),
            (First, contact(1, [999, -42, 0])),
            (First, contact(1, [999, 161, 0])),
        ]),
        [305, 119, 300]
    );
}

#[test]
fn ordinary_bounds_query_commits_raw_normals_to_all_axes_and_marks_only_nonzero_sum() {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement,
    };
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{OrdinaryObstacleQuery, ScriptInputs, ScriptStage};
    for (normal_scale, expected_position, expected_flags) in [
        (0.25, [133632, 5248, 7936], 0x4000),
        (0.75, [133632, 5120, 7680], 0),
    ] {
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
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: (0..2)
                .map(|id| StaticPlacement {
                    id,
                    model: "SLOPE".into(),
                    position: [542, 20, 50],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .collect(),
            models: BTreeMap::from([(
                "SLOPE".into(),
                CollisionModel {
                    flags: 0,
                    geometry: CollisionGeometry {
                        version: Some(*b"v5.0"),
                        radius: 1000.0,
                        vertices: vec![
                            [-100.0, -200.0, 100.0],
                            [100.0, -200.0, 100.0],
                            [100.0, 200.0, -100.0],
                            [-100.0, 200.0, -100.0],
                        ],
                        normals: vec![[0.0, normal_scale, normal_scale * 2.0]],
                        faces: vec![vec![0, 1, 2, 3]],
                        textures: None,
                        subobjects: vec![CollisionSubobject {
                            center: [0.0; 3],
                            radius: 1000.0,
                            extents: Some([1.0; 3]),
                            faces: vec![0],
                        }],
                    },
                },
            )]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
            .unwrap();
        world
            .resolve_ordinary_bounds_obstacle(
                id,
                OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 9999,
                    respond: false,
                },
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected_position);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            expected_flags
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
}
