//! Complete-slot world/file authority comes from the session and installed catalog.
use anyhow::{Result, ensure};
use cyrus::original_slot::{
    CompleteSlotValidator, FileLayout, SlotLayout, assembly::SavedImages, request::ExportRequest,
    worlds::WorldFiles,
};
use rgpre::import::{
    savegame::{
        SaveGame, SaveImageKind, SaveSection, SaveSlot, encode_savegame_file, load_save_slot,
        parse_savegame_file,
    },
    world_ini::WorldIni,
};
use std::{collections::BTreeMap, fs};

// Fixture-only residual proof. Production inverse mappings must supply their own validator.
struct Retained(SaveSlot);
impl CompleteSlotValidator for Retained {
    fn layout(&self) -> Result<SlotLayout> {
        Ok(SlotLayout {
            files: std::iter::once(("SAVEGAME.SAV".to_owned(), &self.0.game))
                .chain(self.0.maps.iter().map(|(name, game)| (name.clone(), game)))
                .map(|(name, game)| {
                    (
                        name,
                        FileLayout {
                            world: game.metadata.world_id,
                            records: game.sections.iter().map(|r| r.name).collect(),
                        },
                    )
                })
                .collect(),
            logbook: self.0.logbook.is_some(),
        })
    }
    fn validate_record(&self, file: &str, _world: u32, record: &SaveSection) -> Result<()> {
        let game = if file == "SAVEGAME.SAV" {
            &self.0.game
        } else {
            &self.0.maps[file]
        };
        ensure!(
            game.sections.contains(record),
            "unsupported changed inverse record"
        );
        Ok(())
    }
    fn validate_slot(&self, slot: &SaveSlot) -> Result<()> {
        ensure!(
            slot.maps == self.0.maps && slot.logbook == self.0.logbook,
            "unsupported changed residual state"
        );
        Ok(())
    }
}
fn game(world: u32) -> SaveGame {
    let mut metadata = vec![0; 272];
    metadata[..4].copy_from_slice(b"1.31");
    metadata[140..144].copy_from_slice(&world.to_le_bytes());
    let sections = vec![
        SaveSection {
            name: *b"SVGM",
            data: metadata,
        },
        SaveSection {
            name: *b"SCR3",
            data: vec![1; SaveImageKind::Screen.width() * SaveImageKind::Screen.height() * 2],
        },
        SaveSection {
            name: *b"THMB",
            data: vec![2; SaveImageKind::Thumbnail.width() * SaveImageKind::Thumbnail.height() * 2],
        },
    ];
    parse_savegame_file(&encode_savegame_file(&sections).unwrap()).unwrap()
}
fn test_catalog(text: &str) -> WorldIni {
    WorldIni::parse(&format!(
        "{text}\nworld_palette[1]=test.col\nworld_palette[2]=test.col\nworld_palette[20]=test.col\n"
    ))
}
fn catalog() -> WorldIni {
    test_catalog("[world]\nworld_map[1]=maps\\island.rgm\nworld_map[20]=maps\\hideout.rgm\n")
}
fn slot() -> SaveSlot {
    SaveSlot {
        game: game(20),
        maps: BTreeMap::from([("ISLAND.TSG".into(), game(1))]),
        logbook: None,
    }
}
fn assemble(census: &WorldFiles, slot: SaveSlot) -> Result<cyrus::original_slot::CompleteSlot> {
    let images = SavedImages::decode(&slot.game)?;
    let remaining = Retained(slot.clone());
    census.assemble(slot, images, &remaining)
}
#[test]
fn required_worlds_follow_current_world_across_a_transition() {
    let census = WorldFiles::resolve(20, [1, 20], &catalog()).unwrap();
    assemble(&census, slot()).unwrap();
    let mut changed = slot();
    changed.game = game(1);
    changed.maps = BTreeMap::from([("HIDEOUT.TSG".into(), game(20))]);
    assert!(assemble(&census, changed.clone()).is_err());
    assemble(
        &WorldFiles::resolve(1, [1, 20], &catalog()).unwrap(),
        changed,
    )
    .unwrap();
}
#[test]
fn invalid_world_resources_reject_before_export_destination_is_touched() {
    let census = WorldFiles::resolve(20, [1, 20], &catalog()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("SAVEGAME");
    for mode in 0..5 {
        let mut bad = slot();
        match mode {
            0 => {
                bad.maps.clear();
            }
            1 => {
                bad.maps.insert("HIDEOUT.TSG".into(), game(20));
            }
            2 => {
                let prior = bad.maps.remove("ISLAND.TSG").unwrap();
                bad.maps.insert("OTHER.TSG".into(), prior);
            }
            3 => {
                bad.maps.insert("ISLAND.TSG".into(), game(20));
            }
            _ => {
                bad.game = game(1);
            }
        }
        let error = ExportRequest::prepare(&absent, 1, || assemble(&census, bad)).unwrap_err();
        let text = format!("{error:#}");
        assert!(
            text.contains("world") && text.contains("SAVEGAME"),
            "{text}"
        );
        assert!(!absent.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
#[test]
fn installed_catalog_must_resolve_every_retained_world_without_alias_redirection() {
    for ids in [vec![1], vec![1, 20, 20], vec![1, 20, 99]] {
        assert!(WorldFiles::resolve(20, ids, &catalog()).is_err());
    }
    let aliases = test_catalog(
        "[world]\nworld_map[1]=maps\\same.rgm\nworld_map[2]=maps\\same.rgm\nworld_map[20]=maps\\hideout.rgm\n",
    );
    assert!(
        format!(
            "{:#}",
            WorldFiles::resolve(20, [1, 2, 20], &aliases).unwrap_err()
        )
        .contains("ambiguous")
    );
    let mut duplicate = catalog();
    duplicate.entries.push(duplicate.entries[0].clone());
    assert!(WorldFiles::resolve(20, [1, 20], &duplicate).is_err());
}
#[test]
fn world_validation_does_not_replace_remaining_inverse_validation() {
    let census = WorldFiles::resolve(20, [1, 20], &catalog()).unwrap();
    let original = slot();
    let images = SavedImages::decode(&original.game).unwrap();
    let mut changed = original.clone();
    changed.logbook = Some(b"unmapped".to_vec());
    assert!(
        census
            .assemble(changed, images, &Retained(original))
            .is_err()
    );
}
#[test]
fn authentic_world_one_and_twenty_slots_match_the_installed_file_catalog() {
    let Some(install) = std::env::var_os("REDGUARD_DIR") else {
        eprintln!("SKIP world-file export: REDGUARD_DIR required");
        return;
    };
    let Some(fixtures) = std::env::var_os("REDGUARD_ORIGINAL_SAVE_INSTALL") else {
        eprintln!("SKIP world-file export: REDGUARD_ORIGINAL_SAVE_INSTALL required");
        return;
    };
    let worlds = WorldIni::parse(
        &fs::read_to_string(std::path::Path::new(&install).join("WORLD.INI")).unwrap(),
    );
    let mut accepted = std::collections::BTreeSet::new();
    for entry in fs::read_dir(std::path::Path::new(&fixtures).join("SAVEGAME")).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let slot = load_save_slot(entry.path()).unwrap();
        let current = slot.game.metadata.world_id;
        if ![1, 20].contains(&current) {
            continue;
        }
        let ids = std::iter::once(current).chain(slot.maps.values().map(|g| g.metadata.world_id));
        let census = WorldFiles::resolve(current, ids, &worlds).unwrap();
        assemble(&census, slot.clone()).unwrap();
        let mut invalid = slot;
        invalid.maps.insert("WRONG.TSG".into(), game(current));
        assert!(assemble(&census, invalid).is_err());
        accepted.insert(current);
    }
    assert_eq!(
        accepted,
        std::collections::BTreeSet::from([1, 20]),
        "both authentic fixture worlds required"
    );
}

#[test]
fn unsafe_or_unsupported_installed_map_names_reject_without_paths() {
    for map in [
        "maps\\TOOLONGSTEM.rgm",
        "maps\\CON.rgm",
        "maps\\island.bin",
        "maps\\bad-name.rgm",
    ] {
        let worlds = test_catalog(&format!("[world]\nworld_map[1]={map}\n"));
        let error = WorldFiles::resolve(1, [1], &worlds).unwrap_err();
        assert!(format!("{error:#}").contains("WORLD.INI world 1"));
    }
}

#[test]
fn an_active_map_alias_does_not_redirect_a_retained_world() {
    let worlds =
        test_catalog("[world]\nworld_map[1]=maps\\same.rgm\nworld_map[20]=maps\\same.rgm\n");
    let census = WorldFiles::resolve(1, [1, 20], &worlds).unwrap();
    let slot = SaveSlot {
        game: game(1),
        maps: BTreeMap::from([("SAME.TSG".into(), game(20))]),
        logbook: None,
    };
    assemble(&census, slot).unwrap();
}
