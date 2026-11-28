//! Original record export reads live typed owners and rejects incomplete mappings.
#![cfg(feature = "original-save-import")]
mod support;
use cyrus_assets::script::ScriptPoolOffsets;
use cyrus_script::script::ScriptWorld;
use rgpre::import::savegame::parse_save_script_locals;

fn source(offset: u32) -> cyrus_assets::script::Scripts {
    let mut scripts = support::cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .insert(
            0,
            cyrus_soup::program::Step {
                next: 1,
                operation: cyrus_soup::program::Operation::End { target: 0 },
            },
        );
    scripts.actors.get_mut("LOGIC").unwrap().pool_offsets = Some(ScriptPoolOffsets {
        locals: Some(offset),
        strings: Some(0),
    });
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn authored_locals_export_round_trips_installed_instance_ranges() {
    use cyrus_soup::program::{Formula, Operation, Place, Step, Value};
    let mut scripts = source(0);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .extend([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Assign {
                        destination: Place::Local { index: 0 },
                        formula: Formula {
                            first: Value::Integer { value: -123 },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    support::advance(&mut world, 0).unwrap();
    let bytes = world.export_original_authored_locals(&scripts).unwrap();
    let saved = parse_save_script_locals(&bytes).unwrap();
    assert_eq!(saved.bind_offset(0, 2).unwrap(), &[-123, 9]);
    assert_eq!(saved.encode(), bytes);
}

#[test]
fn authored_locals_export_rejects_unknown_pool_and_unowned_words() {
    let mut scripts = source(0);
    scripts.actors.get_mut("LOGIC").unwrap().pool_offsets = None;
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = format!(
        "{:#}",
        world.export_original_authored_locals(&scripts).unwrap_err()
    );
    assert!(
        error.contains("world 7") && error.contains("SVRG") && error.contains("LOGIC"),
        "{error}"
    );
    let scripts = source(4);
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = format!(
        "{:#}",
        world.export_original_authored_locals(&scripts).unwrap_err()
    );
    assert!(
        error.contains("SVRG") && error.contains("retained authored pool missing"),
        "{error}"
    );
}

#[test]
fn authored_locals_export_rejects_mismatched_source() {
    let mut scripts = source(0);
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    scripts.world = 8;
    let error = format!(
        "{:#}",
        world.export_original_authored_locals(&scripts).unwrap_err()
    );
    assert!(
        error.contains("world 7") && error.contains("source"),
        "{error}"
    );
}

#[test]
fn authored_locals_export_rejects_overlapping_installed_owners() {
    let mut scripts = source(0);
    let duplicate = scripts.actors["LOGIC"].clone();
    scripts.actors.insert("OTHER".into(), duplicate);
    let mut placements = scripts.instances.clone();
    for placement in &mut placements {
        placement.actor = "OTHER".into();
        placement.object_id += 2;
    }
    scripts.instances.extend(placements);
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = format!(
        "{:#}",
        world.export_original_authored_locals(&scripts).unwrap_err()
    );
    assert!(
        error.contains("SVRG") && error.contains("overlapping"),
        "{error}"
    );
}

#[test]
fn authored_locals_export_rejects_unaligned_ranges() {
    let scripts = source(1);
    let error = ScriptWorld::prepare(&scripts, 1)
        .err()
        .expect("unaligned source must reject");
    assert!(format!("{error:#}").contains("unaligned"));
}

fn installed_source() -> anyhow::Result<
    Option<(
        rgpre::import::savegame::SaveSlot,
        cyrus_assets::script::Scripts,
    )>,
> {
    use anyhow::Context;
    let Some(path) = std::env::var_os("CYRUS_ORIGINAL_SAVE") else {
        eprintln!("skipped: supply CYRUS_ORIGINAL_SAVE for installed export");
        return Ok(None);
    };
    let slot = rgpre::import::savegame::load_save_slot(path)?;
    let root =
        std::env::var_os("REDGUARD_DIR").context("installed export requires REDGUARD_DIR")?;
    let install = cyrus_assets::install::GameInstall::locate(Some(std::path::Path::new(&root)))?;
    let worlds = cyrus_assets::scene::read_worlds(&install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|e| e.index == slot.game.metadata.world_id)
        .context("saved world absent from WORLD.INI")?;
    let filename = entry
        .map
        .rsplit(['\\', '/'])
        .next()
        .context("map name absent")?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(
        cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
            .context("map absent")?,
    )?)?;
    let scripts = cyrus_assets::script_export::prepare(&map, &install.root, entry.index)?;
    Ok(Some((slot, scripts)))
}

#[test]
fn installed_fresh_export_rejects_unretained_pool_storage() -> anyhow::Result<()> {
    let Some((_slot, scripts)) = installed_source()? else {
        return Ok(());
    };
    let world = ScriptWorld::prepare(&scripts, 1)?;
    assert!(scripts.actors.contains_key("COMMON"));
    // Definitions alone do not provide unassigned pool words. Fresh-world export
    // must remain unsupported until the complete allocation is available.
    let error = format!(
        "{:#}",
        world.export_original_authored_locals(&scripts).unwrap_err()
    );
    assert!(error.contains("retained authored pool missing"), "{error}");
    Ok(())
}

#[test]
fn authored_locals_export_rejects_ambiguous_live_pool_owners() {
    let mut scripts = source(0);
    let mut extra = scripts.instances[0].clone();
    extra.object_id = 2;
    scripts.instances.push(extra);
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = format!(
        "{:#}",
        world.export_original_authored_locals(&scripts).unwrap_err()
    );
    assert!(
        error.contains("expected one live pool owner, found 2"),
        "{error}"
    );
}

#[test]
fn global_flags_export_current_values_without_initializing_or_clearing_latches() {
    use cyrus_script::script::GlobalFlagsSnapshot;
    use rgpre::import::savegame::parse_save_flags;
    let mut scripts = source(0);
    scripts.definition.flags = ["BOOL", "FLIPFLOP", "COUNTER", "NUMBER"]
        .into_iter()
        .enumerate()
        .map(|(index, flag_type)| cyrus_soup::program::Flag {
            name: format!("Flag{index}"),
            flag_type: flag_type.into(),
            initial_value: 0,
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut value = serde_json::to_value(world.snapshot_flags()).unwrap();
    let current = [-123, i32::MIN, i32::MAX, 17];
    for (flag, value) in value["flags"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(current)
    {
        flag["value"] = value.into();
    }
    world
        .restore_flags(serde_json::from_value::<GlobalFlagsSnapshot>(value).unwrap())
        .unwrap();
    let before = serde_json::to_value(world.snapshot_flags()).unwrap();
    let bytes = world
        .snapshot_flags()
        .export_original(&scripts.definition)
        .unwrap();
    let decoded = parse_save_flags(&bytes).unwrap();
    assert_eq!(decoded.values, current);
    assert_eq!(decoded.states, [0, 1, 2, 3]);
    assert_ne!(decoded.values[0], scripts.definition.flags[0].initial_value);
    assert_eq!(
        serde_json::to_value(world.snapshot_flags()).unwrap(),
        before
    );
}

#[test]
fn global_flags_export_rejects_latches_and_different_installed_owners() {
    use cyrus_script::script::GlobalFlagsSnapshot;
    let scripts = source(0);
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = world.snapshot_flags();
    let mut value = serde_json::to_value(&snapshot).unwrap();
    value["flags"][0]["latched"] = true.into();
    let latched = serde_json::from_value::<GlobalFlagsSnapshot>(value).unwrap();
    let error = format!(
        "{:#}",
        latched.export_original(&scripts.definition).unwrap_err()
    );
    assert!(
        error.contains("SVGF") && error.contains("Counter") && error.contains("latched"),
        "{error}"
    );
    let mut changed = scripts.definition.clone();
    changed.flags[0].name = "DifferentOwner".into();
    let error = format!("{:#}", snapshot.export_original(&changed).unwrap_err());
    assert!(
        error.contains("SVGF") && error.contains("installed definition"),
        "{error}"
    );
    changed.flags.clear();
    assert!(format!("{:#}", snapshot.export_original(&changed).unwrap_err()).contains("count"));
}

#[test]
fn global_flags_export_installed_changed_values_roundtrip() -> anyhow::Result<()> {
    use anyhow::Context;
    use cyrus_script::script::GlobalFlagsSnapshot;
    use rgpre::import::savegame::parse_save_flags;
    let Some((slot, scripts)) = installed_source()? else {
        return Ok(());
    };
    let original = &slot
        .game
        .sections
        .iter()
        .find(|s| s.name == *b"SVGF")
        .context("installed SAVEGAME.SAV SVGF absent")?
        .data;
    let saved = parse_save_flags(original)?;
    let mut world = ScriptWorld::prepare(&scripts, 1)?;
    let mut snapshot = serde_json::to_value(world.snapshot_flags())?;
    anyhow::ensure!(
        saved.values.len() == snapshot["flags"].as_array().unwrap().len(),
        "installed flag census differs"
    );
    for (flag, &value) in snapshot["flags"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&saved.values)
    {
        flag["value"] = value.into();
    }
    world.restore_flags(serde_json::from_value::<GlobalFlagsSnapshot>(snapshot)?)?;
    assert_eq!(
        world
            .snapshot_flags()
            .export_original(&scripts.definition)?,
        *original
    );
    let mut snapshot = serde_json::to_value(world.snapshot_flags())?;
    let changed = saved
        .values
        .first()
        .context("installed flag list empty")?
        .wrapping_add(17);
    snapshot["flags"][0]["value"] = changed.into();
    world.restore_flags(serde_json::from_value::<GlobalFlagsSnapshot>(snapshot)?)?;
    let bytes = world
        .snapshot_flags()
        .export_original(&scripts.definition)?;
    assert_ne!(bytes, *original);
    let decoded = parse_save_flags(&bytes)?;
    assert_eq!(decoded.values[0], changed);
    assert_eq!(decoded.values[1..], saved.values[1..]);
    assert_eq!(decoded.states, saved.states);
    Ok(())
}
