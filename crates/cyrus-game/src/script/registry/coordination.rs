//! Combat coordination shares attacker eligibility, close counters and addressed class queues.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use anyhow::{Context, Result};

/// The original branch remains selected for the pass even when coordination changes its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryCombatBranch {
    Active,
    Waiting,
    Disengaged,
}

/// Scheduler-owned engagement, selected-player identity and shared timed-taunt counter.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryCombatCoordinationInput {
    pub engaged: bool,
    pub selected_player: ScriptActorId,
    pub frame_value: i32,
}

impl ScriptWorld {
    /// Consume at most one combat message. Coordination transfers clear living
    /// active attackers in ordinary list order before assigning state and budget.
    /// Block response displaces the sender before coordination handoff.
    /// Unknown codes are inert; hit and damage responses remain unsupported.
    /// Consumption and earlier state writes survive a later service error.
    /// Production dispatch sees scheduler services before consuming hit/damage messages.
    /// The d001 response still stops after its verified intent writes; d005 remains inert
    /// until its distinct response service is integrated.
    pub fn process_combat_message_with_services(
        &mut self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
        inputs: super::ScriptInputs<'_>,
        random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let Some(message) = self.peek_combat_message(id)? else {
            return Ok(());
        };
        if message.code != 0xd001 {
            return self.process_combat_message(id, selected_player);
        }
        let message = self
            .consume_combat_message(id)?
            .context("peeked hit message disappeared before consume")?;
        for actor in [id, message.sender] {
            let index = self.resolve(actor)?;
            self.actors[index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("hit message actor {index} combat unavailable"))?
                .flags &= !3;
        }
        let random = random.context("hit response random stream unavailable")?;
        self.process_hit_response_core(id, message.sender, inputs, random, sounds)?;
        // The d001 tail clears the sender's active action only when the receiver
        // was in an ordinary action.  Preserve -1 (idle) and 9 (counter/dead)
        // exactly; this write occurs after the response services succeed.
        let receiver_action = self.actors[self.resolve(id)?]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("hit response receiver combat unavailable")?
            .action;
        if !matches!(receiver_action, -1 | 9) {
            let sender_index = self.resolve(message.sender)?;
            self.actors[sender_index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .context("hit response sender combat unavailable")?
                .action = -1;
        }
        Ok(())
    }

    pub fn process_combat_message(
        &mut self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
    ) -> Result<()> {
        let result = (|| -> Result<()> {
            let index = self.resolve(id)?;
            if self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_none()
            {
                return Ok(());
            }
            let Some(message) = self.consume_combat_message(id)? else {
                return Ok(());
            };
            let selected = match message.code {
                0xd002 => id,
                0xd003 => message.sender,
                0xd000 => return self.process_block_message(id, message.sender, selected_player),
                0xd001 => {
                    // Original order clears receiver intent, then sender intent, before
                    // hit-reaction services. Those services remain unsupported here.
                    for actor in [id, message.sender] {
                        let index = self.resolve(actor)?;
                        self.actors[index]
                            .control
                            .as_mut()
                            .and_then(|control| control.combat.as_mut())
                            .with_context(|| {
                                format!("hit message actor {index} combat unavailable")
                            })?
                            .flags &= !3;
                    }
                    anyhow::bail!(
                        "ordinary combat message {} response services unsupported",
                        message.code
                    )
                }
                0xd005 => anyhow::bail!(
                    "ordinary combat message {} response services unsupported",
                    message.code
                ),
                _ => return Ok(()),
            };
            let selected_index = self.resolve(selected)?;
            let target = self.actors[selected_index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("coordination recipient combat unavailable")?
                .target;
            if let Some(target) = target {
                let traversal = self
                    .actor_lists
                    .as_ref()
                    .and_then(|lists| lists.retained_selected())
                    .context("coordination message ordinary traversal unavailable")?
                    .to_vec();
                for candidate in traversal {
                    let index = self.resolve(candidate)?;
                    let Some(control) = self.actors[index].control.as_ref() else {
                        continue;
                    };
                    let Some(combat) = control.combat.as_ref() else {
                        continue;
                    };
                    if candidate != target
                        && combat.target == Some(target)
                        && combat.attacker_selection_state == 1
                        && !self.query_is_dead(candidate, selected_player)?
                    {
                        self.set_combat_coordination(candidate, 0)?;
                    }
                }
            }
            self.set_combat_coordination(selected, 1)?;
            self.actors[selected_index]
                .control
                .as_mut()
                .expect("admitted coordination control")
                .combat
                .as_mut()
                .expect("admitted coordination combat")
                .decision
                .aggression = 50;
            Ok(())
        })();
        result.with_context(|| format!("world {} instance {} combat message", self.world, id.index))
    }

    /// Block-response order follows preservation's combat-message contract.
    /// Intent clears and sender displacement survive a later handoff query failure.
    fn process_block_message(
        &mut self,
        receiver: ScriptActorId,
        sender: ScriptActorId,
        selected_player: Option<ScriptActorId>,
    ) -> Result<()> {
        for actor in [receiver, sender] {
            let index = self.resolve(actor)?;
            self.actors[index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("block message actor {index} combat unavailable"))?
                .flags &= !3;
        }
        let origin = self.spatial(sender)?.local.position;
        let other = self.spatial(receiver)?.local.position;
        let heading = super::super::spatial::query_bearing(
            origin[0].wrapping_sub(other[0]),
            origin[2].wrapping_sub(other[2]),
        );
        let moved = cyrus_world::transform::translate_forward(origin, heading, 3000)?;
        self.spatial
            .as_mut()
            .context("block message spatial owner unavailable")?
            .actor_mut(sender.index)?
            .local
            .position = moved;
        let combat = self.actors[receiver.index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .expect("admitted block receiver");
        if combat.attacker_selection_state != 0
            || selected_player == Some(receiver)
            || self.query_is_dead(receiver, selected_player)?
        {
            return Ok(());
        }
        if let Some(coordinator) = self.ordinary_waiting_coordinator(sender)? {
            self.send_combat_message(receiver, coordinator, 0xd003)?;
        } else {
            self.set_combat_coordination(receiver, 1)?;
        }
        Ok(())
    }

    /// Assign coordination state, resetting consecutive close-distance passes first.
    /// This is also the state consumed by ordinary attacker and spacing queries.
    pub fn set_combat_coordination(&mut self, id: ScriptActorId, state: i32) -> Result<()> {
        let index = self.resolve(id)?;
        let combat = self.actors[index]
            .control
            .as_mut()
            .and_then(|control| control.combat.as_mut())
            .with_context(|| {
                format!(
                    "world {} instance {} coordination combat unavailable",
                    self.world, id.index
                )
            })?;
        combat.close_distance_passes = 0;
        combat.attacker_selection_state = state;
        Ok(())
    }

    /// First active coordinator in list order, excluding the supplied target. A nonnull
    /// candidate target is required, but equality with the supplied target is not.
    pub fn ordinary_waiting_coordinator(
        &self,
        target: ScriptActorId,
    ) -> Result<Option<ScriptActorId>> {
        self.resolve(target)?;
        let traversal = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.retained_selected())
            .context("waiting coordination ordinary traversal unavailable")?;
        for &candidate in traversal {
            let index = self.resolve(candidate)?;
            if candidate == target {
                continue;
            }
            let Some(control) = self.actors[index].control.as_ref() else {
                continue;
            };
            let Some(combat) = control.combat.as_ref() else {
                continue;
            };
            if combat.target.is_some()
                && control.death_selector != 8
                && combat.attacker_selection_state == 1
            {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// First other armed/combat-stance actor sharing the receiver's target, cycling
    /// from the next ordinary actor and excluding the selected player.
    pub fn ordinary_active_coordinator(
        &mut self,
        id: ScriptActorId,
        selected_player: ScriptActorId,
        inputs: ScriptInputs<'_>,
    ) -> Result<Option<ScriptActorId>> {
        let index = self.resolve(id)?;
        self.resolve(selected_player)?;
        let target = self.actors[index]
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .context("active coordination receiver combat unavailable")?
            .target;
        let traversal = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.retained_selected())
            .context("active coordination ordinary traversal unavailable")?
            .to_vec();
        let start = traversal
            .iter()
            .position(|candidate| *candidate == id)
            .context("active coordination receiver absent from ordinary traversal")?;
        for offset in 1..traversal.len() {
            let candidate = traversal[(start + offset) % traversal.len()];
            let candidate_index = self.resolve(candidate)?;
            let Some(control) = self.actors[candidate_index].control.as_ref() else {
                continue;
            };
            let Some(combat) = control.combat.as_ref() else {
                continue;
            };
            if combat.target != target || control.death_selector == 8 {
                continue;
            }
            // The original tests the held-weapon owner before the stance flag and player exclusion.
            let armed = self.spacing_weapon(candidate, inputs)?.1;
            let flags = self.actors[candidate_index]
                .control
                .as_ref()
                .expect("admitted control")
                .combat
                .as_ref()
                .expect("admitted combat")
                .flags;
            if (armed || flags & 4 != 0) && candidate != selected_player {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// Run coordination after engagement sampling and inactive cleanup executors.
    /// Message sends precede taunts and state assignment; earlier writes survive errors.
    /// The returned original branch tells the caller which later decision stages to run.
    /// This explicit service neither repeats sampling nor schedules automatic traversal.
    pub fn coordinate_ordinary_combat(
        &mut self,
        id: ScriptActorId,
        sample: OrdinaryCombatCoordinationInput,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<OrdinaryCombatBranch> {
        let result = (|| -> Result<_> {
            let index = self.resolve(id)?;
            self.resolve(sample.selected_player)?;
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let active = combat.attacker_selection_state == 1;
            let aggression = combat.decision.aggression;
            let close = combat.close_distance_passes;
            let target = combat.target;
            if !sample.engaged {
                if active {
                    if let Some(candidate) =
                        self.ordinary_active_coordinator(id, sample.selected_player, inputs)?
                    {
                        self.send_combat_message(id, candidate, 0xd002)?;
                    }
                    self.set_combat_coordination(id, 0)?;
                }
                return Ok(OrdinaryCombatBranch::Disengaged);
            }
            if active {
                if aggression < 0
                    && let Some(candidate) =
                        self.ordinary_active_coordinator(id, sample.selected_player, inputs)?
                {
                    let far = self.actors[candidate.index]
                        .control
                        .as_ref()
                        .expect("admitted control")
                        .combat
                        .as_ref()
                        .expect("admitted combat")
                        .spacing_ranges[1]
                        .wrapping_mul(3);
                    let from = self.query_position(candidate)?;
                    let player = self.query_position(sample.selected_player)?;
                    let distance =
                        cyrus_collision::distance::rounded_distance(std::array::from_fn(|axis| {
                            player[axis].wrapping_sub(from[axis])
                        }))?;
                    if distance < far {
                        self.send_combat_message(id, candidate, 0xd002)?;
                        self.request_timed_combat_taunt(
                            id,
                            9,
                            sample.frame_value,
                            inputs,
                            random,
                            sounds,
                        )?;
                        self.set_combat_coordination(id, 0)?;
                    }
                }
                Ok(OrdinaryCombatBranch::Active)
            } else {
                let target = target.context("waiting coordination target unavailable")?;
                if let Some(candidate) = self.ordinary_waiting_coordinator(target)? {
                    if close > 10 {
                        self.send_combat_message(id, candidate, 0xd003)?;
                        self.request_timed_combat_taunt(
                            id,
                            8,
                            sample.frame_value,
                            inputs,
                            random,
                            sounds,
                        )?;
                    }
                } else {
                    self.set_combat_coordination(id, 1)?;
                }
                Ok(OrdinaryCombatBranch::Waiting)
            }
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} combat coordination",
                self.world, id.index
            )
        })
    }
}
