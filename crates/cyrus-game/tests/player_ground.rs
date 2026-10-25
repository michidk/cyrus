use cyrus_collision::ground::TerrainGround;

#[test]
fn original_terrain_probe_preserves_integer_query_and_actor_origin() {
    let mut heights = vec![-680; 256 * 256];
    heights[(256 - 161) * 256 + 160] = -720;
    let terrain = TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap();
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
fn water_cells_support_with_live_heights_near_the_camera_and_outside_grid_does_not() {
    let mut water = vec![false; 256 * 256];
    water[(256 - 161) * 256 + 159] = true;
    let terrain = TerrainGround::new(vec![-680; 256 * 256], water, None).unwrap();
    let water_position = [10_448_896, 0, 10_502_144];
    // A water cell supplies the same interpolated height as a dry one; settling still refuses it.
    assert_eq!(terrain.sample(water_position).unwrap().height, -680.0);
    assert!(terrain.sample_dry(water_position).is_none());
    // Within 15 cells of the camera the live vertex cache replaces the decoded heights.
    let mut live = vec![-680.0f32; 256 * 256];
    live[(256 - 161) * 256 + 159] = -650.0;
    live[(256 - 161) * 256 + 160] = -650.0;
    live[(256 - 160) * 256 + 159] = -650.0;
    live[(256 - 160) * 256 + 160] = -650.0;
    terrain.live().publish([160, 160], &live);
    assert_eq!(terrain.sample(water_position).unwrap().height, -650.0);
    terrain.live().publish([160 + 16, 160], &live);
    assert_eq!(terrain.sample(water_position).unwrap().height, -680.0);
    assert!(terrain.sample([-256, 0, 0]).is_none());
    assert!(terrain.sample([65_536 * 256, 0, 0]).is_none());
    assert!(TerrainGround::new(vec![], vec![], None).is_err());
}

#[test]
fn original_walking_route_uses_single_precision_heading_projection() {
    use cyrus_game::player::motion::translate;
    use cyrus_game::player::walk::WalkingStartup;
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
        let distance = startup.advance(8, 0, 170, permits_translation);
        if permits_translation {
            position = translate(position, 2036, distance as i64);
        }
        startup.finish_update(true);
        assert_eq!(position, expected);
    }
}

#[test]
fn original_wall_contacts_roll_back_or_project_and_separate() {
    use cyrus_game::player::contact::WallResponse;
    use cyrus_game::player::contact::resolve_wall;
    use cyrus_game::player::contact::separate_residual;
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
    use cyrus_collision::geometry::CollisionFace;
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
    use cyrus_game::player::definition::BodySphere;
    use cyrus_game::player::definition::PlayerDefinition;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (player body check)");
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
    assert!(
        scene
            .unavailable
            .as_ref()
            .unwrap()
            .contains("world 1 player: CYRUS")
    );
    assert!(scene.model_radii.is_empty());
    let missing = scene.candidates([0, 0], [0; 3], Some("CYRSA001"));
    assert!(missing.unwrap_err().to_string().contains("radius absent"));
    let mut invalid_radius: serde_json::Value =
        serde_json::from_slice(&bytes(2, serde_json::Value::Null, serde_json::Value::Null))
            .unwrap();
    invalid_radius["model_radii"] = serde_json::json!({"CYRSA001": -1.0});
    let error = PlayerScene::decode(&serde_json::to_vec(&invalid_radius).unwrap())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("world 1 player model CYRSA001: invalid radius"));
    let mut stray_textures: serde_json::Value =
        serde_json::from_slice(&bytes(2, serde_json::Value::Null, serde_json::Value::Null))
            .unwrap();
    stray_textures["terrain_textures"] = serde_json::json!(vec![0; 256 * 256]);
    assert!(PlayerScene::decode(&serde_json::to_vec(&stray_textures).unwrap()).is_err());
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
    use cyrus_game::player::contact::wall_heading;
    use cyrus_game::player::motion::displacement_heading;
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
    use cyrus_collision::geometry::CollisionFace;
    use cyrus_collision::geometry::SphereCandidates;
    use cyrus_collision::geometry::ordered_contacts;
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

#[test]
fn ordered_grounded_contacts_preserve_displacement_and_filter_facing() {
    use cyrus_game::player::contact::{GroundedContact, resolve_grounded_contacts};
    let previous = [10447534, -190464, 10771472];
    let attempted = [10444193, -190464, 10773365];
    let mut heading = 344;
    assert_eq!(
        resolve_grounded_contacts(
            previous,
            attempted,
            &mut heading,
            &[GroundedContact {
                normal: [256, 0, 0],
                classification: 1
            }],
            -60000
        ),
        [10447534, -190464, 10773491]
    );
    assert_eq!(heading, 313);
    for (normal, classification) in [([256, 0, 0], 4), ([-256, 0, 0], 1)] {
        let mut heading = 344;
        assert_eq!(
            resolve_grounded_contacts(
                previous,
                attempted,
                &mut heading,
                &[GroundedContact {
                    normal,
                    classification
                }],
                -60000
            ),
            attempted
        );
        assert_eq!(heading, 344);
    }
    // The original 2047 wrap admits one geometric quarter-turn boundary.
    for (normal, distance) in [([256, 0, 0], 256), ([-256, 0, 0], 4096)] {
        let mut heading = 0;
        assert_eq!(
            resolve_grounded_contacts(
                [0; 3],
                [0, 0, 256],
                &mut heading,
                &[GroundedContact {
                    normal,
                    classification: 1
                }],
                -60000
            ),
            [0, 0, distance]
        );
    }
    // A second contact uses the original heading and displacement, not the first projection.
    let mut heading = 344;
    assert_eq!(
        resolve_grounded_contacts(
            previous,
            attempted,
            &mut heading,
            &[
                GroundedContact {
                    normal: [256, 0, 0],
                    classification: 1
                },
                GroundedContact {
                    normal: [0, 0, -256],
                    classification: 1
                }
            ],
            -60000
        ),
        [10443970, -190464, 10771472]
    );
}

#[test]
fn upright_orientation_retains_original_half_turn_residual_and_heading_sign() {
    use bevy::math::Vec3;
    use cyrus_game::player::motion::upright_rotation;
    let half = upright_rotation(1024);
    assert_eq!(
        half.x_axis,
        Vec3::new(-1.0, 0.0, -f32::from_bits(0x3366_2a9a))
    );
    assert_eq!(half.y_axis, Vec3::Y);
    assert_eq!(
        half.z_axis,
        Vec3::new(f32::from_bits(0x3366_2a9a), 0.0, -1.0)
    );
    assert_eq!(upright_rotation(0), upright_rotation(2048));
    assert!(upright_rotation(2036).z_axis.x > 0.0);
    assert!(upright_rotation(12).z_axis.x < 0.0);
}

#[test]
fn incremental_upright_refresh_matches_selected_player_without_rebuilding() {
    use bevy::math::{Mat3, Vec3};
    use cyrus_game::player::motion::refresh_upright_rotation;
    let mut matrix = Mat3::from_diagonal(Vec3::new(-1.0, 1.0, -1.0));
    refresh_upright_rotation(&mut matrix, 0, 12);
    assert_eq!(matrix.x_axis, Vec3::new(-0.999_322_35, 0.0, -0.036_807_33));
    assert_eq!(matrix.y_axis, Vec3::Y);
    assert_eq!(matrix.z_axis, Vec3::new(0.036_807_33, 0.0, -0.999_322_35));
    let refreshed = matrix;
    refresh_upright_rotation(&mut matrix, 12, 12);
    assert_eq!(matrix, refreshed);
}

#[test]
fn columns_past_the_east_edge_read_the_next_array_row() {
    // The original's terrain probe has no bounds test: column 256 of one row is column 0 of the next
    // index row. Observed in NECRISLE: swimming east past the grid climbs onto the west-edge heights.
    let mut heights = vec![0; 256 * 256];
    for row in 246..=247 {
        heights[(256 - row) * 256] = -1000;
    }
    let terrain = TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap();
    let at = |x: i64, z: i64| terrain.height([x << 8, 0, z << 8]);
    assert_eq!(at(65_054, 63_410), Some(0.0));
    let edge = at(65_336, 63_465).unwrap();
    assert!((-300.0..-100.0).contains(&edge), "edge height {edge}");
    // Further east the interpolation climbs back to column 1, which is level ground.
    assert!(at(65_779, 63_552).unwrap() > edge);
    // Rows outside the supported range and corners outside the array have no support.
    assert_eq!(at(65_336, 100), None);
    assert_eq!(at(65_336, 100_000), None);
}
