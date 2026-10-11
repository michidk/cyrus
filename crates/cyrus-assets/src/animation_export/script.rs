//! Native script extraction through rgpre; portable playback reads descriptors only.
use crate::script::{Operation, Program, Step};
use anyhow::Context;
use rgpre::import::{
    rgm::{
        self, RgmFile,
        script_step::{LiteralScriptOperation, decode_literal_script_step},
    },
    soup_def,
};
use std::{collections::BTreeMap, path::Path};

pub(super) fn export(rgm: &RgmFile, root: &Path) -> anyhow::Result<serde_json::Value> {
    let definition = soup_def::try_load_soup_def(root);
    let mut programs = BTreeMap::new();
    for (actor, source) in rgm::disassemble_rgm_scripts(rgm, definition.as_ref()) {
        let entry = u32::try_from(source.script_pc)
            .with_context(|| format!("actor {actor}: invalid script entry {}", source.script_pc))?;
        let mut cursor = 0;
        let mut steps = BTreeMap::new();
        loop {
            let offset = u32::try_from(cursor)
                .with_context(|| format!("actor {actor}: script offset outside u32 range"))?;
            let decoded = match decode_literal_script_step(&source.bytecode, cursor) {
                Ok(decoded) => decoded,
                Err(reason) => {
                    steps.insert(
                        offset,
                        Step {
                            next: offset,
                            operation: Operation::Unsupported { reason },
                        },
                    );
                    break;
                }
            };
            let operation = match decoded.operation {
                LiteralScriptOperation::Task {
                    function_id,
                    arguments,
                } => {
                    match definition
                        .as_ref()
                        .and_then(|def| def.functions.get(usize::from(function_id)))
                    {
                        Some(function) if usize::from(function.param_count) == arguments.len() => {
                            Operation::Task {
                                function: function.name.clone(),
                                arguments,
                            }
                        }
                        _ => Operation::Unsupported {
                            reason: format!(
                                "installed SOUP386.DEF function {function_id} or its argument count unavailable"
                            ),
                        },
                    }
                }
                LiteralScriptOperation::End { target } => Operation::End { target },
            };
            let next = u32::try_from(decoded.next)
                .with_context(|| format!("actor {actor}: script continuation outside u32 range"))?;
            let terminal =
                decoded.next == source.bytecode.len() && matches!(operation, Operation::End { .. });
            steps.insert(offset, Step { next, operation });
            cursor = decoded.next;
            if terminal {
                break;
            }
        }
        anyhow::ensure!(
            programs
                .insert(
                    actor.clone(),
                    Program {
                        version: 1,
                        entry,
                        steps
                    }
                )
                .is_none(),
            "actor {actor}: duplicate script program"
        );
    }
    Ok(serde_json::to_value(programs)?)
}
