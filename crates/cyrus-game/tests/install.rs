//! Needs an install: skips (and says so) when `redguard`/`$REDGUARD_DIR` is absent.

use cyrus_game::install::GameInstall;
use cyrus_game::loading::parse_one;
use rgpre::import::registry;

fn install() -> Option<GameInstall> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    match GameInstall::open(&root) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("skipped: no install ({e})");
            None
        }
    }
}

#[test]
fn rejects_a_directory_that_is_not_an_install() {
    let err = GameInstall::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_err()
        .to_string();
    assert!(err.contains("missing"), "{err}");
}

#[test]
fn every_scene_file_in_maps_parses() {
    let Some(install) = install() else { return };
    let registry = registry::scan_dir(install.maps()).unwrap();
    assert!(!registry.files.is_empty());
    let failed: Vec<_> = registry
        .files
        .values()
        .filter_map(|e| {
            parse_one(&e.path, e.file_type)
                .err()
                .map(|err| format!("{}: {err}", e.name))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn world_exports_include_declared_terrain_and_textures() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("world-export-test-{}", std::process::id()));
    for (name, terrain) in [("START", true), ("PALACE", false)] {
        let path = output.join(format!("{name}.glb"));
        cyrus_game::scene::export_scene(&install, name, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let nodes = document["nodes"].as_array().unwrap();
        assert_eq!(nodes.iter().any(|node| node["name"] == "Terrain"), terrain);
        assert!(!document["images"].as_array().unwrap().is_empty());
        assert!(nodes.iter().any(|node| node.get("mesh").is_some()));
        let environment: cyrus_game::environment::Environment =
            serde_json::from_slice(&std::fs::read(path.with_extension("environment")).unwrap())
                .unwrap();
        assert_eq!(environment.sky.is_some(), terrain);
        assert_eq!(environment.water.is_some(), terrain);
        std::fs::remove_file(path.with_extension("environment")).unwrap();
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(output).unwrap();
}

#[test]
fn glide_sky_exports_use_their_own_palettes_and_plane_inputs() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("sky-export-test-{}", std::process::id()));
    let mut skies = Vec::new();
    // Verified installed WORLD.INI and matching sky COL samples; no game bytes are fixtures.
    for (world, first_pixel, scale) in [
        ("1", [107, 181, 231, 255], 2000.0),
        ("27", [0, 8, 41, 255], 2000.0),
        ("28", [107, 181, 231, 255], 2000.0),
        ("6", [49, 33, 41, 255], 1500.0),
    ] {
        let path = output.join(format!("{world}.glb"));
        cyrus_game::scene::export_scene(&install, world, &path).unwrap();
        let environment: cyrus_game::environment::Environment =
            serde_json::from_slice(&std::fs::read(path.with_extension("environment")).unwrap())
                .unwrap();
        let sky = environment.sky.unwrap();
        assert_eq!((sky.width, sky.height), (256, 256), "world {world}");
        assert_eq!(sky.level, -4500.0, "world {world}");
        assert_eq!(sky.scale, scale, "world {world}");
        assert_eq!(&sky.rgba[..4], &first_pixel, "world {world}");
        assert!(
            sky.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        skies.push(sky.rgba);
        std::fs::remove_file(path.with_extension("environment")).unwrap();
        if path.with_extension("animations").exists() {
            std::fs::remove_file(path.with_extension("animations")).unwrap();
        }
        std::fs::remove_file(path).unwrap();
    }
    assert_ne!(
        skies[0], skies[1],
        "day and night must use different sky assets"
    );
    assert_ne!(
        skies[0], skies[2],
        "day and sunset must use different sky assets"
    );
    std::fs::remove_dir(output).unwrap();
}
