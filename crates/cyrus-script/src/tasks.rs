//! Actor-wide task slots and ordered operand reevaluation from preservation's SOUP rules.
use super::{CallResult, Host, Runtime, Thread};
use anyhow::{Context, Result};
use cyrus_assets::script::{Call, CallKind, Operation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub token: u64,
    pub instruction: u32,
    pub owner: Thread,
    pub blocking: bool,
    pub paused: bool,
}
impl Runtime {
    pub fn active_tasks(&self) -> usize {
        self.state.tasks.iter().flatten().count()
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
        if let Some(error) = &self.state.failure {
            anyhow::bail!("{error}");
        }
        let result = self.poll_tasks(host, flags);
        if let Err(error) = result {
            let error = format!("{} task stage: {error:#}", self.context);
            self.state.failure = Some(error.clone());
            anyhow::bail!("{error}");
        }
        Ok(())
    }
    fn poll_tasks(&mut self, host: &mut dyn Host, flags: &mut [i32]) -> Result<()> {
        let program = Arc::clone(&self.program);
        for index in 0..self.state.tasks.len() {
            let Some(task) = self.state.tasks[index].clone() else {
                continue;
            };
            if task.paused {
                continue;
            }
            let completed = (|| {
                let step = program
                    .steps
                    .get(&task.instruction)
                    .context("task instruction absent")?;
                let Operation::Call { call } = &step.operation else {
                    anyhow::bail!("task instruction is not a call")
                };
                self.state.script_return = 0;
                let arguments = self.arguments(call, flags, host)?;
                host.poll(task.token, &arguments, flags)
                    .with_context(|| format!("native {}", call.function))
            })()
            .with_context(|| {
                format!(
                    "instruction {} thread {:?} token {}",
                    task.instruction, task.owner, task.token
                )
            })?;
            if completed {
                self.state.tasks[index] = None;
            }
        }
        Ok(())
    }
    pub(super) fn start_call(
        &mut self,
        call: &Call,
        cursor: u32,
        thread: Thread,
        host: &mut dyn Host,
        flags: &mut [i32],
    ) -> Result<bool> {
        if call.kind != CallKind::Function
            && self
                .state
                .tasks
                .iter()
                .flatten()
                .any(|task| task.instruction == cursor)
        {
            return Ok(false);
        }
        anyhow::ensure!(
            call.kind == CallKind::Function || call.target == cyrus_assets::script::Object::Me,
            "cross-actor task ownership unsupported"
        );
        let slot = self.state.tasks.iter().position(Option::is_none);
        anyhow::ensure!(
            call.kind == CallKind::Function || slot.is_some(),
            "actor task capacity exhausted"
        );
        let target = self.target(&call.target)?;
        let arguments = self.arguments(call, flags, host)?;
        match host.call(&target, call.kind, &call.function, &arguments, flags)? {
            CallResult::Complete(_) => Ok(false),
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
                    instruction: cursor,
                    owner: thread,
                    blocking,
                    paused: false,
                });
                Ok(blocking)
            }
        }
    }
    pub(super) fn control_tasks(
        &mut self,
        label: u32,
        operation: &Operation,
        host: &mut dyn Host,
    ) -> Result<()> {
        for task in &mut self.state.tasks {
            if task.as_ref().is_none_or(|task| task.instruction != label) {
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
