//! The follow camera's body contacts take the original's polygon-sidedness setting (preservation:
//! "Camera contact-normal response"; the original's camera body routine enables sidedness before
//! its sphere query), so a face more than 4 map units behind a body sphere's centre is not a contact.
use super::*;
use cyrus_collision::actor_models::ActorModelCollision;
use cyrus_collision::collision_world::StaticCollision;
use cyrus_script::script::camera::CameraGeometry;

fn contacts_at(y: i32) -> Vec<[i32; 3]> {
    use cyrus_assets::collision::{ActorCollisionModels, CollisionScene, StaticPlacement};
    let (world, _, _) = static_model_fixture(cache());
    // A floor at y 0 whose stored normal points toward -y (the shape used by the collision tests).
    let mut statics = CollisionScene::default();
    statics.models.insert("floor".into(), floor_model());
    statics.placements = vec![StaticPlacement {
        id: 10,
        position: [600, 0, 600],
        model: "floor".into(),
        rotation: [268_435_456, 0, 0, 0, 268_435_456, 0, 0, 0, 268_435_456],
    }];
    let mut scene = ordinary_collision_scene(
        world.world,
        ActorModelCollision::new(&ActorCollisionModels::default()).unwrap(),
    );
    scene.static_collision = StaticCollision::new(&statics).unwrap();
    let limits = cyrus_assets::script::CameraLimits {
        obstacle_size: 20,
        max_acc: 5000,
        max_y_vel: 5000,
        camera_size: 10,
        camera_scape_size: 5,
    };
    let mut geometry = world
        .scene_camera_geometry::<NoTerrain>(&scene, [600, 600], &[], limits, None)
        .unwrap();
    let id = world.id(0).unwrap();
    geometry.contacts([600 << 8, y << 8, 600 << 8], id).unwrap()
}

struct NoTerrain;
impl cyrus_script::script::camera::CameraTerrainHeights for NoTerrain {
    fn retained_center(&self) -> [i32; 2] {
        [-1000, -1000]
    }
    fn retained_height(&self, _: [i32; 2]) -> anyhow::Result<f32> {
        anyhow::bail!("unused")
    }
    fn map_height(&self, _: [i32; 2]) -> anyhow::Result<f32> {
        anyhow::bail!("unused")
    }
}

fn floor_model() -> cyrus_assets::collision::CollisionModel {
    use cyrus_assets::collision::{CollisionGeometry, CollisionModel, CollisionSubobject};
    CollisionModel {
        flags: 0,
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
    }
}

#[test]
fn a_face_behind_the_camera_body_sphere_is_not_a_contact() {
    // Sphere centre at y = +10: the face's signed plane distance is +10, above the 4 unit
    // sidedness limit, so it is rejected although the sphere radius (10) reaches the plane.
    assert!(contacts_at(10).is_empty());
}

#[test]
fn a_face_in_front_of_the_camera_body_sphere_is_a_contact() {
    // Control: centre at y = -10, distance -10 passes the sidedness test and touches the sphere.
    let contacts = contacts_at(-10);
    assert_eq!(contacts.len(), 1, "{contacts:?}");
    assert_eq!(contacts[0], [0, -256, 0]);
}
