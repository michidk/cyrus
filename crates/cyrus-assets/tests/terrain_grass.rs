#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Installed terrain textures classified as grass or not.
use anyhow::Result;
use cyrus_assets::{
    install::GameInstall,
    native::Files,
    runtime::geometry,
    terrain_grass::{grass_share, has_grass},
};
use std::path::PathBuf;

/// The terrain texture bank every WLD terrain draws from.
const TERRAIN_BANK: u16 = 302;

#[test]
fn installed_terrain_tiles_are_classified_by_their_pixels() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (terrain grass)");
        return Ok(());
    };
    let mut world = geometry::world(&Files::open(&install)?, "ISLAND")?;
    let shares: Vec<f32> = (0..64)
        .map(|tile| {
            world
                .textures
                .get_image_rgba(TERRAIN_BANK, tile)
                .map_or(0.0, |(rgba, _, _)| grass_share(&rgba))
        })
        .collect();
    // Observed in the bank's images (TEXBSI.302 with the ISLAND palette): solid grass tiles and
    // grass blended into sand or dirt.
    let grass = [
        2, 10, 11, 12, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 28, 29, 33, 34, 35, 36, 37, 38,
        49, 50, 51,
    ];
    for tile in 0..64u8 {
        assert_eq!(
            has_grass(shares[usize::from(tile)]),
            grass.contains(&tile),
            "tile {tile}: share {}",
            shares[usize::from(tile)]
        );
    }
    Ok(())
}
