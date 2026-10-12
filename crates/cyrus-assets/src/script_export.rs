//! Native SOUP cache export through preservation's typed decoder and installed definitions.
use crate::script::{
    Actor, CallKind, Definition, Flag, Function, Instance, Operation, Program, Scripts, Step,
};
use anyhow::{Context, Result};
use rgpre::import::{
    rgm::{self, RgmFile, script_program::decode_script_program},
    soup_def::{self, SoupFunctionKind},
};
use std::{collections::BTreeMap, path::Path};

/// Exports actor programs independently of mesh availability; unsupported actors remain visible.
pub fn prepare(rgm: &RgmFile, root: &Path, world: u32) -> Result<Scripts> {
    let config_path = crate::scene::find_file(root, "SYSTEM.INI")
        .with_context(|| format!("world {world}: SYSTEM.INI absent"))?;
    let config = rgpre::import::soup_ini::SoupConfig::parse(
        &std::fs::read_to_string(&config_path)
            .with_context(|| format!("world {world}: {}", config_path.display()))?,
    )
    .map_err(anyhow::Error::msg)
    .with_context(|| format!("world {world} script settings"))?;
    let definition = soup_def::load_soup_def(root).map_err(anyhow::Error::msg)?;
    let metadata = rgm::export_rgm_metadata_json(rgm, Some(&definition));
    let mut actors = BTreeMap::new();
    let mut unsupported = BTreeMap::new();
    for (name, source) in rgm::disassemble_rgm_scripts(rgm, Some(&definition)) {
        let context = format!("world {world} actor {name} script");
        let entry =
            u32::try_from(source.script_pc).with_context(|| format!("{context}: invalid entry"))?;
        let record = metadata["actors"]
            .as_array()
            .and_then(|actors| {
                actors
                    .iter()
                    .find(|actor| actor["script_name"].as_str() == Some(&name))
            })
            .with_context(|| format!("{context}: actor metadata absent"))?;
        let mut attributes: BTreeMap<String, u8> = record
            .get("attributes")
            .map(|data| serde_json::from_value(data.clone()))
            .transpose()?
            .unwrap_or_default();
        let attribute_keys: Vec<_> = (0..256)
            .map(|index| {
                definition
                    .attributes
                    .get(index)
                    .filter(|name| !name.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("attr_{index}"))
            })
            .collect();
        let mut initialized = std::array::from_fn(|index| {
            attributes.get(&attribute_keys[index]).copied().unwrap_or(0)
        });
        config.initialize_attributes(&mut initialized);
        for (key, value) in attribute_keys.into_iter().zip(initialized) {
            attributes.insert(key, value);
        }
        let hooks: Vec<u32> = record
            .get("rahk_hooks")
            .map(|data| serde_json::from_value(data.clone()))
            .transpose()?
            .unwrap_or_default();
        let decoded = decode_script_program(&source.bytecode, entry, &definition);
        let steps = match decoded {
            Ok(steps) => serde_json::from_value(serde_json::to_value(steps)?)?,
            Err(reason) => {
                unsupported.insert(name.clone(), reason.clone());
                BTreeMap::from_iter([0, entry].into_iter().map(|offset| {
                    (
                        offset,
                        Step {
                            next: offset,
                            operation: Operation::Unsupported {
                                reason: reason.clone(),
                            },
                        },
                    )
                }))
            }
        };
        let program = Program {
            version: 2,
            entry,
            variables: source.variables,
            strings: source.strings,
            steps,
        };
        program.validate().with_context(|| context.clone())?;
        for step in program.steps.values() {
            if let Operation::Unsupported { reason } = &step.operation {
                unsupported
                    .entry(name.clone())
                    .or_insert_with(|| reason.clone());
            }
        }
        anyhow::ensure!(
            actors
                .insert(
                    name.clone(),
                    Actor {
                        header_index: source.header_index,
                        code_origin: u32::try_from(source.script_data_offset)
                            .with_context(|| format!("{context}: invalid script origin"))?,
                        instances: source.instances,
                        namespace: record
                            .get("ranm_name")
                            .and_then(|value| value.as_str())
                            .map(str::to_owned),
                        instance_variables: source.instance_variables,
                        program,
                        attributes,
                        hooks
                    }
                )
                .is_none(),
            "{context}: duplicate script definition"
        );
    }
    let definition = Definition {
        functions: definition
            .functions
            .into_iter()
            .map(|function| Function {
                name: function.name,
                param_count: function.param_count,
                kind: match function.kind {
                    SoupFunctionKind::Task => CallKind::Task,
                    SoupFunctionKind::Multitask => CallKind::Multitask,
                    SoupFunctionKind::Function => CallKind::Function,
                },
            })
            .collect(),
        flags: definition
            .flags
            .into_iter()
            .map(|flag| Flag {
                name: flag.name,
                flag_type: flag.flag_type,
                initial_value: flag.initial_value,
            })
            .collect(),
        references: definition.references,
        attributes: definition.attributes,
    };
    let mut scripts = Scripts {
        version: 5,
        actor_pool_capacity: config.actor_pool_capacity,
        world,
        content_id: String::new(),
        definition,
        actors,
        instances: rgm
            .sections
            .iter()
            .filter_map(|section| match section {
                rgm::RgmSection::MpobParsed(_, records) => Some(records),
                _ => None,
            })
            .flatten()
            .map(|record| Instance {
                object_id: record.id,
                actor: record.script_name(),
                object_type: record.object_type,
                active: record.is_active,
                model_name: record.model_name(),
                model_mode: record.is_static,
                map_position: [
                    i64::from(record.pos_x),
                    i64::from(record.pos_y),
                    i64::from(record.pos_z),
                ],
                angles: [record.angle_x, record.angle_y, record.angle_z],
            })
            .collect(),
        unsupported,
    };
    scripts.content_id = scripts.content_identity()?;
    scripts.validate()?;
    Ok(scripts)
}

pub fn export(scripts: &Scripts, out: &Path) -> Result<()> {
    let path = out.with_extension("scripts");
    crate::native::publish(&path, &serde_json::to_vec(scripts)?)
        .with_context(|| format!("commit {}", path.display()))?;
    Ok(())
}
