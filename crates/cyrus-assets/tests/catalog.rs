//! Public catalog/cache contracts, without copyrighted fixtures.
#![cfg(not(target_arch = "wasm32"))]
use cyrus_assets::{Category, Content, Preview, Source, install::GameInstall, native::Library};
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cyrus-assets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn changed_sources_and_interrupted_cache_are_rebuilt() {
    let scratch = Scratch::new();
    let root = scratch.0.join("install");
    for dir in ["maps", "fxart"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("RGFX.EXE"), []).unwrap();
    std::fs::write(root.join("notes.txt"), "first").unwrap();
    let install = GameInstall::open(&root).unwrap();
    let cache = scratch.0.join("cache");
    assert_eq!(install.cache_path(&cache).unwrap(), cache);
    assert!(install.cache_path(&root.join("new-cache")).is_err());
    assert!(!root.join("new-cache").exists());
    #[cfg(unix)]
    {
        let alias = scratch.0.join("install-alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert!(install.cache_path(&alias.join("new-cache")).is_err());
    }
    let first = Library::open(install.clone(), cache.clone(), |_, _, _| {}).unwrap();
    let entry = first
        .catalog
        .entries
        .iter()
        .find(|e| e.path == "notes.txt")
        .unwrap();
    let preview = first.prepare(&entry.id, None).unwrap();
    std::fs::write(&preview, b"interrupted").unwrap();
    first.prepare(&entry.id, None).unwrap();
    let decoded: Preview = serde_json::from_slice(&std::fs::read(preview).unwrap()).unwrap();
    assert!(matches!(decoded.content, Content::Data { .. }));
    std::fs::write(root.join("notes.txt"), "other").unwrap();
    let second = Library::open(install, cache, |_, _, _| {}).unwrap();
    assert_ne!(first.catalog.generation, second.catalog.generation);
    assert_eq!(
        entry.id,
        second
            .catalog
            .entries
            .iter()
            .find(|e| e.path == "notes.txt")
            .unwrap()
            .id
    );
    assert!(GameInstall::open(&scratch.0).is_err());
}

#[test]
fn installed_asset_families_prepare_portable_previews() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.join("RGFX.EXE").is_file() {
        eprintln!("skipped: no install (asset viewer families)");
        return;
    }
    let install = GameInstall::open(&root).unwrap();
    let geometry = cyrus_assets::native::geometry::world(&install.root, "START").unwrap();
    assert_eq!(geometry.registry.root_path, install.root);
    assert!(
        geometry
            .registry
            .files
            .values()
            .all(|file| file.path.starts_with(&install.root) && file.path.is_file())
    );
    drop(geometry);
    let scratch = Scratch::new();
    let cache = scratch.0.join("cache");
    let library = Library::open(install, cache, |_, _, _| {}).unwrap();
    let archives: Vec<_> = library
        .catalog
        .entries
        .iter()
        .filter(|entry| matches!(entry.source, Source::Archive))
        .map(|entry| entry.path.clone())
        .collect();
    let mut catalog = library.catalog.clone();
    for path in archives {
        catalog = library.expand(&path).unwrap();
    }
    for category in Category::ALL {
        assert!(
            catalog.entries.iter().any(|e| e.category == category),
            "missing category {category:?}"
        );
    }
    type Family = (&'static str, fn(&cyrus_assets::Entry) -> bool);
    let predicates: [Family; 14] = [
        ("indexed image", |e| matches!(e.source, Source::Image(_))),
        ("title", |e| e.path.to_uppercase().ends_with("STARTUP2.GXA")),
        ("palette", |e| e.category == Category::Palettes),
        ("font", |e| e.path.eq_ignore_ascii_case("fonts/FONTSEL.FNT")),
        ("standalone model", |e| {
            e.category == Category::Models && matches!(e.source, Source::File)
        }),
        ("archive model", |e| {
            e.path.eq_ignore_ascii_case("fxart/BELLTOWR.ROB") && e.name == "TOBYMUG"
        }),
        ("effect", |e| matches!(e.source, Source::Effect(_))),
        ("speech", |e| {
            e.category == Category::Speech && matches!(e.source, Source::Speech(_))
        }),
        ("definition", |e| e.path.eq_ignore_ascii_case("MENU.INI")),
        ("world", |e| {
            matches!(e.source, Source::World(_)) && e.name.to_uppercase().contains("START.RGM")
        }),
        ("spatial data", |e| e.path.to_uppercase().ends_with(".PVO")),
        ("splash", |e| matches!(e.source, Source::Splash(0))),
        ("CD track", |e| matches!(e.source, Source::CdTrack(2))),
        (
            "movie",
            |e| matches!(&e.source,Source::CdMovie(name) if name.eq_ignore_ascii_case("INTRO.SMK")),
        ),
    ];
    for (family, predicate) in predicates {
        let entry = catalog
            .entries
            .iter()
            .find(|e| predicate(e))
            .unwrap_or_else(|| panic!("no {family}"));
        eprintln!("preparing {family}: {}", entry.name);
        if matches!(entry.source, Source::World(_)) {
            let manifest = library.cache.join(catalog.preview_path(entry, None));
            if manifest.exists() {
                std::fs::remove_file(manifest).unwrap();
            }
        }
        let path = library.prepare(&entry.id, None).unwrap();
        let preview: Preview = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        preview.validate().unwrap();
        if family == "archive model" {
            let Content::Model { palettes, .. } = &preview.content else {
                panic!("expected model");
            };
            assert_eq!(palettes, &["fxart/ISLAND.COL"]);
        }
        assert!(
            !matches!(preview.content, Content::Unavailable { .. }),
            "{family}: {:?}",
            preview.content
        );
        for file in preview.files() {
            assert!(
                library.cache.join(file).is_file(),
                "{family}: missing {file}"
            );
        }
        if let Content::Movie { smk, .. } = &preview.content {
            let mut movie =
                cyrus_assets::media::Movie::open(&std::fs::read(library.cache.join(smk)).unwrap())
                    .unwrap();
            assert_eq!(
                movie.rgba().len(),
                movie.width as usize * movie.height as usize * 4
            );
            movie.advance().unwrap();
            movie.seek(0).unwrap();
            assert_eq!(movie.frame, 0);
        }
        if let Content::Audio { wav, .. } = &preview.content {
            std::fs::remove_file(library.cache.join(wav)).unwrap();
            library.prepare(&entry.id, None).unwrap();
            assert!(
                library.cache.join(wav).is_file(),
                "missing derived audio was not repaired"
            );
        }
    }
}

#[test]
fn model_texture_loading_reads_only_referenced_banks() {
    use cyrus_assets::source::{FileInfo, Manifest, ReadAssets};
    use std::cell::RefCell;
    struct Source {
        manifest: Manifest,
        reads: RefCell<Vec<String>>,
    }
    impl ReadAssets for Source {
        fn manifest(&self) -> &Manifest {
            &self.manifest
        }
        fn read_range(&self, path: &str, _: u64, _: usize) -> anyhow::Result<Vec<u8>> {
            self.reads.borrow_mut().push(path.into());
            Ok(Vec::new())
        }
    }
    let source = Source {
        manifest: Manifest {
            files: [188, 200, 415]
                .map(|id| FileInfo {
                    path: format!("fxart/TEXBSI.{id}"),
                    bytes: 0,
                    digest: "a".repeat(64),
                })
                .into(),
        },
        reads: RefCell::new(Vec::new()),
    };
    cyrus_assets::runtime::geometry::textures(&source, None, Some(&[700].into())).unwrap();
    assert_eq!(*source.reads.borrow(), ["fxart/TEXBSI.188"]);
}

#[test]
fn standalone_model_palette_choices_follow_world_usage_and_separate_cached_colors() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.join("RGFX.EXE").is_file() {
        eprintln!("skipped: no install (model palette choices)");
        return;
    }
    let scratch = Scratch::new();
    let library = Library::open(
        GameInstall::open(&root).unwrap(),
        scratch.0.join("cache"),
        |_, _, _| {},
    )
    .unwrap();
    let entry = library
        .catalog
        .entries
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case("fxart/AVIKA001.3DC"))
        .unwrap();
    let plain_path = library.prepare(&entry.id, Some("")).unwrap();
    let plain: Preview = serde_json::from_slice(&std::fs::read(&plain_path).unwrap()).unwrap();
    let Content::Model {
        frames, palettes, ..
    } = &plain.content
    else {
        panic!("expected model: {:?}", plain.content);
    };
    assert_eq!(
        palettes,
        &["fxart/ISLAND.COL", "fxart/NIGHTSKY.COL", "fxart/sunset.col"]
    );
    let colored_path = library
        .prepare(&entry.id, Some("fxart/ISLAND.COL"))
        .unwrap();
    assert_ne!(plain_path, colored_path);
    let colored: Preview = serde_json::from_slice(&std::fs::read(&colored_path).unwrap()).unwrap();
    let Content::Model {
        frames: colored_frames,
        ..
    } = &colored.content
    else {
        panic!("expected colored model");
    };
    assert_ne!(frames, colored_frames);
    assert_ne!(
        std::fs::read(library.cache.join(&frames[0])).unwrap(),
        std::fs::read(library.cache.join(&colored_frames[0])).unwrap()
    );
    assert_eq!(library.prepare(&entry.id, Some("")).unwrap(), plain_path);
    let default_path = library.prepare(&entry.id, None).unwrap();
    let default_preview: Preview =
        serde_json::from_slice(&std::fs::read(default_path).unwrap()).unwrap();
    let Content::Model {
        frames: default_frames,
        ..
    } = default_preview.content
    else {
        panic!("expected default model")
    };
    assert_eq!(&default_frames, colored_frames);
    let unrelated_path = library
        .prepare(&entry.id, Some("fxart/CATACOMB.COL"))
        .unwrap();
    let unrelated: Preview =
        serde_json::from_slice(&std::fs::read(unrelated_path).unwrap()).unwrap();
    assert!(matches!(unrelated.content, Content::Unavailable { .. }));
}

#[test]
fn installed_reported_archives_and_animation_have_content() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.join("RGFX.EXE").is_file() {
        eprintln!("skipped: no install (reported viewer assets)");
        return;
    }
    let scratch = Scratch::new();
    let library = Library::open(
        GameInstall::open(&root).unwrap(),
        scratch.0.join("cache"),
        |_, _, _| {},
    )
    .unwrap();
    for path in ["fxart/DRINT.ROB", "sound/MAIN.SFX", "ENGLISH.RTX"] {
        let catalog = library.expand(path).unwrap();
        let members: Vec<_> = catalog
            .entries
            .iter()
            .filter(|e| e.path == path && !matches!(e.source, Source::Archive))
            .collect();
        assert!(!members.is_empty(), "{path}: no indexed members");
        eprintln!("{path}: {} members", members.len());
        let output = library.prepare(&members[0].id, None).unwrap();
        let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert!(
            !matches!(preview.content, Content::Unavailable { .. }),
            "{path}: {:?}",
            preview.content
        );
    }
    for entry in library
        .catalog
        .entries
        .iter()
        .filter(|entry| entry.path.to_ascii_uppercase().ends_with(".PVO"))
    {
        let output = library.prepare(&entry.id, None).unwrap();
        let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let Content::Spatial { cells, .. } = preview.content else {
            panic!("{}: {:?}", entry.path, preview.content)
        };
        assert!(!cells.is_empty());
        assert_eq!(cells[0].depth, 0);
        assert!(cells.iter().any(|cell| cell.visible_indices > 0));
    }
    let avik = library
        .catalog
        .entries
        .iter()
        .find(|e| e.path == "fxart/AVIKA001.3DC")
        .unwrap();
    let output = library.prepare(&avik.id, None).unwrap();
    let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    let Content::Model {
        frames, palettes, ..
    } = preview.content
    else {
        panic!("Avik: {:?}", preview.content)
    };
    assert_eq!(frames.len(), 124);
    assert_eq!(palettes.first().unwrap(), "fxart/ISLAND.COL");
    assert_ne!(
        std::fs::read(library.cache.join(&frames[0])).unwrap(),
        std::fs::read(library.cache.join(&frames[1])).unwrap()
    );
}
