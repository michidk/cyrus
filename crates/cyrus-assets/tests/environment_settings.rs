#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! World preparation uses the same installed settings contract as other consumers.
use anyhow::Result;
use cyrus_assets::{
    environment::Environment,
    install::GameInstall,
    native::Files,
    source::{Manifest, ReadAssets},
};
use std::path::PathBuf;

struct SystemSettings<'a>(&'a Files, Vec<u8>);
impl ReadAssets for SystemSettings<'_> {
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

#[test]
fn installed_world_clipping_retains_first_settings_and_accepts_comments() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (world clipping settings)");
        return Ok(());
    };
    let files = Files::open(&install)?;
    let original = files.read("SYSTEM.INI")?;
    let text = std::str::from_utf8(&original)?;
    let prepare = |source: &dyn ReadAssets| -> Result<Environment> {
        let output = cyrus_assets::game::prepare(source, "START.environment")?;
        Ok(serde_json::from_slice(&output["START.environment"])?)
    };
    let expected = prepare(&files)?.clipping;
    let duplicate = format!("{text}\n[xngine]\nfront_plane=700\nback_plane=3700\n");
    let commented = text
        .lines()
        .map(|line| {
            if line.split_once('=').is_some_and(|(key, _)| {
                key.trim().eq_ignore_ascii_case("front_plane")
                    || key.trim().eq_ignore_ascii_case("back_plane")
            }) {
                format!("{line} ; clipping distance in engine units")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    for (label, settings) in [("duplicate", duplicate), ("commented", commented)] {
        let actual = prepare(&SystemSettings(&files, settings.into_bytes()))?;
        assert_eq!(actual.clipping, expected, "{label} SYSTEM.INI");
    }
    assert_eq!(files.read("SYSTEM.INI")?, original);
    Ok(())
}
