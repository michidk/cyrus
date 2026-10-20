//! The queries the camera's geometry adapter needs from the static registry: the view window's
//! models in cell order and the segment contacts of one collected model.
use cyrus_assets::collision::{
    CollisionGeometry, CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement,
};
use cyrus_collision::candidates::ModelInstance;
use cyrus_collision::collision_world::StaticCollision;
use std::collections::BTreeMap;

fn scene() -> StaticCollision {
    let mut scene = CollisionScene::default();
    scene.models.insert(
        "floor".into(),
        CollisionModel {
            flags: 4,
            geometry: CollisionGeometry {
                version: Some(*b"v5.0"),
                textures: None,
                radius: 100.0,
                vertices: vec![[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [0.0, 0.0, 50.0]],
                normals: vec![[0.0, -1.0, 0.0]],
                faces: vec![vec![0, 1, 2]],
                subobjects: vec![CollisionSubobject {
                    center: [0.0; 3],
                    radius: 100.0,
                    extents: Some([1.0; 3]),
                    faces: vec![0],
                }],
            },
        },
    );
    for (id, position) in [(0, [1000, -8, 1000]), (1, [60000, -8, 60000])] {
        scene.placements.push(StaticPlacement {
            id,
            position,
            model: "floor".into(),
            rotation: [268_435_456, 0, 0, 0, 268_435_456, 0, 0, 0, 268_435_456],
        });
    }
    StaticCollision::new(&scene).unwrap()
}

#[test]
fn the_window_lists_only_placements_near_the_view_without_a_distance_test() {
    let world = scene();
    let near = world.window_models([1000, 1000]).unwrap();
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].instance, ModelInstance::StaticPlacement(0));
    assert_eq!(near[0].radius, 100.0);
    assert_eq!(near[0].position, [1000.0, -8.0, 1000.0]);
    let far = world.window_models([60000, 60000]).unwrap();
    assert_eq!(far[0].instance, ModelInstance::StaticPlacement(1));
    assert!(world.window_models([30000, 30000]).unwrap().is_empty());
    assert!(
        world.window_models([-1, 0]).is_err(),
        "a view outside the map is rejected"
    );
}

#[test]
fn a_collected_static_model_reports_the_segment_contacts_of_its_polygons() {
    let world = scene();
    let none = BTreeMap::new();
    let model = ModelInstance::StaticPlacement(0);
    let contacts = |start: [i32; 3], end: [i32; 3]| {
        world
            .model_segment_contacts(model, None, &none, start, end, false)
            .unwrap()
    };
    let down = contacts([1000, -108, 1000], [1000, 92, 1000]);
    let up = contacts([1000, 92, 1000], [1000, -108, 1000]);
    assert_eq!(
        (down.len(), up.len()),
        (1, 1),
        "the original's default mode reports the crossing from both sides"
    );
    assert_eq!((down[0].face, up[0].face), (0, 0));
    assert!(
        contacts([5000, -108, 5000], [5000, 92, 5000]).is_empty(),
        "a segment elsewhere misses"
    );
}

#[test]
fn a_live_model_needs_its_pose_and_the_live_registry() {
    let world = scene();
    let error = world
        .model_segment_contacts(
            ModelInstance::Runtime(3),
            None,
            &BTreeMap::new(),
            [0; 3],
            [0, 100, 0],
            false,
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("stored model pose unavailable"),
        "{error:#}"
    );
}
