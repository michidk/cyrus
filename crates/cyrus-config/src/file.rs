//! Atomic reads and writes of local state documents, shared by every native store.
use anyhow::{Context, Result};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_CANDIDATE: AtomicUsize = AtomicUsize::new(0);

/// Create a local-state directory and persist its ancestry on Unix.
/// Existing ancestors are synced too: a retry may follow an earlier failed sync.
/// Directory access or sync failures are reported before callers publish state.
pub fn create_dir_all(path: &Path) -> Result<()> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    std::fs::create_dir_all(path)
        .with_context(|| format!("creating local state directory {}", path.display()))?;
    #[cfg(unix)]
    {
        let directory = path
            .canonicalize()
            .with_context(|| format!("resolving local state directory {}", path.display()))?;
        for parent in directory.ancestors().skip(1) {
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("syncing local state ancestor {}", parent.display()))?;
        }
    }
    Ok(())
}

/// Only absence is missing state; unreadable files retain their path in the error.
pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

/// Replace an already encoded and validated local document with one rename.
/// Sync the complete candidate before publication and the directory after it on Unix;
/// pre-publication errors retain the accepted destination. Failed cleanup may leave an
/// unaccepted candidate.
/// The game thread is the single writer for each destination.
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let result = (|| -> Result<()> {
        let parent = path.parent().context("local state path has no parent")?;
        create_dir_all(parent)?;
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
            publish_candidate(&temporary, path)
                .with_context(|| format!("publishing candidate {}", temporary.display()))?;
            // Persist the rename itself; an error here means the replacement is published
            // but its durability is unconfirmed. Windows exposes no directory sync.
            #[cfg(unix)]
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("syncing directory {}", parent.display()))?;
            Ok(())
        })();
        if publish.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        publish
    })();
    result.with_context(|| format!("writing {}", path.display()))
}

#[cfg(not(windows))]
fn publish_candidate(candidate: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(candidate, destination)
}

#[cfg(windows)]
fn publish_candidate(candidate: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }

    fn terminated_path(path: &Path) -> std::io::Result<Vec<u16>> {
        let mut units: Vec<_> = path.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "local state path contains a null character",
            ));
        }
        units.push(0);
        Ok(units)
    }

    // Canonical paths preserve support for long Windows paths without requiring
    // the replacement to exist yet.
    let candidate = terminated_path(&candidate.canonicalize()?)?;
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent",
        )
    })?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let name = destination.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no file name",
        )
    })?;
    let destination = terminated_path(&parent.canonicalize()?.join(name))?;
    // MoveFileExW documents write-through as waiting for the move on disk.
    // Do not allow cross-volume copying: the candidate shares the destination directory.
    const REPLACE_EXISTING: u32 = 0x1;
    const WRITE_THROUGH: u32 = 0x8;
    // SAFETY: both UTF-16 paths are null-terminated and live throughout the call.
    if unsafe {
        MoveFileExW(
            candidate.as_ptr(),
            destination.as_ptr(),
            REPLACE_EXISTING | WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
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

    #[cfg(windows)]
    #[test]
    fn replacement_supports_unicode_and_long_paths() {
        let directory = Directory::new();
        let mut parent = directory.0.clone();
        for _ in 0..12 {
            parent.push("long-save-directory");
        }
        // Create the ancestry using Windows' extended path namespace, so the
        // test does not depend on the host's long-path policy.
        let parent = directory
            .0
            .canonicalize()
            .unwrap()
            .join(parent.strip_prefix(&directory.0).unwrap());
        let path = parent.join("保存-é.cyrus");
        write(&path, b"accepted").unwrap();
        write(&path, b"replacement").unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), b"replacement");
        assert_eq!(std::fs::read_dir(parent).unwrap().count(), 1);
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
        let path = directory.0.join("saves/nested/session.json");
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
