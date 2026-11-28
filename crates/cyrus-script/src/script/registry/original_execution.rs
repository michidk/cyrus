//! Original interpreter and native continuations share one validated ownership census.
use super::*;
use crate::script::original_save::{
    OriginalActorBinding, OriginalCodeAllocation, OriginalCodeBinding, OriginalOwnerContinuation,
    OriginalShellBinding,
};
use crate::script::tasks::{NativeTaskBindings, NativeTaskCommandState, TaskWord};
use cyrus_soup::{TaskSource, persistence::ContinuationTask};
use rgpre::import::savegame::{
    SaveCodeOwner, SaveObject, SaveScriptLocals, SaveShell, SaveTaskSlot,
};

/// Complete execution populations for one original world. Shared session services
/// must be restored off-live before constructing this component replacement.
pub struct OriginalExecutionOwners<'a> {
    pub actors: &'a [SaveObject],
    pub codes: &'a [SaveCodeOwner],
    pub shells: &'a [SaveShell],
    pub locals: &'a SaveScriptLocals,
    /// Original map identities in independently initialized native map order.
    pub walk_map_ids: &'a [u32],
    /// Exact slot mapping owned by the enclosing stopped replacement audio service.
    pub ambient_channels: &'a BTreeMap<u8, u8>,
}

impl ScriptWorld {
    /// Bind and replace every interpreter, callback and native task together.
    ///
    /// The installed allocation census determines receivers. Saved traversal order
    /// never selects a code owner. All VM cursors, native handlers, invocation words,
    /// navigation owners and cleared dialogue bindings validate before mutation.
    /// This must run on the enclosing session's off-live world; it does not publish
    /// a partial session or reconstruct shared camera/audio/global state.
    pub fn restore_original_execution(
        &mut self,
        saved: OriginalExecutionOwners<'_>,
        scripts: &Scripts,
        animations: &Animations,
    ) -> Result<()> {
        anyhow::ensure!(
            self.world == scripts.world && self.content_id == scripts.content_id,
            "world {} original execution source differs",
            self.world
        );
        anyhow::ensure!(
            animations.world == self.world
                && animations.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} original execution animation source differs",
            self.world
        );
        let mut programs = BTreeMap::new();
        let mut operands = BTreeMap::new();
        for definition in scripts.actors.values() {
            let origin = CodeOrigin(definition.code_origin);
            let program = Arc::new(definition.program.clone());
            if let Some(previous) = programs.insert(origin, program.clone()) {
                anyhow::ensure!(previous == program, "aliased original program differs");
            }
            for (&instruction, step) in &program.steps {
                let (call, object_call) = match &step.operation {
                    Operation::Call { call } => (call, false),
                    Operation::ObjectCall { call } => (call, true),
                    _ => continue,
                };
                if call.kind == CallKind::Function {
                    continue;
                }
                let source = TaskSource {
                    origin,
                    instruction,
                    object_call,
                    thread: Thread::Main,
                };
                let operand = source.operand()?;
                if let Some(previous) = operands.insert(operand, source) {
                    anyhow::ensure!(previous == source, "ambiguous original callback operand");
                }
            }
        }
        let tasks = |slots: [SaveTaskSlot; 6]| -> Result<[Option<ContinuationTask>; 6]> {
            let mut tasks = [None; 6];
            for (index, slot) in slots.into_iter().enumerate() {
                if slot.callback != 0 {
                    tasks[index] = Some(ContinuationTask {
                        token: index as u64 + 1,
                        source: *operands.get(&slot.callback).with_context(|| {
                            format!(
                                "world {} slot {index}: installed callback absent",
                                self.world
                            )
                        })?,
                    });
                }
            }
            Ok(tasks)
        };
        let allocations = OriginalCodeAllocation::bind(scripts, saved.codes)?;
        let maps = animations.metadata["walk_node_maps"]
            .as_array()
            .context("original execution initialized walk maps absent")?;
        anyhow::ensure!(
            maps.len() == saved.walk_map_ids.len(),
            "original execution walk-map allocation census differs"
        );
        let map_ids = saved.walk_map_ids;
        let dialogue = self
            .globals
            .dialogue
            .as_ref()
            .context("original execution cleared dialogue service absent")?;
        dialogue.validate_original_load()?;
        let mut replacements = Vec::new();
        let mut receivers = BTreeSet::new();
        let mut actor_owners = BTreeSet::new();
        for object in saved.actors {
            anyhow::ensure!(
                actor_owners.insert(object.object_id),
                "duplicate original actor owner"
            );
            let matches = self
                .actors
                .iter()
                .enumerate()
                .filter(|(_, actor)| match &actor.source {
                    ScriptActorSource::Player => object.object_id == -2,
                    ScriptActorSource::AlternatePlayer => object.object_id == -3,
                    ScriptActorSource::Camera => object.object_id == 2,
                    ScriptActorSource::Map(placement) => {
                        !placement.object_type.has_separate_record()
                            && object.object_id >= 0
                            && actor
                                .runtime_identifier
                                .is_some_and(|id| id as u16 == object.runtime_identifier())
                    }
                    _ => false,
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            anyhow::ensure!(
                matches.len() == 1,
                "original actor {} receiver is absent or ambiguous",
                object.object_id
            );
            let index = matches[0];
            anyhow::ensure!(
                receivers.insert(index),
                "duplicate original execution receiver"
            );
            let actor = &self.actors[index];
            actor
                .control
                .as_ref()
                .context("original actor native controller absent")?;
            let definition = scripts
                .actors
                .get(actor.name())
                .context("original actor definition absent")?;
            let continuation = if object.script_continuation().data == 0 {
                anyhow::ensure!(
                    object.task_slots().iter().all(|slot| slot.callback == 0),
                    "original nonscript actor retains callback"
                );
                None
            } else {
                Some(OriginalOwnerContinuation::decode(
                    object,
                    saved.locals,
                    OriginalActorBinding {
                        object_id: object.object_id,
                        runtime_identifier: object.runtime_identifier(),
                        origin: CodeOrigin(definition.code_origin),
                        tasks: tasks(object.task_slots())?,
                    },
                    &programs,
                )?)
            };
            let navigation =
                super::super::ActorNavigation::from_original(object.navigation(), map_ids)?;
            replacements.push((index, continuation, Some(navigation)));
        }
        for owner in saved.codes {
            let allocation = allocations
                .iter()
                .find(|a| a.owner_id == owner.owner_id)
                .context("original code allocation absent")?;
            let index = allocation.placement_index;
            anyhow::ensure!(receivers.insert(index), "duplicate original code receiver");
            let definition = &scripts.actors[self.actors[index].name()];
            let continuation = OriginalOwnerContinuation::decode_code(
                owner,
                saved.locals,
                OriginalCodeBinding {
                    owner_id: owner.owner_id,
                    origin: CodeOrigin(definition.code_origin),
                    tasks: tasks(owner.task_slots())?,
                },
                &programs,
            )?;
            replacements.push((index, Some(continuation), None));
        }
        let mut shell_names = BTreeSet::new();
        let mut extra_shells = Vec::new();
        for owner in saved.shells {
            let name = std::str::from_utf8(owner.name())?.trim_end_matches('\0');
            anyhow::ensure!(shell_names.insert(name), "duplicate original shell {name}");
            let indices = self
                .actors
                .iter()
                .enumerate()
                .filter_map(|(index, actor)| {
                    matches!(&actor.source, ScriptActorSource::Shell(shell) if shell == name)
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            anyhow::ensure!(
                indices.len() <= 1,
                "original shell {name} receiver ambiguous"
            );
            let index = if let Some(&index) = indices.first() {
                index
            } else {
                let index = self.actors.len() + extra_shells.len();
                let template = self
                    .shell_templates
                    .get(name)
                    .context("original shell source absent")?;
                extra_shells.push(ScriptActor::shell(self.world, index, name, Some(template)));
                index
            };
            anyhow::ensure!(receivers.insert(index), "duplicate original shell receiver");
            let definition = scripts
                .actors
                .get(name)
                .context("original shell definition absent")?;
            let continuation = OriginalOwnerContinuation::decode_shell(
                owner,
                OriginalShellBinding {
                    name: owner.name().try_into()?,
                    origin: CodeOrigin(definition.code_origin),
                    tasks: tasks(owner.task_slots())?,
                },
                &programs,
            )?;
            replacements.push((index, Some(continuation), None));
        }
        let expected = self
            .actors
            .iter()
            .filter(|actor| match &actor.source {
                ScriptActorSource::Map(placement) => {
                    !placement.object_type.has_separate_record()
                        || placement.object_type == PlacementType::SCRIPT_ONLY
                }
                _ => true,
            })
            .count();
        anyhow::ensure!(
            receivers.len() == expected + extra_shells.len(),
            "world {} original execution owner census differs: {} of {expected}; absent {:?}",
            self.world,
            receivers.len(),
            self.actors
                .iter()
                .enumerate()
                .filter(|(index, actor)| !receivers.contains(index)
                    && match &actor.source {
                        ScriptActorSource::Map(placement) =>
                            !placement.object_type.has_separate_record()
                                || placement.object_type == PlacementType::SCRIPT_ONLY,
                        _ => true,
                    })
                .map(|(index, actor)| (index, actor.name(), format!("{:?}", actor.source)))
                .collect::<Vec<_>>()
        );
        let actor_at = |index: usize| {
            if index < self.actors.len() {
                &self.actors[index]
            } else {
                &extra_shells[index - self.actors.len()]
            }
        };
        let replacements = replacements
            .into_iter()
            .map(|(index, continuation, navigation)| -> Result<_> {
                let Some(continuation) = continuation else {
                    return Ok((
                        index,
                        None,
                        super::super::tasks::NativeTasks::default(),
                        Default::default(),
                        navigation,
                    ));
                };
                let bindings = continuation
                    .vm
                    .task_calls()?
                    .iter()
                    .map(|call| (call.token, call.token))
                    .collect();
                let words = continuation
                    .vm
                    .task_calls()?
                    .iter()
                    .map(|call| {
                        let slot = usize::try_from(call.token - 1)
                            .context("original token outside slot range")?;
                        Ok((
                            call.token,
                            *continuation
                                .invocation_words
                                .get(slot)
                                .context("original token slot absent")?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?;
                let native = super::super::tasks::NativeTasks::from_original_with_words(
                    &continuation.vm,
                    &words,
                    NativeTaskBindings {
                        animation: actor_at(index).animation.as_ref().map(|a| &a.playback),
                        dialogue: Some(dialogue),
                        ambient_channels: Some(saved.ambient_channels),
                    },
                )
                .with_context(|| {
                    format!(
                        "world {} actor {index} original native services",
                        self.world
                    )
                })?;
                let origin = CodeOrigin(scripts.actors[actor_at(index).name()].code_origin);
                let runtime = Runtime::from_snapshot(
                    origin,
                    continuation.vm,
                    &programs,
                    &bindings,
                    format!("world {} actor {index} original execution", self.world),
                )?;
                let commands = continuation.invocation_words.map(|words| {
                    let mut command = NativeTaskCommandState::default();
                    for (field, value) in [
                        TaskWord::Continuation,
                        TaskWord::PauseTicks,
                        TaskWord::ContinuationDirection,
                        TaskWord::PointCommand,
                    ]
                    .into_iter()
                    .zip(words)
                    {
                        command.set_word(field, value);
                    }
                    command
                });
                Ok((index, Some(runtime), native, commands, navigation))
            })
            .collect::<Result<Vec<_>>>()?;
        let authored_pool = super::authored_pool::AuthoredPool::imported(scripts, saved.locals)
            .with_context(|| format!("world {} SVRG authored pool import", self.world))?;
        // Every fallible owner/service binding has completed. No callback is run.
        for actor in extra_shells {
            let index = self.actors.len();
            self.globals
                .group_sync
                .register(index, actor.attributes[Attribute::Group]);
            self.globals
                .group_sync
                .register_user(index, actor.attributes[Attribute::User]);
            self.actors.push(actor);
            if let Some(spatial) = &mut self.spatial {
                spatial.push_absent();
            }
        }
        for (index, runtime, tasks, commands, navigation) in replacements {
            let actor = &mut self.actors[index];
            actor.runtime = runtime;
            actor.tasks = tasks;
            actor.native_commands = commands;
            actor.unavailable = None;
            if let Some(navigation) = navigation {
                actor.control.as_mut().unwrap().navigation = navigation;
            }
        }
        self.authored_pool = Some(authored_pool);
        if let Some(nodes) = &mut self.walk_nodes {
            nodes.reset();
        }
        Ok(())
    }
}
