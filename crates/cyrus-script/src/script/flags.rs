//! Installed global flag types and native latch operations from preservation's SOUP rules.
use anyhow::{Context, Result};
use cyrus_soup::Argument;
use cyrus_soup::Native;
use cyrus_soup::program::{Definition, Flag};
use serde::{Deserialize, Serialize};

/// Ordered global values and latches, tied to the exact installed flag schema.
///
/// Preserves the independent state described by preservation's
/// `docs/formats/SAVEGAME.md#global-flags` independently of original SVGF encoding.
/// Deserialize first, then validate and commit with `ScriptWorld::restore_flags`.
/// Capturing or restoring does not execute native flag operations or scripts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalFlagsSnapshot {
    version: u32,
    flags: Vec<SavedFlag>,
}

#[cfg(feature = "original-save-import")]
impl GlobalFlagsSnapshot {
    /// Encode current values in exact installed flag order using the documented
    /// unlatched runtime kinds. Latched-byte encoding remains unsupported.
    /// This emits one SVGF payload, never a complete original save slot.
    pub fn export_original(&self, definition: &Definition) -> Result<Vec<u8>> {
        let mut flags =
            Flags::prepare(definition).context("SAVEGAME.SAV SVGF: installed flag schema")?;
        let mut values = Vec::new();
        flags
            .restore(&mut values, self.clone())
            .context("SAVEGAME.SAV SVGF: native flag snapshot validation")?;
        let states = flags
            .kinds
            .iter()
            .zip(&flags.latched)
            .zip(&flags.definitions)
            .map(|((kind, latched), definition)| {
                anyhow::ensure!(
                    !latched,
                    "SAVEGAME.SAV SVGF flag {}: latched state encoding unsupported",
                    definition.name
                );
                // Preservation's SOUP global-flag rules define unlatched kinds.
                Ok(match kind {
                    Kind::Bool => 0,
                    Kind::FlipFlop => 1,
                    Kind::Counter => 2,
                    Kind::Number => 3,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        rgpre::import::savegame::encode_save_flags(&rgpre::import::savegame::SaveFlags {
            values,
            states,
        })
        .map_err(Into::into)
    }

    pub(super) fn from_original(
        definition: &Definition,
        saved: &rgpre::import::savegame::SaveFlags,
    ) -> Result<Self> {
        anyhow::ensure!(
            saved.values.len() == definition.flags.len()
                && saved.states.len() == definition.flags.len(),
            "original SVGF count differs from SOUP386.DEF"
        );
        let flags = definition.flags.iter().zip(&saved.values).zip(&saved.states)
            .map(|((definition, &value), &state)| {
                let kind = match definition.flag_type.to_ascii_lowercase().as_str() {
                    "bool" => 0, "flipflop" => 1, "counter" => 2, "number" => 3,
                    other => anyhow::bail!("original SVGF flag {}: unsupported kind {other}", definition.name),
                };
                // The limited importer accepts the verified unlatched encoding only.
                anyhow::ensure!(state == kind,
                    "original SVGF flag {}: state {state} unsupported by limited import (expected unlatched kind {kind})", definition.name);
                Ok(SavedFlag { definition: definition.clone(), value, latched: false })
            }).collect::<Result<Vec<_>>>()?;
        Ok(Self { version: 1, flags })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedFlag {
    definition: Flag,
    value: i32,
    latched: bool,
}

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    FlipFlop,
    Counter,
    Number,
}
pub(super) struct Flags {
    definitions: Vec<Flag>,
    kinds: Vec<Kind>,
    latched: Vec<bool>,
}
impl Flags {
    pub(super) fn snapshot(&self, values: &[i32]) -> GlobalFlagsSnapshot {
        GlobalFlagsSnapshot {
            version: 1,
            flags: self
                .definitions
                .iter()
                .enumerate()
                .map(|(index, definition)| SavedFlag {
                    definition: definition.clone(),
                    value: values[index],
                    latched: self.latched[index],
                })
                .collect(),
        }
    }

    pub(super) fn restore(
        &mut self,
        values: &mut Vec<i32>,
        snapshot: GlobalFlagsSnapshot,
    ) -> Result<()> {
        anyhow::ensure!(
            snapshot.version == 1,
            "global flag snapshot version {} is unsupported",
            snapshot.version
        );
        anyhow::ensure!(
            snapshot.flags.len() == self.definitions.len(),
            "global flag snapshot count {} differs from installed count {}",
            snapshot.flags.len(),
            self.definitions.len()
        );
        for (index, (saved, installed)) in snapshot.flags.iter().zip(&self.definitions).enumerate()
        {
            anyhow::ensure!(
                saved.definition.name == installed.name
                    && saved.definition.flag_type == installed.flag_type
                    && saved.definition.initial_value == installed.initial_value,
                "global flag snapshot index {index} ({}) differs from installed definition",
                installed.name
            );
        }
        let (restored_values, restored_latches) = snapshot
            .flags
            .into_iter()
            .map(|flag| (flag.value, flag.latched))
            .unzip();
        *values = restored_values;
        self.latched = restored_latches;
        Ok(())
    }

    pub(super) fn is_set(&self, index: usize) -> bool {
        self.latched[index]
    }
    pub(super) fn mark_set(&mut self, index: usize) {
        self.latched[index] = true;
    }

    pub fn prepare(definition: &Definition) -> Result<Self> {
        let kinds = definition
            .flags
            .iter()
            .map(|flag| {
                Ok(match flag.flag_type.to_ascii_lowercase().as_str() {
                    "bool" => Kind::Bool,
                    "flipflop" => Kind::FlipFlop,
                    "counter" => Kind::Counter,
                    "number" => Kind::Number,
                    _ => anyhow::bail!("flag {}: type {} unsupported", flag.name, flag.flag_type),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            definitions: definition.flags.clone(),
            latched: vec![false; kinds.len()],
            kinds,
        })
    }
    pub fn call(
        &mut self,
        name: Native,
        arguments: &[Argument],
        values: &mut [i32],
    ) -> Result<Option<i32>> {
        let set = name == Native::SetGlobalFlag;
        let reset = name == Native::ResetGlobalFlag;
        let test = name == Native::TestGlobalFlag;
        if !set && !reset && !test {
            return Ok(None);
        }
        let [index] = arguments else {
            anyhow::bail!("{name} requires one argument")
        };
        let index = usize::try_from(index.integer()?).context("negative global flag index")?;
        let kind = self.kinds.get(index).context("global flag index absent")?;
        let value = values.get_mut(index).context("global flag value absent")?;
        let latch = &mut self.latched[index];
        if test {
            return Ok(Some(i32::from(*latch)));
        }
        if reset {
            *latch = false;
            return Ok(Some(0));
        }
        match kind {
            Kind::Bool if !*latch => {
                *value = i32::from(*value == 0);
                *latch = true;
            }
            Kind::FlipFlop => {
                *value = i32::from(*value == 0);
                *latch = true;
            }
            Kind::Counter if *value != 0 => {
                *value = value.wrapping_sub(1);
                *latch = true;
            }
            Kind::Number => *latch = true,
            _ => {}
        }
        // The script-call wrapper returns zero even when the underlying flag operation signals a change.
        Ok(Some(0))
    }
}

#[cfg(all(test, feature = "original-save-import"))]
mod original_import_tests {
    use super::*;

    #[test]
    fn original_flags_restore_values_without_setting_latches() -> Result<()> {
        let definition = Definition {
            functions: vec![],
            flags: vec![
                Flag {
                    name: "Toggle".into(),
                    flag_type: "BOOL".into(),
                    initial_value: 0,
                },
                Flag {
                    name: "Count".into(),
                    flag_type: "NUMBER".into(),
                    initial_value: 0,
                },
            ],
            references: vec![],
            attributes: vec![],
        };
        let mut saved = rgpre::import::savegame::SaveFlags {
            values: vec![17, -2],
            states: vec![0, 3],
        };
        let snapshot = GlobalFlagsSnapshot::from_original(&definition, &saved)?;
        let mut flags = Flags::prepare(&definition)?;
        let mut values = vec![0, 0];
        flags.restore(&mut values, snapshot)?;
        assert_eq!(values, [17, -2]);
        assert!(!flags.is_set(0) && !flags.is_set(1));
        saved.states[0] = 255;
        assert!(
            GlobalFlagsSnapshot::from_original(&definition, &saved)
                .unwrap_err()
                .to_string()
                .contains("unsupported")
        );
        saved.states.pop();
        assert!(
            GlobalFlagsSnapshot::from_original(&definition, &saved)
                .unwrap_err()
                .to_string()
                .contains("count differs")
        );
        Ok(())
    }
}
