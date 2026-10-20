//! Separate map records retain scripts/tasks and rebuild temporary actor attributes.
use super::{ScriptActorId, ScriptInputs, ScriptStage, ScriptWorld};
use anyhow::{Result, ensure};

impl ScriptWorld {
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
        self.actors.push(actor);
        if let Some(spatial) = &mut self.spatial {
            spatial.push_absent();
        }
        self.id(index)
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
        random: &mut crate::random::EngineRandom,
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
            actor.attributes.fill(0);
            actor.control = Some(Default::default());
            actor.surface = Some(0);
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
            actor.attributes.fill(0);
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
        random: &mut crate::random::EngineRandom,
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
            let actor = &mut self.actors[id.index];
            if actor.failure().is_some() || actor.unavailable().is_some() {
                continue;
            }
            actor.attributes.fill(0);
            actor.control = Some(Default::default());
            actor.surface = Some(0);
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
                self.actors[id.index].maintain_light(inputs.nominal_ticks.unwrap());
                Ok(())
            })();
            let actor = &mut self.actors[id.index];
            actor.attributes.fill(0);
            actor.control = None;
            actor.surface = None;
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
