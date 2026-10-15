//! Native byte storage shared by local preferences and cheat persistence.
use anyhow::{Context, Result};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_CANDIDATE: AtomicUsize = AtomicUsize::new(0);

/// Only absence is missing state; unreadable files retain their path in the error.
pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

/// Replace an already encoded and validated local document with one rename.
/// Sync the complete candidate before publication; pre-publication errors retain
/// the accepted destination. Failed cleanup may leave an unaccepted candidate.
/// The game thread is the single writer for each destination.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let result = (|| -> Result<()> {
        let parent = path.parent().context("local state path has no parent")?;
        std::fs::create_dir_all(parent)?;
        let (temporary, mut file) = loop {
            let temporary = parent.join(format!(
                ".cyrus-{}-{}.tmp",
                std::process::id(),
                NEXT_CANDIDATE.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("creating candidate {}", temporary.display()));
                }
            }
        };
        let publish = (|| -> Result<()> {
            file.write_all(bytes)
                .with_context(|| format!("writing candidate {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("syncing candidate {}", temporary.display()))?;
            drop(file);
            std::fs::rename(&temporary, path)
                .with_context(|| format!("publishing candidate {}", temporary.display()))?;
            Ok(())
        })();
        if publish.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        publish
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
    fn replacement_preserves_an_unrelated_temporary_document() {
        let directory = Directory::new();
        let path = directory.0.join("session.json");
        let unrelated = path.with_extension("tmp");
        std::fs::write(&path, b"previous accepted document").unwrap();
        std::fs::write(&unrelated, b"another writer's candidate").unwrap();

        write(&path, b"complete replacement document").unwrap();

        assert_eq!(
            read(&path).unwrap().unwrap(),
            b"complete replacement document"
        );
        assert_eq!(
            std::fs::read(&unrelated).unwrap(),
            b"another writer's candidate"
        );
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[test]
    fn complete_opaque_documents_replace_and_reopen_without_candidate_files() {
        let directory = Directory::new();
        let path = directory.0.join("saves/session.json");
        assert!(read(&path).unwrap().is_none());
        write(&path, b"previous\0document").unwrap();
        write(&path, b"replacement\xffdocument\0").unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), b"replacement\xffdocument\0");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        write(&path, b"").unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), b"");
    }

    #[test]
    fn failed_publication_preserves_destination_and_cleans_only_its_candidate() {
        let directory = Directory::new();
        let path = directory.0.join("session.json");
        std::fs::create_dir(&path).unwrap();
        let accepted = path.join("accepted");
        std::fs::write(&accepted, b"preserve accepted state").unwrap();
        let unrelated = directory.0.join("session.tmp");
        std::fs::write(&unrelated, b"preserve unrelated candidate").unwrap();

        let error = write(&path, b"rejected replacement").unwrap_err();

        assert!(format!("{error:#}").contains(&path.display().to_string()));
        assert!(format!("{error:#}").contains("publishing candidate"));
        assert_eq!(
            std::fs::read(&accepted).unwrap(),
            b"preserve accepted state"
        );
        assert_eq!(
            std::fs::read(&unrelated).unwrap(),
            b"preserve unrelated candidate"
        );
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
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
