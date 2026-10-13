//! Actor-wide task slots and ordered operand reevaluation from preservation's SOUP rules.
use super::{CallResult, Host, Invocation, Runtime, Target, TaskSource};
use anyhow::{Context, Result};
use cyrus_assets::script::{Call, CallKind, Operation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub token: u64,
    pub source: TaskSource,
    pub operand: u32,
    pub blocking: bool,
    pub paused: bool,
    pub counter: u16,
}
impl Runtime {
    pub fn active_tasks(&self) -> usize {
        self.state.tasks.iter().flatten().count()
    }
    /// Ordinary native hooks are gated by every unpaused slot, including multitasks.
    pub fn has_unpaused_task(&self) -> bool {
        self.state.tasks.iter().flatten().any(|task| !task.paused)
    }
    pub fn has_blocking_task(&self) -> bool {
        self.state
            .tasks
            .iter()
            .flatten()
            .any(|task| task.blocking && !task.paused)
    }
    /// Executes existing slots in allocation order. Newly issued tasks run in a later pass.
    pub fn invoke_tasks(&mut self, host: &mut dyn Host, flags: &mut [i32]) -> Result<()> {
        self.task_pass(host, flags, None)
    }
    /// Scale has eight fractional bits; elapsed is recomputed, never accumulated.
    pub fn invoke_tasks_timed(
        &mut self,
        host: &mut dyn Host,
        flags: &mut [i32],
        frame_scale: u32,
    ) -> Result<()> {
        self.task_pass(host, flags, Some(frame_scale))
    }
    fn task_pass(
        &mut self,
        host: &mut dyn Host,
        flags: &mut [i32],
        frame_scale: Option<u32>,
    ) -> Result<()> {
        if let Some(error) = &self.state.failure {
            anyhow::bail!("{error}");
        }
        let result = self.poll_tasks(host, flags, frame_scale);
        if let Err(error) = result {
            let error = format!("{} task stage: {error:#}", self.context);
            self.state.failure = Some(error.clone());
            anyhow::bail!("{error}");
        }
        Ok(())
    }
    pub(super) fn invocation(&self, elapsed: Option<i32>) -> Invocation {
        Invocation {
            elapsed,
            unpaused_tasks: self
                .state
                .tasks
                .iter()
                .flatten()
                .filter(|task| !task.paused)
                .count(),
            unpaused_blocking_tasks: self
                .state
                .tasks
                .iter()
                .flatten()
                .filter(|task| !task.paused && task.blocking)
                .count(),
        }
    }
    fn poll_tasks(
        &mut self,
        host: &mut dyn Host,
        flags: &mut [i32],
        frame_scale: Option<u32>,
    ) -> Result<()> {
        let program = Arc::clone(&self.program);
        for index in 0..self.state.tasks.len() {
            let Some(task) = self.state.tasks[index].clone() else {
                continue;
            };
            if task.paused {
                continue;
            }
            let completed = (|| {
                let call = if task.source.origin == self.origin {
                    let step = program
                        .steps
                        .get(&task.source.instruction)
                        .context("task instruction absent")?;
                    match &step.operation {
                        Operation::Call { call } | Operation::ObjectCall { call } => call.clone(),
                        _ => anyhow::bail!("task instruction is not a call"),
                    }
                } else {
                    host.task_call(task.source.origin, task.source.instruction)?
                };
                self.state.script_return = 0;
                let arguments = self.arguments(&call, flags, host, &program.strings)?;
                let elapsed = frame_scale
                    .map(|scale| (u32::from(task.counter).wrapping_mul(scale) >> 8) as i32);
                let invocation = self.invocation(elapsed);
                let result = host.poll(task.token, &arguments, invocation, flags);
                self.state.tasks[index].as_mut().unwrap().counter = task.counter.wrapping_add(1);
                result.with_context(|| format!("native {}", call.function))
            })()
            .with_context(|| {
                format!(
                    "instruction {} thread {:?} token {}",
                    task.source.instruction, task.source.thread, task.token
                )
            })?;
            if completed {
                self.state.tasks[index] = None;
            }
        }
        Ok(())
    }
    pub(super) fn receive_inner(
        &mut self,
        call: &Call,
        strings: &[String],
        source: Option<TaskSource>,
        host: &mut dyn Host,
        flags: &mut [i32],
    ) -> Result<(i32, bool)> {
        anyhow::ensure!(
            (call.kind == CallKind::Function) == source.is_none(),
            "native call/task source mismatch"
        );
        let operand = source.map(TaskSource::operand).transpose()?;
        if call.kind != CallKind::Function
            && self
                .state
                .tasks
                .iter()
                .flatten()
                .any(|task| Some(task.operand) == operand)
        {
            return Ok((0, false));
        }
        let slot = self.state.tasks.iter().position(Option::is_none);
        anyhow::ensure!(
            call.kind == CallKind::Function || slot.is_some(),
            "actor task capacity exhausted"
        );
        let arguments = self.arguments(call, flags, host, strings)?;
        let invocation = self.invocation((call.kind != CallKind::Function).then_some(0));
        match host.call(
            &Target::Me,
            call.kind,
            &call.function,
            &arguments,
            invocation,
            flags,
        )? {
            CallResult::Complete(value) => {
                anyhow::ensure!(
                    call.kind == CallKind::Function,
                    "native task {} must retain a token for its later completion pass",
                    call.function
                );
                Ok((value, false))
            }
            CallResult::Pending(token) => {
                anyhow::ensure!(
                    call.kind != CallKind::Function,
                    "pending function {} unsupported",
                    call.function
                );
                anyhow::ensure!(
                    !self
                        .state
                        .tasks
                        .iter()
                        .flatten()
                        .any(|task| task.token == token),
                    "duplicate task token {token}"
                );
                let blocking = call.kind == CallKind::Task;
                self.state.tasks[slot.context("task slot absent")?] = Some(Task {
                    token,
                    source: source.context("task source absent")?,
                    operand: operand.context("task operand absent")?,
                    blocking,
                    paused: false,
                    counter: 1,
                });
                Ok((0, blocking))
            }
        }
    }
    pub(super) fn control_tasks(
        &mut self,
        label: u32,
        operation: &Operation,
        host: &mut dyn Host,
    ) -> Result<()> {
        let operand = self
            .origin
            .0
            .checked_add(label)
            .and_then(|value| value.checked_add(1))
            .context("task label overflow")?;
        for task in &mut self.state.tasks {
            if task.as_ref().is_none_or(|task| task.operand != operand) {
                continue;
            }
            match operation {
                Operation::TaskStop { .. } => {
                    host.cancel(task.as_ref().unwrap().token)?;
                    *task = None;
                }
                Operation::TaskPause { .. } => task.as_mut().unwrap().paused = true,
                Operation::TaskResume { .. } => task.as_mut().unwrap().paused = false,
                _ => anyhow::bail!("invalid task control"),
            }
        }
        Ok(())
    }
    /// Cancels host task records before forgetting tokens. The game owns effect teardown.
    pub fn cancel_tasks(&mut self, host: &mut dyn Host) -> Result<()> {
        for task in &mut self.state.tasks {
            if let Some(active) = task {
                host.cancel(active.token)?;
                *task = None;
            }
        }
        Ok(())
    }
}
