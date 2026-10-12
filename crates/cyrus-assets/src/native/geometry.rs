//! Shared rgpre geometry export; game sidecars and preview presentation are separate.
use anyhow::Result;
use std::path::Path;

pub use crate::runtime::geometry::{WorldGeometry, resolve_world};
/// Builds a world from the installed files; registry paths are absolute for later sidecar exports.
pub fn world(root: &Path, name: &str) -> Result<WorldGeometry> {
    let install = crate::install::GameInstall::open(root)?;
    let mut world = crate::runtime::geometry::world(&super::Files::scan(&install, false)?, name)?;
    world.registry.root_path = install.root.clone();
    for file in world.registry.files.values_mut() {
        file.path = install.root.join(&file.path);
    }
    Ok(world)
}
