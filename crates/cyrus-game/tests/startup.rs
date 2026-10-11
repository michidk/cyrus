use cyrus_game::{install::GameInstall, startup::export_startup};

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
        .join("../../cyrus-config")
        .join(format!("startup-test-{}", std::process::id()));
    let output = cache.join("startup");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("source.txt"), "legacy shield cache").unwrap();
    std::fs::write(output.join("splash-01.png"), "legacy shield frame").unwrap();
    export_startup(&install, &cache).unwrap();
    assert!((0..150).all(|frame| output.join(format!("splash-{frame:02}.png")).is_file()));
    let first = image::open(output.join("splash-00.png"))
        .unwrap()
        .to_rgba8();
    assert!(first.pixels().all(|pixel| pixel.0 == [0, 0, 0, 255]));
    let logo = image::open(output.join("splash-89.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(logo.dimensions(), (640, 480));
    assert!(logo.pixels().any(|pixel| {
        pixel[0] > 80
            && u16::from(pixel[0]) > 2 * u16::from(pixel[2])
            && 2 * u16::from(pixel[0]) > 3 * u16::from(pixel[1])
    }));
    let faded = image::open(output.join("splash-149.png"))
        .unwrap()
        .to_rgba8();
    assert!(
        faded
            .pixels()
            .all(|pixel| pixel.0[..3].iter().all(|channel| *channel < 20))
    );
    let frame = output.join("splash-89.png");
    let modified = std::fs::metadata(&frame).unwrap().modified().unwrap();
    export_startup(&install, &cache).unwrap();
    assert_eq!(
        std::fs::metadata(&frame).unwrap().modified().unwrap(),
        modified
    );
    std::fs::remove_file(output.join("splash-149.png")).unwrap();
    export_startup(&install, &cache).unwrap();
    assert!(output.join("splash-149.png").is_file());
    std::fs::remove_dir_all(cache).unwrap();
}
