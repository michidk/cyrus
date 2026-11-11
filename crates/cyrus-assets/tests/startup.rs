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
    export_startup(&install, &cache).unwrap();
    let title = image::open(output.join("title.png")).unwrap().to_rgba8();
    assert_eq!(title.get_pixel(10, 469).0, [105, 66, 30, 255]);
    let version_pixels = (469..=474)
        .flat_map(|y| (10..=86).map(move |x| (x, y)))
        .filter(|&(x, y)| title.get_pixel(x, y).0 == [105, 66, 30, 255])
        .count();
    assert_eq!(version_pixels, 187);
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
    let modified = std::fs::metadata(&splash_path).unwrap().modified().unwrap();
    export_startup(&install, &cache).unwrap();
    assert_eq!(
        std::fs::metadata(&splash_path).unwrap().modified().unwrap(),
        modified
    );
    std::fs::remove_file(&splash_path).unwrap();
    std::fs::remove_file(output.join("title-sound.png")).unwrap();
    export_startup(&install, &cache).unwrap();
    assert!(splash_path.is_file());
    assert!(output.join("title-sound.png").is_file());
    std::fs::remove_dir_all(cache).unwrap();
}
