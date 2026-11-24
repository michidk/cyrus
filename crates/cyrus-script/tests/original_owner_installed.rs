//! Public owner staging against an explicitly supplied original save and install.
//! Source validation here does not establish initialized native allocation identity.
#![cfg(feature = "original-save-import")]
use anyhow::{Context, Result, ensure};
use cyrus_script::script::original_save::{
    OriginalActorBinding, OriginalCodeBinding, OriginalOwnerContinuation, OriginalShellBinding,
};
use cyrus_soup::{
    CodeOrigin, TaskSource, Thread,
    persistence::ContinuationTask,
    program::{CallKind, Operation, Program},
};
use rgpre::import::savegame::*;
use std::{collections::BTreeMap, sync::Arc};

type Sources = BTreeMap<CodeOrigin, Arc<Program>>;
type Operands = BTreeMap<u32, TaskSource>;

fn installed_sources(scripts: &cyrus_assets::script::Scripts) -> Result<(Sources, Operands)> {
    let mut sources = BTreeMap::new();
    let mut operands = BTreeMap::new();
    for actor in scripts.actors.values() {
        let origin = CodeOrigin(actor.code_origin);
        sources.insert(origin, Arc::new(actor.program.clone()));
        for (&instruction, step) in &actor.program.steps {
            let (call, object_call) = match &step.operation {
                Operation::Call { call } => (call, false),
                Operation::ObjectCall { call } => (call, true),
                _ => continue,
            };
            if call.kind == CallKind::Function {
                continue;
            }
            let source = TaskSource {
                origin,
                instruction,
                object_call,
                thread: Thread::Main,
            };
            operands.insert(source.operand()?, source);
        }
    }
    Ok((sources, operands))
}

fn tasks(slots: [SaveTaskSlot; 6], operands: &Operands) -> Result<[Option<ContinuationTask>; 6]> {
    let mut tasks = [None; 6];
    for (index, slot) in slots.into_iter().enumerate() {
        if slot.callback != 0 {
            tasks[index] = Some(ContinuationTask {
                token: index as u64 + 1,
                source: *operands
                    .get(&slot.callback)
                    .context("saved callback has no installed task operand")?,
            });
        }
    }
    Ok(tasks)
}

#[test]
fn supplied_original_save_stages_every_semantic_owner_without_execution() -> Result<()> {
    let Some(path) = std::env::var_os("CYRUS_ORIGINAL_SAVE") else {
        eprintln!("skipped: no original save supplied with CYRUS_ORIGINAL_SAVE");
        return Ok(());
    };
    let slot = load_save_slot(path)?;
    let install = cyrus_assets::install::GameInstall::locate(None)?;
    let worlds = cyrus_assets::scene::read_worlds(&install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == slot.game.metadata.world_id)
        .context("saved world absent from installed WORLD.INI")?;
    let filename = entry
        .map
        .rsplit(['\\', '/'])
        .next()
        .context("empty world map path")?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(
        cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
            .context("installed map missing")?,
    )?)?;
    let scripts = cyrus_assets::script_export::prepare(&map, &install.root, entry.index)?;
    let (sources, operands) = installed_sources(&scripts)?;
    let section = |name| -> Result<&[u8]> {
        Ok(&slot
            .game
            .sections
            .iter()
            .find(|section| section.name == name)
            .context("save section missing")?
            .data)
    };
    let locals = parse_save_script_locals(section(*b"SVRG")?)?;
    let codes = parse_save_code_owners(section(*b"SMCO")?)?;
    stage_codes(&codes, &locals, &scripts, &sources, &operands)?;
    let shells = parse_save_shells(section(*b"SMSH")?)?;
    stage_shells(&shells, &scripts, &sources, &operands)?;
    let objects = parse_save_objects(section(*b"SMOB")?)?;
    let actors = stage_actors(&objects, &locals, &scripts, &sources, &operands)?;
    eprintln!(
        "staged {actors} actors, {} code owners ({} active callback owners) and {} shells",
        codes.len(),
        codes
            .iter()
            .filter(|owner| owner.task_slots().iter().any(|slot| slot.callback != 0))
            .count(),
        shells.len()
    );
    Ok(())
}

fn stage_codes(
    codes: &[SaveCodeOwner],
    locals: &SaveScriptLocals,
    scripts: &cyrus_assets::script::Scripts,
    sources: &Sources,
    operands: &Operands,
) -> Result<()> {
    for owner in codes {
        let (name, definition) = scripts
            .actors
            .iter()
            .find(|(_, definition)| definition.code_origin == owner.script_continuation().data)
            .context("saved code source absent from installed programs")?;
        OriginalOwnerContinuation::decode_code(
            owner,
            locals,
            OriginalCodeBinding {
                owner_id: owner.owner_id,
                origin: CodeOrigin(definition.code_origin),
                tasks: tasks(owner.task_slots(), operands)?,
            },
            sources,
        )
        .with_context(|| format!("code owner {} {}", owner.owner_id, name))?;
    }
    Ok(())
}

fn stage_shells(
    shells: &[SaveShell],
    scripts: &cyrus_assets::script::Scripts,
    sources: &Sources,
    operands: &Operands,
) -> Result<()> {
    for owner in shells {
        let name = std::str::from_utf8(owner.name())?.trim_end_matches('\0');
        let definition = scripts
            .actors
            .get(name)
            .context("saved shell program absent")?;
        OriginalOwnerContinuation::decode_shell(
            owner,
            OriginalShellBinding {
                name: owner.name().try_into()?,
                origin: CodeOrigin(definition.code_origin),
                tasks: tasks(owner.task_slots(), operands)?,
            },
            sources,
        )?;
    }
    Ok(())
}

fn stage_actors(
    objects: &[SaveObject],
    locals: &SaveScriptLocals,
    scripts: &cyrus_assets::script::Scripts,
    sources: &Sources,
    operands: &Operands,
) -> Result<usize> {
    let mut actors = 0;
    for owner in objects {
        let saved = owner.script_continuation();
        if saved.data == 0 {
            continue;
        }
        let (name, definition) = scripts
            .actors
            .iter()
            .find(|(_, definition)| definition.code_origin == saved.data)
            .context("saved actor source absent from installed programs")?;
        match owner.object_id {
            -2 => ensure!(name == "CYRUS", "base player source differs"),
            -3 => ensure!(name == "GREMLIN", "alternate player source differs"),
            _ => {}
        }
        OriginalOwnerContinuation::decode(
            owner,
            locals,
            OriginalActorBinding {
                object_id: owner.object_id,
                runtime_identifier: owner.runtime_identifier(),
                origin: CodeOrigin(definition.code_origin),
                tasks: tasks(owner.task_slots(), operands)?,
            },
            sources,
        )
        .with_context(|| format!("actor {} {name}", owner.object_id))?;
        actors += 1;
    }
    Ok(actors)
}
