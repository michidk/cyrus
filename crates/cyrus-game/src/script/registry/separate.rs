//! Separate map records retain scripts/tasks and rebuild temporary actor attributes.
use super::{ScriptActorId, ScriptInputs, ScriptStage, ScriptWorld};
use anyhow::{Context, Result, ensure};

impl ScriptWorld {
    /// Separate-record selection shares the cached view window and cell order.
    /// Activation does not grant persistent membership; ordinary border filters
    /// do not apply. Type-six light eligibility remains unsupported by its caller.
    pub fn separate_actors(&mut self, view: [i32; 2]) -> Result<&[ScriptActorId]> {
        self.ordinary_actors(view)?;
        Ok(self
            .actor_lists
            .as_ref()
            .expect("actor lists prepared")
            .separate_selected())
    }

    /// Run the admitted type-two script/task traversal after ordinary actors.
    /// Each invocation clears temporary attributes/control, while position, locals,
    /// continuations and task state remain with the separate record. Type-six light
    /// state and deferred separate-list relocation are not implemented here.
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
            let placement = actor.placement().context("separate map placement absent")?;
            if placement.object_type == 6 {
                actor.update_failure = Some(format!(
                    "world {} instance {} actor {}: separate light eligibility/lifecycle unavailable",
                    self.world,
                    id.index,
                    actor.name()
                ));
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
