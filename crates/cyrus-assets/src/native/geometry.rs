//! Shared rgpre geometry export; game sidecars and preview presentation are separate.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
}
pub fn map_path(root: &Path, path: &str) -> Result<PathBuf> {
    let name = path.rsplit(['/', '\\']).next().context("empty map path")?;
    find_file(&root.join("maps"), name).with_context(|| format!("missing map {path}"))
}
pub use crate::runtime::geometry::{WorldGeometry, resolve_world};
pub fn world(root: &Path, name: &str) -> Result<WorldGeometry> {
    let install = crate::install::GameInstall::open(root)?;
    let mut world = crate::runtime::geometry::world(&super::Files::scan(&install, false)?, name)?;
    world.registry.root_path = install.root.clone();
    for file in world.registry.files.values_mut() {
        file.path = install.root.join(&file.path);
    }
    Ok(world)
}
