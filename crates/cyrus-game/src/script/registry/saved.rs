//! Cyrus script snapshots restore against matching installed definitions before world replacement.
use super::*;
use serde::{Deserialize, Serialize};

/// Mutable logical-world state. Installed programs are identified, not embedded.
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedScripts {
    world: u32,
    content_id: String,
    flags: Vec<i32>,
    latches: Vec<bool>,
    actors: Vec<SavedActor>,
    spatial: Option<crate::script::spatial::SpatialWorld>,
    activation: crate::script::ActivationState,
    selection: Option<(Option<[i32; 2]>, Vec<usize>)>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedActor {
    state: Option<cyrus_script::State>,
    attributes: Vec<u8>,
    control: Option<ActorControlState>,
    tasks: crate::script::tasks::NativeTasks,
    animation: Option<ScriptAnimation>,
    animation_failure: Option<String>,
}

impl ScriptWorld {
    /// Capture all implemented actor, task, flag, activation and spatial state.
    pub fn snapshot(&self) -> SavedScripts {
        SavedScripts {
            world: self.world,
            content_id: self.content_id.clone(),
            flags: self.flags.clone(),
            latches: self.flag_state.latches().to_vec(),
            actors: self
                .actors
                .iter()
                .map(|actor| SavedActor {
                    state: actor
                        .runtime
                        .as_ref()
                        .map(|runtime| runtime.state().clone()),
                    attributes: actor.attributes.clone(),
                    control: actor.control.clone(),
                    tasks: actor.tasks.clone(),
                    animation: actor.animation.clone(),
                    animation_failure: actor.animation_failure.clone(),
                })
                .collect(),
            spatial: self.spatial.clone(),
            activation: self.activation.clone(),
            selection: self
                .actor_lists
                .as_ref()
                .map(crate::script::order::ActorLists::snapshot),
        }
    }
}

impl SavedScripts {
    /// Build a replacement in isolation. Failure leaves the running world untouched.
    pub fn restore(
        self,
        scripts: &Scripts,
        animations: &Animations,
        generation: u64,
    ) -> Result<ScriptWorld> {
        anyhow::ensure!(
            self.world == scripts.world && self.content_id == scripts.content_id,
            "world {}: saved script definitions differ from the installed world",
            self.world
        );
        let mut world = ScriptWorld::prepare(scripts, generation)?;
        world.prepare_markers(animations)?;
        anyhow::ensure!(
            self.flags.len() == world.flags.len() && self.actors.len() == world.actors.len(),
            "world {}: saved flag or actor count mismatch",
            self.world
        );
        world.flag_state.restore_latches(self.latches)?;
        world.flags = self.flags;
        if let Some(spatial) = &self.spatial {
            spatial.validate_saved(&world.actors)?;
        }
        if let Some((first, indices)) = self.selection {
            let mut lists = crate::script::order::ActorLists::new(&world.actors)?;
            lists.restore_selection(first, indices, generation, world.actors.len())?;
            world.actor_lists = Some(lists);
        }
        for (index, (actor, saved)) in world.actors.iter_mut().zip(self.actors).enumerate() {
            (|| -> Result<()> {
                anyhow::ensure!(
                    saved.attributes.len() == actor.attributes.len()
                        && saved.control.is_some() == actor.control.is_some(),
                    "saved actor layout mismatch"
                );
                match (&mut actor.runtime, saved.state) {
                    (Some(runtime), Some(state)) => {
                        runtime.restore(state)?;
                        for (_, source) in runtime.task_tokens() {
                            anyhow::ensure!(
                                world
                                    .task_calls
                                    .contains_key(&(source.origin, source.instruction)),
                                "saved task source absent from installed programs"
                            );
                        }
                        saved
                            .tasks
                            .validate_saved(runtime.task_tokens().map(|(token, _)| token))?;
                    }
                    (None, None) => saved.tasks.validate_saved(std::iter::empty())?,
                    _ => anyhow::bail!("saved actor program membership mismatch"),
                }
                if let Some(animation) = &saved.animation {
                    animation
                        .playback
                        .validate_saved(animations, actor.name())?;
                }
                actor.attributes = saved.attributes;
                actor.control = saved.control;
                actor.tasks = saved.tasks;
                actor.animation = saved.animation;
                actor.animation_failure = saved.animation_failure;
                Ok(())
            })()
            .with_context(|| {
                format!("world {} saved actor {index} {}", self.world, actor.name())
            })?;
        }
        world.spatial = self.spatial;
        world.activation = self.activation;
        world
            .activation
            .restore_generation(generation, world.actors.len())?;
        Ok(world)
    }
}
