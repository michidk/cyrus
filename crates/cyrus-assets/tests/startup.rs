#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
use cyrus_assets::{install::GameInstall, startup::export_startup};

#[test]
fn missing_dll_reports_required_asset() {
    let root = std::path::PathBuf::from("/nonexistent-redguard-splash-install");
    let error = export_startup(
        &GameInstall { root },
        &std::path::PathBuf::from("/nonexistent-splash-cache"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("3DfxSpl2.dll"));
}

#[test]
fn installed_swoosh_export_replaces_legacy_cache_and_reuses_complete_cache() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
        });
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (startup splash)");
        return;
    };
    let cache = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../game-config")
        .join(format!("startup-test-{}", std::process::id()));
    let output = cache.join("startup");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("source.txt"), "legacy shield cache").unwrap();
    std::fs::write(output.join("splash-01.png"), "legacy shield frame").unwrap();
    for name in ["title-preparing.png", "title-menu.png", "title-sound.png"] {
        std::fs::write(output.join(name), "legacy labelled title").unwrap();
    }
    export_startup(&install, &cache).unwrap();
    let title_images = std::fs::read_dir(&output)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("title")
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "png")
        })
        .count();
    assert_eq!(title_images, 1, "cache stores one title background");
    let title = image::open(output.join("title.png")).unwrap().to_rgba8();
    assert_eq!(title.dimensions(), (1820, 1024));
    let labels: cyrus_assets::title::TitleLabels =
        serde_json::from_slice(&std::fs::read(output.join("labels.title")).unwrap()).unwrap();
    for (name, status) in [
        ("title.png", "Loading..."),
        ("title-preparing.png", "Preparing menu and HUD..."),
        ("title-menu.png", "Loading menu..."),
        ("title-sound.png", "Loading sound..."),
    ] {
        let mut composed = title.clone();
        labels.draw(status, [title.width(), title.height()], |x, y, color| {
            composed.put_pixel(x, y, image::Rgba(color));
        });
        assert_ne!(
            composed, title,
            "runtime lettering is separate from the background"
        );
        assert!(
            composed
                .enumerate_pixels()
                .any(|(x, y, p)| { x < 600 && y >= 990 && p.0 == [105, 66, 30, 255] })
        );
        if let Some(directory) = std::env::var_os("CYRUS_TITLE_REFERENCE_DIR") {
            let reference = image::open(std::path::Path::new(&directory).join(name))
                .unwrap()
                .to_rgba8();
            assert_eq!(
                composed, reference,
                "runtime pixels differ from retained {name}"
            );
        }
        if let Some(directory) = std::env::var_os("CYRUS_TITLE_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            composed
                .save(std::path::Path::new(&directory).join(name))
                .unwrap();
        }
    }
    let splash_path = output.join("splash.splash");
    let asset: cyrus_assets::splash_asset::SplashAsset =
        serde_json::from_slice(&std::fs::read(&splash_path).unwrap()).unwrap();
    asset.validate().unwrap();
    // The authored frames and the two-second fade make up the 150 frames the game plays.
    assert_eq!(
        asset.frames.len() + 2 * asset.frames_per_second as usize,
        cyrus_assets::SPLASH_FRAMES
    );
    assert!(asset.frames.iter().all(|frame| !frame.is_empty()));
    assert!(!asset.final_draws.is_empty());
    // The splash has something to show: the held pose and an authored frame draw meshes that have
    // faces, and the textures they use carry visible colour.
    let has_faces = |draws: &[cyrus_assets::splash_asset::SplashDraw]| {
        draws
            .iter()
            .any(|draw| !asset.meshes[draw.mesh].faces.is_empty())
    };
    assert!(has_faces(&asset.final_draws));
    assert!(has_faces(&asset.frames[asset.frames.len() / 2]));
    assert!(asset.textures.iter().all(|texture| {
        texture
            .rgba
            .chunks(4)
            .any(|texel| texel[..3].iter().any(|c| *c > 32))
    }));
    let modified = std::fs::metadata(&splash_path).unwrap().modified().unwrap();
    export_startup(&install, &cache).unwrap();
    assert_eq!(
        std::fs::metadata(&splash_path).unwrap().modified().unwrap(),
        modified
    );
    std::fs::remove_file(&splash_path).unwrap();
    std::fs::remove_file(output.join("labels.title")).unwrap();
    export_startup(&install, &cache).unwrap();
    assert!(splash_path.is_file());
    assert!(output.join("labels.title").is_file());
    std::fs::remove_dir_all(cache).unwrap();
}
