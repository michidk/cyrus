//! Public owner staging and exact compact allocation binding against supplied saves.
//! This does not establish complete native-service or session restoration.
#![cfg(feature = "original-save-import")]
use anyhow::{Context, Result, ensure};
use cyrus_script::script::original_save::{
    OriginalActorBinding, OriginalCodeAllocation, OriginalCodeBinding, OriginalOwnerContinuation,
    OriginalShellBinding,
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
    let allocations = OriginalCodeAllocation::bind(scripts, codes)?;
    let mut reordered = codes.to_vec();
    reordered.reverse();
    ensure!(
        OriginalCodeAllocation::bind(scripts, &reordered)? == allocations,
        "save traversal must not determine allocation identity"
    );
    if !codes.is_empty() {
        let mut invalid = codes.to_vec();
        invalid[0].owner_id = u32::MAX;
        ensure!(
            OriginalCodeAllocation::bind(scripts, &invalid).is_err(),
            "foreign allocation accepted"
        );
        ensure!(
            OriginalCodeAllocation::bind(scripts, &codes[1..]).is_err(),
            "partial population accepted"
        );
    }
    if codes.len() > 1 {
        let mut duplicate = codes.to_vec();
        duplicate[0] = duplicate[1].clone();
        ensure!(
            OriginalCodeAllocation::bind(scripts, &duplicate).is_err(),
            "duplicate owner accepted"
        );
    }
    for owner in codes {
        let allocation = allocations
            .iter()
            .find(|allocation| allocation.owner_id == owner.owner_id)
            .context("saved allocation absent")?;
        let name = &scripts.instances[allocation.placement_index].actor;
        let definition = &scripts.actors[name];
        let bound_tasks = tasks(owner.task_slots(), operands)?;
        let origin = CodeOrigin(definition.code_origin);
        let binding = || OriginalCodeBinding {
            owner_id: owner.owner_id,
            origin,
            tasks: bound_tasks,
        };
        let imported = OriginalOwnerContinuation::decode_code(owner, locals, binding(), sources)
            .with_context(|| format!("code owner {} {}", owner.owner_id, name))?;
        let before = encode_save_code_owners(std::slice::from_ref(owner));
        let mut changed = imported.vm.continuation(origin, sources)?;
        changed.result = changed.result.wrapping_add(7);
        changed.saved_result = changed.saved_result.wrapping_add(11);
        changed.status[5] ^= 0x80;
        changed.timers[5] = changed.timers[5].wrapping_add(17);
        let tokens = imported
            .vm
            .task_tokens()
            .map(|token| (token, token))
            .collect();
        let native = cyrus_soup::Runtime::from_snapshot(
            origin,
            cyrus_soup::persistence::Snapshot::from_continuation(origin, changed, sources)?,
            sources,
            &tokens,
            "changed installed code owner".into(),
        )?;
        let mut current = OriginalOwnerContinuation {
            vm: native.snapshot(sources)?,
            invocation_words: imported.invocation_words,
        };
        current.invocation_words[5][3] ^= u32::MAX;
        let exported = current.encode_code(owner, binding(), sources)?;
        let encoded = encode_save_code_owners(&[exported]);
        ensure!(
            encoded != before,
            "changed native execution was not exported"
        );
        let exported = parse_save_code_owners(&encoded)?.remove(0);
        ensure!(
            exported.owner_id == owner.owner_id
                && exported.script_local_offset() == owner.script_local_offset()
                && exported.script_references().native_hook
                    == owner.script_references().native_hook,
            "independent owner allocation changed"
        );
        let roundtrip =
            OriginalOwnerContinuation::decode_code(&exported, locals, binding(), sources)?;
        ensure!(
            serde_json::to_value(&roundtrip.vm)? == serde_json::to_value(&current.vm)?
                && roundtrip.invocation_words == current.invocation_words,
            "changed native execution did not survive original record roundtrip"
        );
        for unsupported in ["anchor", "pause"] {
            let mut state = current.vm.continuation(origin, sources)?;
            if unsupported == "anchor" {
                state.anchor = 1;
            } else {
                state.pause[5] = 1;
            }
            let invalid = OriginalOwnerContinuation {
                vm: cyrus_soup::persistence::Snapshot::from_continuation(origin, state, sources)?,
                invocation_words: current.invocation_words,
            };
            let error = format!(
                "{:#}",
                invalid.encode_code(owner, binding(), sources).unwrap_err()
            );
            ensure!(
                error.contains("SMCO") && error.to_ascii_lowercase().contains(unsupported),
                "missing contextual unsupported-state error: {error}"
            );
            ensure!(
                encode_save_code_owners(std::slice::from_ref(owner)) == before,
                "failed export mutated source owner"
            );
        }
        let wrong = OriginalCodeBinding {
            owner_id: u32::MAX,
            ..binding()
        };
        ensure!(
            current.encode_code(owner, wrong, sources).is_err(),
            "foreign owner accepted"
        );
        let mut wrong = binding();
        if let Some(task) = wrong.tasks.iter_mut().flatten().next() {
            task.token = task.token.wrapping_add(1000);
        } else {
            wrong.tasks[0] = Some(ContinuationTask {
                token: 1000,
                source: *operands
                    .values()
                    .next()
                    .context("installed task source absent")?,
            });
        }
        ensure!(
            current.encode_code(owner, wrong, sources).is_err(),
            "task ownership mismatch accepted"
        );
        ensure!(
            encode_save_code_owners(std::slice::from_ref(owner)) == before,
            "failed ownership export mutated source owner"
        );
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
