//! Installed global flag types and native latch operations from preservation's SOUP rules.
use anyhow::{Context, Result};
use cyrus_script::Argument;
use cyrus_script::program::Definition;

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    FlipFlop,
    Counter,
    Number,
}
pub(super) struct Flags {
    kinds: Vec<Kind>,
    latched: Vec<bool>,
}
impl Flags {
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
            latched: vec![false; kinds.len()],
            kinds,
        })
    }
    pub fn call(
        &mut self,
        name: &str,
        arguments: &[Argument],
        values: &mut [i32],
    ) -> Result<Option<i32>> {
        let set = name.eq_ignore_ascii_case("SetGlobalFlag");
        let reset = name.eq_ignore_ascii_case("ResetGlobalFlag");
        let test = name.eq_ignore_ascii_case("TestGlobalFlag");
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
