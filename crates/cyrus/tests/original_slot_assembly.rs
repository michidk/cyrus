//! Saved-image assembly owns only its supported group; other mappings stay mandatory.
use anyhow::{Result, ensure};
use cyrus::original_slot::{
    CompleteSlotValidator, FileLayout, SlotLayout,
    assembly::{SavedImages, assemble_with_images},
    request::{ExportDecision, ExportOutcome, ExportRequest},
};
use rgpre::import::savegame::{
    SaveGame, SaveImageKind, SaveSection, SaveSlot, encode_savegame_file, load_save_slot,
    parse_savegame_file,
};
use std::{cell::Cell, collections::BTreeMap, fs};

// A test-only witness for unchanged residual records, never a native inverse mapper.
struct RetainedContract {
    slot: SaveSlot,
    calls: Cell<u32>,
    reject: bool,
}
impl CompleteSlotValidator for RetainedContract {
    fn layout(&self) -> Result<SlotLayout> {
        self.calls.set(self.calls.get() + 1);
        Ok(SlotLayout {
            files: std::iter::once(("SAVEGAME.SAV".to_owned(), &self.slot.game))
                .chain(
                    self.slot
                        .maps
                        .iter()
                        .map(|(name, game)| (name.clone(), game)),
                )
                .map(|(name, game)| {
                    (
                        name,
                        FileLayout {
                            world: game.metadata.world_id,
                            records: game.sections.iter().map(|section| section.name).collect(),
                        },
                    )
                })
                .collect(),
            logbook: self.slot.logbook.is_some(),
        })
    }
    fn validate_record(&self, file: &str, _world: u32, record: &SaveSection) -> Result<()> {
        ensure!(!self.reject, "residual inverse mapping unsupported");
        ensure!(
            !matches!(&record.name, b"THMB" | b"SCR3"),
            "image group leaked to residual validator"
        );
        let game = if file == "SAVEGAME.SAV" {
            &self.slot.game
        } else {
            &self.slot.maps[file]
        };
        ensure!(game.sections.contains(record), "residual record changed");
        Ok(())
    }
    fn validate_slot(&self, slot: &SaveSlot) -> Result<()> {
        ensure!(
            slot.maps == self.slot.maps && slot.logbook == self.slot.logbook,
            "retained resources changed"
        );
        Ok(())
    }
}
fn contract(slot: &SaveSlot) -> RetainedContract {
    RetainedContract {
        slot: slot.clone(),
        calls: Cell::new(0),
        reject: false,
    }
}
fn game(world: u32, images: bool) -> SaveGame {
    let mut metadata = vec![0; 272];
    metadata[..4].copy_from_slice(b"1.31");
    metadata[140..144].copy_from_slice(&world.to_le_bytes());
    let sections = vec![
        SaveSection {
            name: *b"SVGM",
            data: metadata,
        },
        SaveSection {
            name: *b"THMB",
            data: if images {
                vec![3; SaveImageKind::Thumbnail.width() * SaveImageKind::Thumbnail.height() * 2]
            } else {
                vec![]
            },
        },
        SaveSection {
            name: *b"SCR3",
            data: if images {
                vec![5; SaveImageKind::Screen.width() * SaveImageKind::Screen.height() * 2]
            } else {
                vec![]
            },
        },
        SaveSection {
            name: *b"SVRG",
            data: vec![4, 0, 0, 0],
        },
    ];
    parse_savegame_file(&encode_savegame_file(&sections).unwrap()).unwrap()
}
fn slot() -> SaveSlot {
    SaveSlot {
        game: game(30, true),
        maps: BTreeMap::from([("START.TSG".into(), game(0, false))]),
        logbook: Some(b"9,Synthetic journal\r\n".to_vec()),
    }
}
fn changed_images(game: &SaveGame) -> SavedImages {
    let mut capture = game.clone();
    for section in &mut capture.sections {
        if matches!(&section.name, b"THMB" | b"SCR3") {
            section.data.fill(0x6b);
        }
    }
    SavedImages::decode(&capture).unwrap()
}
fn publish_assembled(slot: SaveSlot, validator: &impl CompleteSlotValidator) -> SaveSlot {
    let root = tempfile::tempdir().unwrap();
    let images = changed_images(&slot.game);
    let request = ExportRequest::prepare(root.path(), 3, || {
        assemble_with_images(slot, images, validator)
    })
    .unwrap();
    assert!(fs::read_dir(root.path()).unwrap().next().is_none());
    let ExportOutcome::Published(result) = request.respond(ExportDecision::SaveNew).unwrap() else {
        panic!("cancelled")
    };
    load_save_slot(result.path).unwrap()
}
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn typed_images_fill_only_the_image_group_and_preserve_snapshot_and_journal_bytes() {
    let expected = slot();
    let validator = contract(&expected);
    let mut input = expected.clone();
    input
        .game
        .sections
        .retain(|s| !matches!(&s.name, b"THMB" | b"SCR3"));
    let images = changed_images(&expected.game);
    let root = tempfile::tempdir().unwrap();
    let request = ExportRequest::prepare(root.path(), 3, || {
        assemble_with_images(input, images, &validator)
    })
    .unwrap();
    let ExportOutcome::Published(result) = request.respond(ExportDecision::SaveNew).unwrap() else {
        panic!("cancelled")
    };
    let output = load_save_slot(result.path).unwrap();
    assert_eq!(validator.calls.get(), 1);
    assert_eq!(output.maps, expected.maps);
    assert_eq!(output.logbook, expected.logbook);
    assert_eq!(
        output
            .game
            .sections
            .iter()
            .map(|s| s.name)
            .collect::<Vec<_>>(),
        expected
            .game
            .sections
            .iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
    );
    for section in &output.game.sections {
        if matches!(&section.name, b"THMB" | b"SCR3") {
            assert!(section.data.iter().all(|b| *b == 0x6b));
        } else {
            assert!(expected.game.sections.contains(section));
        }
    }
}
#[test]
fn malformed_missing_duplicate_and_empty_capture_images_reject() {
    for name in [*b"THMB", *b"SCR3"] {
        for mode in 0..4 {
            let mut capture = game(30, true);
            let i = capture
                .sections
                .iter()
                .position(|s| s.name == name)
                .unwrap();
            match mode {
                0 => {
                    capture.sections.remove(i);
                }
                1 => capture.sections[i].data.clear(),
                2 => {
                    capture.sections[i].data.pop();
                }
                _ => capture.sections.push(capture.sections[i].clone()),
            }
            let error = format!("{:#}", SavedImages::decode(&capture).unwrap_err());
            assert!(
                error.contains("30") && error.contains(std::str::from_utf8(&name).unwrap()),
                "{error}"
            );
        }
    }
}
#[test]
fn incomplete_or_unsupported_assembly_never_reaches_filesystem_publication() {
    let expected = slot();
    for mode in 0..7 {
        let mut input = expected.clone();
        let mut validator = contract(&expected);
        match mode {
            0 => {
                input.maps.clear();
            }
            1 => input.logbook = None,
            2 => {
                input.game.sections.pop();
            }
            3 => input.game.sections.push(input.game.sections[3].clone()),
            4 => {
                input.maps.insert("EXTRA.TSG".into(), game(2, false));
            }
            5 => validator.reject = true,
            _ => input.game.sections.push(SaveSection {
                name: *b"TEST",
                data: vec![],
            }),
        }
        let root = tempfile::tempdir().unwrap();
        let images = changed_images(&expected.game);
        let error = format!(
            "{:#}",
            ExportRequest::prepare(root.path(), 3, || assemble_with_images(
                input, images, &validator
            ))
            .unwrap_err()
        );
        assert!(error.contains("SAVEGAME.003"), "{error}");
        if mode == 5 {
            assert!(
                error.contains("residual inverse mapping unsupported"),
                "{error}"
            );
        }
        assert!(fs::read_dir(root.path()).unwrap().next().is_none());
    }
}
#[test]
fn a_complete_census_must_include_each_image_exactly_once() {
    let expected = slot();
    for name in [*b"THMB", *b"SCR3"] {
        for duplicate in [false, true] {
            let mut validator = contract(&expected);
            if duplicate {
                let image = validator
                    .slot
                    .game
                    .sections
                    .iter()
                    .find(|s| s.name == name)
                    .unwrap()
                    .clone();
                validator.slot.game.sections.push(image);
            } else {
                validator.slot.game.sections.retain(|s| s.name != name);
            }
            let error = format!(
                "{:#}",
                assemble_with_images(expected.clone(), changed_images(&expected.game), &validator)
                    .unwrap_err()
            );
            assert!(
                error.contains("census") && error.contains(std::str::from_utf8(&name).unwrap()),
                "{error}"
            );
        }
    }
}
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn an_absent_journal_is_not_synthesized() {
    let mut expected = slot();
    expected.logbook = None;
    let validator = contract(&expected);
    let output = publish_assembled(expected, &validator);
    assert!(output.logbook.is_none());
}
#[test]
fn images_cannot_be_rebound_to_another_world_or_admit_a_bad_visited_image() {
    let expected = slot();
    let validator = contract(&expected);
    let error = format!(
        "{:#}",
        assemble_with_images(expected.clone(), changed_images(&game(1, true)), &validator)
            .unwrap_err()
    );
    assert!(error.contains("30") && error.contains("1"), "{error}");
    let mut input = expected.clone();
    input.maps.get_mut("START.TSG").unwrap().sections[2]
        .data
        .push(7);
    let error = format!(
        "{:#}",
        assemble_with_images(input, changed_images(&expected.game), &validator).unwrap_err()
    );
    assert!(
        error.contains("START.TSG") && error.contains("SCR3") && error.contains('0'),
        "{error}"
    );
}
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn assembled_payload_still_requires_explicit_replacement_and_preserves_native_fallback() {
    let expected = slot();
    let validator = contract(&expected);
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("SAVEGAME.003")).unwrap();
    fs::write(root.path().join("SAVEGAME.003/OLD"), b"old").unwrap();
    fs::write(root.path().join("native-save"), b"native fallback").unwrap();
    for decision in [ExportDecision::SaveNew, ExportDecision::ConfirmReplacement] {
        let request = ExportRequest::prepare(root.path(), 3, || {
            assemble_with_images(expected.clone(), changed_images(&expected.game), &validator)
        })
        .unwrap();
        assert_eq!(
            fs::read(root.path().join("SAVEGAME.003/OLD")).unwrap(),
            b"old"
        );
        let result = request.respond(decision);
        if decision == ExportDecision::SaveNew {
            assert!(result.is_err());
        } else {
            assert!(matches!(result.unwrap(), ExportOutcome::Published(_)));
        }
    }
    assert_eq!(
        fs::read(root.path().join("native-save")).unwrap(),
        b"native fallback"
    );
}
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn installed_saved_images_assemble_without_changing_other_records_or_resources() {
    let Some(install) = std::env::var_os("REDGUARD_ORIGINAL_SAVE_INSTALL") else {
        eprintln!("SKIP installed assembler: REDGUARD_ORIGINAL_SAVE_INSTALL required");
        return;
    };
    let root = std::path::Path::new(&install).join("SAVEGAME");
    let slots = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|e| e.file_type().unwrap().is_dir())
        .collect::<Vec<_>>();
    assert!(!slots.is_empty(), "explicit save fixture contains no slots");
    let mut worlds = Vec::new();
    for entry in slots {
        let expected = load_save_slot(entry.path()).unwrap();
        let validator = contract(&expected);
        let output = publish_assembled(expected.clone(), &validator);
        assert_eq!(output.maps, expected.maps);
        assert_eq!(output.logbook, expected.logbook);
        for section in &output.game.sections {
            if !matches!(&section.name, b"THMB" | b"SCR3") {
                assert!(expected.game.sections.contains(section));
            }
        }
        assert!(
            output
                .game
                .image(SaveImageKind::Screen)
                .unwrap()
                .unwrap()
                .pixels()
                .iter()
                .all(|word| *word == 0x6b6b)
        );
        worlds.push(output.game.metadata.world_id);
    }
    println!(
        "installed image assembly worlds {worlds:?}; unchanged typed snapshots and journal; no continuation claim"
    );
}
