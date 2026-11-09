#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Installed terrain textures classified as grass or not.
use anyhow::Result;
use cyrus_assets::{
    install::GameInstall,
    native::Files,
    runtime::{geometry, terrain_grass},
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
    let coverage = terrain_grass::terrain_grass(&mut world.textures, TERRAIN_BANK);
    assert_eq!(coverage.len(), usize::from(terrain_grass::TERRAIN_TILES));
    // Observed in the bank's images (TEXBSI.302 with the ISLAND palette): solid grass tiles.
    for tile in [2, 28, 29, 33, 34] {
        assert!(coverage[tile] > 0.95, "tile {tile}: {}", coverage[tile]);
    }
    // Water, sand, dirt, gravel and stone.
    for tile in [0, 1, 3, 4, 5, 25, 43, 55, 59, 63] {
        assert_eq!(coverage[tile], 0.0, "tile {tile}");
    }
    // Grass blended into sand or dirt.
    for tile in [12, 16, 49] {
        assert!(
            coverage[tile] > 0.2 && coverage[tile] < 0.95,
            "tile {tile}: {}",
            coverage[tile]
        );
    }
    Ok(())
}
