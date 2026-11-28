//! Complete original-slot validation and atomic native filesystem publication.
pub mod request;

use anyhow::{Context, Result, ensure};
use rgpre::import::savegame::{
    SaveGame, SaveSection, SaveSlot, encode_savegame_file, parse_savegame_file,
};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// The complete ordered record census for one supported world snapshot.
#[derive(Debug, Clone)]
pub struct FileLayout {
    /// Installed WORLD.INI identity, checked against encoded SVGM metadata.
    pub world: u32,
    /// Every required record in its encoded order, including SVGM but excluding END.
    pub records: Vec<[u8; 4]>,
}

/// Exact resource census supplied by the complete inverse-mapping implementation.
#[derive(Debug, Clone)]
pub struct SlotLayout {
    /// SAVEGAME.SAV and every required uppercase per-world TSG filename.
    pub files: BTreeMap<String, FileLayout>,
    /// Whether this session requires LOGBOOK.TXT, including an empty journal.
    pub logbook: bool,
}

/// Semantic validation required before any original slot can reach the filesystem.
///
/// Implementations must derive the complete census from the session and installed
/// world owners, reject every unsupported inverse mapping, and check record
/// contents and cross-record references. Container framing alone is insufficient.
/// There is deliberately no default or partial implementation.
pub trait CompleteSlotValidator {
    /// Require the current world, every visited snapshot, and all their records.
    fn layout(&self) -> Result<SlotLayout>;
    /// Reject unsupported or malformed contents, including owner/count mismatches.
    fn validate_record(&self, file: &str, world: u32, record: &SaveSection) -> Result<()>;
    /// Validate cross-file identity, references and journal state without I/O.
    fn validate_slot(&self, slot: &SaveSlot) -> Result<()>;
}

/// Immutable resource bytes admitted by a complete semantic validator.
#[derive(Debug)]
pub struct CompleteSlot {
    files: BTreeMap<String, EncodedResource>,
    world: u32,
}

#[derive(Debug)]
struct EncodedResource {
    world: u32,
    bytes: Vec<u8>,
}

impl CompleteSlot {
    /// Validate the whole slot and encode its records entirely in memory.
    pub fn validate(slot: &SaveSlot, validator: &impl CompleteSlotValidator) -> Result<Self> {
        let world = slot.game.metadata.world_id;
        let layout = validator
            .layout()
            .with_context(|| format!("SAVEGAME.SAV world {world}: complete export layout"))?;
        ensure!(
            layout.files.contains_key("SAVEGAME.SAV"),
            "SAVEGAME.SAV world {world}: layout missing main save"
        );
        for (name, expected) in layout
            .files
            .iter()
            .filter(|(name, _)| *name != "SAVEGAME.SAV")
        {
            validate_map_name(name)
                .with_context(|| format!("{name} world {}: export layout", expected.world))?;
        }
        let mut games = BTreeMap::from([("SAVEGAME.SAV", &slot.game)]);
        for (name, game) in &slot.maps {
            validate_map_name(name)
                .with_context(|| format!("{name} world {}", game.metadata.world_id))?;
            games.insert(name.as_str(), game);
        }
        for (file, expected) in &layout.files {
            ensure!(
                games.contains_key(file.as_str()),
                "{file} world {}: required snapshot missing",
                expected.world
            );
        }
        let mut files = BTreeMap::new();
        for (file, game) in games {
            let world = game.metadata.world_id;
            let expected = layout.files.get(file).with_context(|| {
                format!("{file} world {world}: snapshot unsupported by complete layout")
            })?;
            let bytes = validate_game(file, game, expected, validator)
                .with_context(|| format!("{file} world {world}: export validation"))?;
            files.insert(file.to_owned(), EncodedResource { world, bytes });
        }
        ensure!(
            slot.logbook.is_some() == layout.logbook,
            "LOGBOOK.TXT world {world}: journal presence differs from complete layout (required {})",
            layout.logbook
        );
        validator.validate_slot(slot).with_context(|| {
            format!("SAVEGAME.SAV world {world}: cross-file/journal validation")
        })?;
        if let Some(journal) = &slot.logbook {
            files.insert(
                "LOGBOOK.TXT".into(),
                EncodedResource {
                    world,
                    bytes: journal.clone(),
                },
            );
        }
        Ok(Self { files, world })
    }
}

fn validate_game(
    file: &str,
    game: &SaveGame,
    layout: &FileLayout,
    validator: &impl CompleteSlotValidator,
) -> Result<Vec<u8>> {
    ensure!(
        game.metadata.world_id == layout.world,
        "SVGM world identity differs (required {})",
        layout.world
    );
    let actual: Vec<_> = game.sections.iter().map(|section| section.name).collect();
    let labels = |names: &[[u8; 4]]| {
        names
            .iter()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect::<Vec<_>>()
    };
    ensure!(
        actual == layout.records,
        "record census differs: required {:?}, found {:?}",
        labels(&layout.records),
        labels(&actual)
    );
    let bytes = encode_savegame_file(&game.sections).context("container encoding")?;
    let encoded = parse_savegame_file(&bytes).context("encoded container validation")?;
    ensure!(
        encoded.metadata == game.metadata,
        "SVGM encoded metadata differs from supplied metadata"
    );
    for record in &game.sections {
        validator
            .validate_record(file, game.metadata.world_id, record)
            .with_context(|| {
                format!(
                    "{} record unsupported or malformed",
                    String::from_utf8_lossy(&record.name)
                )
            })?;
    }
    Ok(bytes)
}

fn validate_map_name(name: &str) -> Result<()> {
    let stem = name
        .strip_suffix(".TSG")
        .context("expected an uppercase TSG filename")?;
    ensure!(
        (1..=8).contains(&stem.len())
            && stem
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
        "unsafe per-world snapshot filename {name:?}"
    );
    ensure!(
        !matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
            && !(stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9')),
        "reserved per-world snapshot filename {name:?}"
    );
    Ok(())
}

/// Consent supplied by the player-facing caller, never inferred from slot presence.
#[derive(Debug, Clone)]
pub enum Replacement {
    /// Fail if any existing slot has this identity.
    Forbid,
    /// The player explicitly confirmed replacement of this selected slot.
    ConfirmedByPlayer(ExistingSlot),
}

/// Read-only version of the exact existing slot presented for confirmation.
/// Private fields prevent manufacturing consent for a different slot or version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingSlot {
    root: PathBuf,
    slot: u16,
    path: PathBuf,
    directory: [u64; 2],
    contents: blake3::Hash,
}

impl ExistingSlot {
    /// Capture the selected slot before asking the player to confirm replacement.
    /// This reads every regular resource, including files unknown to the exporter.
    /// Symlinks and nested resources reject instead of redirecting reads or writes.
    pub fn inspect(root: &Path, slot: u16) -> Result<Self> {
        ensure!(slot <= 999, "slot identity must fit three decimal digits");
        let root = save_root(root)?;
        let parent = File::open(&root)?;
        parent
            .lock()
            .context("locking SAVEGAME directory for confirmation")?;
        Self::read(&root, slot)
            .with_context(|| format!("inspecting SAVEGAME.{slot:03} in {}", root.display()))
    }

    /// Exact directory the player is confirming, with its original spelling.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(root: &Path, slot: u16) -> Result<Self> {
        let path = destination(root, slot)?.context("selected existing slot is absent")?;
        let directory = directory_identity(&fs::metadata(&path)?);
        let mut entries = fs::read_dir(&path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut hash = blake3::Hasher::new();
        for entry in entries {
            ensure!(
                entry.file_type()?.is_file(),
                "{}: confirmation requires regular resources; links and nested directories unsupported",
                entry.path().display()
            );
            let name = entry.file_name();
            let name = name.as_encoded_bytes();
            hash.update(&(name.len() as u64).to_le_bytes());
            hash.update(name);
            let mut file = File::open(entry.path())?;
            let mut content = blake3::Hasher::new();
            let mut buffer = [0; 65536];
            loop {
                let length = file.read(&mut buffer)?;
                if length == 0 {
                    break;
                }
                content.update(&buffer[..length]);
            }
            hash.update(content.finalize().as_bytes());
        }
        ensure!(
            directory_identity(&fs::metadata(&path)?) == directory,
            "existing slot changed during confirmation inspection"
        );
        Ok(Self {
            root: root.to_owned(),
            slot,
            path,
            directory,
            contents: hash.finalize(),
        })
    }
}

#[cfg(unix)]
fn directory_identity(metadata: &fs::Metadata) -> [u64; 2] {
    use std::os::unix::fs::MetadataExt;
    [metadata.dev(), metadata.ino()]
}

#[cfg(not(unix))]
fn directory_identity(_metadata: &fs::Metadata) -> [u64; 2] {
    // Publication is unsupported on these platforms, so no token is consumed.
    [0, 0]
}

/// Publication has committed even if later cleanup or directory syncing failed.
#[derive(Debug)]
#[must_use = "publication committed; inspect cleanup_error and sync_error before reporting success"]
pub struct Publication {
    /// Installed complete slot directory, preserving an existing spelling.
    pub path: PathBuf,
    /// Old slot remains at the named staging path if cleanup failed after commit.
    pub cleanup_error: Option<String>,
    /// A post-commit directory sync failed; callers must not retry as a failed save.
    pub sync_error: Option<String>,
}

/// Stage and atomically publish under an existing, real SAVEGAME directory.
///
/// Failed validation cannot obtain a `CompleteSlot`. Failed staging/publication
/// preserves an existing slot and removes only this operation's temporary files.
/// Callers must obtain explicit player confirmation before selecting replacement.
pub fn publish(
    root: &Path,
    slot: u16,
    payload: &CompleteSlot,
    replacement: Replacement,
) -> Result<Publication> {
    publish_inner(root, slot, payload, replacement).with_context(|| {
        format!(
            "original slot SAVEGAME.{slot:03} world {} in {}",
            payload.world,
            root.display()
        )
    })
}

fn publish_inner(
    root: &Path,
    slot: u16,
    payload: &CompleteSlot,
    replacement: Replacement,
) -> Result<Publication> {
    ensure!(slot <= 999, "slot identity must fit three decimal digits");
    ensure!(
        cfg!(any(target_os = "linux", target_os = "macos")),
        "atomic slot directory publication is unsupported on this platform"
    );
    let root = save_root(root)?;
    let parent = File::open(&root).context("opening SAVEGAME directory")?;
    parent
        .lock()
        .context("locking SAVEGAME directory for publication")?;
    let existing = destination(&root, slot)?;
    match &replacement {
        Replacement::Forbid => ensure!(
            existing.is_none(),
            "explicit player confirmation required to replace this existing slot"
        ),
        Replacement::ConfirmedByPlayer(confirmed) => {
            ensure!(
                confirmed.root == root && confirmed.slot == slot,
                "confirmation belongs to a different root or slot"
            );
            ensure!(
                *confirmed == ExistingSlot::read(&root, slot)?,
                "existing slot changed since player confirmation"
            );
        }
    }
    let target = existing
        .clone()
        .unwrap_or_else(|| root.join(format!("SAVEGAME.{slot:03}")));
    let stage = stage(&root, payload, write_resource)?;
    let before_commit = (|| -> Result<()> {
        ensure!(
            destination(&root, slot)? == existing,
            "slot identity changed during staging; confirmation is stale"
        );
        if let Replacement::ConfirmedByPlayer(confirmed) = &replacement {
            ensure!(
                *confirmed == ExistingSlot::read(&root, slot)?,
                "existing slot changed during staging; confirmation is stale"
            );
        }
        atomic_publish(&parent, stage.path(), &target, existing.is_some())
            .context("atomic directory publication")
    })();
    if let Err(error) = before_commit {
        return Err(discard(stage, error));
    }
    // After exchange the old complete slot occupies the staging path. After a
    // no-replace rename that path is absent. Neither case is a failed save.
    let temporary = stage.path().to_owned();
    let cleanup_error = if existing.is_some() {
        stage
            .close()
            .err()
            .map(|error| format!("removing replaced slot {}: {error}", temporary.display()))
    } else {
        drop(stage);
        None
    };
    let sync_error = parent.sync_all().err().map(|error| {
        format!(
            "syncing SAVEGAME root {} after publication: {error}",
            root.display()
        )
    });
    Ok(Publication {
        path: target,
        cleanup_error,
        sync_error,
    })
}

fn save_root(root: &Path) -> Result<PathBuf> {
    ensure!(
        fs::symlink_metadata(root)
            .context("examining SAVEGAME root")?
            .file_type()
            .is_dir(),
        "SAVEGAME root must be a real directory, not a symlink"
    );
    root.canonicalize().context("resolving SAVEGAME root")
}

fn destination(root: &Path, slot: u16) -> Result<Option<PathBuf>> {
    let name = format!("SAVEGAME.{slot:03}");
    let mut found = None;
    for entry in fs::read_dir(root).context("listing SAVEGAME directory")? {
        let entry = entry.context("reading SAVEGAME entry")?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(&name)
        {
            ensure!(
                found.is_none(),
                "ambiguous case-insensitive slot identity {name}"
            );
            ensure!(
                entry.file_type()?.is_dir(),
                "{}: existing slot must be a real directory, not a file or symlink",
                entry.path().display()
            );
            found = Some(entry.path());
        }
    }
    Ok(found)
}

fn stage(
    root: &Path,
    payload: &CompleteSlot,
    mut write: impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<tempfile::TempDir> {
    let directory = tempfile::Builder::new()
        .prefix(".cyrus-slot-")
        .tempdir_in(root)
        .context("creating complete-slot staging directory")?;
    let result = (|| -> Result<()> {
        for (name, resource) in &payload.files {
            write(&directory.path().join(name), &resource.bytes)
                .with_context(|| format!("staging {name} world {}", resource.world))?;
        }
        File::open(directory.path())?
            .sync_all()
            .context("syncing complete staging directory")?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(directory),
        Err(error) => Err(discard(directory, error)),
    }
}

fn write_resource(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn discard(directory: tempfile::TempDir, error: anyhow::Error) -> anyhow::Error {
    let path = directory.path().to_owned();
    match directory.close() {
        Ok(()) => error,
        Err(cleanup) => error.context(format!(
            "also failed to remove staging directory {}: {cleanup}",
            path.display()
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn atomic_publish(parent: &File, stage: &Path, target: &Path, replace: bool) -> Result<()> {
    use rustix::fs::{RenameFlags, renameat_with};
    let flags = if replace {
        RenameFlags::EXCHANGE
    } else {
        RenameFlags::NOREPLACE
    };
    renameat_with(
        parent,
        stage.file_name().context("staging filename missing")?,
        parent,
        target.file_name().context("target filename missing")?,
        flags,
    )?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn atomic_publish(_parent: &File, _stage: &Path, _target: &Path, _replace: bool) -> Result<()> {
    anyhow::bail!("atomic slot directory publication is unsupported on this platform")
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    fn payload() -> CompleteSlot {
        CompleteSlot {
            files: BTreeMap::from([
                (
                    "SAVEGAME.SAV".into(),
                    EncodedResource {
                        world: 30,
                        bytes: b"staged main".to_vec(),
                    },
                ),
                (
                    "START.TSG".into(),
                    EncodedResource {
                        world: 1,
                        bytes: b"staged map".to_vec(),
                    },
                ),
            ]),
            world: 30,
        }
    }

    #[test]
    fn failed_second_resource_write_removes_the_first_staged_resource() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("SAVEGAME.007");
        fs::create_dir(&old).unwrap();
        fs::write(old.join("SAVEGAME.SAV"), b"old complete slot").unwrap();
        let mut writes = 0;
        let error = stage(root.path(), &payload(), |path, bytes| {
            writes += 1;
            if writes == 2 {
                // A failed write may have created a partial file already.
                fs::write(path, &bytes[..1])?;
                anyhow::bail!("injected write failure");
            }
            write_resource(path, bytes)
        })
        .unwrap_err();
        assert_eq!(writes, 2);
        let error = format!("{error:#}");
        assert!(error.contains("START.TSG world 1"), "{error}");
        assert!(error.contains("injected write failure"), "{error}");
        assert_eq!(
            fs::read(old.join("SAVEGAME.SAV")).unwrap(),
            b"old complete slot"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn no_replace_commit_preserves_a_destination_created_after_staging() {
        let root = tempfile::tempdir().unwrap();
        let parent = File::open(root.path()).unwrap();
        let stage = stage(root.path(), &payload(), write_resource).unwrap();
        let target = root.path().join("SAVEGAME.007");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("SAVEGAME.SAV"), b"concurrent slot").unwrap();
        let error = atomic_publish(&parent, stage.path(), &target, false).unwrap_err();
        let _error = discard(stage, error);
        assert_eq!(
            fs::read(target.join("SAVEGAME.SAV")).unwrap(),
            b"concurrent slot"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn exchange_failure_preserves_staging_and_cleanup_removes_it() {
        let root = tempfile::tempdir().unwrap();
        let parent = File::open(root.path()).unwrap();
        let stage = stage(root.path(), &payload(), write_resource).unwrap();
        let target = root.path().join("SAVEGAME.007");
        let error = atomic_publish(&parent, stage.path(), &target, true).unwrap_err();
        let _error = discard(stage, error);
        assert!(!target.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
