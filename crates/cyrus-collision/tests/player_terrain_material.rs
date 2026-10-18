//! Terrain material coordinates from preservation engine/player.md.
use cyrus_collision::ground::TerrainGround;

#[test]
fn terrain_material_lookup_preserves_row_orientation_rotation_and_ties() {
    let mut textures = vec![0; 256 * 256];
    textures[97 * 256 + 164] = 89;
    textures[254 * 256] = 31;
    textures[254 * 256 + 2] = 52;
    let terrain =
        TerrainGround::new(vec![0; 256 * 256], vec![false; 256 * 256], Some(textures)).unwrap();
    assert_eq!(
        terrain.texture([42008 * 256, 0, 40607 * 256]).unwrap(),
        Some(89)
    );
    assert_eq!(
        terrain
            .texture([42008 * 256 + 255, 0, 40607 * 256 + 255])
            .unwrap(),
        Some(89)
    );
    assert_eq!(
        terrain.texture([256 * 256, 0, 256 * 256]).unwrap(),
        Some(31)
    );
    assert_eq!(
        terrain.texture([512 * 256, 0, 512 * 256]).unwrap(),
        Some(52)
    );
    for position in [
        [-256, 0, 256 * 256],
        [0, 0, 0],
        [65535 * 256, 0, 256 * 256],
        [0, 0, 65535 * 256],
    ] {
        assert_eq!(terrain.texture(position).unwrap(), None);
    }
    assert!(TerrainGround::new(vec![0; 256 * 256], vec![false; 256 * 256], Some(vec![0])).is_err());
    let old_cache = TerrainGround::new(vec![0; 256 * 256], vec![false; 256 * 256], None).unwrap();
    assert!(
        old_cache
            .texture([256 * 256, 0, 256 * 256])
            .unwrap_err()
            .to_string()
            .contains("re-export")
    );
}

#[test]
fn shoreline_materials_keep_partial_and_full_water_distinct() {
    use cyrus_collision::ground::TerrainMaterial;
    use std::collections::BTreeMap;
    let sample = |image: u8, rotation: u8, u: i64, v: i64, base| {
        let terrain = TerrainGround::new(
            vec![0; 256 * 256],
            vec![false; 256 * 256],
            Some(vec![image | (rotation << 6); 256 * 256]),
        )
        .unwrap();
        terrain
            .material(
                [(512 + 2 * u) * 256, 0, (512 + 2 * (127 - v)) * 256],
                &BTreeMap::from([((302 << 7) | u16::from(image), base)]),
            )
            .unwrap()
            .unwrap()
    };
    for image in [6, 52] {
        for (a, coverage, surface) in [(28, 0, 6), (29, 3, 3), (99, 252, 3), (100, 256, 4)] {
            assert_eq!(
                sample(image, 0, a, 0, 0),
                TerrainMaterial { surface, coverage }
            );
        }
    }
    for (rotation, coverage, surface) in [(0, 0, 6), (1, 209, 3), (2, 256, 4), (3, 42, 3)] {
        assert_eq!(
            sample(6, rotation, 10, 40, 0),
            TerrainMaterial { surface, coverage }
        );
    }
    for (image, a, b, base, coverage, surface) in [
        (0, 0, 0, 0, 256, 4),
        (5, 0, 0, 1, 0, 6),
        (5, 0, 0, 10, 0, 10),
        (5, 127, 127, 1, 255, 3),
        (7, 0, 0, 2, 0, 6),
        (7, 127, 127, 2, 128, 3),
        (30, 0, 0, 0, 4, 3),
        (30, 127, 127, 0, 255, 3),
        (31, 4, 0, 0, 0, 10),
        (31, 5, 0, 0, 256, 4),
        (32, 63, 63, 0, 0, 10),
        (32, 64, 63, 0, 256, 4),
        (25, 10, 40, 6, 0, 6),
        (25, 10, 40, 2, 0, 2),
    ] {
        assert_eq!(
            sample(image, 0, a, b, base),
            TerrainMaterial { surface, coverage }
        );
    }
}

#[test]
fn edge_probe_blocks_only_uncovered_rock_on_a_class_two_slope() {
    use std::collections::BTreeMap;
    // Image 31 is uncovered rock (surface 10) near the cell edge and fully covered beyond it.
    let slope: Vec<i16> = (0..256 * 256)
        .map(|index| ((index % 256) * 512).min(32767) as i16)
        .collect();
    let textures = vec![31u8; 256 * 256];
    let terrain =
        TerrainGround::new(slope, vec![false; 256 * 256], Some(textures.clone())).unwrap();
    let flat =
        TerrainGround::new(vec![0; 256 * 256], vec![false; 256 * 256], Some(textures)).unwrap();
    let assignments = BTreeMap::new();
    let near_edge = [258 << 8, 0, 1000 << 8];
    let covered = [300 << 8, 0, 1000 << 8];
    assert_eq!(
        terrain.sample(near_edge).unwrap().classification(),
        2,
        "the fixture slope must be class two"
    );
    assert!(terrain.edge_blocked(near_edge, &assignments).unwrap());
    assert!(!terrain.edge_blocked(covered, &assignments).unwrap());
    assert!(!flat.edge_blocked(near_edge, &assignments).unwrap());
    let no_textures = TerrainGround::new(vec![0; 256 * 256], vec![false; 256 * 256], None).unwrap();
    assert!(no_textures.edge_blocked(near_edge, &assignments).is_err());
}
