#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Native launch selections resolve to one validated read-only install root.

use cyrus_assets::install::GameInstall;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "cyrus-install-resolution-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn install(&self, relative: &str) -> PathBuf {
        let root = self.0.join(relative);
        std::fs::create_dir_all(root.join("fxart/nested")).unwrap();
        std::fs::create_dir_all(root.join("maps")).unwrap();
        std::fs::write(root.join("RGFX.EXE"), []).unwrap();
        root.canonicalize().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn resolves_library_package_emulator_and_install_descendants() {
    let scratch = Scratch::new();
    let library = scratch.0.join("GOG Galaxy");
    let package = library.join("Redguard");
    let root = scratch.install("GOG Galaxy/Redguard/Redguard");
    let emulator = package.join("DOSBOX");
    std::fs::create_dir(&emulator).unwrap();
    for selected in [
        library,
        package,
        emulator,
        root.clone(),
        root.join("fxart/nested"),
        root.join("maps"),
    ] {
        assert_eq!(
            GameInstall::locate(Some(&selected)).unwrap().root,
            root,
            "{}",
            selected.display()
        );
    }
    assert!(GameInstall::open(&scratch.0.join("GOG Galaxy")).is_err());
}

#[test]
fn resolves_extracted_gog_layout() {
    let scratch = Scratch::new();
    let root = scratch.install("GOG Games/Redguard/app/Redguard");
    for relative in ["GOG Games", "GOG Games/Redguard", "GOG Games/Redguard/app"] {
        assert_eq!(
            GameInstall::locate(Some(&scratch.0.join(relative)))
                .unwrap()
                .root,
            root
        );
    }
}

#[test]
fn enclosing_install_wins_over_nested_install() {
    let scratch = Scratch::new();
    let root = scratch.install("Redguard");
    scratch.install("Redguard/maps/Redguard");
    assert_eq!(GameInstall::resolve(&root.join("maps")).unwrap().root, root);
}

#[test]
fn invalid_selection_reports_install_argument_and_searched_paths() {
    let scratch = Scratch::new();
    std::fs::create_dir(scratch.0.join("fxart")).unwrap();
    let error = GameInstall::locate(Some(&scratch.0))
        .unwrap_err()
        .to_string();
    assert!(error.contains("--install PATH"), "{error}");
    assert!(error.contains("Searched:"), "{error}");
    assert!(error.contains("fxart, maps and RGFX.EXE"), "{error}");
    let error = GameInstall::locate(Some(&scratch.0.join("missing")))
        .unwrap_err()
        .to_string();
    assert!(error.contains("--install PATH"), "{error}");
    assert!(error.contains("missing"), "{error}");
    assert!(GameInstall::open(Path::new(env!("CARGO_MANIFEST_DIR"))).is_err());
}

#[test]
fn resolved_installed_subdirectory_can_list_worlds() {
    let selected = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&selected) else {
        eprintln!("skipped: no install ({})", selected.display());
        return;
    };
    let resolved = GameInstall::locate(Some(&install.root.join("fxart"))).unwrap();
    assert_eq!(resolved.root, install.root);
    assert!(
        !cyrus_assets::scene::world_names(&resolved)
            .unwrap()
            .is_empty()
    );
}
