//! Actor-owned wait tokens apply preservation's elapsed and unpaused-slot predicates.
use anyhow::{Context, Result};
use cyrus_assets::script::CallKind;
use cyrus_script::{Argument, CallResult, Invocation};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Wait {
    Duration,
    Tasks,
    Blocking,
}
impl Wait {
    fn active(self, arguments: &[Argument], invocation: Invocation) -> Result<bool> {
        Ok(match self {
            Self::Duration => {
                let [duration] = arguments else {
                    anyhow::bail!("Wait requires one argument")
                };
                invocation
                    .elapsed
                    .context("Wait requires explicit elapsed input")?
                    < duration.integer()?.wrapping_sub(1)
            }
            Self::Tasks | Self::Blocking => {
                anyhow::ensure!(
                    arguments.is_empty(),
                    "task-count wait requires no arguments"
                );
                match self {
                    Self::Tasks => invocation.unpaused_tasks > 1,
                    _ => invocation.unpaused_blocking_tasks > 1,
                }
            }
        })
    }
}

#[derive(Default)]
pub(super) struct Waits {
    next: u64,
    tasks: BTreeMap<u64, Wait>,
}
impl Waits {
    pub fn call(
        &mut self,
        function: &str,
        kind: CallKind,
        arguments: &[Argument],
        invocation: Invocation,
    ) -> Result<Option<CallResult>> {
        let wait = if function.eq_ignore_ascii_case("Wait") {
            Wait::Duration
        } else if function.eq_ignore_ascii_case("WaitOnTasks") {
            Wait::Tasks
        } else if function.eq_ignore_ascii_case("WaitNonMulti") {
            Wait::Blocking
        } else {
            return Ok(None);
        };
        anyhow::ensure!(
            kind != CallKind::Function,
            "function-encoded {function} timing unsupported"
        );
        // Initial native completion does not skip slot allocation.
        wait.active(arguments, invocation)?;
        self.next = self
            .next
            .checked_add(1)
            .context("wait task identity exhausted")?;
        self.tasks.insert(self.next, wait);
        Ok(Some(CallResult::Pending(self.next)))
    }
    pub fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
    ) -> Result<bool> {
        let wait = *self
            .tasks
            .get(&token)
            .with_context(|| format!("wait task {token} absent"))?;
        let completed = !wait.active(arguments, invocation)?;
        if completed {
            self.tasks.remove(&token);
        }
        Ok(completed)
    }
    pub fn cancel(&mut self, token: u64) -> Result<()> {
        self.tasks
            .remove(&token)
            .with_context(|| format!("wait task {token} absent"))?;
        Ok(())
    }
}
