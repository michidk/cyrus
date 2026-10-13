//! Native byte storage shared by local preferences and cheat persistence.
use anyhow::{Context, Result};
use std::path::Path;

/// Only absence is missing state; unreadable files retain their path in the error.
pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

/// Replace a local document only after writing its complete contents.
/// The game thread is the single writer for each destination.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let result = (|| -> Result<()> {
        let parent = path.parent().context("local state path has no parent")?;
        std::fs::create_dir_all(parent)?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    result.with_context(|| format!("writing {}", path.display()))
}
