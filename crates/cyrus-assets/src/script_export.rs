//! Shared SOUP preparation through preservation's typed decoder and installed definitions.
use crate::script::{Actor, ClockSettings, CombatTaunt, Instance, Scripts, UpdateSettings};
use anyhow::{Context, Result};
use cyrus_script::program::{CallKind, Definition, Flag, Function, Operation, Program, Step};
use rgpre::import::{
    rgm::{
        self, RgmFile,
        script_program::{decode_native_hook, decode_script_program},
    },
    soup_def::SoupFunctionKind,
};
use std::collections::BTreeMap;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
use std::path::Path;

/// Exports actor programs independently of mesh availability; unsupported actors remain visible.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn prepare(rgm: &RgmFile, root: &Path, world: u32) -> Result<Scripts> {
    let install = crate::install::GameInstall::open(root)?;
    prepare_source(rgm, &crate::native::Files::scan(&install, false)?, world)
}

/// Decode the same actor programs and installed defaults on either target.
pub fn prepare_source(
    rgm: &RgmFile,
    source: &dyn crate::source::ReadAssets,
    world: u32,
) -> Result<Scripts> {
    let system_text = crate::preparation::text(source, "SYSTEM.INI")?;
    let config = rgpre::import::soup_ini::SoupConfig::parse(&system_text)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("world {world} script settings"))?;
    let system = rgpre::import::system_ini::SystemIni::parse(&system_text)
        .with_context(|| format!("world {world} SYSTEM.INI clock settings"))?;
    let clock = ClockSettings::from_system(&system.system)
        .with_context(|| format!("world {world} script clock"))?;
    let updates = UpdateSettings::from_system(&system)
        .with_context(|| format!("world {world} script update settings"))?;
    let camera = crate::script::CameraSettings::from_system(&system)
        .with_context(|| format!("world {world} script camera settings"))?;
    let dialogue = crate::script::DialogueSettings::from_system(&system)
        .with_context(|| format!("world {world} script dialogue settings"))?;
    let player_distance_override = player_distance_override(source)
        .with_context(|| format!("world {world} REDGUARD.CHT script query settings"))?;
    let definition = crate::preparation::soup(source)?;
    let metadata = rgm::export_rgm_metadata_json(rgm, Some(&definition));
    let assets_source = source;
    let mut combat = None;
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
        if (initialized[14] != 0 || attributes.get("attr_num_taunts").copied().unwrap_or(0) != 0)
            && combat.is_none()
        {
            combat = Some(
                rgpre::import::combat_ini::CombatIni::parse(&crate::preparation::text(
                    assets_source,
                    "COMBAT.INI",
                )?)
                .with_context(|| format!("{context}: COMBAT.INI"))?,
            );
        }
        let combat_taunts = match &combat {
            Some(combat) => prepare_combat_taunts(record, &attributes, combat, &context)?,
            None => vec![],
        };
        let native_hook = decode_native_hook(rgm, source.header_index, &definition)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("{context}: native hook"))?
            .map(|hook| serde_json::from_value(serde_json::to_value(hook)?))
            .transpose()?;
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
            version: 4,
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
                        location_offsets: rgm::actor_location_offsets(rgm, source.header_index)
                            .map_err(anyhow::Error::msg)
                            .with_context(|| format!("{context}: authored locations"))?,
                        program,
                        attributes,
                        native_hook,
                        combat_taunts
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
        version: 19,
        in_combat_threshold: combat
            .as_ref()
            .and_then(|combat| combat.misc.in_combat_threshold)
            .map(|value| value.wrapping_shl(8)),
        node_timer: combat.as_ref().and_then(|combat| combat.misc.node_timer),
        non_engaged_spacing: combat
            .as_ref()
            .and_then(|combat| combat.misc.non_engaged_spacing),
        combat_attacks: combat
            .as_ref()
            .map(crate::script::combat::prepare)
            .transpose()
            .with_context(|| format!("world {world} combat attacks"))?
            .unwrap_or_default(),
        combat_defenses: combat
            .as_ref()
            .map(crate::script::combat::prepare_defenses)
            .transpose()
            .with_context(|| format!("world {world} combat defenses"))?
            .unwrap_or_default(),
        player_distance_override,
        clock,
        updates,
        camera,
        dialogue,
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
                light_intensity: record.intensity,
                light_radius: record.radius,
                light_rgb: [record.red, record.green, record.blue],
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

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(scripts: &Scripts, out: &Path) -> Result<()> {
    let path = out.with_extension("scripts");
    crate::native::publish(&path, &serde_json::to_vec(scripts)?)
        .with_context(|| format!("commit {}", path.display()))?;
    Ok(())
}

fn player_distance_override(source: &dyn crate::source::ReadAssets) -> Result<bool> {
    let Some(file) = source.manifest().find("REDGUARD.CHT") else {
        return Ok(false);
    };
    let cheats = crate::cheats::decode(&source.read(&file.path)?)?;
    Ok(cheats
        .value("neversaydie")
        .context("missing neversaydie state")?
        != 0)
}

fn prepare_combat_taunts(
    record: &serde_json::Value,
    attributes: &BTreeMap<String, u8>,
    combat: &rgpre::import::combat_ini::CombatIni,
    context: &str,
) -> Result<Vec<CombatTaunt>> {
    use rgpre::import::combat_ini::TauntTrigger;
    let count = attributes.get("attr_num_taunts").copied().unwrap_or(0);
    if count == 0 {
        return Ok(vec![]);
    }
    let first = match record.get("raex") {
        None => 0,
        Some(raex) => raex["taunt_id"]
            .as_i64()
            .with_context(|| format!("{context}: RAEX taunt_id absent"))?,
    };
    let first = u32::try_from(first)
        .with_context(|| format!("{context}: negative RAEX taunt_id {first}"))?;
    let maximum = combat
        .misc
        .max_taunts
        .with_context(|| format!("{context}: COMBAT.INI max_taunts absent"))?;
    (0..u32::from(count))
        .map(|offset| {
            let index = first
                .checked_add(offset)
                .context("combat taunt range overflow")?;
            anyhow::ensure!(
                index <= maximum,
                "{context}: combat taunt index {index} exceeds max_taunts {maximum}"
            );
            let Some(taunt) = combat.taunts.get(&index).filter(|_| index != maximum) else {
                return Ok(CombatTaunt {
                    trigger: 0,
                    label: [0; 4],
                    animation: 0,
                });
            };
            let trigger = match taunt
                .trigger
                .with_context(|| format!("{context}: COMBAT.INI [taunt{index}] type absent"))?
            {
                TauntTrigger::BeginCombat => 0,
                TauntTrigger::Attack => 1,
                TauntTrigger::HurtByOpponent => 2,
                TauntTrigger::HitOpponent => 3,
                TauntTrigger::Defend => 4,
                TauntTrigger::Move => 5,
                TauntTrigger::Death => 6,
                TauntTrigger::KilledOpponent => 7,
                TauntTrigger::WantSwitchOut => 8,
                TauntTrigger::WantSwitchIn => 9,
                TauntTrigger::UnarmedMaleOpponent => 10,
                TauntTrigger::UnarmedFemaleOpponent => 11,
                TauntTrigger::Unknown(code) => u8::try_from(code).with_context(|| {
                    format!("{context}: COMBAT.INI [taunt{index}] type {code} outside byte range")
                })?,
            };
            let label = taunt.rtx_label.as_ref().with_context(|| {
                format!("{context}: COMBAT.INI [taunt{index}] rtx_label absent")
            })?;
            let label = label.as_bytes().get(..4).with_context(|| {
                format!("{context}: COMBAT.INI [taunt{index}] rtx_label shorter than four bytes")
            })?;
            Ok(CombatTaunt {
                trigger,
                label: label.try_into().context("four-byte combat taunt label")?,
                animation: match taunt.animation.with_context(|| {
                    format!("{context}: COMBAT.INI [taunt{index}] animation absent")
                })? {
                    0 => 0,
                    animation => animation.wrapping_add(152),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod combat_tests {
    use super::*;
    #[test]
    fn actor_taunts_use_consecutive_authored_records_and_keep_reference_spelling() {
        let ini = rgpre::import::combat_ini::CombatIni::parse("[misc]\nmax_taunts=8\n[taunt3]\ntype=2\nrtx_label=Nope\nanimation=0\n[taunt4]\ntype=0\nrtx_label=AbCd\nanimation=7\n[taunt5]\ntype=6\nrtx_label=&123\nanimation=0\n").unwrap();
        let mut attributes = BTreeMap::from([("attr_num_taunts".into(), 2)]);
        let record = serde_json::json!({"raex":{"taunt_id":4}});
        let taunts =
            prepare_combat_taunts(&record, &attributes, &ini, "world 7 actor TEST").unwrap();
        assert_eq!(
            taunts,
            [
                CombatTaunt {
                    trigger: 0,
                    label: *b"AbCd",
                    animation: 159
                },
                CombatTaunt {
                    trigger: 6,
                    label: *b"&123",
                    animation: 0
                }
            ]
        );
        attributes.insert("attr_num_taunts".into(), 3);
        let sparse =
            prepare_combat_taunts(&record, &attributes, &ini, "world 7 actor TEST").unwrap();
        assert_eq!(
            sparse[2],
            CombatTaunt {
                trigger: 0,
                label: [0; 4],
                animation: 0
            }
        );
        attributes.insert("attr_num_taunts".into(), 6);
        assert!(
            prepare_combat_taunts(&record, &attributes, &ini, "world 7 actor TEST")
                .unwrap_err()
                .to_string()
                .contains("combat taunt index 9 exceeds max_taunts 8")
        );
        attributes.insert("attr_num_taunts".into(), 0);
        assert!(
            prepare_combat_taunts(&serde_json::Value::Null, &attributes, &ini, "TEST")
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn combat_taunt_sentinel_ignores_configured_record_and_absent_raex_starts_at_zero() {
        let ini = rgpre::import::combat_ini::CombatIni::parse(
            "[misc]\nmax_taunts=8\n[taunt8]\ntype=0\nrtx_label=Test\nanimation=1\n",
        )
        .unwrap();
        let attributes = BTreeMap::from([("attr_num_taunts".into(), 1)]);
        let empty = CombatTaunt {
            trigger: 0,
            label: [0; 4],
            animation: 0,
        };
        let sentinel = prepare_combat_taunts(
            &serde_json::json!({"raex":{"taunt_id":8}}),
            &attributes,
            &ini,
            "TEST",
        )
        .unwrap();
        assert_eq!(sentinel.as_slice(), std::slice::from_ref(&empty));
        let absent =
            prepare_combat_taunts(&serde_json::json!({}), &attributes, &ini, "TEST").unwrap();
        assert_eq!(absent, [empty]);
    }
}
