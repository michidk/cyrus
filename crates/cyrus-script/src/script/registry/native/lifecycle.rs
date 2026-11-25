//! Lifecycle native-call dispatch.
use super::super::{
    ActorControlState, AiType, Argument, CallResult, GameHost, JumpMotion, Native, Result,
    ScriptActorId,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_lifecycle(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if function == Native::Resurrect {
            anyhow::ensure!(arguments.is_empty(), "Resurrect requires no arguments");
            self.registry.release_held_walk_nodes(id)?;
            let resurrected = {
                let actor = &mut self.registry.actors[self.index];
                let control = actor
                    .control
                    .as_mut()
                    .context("Resurrect: receiver lifecycle unavailable")?;
                if control.killed || control.ai_type == AiType::DEAD {
                    control.killed = false;
                    self.registry.lifecycle.revive(self.index);
                    // A selector-6 receiver with combat state first runs the unconditional part
                    // of the end-of-combat reset; the script-control bit stays (it is only
                    // cleared there while the killed flag is still set).
                    if control.ai_type == AiType::COMBAT {
                        let script_control = control.script_control;
                        crate::script::combat::end_combat_reset(control);
                        control.script_control = script_control;
                    }
                    if let Some(profile) =
                        control.combat.as_ref().map(|record| record.contact_profile)
                    {
                        control.combat = crate::script::CombatControlState::initialize(
                            &actor.attributes,
                            control.combat.as_ref(),
                            &mut self.registry.next_combat_index,
                            &self.registry.combat_attacks,
                            profile,
                        )
                        .context("Resurrect: combat reinitialization")?;
                    }
                    true
                } else {
                    false
                }
            };
            // The original consumes one waiting class message as part of the combat
            // resurrection path.  Keep transport separate from combat response: a
            // message for another receiver is skipped by the queue's addressed cursor.
            if resurrected
                && self.registry.actors[self.index]
                    .control
                    .as_ref()
                    .is_some_and(|control| control.combat.is_some())
            {
                let _ = self.registry.consume_combat_message(id)?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::KillMe {
            anyhow::ensure!(arguments.is_empty(), "KillMe requires no arguments");
            self.registry.release_held_walk_nodes(id)?;
            let _ = self.registry.actors[self.index]
                .control
                .as_ref()
                .context("KillMe: receiver lifecycle unavailable")?;
            self.registry.set_actor_ai_type(self.index, AiType::DEAD);
            self.registry.actors[self.index]
                .control
                .as_mut()
                .expect("validated KillMe control")
                .killed = true;
            self.registry.actors[self.index].model.current = None;
            self.registry.actors[self.index].model.weapon = None;
            // The original frees the receiver's light and flat records, so a later call starts new ones.
            self.registry.actors[self.index].light = None;
            self.registry.actors[self.index].flat = None;
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id,
                crate::script::ScriptSound::StopOwned,
            )?;
            // The VM cancels the receiver's runtime task slots after this function returns, while
            // its runtime is still borrowed by the invocation.
            return Ok(Some(CallResult::CompleteAfterClearingTasks(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_animal(
        &mut self,
        function: Native,
    ) -> Result<Option<CallResult>> {
        // Animal ignores its arguments and returns zero.
        if function == Native::Animal {
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_dead_query(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::IsDead {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(Some(CallResult::Complete(i32::from(
                self.actor_is_dead(self.index)?,
            ))));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_lifecycle_query(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // Receiver lifecycle bytes: MyAIType is the lifecycle selector, MyAIMode the death
        // notification byte; IsOnEdge holds for the ledge living states.
        let control_test: Option<fn(&ActorControlState) -> i32> = match function {
            Native::MyAiType => Some(|control| i32::from(control.ai_type.byte())),
            Native::MyAiMode => Some(|control| i32::from(control.ai_mode)),
            Native::IsOnEdge => Some(|control| i32::from(control.living_state.is_ledge())),
            // The original also answers 0 while its transition lock (global word read by
            // dozens of routines) is set; cyrus has no transition state, so it is clear.
            Native::IsInAir => Some(|control| {
                i32::from(
                    control.sliding_state != 0
                        || control.falling_state != JumpMotion::NONE
                        || control.queued_motion != JumpMotion::NONE
                        || control.living_state.is_actor_bounce(),
                )
            }),
            _ => None,
        };

        if let Some(test) = control_test {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_ref()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            return Ok(Some(CallResult::Complete(test(control))));
        }
        Ok(None)
    }
}
