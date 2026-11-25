//! Animation native-call dispatch.
use super::super::{
    AnimationGroup, Argument, CallKind, CallResult, GameHost, Native, Result, ScriptActorId,
    animation_sound_position, effects,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_animation_control(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // HoldAnimation, PauseAnimation and ReleaseAnimation write the receiver's animation wait
        // byte: 255 holds, a pause value counts nominal ticks, release clears only a hold.
        if kind == CallKind::Function
            && [
                Native::HoldAnimation,
                Native::PauseAnimation,
                Native::ReleaseAnimation,
            ]
            .contains(&function)
        {
            let wait = match (arguments, function) {
                ([], Native::HoldAnimation) => Some(u8::MAX),
                ([value], Native::PauseAnimation) => Some(value.integer()? as u8),
                ([], Native::ReleaseAnimation) => None,
                _ => anyhow::bail!("{function}: wrong argument count"),
            };
            if let Some(animation) = self.registry.actors[self.index].animation.as_mut() {
                match wait {
                    Some(wait) => animation.playback.set_wait(wait)?,
                    None => animation.playback.release_hold()?,
                }
            }
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_animation_playback(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function && function == Native::PlayAnimation {
            let [group, modifier] = arguments else {
                anyhow::bail!("PlayAnimation requires two arguments")
            };
            let group = group.integer()?;
            let group_bits = i16::try_from(group)
                .map(|value| AnimationGroup::from_index(value as u16))
                .context("PlayAnimation group outside supported signed range")?;
            // The modifier is stored as one byte.
            let modifier = modifier.integer()? as u8;
            let excluded = self.registry.actors[self.index]
                .control
                .as_ref()
                .is_some_and(|control| control.object_update_excluded);
            if !self.animation_system
                || excluded
                || self.registry.actors[self.index].animation.is_none()
            {
                return Ok(Some(CallResult::Complete(0)));
            }
            let ticks = self
                .inputs
                .nominal_ticks
                .context("PlayAnimation requires nominal_ticks")?;
            let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
            let surface = self.registry.actors[self.index].surface;
            let events = effects::ActorEventContext::capture(self.registry, id);
            let camera = &mut self.registry.camera;
            let effects = &mut self.registry.effects;
            let mut emit = |sound| {
                if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                    return events.dispatch(flags, camera.as_mut(), effects);
                }
                match &mut self.sounds {
                    Some(sounds) => sounds(
                        id,
                        crate::script::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    ),
                    None => {
                        anyhow::bail!("world actor {}: animation sound handler absent", id.index)
                    }
                }
            };
            let animation = self.registry.actors[self.index]
                .animation
                .as_mut()
                .expect("checked animation");
            let frame = animation
                .playback
                .request_with_sounds(group_bits, modifier, ticks, &mut emit)?;
            animation.frame = frame.or(animation.frame);
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::GetFramePassed {
            anyhow::ensure!(arguments.is_empty(), "GetFramePassed requires no arguments");
            let ticks = self
                .inputs
                .nominal_ticks
                .context("GetFramePassed requires nominal_ticks input")?;
            return Ok(Some(CallResult::Complete(
                i32::try_from(ticks).context("GetFramePassed ticks exceed the signed range")?,
            )));
        }
        Ok(None)
    }
}
