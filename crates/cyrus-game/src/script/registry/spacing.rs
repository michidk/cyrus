//! Fight spacing reads and mutates the receiver's shared movement state.
use super::{ScriptActorId, ScriptWorld, obstacle::OrdinaryObstacleQuery};
use anyhow::{Context, Result, ensure};

/// Live host predicates and timing for preservation's forward movement amount query.
/// Weapon state comes from the item/shell owner; combat, surface, animation and
/// packed startup state are read from the receiver at invocation time.
#[derive(Clone, Copy)]
pub struct OrdinaryForwardAmountInputs {
    pub selected_player: Option<ScriptActorId>,
    pub holding_weapon: bool,
    pub frame_scale: u32,
    pub ticks: u32,
    pub duration: i32,
    pub elapsed: i32,
}

impl ScriptWorld {
    /// Query the forward Q8 amount without translating. This requests animation,
    /// marks the shared forward-called flag and advances the actual startup
    /// counter. Translation suppression returns the adjusted unscaled amount.
    /// A following movement invocation must read the resulting live state.
    pub fn ordinary_forward_amount(
        &mut self,
        id: ScriptActorId,
        inputs: OrdinaryForwardAmountInputs,
        query: Option<OrdinaryObstacleQuery<'_>>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<i32> {
        let result = (|| -> Result<i32> {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("forward amount control unavailable")?;
            if control.rope_attached
                || control.sliding_state != 0
                || control.falling_state != 0
                || control.queued_motion != 0
                || control.living_state != 0
            {
                return Ok(0);
            }
            if (control.movement_flags >> 4) & 3 < 3
                && !self
                    .forward_step_clear(
                        id,
                        inputs.selected_player,
                        inputs.frame_scale as i32,
                        query,
                    )?
                    .context("forward amount step geometry unavailable")?
            {
                return Ok(0);
            }
            let deep = matches!(actor.surface, Some(2 | 4));
            let combat = control.combat.as_ref();
            let (group, attribute) =
                if inputs.holding_weapon || combat.is_some_and(|combat| combat.flags & 4 != 0) {
                    if combat.is_some_and(|combat| combat.action == 4) {
                        (106, 16)
                    } else {
                        (92, 17)
                    }
                } else {
                    (
                        if deep {
                            142
                        } else if control.torch_burning != 0 {
                            30
                        } else {
                            18
                        },
                        8,
                    )
                };
            let step = actor.attributes[attribute];
            let adjustment = actor.attributes[11];
            ensure!(
                actor
                    .animation
                    .as_ref()
                    .context("forward amount animation unavailable")?
                    .playback
                    .has_group(group),
                "forward amount animation group {group} unavailable"
            );
            self.actors[index]
                .control
                .as_mut()
                .context("forward amount control unavailable")?
                .motion_direction = 2;
            self.request_ordinary_group(id, group, false, inputs.ticks, sounds)?;
            let animation = &self.actors[index]
                .animation
                .as_ref()
                .context("forward amount animation unavailable")?
                .playback;
            if self.updates.animation_system && animation.current_group() != Some(group) {
                return Ok(i32::from(super::super::locomotion::continues(
                    inputs.duration,
                    inputs.elapsed,
                )));
            }
            let suppressed = animation.translation_suppressed();
            self.globals.locomotion_called |= 4;
            let amount = i32::from(step) << 8;
            let amount = if deep { amount >> 1 } else { amount };
            let amount = super::super::locomotion::initial_step_distance(
                &mut self.actors[index]
                    .control
                    .as_mut()
                    .context("forward amount control unavailable")?
                    .movement_flags,
                4,
                adjustment,
                amount,
            );
            Ok(if suppressed {
                amount
            } else {
                amount.wrapping_mul(inputs.frame_scale as i32) >> 8
            })
        })();
        result.with_context(|| format!("world {} instance {} forward amount", self.world, id.index))
    }
}
