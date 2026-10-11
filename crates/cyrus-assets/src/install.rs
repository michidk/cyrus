//! Explicit, read-only installation validation shared by applications.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GameInstall {
    pub root: PathBuf,
}
impl GameInstall {
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
