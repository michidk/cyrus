//! Receiver-owned attack execution through the four defense slots and shared animation owner.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::attack_step_with_animation;
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Execute a caller-selected attack move synchronously, without allocating a VM task.
    /// The pending move selects one of this receiver's four defense slots. Animation and
    /// sound services precede combat start writes; prior service writes survive errors.
    /// Idle cleanup avoids metadata and timing queries. This does not schedule combat.
    pub fn execute_combat_attack(
        &mut self,
        id: ScriptActorId,
        requested: i32,
        inputs: ScriptInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let index = self.resolve(id)?;
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let pending = combat.retained_action_indices[0];
            if combat.decision.attack_state == 0 && requested == -1
                || combat.decision.attack_state != 0 && pending == -1
            {
                return Ok(false);
            }
            let slot = usize::try_from(pending).context("negative pending attack move")?;
            let record = combat
                .defense_indices
                .get(slot)
                .copied()
                .flatten()
                .with_context(|| format!("defense slot {slot}: unpopulated"))?;
            let group = self
                .combat_defenses
                .get(usize::from(record))
                .with_context(|| format!("defense slot {slot} record {record}: unavailable"))?
                .animation_group;
            let actor = &self.actors[index];
            let current = actor
                .animation
                .as_ref()
                .context("receiver animation unavailable")?
                .playback
                .current_group()
                .map(i32::from);
            let starting = combat.decision.attack_state == 0 && current != Some(group);
            let timing = if starting {
                Some(inputs.nominal_ticks.context("nominal ticks unavailable")?)
            } else {
                None
            };
            let position = if starting {
                Some(self.sight_position(index)?)
            } else {
                None
            };
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor.animation.as_mut().expect("admitted animation");
            let combat = actor
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            let step = attack_step_with_animation(
                &mut combat.decision,
                &mut combat.action,
                &mut combat.retained_action_indices[0],
                requested,
                current,
                || Some(group),
                &mut |group| {
                    let group =
                        u16::try_from(group).context("animation group outside u16 range")?;
                    let frame = animation.playback.force_request_with_sounds(
                        group,
                        0,
                        timing.expect("start timing"),
                        &mut |sound| {
                            sounds(
                                id,
                                crate::script::ScriptSound::Animation {
                                    sound,
                                    surface,
                                    position,
                                },
                            )
                        },
                    )?;
                    animation.frame = frame.or(animation.frame);
                    animation.playback.set_loop_count(1)?;
                    animation.playback.clear_pending_request();
                    Ok(())
                },
            )?;
            Ok(step.active)
        })();
        result
            .with_context(|| format!("world {} instance {} attack executor", self.world, id.index))
    }
}
