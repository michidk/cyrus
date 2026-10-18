//! Live defence execution binds receiver-owned attacks to animation, sound and timed taunts.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::{DefenceView, defence_step_with_animation};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Execute an already selected defence synchronously. The defence selector addresses this
    /// receiver's nine attack slots; missing authored motions retain zero group/type defaults.
    /// Animation and sounds run before combat start writes, followed by the kind-one taunt.
    /// A failed service retains earlier writes. This does not select or start ordinary combat.
    /// Service order follows [preservation](https://github.com/michidk/redguard-preservation/pull/115).
    pub fn execute_combat_defence(
        &mut self,
        id: ScriptActorId,
        requested: i32,
        frame_value: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let index = self.resolve(id)?;
            let combat = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .context("receiver combat unavailable")?;
            let selector = combat.retained_action_indices[1];
            if combat.decision.attack_state == 0 && requested == -1
                || combat.decision.attack_state != 0 && selector == -1
            {
                return Ok(false);
            }
            let slot = usize::try_from(selector).context("negative defence selector")?;
            let attack = self
                .combat_attack(id, slot)?
                .context("selected attack slot is unpopulated")?;
            let groups: [i32; 3] = std::array::from_fn(|motion| {
                attack
                    .motions
                    .get(motion)
                    .map_or(0, |motion| motion.animation_group)
            });
            let kinds: [i32; 3] = std::array::from_fn(|motion| {
                attack.motions.get(motion).map_or(0, |motion| motion.kind)
            });
            let ticks = inputs.nominal_ticks.context("nominal ticks unavailable")?;
            let position = Some(self.sight_position(index)?);
            let actor = &mut self.actors[index];
            let surface = actor.surface;
            let animation = actor
                .animation
                .as_mut()
                .context("receiver animation unavailable")?;
            let combat = actor
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            let view = DefenceView {
                current_group: animation.playback.current_group().map(i32::from),
                pending_group: animation.playback.pending_group().map(i32::from),
                defence_group: &|motion| {
                    usize::try_from(motion)
                        .ok()
                        .and_then(|motion| groups.get(motion).copied())
                },
                entry_type_one: &|motion| {
                    usize::try_from(motion)
                        .ok()
                        .and_then(|motion| motion.checked_add(1))
                        .and_then(|next| kinds.get(next))
                        == Some(&1)
                },
            };
            let step = defence_step_with_animation(
                &mut combat.decision,
                &mut combat.flags,
                &mut combat.action,
                &mut combat.retained_action_indices[1],
                requested,
                &view,
                &mut |group| {
                    let group =
                        u16::try_from(group).context("animation group outside u16 range")?;
                    let frame =
                        animation
                            .playback
                            .request_with_sounds(group, 0, ticks, &mut |sound| {
                                sounds(
                                    id,
                                    crate::script::ScriptSound::Animation {
                                        sound,
                                        surface,
                                        position,
                                    },
                                )
                            })?;
                    animation.frame = frame.or(animation.frame);
                    animation.playback.set_loop_count(1)
                },
            )?;
            if step.effect.taunt_kind_one {
                self.request_timed_combat_taunt(id, 1, frame_value, inputs, random, sounds)?;
            }
            Ok(step.active)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} defence executor",
                self.world, id.index
            )
        })
    }
}
