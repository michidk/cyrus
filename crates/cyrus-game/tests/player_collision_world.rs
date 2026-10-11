use cyrus_game::player::{
    collision_data::{CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement},
    collision_world::StaticCollision,
    ground::GroundProbe,
};

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
fn forward_slope_selects_nearest_midpoint_and_keeps_first_equal_model() {
    use cyrus_game::player::ground::{GroundSupport, ModelSupport};
    let probe = GroundProbe::new([0; 3], 0, 50).unwrap();
    let steep = GroundSupport {
        height: -10.0,
        normal: [230, -100, 0],
    };
    let flat = GroundSupport {
        height: 10.0,
        normal: [0, -256, 0],
    };
    let contacts = [
        ModelSupport {
            face: 3,
            model: 0,
            rank: 0,
            support: flat,
        },
        ModelSupport {
            face: 3,
            model: 1,
            rank: 16000,
            support: steep,
        },
        ModelSupport {
            face: 3,
            model: 2,
            rank: 16768,
            support: flat,
        },
    ];
    assert_eq!(probe.slope_surface(None, &contacts), Some(steep));
    assert_eq!(probe.slope_surface(Some(flat), &contacts), Some(steep));
    let closer = GroundSupport {
        height: 5.0,
        ..flat
    };
    assert_eq!(probe.slope_surface(Some(closer), &contacts), Some(closer));
    for height in [-9.75, 9.75] {
        let fractional = GroundSupport { height, ..flat };
        assert_eq!(
            probe.slope_surface(Some(fractional), &contacts),
            Some(fractional)
        );
    }
    let world = StaticCollision::new(&CollisionScene::default()).unwrap();
    let candidates = world.candidates([5000, 5000], [0; 3], None).unwrap();
    assert!(
        world
            .forward_support(&candidates, &probe, Some(steep), true, None, false)
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .forward_support(&candidates, &probe, Some(steep), false, None, false)
            .unwrap()
            .is_some()
    );
    assert!(
        world
            .forward_support(&candidates, &probe, None, true, None, false)
            .is_err()
    );
    let selected = world
        .forward_support(&candidates, &probe, Some(steep), false, None, false)
        .unwrap()
        .unwrap();
    assert!(!selected.allows_forward(2, 0));
    assert!(selected.allows_forward(0, 0));
    assert!(!selected.allows_forward(0, 1));
    assert!(selected.allows_forward(0, 2));
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

#[test]
fn fallback_keeps_first_non_side_contact_and_fractional_actor_height() {
    use cyrus_game::player::{
        collision_world::StaticCandidates,
        ground::{FallbackProbe, SupportSource},
    };
    let mut scene = CollisionScene::default();
    let mut wall = model(0);
    wall.vertices = vec![[-50.0, -50.0, 0.0], [50.0, -50.0, 0.0], [0.0, 50.0, 0.0]];
    wall.normals = vec![[0.0, 0.0, 1.0]];
    scene.models.insert("wall".into(), wall);
    scene.models.insert("floor".into(), model(0));
    scene.placements = vec![
        placement(0, [1000, 0, 1000], "wall"),
        placement(1, [1000, -8, 1000], "floor"),
        placement(2, [1000, -4, 1000], "floor"),
    ];
    let world = StaticCollision::new(&scene).unwrap();
    let candidates = StaticCandidates {
        faces: vec![0, 1, 2],
        actors: vec![],
    };
    let position = [1000 * 256, 127, 1000 * 256];
    let primary = GroundProbe::new(position, 0, 1).unwrap();
    assert!(
        world
            .ground_contacts(&candidates, &primary, false)
            .unwrap()
            .is_empty()
    );
    let fallback = FallbackProbe::new(position, [0; 3], 0, false).unwrap();
    let support = world
        .forward_support(&candidates, &primary, None, false, Some(&fallback), false)
        .unwrap()
        .unwrap();
    assert_eq!(support.source(), SupportSource::Model { model: 1, face: 0 });
    assert_eq!(support.actor_height(62, -2), 127);
    assert_eq!(support.classification(2), 4);
}

#[test]
fn fallback_history_expands_radius_and_accepts_undersides() {
    use cyrus_game::player::{collision_world::StaticCandidates, ground::FallbackProbe};
    let mut scene = CollisionScene::default();
    let mut underside = model(0);
    underside.normals = vec![[0.0, 1.0, 0.0]];
    underside.faces[0].reverse();
    scene.models.insert("ceiling".into(), underside);
    scene
        .placements
        .push(placement(0, [1000, 9, 1000], "ceiling"));
    let world = StaticCollision::new(&scene).unwrap();
    let candidates = StaticCandidates {
        faces: vec![0],
        actors: vec![],
    };
    let position = [1000 * 256, 127, 1000 * 256];
    let small = FallbackProbe::new(position, [0; 3], 0, false).unwrap();
    assert!(
        world
            .fallback_support(&candidates, &small, false)
            .unwrap()
            .is_none()
    );
    let large = FallbackProbe::new(position, [0; 3], 0, true).unwrap();
    let support = world
        .fallback_support(&candidates, &large, false)
        .unwrap()
        .unwrap();
    assert_eq!(support.classification(2), 8);
    assert_eq!(support.actor_height(0, 0), 127);
    assert!(FallbackProbe::new([i64::MAX; 3], [0; 3], 0, false).is_err());
}
