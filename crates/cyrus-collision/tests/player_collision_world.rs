use cyrus_assets::collision::CollisionModel;
use cyrus_assets::collision::CollisionScene;
use cyrus_assets::collision::CollisionSubobject;
use cyrus_assets::collision::StaticPlacement;
use cyrus_collision::collision_world::StaticCollision;
use cyrus_collision::ground::GroundProbe;

fn model(flags: u8) -> CollisionModel {
    CollisionModel {
        version: Some(*b"v5.0"),
        textures: None,
        radius: 100.0,
        flags,
        vertices: vec![[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [0.0, 0.0, 50.0]],
        normals: vec![[0.0, -1.0, 0.0]],
        faces: vec![vec![0, 1, 2]],
        subobjects: vec![CollisionSubobject {
            center: [0.0; 3],
            radius: 100.0,
            extents: Some([1.0; 3]),
            faces: vec![0],
        }],
    }
}

fn placement(id: u32, position: [i32; 3], model: &str) -> StaticPlacement {
    StaticPlacement {
        id,
        position,
        model: model.into(),
        rotation: [268_435_456, 0, 0, 0, 268_435_456, 0, 0, 0, 268_435_456],
    }
}

#[test]
fn support_metadata_retains_all_model_flags_and_terrain_clears_them() {
    use cyrus_collision::ground::SupportSource;
    let mut scene = CollisionScene::default();
    scene.models.insert("floor".into(), model(0x84));
    scene.placements.push(placement(7, [600, 0, 600], "floor"));
    let world = StaticCollision::new(&scene).unwrap();
    assert_eq!(
        world
            .support_metadata(SupportSource::Model { model: 0, face: 0 })
            .unwrap(),
        0x84
    );
    assert_eq!(world.support_metadata(SupportSource::Terrain).unwrap(), 0);
    assert!(
        world
            .support_metadata(SupportSource::Model { model: 1, face: 0 })
            .is_err()
    );
}

#[test]
fn selected_player_query_commits_history_on_hits_and_misses() {
    use cyrus_collision::ground::FallbackProbe;
    use cyrus_collision::ground::GroundSupport;
    use cyrus_collision::ground::SupportHistory;
    let mut scene = CollisionScene::default();
    scene.models.insert("floor".into(), model(4));
    scene.placements.push(placement(7, [600, 0, 600], "floor"));
    let world = StaticCollision::new(&scene).unwrap();
    let position = [600 * 256, 0, 600 * 256];
    let candidates = world.candidates([5000, 5000], position, None).unwrap();
    let mut history = SupportHistory::new(0, 0);
    let query = |position, terrain, history: &mut SupportHistory| {
        world
            .selected_player_support(
                &candidates,
                &GroundProbe::new(position, 0, 50).unwrap(),
                terrain,
                &FallbackProbe::new(position, [0; 3], 0, history.ticks() != 0).unwrap(),
                true,
                history,
            )
            .unwrap()
    };
    assert!(query(position, None, &mut history).is_some());
    assert_eq!(history.ticks(), 12);
    history.begin_player_update(5);
    let missed_position = [1000 * 256, 0, 1000 * 256];
    assert!(query(missed_position, None, &mut history).is_none());
    assert_eq!(history.ticks(), 12);
    assert!(
        query(
            missed_position,
            Some(GroundSupport {
                height: 0.0,
                normal: [0, -256, 0],
            }),
            &mut history
        )
        .is_some()
    );
    history.begin_player_update(5);
    assert!(query(missed_position, None, &mut history).is_none());
    assert_eq!(history.ticks(), 7);
}

#[test]
fn static_candidates_keep_cells_reverse_authored_order_and_separate_flags() {
    let mut scene = CollisionScene::default();
    scene.models.insert("floor".into(), model(0));
    scene.models.insert("actor".into(), model(1));
    scene.models.insert("excluded".into(), model(2));
    scene.placements = vec![
        placement(0, [600, 0, 600], "floor"),
        placement(1, [1200, 0, 600], "floor"),
        placement(2, [700, 0, 700], "floor"),
        placement(3, [0, 0, 0], "floor"),
        placement(4, [800, 0, 600], "actor"),
        placement(5, [900, 0, 600], "excluded"),
    ];
    let world = StaticCollision::new(&scene).unwrap();
    let candidates = world
        .candidates([5000, 5000], [1000 * 256, 0, 1000 * 256], Some(1000.0))
        .unwrap();
    assert_eq!(candidates.faces, [2, 0, 1]);
    assert_eq!(candidates.actors, [4]);
    let contacts = world
        .ground_contacts(
            &candidates,
            &GroundProbe::new([600 * 256, 0, 600 * 256], 0, 50).unwrap(),
            true,
        )
        .unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].model, 0);
}

#[test]
fn loaded_static_world_transforms_ground_contacts_and_preserves_rank() {
    let mut scene = CollisionScene::default();
    scene.models.insert("floor".into(), model(0));
    scene
        .placements
        .push(placement(77, [1000, -20, 1000], "floor"));
    let world = StaticCollision::new(&scene).unwrap();
    let position = [1000 * 256, -20 * 256, 1000 * 256];
    let candidates = world.candidates([5000, 5000], position, None).unwrap();
    assert_eq!(candidates.faces, [0]);
    let contacts = world
        .ground_contacts(
            &candidates,
            &GroundProbe::new(position, 0, 50).unwrap(),
            true,
        )
        .unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].model, 0);
    assert_eq!(contacts[0].support.height, -20.0);
    assert_eq!(contacts[0].support.normal, [0, -256, 0]);
    assert_eq!(contacts[0].rank, 16384);
}

#[test]
fn static_face_overflow_ends_scan_but_actor_overflow_only_skips_entries() {
    let mut scene = CollisionScene::default();
    scene.models.insert("floor".into(), model(0));
    scene.models.insert("actor".into(), model(1));
    // Reverse authored order visits 65 actors, 65 floors, then one final actor.
    scene.placements.push(placement(0, [600, 0, 600], "actor"));
    for id in 1..=65 {
        scene.placements.push(placement(id, [600, 0, 600], "floor"));
    }
    for id in 66..=130 {
        scene.placements.push(placement(id, [600, 0, 600], "actor"));
    }
    let world = StaticCollision::new(&scene).unwrap();
    let candidates = world
        .candidates([5000, 5000], [600 * 256, 0, 600 * 256], None)
        .unwrap();
    assert_eq!(candidates.actors, (67..=130).rev().collect::<Vec<_>>());
    assert_eq!(candidates.faces, (2..=65).rev().collect::<Vec<_>>());

    scene.placements.truncate(66);
    let world = StaticCollision::new(&scene).unwrap();
    let candidates = world
        .candidates([5000, 5000], [600 * 256, 0, 600 * 256], None)
        .unwrap();
    assert!(candidates.actors.is_empty());
    assert_eq!(candidates.faces, (2..=65).rev().collect::<Vec<_>>());
}

#[test]
fn body_probe_uses_squared_radius_sum_and_original_polygons() {
    use bevy::math::{Mat3, Vec3};
    let mut scene = CollisionScene::default();
    let mut floor = model(0);
    floor.subobjects[0].radius = 1000.0;
    for vertex in &mut floor.vertices {
        vertex[1] = -50.0;
    }
    scene.models.insert("floor".into(), floor);
    scene
        .placements
        .push(placement(0, [1000, 0, 1000], "floor"));
    let world = StaticCollision::new(&scene).unwrap();
    let position = [1000 * 256, -60 * 256, 1000 * 256];
    let candidates = world.candidates([5000, 5000], position, None).unwrap();
    let contacts = world
        .body_contacts(
            &candidates,
            position,
            Mat3::IDENTITY,
            &[(Vec3::ZERO, 11.0), (Vec3::ZERO, 12.0)],
            false,
        )
        .unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].model, 0);
    assert_eq!(contacts[0].face, 0);
    assert_eq!(contacts[0].point, Vec3::new(1000.0, -50.0, 1000.0));
    assert_eq!(contacts[0].normal, Vec3::NEG_Y);
    assert!(
        world
            .blocks_body(
                &candidates,
                position,
                Mat3::IDENTITY,
                &[(Vec3::ZERO, 11.0)],
                false
            )
            .unwrap()
    );
    assert!(
        !world
            .blocks_body(
                &candidates,
                position,
                Mat3::IDENTITY,
                &[(Vec3::ZERO, 9.0)],
                false
            )
            .unwrap()
    );
    // A large sphere reaches the polygon, but the model gate rejects it.
    // Squaring the sum of radii would incorrectly admit this pair.
    assert!(
        !world
            .blocks_body(
                &candidates,
                [1000 * 256, -150 * 256, 1000 * 256],
                Mat3::IDENTITY,
                &[(Vec3::ZERO, 110.0)],
                false
            )
            .unwrap()
    );
}

#[test]
fn contact_texture_distinguishes_solid_faces_from_missing_cache_metadata() {
    let mut scene = CollisionScene::default();
    scene.models.insert("floor".into(), model(0));
    scene
        .placements
        .push(placement(77, [1000, 0, 1000], "floor"));
    assert!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_texture(0, 0)
            .is_err()
    );
    scene.models.get_mut("floor").unwrap().textures = Some(vec![Some([42, 3])]);
    let world = StaticCollision::new(&scene).unwrap();
    assert_eq!(world.face_texture(0, 0).unwrap(), Some([42, 3]));
    assert!(world.face_surface(0, 0).is_err());
    assert!(world.face_texture(0, 1).is_err());
    scene.texture_surfaces = Some([(((42 << 7) | 3), 7)].into());
    assert_eq!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_surface(0, 0)
            .unwrap(),
        7
    );
    scene.texture_surfaces = Some(Default::default());
    assert_eq!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_surface(0, 0)
            .unwrap(),
        0
    );
    scene.texture_surfaces = Some([(((42 << 7) | 3), 12)].into());
    assert!(StaticCollision::new(&scene).is_err());
    scene.texture_surfaces = Some(Default::default());
    scene.models.get_mut("floor").unwrap().textures = Some(vec![None]);
    assert_eq!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_texture(0, 0)
            .unwrap(),
        None
    );
    assert_eq!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_surface(0, 0)
            .unwrap(),
        0
    );
    scene.models.get_mut("floor").unwrap().version = Some(*b"v4.0");
    assert_eq!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_surface(0, 0)
            .unwrap(),
        0
    );
    scene.models.get_mut("floor").unwrap().version = Some(*b"v2.7");
    assert!(
        StaticCollision::new(&scene)
            .unwrap()
            .face_surface(0, 0)
            .is_err()
    );
    scene.models.get_mut("floor").unwrap().textures = Some(vec![]);
    assert!(StaticCollision::new(&scene).is_err());
}
