//! Explicit, read-only installation validation shared by applications.
use std::path::{Path, PathBuf};

#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct GameInstall {
    pub root: PathBuf,
}
pub const ENV_VAR: &str = "REDGUARD_DIR";
pub const LINK_NAME: &str = "redguard";

impl GameInstall {
    /// Explicit path, then REDGUARD_DIR, then conventional local/GOG install folders.
    /// Invalid explicit or environment paths are errors rather than silent fallbacks.
    pub fn locate(explicit: Option<&Path>) -> anyhow::Result<Self> {
        if let Some(path) = explicit {
            return Self::open(path);
        }
        if let Some(path) = std::env::var_os(ENV_VAR) {
            return Self::open(Path::new(&path));
        }
        let mut candidates = vec![PathBuf::from(LINK_NAME), PathBuf::from("Redguard")];
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.extend([
                home.join("GOG Games/Redguard"),
                home.join("GOG Games/Redguard/app/Redguard"),
            ]);
        }
        if let Some(drive) = std::env::var_os("SystemDrive") {
            candidates.push(PathBuf::from(format!(
                "{}/GOG Games/Redguard",
                drive.to_string_lossy()
            )));
        }
        for path in &candidates {
            if let Ok(install) = Self::open(path) {
                return Ok(install);
            }
        }
        anyhow::bail!(
            "game files not found; provide --install PATH or REDGUARD_DIR. Searched: {}",
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("{}: {e}", root.display()))?;
        for required in ["fxart", "maps", "RGFX.EXE"] {
            anyhow::ensure!(
                root.join(required).exists(),
                "{}: missing {required}; select your original Redguard installation",
                root.display()
            );
        }
        Ok(Self { root })
    }
    /// Resolves an output directory without creating anything in the read-only install.
    pub fn cache_path(&self, requested: &Path) -> anyhow::Result<PathBuf> {
        let path = std::path::absolute(requested)?;
        anyhow::ensure!(
            !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "cache path must not contain parent traversal"
        );
        let mut existing = path.as_path();
        let mut suffix = Vec::new();
        while !existing.exists() {
            suffix.push(
                existing
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("invalid cache path"))?,
            );
            existing = existing
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid cache parent"))?;
        }
        let mut resolved = existing.canonicalize()?;
        for part in suffix.into_iter().rev() {
            resolved.push(part);
        }
        anyhow::ensure!(
            !resolved.starts_with(&self.root),
            "cache must be outside the original install: {}",
            resolved.display()
        );
        Ok(resolved)
    }
    pub fn maps(&self) -> PathBuf {
        self.root.join("maps")
    }
    pub fn fxart(&self) -> PathBuf {
        self.root.join("fxart")
    }
}
/// Installed physical keyboard codes in the original up/down/left/right/a/b/c/d order.
pub fn keyboard_codes(install: &GameInstall) -> Result<[u8; 8], String> {
    let path = install.root.join("KEYS.INI");
    std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| rgpre::import::keys_ini::KeysIni::parse(&text))
        .map(|ini| ini.keyboard_codes())
}
/// Decodes the selected installed bitmap font without retaining parser types.
pub fn font(path: &std::path::Path) -> Result<cyrus_text::Font, String> {
    std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| rgpre::import::fnt::parse_fnt(&bytes).map_err(|e| e.to_string()))
        .map(|font| cyrus_text::Font::from(&font))
}
/// Installed SYSTEM.INI text, read-only; presentation chooses its existing fallback policy.
pub fn system_ini(install: &GameInstall) -> std::io::Result<String> {
    std::fs::read_to_string(install.root.join("SYSTEM.INI"))
}
