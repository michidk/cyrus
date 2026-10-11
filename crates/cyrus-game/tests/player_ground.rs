use cyrus_game::player::ground::TerrainGround;

#[test]
fn original_terrain_probe_preserves_integer_query_and_actor_origin() {
    let mut heights = vec![-680; 256 * 256];
    heights[(256 - 161) * 256 + 160] = -720;
    let terrain = TerrainGround::new(heights, vec![false; 256 * 256]).unwrap();
    let spawn = terrain.sample([10_448_896, -178_944, 10_502_144]).unwrap();
    assert_eq!(spawn.height, -680.0);
    assert_eq!(spawn.normal, [0, -256, 0]);
    assert_eq!(spawn.classification(), 4);
    assert_eq!(spawn.actor_height(62, -2), -190_464);

    let probe = terrain.sample([10_448_896, -190_464, 10_526_387]).unwrap();
    assert!((probe.height - -682.1875).abs() < 1e-9);
    assert_eq!(probe.normal, [-39, -250, -39]);
    assert_eq!(probe.actor_height(62, -2), -190_976);
    assert_eq!(
        probe,
        terrain.sample([10_448_896, 0, 41_118 * 256]).unwrap()
    );
}

#[test]
fn unsupported_water_and_outside_grid_do_not_fabricate_support() {
    let mut water = vec![false; 256 * 256];
    water[(256 - 161) * 256 + 159] = true;
    let terrain = TerrainGround::new(vec![-680; 256 * 256], water).unwrap();
    assert!(terrain.sample([10_448_896, 0, 10_502_144]).is_none());
    assert!(terrain.sample([-256, 0, 0]).is_none());
    assert!(terrain.sample([65_536 * 256, 0, 0]).is_none());
    assert!(TerrainGround::new(vec![], vec![]).is_err());
}

#[test]
fn original_walking_route_uses_single_precision_heading_projection() {
    use cyrus_game::player::{motion::translate, walk::WalkingStartup};
    let mut startup = WalkingStartup::new(1).unwrap();
    let mut position = [10_446_802, -190_464, 10_818_615];
    for expected in [
        [10_446_802, -190_464, 10_818_615],
        [10_446_802, -190_464, 10_818_615],
        [10_446_827, -190_464, 10_819_295],
        [10_446_877, -190_464, 10_820_654],
        [10_446_927, -190_464, 10_822_013],
        [10_446_977, -190_464, 10_823_372],
        [10_447_027, -190_464, 10_824_731],
        [10_447_077, -190_464, 10_826_090],
        [10_447_127, -190_464, 10_827_449],
    ] {
        let permits_translation = expected != [10_446_802, -190_464, 10_818_615];
        let distance = startup.advance(8, 170, permits_translation);
        if permits_translation {
            position = translate(position, 2036, distance as i64);
        }
        startup.finish_update(true);
        assert_eq!(position, expected);
    }
}

#[test]
fn original_wall_contacts_roll_back_or_project_and_separate() {
    use cyrus_game::player::contact::{WallResponse, resolve_wall, separate_residual};
    assert_eq!(
        resolve_wall(
            [10_449_205, -190_464, 10_770_526],
            [10_444_931, -190_464, 10_771_486],
            [10_444_931, -190_464, 10_771_486],
            [256, 0, 0],
            -60_000,
        ),
        WallResponse::Rollback([10_449_205, -190_464, 10_770_526])
    );
    assert_eq!(
        resolve_wall(
            [10_447_534, -190_464, 10_771_472],
            [10_444_193, -190_464, 10_773_365],
            [10_444_193, -190_464, 10_773_365],
            [256, 0, 0],
            -60_000,
        ),
        WallResponse::Projected([10_447_534, -190_464, 10_773_491])
    );
    let mut position = [10_444_931, -190_464, 10_771_486];
    assert!(separate_residual(0, &mut position, &[[256, 0, 0]], 16));
    assert_eq!(position, [10_449_027, -190_464, 10_771_486]);
    let unchanged = position;
    assert!(!separate_residual(0, &mut position, &[], 16));
    assert_eq!(position, unchanged);
}

#[test]
fn sphere_polygon_contact_keeps_plane_projection_and_boundary_equality() {
    use bevy::math::Vec3;
    use cyrus_game::player::geometry::CollisionFace;
    let face = CollisionFace::new(
        vec![
            Vec3::ZERO,
            Vec3::X * 100.0,
            Vec3::new(100.0, 100.0, 0.0),
            Vec3::Y * 100.0,
        ],
        Vec3::Z,
    )
    .unwrap();
    assert_eq!(
        face.sphere_contact(Vec3::new(50.0, 50.0, 24.0), 24.0, true),
        Some(Vec3::new(50.0, 50.0, 0.0))
    );
    assert_eq!(
        face.sphere_contact(Vec3::new(50.0, 50.0, 24.0), 23.0, true),
        None
    );
    assert_eq!(
        face.sphere_contact(Vec3::new(50.0, 50.0, -5.0), 24.0, true),
        None
    );
    assert!(
        face.sphere_contact(Vec3::new(50.0, 50.0, -5.0), 24.0, false)
            .is_some()
    );
    // An edge can intersect although the projected point is outside the polygon.
    assert_eq!(
        face.sphere_contact(Vec3::new(-3.0, 50.0, 4.0), 5.0, true),
        Some(Vec3::new(-3.0, 50.0, 0.0))
    );
    assert_eq!(
        face.sphere_contact(Vec3::new(-4.0, 50.0, 4.0), 5.0, true),
        None
    );
    assert!(CollisionFace::new(vec![Vec3::ZERO; 2], Vec3::Z).is_err());
}

#[test]
fn installed_player_attributes_build_original_upright_spheres() {
    use cyrus_game::player::definition::{BodySphere, PlayerDefinition};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let Ok(install) = cyrus_game::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipping player body check: no Redguard install");
        return;
    };
    let path = std::fs::read_dir(install.maps())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("ISLAND.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let names =
        rgpre::import::soup_def::try_load_soup_def(&install.root).expect("installed SOUP386.DEF");
    let metadata = rgpre::import::rgm::export_rgm_metadata_json(&map, Some(&names));
    let player = PlayerDefinition::from_metadata(&metadata).unwrap();
    assert_eq!(player.forward_walk, 8);
    assert_eq!(player.step_type, 1);
    assert_eq!(player.positive_extent, [24, 62, 11]);
    assert_eq!(player.negative_extent, [24, 66, 38]);
    assert_eq!(
        player.body_spheres(50).unwrap(),
        vec![
            BodySphere {
                center: [0, -42, 0],
                radius: 24
            },
            BodySphere {
                center: [0, -12, 0],
                radius: 24
            },
        ]
    );
    let unnamed = rgpre::import::rgm::export_rgm_metadata_json(&map, None);
    assert!(PlayerDefinition::from_metadata(&unnamed).is_err());
}

#[test]
fn player_cache_rejects_partial_terrain_and_retains_unavailable_definition() {
    use cyrus_game::player::data::PlayerScene;
    let bytes = |version, heights: serde_json::Value, water: serde_json::Value| {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "world": 1,
            "definition": {"actors": []},
            "settings": {"standing_height": -2, "post_collide_height": 50, "slide_range": -60000, "turn_speed": 12, "turn_max_speed": 48, "poly_push_units": 16},
            "heights": heights,
            "water": water,
            "collision": {"placements": [], "models": {}, "unsupported": {}},
        }))
        .unwrap()
    };
    let scene =
        PlayerScene::decode(&bytes(2, serde_json::Value::Null, serde_json::Value::Null)).unwrap();
    assert_eq!(scene.world, 1);
    assert!(scene.terrain.is_none());
    assert!(scene.definition.is_none());
    assert!(scene.unavailable.unwrap().contains("world 1 player: CYRUS"));
    assert!(
        PlayerScene::decode(&bytes(
            999,
            serde_json::Value::Null,
            serde_json::Value::Null
        ))
        .is_err()
    );
    assert!(
        PlayerScene::decode(&bytes(2, serde_json::json!([]), serde_json::Value::Null)).is_err()
    );
    assert!(PlayerScene::decode(&bytes(2, serde_json::json!([]), serde_json::json!([]))).is_err());
    let scene = PlayerScene::decode(&bytes(
        2,
        serde_json::json!(vec![-680; 256 * 256]),
        serde_json::json!(vec![false; 256 * 256]),
    ))
    .unwrap();
    assert_eq!(
        scene
            .terrain
            .unwrap()
            .sample([10_448_896, 0, 10_502_144])
            .unwrap()
            .actor_height(62, -2),
        -190_464
    );
}

#[test]
fn original_wall_slide_adjusts_quantized_movement_heading() {
    use cyrus_game::player::{contact::wall_heading, motion::displacement_heading};
    let movement = displacement_heading(-3341, 1893);
    assert_eq!(movement, 345);
    assert_eq!(wall_heading(344, movement, [256, 0, 0]), 313);
    assert_eq!(wall_heading(313, 314, [256, 0, 0]), 282);
    for (x, z, heading) in [(0, 1, 0), (1, 0, 1536), (0, -1, 1024), (-1, 0, 512)] {
        assert_eq!(displacement_heading(x, z), heading);
    }
}

#[test]
fn collision_traversal_retries_misses_and_keeps_first_success_order() {
    use bevy::math::Vec3;
    use cyrus_game::player::geometry::{CollisionFace, SphereCandidates, ordered_contacts};
    let square = |z| {
        CollisionFace::new(
            vec![
                Vec3::new(0.0, 0.0, z),
                Vec3::new(100.0, 0.0, z),
                Vec3::new(100.0, 100.0, z),
                Vec3::new(0.0, 100.0, z),
            ],
            Vec3::Z,
        )
        .unwrap()
    };
    let faces = [square(0.0), square(30.0), square(30.0)];
    let spheres = [
        SphereCandidates {
            center: Vec3::new(50.0, 50.0, 32.0),
            radius: 4.0,
            faces: &[0, 1, 1],
        },
        SphereCandidates {
            center: Vec3::new(50.0, 50.0, 2.0),
            radius: 4.0,
            faces: &[0, 1, 0],
        },
    ];
    let contacts = ordered_contacts(&faces, &spheres, true).unwrap();
    assert_eq!(
        contacts
            .iter()
            .map(|contact| contact.face)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(contacts[0].projection, Vec3::new(50.0, 50.0, 30.0));
    assert_eq!(contacts[1].projection, Vec3::new(50.0, 50.0, 0.0));
    assert!(
        ordered_contacts(
            &faces,
            &[SphereCandidates {
                center: Vec3::ZERO,
                radius: 1.0,
                faces: &[3],
            }],
            true
        )
        .is_err()
    );
}
