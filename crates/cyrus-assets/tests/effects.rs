#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Installed effect preparation through the portable world-cache entry point.
use anyhow::Result;
use cyrus_assets::{environment::Environment, install::GameInstall, native::Files};
use std::path::PathBuf;

#[test]
fn caverns_prepares_placed_models_whose_extensionless_name_matches_two_standalone_files()
-> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (caverns models)");
        return Ok(());
    };
    let files = cyrus_assets::game::prepare(&Files::open(&install)?, "CAVERNS.player")?;
    let scene: cyrus_assets::player::SceneData = serde_json::from_slice(&files["CAVERNS.player"])?;
    let models = scene
        .collision
        .and_then(|collision| collision.actor_models)
        .expect("live model geometry");
    assert!(models.unsupported.is_empty(), "{:?}", models.unsupported);
    assert!(models.models.contains_key("CRAK0000"));
    Ok(())
}

#[test]
fn installed_world_prepares_effect_frames_and_map_drawing_attributes() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (effect preparation)");
        return Ok(());
    };
    let files = cyrus_assets::game::prepare(&Files::open(&install)?, "START.environment")?;
    let environment: Environment = serde_json::from_slice(&files["START.environment"])?;
    environment.validate()?;
    let effects = environment.effects.expect("prepared effect inputs");
    assert!(!effects.disabled);
    assert_eq!(effects.capacity, 32);
    assert_eq!(effects.ambient_brightness, Some(78));
    // The four death effect images, 0x2380 (the image `AddFlatEffect` calls name) and 0x2381,
    // the texture START's actor flat records draw.
    assert_eq!(effects.images.len(), 6);
    assert!(!effects.images[&0x2380].frames.is_empty());
    // Verified installed BSI frame ranges and map FLAT alpha inputs.
    for (identity, size, count, delay, alpha) in [
        (0xb081, [74, 70], 16, 71, 151),
        (0x2480, [82, 54], 13, 114, 141),
        (0x2580, [86, 64], 5, 114, 161),
        (0x2500, [64, 60], 4, 71, 151),
    ] {
        let image = &effects.images[&identity];
        assert_eq!(image.size, size);
        assert_eq!(image.frames.len(), count);
        assert_eq!(image.delay_ms, delay);
        assert_eq!(image.alpha, alpha);
        assert_eq!(image.flags, 1);
        assert_eq!(image.texture_scale, 256);
        assert!(
            image
                .frames
                .iter()
                .all(|frame| frame.len() == (size[0] * size[1] * 4) as usize)
        );
        assert!(
            image.frames[0]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 0)
        );
        assert!(
            image.frames[0]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 255)
        );
    }
    Ok(())
}

#[test]
fn installed_glide_sky_ignores_unused_motion_setting() -> Result<()> {
    use cyrus_assets::source::{Manifest, ReadAssets};
    struct Settings<'a>(&'a Files, Vec<u8>);
    impl ReadAssets for Settings<'_> {
        fn manifest(&self) -> &Manifest {
            self.0.manifest()
        }
        fn read_range(&self, path: &str, start: u64, length: usize) -> Result<Vec<u8>> {
            self.0.read_range(path, start, length)
        }
        fn read(&self, path: &str) -> Result<Vec<u8>> {
            if path.eq_ignore_ascii_case("SYSTEM.INI") {
                Ok(self.1.clone())
            } else {
                self.0.read(path)
            }
        }
    }
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (sky motion switch)");
        return Ok(());
    };
    let files = Files::open(&install)?;
    let original = files.read("SYSTEM.INI")?;
    let mut skies = Vec::new();
    for enabled in [false, true] {
        let mut settings = std::str::from_utf8(&original)?
            .lines()
            .filter(|line| {
                !line
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case("sky_move"))
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        settings
            .extend_from_slice(format!("\n[xngine]\nsky_move={}\n", u8::from(enabled)).as_bytes());
        let prepared =
            cyrus_assets::game::prepare(&Settings(&files, settings), "START.environment")?;
        let environment: serde_json::Value =
            serde_json::from_slice(&prepared["START.environment"])?;
        assert!(environment["sky"]["speed"].as_u64().unwrap() > 0);
        skies.push(environment["sky"].clone());
    }
    assert!(skies[0] == skies[1], "Glide sky does not consume sky_move");
    assert_eq!(skies[0]["rgba"].as_array().unwrap().len(), 256 * 256 * 4);
    assert_eq!(skies[0]["speed"], skies[1]["speed"]);
    assert_eq!(files.read("SYSTEM.INI")?, original);
    Ok(())
}
