//! Logical script instances retain authored object order and reject stale world identities.
use anyhow::{Context, Result};
use bevy::prelude::{Assets, DetectChanges, Handle, Res, ResMut, Resource};
use cyrus_assets::{
    animation::Animations,
    script::{Call, CallKind, Instance, Operation, Scripts},
};
use cyrus_script::{
    Argument, CallResult, CodeOrigin, Host, Route, Runtime, Target, TaskSource, Thread,
};
use cyrus_world::lifecycle::PreparedWorld;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Runtime identity is independent of render nodes and original object-name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ScriptActorId {
    pub generation: u64,
    pub index: usize,
}

pub struct ScriptActor {
    pub placement: Instance,
    pub namespace: Option<String>,
    runtime: Option<Runtime>,
    unavailable: Option<String>,
    attributes: Vec<u8>,
    waits: super::waits::Waits,
}
impl ScriptActor {
    pub fn runtime(&self) -> Option<&Runtime> {
        self.runtime.as_ref()
    }
    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
}

/// Explicit inspection state. Preparation does not grant ordinary update eligibility.
pub struct ScriptWorld {
    pub world: u32,
    pub content_id: String,
    generation: u64,
    flags: Vec<i32>,
    flag_state: super::flags::Flags,
    actors: Vec<ScriptActor>,
    task_calls: BTreeMap<(CodeOrigin, u32), Call>,
}
impl ScriptWorld {
    pub fn prepare(scripts: &Scripts, generation: u64) -> Result<Self> {
        scripts.validate()?;
        let programs: BTreeMap<_, _> = scripts
            .actors
            .iter()
            .map(|(name, actor)| (name, Arc::new(actor.program.clone())))
            .collect();
        let mut task_calls = BTreeMap::new();
        for actor in scripts.actors.values() {
            for (&instruction, step) in &actor.program.steps {
                if let Operation::Call { call } | Operation::ObjectCall { call } = &step.operation {
                    let key = (CodeOrigin(actor.code_origin), instruction);
                    if let Some(existing) = task_calls.insert(key, call.clone()) {
                        anyhow::ensure!(
                            existing == *call,
                            "world {}: aliased script call differs at origin {} instruction {instruction}",
                            scripts.world,
                            actor.code_origin
                        );
                    }
                }
            }
        }
        let mut allocations = BTreeMap::<&str, usize>::new();
        let actors = scripts
            .instances
            .iter()
            .enumerate()
            .map(|(index, placement)| {
                let context = format!(
                    "world {} instance {index} actor {} script",
                    scripts.world, placement.actor
                );
                let metadata = scripts.actors.get(&placement.actor);
                let result = (|| {
                    let actor = metadata.context("program absent")?;
                    let allocated = allocations.entry(&placement.actor).or_default();
                    let locals = actor
                        .instance_variables
                        .get(*allocated)
                        .context("authored instance allocation exceeded")?
                        .clone();
                    *allocated += 1;
                    Runtime::from_shared(
                        Arc::clone(&programs[&placement.actor]),
                        cyrus_script::CodeOrigin(actor.code_origin),
                        locals,
                        context.clone(),
                    )
                })();
                let (runtime, unavailable) = match result {
                    Ok(runtime) => (Some(runtime), None),
                    Err(error) => (None, Some(format!("{context}: {error:#}"))),
                };
                ScriptActor {
                    placement: placement.clone(),
                    namespace: metadata.and_then(|actor| actor.namespace.clone()),
                    runtime,
                    unavailable,
                    waits: Default::default(),
                    attributes: (0..256)
                        .map(|index| {
                            metadata
                                .and_then(|actor| {
                                    let key = scripts
                                        .definition
                                        .attributes
                                        .get(index)
                                        .filter(|name| !name.is_empty())
                                        .cloned()
                                        .unwrap_or_else(|| format!("attr_{index}"));
                                    actor.attributes.get(&key).copied()
                                })
                                .unwrap_or(0)
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(Self {
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            generation,
            flag_state: super::flags::Flags::prepare(&scripts.definition)
                .with_context(|| format!("world {} global flag initialization", scripts.world))?,
            flags: scripts
                .definition
                .flags
                .iter()
                .map(|flag| flag.initial_value)
                .collect(),
            actors,
            task_calls,
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Deterministic inspection of one body. This does not establish actor eligibility.
    pub fn step(&mut self, id: ScriptActorId, thread: Thread, budget: usize) -> Result<()> {
        self.execute(id, |runtime, host, flags| {
            runtime.invoke_body(thread, host, flags, budget)
        })
    }
    /// Explicit task timing only. Missing scale fails before changing any actor state.
    pub fn step_tasks(&mut self, id: ScriptActorId, frame_scale: Option<u32>) -> Result<()> {
        self.resolve(id)?;
        let frame_scale = frame_scale.with_context(|| {
            format!(
                "world {} instance {} task stage requires frame_scale",
                self.world, id.index
            )
        })?;
        self.execute(id, |runtime, host, flags| {
            runtime.invoke_tasks_timed(host, flags, frame_scale)
        })
    }
    fn execute(
        &mut self,
        id: ScriptActorId,
        run: impl FnOnce(&mut Runtime, &mut InspectionHost<'_>, &mut [i32]) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &mut self.actors[index];
        let mut runtime = actor.runtime.take().with_context(|| {
            actor
                .unavailable
                .clone()
                .unwrap_or_else(|| "program absent".into())
        })?;
        let mut host = InspectionHost {
            index,
            actors: &mut self.actors,
            task_calls: &self.task_calls,
            flag_state: &mut self.flag_state,
        };
        let result = run(&mut runtime, &mut host, &mut self.flags);
        host.actors[index].runtime = Some(runtime);
        result
    }
    fn resolve(&self, actor: ScriptActorId) -> Result<usize> {
        anyhow::ensure!(
            actor.generation == self.generation,
            "stale script actor generation {} in world {} generation {}",
            actor.generation,
            self.world,
            self.generation
        );
        anyhow::ensure!(
            actor.index < self.actors.len(),
            "world {} instance {} absent",
            self.world,
            actor.index
        );
        Ok(actor.index)
    }
    /// Bounded diagnostics carry logical state, never installed bytecode.
    pub fn inspect(&self, offset: usize, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "world": self.world, "generation": self.generation, "content_id": self.content_id, "total": self.actors.len(),
            "actors": self.actors.iter().enumerate().skip(offset).take(limit.min(64)).map(|(index, actor)| serde_json::json!({
                "id": ScriptActorId { generation: self.generation, index }, "name": actor.placement.actor,
                "object_id": actor.placement.object_id, "unavailable": actor.unavailable,
                "main": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Main)),
                "interrupt": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Interrupt)),
                "locals": actor.runtime.as_ref().map(|runtime| runtime.variables()),
                "tasks": actor.runtime.as_ref().map(|runtime| runtime.active_tasks()),
                "failure": actor.runtime.as_ref().and_then(|runtime| runtime.failure()),
            })).collect::<Vec<_>>()
        })
    }
    pub fn flags(&self) -> &[i32] {
        &self.flags
    }
    pub fn actors(&self) -> &[ScriptActor] {
        &self.actors
    }
    pub fn id(&self, index: usize) -> Result<ScriptActorId> {
        anyhow::ensure!(
            index < self.actors.len(),
            "world {} instance {index} absent",
            self.world
        );
        Ok(ScriptActorId {
            generation: self.generation,
            index,
        })
    }
    pub fn invoke(
        &mut self,
        actor: ScriptActorId,
        thread: Thread,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        self.resolve(actor)?;
        let actor = self
            .actors
            .get_mut(actor.index)
            .context("script instance absent")?;
        let runtime = actor.runtime.as_mut().with_context(|| {
            actor
                .unavailable
                .clone()
                .unwrap_or_else(|| "program absent".into())
        })?;
        runtime.invoke(thread, host, &mut self.flags, budget)
    }
}

#[derive(Resource, Default)]
pub struct ScriptActors {
    pub current: Option<ScriptWorld>,
    generation: u64,
    requested: Option<Handle<Scripts>>,
    pub(super) error: Option<String>,
}
pub(super) fn prepare(
    prepared: Res<PreparedWorld>,
    assets: Res<Assets<Scripts>>,
    animations: Res<Assets<Animations>>,
    mut actors: ResMut<ScriptActors>,
) {
    if prepared.is_changed() {
        actors.current = None;
        actors.error = None;
        actors.requested = Some(prepared.scripts.clone());
        let Some(generation) = actors.generation.checked_add(1) else {
            actors.error = Some("script world generation exhausted".into());
            actors.requested = None;
            return;
        };
        actors.generation = generation;
    }
    if actors.current.is_some() || actors.requested.as_ref() != Some(&prepared.scripts) {
        return;
    }
    let Some(scripts) = assets.get(&prepared.scripts) else {
        return;
    };
    let Some(animation_data) = animations.get(&prepared.animations) else {
        return;
    };
    let expected_world = prepared.world.unwrap_or(animation_data.world);
    let result = (|| {
        anyhow::ensure!(
            expected_world == scripts.world && animation_data.world == scripts.world,
            "world {} script identity differs from prepared world {}",
            scripts.world,
            expected_world
        );
        anyhow::ensure!(
            animation_data.metadata["script_content_id"].as_str() == Some(&scripts.content_id),
            "world {} animation/script cache generation mismatch; re-export the world",
            scripts.world
        );
        ScriptWorld::prepare(scripts, actors.generation)
    })();
    match result {
        Ok(world) => actors.current = Some(world),
        Err(error) => {
            actors.error = Some(format!(
                "world {} script preparation: {error:#}",
                scripts.world
            ));
            actors.requested = None;
        }
    }
}

struct InspectionHost<'a> {
    index: usize,
    actors: &'a mut [ScriptActor],
    task_calls: &'a BTreeMap<(CodeOrigin, u32), Call>,
    flag_state: &'a mut super::flags::Flags,
}
impl InspectionHost<'_> {
    fn target_index(&self, target: &Target) -> Result<usize> {
        let indices = match target {
            Target::Me => return Ok(self.index),
            Target::Named(name) => self
                .actors
                .iter()
                .enumerate()
                .filter(|(_, actor)| {
                    actor
                        .namespace
                        .as_ref()
                        .is_some_and(|namespace| namespace.eq_ignore_ascii_case(name))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>(),
            Target::Player => self
                .actors
                .iter()
                .enumerate()
                .filter(|(_, actor)| actor.placement.actor == "CYRUS")
                .map(|(index, _)| index)
                .collect::<Vec<_>>(),
            _ => anyhow::bail!("object target {target:?} unsupported"),
        };
        let [index] = indices.as_slice() else {
            anyhow::bail!(
                "object target {target:?}: {} matching receivers; runtime identifier ordering unavailable",
                indices.len()
            );
        };
        Ok(*index)
    }
}
impl Host for InspectionHost<'_> {
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        flags: &mut [i32],
    ) -> Result<CallResult> {
        anyhow::ensure!(
            target == &Target::Me,
            "inspection target {target:?} unsupported"
        );
        if let Some(result) = self.actors[self.index]
            .waits
            .call(function, kind, arguments, invocation)?
        {
            return Ok(result);
        }
        anyhow::ensure!(
            kind == CallKind::Function,
            "native {:?} {function} unsupported by inspection host",
            kind
        );
        if let Some(value) = self.flag_state.call(function, arguments, flags)? {
            return Ok(CallResult::Complete(value));
        }
        if function.eq_ignore_ascii_case("GetMyAttr") {
            let [index] = arguments else {
                anyhow::bail!("GetMyAttr requires one argument")
            };
            let value = usize::try_from(index.integer()?)
                .ok()
                .and_then(|index| self.actors[self.index].attributes.get(index))
                .copied()
                .unwrap_or(0);
            return Ok(CallResult::Complete(i32::from(value)));
        }
        anyhow::bail!(
            "native {:?} {function} unsupported by inspection host",
            kind
        )
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        self.actors[self.index]
            .waits
            .poll(token, arguments, invocation)
    }
    fn cancel(&mut self, token: u64) -> Result<()> {
        self.actors[self.index].waits.cancel(token)
    }
    fn route(
        &mut self,
        target: &Target,
        call: &Call,
        strings: &[String],
        source: Option<TaskSource>,
        flags: &mut [i32],
    ) -> Result<Route> {
        let index = self.target_index(target)?;
        if index == self.index {
            return Ok(Route::Here);
        }
        let actor = &mut self.actors[index];
        let mut runtime = actor.runtime.take().with_context(|| {
            format!(
                "receiver instance {index} {}: {}",
                actor.placement.actor,
                actor
                    .unavailable
                    .as_deref()
                    .unwrap_or("script runtime already active")
            )
        })?;
        let caller = std::mem::replace(&mut self.index, index);
        let result = runtime.receive(call, strings, source, self, flags);
        self.index = caller;
        self.actors[index].runtime = Some(runtime);
        result.map(Route::Complete)
    }
    fn task_call(&mut self, origin: CodeOrigin, instruction: u32) -> Result<Call> {
        self.task_calls
            .get(&(origin, instruction))
            .cloned()
            .with_context(|| format!("task source {} instruction {instruction} absent", origin.0))
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        anyhow::ensure!(
            self.actors[self.index].attributes[29] == 0
                && self.actors[self.index].attributes[30] == 0,
            "synchronization channels unsupported"
        );
        Ok(())
    }
}

pub(super) fn clear_unprepared(
    prepared: Option<Res<PreparedWorld>>,
    mut actors: ResMut<ScriptActors>,
    mut status: ResMut<super::ScriptStatus>,
) {
    if prepared.is_some() {
        return;
    }
    actors.current = None;
    actors.requested = None;
    actors.error = None;
    *status = super::ScriptStatus::NotRequested;
}
