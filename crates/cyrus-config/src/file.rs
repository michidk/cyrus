//! Filesystem reads and writes of local state documents, shared by native stores.
use anyhow::{Context, Result};
use std::path::Path;

/// Create a local-state directory and any missing parents.
pub fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("creating local state directory {}", path.display()))
}

/// Only absence is missing state; unreadable files retain their path in the error.
pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

/// Write an encoded document, creating its parent directories when needed.
/// Writes overwrite directly; an interrupted or failed write may leave a partial file.
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let result = (|| -> Result<()> {
        let parent = path.parent().context("local state path has no parent")?;
        create_dir_all(parent)?;
        std::fs::write(path, bytes)?;
        Ok(())
    })();
    result.with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cyrus-storage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn documents_round_trip_and_replace_previous_contents() {
        let directory = Directory::new();
        let path = directory.0.join("saves/nested/session.json");
        assert!(read(&path).unwrap().is_none());
        write(&path, b"previous document").unwrap();
        write(&path, b"new").unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), b"new");
    }

    #[test]
    fn write_errors_include_the_destination() {
        let directory = Directory::new();
        let path = directory.0.join("session.json");
        std::fs::create_dir(&path).unwrap();
        let error = write(&path, b"document").unwrap_err();
        assert!(format!("{error:#}").contains(&path.display().to_string()));
    }

    #[test]
    fn read_errors_retain_the_destination_instead_of_reporting_absence() {
        let directory = Directory::new();
        let path = directory.0.join("session.json");
        std::fs::create_dir(&path).unwrap();
        let error = read(&path).unwrap_err();
        assert!(format!("{error:#}").contains(&path.display().to_string()));
        assert!(path.is_dir());
    }
}
