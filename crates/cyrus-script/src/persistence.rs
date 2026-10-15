//! Validated VM snapshots; game hosts restore actor identities and native task state separately.
use crate::{
    CodeOrigin, Runtime, State,
    program::{CallKind, Operation, Program},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A versioned VM snapshot including exact immutable source programs for compatibility.
/// Contains no world/actor identity, gameplay globals or native task implementation state.
/// Save it only as part of its owning actor/world document, outside the installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    version: u32,
    origin: CodeOrigin,
    programs: BTreeMap<CodeOrigin, Program>,
    state: State,
}

impl Runtime {
    /// Captures all VM fields and the foreign programs referenced by active task slots.
    /// `sources` contains installed programs keyed in the same RASC offset domain.
    /// This performs no native calls, polling, operand evaluation or cancellation.
    pub fn snapshot(&self, sources: &BTreeMap<CodeOrigin, Arc<Program>>) -> Result<Snapshot> {
        let mut programs = BTreeMap::from([(self.origin, self.program.as_ref().clone())]);
        for task in self.state.tasks.iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                programs.entry(task.source.origin)
            {
                let program = sources.get(&task.source.origin).with_context(|| {
                    format!("snapshot task source {} unavailable", task.source.origin.0)
                })?;
                entry.insert(program.as_ref().clone());
            }
        }
        let snapshot = Snapshot {
            version: 1,
            origin: self.origin,
            programs,
            state: self.state.clone(),
        };
        snapshot
            .validate()
            .with_context(|| format!("{} snapshot", self.context))?;
        Ok(snapshot)
    }

    /// Builds a replacement VM after validating its complete saved state and source content.
    /// `sources` must contain the owning and foreign installed programs. `tokens` maps
    /// every saved token to an already prepared token owned by this actor's new generation.
    /// Bindings must be complete and injective. No Host method or saved instruction runs.
    /// The caller validates actor/world identity and prepares native task state before
    /// calling; commit the returned VM only with the rest of that validated world.
    pub fn from_snapshot(
        origin: CodeOrigin,
        snapshot: Snapshot,
        sources: &BTreeMap<CodeOrigin, Arc<Program>>,
        tokens: &BTreeMap<u64, u64>,
        context: String,
    ) -> Result<Self> {
        let restore = || -> Result<Self> {
            ensure!(snapshot.origin == origin, "owning code origin mismatch");
            snapshot.validate()?;
            for (origin, saved) in &snapshot.programs {
                let installed = sources
                    .get(origin)
                    .with_context(|| format!("code source {} unavailable", origin.0))?;
                ensure!(
                    installed.as_ref() == saved,
                    "code source {} content mismatch",
                    origin.0
                );
            }
            let mut state = snapshot.state;
            let count = state.tasks.iter().flatten().count();
            ensure!(tokens.len() == count, "task token binding count mismatch");
            let mut restored_tokens = BTreeSet::new();
            for (slot, task) in state.tasks.iter_mut().enumerate() {
                let Some(task) = task else { continue };
                let token = *tokens.get(&task.token).with_context(|| {
                    format!("task slot {slot} token {} has no binding", task.token)
                })?;
                ensure!(
                    restored_tokens.insert(token),
                    "task slot {slot} duplicates restored token {token}"
                );
                task.token = token;
            }
            Ok(Self {
                program: Arc::clone(sources.get(&origin).context("owning program unavailable")?),
                origin,
                context: context.clone(),
                state,
            })
        };
        restore().with_context(|| format!("{context} restore"))
    }
}

impl Snapshot {
    /// Saved actor-owned tokens in slot order, for preparing explicit host bindings.
    /// Native task state must come from the same owning actor/world snapshot.
    pub fn task_tokens(&self) -> impl Iterator<Item = u64> + '_ {
        self.state.tasks.iter().flatten().map(|task| task.token)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported VM snapshot version {}",
            self.version
        );
        let program = self
            .programs
            .get(&self.origin)
            .context("owning program absent")?;
        for (origin, program) in &self.programs {
            program
                .validate()
                .with_context(|| format!("code source {}", origin.0))?;
        }
        ensure!(
            self.state.variables.len() == program.variables.len(),
            "local variable count mismatch"
        );
        for (index, thread) in self.state.threads.iter().enumerate() {
            ensure!(
                thread.is_some() == (index == 0 || program.entry != 0),
                "thread {index} continuation presence mismatch"
            );
            if let Some(thread) = thread {
                ensure!(
                    program.steps.contains_key(&thread.cursor),
                    "thread {index} cursor {} is not an instruction",
                    thread.cursor
                );
            }
        }
        if let Some(cursor) = self.state.return_to {
            ensure!(
                program.steps.contains_key(&cursor),
                "return cursor {cursor} is not an instruction"
            );
        }
        let mut tokens = BTreeSet::new();
        let mut operands = BTreeSet::new();
        let mut origins = BTreeSet::from([self.origin]);
        for (slot, task) in self.state.tasks.iter().enumerate() {
            let Some(task) = task else { continue };
            let mut result = || -> Result<()> {
                origins.insert(task.source.origin);
                let source = self
                    .programs
                    .get(&task.source.origin)
                    .context("source program absent")?;
                let step = source
                    .steps
                    .get(&task.source.instruction)
                    .context("source instruction absent")?;
                let (call, object_call) = match &step.operation {
                    Operation::Call { call } => (call, false),
                    Operation::ObjectCall { call } => (call, true),
                    _ => anyhow::bail!("source instruction is not a call"),
                };
                ensure!(
                    task.source.object_call == object_call,
                    "source operand kind mismatch"
                );
                ensure!(
                    call.kind != CallKind::Function,
                    "source instruction is a function"
                );
                ensure!(
                    task.blocking == (call.kind == CallKind::Task),
                    "blocking state mismatch"
                );
                ensure!(
                    task.operand == task.source.operand()?,
                    "operand identity mismatch"
                );
                ensure!(
                    tokens.insert(task.token),
                    "duplicate saved token {}",
                    task.token
                );
                ensure!(
                    operands.insert(task.operand),
                    "duplicate operand identity {}",
                    task.operand
                );
                Ok(())
            };
            result().with_context(|| format!("task slot {slot}"))?;
        }
        ensure!(
            origins.len() == self.programs.len(),
            "unreferenced source program"
        );
        Ok(())
    }
}
