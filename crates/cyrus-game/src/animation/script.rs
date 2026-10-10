//! Explicit main-script stages from preservation's SOUP and animation contracts.
use super::{ActorAnimation, Animations};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize, Serialize)]
pub(crate) struct Program {
    pub version: u32,
    pub entry: u32,
    pub steps: BTreeMap<u32, Step>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Step {
    pub next: u32,
    pub operation: Operation,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Operation {
    Task {
        function: String,
        arguments: Vec<i32>,
    },
    End {
        target: u32,
    },
    Unsupported {
        reason: String,
    },
}

struct BlockingTask {
    group: u16,
    count: u8,
    started: bool,
}

/// A literal PushAnimation/End main program, scheduled explicitly by its caller.
/// This owns no clock, activation, interrupt handler, or ordinary actor update.
pub struct AnimationScript {
    context: String,
    program: Program,
    cursor: u32,
    task: Option<BlockingTask>,
    failure: Option<String>,
}

impl Animations {
    /// Loads the exported authored program. Interrupt entries and synchronization
    /// channels remain unsupported; older caches must be re-exported for script inputs.
    pub fn animation_script(&self, actor: &str) -> anyhow::Result<AnimationScript> {
        self.require_version()?;
        let context = format!("world {} actor {actor} script", self.world);
        let metadata = self.actor(actor)?;
        if let Some(attributes) = metadata.get("attributes") {
            let attributes = attributes
                .as_object()
                .with_context(|| format!("{context}: invalid attributes"))?;
            for attribute in [29, 30] {
                if let Some(value) = attributes.get(&format!("attr_{attribute}")) {
                    let value = value
                        .as_u64()
                        .with_context(|| format!("{context}: invalid attribute {attribute}"))?;
                    anyhow::ensure!(
                        value == 0,
                        "{context}: synchronization channels unsupported"
                    );
                }
            }
        }
        let descriptor = self.metadata["animation_scripts"]
            .as_object()
            .and_then(|scripts| {
                scripts
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(actor))
            })
            .map(|(_, descriptor)| descriptor)
            .with_context(|| format!("{context}: program absent; re-export the world"))?;
        let program: Program = serde_json::from_value(descriptor.clone())
            .with_context(|| format!("{context}: invalid program descriptor"))?;
        anyhow::ensure!(
            program.version == 1,
            "{context}: unsupported program version {}",
            program.version
        );
        anyhow::ensure!(program.entry == 0, "{context}: interrupt entry unsupported");
        anyhow::ensure!(
            program.steps.contains_key(&program.entry),
            "{context}: entry instruction absent"
        );
        for (&offset, step) in &program.steps {
            match step.operation {
                Operation::Task { .. } => {
                    anyhow::ensure!(
                        step.next > offset && program.steps.contains_key(&step.next),
                        "{context} instruction {offset}: invalid task continuation {}",
                        step.next
                    );
                }
                Operation::End { target } => {
                    anyhow::ensure!(
                        program.steps.contains_key(&target),
                        "{context} instruction {offset}: End target {target} is not an instruction"
                    );
                }
                Operation::Unsupported { .. } => {}
            }
        }
        Ok(AnimationScript {
            cursor: program.entry,
            context,
            program,
            task: None,
            failure: None,
        })
    }
}

impl AnimationScript {
    /// Saved main continuation, including the authored target after End yields.
    pub fn program_counter(&self) -> u32 {
        self.cursor
    }

    /// The current blocking task's group; completion does not restart that task.
    pub fn waiting_for_group(&self) -> Option<u16> {
        self.task.as_ref().map(|task| task.group)
    }

    /// Runs one explicitly eligible main-script stage after ordinary advancement.
    /// Existing tasks run first; completed tasks permit the next instruction in this
    /// stage. End stores its target and yields until another call. Immediate requests
    /// reuse `ticks`, returning the latest mesh reference without inventing an update.
    pub fn advance(
        &mut self,
        animation: &mut ActorAnimation,
        ticks: u32,
    ) -> anyhow::Result<Option<u32>> {
        if let Some(message) = &self.failure {
            anyhow::bail!("{message}");
        }
        let result = self.advance_stage(animation, ticks);
        if let Err(error) = &result {
            let message = format!("{} instruction {}: {error:#}", self.context, self.cursor);
            self.failure = Some(message.clone());
            anyhow::bail!("{message}");
        }
        result
    }

    fn advance_stage(
        &mut self,
        animation: &mut ActorAnimation,
        ticks: u32,
    ) -> anyhow::Result<Option<u32>> {
        if let Some(task) = &mut self.task {
            if task.started {
                if animation.current_group() == Some(task.group) {
                    return Ok(None);
                }
                self.task = None;
            } else {
                return task.invoke(animation, ticks);
            }
        }
        let step = self
            .program
            .steps
            .get(&self.cursor)
            .context("instruction absent")?;
        match &step.operation {
            Operation::Task {
                function,
                arguments,
            } => {
                anyhow::ensure!(
                    function.eq_ignore_ascii_case("PushAnimation"),
                    "task {function} unsupported"
                );
                let [group, count] = arguments.as_slice() else {
                    anyhow::bail!("PushAnimation requires two arguments");
                };
                let group =
                    u16::try_from(*group).context("PushAnimation group outside supported range")?;
                anyhow::ensure!(
                    group <= i16::MAX as u16,
                    "PushAnimation signed group unsupported"
                );
                let mut task = BlockingTask {
                    group,
                    count: *count as u8,
                    started: false,
                };
                let frame = task.invoke(animation, ticks)?;
                self.task = Some(task);
                self.cursor = step.next;
                Ok(frame)
            }
            Operation::End { target } => {
                self.cursor = *target;
                Ok(None)
            }
            Operation::Unsupported { reason } => anyhow::bail!("{reason}"),
        }
    }
}

impl BlockingTask {
    fn invoke(
        &mut self,
        animation: &mut ActorAnimation,
        ticks: u32,
    ) -> anyhow::Result<Option<u32>> {
        if animation.current_group() == Some(self.group) {
            animation.set_loop_count(self.count)?;
            self.started = true;
            Ok(None)
        } else {
            animation.request(self.group, 0, ticks)
        }
    }
}
