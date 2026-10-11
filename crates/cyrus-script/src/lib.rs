//! Explicit SOUP invocation, continuations and values from preservation's engine contract.
mod tasks;
mod values;
use anyhow::{Context, Result};
use cyrus_assets::script::{CallKind, Object, Operation, Place, Program};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A caller selects an eligible stage; this crate supplies no production clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Thread {
    Main,
    Interrupt,
}
impl Thread {
    fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Interrupt => 1,
        }
    }
}

/// Host targets retain semantic identities rather than renderer entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Me,
    Player,
    Camera,
    Named(String),
    Variable(i32),
}

/// Numeric bit patterns remain intact until a native handler interprets their units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Argument {
    Integer(i32),
    String(String),
}
impl Argument {
    pub fn integer(&self) -> Result<i32> {
        match self {
            Self::Integer(value) => Ok(*value),
            _ => anyhow::bail!("integer operand required"),
        }
    }
}

/// A host-issued token identifies one blocking operation, never an audio/entity handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallResult {
    Complete(i32),
    Pending(u64),
}

/// Immediate state changes occur synchronously. Unsupported services return an error.
/// Task tokens must remain valid only for their owning world/actor generation.
pub trait Host {
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        flags: &mut [i32],
    ) -> Result<CallResult>;
    /// Active task operands are evaluated again on each eligible task invocation.
    fn poll(&mut self, task: u64, arguments: &[Argument], flags: &mut [i32]) -> Result<bool>;
    /// Removes task bookkeeping without reversing already issued effects.
    fn cancel(&mut self, task: u64) -> Result<()> {
        anyhow::bail!("task {task} cancellation unsupported")
    }
    fn read_property(&mut self, target: &Target, reference: u8) -> Result<i32> {
        anyhow::bail!("property {target:?}.{reference} unsupported")
    }
    fn write_property(&mut self, target: &Target, reference: u8, _: i32) -> Result<()> {
        anyhow::bail!("property {target:?}.{reference} unsupported")
    }
    /// Performs the researched End notifications before the invocation yields.
    fn end(&mut self, _: Thread) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Continuation {
    cursor: u32,
}
/// Actor-local state is shared by its two continuations. Host tasks are owned separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    variables: Vec<i32>,
    return_to: Option<u32>,
    script_return: u8,
    saved_return: u8,
    anchor: u8,
    threads: [Option<Continuation>; 2],
    tasks: [Option<tasks::Task>; 6],
    failure: Option<String>,
}

/// Validated execution core. Invocations continue until a real wait or authored yield.
pub struct Runtime {
    program: Arc<Program>,
    context: String,
    state: State,
}

impl Runtime {
    pub fn new(program: Program, context: String) -> Result<Self> {
        let variables = program.variables.clone();
        Self::from_shared(Arc::new(program), variables, context)
    }
    /// Instances share immutable instructions while retaining separate authored locals.
    pub fn from_shared(
        program: Arc<Program>,
        variables: Vec<i32>,
        context: String,
    ) -> Result<Self> {
        program
            .validate()
            .map_err(|error| anyhow::anyhow!("{context}: {error:#}"))?;
        anyhow::ensure!(
            variables.len() == program.variables.len(),
            "{context}: local variable count mismatch"
        );
        let continuation = |cursor| Some(Continuation { cursor });
        let threads = [
            continuation(program.entry),
            if program.entry != 0 {
                continuation(0)
            } else {
                None
            },
        ];
        let state = State {
            variables,
            return_to: None,
            script_return: 0,
            saved_return: 0,
            anchor: 0,
            threads,
            tasks: Default::default(),
            failure: None,
        };
        Ok(Self {
            program,
            context,
            state,
        })
    }
    pub fn script_return(&self) -> u8 {
        self.state.script_return
    }
    pub fn set_script_return(&mut self, value: u8) {
        self.state.script_return = value;
    }
    pub fn anchor(&self) -> u8 {
        self.state.anchor
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn variables(&self) -> &[i32] {
        &self.state.variables
    }
    pub fn program_counter(&self, thread: Thread) -> Option<u32> {
        self.state.threads[thread.index()]
            .as_ref()
            .map(|state| state.cursor)
    }
    pub fn waiting(&self, thread: Thread) -> Option<u64> {
        self.state
            .tasks
            .iter()
            .flatten()
            .find(|task| task.owner == thread && task.blocking && !task.paused)
            .map(|task| task.token)
    }
    pub fn failure(&self) -> Option<&str> {
        self.state.failure.as_deref()
    }

    /// Runs an explicitly eligible stage. Budget exhaustion is a sticky failure,
    /// never an implicit yield or a gameplay timing rule.
    pub fn invoke(
        &mut self,
        thread: Thread,
        host: &mut dyn Host,
        flags: &mut [i32],
        budget: usize,
    ) -> Result<()> {
        if let Some(error) = &self.state.failure {
            anyhow::bail!("{error}");
        }
        if thread == Thread::Main {
            self.invoke_tasks(host, flags)?;
        }
        self.invoke_body(thread, host, flags, budget)
    }
    /// Runs a body without an existing-task pass. Main work waits for unpaused blocking slots.
    pub fn invoke_body(
        &mut self,
        thread: Thread,
        host: &mut dyn Host,
        flags: &mut [i32],
        budget: usize,
    ) -> Result<()> {
        if let Some(error) = &self.state.failure {
            anyhow::bail!("{error}");
        }
        let instruction = self
            .program_counter(thread)
            .with_context(|| format!("{} thread {thread:?}: continuation absent", self.context))?;
        let result = self.run(thread, host, flags, budget);
        if let Err(error) = result {
            let cursor = self.program_counter(thread).unwrap_or(instruction);
            let error = format!(
                "{} instruction {cursor} thread {thread:?}: {error:#}",
                self.context
            );
            self.state.failure = Some(error.clone());
            anyhow::bail!("{error}");
        }
        Ok(())
    }
    fn run(
        &mut self,
        thread: Thread,
        host: &mut dyn Host,
        flags: &mut [i32],
        budget: usize,
    ) -> Result<()> {
        let index = thread.index();
        if thread == Thread::Main && self.has_blocking_task() {
            return Ok(());
        }
        let program = Arc::clone(&self.program);
        for _ in 0..budget {
            let cursor = self.state.threads[index].as_ref().unwrap().cursor;
            let step = program.steps.get(&cursor).context("instruction absent")?;
            let mut next = step.next;
            let mut yield_now = false;
            match &step.operation {
                Operation::Assign {
                    destination,
                    formula,
                } => {
                    let value = self.formula(formula, flags, host)?;
                    self.write(destination, value, flags, host)?;
                }
                Operation::Increment { destination, delta } => {
                    let value = self.read(destination, flags, host)?.wrapping_add(*delta);
                    self.write(destination, value, flags, host)?;
                }
                Operation::If { conditions, target } => {
                    if !self.condition(conditions, flags, host)? {
                        next = *target;
                    }
                }
                Operation::Goto { target } => next = *target,
                Operation::Gosub { target } => {
                    self.state.return_to = Some(next);
                    next = *target;
                }
                Operation::Return => {
                    next = self.state.return_to.context("Return without Gosub")?;
                    self.state.script_return = self.state.saved_return;
                }
                Operation::End { target } => {
                    host.end(thread)?;
                    next = *target;
                    yield_now = true;
                }
                Operation::Endint => {
                    next = 0;
                    yield_now = true;
                }
                Operation::Call { call } => {
                    yield_now = self.start_call(call, cursor, thread, host, flags)?;
                }
                Operation::ObjectCall { call } => {
                    anyhow::ensure!(
                        call.kind == CallKind::Function,
                        "object task routing unsupported for {}",
                        call.function
                    );
                    self.start_call(call, cursor, thread, host, flags)?;
                }
                Operation::Anchor { value } => self.state.anchor = *value,
                Operation::Yield => yield_now = true,
                Operation::ScriptRv {
                    expected,
                    target,
                    save_return,
                } => {
                    if self.state.script_return != *expected {
                        next = *target;
                    } else if *save_return {
                        self.state.saved_return = self.state.script_return;
                    }
                }
                Operation::TaskStop { label }
                | Operation::TaskPause { label }
                | Operation::TaskResume { label } => {
                    self.control_tasks(*label, &step.operation, host)?
                }
                Operation::Unsupported { reason } => anyhow::bail!("{reason}"),
            }
            self.state.threads[index].as_mut().unwrap().cursor = next;
            if yield_now {
                return Ok(());
            }
        }
        anyhow::bail!("non-yielding script exceeded execution budget {budget}")
    }
    fn target(&self, object: &Object) -> Result<Target> {
        Ok(match object {
            Object::Me => Target::Me,
            Object::Player => Target::Player,
            Object::Camera => Target::Camera,
            Object::Named { string } => Target::Named(
                self.program
                    .strings
                    .get(*string as usize)
                    .context("object string absent")?
                    .clone(),
            ),
            Object::Local { index } => Target::Variable(
                *self
                    .state
                    .variables
                    .get(usize::from(*index))
                    .context("object local absent")?,
            ),
        })
    }
    fn read(&self, place: &Place, flags: &[i32], host: &mut dyn Host) -> Result<i32> {
        match place {
            Place::Local { index } => self
                .state
                .variables
                .get(usize::from(*index))
                .copied()
                .context("local variable absent"),
            Place::Flag { index } => flags
                .get(usize::from(*index))
                .copied()
                .context("global flag absent"),
            Place::Property { object, reference } => {
                host.read_property(&self.target(object)?, *reference)
            }
        }
    }
    fn write(
        &mut self,
        place: &Place,
        value: i32,
        flags: &mut [i32],
        host: &mut dyn Host,
    ) -> Result<()> {
        match place {
            Place::Local { index } => {
                *self
                    .state
                    .variables
                    .get_mut(usize::from(*index))
                    .context("local variable absent")? = value
            }
            Place::Flag { index } => {
                *flags
                    .get_mut(usize::from(*index))
                    .context("global flag absent")? = value
            }
            Place::Property { object, reference } => {
                host.write_property(&self.target(object)?, *reference, value)?
            }
        }
        Ok(())
    }
}
