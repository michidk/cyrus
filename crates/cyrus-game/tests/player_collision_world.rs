use cyrus_assets::collision::CollisionModel;
use cyrus_assets::collision::CollisionScene;
use cyrus_assets::collision::CollisionSubobject;
use cyrus_assets::collision::StaticPlacement;
use cyrus_world::collision::collision_world::StaticCollision;
use cyrus_world::collision::ground::GroundProbe;

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
fn startup_settling_keeps_first_equal_contact_and_uses_full_search_radius() {
    use cyrus_world::collision::ground::SupportSource;
    let world = StaticCollision::new(&CollisionScene {
        models: [("GROUND".into(), model(0))].into(),
        placements: vec![
            placement(1, [5000, 400, 5000], "GROUND"),
            placement(2, [5000, 400, 5000], "GROUND"),
        ],
        ..Default::default()
    })
    .unwrap();
    let position = [5000 * 256, -62 * 256, 5000 * 256];
    assert!(
        world
            .candidates([5000, 5000], position, None)
            .unwrap()
            .faces
            .is_empty()
    );
    let support = world.startup_support(position, 62, None).unwrap().unwrap();
    assert_eq!(support.source(), SupportSource::Model { model: 1, face: 0 });
    assert_eq!(support.actor_height(62, -2), 86_016);
}

#[test]
fn startup_settling_accepts_static_support_above_unsupported_water() {
    use cyrus_game::player::data::PlayerScene;
    use cyrus_game::player::placement::{PlayerPose, settle_static};

    let collision = CollisionScene {
        models: [("DECK".into(), model(0))].into(),
        placements: vec![placement(1, [5000, 400, 5000], "DECK")],
        ..Default::default()
    };
    let mut scene = PlayerScene::decode(
        &serde_json::to_vec(&serde_json::json!({
            "version":2,"world":0,
            "settings":{"standing_height":-2,"post_collide_height":50,"slide_range":-60000,
                "turn_speed":12,"turn_max_speed":48,"poly_push_units":16},
            "definition":{
                "actors":[{"script_name":"CYRUS","attributes":{},"mpsz_bounds_primary":0,
                    "raex":{"unknown_12":256}}],
                "mpsz_entries":[{"total_extent":[48,128,49],"pos_extent":[24,62,11],
                    "neg_extent":[24,66,38],"center_offset":[0,0,0],"flags":0}]
            },
            "heights":vec![0;256*256],"water":vec![true;256*256],
            "collision":collision
        }))
        .unwrap(),
    )
    .unwrap();
    let mut pose = PlayerPose {
        position: [5000 * 256, -62 * 256, 5000 * 256],
        heading: 0,
    };
    settle_static(&scene, &mut pose).unwrap();
    assert_eq!(pose.position, [1_280_000, 86_016, 1_280_000]);

    scene.static_collision = StaticCollision::new(&CollisionScene::default()).unwrap();
    assert!(settle_static(&scene, &mut pose).is_err());
}

#[test]
fn startup_settling_truncates_terrain_before_strict_endpoint_admission() {
    use cyrus_world::collision::ground::GroundSupport;
    let world = StaticCollision::new(&CollisionScene::default()).unwrap();
    let position = [5000 * 256, -62 * 256, 5000 * 256];
    for height in [-500.9, -500.0, 500.0, 500.9] {
        assert!(
            world
                .startup_support(
                    position,
                    62,
                    Some(GroundSupport {
                        height,
                        normal: [0, -256, 0],
                    })
                )
                .unwrap()
                .is_none()
        );
    }
    for (height, expected) in [(-499.9, -144_128), (499.9, 111_360)] {
        let support = world
            .startup_support(
                position,
                62,
                Some(GroundSupport {
                    height,
                    normal: [0, -256, 0],
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(support.actor_height(62, -2), expected);
    }
}

#[test]
fn startup_settling_retains_distinct_terrain_and_model_rank_scales() {
    use cyrus_world::collision::ground::{GroundSupport, SupportSource};
    let world = StaticCollision::new(&CollisionScene {
        models: [("GROUND".into(), model(0))].into(),
        placements: vec![placement(1, [5000, 100, 5000], "GROUND")],
        ..Default::default()
    })
    .unwrap();
    let support = world
        .startup_support(
            [5000 * 256, -62 * 256, 5000 * 256],
            62,
            Some(GroundSupport {
                height: -100.0,
                normal: [0, -256, 0],
            }),
        )
        .unwrap()
        .unwrap();
    assert_eq!(support.source(), SupportSource::Model { model: 0, face: 0 });
}

#[test]
fn forward_slope_selects_nearest_midpoint_and_keeps_first_equal_model() {
    use cyrus_world::collision::ground::GroundSupport;
    use cyrus_world::collision::ground::ModelSupport;
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
    assert!(!cyrus_game::player::support::allows_forward(
        &selected, 2, 0
    ));
    assert!(cyrus_game::player::support::allows_forward(&selected, 0, 0));
    assert!(!cyrus_game::player::support::allows_forward(
        &selected, 0, 1
    ));
    assert!(cyrus_game::player::support::allows_forward(&selected, 0, 2));
}

#[test]
fn fallback_keeps_first_non_side_contact_and_fractional_actor_height() {
    use cyrus_world::collision::collision_world::StaticCandidates;
    use cyrus_world::collision::ground::FallbackProbe;
    use cyrus_world::collision::ground::SupportSource;
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
    assert_eq!(cyrus_game::player::support::classification(&support, 2), 4);
}

#[test]
fn fallback_history_expands_radius_and_accepts_undersides() {
    use cyrus_world::collision::collision_world::StaticCandidates;
    use cyrus_world::collision::ground::FallbackProbe;
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
    assert_eq!(cyrus_game::player::support::classification(&support, 2), 8);
    assert_eq!(support.actor_height(0, 0), 127);
    assert!(FallbackProbe::new([i64::MAX; 3], [0; 3], 0, false).is_err());
}
