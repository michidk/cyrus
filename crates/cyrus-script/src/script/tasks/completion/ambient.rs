//! Ambient playback task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_ambient(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        let ambient = services.ambient;
        Ok(match self {
            Self::Ambient { channel } => {
                let [effect, volume, rate] = arguments else {
                    anyhow::bail!("AmbientSound requires three arguments")
                };
                invocation
                    .elapsed
                    .context("AmbientSound requires elapsed input")?;
                let input = ambient.context("AmbientSound playback service unavailable")?;
                let reset = input
                    .reset
                    .context("AmbientSound sample-reset input unavailable")?;
                let driver = input
                    .driver
                    .context("AmbientSound playback service unavailable")?;
                let position = input
                    .position
                    .context("AmbientSound receiver pose unavailable")?;
                let excluded = input
                    .excluded
                    .context("AmbientSound receiver exclusion state unavailable")?;
                // Keep handles across zero-elapsed updates to avoid frame-rate-dependent loop
                // accumulation. The original can retain overlapping channels (CAVERNS effect72);
                // this shared-path difference is recorded in docs/DIFFERENCES.md and issue #328.
                if reset {
                    *channel = None;
                }
                let updated = driver
                    .update(cyrus_world::sound::AmbientSoundRequest {
                        generation: input.generation,
                        actor_index: input.actor_index,
                        channel: *channel,
                        effect: effect.integer()?,
                        volume: volume.integer()?,
                        rate_offset_hz: rate.integer()?,
                        position,
                        object_update_excluded: excluded,
                    })
                    .map_err(anyhow::Error::msg)?;
                anyhow::ensure!(
                    updated.channel.is_none_or(|slot| slot < 16),
                    "AmbientSound playback returned an invalid sample slot"
                );
                *channel = updated.channel;
                !updated.active
            }
            Self::AmbientRtx { slot, lookup } => {
                let [label, volume, _] = arguments else {
                    anyhow::bail!("AmbientRtx requires three arguments")
                };
                let elapsed = invocation
                    .elapsed
                    .context("AmbientRtx requires elapsed input")?;
                let input = ambient.context("AmbientRtx playback inputs unavailable")?;
                let dialogue_input = services
                    .player_dialogue
                    .context("AmbientRtx service inputs unavailable")?;
                let source = dialogue_input
                    .source
                    .context("AmbientRtx source service unavailable")?;
                let attempt = match *lookup {
                    Lookup::Idle => elapsed == 0,
                    Lookup::Pending => true,
                    Lookup::Done => false,
                };
                if attempt {
                    let width = services
                        .dialogue
                        .context("AmbientRtx dialogue settings unavailable")?
                        .settings()
                        .max_text_width;
                    let std::task::Poll::Ready(line) = source.speaker_line(
                        label.integer()?.to_le_bytes(),
                        dialogue_input.speaker,
                        width,
                    )?
                    else {
                        *lookup = Lookup::Pending;
                        return Ok(false);
                    };
                    *lookup = Lookup::Done;
                    // No dialogue line is created: only the clip's sample matters.
                    *slot = line.and_then(|line| line.sample);
                    anyhow::ensure!(
                        slot.is_none_or(|slot| slot < 16),
                        "AmbientRtx source returned invalid sample slot"
                    );
                }
                let Some(slot) = *slot else {
                    // A missing or text-only entry has no clip, so there is nothing to keep playing.
                    return Ok(true);
                };
                let position = input
                    .position
                    .context("AmbientRtx receiver pose unavailable")?;
                let killed = input
                    .killed
                    .context("AmbientRtx receiver killed state unavailable")?;
                !source.update_ambient_rtx(slot, position, volume.integer()?, killed)?
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
