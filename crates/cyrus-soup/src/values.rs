//! Typed actor references and original integer formula and condition evaluation.
use super::{Argument, Host, Runtime, ScriptValue};
use crate::program::{Call, CallKind, Condition, Formula, Value};
use anyhow::{Context, Result};

impl Runtime {
    pub(super) fn arguments(
        &mut self,
        call: &Call,
        flags: &mut [i32],
        host: &mut dyn Host,
        strings: &[String],
    ) -> Result<Vec<Argument>> {
        let mut arguments = call
            .arguments
            .iter()
            .rev()
            .map(|value| self.value(value, flags, host, strings))
            .collect::<Result<Vec<_>>>()?;
        arguments.reverse();
        Ok(arguments)
    }
    pub(super) fn value(
        &mut self,
        value: &Value,
        flags: &mut [i32],
        host: &mut dyn Host,
        strings: &[String],
    ) -> Result<Argument> {
        Ok(match value {
            Value::Integer { value } => Argument::Integer(*value),
            Value::String { index } => {
                Argument::String(host.native_string_parameter(*index, strings)?)
            }
            Value::Read { place } => self.read(place, flags, host)?.into(),
            Value::Update { place, delta } => {
                let value = self
                    .read(place, flags, host)?
                    .integer()?
                    .wrapping_add(*delta);
                self.write(place, ScriptValue::Integer(value), flags, host)?;
                Argument::Integer(value)
            }
            Value::Call { call } | Value::ObjectCall { call } => {
                anyhow::ensure!(
                    call.kind == CallKind::Function,
                    "value call is not a function"
                );
                let site = if matches!(value, Value::ObjectCall { .. }) {
                    super::NativeCallSite::ObjectFunctionValue
                } else {
                    super::NativeCallSite::FunctionValue
                };
                let target = self.target(&call.target)?;
                // A stale reference from an earlier statement call must not reach this value.
                host.take_actor_result();
                let value = match host.route(&target, call, strings, site, flags)? {
                    super::Route::Here => self.receive_inner(call, strings, site, host, flags)?.0,
                    super::Route::Complete(value) => value,
                };
                if let Some(actor) = host.take_actor_result() {
                    return Ok(Argument::Actor(actor));
                }
                Argument::Integer(value.with_context(|| {
                    format!("native {} has no supported numeric result", call.function)
                })?)
            }
        })
    }
    pub(super) fn formula(
        &mut self,
        formula: &Formula,
        flags: &mut [i32],
        host: &mut dyn Host,
    ) -> Result<ScriptValue> {
        let program = std::sync::Arc::clone(&self.program);
        let mut result = self
            .value(&formula.first, flags, host, &program.strings)?
            .scalar()?;
        for (operator, value) in &formula.rest {
            let lhs = result.integer()?;
            let rhs = self
                .value(value, flags, host, &program.strings)?
                .integer()?;
            result = ScriptValue::Integer(match operator {
                1 => lhs.wrapping_add(rhs),
                2 => lhs.wrapping_sub(rhs),
                3 => lhs.wrapping_mul(rhs),
                4 => (lhs as u32)
                    .checked_div(rhs as u32)
                    .context("SOUP division by zero")? as i32,
                5 => lhs.wrapping_shl(rhs as u32),
                6 => lhs.wrapping_shr(rhs as u32),
                7 => lhs & rhs,
                8 => lhs | rhs,
                9 => lhs ^ rhs,
                _ => anyhow::bail!("invalid formula operator {operator}"),
            });
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
        let program = std::sync::Arc::clone(&self.program);
        let mut result = false;
        let mut previous = 0;
        for (index, condition) in conditions.iter().enumerate() {
            let lhs = self
                .value(&condition.lhs, flags, host, &program.strings)?
                .scalar()?;
            let rhs = self
                .value(&condition.rhs, flags, host, &program.strings)?
                .scalar()?;
            let value = match condition.comparison {
                0 => lhs == rhs,
                1 => lhs != rhs,
                2 => lhs.integer()? < rhs.integer()?,
                3 => lhs.integer()? > rhs.integer()?,
                4 => lhs.integer()? <= rhs.integer()?,
                5 => lhs.integer()? >= rhs.integer()?,
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
