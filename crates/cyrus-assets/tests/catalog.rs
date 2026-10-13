//! Public catalog/cache contracts, without copyrighted fixtures.
#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
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
    std::fs::write(root.join("notes.def"), "first").unwrap();
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
        .find(|e| e.path == "notes.def")
        .unwrap();
    let preview = first.prepare(&entry.id, None).unwrap();
    std::fs::write(&preview, b"interrupted").unwrap();
    first.prepare(&entry.id, None).unwrap();
    let decoded: Preview = serde_json::from_slice(&std::fs::read(preview).unwrap()).unwrap();
    assert!(matches!(decoded.content, Content::Data { .. }));
    std::fs::write(root.join("notes.def"), "other").unwrap();
    let second = Library::open(install, cache, |_, _, _| {}).unwrap();
    assert_ne!(first.catalog.generation, second.catalog.generation);
    assert_eq!(
        entry.id,
        second
            .catalog
            .entries
            .iter()
            .find(|e| e.path == "notes.def")
            .unwrap()
            .id
    );
    assert!(GameInstall::open(&scratch.0).is_err());
}

#[test]
fn windows_bitmap_still_prepares_an_image() {
    let scratch = Scratch::new();
    let root = scratch.0.join("install");
    for dir in ["maps", "fxart"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("RGFX.EXE"), []).unwrap();
    image::RgbImage::from_pixel(1, 1, image::Rgb([12, 34, 56]))
        .save(root.join("sample.BMP"))
        .unwrap();
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
        .find(|entry| entry.path == "sample.BMP")
        .unwrap();
    let output = library.prepare(&entry.id, None).unwrap();
    let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    let Content::Images { frames } = preview.content else {
        panic!("expected BMP image")
    };
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].rgba, [12, 34, 56, 255]);
    assert_eq!(preview.metadata.unwrap()["frames"][0]["width"], 1);
}

#[test]
fn support_and_unknown_files_are_hidden_and_malformed_images_report_their_source() {
    let scratch = Scratch::new();
    let root = scratch.0.join("install");
    for dir in ["maps", "fxart", "sound", "tools.exe"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for path in [
        "RGFX.EXE",
        "fxart/helper.eXe",
        "driver.DLL",
        "sound/MSSDRVR.LST",
        "sound/content.lst",
        "maps/helper.dLl",
        "broken.MDI",
        "broken.dig",
        "broken.PIF",
        "broken.ad",
        "broken.anm",
        "broken.lbm",
        "broken.pcx",
        "broken.bmp",
        "broken.opl",
        "broken.rst",
        "tools.exe/notes.def",
        "unknown.bin",
        "notes.txt",
        "README.TXT",
    ] {
        std::fs::write(root.join(path), []).unwrap();
    }
    let library = Library::open(
        GameInstall::open(&root).unwrap(),
        scratch.0.join("cache"),
        |_, _, _| {},
    )
    .unwrap();
    let paths: Vec<_> = library
        .catalog
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "broken.anm",
            "broken.bmp",
            "broken.lbm",
            "broken.pcx",
            "sound/content.lst",
            "tools.exe/notes.def"
        ]
    );
    for entry in library
        .catalog
        .entries
        .iter()
        .filter(|entry| entry.category == Category::Images)
    {
        let output = library.prepare(&entry.id, None).unwrap();
        let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let Content::Unavailable { reason } = preview.content else {
            panic!("malformed image must report a decoder error")
        };
        assert!(reason.contains(&entry.path), "{reason}");
        assert!(reason.contains(&format!("{}:", entry.format)), "{reason}");
    }
    let (tx, rx) = std::sync::mpsc::channel();
    cyrus_assets::loading::preload(&root, &tx);
    let report = rx
        .into_iter()
        .find_map(|progress| match progress {
            cyrus_assets::loading::Progress::Done(report) => Some(report),
            _ => None,
        })
        .unwrap();
    assert_eq!(report.by_type.len(), 3);
    assert_eq!(report.failed.len(), 3);
    assert_eq!(
        report
            .failed
            .iter()
            .map(|(file, _)| file.as_str())
            .collect::<Vec<_>>(),
        ["broken.anm", "broken.lbm", "broken.pcx"]
    );
    assert_eq!(report.parsed, 0);
}

#[test]
fn installed_preservation_images_prepare_pixels_and_animation_timing() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.join("RGFX.EXE").is_file() {
        eprintln!("skipped: no install (preservation image previews)");
        return;
    }
    let scratch = Scratch::new();
    let library = Library::open(
        GameInstall::open(&root).unwrap(),
        scratch.0.join("cache"),
        |_, _, _| {},
    )
    .unwrap();
    for (path, count) in [
        ("system/SKY_61.PCX", 1),
        ("system/gui.lbm", 1),
        ("system/gui.anm", 25),
        ("system/pointers.bmp", 30),
    ] {
        let entry = library
            .catalog
            .entries
            .iter()
            .find(|entry| entry.path.eq_ignore_ascii_case(path))
            .unwrap();
        assert_eq!(entry.category, Category::Images);
        let output = library.prepare(&entry.id, None).unwrap();
        let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        preview.validate().unwrap();
        let Content::Images { frames } = preview.content else {
            panic!("{path}: {:?}", preview.content)
        };
        assert_eq!(frames.len(), count, "{path}");
        let metadata = preview.metadata.as_ref().expect("image JSON metadata");
        if path.ends_with(".bmp") {
            assert_eq!(metadata["encoding"], "GXA");
            assert_eq!(metadata["frame_count"], 30);
            assert!(frames.iter().any(|frame| {
                frame
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 0)
            }));
        }
        for frame in &frames {
            assert_eq!(frame.embedded_palette, !path.ends_with(".bmp"));
            assert_eq!(
                frame.rgba.len(),
                frame.width as usize * frame.height as usize * 4
            );
            assert!(
                frame
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[..3] != [0, 0, 0]),
                "{path}: blank image"
            );
            assert!(
                frame
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| path.ends_with(".bmp") || pixel[3] == 255),
                "{path}: opaque indexed image"
            );
        }
        if path.ends_with(".anm") {
            assert_eq!(metadata["declared_frame_count"], 26);
            assert_eq!(metadata["display_frame_count"], 25);
            assert_eq!(metadata["frames_per_second"], 10);
            assert_eq!(metadata["pages"].as_array().unwrap().len(), 2);
            assert_eq!(metadata["record_sizes"].as_array().unwrap().len(), 26);
            assert!(metadata["loop_delta_bytes"].as_u64().unwrap() > 0);
            assert!(metadata.get("pixels").is_none());
            assert_eq!(preview.frame_interval_ms, Some(100.));
            assert_ne!(frames[0].rgba, frames[1].rgba);
        } else {
            assert_eq!(preview.frame_interval_ms, None);
        }
    }
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
    assert!(
        catalog
            .entries
            .iter()
            .all(|entry| entry.category != Category::Other)
    );
    for category in Category::ALL
        .into_iter()
        .filter(|category| *category != Category::Other)
    {
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
        assert!(
            preview.metadata.is_some()
                || matches!(
                    &preview.content,
                    Content::Data { .. }
                        | Content::Spatial { .. }
                        | Content::Model { data: Some(_), .. }
                ),
            "{family}: no JSON inspection fields"
        );
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
                cyrus_media::Movie::open(&std::fs::read(library.cache.join(smk)).unwrap()).unwrap();
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
        let Content::Spatial { cells, scene, .. } = preview.content else {
            panic!("{}: {:?}", entry.path, preview.content)
        };
        assert!(!cells.is_empty());
        assert_eq!(cells[0].depth, 0);
        assert!(scene.is_some(), "{}: missing related world", entry.path);
        assert!(cells.iter().any(|cell| !cell.visibility_ids.is_empty()));
        if entry.path.eq_ignore_ascii_case("maps/ISLAND.PVO") {
            let expected = [-1779.2, 819.2, -1446.3998];
            for (actual, expected) in cells[0].center.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.001);
            }
        }
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

#[test]
fn installed_inspection_retains_terrain_and_authored_model_data() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.join("RGFX.EXE").is_file() {
        eprintln!("skipped: no install (viewer inspection data)");
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
        .find(|e| e.path.eq_ignore_ascii_case("maps/ISLAND.WLD"))
        .unwrap();
    let output = library.prepare(&entry.id, None).unwrap();
    let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    preview.validate().unwrap();
    let Content::Model {
        data: Some(data), ..
    } = preview.content
    else {
        panic!("missing terrain inspection")
    };
    assert_eq!(data["sections"].as_array().unwrap().len(), 4);
    assert_eq!(data["sections"][0]["maps"].as_array().unwrap().len(), 4);
    assert_eq!(
        data["sections"][0]["maps"][0].as_array().unwrap().len(),
        16384
    );
    for key in [
        "scene_y",
        "wave_vertices",
        "quarter_turns",
        "texture_indices",
        "height_engine_units",
    ] {
        assert_eq!(
            data["combined"][key].as_array().unwrap().len(),
            65536,
            "{key}"
        );
    }
    assert_eq!(data["combined"]["scene_x"][0], 0.);
    assert_eq!(data["combined"]["scene_x"].as_array().unwrap().len(), 256);
    assert_eq!(data["combined"]["scene_z"].as_array().unwrap().len(), 256);
    let entry = library
        .catalog
        .entries
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case("fxart/AVIKA001.3DC"))
        .unwrap();
    let output = library.prepare(&entry.id, None).unwrap();
    let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    let data = preview.metadata.unwrap();
    assert!(!data["model"]["face_data"].as_array().unwrap().is_empty());
    assert!(
        !data["model"]["vertex_coords"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!data["model"]["frame_data"].as_array().unwrap().is_empty());
    assert!(data["scene_subobjects"].is_array());
}

#[test]
fn bsi_palette_references_follow_terrain_and_sky_usage() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    if !root.join("RGFX.EXE").is_file() {
        eprintln!("skipped: no install (BSI palette references)");
        return;
    }
    let scratch = Scratch::new();
    let library = Library::open(
        GameInstall::open(&root).unwrap(),
        scratch.0.join("cache"),
        |_, _, _| {},
    )
    .unwrap();
    for (path, expected) in [
        ("fxart/TEXBSI.302", "fxart/ISLAND.COL"),
        ("fxart/SKY888.BSI", "fxart/SKY888.COL"),
    ] {
        let catalog = library.expand(path).unwrap();
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.path == path && matches!(entry.source, Source::Image(0)))
            .unwrap();
        let output = library.prepare(&entry.id, None).unwrap();
        let preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert!(
            matches!(preview.content, Content::Images { .. }),
            "{path}: {:?}",
            preview.content
        );
        let palettes: Vec<_> = catalog
            .entries
            .iter()
            .filter(|entry| {
                entry.category == Category::Palettes && preview.related.contains(&entry.id)
            })
            .map(|entry| entry.path.as_str())
            .collect();
        assert!(palettes.contains(&expected), "{path}: {palettes:?}");
        assert!(
            !palettes.contains(&"fxart/CATACOMB.COL"),
            "{path}: {palettes:?}"
        );
        if path.ends_with(".BSI") {
            assert_eq!(palettes, [expected]);
        }
    }
}
