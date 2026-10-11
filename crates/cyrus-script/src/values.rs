//! SOUP formula and condition evaluation in the original integer domain.
use super::{Argument, CallResult, Host, Runtime};
use anyhow::{Context, Result};
use cyrus_assets::script::{Call, CallKind, Condition, Formula, Value};

impl Runtime {
    pub(super) fn arguments(
        &mut self,
        call: &Call,
        flags: &mut [i32],
        host: &mut dyn Host,
    ) -> Result<Vec<Argument>> {
        let mut arguments = call
            .arguments
            .iter()
            .rev()
            .map(|value| self.value(value, flags, host))
            .collect::<Result<Vec<_>>>()?;
        arguments.reverse();
        Ok(arguments)
    }
    pub(super) fn value(
        &mut self,
        value: &Value,
        flags: &mut [i32],
        host: &mut dyn Host,
    ) -> Result<Argument> {
        Ok(match value {
            Value::Integer { value } => Argument::Integer(*value),
            Value::String { index } => Argument::String(
                self.program
                    .strings
                    .get(*index as usize)
                    .context("script string absent")?
                    .clone(),
            ),
            Value::Read { place } => Argument::Integer(self.read(place, flags, host)?),
            Value::Update { place, delta } => {
                let value = self.read(place, flags, host)?.wrapping_add(*delta);
                self.write(place, value, flags, host)?;
                Argument::Integer(value)
            }
            Value::Call { call } => {
                anyhow::ensure!(
                    call.kind == CallKind::Function,
                    "value call is not a function"
                );
                let target = self.target(&call.target)?;
                let args = self.arguments(call, flags, host)?;
                let CallResult::Complete(value) =
                    host.call(&target, CallKind::Function, &call.function, &args, flags)?
                else {
                    anyhow::bail!("function {} returned a blocking task", call.function);
                };
                Argument::Integer(value)
            }
        })
    }
    pub(super) fn formula(
        &mut self,
        formula: &Formula,
        flags: &mut [i32],
        host: &mut dyn Host,
    ) -> Result<i32> {
        let mut result = self.value(&formula.first, flags, host)?.integer()?;
        for (operator, value) in &formula.rest {
            let rhs = self.value(value, flags, host)?.integer()?;
            result = match operator {
                1 => result.wrapping_add(rhs),
                2 => result.wrapping_sub(rhs),
                3 => result.wrapping_mul(rhs),
                4 => (result as u32)
                    .checked_div(rhs as u32)
                    .context("SOUP division by zero")? as i32,
                5 => result.wrapping_shl(rhs as u32),
                6 => result.wrapping_shr(rhs as u32),
                7 => result & rhs,
                8 => result | rhs,
                9 => result ^ rhs,
                _ => anyhow::bail!("invalid formula operator {operator}"),
            };
        }
        Ok(result)
    }
    pub(super) fn condition(
        &mut self,
        conditions: &[Condition],
        flags: &mut [i32],
        host: &mut dyn Host,
    ) -> Result<bool> {
        anyhow::ensure!(!conditions.is_empty(), "condition list absent");
        let mut result = false;
        let mut previous = 0;
        for (index, condition) in conditions.iter().enumerate() {
            let lhs = self.value(&condition.lhs, flags, host)?.integer()?;
            let rhs = self.value(&condition.rhs, flags, host)?.integer()?;
            let value = match condition.comparison {
                0 => lhs == rhs,
                1 => lhs != rhs,
                2 => lhs < rhs,
                3 => lhs > rhs,
                4 => lhs <= rhs,
                5 => lhs >= rhs,
                comparison => anyhow::bail!("invalid comparison {comparison}"),
            };
            result = match (index, previous) {
                (0, _) => value,
                (_, 1) => result & value,
                (_, 2) => result | value,
                _ => anyhow::bail!("invalid conjunction {previous}"),
            };
            previous = condition.conjunction;
        }
        anyhow::ensure!(previous == 0, "condition list is not terminated");
        Ok(result)
    }
}
