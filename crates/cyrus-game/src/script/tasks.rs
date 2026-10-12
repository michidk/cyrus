//! Actor-owned native task tokens share waits and animation requests.
use anyhow::{Context, Result};
use cyrus_assets::script::CallKind;
use cyrus_script::{Argument, CallResult, Invocation};
use cyrus_world::animation::{ActorAnimation, AnimationSound};
use std::collections::BTreeMap;

/// One explicit invocation's animation and synchronous sound destination.
pub(super) struct AnimationCall<'a, 'sound> {
    pub animation: &'a mut ActorAnimation,
    pub ticks: u32,
    pub frame: &'a mut Option<u32>,
    pub sounds: Option<&'sound mut dyn FnMut(AnimationSound) -> Result<()>>,
}

#[derive(Clone, Copy)]
enum Task {
    Duration,
    Tasks,
    Blocking,
    Animation { group: u16, started: bool },
}
impl Task {
    fn complete(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
    ) -> Result<bool> {
        Ok(match self {
            Self::Duration => {
                let [duration] = arguments else {
                    anyhow::bail!("Wait requires one argument")
                };
                invocation
                    .elapsed
                    .context("Wait requires explicit elapsed input")?
                    >= duration.integer()?.wrapping_sub(1)
            }
            Self::Tasks | Self::Blocking => {
                anyhow::ensure!(
                    arguments.is_empty(),
                    "task-count wait requires no arguments"
                );
                match self {
                    Self::Tasks => invocation.unpaused_tasks <= 1,
                    _ => invocation.unpaused_blocking_tasks <= 1,
                }
            }
            Self::Animation { group, started } => {
                let [requested, count] = arguments else {
                    anyhow::bail!("PushAnimation requires two arguments")
                };
                let requested = u16::try_from(requested.integer()?)
                    .context("PushAnimation group outside supported range")?;
                anyhow::ensure!(
                    requested <= i16::MAX as u16,
                    "PushAnimation signed group unsupported"
                );
                let count = count.integer()? as u8;
                let input = animation.as_mut().context(
                    "PushAnimation requires prepared animation and explicit nominal ticks",
                )?;
                *group = requested;
                if *started {
                    input.animation.current_group() != Some(requested)
                } else {
                    if input.animation.current_group() == Some(requested) {
                        input.animation.set_loop_count(count)?;
                        *started = true;
                    } else {
                        let frame = match &mut input.sounds {
                            Some(sounds) => input.animation.request_with_sounds(
                                requested,
                                0,
                                input.ticks,
                                *sounds,
                            )?,
                            None => input.animation.request(requested, 0, input.ticks)?,
                        };
                        *input.frame = frame.or(*input.frame);
                    }
                    false
                }
            }
        })
    }
}

#[derive(Default)]
pub(super) struct NativeTasks {
    next: u64,
    tasks: BTreeMap<u64, Task>,
}
impl NativeTasks {
    pub fn call(
        &mut self,
        function: &str,
        kind: CallKind,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
    ) -> Result<Option<CallResult>> {
        let mut task = if function.eq_ignore_ascii_case("Wait") {
            Task::Duration
        } else if function.eq_ignore_ascii_case("WaitOnTasks") {
            Task::Tasks
        } else if function.eq_ignore_ascii_case("WaitNonMulti") {
            Task::Blocking
        } else if function.eq_ignore_ascii_case("PushAnimation") {
            Task::Animation {
                group: 0,
                started: false,
            }
        } else {
            return Ok(None);
        };
        anyhow::ensure!(
            kind != CallKind::Function,
            "function-encoded {function} timing unsupported"
        );
        let next = self
            .next
            .checked_add(1)
            .context("native task identity exhausted")?;
        // Initial completion still allocates a slot and yields until a later task pass.
        task.complete(arguments, invocation, animation)?;
        self.next = next;
        self.tasks.insert(next, task);
        Ok(Some(CallResult::Pending(next)))
    }
    pub fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
    ) -> Result<bool> {
        let task = self
            .tasks
            .get_mut(&token)
            .with_context(|| format!("native task {token} absent"))?;
        let completed = task.complete(arguments, invocation, animation)?;
        if completed {
            self.tasks.remove(&token);
        }
        Ok(completed)
    }
    pub fn cancel(&mut self, token: u64) -> Result<()> {
        self.tasks
            .remove(&token)
            .with_context(|| format!("native task {token} absent"))?;
        Ok(())
    }
    pub fn animation_group(&self) -> Option<u16> {
        self.tasks.values().find_map(|task| match task {
            Task::Animation { group, .. } => Some(*group),
            _ => None,
        })
    }
}
