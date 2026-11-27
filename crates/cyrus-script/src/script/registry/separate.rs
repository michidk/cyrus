//! Separate map records retain scripts/tasks and rebuild temporary actor attributes.
use super::{ScriptActorId, ScriptInputs, ScriptStage, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_assets::surface::Surface;
use cyrus_soup::Attribute;

impl ScriptWorld {
    /// Local parameters use the receiver's live pool; keep the retained source's
    /// exposed view current when that receiver writes an overlapping range.
    pub(super) fn refresh_retained_source_locals(&mut self, index: usize) {
        let actor = &self.actors[index];
        let Some((base, runtime)) = actor
            .pool_offsets
            .and_then(|p| p.locals)
            .zip(actor.runtime.as_ref())
        else {
            return;
        };
        let Some(source) = self.globals.separate_native_source.as_mut() else {
            return;
        };
        let Some(source_base) = source.pool_offsets.and_then(|p| p.locals) else {
            return;
        };
        for (slot, value) in source.locals.iter_mut().enumerate() {
            let Some(offset) = u32::try_from(slot)
                .ok()
                .and_then(|n| n.checked_mul(4))
                .and_then(|n| source_base.checked_add(n))
            else {
                continue;
            };
            if let Some(receiver_slot) = offset
                .checked_sub(base)
                .map(|n| n / 4)
                .and_then(|n| usize::try_from(n).ok())
                && let Some(current) = runtime.variables().get(receiver_slot)
            {
                *value = *current;
            }
        }
    }
    /// Resolve only an active conversion alias, never retained provenance after Load.
    pub(super) fn active_separate_native_source(&self) -> Option<usize> {
        let source = self.globals.separate_native_source.as_ref()?.semantic_actor;
        (source.generation == self.generation
            && self.actors.get(source.index).is_some_and(|actor| {
                actor.control.is_some()
                    && actor
                        .placement()
                        .is_some_and(|placement| placement.object_type.has_separate_record())
            }))
        .then_some(source.index)
    }

    /// Copy the temporary source before its semantic record loses scratch state.
    /// No interpreter entry is implied: an existing Separate source observes the
    /// next conversion's contents even when that conversion runs only a hook.
    pub(super) fn retain_separate_native_source(&mut self, index: usize) {
        let actor = &self.actors[index];
        if !actor
            .placement()
            .is_some_and(|placement| placement.object_type.has_separate_record())
        {
            return;
        }
        let Some(runtime) = actor.runtime.as_ref() else {
            return;
        };
        let local = self
            .spatial
            .as_ref()
            .and_then(|spatial| spatial.actor(index).ok())
            .map(|spatial| super::super::activation::NativeSourcePose {
                position: spatial.local.position,
                angles: spatial.local.angles,
                matrix: spatial.local.matrix.0.map(|row| row.map(f32::to_bits)),
            });
        self.globals.separate_native_source =
            Some(super::super::activation::SeparateNativeSource {
                semantic_actor: super::ScriptActorId {
                    generation: self.generation,
                    index,
                },
                world: self.world,
                name: actor.name().to_owned(),
                attributes: actor.attributes.clone(),
                pool_offsets: actor.pool_offsets,
                locals: runtime.variables().to_vec(),
                strings: runtime.shared_program().strings.clone(),
                position: self
                    .id(index)
                    .ok()
                    .and_then(|id| self.query_position(id).ok()),
                local,
                model_position: actor
                    .model
                    .position
                    .map(|position| position.map(f32::to_bits)),
            });
    }
    /// Start a shell script from its installed definition, as item use and shell-creating natives do.
    /// The new shell runs from the next update, newest first. Fails when the world has no such script.
    pub fn spawn_shell(&mut self, name: &str) -> Result<ScriptActorId> {
        let index = self.actors.len();
        // Script names compare without case: ITEM.INI and scripts spell them differently.
        let installed = self
            .shell_templates
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name));
        let Some((key, template)) = installed else {
            anyhow::bail!("world {}: shell script {name} not installed", self.world)
        };
        let actor = super::ScriptActor::shell(self.world, index, key, Some(template));
        let channel = actor.attributes()[Attribute::Group];
        self.actors.push(actor);
        self.globals.group_sync.register(index, channel);
        self.globals
            .group_sync
            .register_user(index, self.actors[index].attributes()[Attribute::User]);
        if let Some(spatial) = &mut self.spatial {
            spatial.push_absent();
        }
        self.id(index)
    }

    /// Create an object from an installed script after load (`shoot`, `shootPlayer`,
    /// `DragonBreath`). Its position is `position`, its orientation `angles`, and the creator is
    /// recorded as its parent. It runs its script from the next update, newest first, and keeps
    /// its own attributes and control state until `KillMe`. Fails when the world has no such
    /// script.
    pub fn create_object(
        &mut self,
        creator: ScriptActorId,
        name: &str,
        position: [i32; 3],
        angles: [u16; 3],
    ) -> Result<ScriptActorId> {
        let creator_index = self.resolve(creator)?;
        let index = self.actors.len();
        let installed = self
            .shell_templates
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name));
        let Some((key, template)) = installed else {
            anyhow::bail!("world {}: script {name} not installed", self.world)
        };
        // Validate every failure that `push_created` can report before appending the
        // actor record. Allocation is a paired actor/spatial operation; leaving an
        // actor behind when spatial admission rejects its pose would shift all later
        // ScriptActorId indices and make the registry internally inconsistent.
        anyhow::ensure!(
            angles.iter().all(|angle| *angle < 2048),
            "created object angles outside wrapped range"
        );
        let spatial = self
            .spatial
            .as_ref()
            .context("created objects need spatial state")?;
        anyhow::ensure!(
            spatial.actor(creator_index).is_ok(),
            "created object creator has no spatial state"
        );
        let mut actor = super::ScriptActor::created(self.world, index, key, template);
        let channel = actor.attributes()[Attribute::Group];
        actor
            .control
            .as_mut()
            .expect("created objects allocate control")
            .creator = Some(creator_index);
        self.actors.push(actor);
        self.globals.group_sync.register(index, channel);
        self.globals
            .group_sync
            .register_user(index, self.actors[index].attributes()[Attribute::User]);
        self.spatial
            .as_mut()
            .expect("created-object spatial state validated above")
            .push_created(&self.actors[index], position, angles, creator_index)?;
        // Created records register an installed namespace under runtime identifier zero. The
        // original keeps the first registration when that identifier is already present.
        if self.actors[index].namespace.is_some() {
            self.names.entry(0).or_insert(index);
        }
        self.id(index)
    }

    /// Add a created object's actor record ahead of a restore that overwrites its state: the
    /// saved spatial and actor snapshots replace everything but the installed script identity.
    pub(super) fn restore_created_placeholder(&mut self, name: &str) -> Result<()> {
        let index = self.actors.len();
        let installed = self
            .shell_templates
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name));
        let Some((key, template)) = installed else {
            anyhow::bail!("world {}: script {name} not installed", self.world)
        };
        let actor = super::ScriptActor::created(self.world, index, key, template);
        let channel = actor.attributes()[Attribute::Group];
        let has_namespace = actor.namespace.is_some();
        self.actors.push(actor);
        self.globals.group_sync.register(index, channel);
        self.globals
            .group_sync
            .register_user(index, self.actors[index].attributes()[Attribute::User]);
        // Created records retain runtime identifier zero and the first namespace
        // registration, just like post-load creation. Restore must rebuild this
        // identity before ActorSnapshot validation and name-target resolution.
        if has_namespace {
            self.names.entry(0).or_insert(index);
        }
        if let Some(spatial) = &mut self.spatial {
            spatial.push_absent();
        }
        Ok(())
    }

    /// Run the created objects, newest first, after the shells and before the ordinary actors.
    /// An object keeps its attributes and control state between updates; its script runs in
    /// ordinary context, and `KillMe` ends it for good.
    pub fn update_created(
        &mut self,
        inputs: ScriptInputs<'_>,
        random: &mut cyrus_game::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
        budget: usize,
    ) -> Result<()> {
        ensure!(
            inputs.nominal_ticks.is_some() && inputs.frame_scale.is_some(),
            "created-object update requires nominal_ticks and frame_scale"
        );
        let created: Vec<_> = (0..self.actors.len())
            .rev()
            .filter(|&index| {
                matches!(
                    self.actors[index].source,
                    super::ScriptActorSource::Created(_)
                )
            })
            .collect();
        for index in created {
            let id = self.id(index)?;
            let actor = &mut self.actors[index];
            if actor.control.as_ref().is_some_and(|control| control.killed) {
                actor.runtime = None;
                actor.created_retired = true;
                continue;
            }
            if actor.runtime.is_none() || actor.failure().is_some() {
                continue;
            }
            self.snapshot_spatial(id)?;
            let result = (|| -> Result<()> {
                if !self.actors[index]
                    .control
                    .as_ref()
                    .is_some_and(|control| control.hidden)
                {
                    self.actors[index].maintain_light(inputs.nominal_ticks.unwrap(), random)?;
                    self.actors[index].maintain_flat(inputs.nominal_ticks.unwrap());
                }
                self.advance(
                    id,
                    ScriptStage::OrdinaryHook,
                    inputs,
                    Some(sounds),
                    Some(random),
                    budget,
                )?;
                if self.updates.task_system
                    && !self.actors[index]
                        .control
                        .as_ref()
                        .is_some_and(|control| control.killed)
                {
                    self.advance(
                        id,
                        ScriptStage::OrdinaryMain,
                        inputs,
                        Some(sounds),
                        Some(random),
                        budget,
                    )?;
                }
                Ok(())
            })();
            let actor = &mut self.actors[index];
            if actor.control.as_ref().is_some_and(|control| control.killed) {
                actor.runtime = None;
                actor.created_retired = true;
            }
            if let Err(error) = result {
                actor.update_failure = Some(format!(
                    "world {} created object {} update: {error:#}",
                    self.world,
                    actor.name()
                ));
            }
        }
        Ok(())
    }

    /// The shell list in the original's link order (newest record first): the name of each running
    /// shell. A shell leaves the list when it finishes.
    pub fn shell_list(&self) -> impl Iterator<Item = &str> {
        self.actors
            .iter()
            .rev()
            .filter(|actor| {
                matches!(actor.source, super::ScriptActorSource::Shell(_))
                    && actor.runtime.is_some()
            })
            .map(super::ScriptActor::name)
    }

    /// `CountAiShells` and `FindAiShell` over the shell list. The query is the base file name
    /// without directory or extension, at most eight characters, upper case. Each entry's name
    /// is copied with nine bytes, so a name of eight characters also compares the low byte of the
    /// entry's next link with zero: the list tail (null link) matches, while another entry's
    /// result depends on a heap address and is an explicit error. `first` stops at the first match.
    pub(super) fn count_shell_matches(&self, query: &str, first: bool) -> Result<i32> {
        let stem: String = super::model::resource_stem(query);
        let names: Vec<String> = self
            .shell_list()
            .map(|name| {
                name.chars()
                    .take(8)
                    .collect::<String>()
                    .to_ascii_uppercase()
            })
            .collect();
        let mut matches = 0;
        for (position, name) in names.iter().enumerate() {
            if *name != stem {
                continue;
            }
            if stem.chars().count() == 8 && position + 1 != names.len() {
                anyhow::bail!(
                    "shell {name}: the nine-byte entry comparison reads the low byte of its next link, which has no recovered value"
                );
            }
            matches += 1;
            if first {
                break;
            }
        }
        Ok(matches)
    }

    /// Running shells of one script: the count an item's instance limit is checked against.
    pub fn running_shells(&self, name: &str) -> usize {
        self.actors
            .iter()
            .filter(|actor| {
                matches!(&actor.source, super::ScriptActorSource::Shell(shell) if shell.eq_ignore_ascii_case(name))
                    && actor.runtime.is_some()
            })
            .count()
    }

    /// Run the shell scripts, newest first, before the ordinary actors. A shell has no map object:
    /// like a separate record it runs on cleared temporary attributes and control, with its locals,
    /// continuation and tasks retained, and in shell context. `KillMe` ends it for good.
    pub fn update_shells(
        &mut self,
        inputs: ScriptInputs<'_>,
        random: &mut cyrus_game::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
        budget: usize,
    ) -> Result<()> {
        ensure!(
            inputs.nominal_ticks.is_some() && inputs.frame_scale.is_some(),
            "shell update requires nominal_ticks and frame_scale"
        );
        let inputs = ScriptInputs {
            shell_execution: Some(true),
            ..inputs
        };
        let shells: Vec<_> = (0..self.actors.len())
            .rev()
            .filter(|&index| {
                matches!(
                    self.actors[index].source,
                    super::ScriptActorSource::Shell(_)
                )
            })
            .collect();
        for index in shells {
            let id = self.id(index)?;
            let actor = &mut self.actors[index];
            if actor.runtime.is_none() || actor.failure().is_some() {
                continue;
            }
            actor.attributes = Default::default();
            actor.control = Some(Default::default());
            actor.surface = Some(Surface::UNKNOWN);
            let result = (|| -> Result<()> {
                self.advance(
                    id,
                    ScriptStage::SeparateHook,
                    inputs,
                    Some(sounds),
                    Some(random),
                    budget,
                )?;
                if self.updates.task_system {
                    self.advance(
                        id,
                        ScriptStage::SeparateMain,
                        inputs,
                        Some(sounds),
                        Some(random),
                        budget,
                    )?;
                }
                Ok(())
            })();
            let actor = &mut self.actors[index];
            let finished = actor.control.as_ref().is_some_and(|control| control.killed);
            actor.attributes = Default::default();
            actor.control = None;
            actor.surface = None;
            if finished {
                actor.runtime = None;
            }
            if let Err(error) = result {
                actor.update_failure = Some(format!(
                    "world {} shell {} update: {error:#}",
                    self.world,
                    actor.name()
                ));
            }
        }
        Ok(())
    }

    /// Separate-record selection shares the cached view window and cell order.
    /// Activation does not grant persistent membership; ordinary border filters
    /// do not apply. Light state remains with its separate record.
    pub fn separate_actors(&mut self, view: [i32; 2]) -> Result<&[ScriptActorId]> {
        self.ordinary_actors(view)?;
        Ok(self
            .actor_lists
            .as_ref()
            .expect("actor lists prepared")
            .separate_selected())
    }

    /// Run the separate script/task traversal after ordinary actors.
    /// Each invocation clears temporary attributes/control, while position, locals,
    /// continuations and task state remain with the separate record. Light maintenance
    /// runs after script/task processing. Deferred separate-list relocation remains unsupported.
    pub fn update_separate(
        &mut self,
        view: [i32; 2],
        inputs: ScriptInputs<'_>,
        random: &mut cyrus_game::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
        budget: usize,
    ) -> Result<()> {
        ensure!(
            inputs.nominal_ticks.is_some() && inputs.frame_scale.is_some(),
            "separate update requires nominal_ticks and frame_scale"
        );
        if self.spatial.is_none() {
            self.prepare_spatial()?;
        }
        let selected = self.separate_actors(view)?.to_vec();
        for id in selected {
            // Every conversion starts from cleared temporary storage; only the
            // separate record's position is restored, not an ordinary pose matrix.
            let spatial = self
                .spatial
                .as_mut()
                .expect("prepared separate spatial state");
            let temporary = spatial.actor_mut(id.index)?;
            temporary.local.angles = [0; 3];
            temporary.local.matrix = cyrus_world::transform::Matrix([[0.0; 3]; 3]);
            let actor = &mut self.actors[id.index];
            if actor.runtime.is_none() || actor.failure().is_some() || actor.unavailable().is_some()
            {
                continue;
            }
            actor.attributes = Default::default();
            actor.control = Some(Default::default());
            actor.surface = Some(Surface::UNKNOWN);
            let result = (|| -> Result<()> {
                self.advance(
                    id,
                    ScriptStage::SeparateHook,
                    inputs,
                    Some(sounds),
                    Some(random),
                    budget,
                )?;
                if self.updates.task_system {
                    self.advance(
                        id,
                        ScriptStage::SeparateMain,
                        inputs,
                        Some(sounds),
                        Some(random),
                        budget,
                    )?;
                }
                self.actors[id.index].maintain_light(inputs.nominal_ticks.unwrap(), random)?;
                self.actors[id.index].maintain_flat(inputs.nominal_ticks.unwrap());
                Ok(())
            })();
            self.retain_separate_native_source(id.index);
            let actor = &mut self.actors[id.index];
            // Separate records use temporary lifecycle state, so retain KillMe's terminal
            // result before clearing that state for the next traversal.
            let finished = actor.control.as_ref().is_some_and(|control| control.killed);
            actor.attributes = Default::default();
            actor.control = None;
            actor.surface = None;
            if finished {
                actor.runtime = None;
            }
            if let Err(error) = result {
                actor.update_failure = Some(format!(
                    "world {} instance {} actor {} separate update: {error:#}",
                    self.world,
                    id.index,
                    actor.name()
                ));
            }
        }
        Ok(())
    }
}
