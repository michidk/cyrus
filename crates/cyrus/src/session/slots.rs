//! Native slot envelopes: bounded metadata followed by an opaque session document.
use anyhow::{Context, Result, ensure};
use bevy::prelude::Resource;
use cyrus_control::SlotOperation;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAGIC: &[u8] = b"CYRUS-SLOT\n";
const HEADER_LIMIT: usize = 4096;
static NEXT_REVISION: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Resource)]
pub(crate) struct SlotStore {
    pub(super) root: PathBuf,
}

impl Default for SlotStore {
    fn default() -> Self {
        Self {
            root: cyrus_config::config_dir().join("slots"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SlotMetadata {
    pub slot: u16,
    pub description: String,
    pub modified_ms: u64,
    pub revision: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    slot: u16,
    description: String,
    revision: String,
    document_bytes: u64,
}

pub(super) struct SavePlan {
    pub slot: u16,
    pub description: String,
    revision: Option<String>,
}

fn description(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|ch| matches!(ch, 11 | 13 | 32..=126)),
        "description must contain 1..64 original input characters"
    );
    Ok(())
}

impl SlotStore {
    fn writer_lock(&self) -> Result<File> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join(".writer-lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("opening slot writer lock {}", path.display()))?;
        file.lock()
            .with_context(|| format!("locking save store {}", self.root.display()))?;
        Ok(file)
    }

    fn path(&self, slot: u16) -> Result<PathBuf> {
        ensure!(slot <= 299, "save slot {slot} outside 0..299");
        Ok(self.root.join(format!("{slot:03}.cyrus")))
    }

    fn open(&self, slot: u16) -> Result<Option<(File, SlotMetadata, u64)>> {
        let path = self.path(slot)?;
        (|| -> Result<_> {
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let mut magic = [0; MAGIC.len()];
            file.read_exact(&mut magic)?;
            ensure!(magic == MAGIC, "slot format unsupported");
            let mut size = [0; 4];
            file.read_exact(&mut size)?;
            let size = u32::from_le_bytes(size) as usize;
            ensure!(
                (1..=HEADER_LIMIT).contains(&size),
                "invalid slot header length {size}"
            );
            let mut bytes = vec![0; size];
            file.read_exact(&mut bytes)?;
            let header: Header = serde_json::from_slice(&bytes)?;
            ensure!(
                header.version == 1 && header.slot == slot && !header.revision.is_empty(),
                "invalid slot version/identity/revision"
            );
            description(&header.description)?;
            let metadata = file.metadata()?;
            ensure!(
                header.document_bytes > 0
                    && metadata.len()
                        == ((MAGIC.len() + 4 + size) as u64)
                            .checked_add(header.document_bytes)
                            .context("slot document length overflow")?,
                "slot document length differs from its header"
            );
            let modified_ms =
                u64::try_from(metadata.modified()?.duration_since(UNIX_EPOCH)?.as_millis())?;
            Ok(Some((
                file,
                SlotMetadata {
                    slot,
                    description: header.description,
                    modified_ms,
                    revision: header.revision,
                },
                header.document_bytes,
            )))
        })()
        .with_context(|| format!("save slot {slot} {}", path.display()))
    }

    pub(super) fn metadata(&self, slot: u16, revision: Option<&str>) -> Result<SlotMetadata> {
        let (_, metadata, _) = self
            .open(slot)?
            .with_context(|| format!("save slot {slot} absent"))?;
        ensure!(
            revision.is_none_or(|revision| revision == metadata.revision),
            "save slot {slot} changed; refresh the save list"
        );
        Ok(metadata)
    }

    pub(super) fn list(&self) -> Result<Vec<SlotMetadata>> {
        let mut slots = Vec::new();
        for slot in 0..=299 {
            if let Some((_, metadata, _)) = self.open(slot)? {
                slots.push(metadata);
            }
        }
        slots.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(a.slot.cmp(&b.slot)));
        Ok(slots)
    }

    pub(super) fn plan(&self, operation: SlotOperation) -> Result<SavePlan> {
        match operation {
            SlotOperation::Create { description: value } => {
                description(&value)?;
                let slot = (2..=299)
                    .find_map(|slot| match self.open(slot) {
                        Ok(None) => Some(Ok(slot)),
                        Ok(Some(_)) => None,
                        Err(error) => Some(Err(error)),
                    })
                    .context("no free manual save slots")??;
                Ok(SavePlan {
                    slot,
                    description: value,
                    revision: None,
                })
            }
            SlotOperation::Overwrite {
                slot,
                revision,
                description: value,
            } => {
                ensure!(slot != 0, "quick-save slot cannot be manually overwritten");
                description(&value)?;
                self.metadata(slot, Some(&revision))?;
                Ok(SavePlan {
                    slot,
                    description: value,
                    revision: Some(revision),
                })
            }
            _ => anyhow::bail!("slot operation does not prepare a save"),
        }
    }

    pub(super) fn publish(&self, plan: SavePlan, document: &[u8]) -> Result<SlotMetadata> {
        let _lock = self.writer_lock()?;
        let path = self.path(plan.slot)?;
        (|| -> Result<_> {
            ensure!(!document.is_empty(), "empty session document");
            match &plan.revision {
                Some(revision) => {
                    self.metadata(plan.slot, Some(revision))?;
                }
                None => ensure!(
                    self.open(plan.slot)?.is_none(),
                    "save slot {} became occupied",
                    plan.slot
                ),
            }
            let revision = format!(
                "{:x}-{:x}-{:x}",
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
                std::process::id(),
                NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
            );
            let header = serde_json::to_vec(&Header {
                version: 1,
                slot: plan.slot,
                description: plan.description,
                revision,
                document_bytes: document.len() as u64,
            })?;
            ensure!(
                header.len() <= HEADER_LIMIT,
                "slot header exceeds its format limit"
            );
            let mut bytes = Vec::with_capacity(MAGIC.len() + 4 + header.len() + document.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&(header.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(document);
            cyrus_config::write(&path, &bytes)?;
            self.metadata(plan.slot, None)
        })()
        .with_context(|| format!("publishing save slot {} {}", plan.slot, path.display()))
    }

    pub(super) fn read(
        &self,
        slot: u16,
        revision: Option<&str>,
    ) -> Result<(Vec<u8>, SlotMetadata)> {
        let (mut file, metadata, length) = self
            .open(slot)?
            .with_context(|| format!("save slot {slot} absent"))?;
        ensure!(
            revision.is_none_or(|revision| revision == metadata.revision),
            "save slot {slot} changed; refresh the save list"
        );
        let mut document = Vec::new();
        file.read_to_end(&mut document)
            .with_context(|| format!("reading save slot {slot} document"))?;
        ensure!(
            document.len() as u64 == length,
            "save slot {slot} document changed while reading"
        );
        Ok((document, metadata))
    }

    pub(super) fn delete(&self, slot: u16, revision: &str) -> Result<()> {
        let _lock = self.writer_lock()?;
        self.metadata(slot, Some(revision))?;
        let path = self.path(slot)?;
        std::fs::remove_file(&path)
            .with_context(|| format!("deleting save slot {slot} {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Store(SlotStore);
    impl Store {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cyrus-native-slots-{}-{}",
                std::process::id(),
                NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(SlotStore { root })
        }
        fn create(&self, value: &str, bytes: &[u8]) -> SlotMetadata {
            self.0
                .publish(
                    self.0
                        .plan(SlotOperation::Create {
                            description: value.into(),
                        })
                        .unwrap(),
                    bytes,
                )
                .unwrap()
        }
    }
    impl Drop for Store {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0.root).unwrap();
        }
    }

    #[test]
    fn first_free_slot_and_stale_overwrite_keep_accepted_document() {
        let store = Store::new();
        let first = store.create("Mixed case\u{b}description\r", b"first");
        let second = store.create("Second", b"second");
        assert_eq!((first.slot, second.slot), (2, 3));
        let plan = store
            .0
            .plan(SlotOperation::Overwrite {
                slot: 2,
                revision: first.revision.clone(),
                description: "Replaced".into(),
            })
            .unwrap();
        let replaced = store.0.publish(plan, b"replacement").unwrap();
        assert_ne!(first.revision, replaced.revision);
        assert!(
            store
                .0
                .plan(SlotOperation::Overwrite {
                    slot: 2,
                    revision: first.revision.clone(),
                    description: "Stale".into()
                })
                .is_err()
        );
        assert_eq!(
            store.0.read(2, Some(&replaced.revision)).unwrap().0,
            b"replacement"
        );
        assert!(store.0.delete(2, &first.revision).is_err());
        store.0.delete(2, &replaced.revision).unwrap();
        assert_eq!(store.create("Reuses free slot", b"third").slot, 2);
        assert!(
            store
                .0
                .plan(SlotOperation::Overwrite {
                    slot: 0,
                    revision: "any".into(),
                    description: "Forbidden".into()
                })
                .is_err()
        );
    }

    #[test]
    fn list_uses_file_time_then_slot_and_does_not_decode_payload() {
        let store = Store::new();
        store.create("Older", &[255]);
        store.create("Newer", b"newer");
        store
            .0
            .publish(
                SavePlan {
                    slot: 0,
                    description: "Quick Save Game".into(),
                    revision: None,
                },
                b"quick",
            )
            .unwrap();
        for (slot, seconds) in [(2, 10), (3, 20), (0, 20)] {
            File::options()
                .write(true)
                .open(store.0.path(slot).unwrap())
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
                .unwrap();
        }
        assert_eq!(
            store
                .0
                .list()
                .unwrap()
                .iter()
                .map(|entry| entry.slot)
                .collect::<Vec<_>>(),
            vec![0, 3, 2]
        );
        assert_eq!(store.0.read(2, None).unwrap().0, [255]);
    }

    #[test]
    fn malformed_or_truncated_slots_are_contextual_and_never_overwritten() {
        let store = Store::new();
        let path = store.0.path(2).unwrap();
        std::fs::write(&path, b"malformed").unwrap();
        let error = store
            .0
            .plan(SlotOperation::Create {
                description: "New".into(),
            })
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), b"malformed");
        std::fs::remove_file(&path).unwrap();
        store.create("Valid", b"payload");
        let file = File::options().write(true).open(&path).unwrap();
        file.set_len(file.metadata().unwrap().len() - 1).unwrap();
        assert!(
            store
                .0
                .list()
                .unwrap_err()
                .to_string()
                .contains(path.to_str().unwrap())
        );
        assert!(store.0.read(2, None).is_err());
    }
}
