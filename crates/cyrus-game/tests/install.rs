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
