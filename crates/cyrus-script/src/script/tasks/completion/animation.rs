//! Actor-animation request and frame-wait completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_animation(
        &mut self,
        arguments: &[Argument],
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        let turning = matches!(self, Self::ControlAnimation { .. });
        Ok(match self {
            Self::AnimationFrame { player } => {
                let [group, threshold] = arguments else {
                    anyhow::bail!("animation frame wait requires two arguments");
                };
                let (group, threshold) = (group.integer()?, threshold.integer()?);
                let input = services
                    .frame_wait
                    .context("animation frame wait inputs unavailable")?;
                let state = if *player {
                    input.player.map_err(anyhow::Error::msg)?
                } else {
                    Some(input.receiver.map_err(anyhow::Error::msg)?)
                };
                match state {
                    None => true,
                    Some((current, pending, progress)) => {
                        if current == group {
                            i32::from(progress) >= threshold
                        } else {
                            pending != group
                        }
                    }
                }
            }
            Self::AnimationRequest => {
                let [group, modifier] = arguments else {
                    anyhow::bail!("PlayAnimation requires two arguments")
                };
                anyhow::ensure!(
                    group.integer()? == i32::from(AnimationGroup::MISC1)
                        && modifier.integer()? == 0,
                    "PlayAnimation multitask operands unsupported; expected installed (10, 0)"
                );
                if services.animation_requests_enabled {
                    let input = animation.as_mut().context(
                        "PlayAnimation multitask requires prepared animation and nominal ticks",
                    )?;
                    let frame = match &mut input.sounds {
                        Some(sounds) => input.animation.request_with_sounds(
                            AnimationGroup::MISC1,
                            0,
                            input.ticks,
                            *sounds,
                        )?,
                        None => input
                            .animation
                            .request(AnimationGroup::MISC1, 0, input.ticks)?,
                    };
                    *input.frame = frame.or(*input.frame);
                }
                false
            }
            Self::Animation { group, started } | Self::ControlAnimation { group, started } => {
                let name = if turning {
                    Native::PushControlAnimation
                } else {
                    Native::PushAnimation
                };
                let [requested, count] = arguments else {
                    anyhow::bail!("{name} requires two arguments")
                };
                let requested = requested.integer()?;
                let requested_bits = animation_group_bits(requested)
                    .with_context(|| format!("{name} group outside supported signed range"))?;
                let count = count.integer()? as u8;
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                *group = requested;
                if *started {
                    let running = input
                        .animation
                        .current_group()
                        .map(|group| i32::from(group.index() as i16))
                        == Some(requested);
                    if turning && running {
                        let pose = spatial
                            .as_mut()
                            .context("PushControlAnimation requires receiver pose")?;
                        pose.angles[1] = i32::from(crate::player::turn::turned(
                            (pose.angles[1] & 2047) as u16,
                            pose.turn,
                        ));
                    }
                    !running
                } else {
                    if input
                        .animation
                        .current_group()
                        .map(|group| i32::from(group.index() as i16))
                        == Some(requested)
                    {
                        input.animation.set_loop_count(count)?;
                        *started = true;
                    } else {
                        let frame = match &mut input.sounds {
                            Some(sounds) => input.animation.request_with_sounds(
                                requested_bits,
                                0,
                                input.ticks,
                                *sounds,
                            )?,
                            None => input.animation.request(requested_bits, 0, input.ticks)?,
                        };
                        *input.frame = frame.or(*input.frame);
                    }
                    false
                }
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
