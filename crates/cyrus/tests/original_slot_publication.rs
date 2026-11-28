//! The filesystem boundary accepts only completely validated slot payloads.
use anyhow::{Result, bail};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use cyrus::original_slot::ExistingSlot;
use cyrus::original_slot::{
    CompleteSlot, CompleteSlotValidator, FileLayout, Replacement, SlotLayout, publish,
};
use rgpre::import::savegame::{SaveGame, SaveSection, SaveSlot, parse_savegame_file};
use std::{collections::BTreeMap, fs, path::Path};

// Synthetic records exercise publication without pretending native inverse mappings exist.
struct Synthetic;

impl CompleteSlotValidator for Synthetic {
    fn layout(&self) -> Result<SlotLayout> {
        Ok(SlotLayout {
            files: BTreeMap::from([
                ("SAVEGAME.SAV".into(), layout(30)),
                ("START.TSG".into(), layout(1)),
            ]),
            logbook: true,
        })
    }

    fn validate_record(&self, _file: &str, _world: u32, record: &SaveSection) -> Result<()> {
        match record.name {
            name if name == *b"SVGM" => Ok(()),
            name if name == *b"SVRG" && record.data == [4, 0, 0, 0] => Ok(()),
            _ => bail!("unsupported synthetic record contents"),
        }
    }

    fn validate_slot(&self, slot: &SaveSlot) -> Result<()> {
        anyhow::ensure!(
            slot.logbook.as_deref() == Some(b"journal\r\n"),
            "journal unsupported"
        );
        Ok(())
    }
}

fn layout(world: u32) -> FileLayout {
    FileLayout {
        world,
        records: vec![*b"SVGM", *b"SVRG"],
    }
}

fn game(world: u32) -> SaveGame {
    let mut metadata = vec![0; 272];
    metadata[..4].copy_from_slice(b"1.31");
    metadata[140..144].copy_from_slice(&world.to_le_bytes());
    let bytes = rgpre::import::savegame::encode_savegame_file(&[
        SaveSection {
            name: *b"SVGM",
            data: metadata,
        },
        SaveSection {
            name: *b"SVRG",
            data: vec![4, 0, 0, 0],
        },
    ])
    .unwrap();
    parse_savegame_file(&bytes).unwrap()
}

fn slot() -> SaveSlot {
    SaveSlot {
        game: game(30),
        maps: BTreeMap::from([("START.TSG".into(), game(1))]),
        logbook: Some(b"journal\r\n".to_vec()),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn confirmation(root: &Path, slot: u16) -> Replacement {
    Replacement::ConfirmedByPlayer(ExistingSlot::inspect(root, slot).unwrap())
}

fn names(root: &Path) -> Vec<String> {
    let mut entries: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    entries.sort();
    entries
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn complete_slot_is_published_as_one_reloadable_directory() {
    let root = tempfile::tempdir().unwrap();
    let original = slot();
    let payload = CompleteSlot::validate(&original, &Synthetic).unwrap();
    let result = publish(root.path(), 7, &payload, Replacement::Forbid).unwrap();
    assert_eq!(result.path, root.path().join("SAVEGAME.007"));
    assert!(result.cleanup_error.is_none());
    assert_eq!(
        rgpre::import::savegame::load_save_slot(&result.path).unwrap(),
        original
    );
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}

#[test]
fn invalid_records_reject_before_even_creating_a_save_root() {
    let parent = tempfile::tempdir().unwrap();
    let absent = parent.path().join("SAVEGAME");
    for (file, world, candidate) in [
        ("SAVEGAME.SAV", 30, {
            let mut s = slot();
            s.game.sections.pop();
            s
        }),
        ("START.TSG", 1, {
            let mut s = slot();
            s.maps.get_mut("START.TSG").unwrap().sections[1].data[0] = 9;
            s
        }),
    ] {
        let error = format!(
            "{:#}",
            CompleteSlot::validate(&candidate, &Synthetic).unwrap_err()
        );
        assert!(error.contains(file), "{error}");
        assert!(error.contains(&format!("world {world}")), "{error}");
        assert!(error.contains("SVRG"), "{error}");
        assert!(!absent.exists());
        assert!(names(parent.path()).is_empty());
    }
}

#[test]
fn failed_complete_validation_preserves_existing_slot_and_native_saves() {
    let install = tempfile::tempdir().unwrap();
    let root = install.path().join("SAVEGAME");
    let existing = root.join("SAVEGAME.007");
    let native = install.path().join("native-config/slots");
    fs::create_dir_all(&existing).unwrap();
    fs::create_dir_all(&native).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"old original save").unwrap();
    fs::write(native.join("007.cyrus"), b"native fallback").unwrap();
    let mut candidate = slot();
    candidate.maps.get_mut("START.TSG").unwrap().sections[1].data[0] = 9;
    let result = CompleteSlot::validate(&candidate, &Synthetic)
        .and_then(|payload| publish(&root, 7, &payload, Replacement::Forbid));
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("START.TSG world 1"), "{error}");
    assert!(error.contains("SVRG"), "{error}");
    assert_eq!(
        fs::read(existing.join("SAVEGAME.SAV")).unwrap(),
        b"old original save"
    );
    assert_eq!(
        fs::read(native.join("007.cyrus")).unwrap(),
        b"native fallback"
    );
    assert_eq!(names(&root), ["SAVEGAME.007"]);
    assert_eq!(names(&native), ["007.cyrus"]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn validated_bytes_cannot_change_with_the_callers_mutable_session() {
    let root = tempfile::tempdir().unwrap();
    let mut candidate = slot();
    let expected = candidate.clone();
    let payload = CompleteSlot::validate(&candidate, &Synthetic).unwrap();
    candidate.maps.clear();
    candidate.game.sections.clear();
    candidate.logbook = None;
    let result = publish(root.path(), 7, &payload, Replacement::Forbid).unwrap();
    assert_eq!(
        rgpre::import::savegame::load_save_slot(result.path).unwrap(),
        expected
    );
}

#[test]
fn missing_save_root_is_not_created_by_publication() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("SAVEGAME");
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    assert!(publish(&root, 7, &payload, Replacement::Forbid).is_err());
    assert!(!root.exists());
    assert!(names(parent.path()).is_empty());
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn replacing_an_existing_slot_requires_explicit_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("savegame.007");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"old save").unwrap();
    fs::write(existing.join("NOTES.TXT"), b"keep on rejection").unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    let error = publish(root.path(), 7, &payload, Replacement::Forbid).unwrap_err();
    assert!(format!("{error:#}").contains("confirmation"));
    assert_eq!(
        fs::read(existing.join("SAVEGAME.SAV")).unwrap(),
        b"old save"
    );
    assert_eq!(
        fs::read(existing.join("NOTES.TXT")).unwrap(),
        b"keep on rejection"
    );
    assert_eq!(names(root.path()), ["savegame.007"]);
    let result = publish(root.path(), 7, &payload, confirmation(root.path(), 7)).unwrap();
    assert_eq!(result.path, existing);
    assert_eq!(
        rgpre::import::savegame::load_save_slot(&result.path).unwrap(),
        slot()
    );
    assert_eq!(names(root.path()), ["savegame.007"]);
    assert_eq!(
        names(&result.path),
        ["LOGBOOK.TXT", "SAVEGAME.SAV", "START.TSG"]
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn stale_and_wrong_slot_confirmations_reject_without_staging() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("SAVEGAME.007");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"confirmed bytes").unwrap();
    let token = ExistingSlot::inspect(root.path(), 7).unwrap();
    assert_eq!(token.path(), existing);
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"intervening save").unwrap();
    let error = publish(
        root.path(),
        7,
        &payload,
        Replacement::ConfirmedByPlayer(token.clone()),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("changed since player confirmation"));
    assert!(
        publish(
            root.path(),
            8,
            &payload,
            Replacement::ConfirmedByPlayer(token.clone())
        )
        .is_err()
    );
    let other = tempfile::tempdir().unwrap();
    assert!(
        publish(
            other.path(),
            7,
            &payload,
            Replacement::ConfirmedByPlayer(token)
        )
        .is_err()
    );
    assert!(names(other.path()).is_empty());
    assert_eq!(
        fs::read(existing.join("SAVEGAME.SAV")).unwrap(),
        b"intervening save"
    );
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn replacing_the_directory_invalidates_consent_even_with_identical_contents() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("SAVEGAME.007");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"same bytes").unwrap();
    let token = ExistingSlot::inspect(root.path(), 7).unwrap();
    fs::rename(&existing, root.path().join("previous")).unwrap();
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"same bytes").unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    assert!(
        publish(
            root.path(),
            7,
            &payload,
            Replacement::ConfirmedByPlayer(token)
        )
        .is_err()
    );
    assert_eq!(
        fs::read(existing.join("SAVEGAME.SAV")).unwrap(),
        b"same bytes"
    );
    assert_eq!(names(root.path()), ["SAVEGAME.007", "previous"]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn changing_an_unknown_resource_also_invalidates_consent() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("SAVEGAME.007");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"old save").unwrap();
    fs::write(existing.join("notes.txt"), b"old notes").unwrap();
    let token = ExistingSlot::inspect(root.path(), 7).unwrap();
    fs::write(existing.join("notes.txt"), b"new notes").unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    assert!(
        publish(
            root.path(),
            7,
            &payload,
            Replacement::ConfirmedByPlayer(token)
        )
        .is_err()
    );
    assert_eq!(fs::read(existing.join("notes.txt")).unwrap(), b"new notes");
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn concurrent_new_saves_do_not_replace_an_unconfirmed_slot() {
    let root = tempfile::tempdir().unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    let results = std::thread::scope(|scope| {
        (0..4)
            .map(|_| scope.spawn(|| publish(root.path(), 7, &payload, Replacement::Forbid)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    for result in results.into_iter().filter_map(Result::err) {
        assert!(format!("{result:#}").contains("confirmation"));
    }
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
    assert_eq!(
        rgpre::import::savegame::load_save_slot(root.path().join("SAVEGAME.007")).unwrap(),
        slot()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn post_commit_cleanup_failure_is_reported_as_success_with_cleanup_context() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("SAVEGAME.007");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"old save").unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o555)).unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    let result = publish(root.path(), 7, &payload, confirmation(root.path(), 7)).unwrap();
    assert_eq!(
        rgpre::import::savegame::load_save_slot(&result.path).unwrap(),
        slot()
    );
    assert!(
        result
            .cleanup_error
            .as_ref()
            .unwrap()
            .contains(".cyrus-slot-")
    );
    let pending = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(".cyrus-slot-")
        })
        .unwrap();
    assert_eq!(fs::read(pending.join("SAVEGAME.SAV")).unwrap(), b"old save");
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_dir_all(&pending).unwrap();
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_atomic_platform_rejects_before_staging() {
    let root = tempfile::tempdir().unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    let error = publish(root.path(), 7, &payload, Replacement::Forbid).unwrap_err();
    assert!(format!("{error:#}").contains("unsupported on this platform"));
    assert!(names(root.path()).is_empty());
}

#[test]
fn incomplete_file_census_and_inconsistent_metadata_reject() {
    for candidate in [
        {
            let mut s = slot();
            s.maps.clear();
            s
        },
        {
            let mut s = slot();
            s.logbook = None;
            s
        },
        {
            let mut s = slot();
            s.maps.insert("EXTRA.TSG".into(), game(2));
            s
        },
        {
            let mut s = slot();
            s.maps.get_mut("START.TSG").unwrap().metadata.world_id = 2;
            s
        },
        {
            let mut s = slot();
            s.game.sections[1].name = *b"XXXX";
            s
        },
        {
            let mut s = slot();
            s.game.sections.push(s.game.sections[1].clone());
            s
        },
        {
            let mut s = slot();
            s.logbook = Some(b"unsupported".to_vec());
            s
        },
    ] {
        assert!(CompleteSlot::validate(&candidate, &Synthetic).is_err());
    }
}

#[test]
fn map_paths_cannot_escape_or_alias_slot_resources() {
    for name in [
        "../BAD.TSG",
        "a/b.TSG",
        "a\\b.TSG",
        "/BAD.TSG",
        "START.tsg",
        "START.TSG.",
        "BAD:NAME.TSG",
        "CON.TSG",
        "SAVEGAME.SAV",
        "LOGBOOK.TXT",
    ] {
        let mut candidate = slot();
        candidate.maps.insert(name.into(), game(2));
        assert!(
            CompleteSlot::validate(&candidate, &Synthetic).is_err(),
            "{name}"
        );
    }
}

#[test]
fn invalid_destination_preserves_the_entire_existing_store() {
    let root = tempfile::tempdir().unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    for number in [1000, u16::MAX] {
        assert!(publish(root.path(), number, &payload, Replacement::Forbid).is_err());
        assert!(names(root.path()).is_empty());
    }
    fs::write(root.path().join("SAVEGAME.007"), b"not a directory").unwrap();
    assert!(publish(root.path(), 7, &payload, Replacement::Forbid).is_err());
    assert_eq!(
        fs::read(root.path().join("SAVEGAME.007")).unwrap(),
        b"not a directory"
    );
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}

#[cfg(unix)]
#[test]
fn symlink_destination_or_save_root_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;
    let parent = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("SAVEGAME.SAV"), b"foreign save").unwrap();
    symlink(other.path(), root.path().join("SAVEGAME.007")).unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    assert!(publish(root.path(), 7, &payload, Replacement::Forbid).is_err());
    symlink(root.path(), parent.path().join("SAVEGAME")).unwrap();
    assert!(
        publish(
            &parent.path().join("SAVEGAME"),
            8,
            &payload,
            Replacement::Forbid
        )
        .is_err()
    );
    assert_eq!(
        fs::read(other.path().join("SAVEGAME.SAV")).unwrap(),
        b"foreign save"
    );
    assert_eq!(names(other.path()), ["SAVEGAME.SAV"]);
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn case_ambiguous_slots_reject_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("SAVEGAME.007")).unwrap();
    // Case-insensitive filesystems cannot contain this ambiguity.
    if fs::create_dir(root.path().join("savegame.007")).is_err() {
        return;
    }
    let before = names(root.path());
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    assert!(publish(root.path(), 7, &payload, Replacement::Forbid).is_err());
    assert_eq!(names(root.path()), before);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn failed_staging_leaves_no_temporary_directories_or_changed_slot() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("SAVEGAME.007");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("SAVEGAME.SAV"), b"old save").unwrap();
    let payload = CompleteSlot::validate(&slot(), &Synthetic).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = publish(root.path(), 7, &payload, confirmation(root.path(), 7));
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err(), "read-only store must prevent staging");
    assert_eq!(
        fs::read(existing.join("SAVEGAME.SAV")).unwrap(),
        b"old save"
    );
    assert_eq!(names(root.path()), ["SAVEGAME.007"]);
}
