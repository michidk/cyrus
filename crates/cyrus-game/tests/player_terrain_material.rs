//! Terrain material coordinates from preservation engine/player.md.
use cyrus_game::player::ground::TerrainGround;

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
