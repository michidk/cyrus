//! Sound native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result, ScriptActorId};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_sound(
        &mut self,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if function == Native::Sound {
            let [effect, volume, rate] = arguments else {
                anyhow::bail!("Sound requires three arguments")
            };
            let position = self.registry.query_position(id)?;
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id,
                crate::script::ScriptSound::Positional {
                    effect: effect.integer()?,
                    volume: volume.integer()?,
                    rate_offset_hz: rate.integer()?,
                    position,
                    start_index: None,
                },
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if let Some(sound) = crate::script::sound::command(function, arguments)? {
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(id, sound)?;
            // The final channel-stop path leaves a residual, not a portable scalar result.
            return Ok(Some(if function == Native::StopAllSounds {
                CallResult::CompleteWithoutValue
            } else {
                CallResult::Complete(0)
            }));
        }
        Ok(None)
    }
}
