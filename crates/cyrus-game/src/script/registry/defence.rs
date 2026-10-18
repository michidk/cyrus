//! Live defence execution binds receiver-owned attacks to animation, sound and timed taunts.
use super::{ScriptActorId, ScriptInputs, ScriptWorld};
use crate::script::decision::{DefenceView, defence_step_with_animation};
use anyhow::{Context, Result};

impl ScriptWorld {
    /// Start this receiver's authored defence synchronously, including live executor services.
    /// Caller-established weapon gates use a generation-checked selected-player identity.
    /// Selected-player kind four invokes movement with Q8 distance and mode one before
    /// selector writes. A movement-service error leaves selectors untouched; executor errors
    /// retain the committed selectors and any earlier animation/combat writes.
    /// Admission follows preservation's SOUP defence-start rules.
    #[allow(clippy::too_many_arguments)] // One synchronous receiver operation with caller-owned services.
    pub fn start_combat_defence(
        &mut self,
        id: ScriptActorId,
        kind: i32,
        weapon: super::WeaponInputs,
        frame_value: i32,
        inputs: ScriptInputs<'_>,
        random: &mut crate::random::EngineRandom,
        movement: impl FnOnce(&mut Self, ScriptActorId, i32, i32) -> Result<()>,
        sounds: &mut dyn FnMut(ScriptActorId, crate::script::ScriptSound) -> Result<()>,
    ) -> Result<bool> {
        let result = (|| -> Result<bool> {
            let index = self.resolve(id)?;
            self.resolve(weapon.selected_player)?;
            if !matches!(kind, 0..=6 | 8) {
                return Ok(false);
            }
            if self.combat_attack(id, kind as usize)?.is_none() {
                return Ok(false);
            }
            let actor = &self.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("receiver control unavailable")?;
            let combat = control
                .combat
                .as_ref()
                .context("receiver combat unavailable")?;
            let (_, drawing, sheathing) = crate::script::activation::weapon_predicates(
                control.living_state,
                actor
                    .animation
                    .as_ref()
                    .map(|animation| crate::script::ActivationAnimation {
                        group: animation.playback.current_group(),
                    }),
                true,
                control.weapon_drawn,
                actor.attributes[110],
                weapon.item_system_ready,
                weapon.shell_execution,
            );
            let admission = crate::script::decision::DefenceStartInput {
                slot_present: true,
                selected_player: id == weapon.selected_player,
                cooldown: combat.cooldown,
                drawing,
                sheathing,
                attribute_16: actor.attributes[16],
            };
            let mut action = combat.action;
            let mut selector = combat.retained_action_indices[1];
            if !crate::script::decision::start_defence(
                kind,
                admission,
                &mut action,
                &mut selector,
                |distance, mode| movement(self, id, distance, mode),
                |_, _, _| Ok(()),
            )? {
                return Ok(false);
            }
            let combat = self.actors[index]
                .control
                .as_mut()
                .expect("admitted control")
                .combat
                .as_mut()
                .expect("admitted combat");
            combat.retained_action_indices[1] = selector;
            combat.action = action;
            self.execute_combat_defence(id, 0, frame_value, inputs, random, sounds)?;
            Ok(true)
        })();
        result.with_context(|| {
            format!(
                "world {} instance {} defence starter kind {kind}",
                self.world, id.index
            )
        })
    }

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
