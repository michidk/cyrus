//! Read-only installed original slot discovery; restoration requires native state mapping.
use anyhow::{Context, Result, ensure};
use cyrus_startup::{SaveSlot, SaveSlotSource};
use std::path::Path;

pub(super) fn list(install: &Path) -> Result<Vec<SaveSlot>> {
    let mut roots = std::fs::read_dir(install)
        .with_context(|| format!("original save install {}", install.display()))?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("SAVEGAME")
        })
        .collect::<Vec<_>>();
    ensure!(
        roots.len() <= 1,
        "original save install {}: duplicate SAVEGAME directories",
        install.display()
    );
    let Some(root) = roots.pop() else {
        return Ok(Vec::new());
    };
    ensure!(
        root.file_type()?.is_dir(),
        "original save root {}: expected a directory",
        root.path().display()
    );
    let mut slots = Vec::new();
    for entry in std::fs::read_dir(root.path())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
        let Some(number) = name.strip_prefix("SAVEGAME.") else {
            continue;
        };
        if number.len() != 3 || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let slot = number.parse::<u16>()?;
        ensure!(
            entry.file_type()?.is_dir(),
            "original save slot {}: expected a directory",
            entry.path().display()
        );
        let saved = rgpre::import::savegame::load_save_slot(entry.path())
            .with_context(|| format!("original save slot {slot} {}", entry.path().display()))?;
        slots.push(SaveSlot {
            source: SaveSlotSource::Original {
                world_id: saved.game.metadata.world_id,
            },
            slot,
            description: saved.game.metadata.description,
            revision: String::new(),
            thumbnail: None,
        });
    }
    slots.sort_by_key(|slot| slot.slot);
    ensure!(
        slots.windows(2).all(|pair| pair[0].slot != pair[1].slot),
        "original save root {}: duplicate slot identities",
        root.path().display()
    );
    Ok(slots)
}

/// Resolve a selected original slot without trusting host-supplied filesystem paths.
pub(super) fn load(
    install: &Path,
    slot: u16,
    world_id: u32,
) -> Result<rgpre::import::savegame::SaveSlot> {
    let roots = std::fs::read_dir(install)?.collect::<std::io::Result<Vec<_>>>()?;
    let mut matches = Vec::new();
    for root in roots {
        if !root
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("SAVEGAME")
        {
            continue;
        }
        ensure!(
            root.file_type()?.is_dir(),
            "original SAVEGAME root is not a directory"
        );
        for entry in std::fs::read_dir(root.path())? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&format!("SAVEGAME.{slot:03}"))
            {
                ensure!(
                    entry.file_type()?.is_dir(),
                    "original slot {slot} is not a directory"
                );
                matches.push(entry.path());
            }
        }
    }
    ensure!(
        matches.len() == 1,
        "original save slot {slot} (world {world_id}): expected one installed slot, found {}",
        matches.len()
    );
    let saved = rgpre::import::savegame::load_save_slot(&matches[0])
        .with_context(|| format!("original save slot {slot} {}", matches[0].display()))?;
    ensure!(
        saved.game.metadata.world_id == world_id,
        "original save slot {slot}: world changed since listing (expected {world_id}, found {})",
        saved.game.metadata.world_id
    );
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Install(std::path::PathBuf);
    impl Install {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cyrus-original-slots-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn slot(&self, number: u16, world: u32) -> Vec<u8> {
            let dir = self
                .0
                .join("savegame")
                .join(format!("SAVEGAME.{number:03}"));
            std::fs::create_dir_all(&dir).unwrap();
            let mut data = vec![0; 272];
            data[..4].copy_from_slice(b"1.31");
            data[12..20].copy_from_slice(b"SYNTHETI");
            data[140..144].copy_from_slice(&world.to_le_bytes());
            let bytes = rgpre::import::savegame::encode_savegame_file(&[
                rgpre::import::savegame::SaveSection {
                    name: *b"SVGM",
                    data,
                },
            ])
            .unwrap();
            std::fs::write(dir.join("SAVEGAME.SAV"), &bytes).unwrap();
            bytes
        }
    }
    impl Drop for Install {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn original_listing_preserves_identity_world_and_source_files() {
        let install = Install::new();
        assert!(list(&install.0).unwrap().is_empty());
        let before = install.slot(2, 15);
        install.slot(0, 1);
        let slots = list(&install.0).unwrap();
        assert_eq!(slots.iter().map(|s| s.slot).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(slots[1].source, SaveSlotSource::Original { world_id: 15 });
        assert_eq!(slots[1].description, "SYNTHETI");
        assert_eq!(
            std::fs::read(install.0.join("savegame/SAVEGAME.002/SAVEGAME.SAV")).unwrap(),
            before
        );
    }
    #[test]
    fn original_load_resolves_case_and_rejects_stale_world_identity_without_writes() {
        let install = Install::new();
        let before = install.slot(2, 15);
        let visited = install.slot(3, 1);
        let directory = install.0.join("savegame/SAVEGAME.002");
        std::fs::write(directory.join("START.TSG"), &visited).unwrap();
        std::fs::write(directory.join("LOGBOOK.TXT"), b"Synthetic journal\r\n").unwrap();
        let retained = load(&install.0, 2, 15).unwrap();
        assert_eq!(retained.game.metadata.world_id, 15);
        assert_eq!(retained.maps["START.TSG"].metadata.world_id, 1);
        assert_eq!(
            retained.logbook.as_deref(),
            Some(b"Synthetic journal\r\n".as_slice())
        );
        assert_eq!(std::fs::read(directory.join("START.TSG")).unwrap(), visited);
        let error = load(&install.0, 2, 1).err().unwrap().to_string();
        assert!(error.contains("slot 2") && error.contains("expected 1, found 15"));
        assert!(load(&install.0, 3, 15).is_err());
        assert_eq!(
            std::fs::read(install.0.join("savegame/SAVEGAME.002/SAVEGAME.SAV")).unwrap(),
            before
        );
    }

    #[test]
    fn malformed_original_slot_has_path_context() {
        let install = Install::new();
        install.slot(2, 15);
        std::fs::write(
            install.0.join("savegame/SAVEGAME.002/SAVEGAME.SAV"),
            b"invalid",
        )
        .unwrap();
        let error = format!("{:#}", list(&install.0).unwrap_err());
        assert!(error.contains("slot 2") && error.contains("SAVEGAME.002"));
    }
    #[test]
    fn installed_original_slots_are_read_only_metadata() {
        let explicit = std::env::var_os("REDGUARD_ORIGINAL_SAVE_INSTALL");
        let Some(path) = explicit
            .clone()
            .or_else(|| std::env::var_os("REDGUARD_DIR"))
        else {
            eprintln!("SKIP original save listing: no install was supplied");
            return;
        };
        let slots = list(Path::new(&path)).unwrap();
        if slots.is_empty() && explicit.is_none() {
            eprintln!("SKIP original save listing: install has no original slots");
            return;
        }
        assert!(
            !slots.is_empty(),
            "supplied save install has no original slots"
        );
        assert!(slots.iter().all(
            |slot| matches!(slot.source, SaveSlotSource::Original { .. })
                && slot.revision.is_empty()
        ));
        // Exercise the published decoder on the same retained slots. Decoding
        // opaque state does not establish a mapping into native VM continuations.
        let root = std::fs::read_dir(&path)
            .unwrap()
            .map(std::result::Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SAVEGAME")
            })
            .unwrap()
            .path();
        for entry in std::fs::read_dir(root)
            .unwrap()
            .map(std::result::Result::unwrap)
        {
            let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
            if !name.starts_with("SAVEGAME.") || !entry.file_type().unwrap().is_dir() {
                continue;
            }
            let saved = rgpre::import::savegame::load_save_slot(entry.path()).unwrap();
            for game in std::iter::once(&saved.game).chain(saved.maps.values()) {
                for section in &game.sections {
                    use rgpre::import::savegame::*;
                    match &section.name {
                        b"SMOB" => {
                            let objects = parse_save_objects(&section.data).unwrap();
                            assert_eq!(encode_save_objects(&objects).unwrap(), section.data);
                            for object in objects
                                .iter()
                                .filter(|object| matches!(object.object_id, -2 | -3))
                            {
                                assert!(object.script_owner_id().is_some());
                            }
                        }
                        b"SMCO" => assert_eq!(
                            encode_save_code_owners(
                                &parse_save_code_owners(&section.data).unwrap()
                            ),
                            section.data
                        ),
                        b"SMSH" => assert_eq!(
                            encode_save_shells(&parse_save_shells(&section.data).unwrap()).unwrap(),
                            section.data
                        ),
                        _ => {}
                    }
                }
            }
        }
    }
}
