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
