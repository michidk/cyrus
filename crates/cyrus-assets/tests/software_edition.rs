//! Software-only DOS install (`3dart`, `TEXTURE.###`, no `fxart`/`RGFX.EXE`): `REDGUARD_SOFTWARE_DIR`
//! names that folder. Skips, and says so, when it is absent.
#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
use cyrus_assets::install::{Edition, GameInstall};

fn install() -> Option<GameInstall> {
    let Some(root) = std::env::var_os("REDGUARD_SOFTWARE_DIR") else {
        eprintln!("skipped: no software install (REDGUARD_SOFTWARE_DIR)");
        return None;
    };
    Some(GameInstall::open(root.as_ref()).unwrap())
}

#[test]
fn software_install_is_admitted_with_its_edition() {
    let Some(install) = install() else { return };
    assert_eq!(install.edition(), Edition::Software);
}

#[test]
fn software_install_lists_its_world_art() {
    let Some(install) = install() else { return };
    let source = cyrus_assets::native::Files::open(&install).unwrap();
    use cyrus_assets::source::ReadAssets;
    let manifest = source.manifest();
    assert!(manifest.find("3dart/TEXTURE.001").is_some());
    assert!(manifest.find("WORLD.INI").is_some());
}

#[test]
fn software_install_loads_the_start_world() {
    let Some(install) = install() else { return };
    let source = cyrus_assets::native::Files::open(&install).unwrap();
    cyrus_assets::runtime::geometry::world(&source, "START").unwrap();
}

#[test]
fn software_world_carries_its_textures() {
    let Some(install) = install() else { return };
    let source = cyrus_assets::native::Files::open(&install).unwrap();
    let world = cyrus_assets::runtime::geometry::world(&source, "START").unwrap();
    let glb = gltf_image_count(&world.glb);
    assert!(glb > 0, "START exported {glb} textures");
}

fn gltf_image_count(glb: &[u8]) -> usize {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
    json["images"].as_array().map_or(0, Vec::len)
}
