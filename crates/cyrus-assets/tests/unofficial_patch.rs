#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! The bundled `unofficial-patch` mod against the installed maps: every patch applies, the patched
//! programs validate, and only the named actors change. With `CYRUS_URP_REFERENCE_MAPS` set to a
//! folder of the maps as the original patch's author wrote them, each patched script is also
//! compared with theirs, statement by statement.
use cyrus_assets::script::Scripts;
use cyrus_assets::script_patch::{ScriptPatch, set_script_patches};
use cyrus_soup::program::{Operation, Program};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Deserialize)]
struct Manifest {
    #[serde(rename = "script-patch")]
    script_patches: Vec<ScriptPatch>,
}

fn prepare(root: &Path, map: &Path, world: u32) -> Scripts {
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(map).unwrap()).unwrap();
    cyrus_assets::script_export::prepare(&map, root, world).unwrap()
}

/// The statements reachable from the entry, with offsets renumbered in discovery order, jump chains
/// collapsed and named receivers spelt out: two programs that do the same print the same.
fn canonical(program: &Program) -> Vec<String> {
    let skip = |mut at: u32| {
        for _ in 0..64 {
            match program.steps.get(&at).map(|step| &step.operation) {
                Some(Operation::Goto { target }) => at = *target,
                _ => break,
            }
        }
        at
    };
    let mut ids: BTreeMap<u32, usize> = BTreeMap::new();
    let mut queue = vec![skip(program.entry)];
    if program.entry != 0 {
        queue.push(skip(0));
    }
    let mut order = Vec::new();
    let mut at = 0;
    while at < queue.len() {
        let offset = queue[at];
        at += 1;
        if ids.contains_key(&offset) {
            continue;
        }
        ids.insert(offset, ids.len());
        order.push(offset);
        let step = &program.steps[&offset];
        let mut successors = Vec::new();
        match &step.operation {
            Operation::Goto { .. } => unreachable!("collapsed"),
            Operation::End { target } => successors.push(*target),
            Operation::If { target, .. }
            | Operation::Gosub { target }
            | Operation::ScriptRv { target, .. } => {
                successors.push(step.next);
                successors.push(*target);
            }
            Operation::Return | Operation::Endint | Operation::Unsupported { .. } => {}
            _ => successors.push(step.next),
        }
        queue.extend(successors.into_iter().map(skip));
    }
    let ident = |offset: u32| ids.get(&skip(offset)).map_or(-1, |&id| id as i64);
    order
        .iter()
        .map(|offset| {
            let step = &program.steps[offset];
            let mut json = serde_json::to_value(&step.operation).unwrap();
            spell_out(&mut json, &program.strings, &ident);
            let next = match &step.operation {
                Operation::Return
                | Operation::Endint
                | Operation::Unsupported { .. }
                | Operation::End { .. } => -2,
                _ => ident(step.next),
            };
            format!("{} next {next} {json}", ids[offset])
        })
        .collect()
}

fn spell_out(value: &mut serde_json::Value, strings: &[String], ident: &dyn Fn(u32) -> i64) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|t| t.as_str()) == Some("named") {
                let name = strings[map["string"].as_u64().unwrap() as usize].to_lowercase();
                map.insert("string".into(), name.into());
            }
            for key in ["target", "label"] {
                let is_jump = map.contains_key("type")
                    && map["type"].as_str().is_some_and(|t| {
                        matches!(t, "goto" | "end" | "if" | "gosub" | "script_rv")
                    });
                if is_jump && key == "target" {
                    let id = ident(map[key].as_u64().unwrap() as u32);
                    map.insert(key.into(), id.into());
                }
            }
            for child in map.values_mut() {
                spell_out(child, strings, ident);
            }
        }
        serde_json::Value::Array(items) => items
            .iter_mut()
            .for_each(|item| spell_out(item, strings, ident)),
        _ => {}
    }
}

#[test]
fn the_unofficial_patch_applies_to_the_installed_maps() {
    let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
        eprintln!("skipped: no install (unofficial patch)");
        return;
    };
    let root = install.root.clone();
    let manifest: Manifest = toml::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/unofficial-patch/mod.toml"),
        )
        .unwrap(),
    )
    .unwrap();
    let worlds = rgpre::import::world_ini::WorldIni::parse(
        &std::fs::read_to_string(root.join("WORLD.INI")).unwrap(),
    );
    let world_of = |stem: &str| {
        worlds
            .entries
            .iter()
            .find(|entry| {
                entry
                    .map
                    .rsplit(['\\', '/'])
                    .next()
                    .is_some_and(|file| file.to_ascii_uppercase() == format!("{stem}.RGM"))
            })
            .unwrap_or_else(|| panic!("{stem} is not in WORLD.INI"))
            .index
    };
    let maps: Vec<String> = {
        let mut maps: Vec<String> = manifest
            .script_patches
            .iter()
            .map(|patch| patch.world.to_ascii_uppercase())
            .collect();
        maps.sort();
        maps.dedup();
        maps
    };
    let reference = std::env::var_os("CYRUS_URP_REFERENCE_MAPS").map(std::path::PathBuf::from);
    // The patch list is fixed by the first set, so everything that needs no patches goes first.
    let mut originals = BTreeMap::new();
    let mut references = BTreeMap::new();
    for map in &maps {
        let path = root.join("maps").join(format!("{map}.RGM"));
        originals.insert(map.clone(), prepare(&root, &path, world_of(map)));
        if let Some(folder) = &reference {
            references.insert(
                map.clone(),
                prepare(&root, &folder.join(format!("{map}.RGM")), world_of(map)),
            );
        }
    }
    assert!(set_script_patches(manifest.script_patches.clone()));
    for map in &maps {
        let path = root.join("maps").join(format!("{map}.RGM"));
        let patched = prepare(&root, &path, world_of(map));
        patched.validate().unwrap();
        let original = &originals[map];
        let named: Vec<String> = manifest
            .script_patches
            .iter()
            .filter(|patch| patch.world.eq_ignore_ascii_case(map))
            .map(|patch| patch.actor.to_ascii_uppercase())
            .collect();
        for (name, actor) in &patched.actors {
            let before = &original.actors[name];
            if named.contains(&name.to_ascii_uppercase()) {
                assert_ne!(
                    canonical(&actor.program),
                    canonical(&before.program),
                    "{map}/{name} is named by a patch and must change"
                );
                // The reference install did not apply the patch's EXTPALAC change, though its notes
                // list it, so there is no statement to compare that map with.
                // COYLE is this mod's own fix, absent from the reference.
                if let Some(reference) = references
                    .get(map)
                    .filter(|_| map != "EXTPALAC" && !name.eq_ignore_ascii_case("COYLE"))
                {
                    let theirs = canonical(&reference.actors[name].program);
                    let ours = canonical(&actor.program);
                    assert_eq!(
                        ours.len(),
                        theirs.len(),
                        "{map}/{name}: statement counts differ"
                    );
                    for (index, (ours, theirs)) in ours.iter().zip(&theirs).enumerate() {
                        assert_eq!(ours, theirs, "{map}/{name}: statement {index}");
                    }
                }
            } else {
                assert_eq!(
                    canonical(&actor.program),
                    canonical(&before.program),
                    "{map}/{name} is not named by a patch and must not change"
                );
            }
        }
    }
    if reference.is_none() {
        eprintln!("note: CYRUS_URP_REFERENCE_MAPS unset; compared with the originals only");
    }
}
