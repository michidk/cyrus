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

#[test]
fn installed_fresh_export_rejects_unretained_pool_storage() -> anyhow::Result<()> {
    use anyhow::Context;
    let Some(path) = std::env::var_os("CYRUS_ORIGINAL_SAVE") else {
        eprintln!("skipped: supply CYRUS_ORIGINAL_SAVE for installed export");
        return Ok(());
    };
    let slot = rgpre::import::savegame::load_save_slot(path)?;
    let install = cyrus_assets::install::GameInstall::locate(None)?;
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
