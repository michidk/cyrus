//! Action native-call dispatch.
use super::super::{
    ActionWords, AnimationGroup, Argument, Attribute, CallKind, CallResult, GameHost, JumpMotion,
    Native, Result, StepDirection, Target,
};
use anyhow::Context;
use cyrus_soup::Host;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_action(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        invocation: cyrus_soup::Invocation,
        flags: &mut [i32],
    ) -> Result<Option<CallResult>> {
        if [
            Native::SetAction,
            Native::ResetAction,
            Native::ResetAllAction,
        ]
        .contains(&function)
        {
            anyhow::ensure!(
                kind == CallKind::Function,
                "{function}: task completion of a void native is unsupported"
            );
            let expected = usize::from(function != Native::ResetAllAction);
            anyhow::ensure!(
                arguments.len() == expected,
                "{function} requires {expected} arguments"
            );
            let mask = arguments
                .first()
                .map(Argument::integer)
                .transpose()?
                .unwrap_or(0) as u32;
            let actor = &mut self.registry.actors[self.index];
            let control = actor
                .control
                .as_mut()
                .context("action native: control unavailable")?;
            if function == Native::ResetAllAction {
                control.actions = Default::default();
            } else if function == Native::ResetAction {
                control.actions.iter_mut().for_each(|word| *word &= !mask);
            } else {
                if !matches!(mask, 1 | 2) || control.actions.iter().any(|word| word & mask != 0) {
                    return Ok(Some(CallResult::CompleteWithoutValue));
                }
                let animation = actor
                    .animation
                    .as_ref()
                    .context("SetAction: animation unavailable")?;
                let group = animation
                    .playback
                    .current_group()
                    .context("SetAction: current group unavailable")?;
                if (AnimationGroup::FIRST_LAND_GROUP..=AnimationGroup::LAST_LAND_GROUP)
                    .contains(&group)
                    || control.queued_motion != JumpMotion::NONE
                    || control.falling_state != JumpMotion::NONE
                {
                    control
                        .actions
                        .iter_mut()
                        .for_each(|word| *word &= !ActionWords::JUMPS);
                } else {
                    let step_hold = actor.attributes[Attribute::StepType] == 0
                        || StepDirection::Forward.counter(control.move_counter) == 3;
                    if step_hold
                        && matches!(group, AnimationGroup::RUN | AnimationGroup::RUN_TORCH)
                        && !animation.playback.exit_reached()
                    {
                        control.actions.pending |= ActionWords::JUMPS;
                        control.jump_window = 0;
                    } else {
                        self.call(target, kind, Native::Jump, &[], invocation, flags)?;
                    }
                }
            }
            return Ok(Some(CallResult::CompleteWithoutValue));
        }
        Ok(None)
    }
}
