//! Application export decisions, using only synthetic complete payloads.
use anyhow::{Result, bail};
use cyrus::original_slot::{
    CompleteSlot, CompleteSlotValidator, FileLayout, SlotLayout,
    request::{ExportDecision, ExportOutcome, ExportPrompt, ExportRequest},
};
use rgpre::import::savegame::{SaveSection, SaveSlot, parse_savegame_file};
use std::{cell::Cell, collections::BTreeMap, fs, path::Path};

struct Synthetic;
impl CompleteSlotValidator for Synthetic {
    fn layout(&self) -> Result<SlotLayout> {
        Ok(SlotLayout {
            files: BTreeMap::from([(
                "SAVEGAME.SAV".into(),
                FileLayout {
                    world: 30,
                    records: vec![*b"SVGM", *b"SVRG"],
                },
            )]),
            logbook: false,
        })
    }
    fn validate_record(&self, _file: &str, _world: u32, section: &SaveSection) -> Result<()> {
        anyhow::ensure!(
            section.name == *b"SVGM" || (section.name == *b"SVRG" && section.data == [4, 0, 0, 0]),
            "unsupported synthetic contents"
        );
        Ok(())
    }
    fn validate_slot(&self, _slot: &SaveSlot) -> Result<()> {
        Ok(())
    }
}

fn payload() -> Result<CompleteSlot> {
    let mut metadata = vec![0; 272];
    metadata[..4].copy_from_slice(b"1.31");
    metadata[140..144].copy_from_slice(&30_u32.to_le_bytes());
    let bytes = rgpre::import::savegame::encode_savegame_file(&[
        SaveSection {
            name: *b"SVGM",
            data: metadata,
        },
        SaveSection {
            name: *b"SVRG",
            data: vec![4, 0, 0, 0],
        },
    ])?;
    CompleteSlot::validate(
        &SaveSlot {
            game: parse_savegame_file(&bytes)?,
            maps: BTreeMap::new(),
            logbook: None,
        },
        &Synthetic,
    )
}

fn entries(path: &Path) -> Vec<String> {
    let mut names = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn existing(root: &Path) -> std::path::PathBuf {
    let target = root.join("savegame.007");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("SAVEGAME.SAV"), b"existing slot").unwrap();
    target
}

#[test]
fn unsupported_payload_rejects_before_prompt_or_filesystem_access() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent-save-root");
    let error = ExportRequest::prepare(&absent, 7, || {
        bail!("SAVEGAME.SAV world 30 SVRG: unsupported source ownership")
    })
    .unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("SAVEGAME.007") && message.contains("absent-save-root"),
        "{message}"
    );
    assert!(
        message.contains("world 30 SVRG: unsupported source ownership"),
        "{message}"
    );
    assert!(!message.contains("examining SAVEGAME root"), "{message}");
    assert!(entries(root.path()).is_empty());
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn new_export_waits_for_the_matching_save_decision() {
    let root = tempfile::tempdir().unwrap();
    let request = ExportRequest::prepare(root.path(), 7, payload).unwrap();
    assert_eq!(
        request.prompt(),
        ExportPrompt::Create {
            slot: 7,
            path: &root.path().join("SAVEGAME.007")
        }
    );
    assert!(entries(root.path()).is_empty());
    let ExportOutcome::Published(result) = request.respond(ExportDecision::SaveNew).unwrap() else {
        panic!("save cancelled")
    };
    assert_eq!(result.path, root.path().join("SAVEGAME.007"));
    assert_eq!(
        rgpre::import::savegame::load_save_slot(result.path)
            .unwrap()
            .game
            .metadata
            .world_id,
        30
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn explicit_replacement_uses_the_prepared_payload_once_and_preserves_native_saves() {
    let root = tempfile::tempdir().unwrap();
    let save_root = root.path().join("SAVEGAME");
    fs::create_dir(&save_root).unwrap();
    let native = root.path().join("native-slots");
    fs::write(&native, b"native fallback").unwrap();
    let target = existing(&save_root);
    let calls = Cell::new(0);
    let request = ExportRequest::prepare(&save_root, 7, || {
        calls.set(calls.get() + 1);
        payload()
    })
    .unwrap();
    assert_eq!(
        request.prompt(),
        ExportPrompt::Replace {
            slot: 7,
            path: &target
        }
    );
    assert_eq!(
        fs::read(target.join("SAVEGAME.SAV")).unwrap(),
        b"existing slot"
    );
    assert_eq!(entries(&save_root), vec!["savegame.007"]);
    let ExportOutcome::Published(result) =
        request.respond(ExportDecision::ConfirmReplacement).unwrap()
    else {
        panic!("replacement cancelled")
    };
    assert_eq!(result.path, target);
    assert_eq!(calls.get(), 1);
    assert_eq!(fs::read(native).unwrap(), b"native fallback");
    assert!(result.cleanup_error.is_none() && result.sync_error.is_none());
}

#[test]
fn cancellation_does_not_publish_new_or_existing_slots() {
    for occupied in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let target = occupied.then(|| existing(root.path()));
        let before = entries(root.path());
        let request = ExportRequest::prepare(root.path(), 7, payload).unwrap();
        assert!(matches!(
            request.respond(ExportDecision::Cancel).unwrap(),
            ExportOutcome::Cancelled
        ));
        assert_eq!(entries(root.path()), before);
        if let Some(target) = target {
            assert_eq!(
                fs::read(target.join("SAVEGAME.SAV")).unwrap(),
                b"existing slot"
            );
        }
    }
}

#[test]
fn a_response_for_the_wrong_prompt_never_writes() {
    for occupied in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let target = occupied.then(|| existing(root.path()));
        let before = entries(root.path());
        let request = ExportRequest::prepare(root.path(), 7, payload).unwrap();
        let decision = if occupied {
            ExportDecision::SaveNew
        } else {
            ExportDecision::ConfirmReplacement
        };
        let error = format!("{:#}", request.respond(decision).unwrap_err());
        assert!(
            error.contains("SAVEGAME.007") && error.contains("decision"),
            "{error}"
        );
        assert_eq!(entries(root.path()), before);
        if let Some(target) = target {
            assert_eq!(
                fs::read(target.join("SAVEGAME.SAV")).unwrap(),
                b"existing slot"
            );
        }
    }
}

#[test]
fn a_slot_created_after_preparation_requires_a_new_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let request = ExportRequest::prepare(root.path(), 7, payload).unwrap();
    let target = existing(root.path());
    assert!(request.respond(ExportDecision::SaveNew).is_err());
    assert_eq!(
        fs::read(target.join("SAVEGAME.SAV")).unwrap(),
        b"existing slot"
    );
    assert_eq!(entries(root.path()), vec!["savegame.007"]);
}

#[test]
fn changed_slot_rejects_the_retained_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let target = existing(root.path());
    let request = ExportRequest::prepare(root.path(), 7, payload).unwrap();
    fs::write(target.join("SAVEGAME.SAV"), b"newer save").unwrap();
    let error = format!(
        "{:#}",
        request
            .respond(ExportDecision::ConfirmReplacement)
            .unwrap_err()
    );
    assert!(error.contains("SAVEGAME.007"), "{error}");
    assert_eq!(
        fs::read(target.join("SAVEGAME.SAV")).unwrap(),
        b"newer save"
    );
    assert_eq!(entries(root.path()), vec!["savegame.007"]);
}
