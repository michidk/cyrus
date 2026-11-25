//! Combat native-call dispatch.
use super::super::{
    Argument, CallKind, CallResult, CombatEntryInputs, GameHost, Native, Result, ScriptActorId,
    WeaponInputs, animation_sound_position, effects, weapon,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_begin_combat(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::BeginCombat {
            anyhow::ensure!(arguments.is_empty(), "beginCombat requires no arguments");
            let inputs = match self.inputs.combat_entry {
                Some(inputs) => inputs,
                None => {
                    let selected_player = self
                        .selected_player()
                        .context("beginCombat selected player unavailable")?;
                    anyhow::ensure!(
                        id != selected_player,
                        "beginCombat selected-player death override unavailable"
                    );
                    let actor = &self.registry.actors[self.index];
                    anyhow::ensure!(
                        actor.model.prepared,
                        "beginCombat receiver model state unavailable"
                    );
                    CombatEntryInputs {
                        weapon: WeaponInputs {
                            selected_player,
                            item_system_ready: self
                                .inputs
                                .hud
                                .context("beginCombat inventory owner unavailable")?
                                .inventory_ready()?,
                            shell_execution: self
                                .inputs
                                .shell_execution
                                .context("beginCombat shell context unavailable")?,
                        },
                        nominal_ticks: self
                            .inputs
                            .nominal_ticks
                            .context("beginCombat nominal ticks unavailable")?,
                        taunt_animation_admitted: self.animation_system
                            && actor.animation.is_some()
                            && actor.model.static_radius.is_none(),
                        selected_player_death_override: false,
                    }
                }
            };
            anyhow::ensure!(
                inputs.weapon.selected_player.generation == self.generation
                    && inputs.weapon.selected_player.index < self.registry.actors.len(),
                "beginCombat selected actor belongs to an expired or absent world"
            );
            let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
            let events = effects::ActorEventDispatch {
                context: effects::ActorEventContext::capture(self.registry, id),
                camera: self.registry.camera.as_mut(),
                effects: &mut self.registry.effects,
            };
            let mut receiver = weapon::CombatReceiver::new(
                &mut self.registry.actors[self.index],
                &mut self.registry.globals,
                self.world,
                id,
                position,
            )?
            .with_events(events);
            let mut emit = |actor, sound| {
                self.sounds
                    .as_deref_mut()
                    .context("beginCombat sound emitter unavailable")?(actor, sound)
            };
            receiver.begin(
                inputs,
                self.random.as_deref_mut(),
                self.inputs.combat_taunts,
                &mut emit,
            )?;
            return Ok(Some(CallResult::CompleteWithoutValue));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_combat_query(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if function == Native::IsCombatCapable || function == Native::PlayerArmed {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let armed = function == Native::PlayerArmed;
            let index = if armed {
                self.selected_player()
                    .context("PlayerArmed: selected player unavailable")?
                    .index
            } else {
                self.index
            };
            return Ok(Some(CallResult::Complete(i32::from(
                self.combat_capable(index, armed)?,
            ))));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_combat_control(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        // DrawSword is the weapon draw admission: it enters `DRAWING_SWORD` when every gate allows it
        // and otherwise leaves the actor unchanged. Its final repositioning step is disabled in
        // the shipped game.
        if function == Native::DrawSword {
            anyhow::ensure!(arguments.is_empty(), "DrawSword requires no arguments");
            self.draw_sword(id)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::SheathSword {
            anyhow::ensure!(arguments.is_empty(), "SheathSword requires no arguments");
            self.sheath_sword(function, id)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // endCombat runs SheathSword's action, then unconditionally resets the lifecycle selector
        // and mode bytes, re-enables the hook, leaves combat mode and clears the close-distance
        // pass counter and the coordination selection state.
        if function == Native::EndCombat {
            anyhow::ensure!(arguments.is_empty(), "endCombat requires no arguments");
            self.sheath_sword(function, id)?;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .context("endCombat: receiver control unavailable")?;
            anyhow::ensure!(
                crate::script::combat::end_combat_reset(control),
                "endCombat: receiver combat record unavailable"
            );
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::IsFighting {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(Some(CallResult::Complete(i32::from(self.is_fighting()?))));
        }
        Ok(None)
    }
}
