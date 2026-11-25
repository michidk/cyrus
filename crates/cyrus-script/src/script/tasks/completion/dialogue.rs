//! Speech, dialogue and topic-menu task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_dialogue(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        mut services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::PlayerDialogue { started } => {
                let [label] = arguments else {
                    anyhow::bail!("RTXp requires one argument");
                };
                let input = services
                    .player_dialogue
                    .context("RTXp service inputs unavailable")?;
                if input.reset.context("RTXp sample-reset input unavailable")? {
                    return Ok(true);
                }
                invocation.elapsed.context("RTXp requires elapsed input")?;
                let dialogue = services
                    .dialogue
                    .context("RTXp dialogue records unavailable")?;
                if !*started {
                    let player = input.player.context("RTXp selected player unavailable")?;
                    let source = input.source.context("RTXp source service unavailable")?;
                    let std::task::Poll::Ready(line) = source.player_line(
                        label.integer()?.to_le_bytes(),
                        player,
                        dialogue.settings().max_text_width,
                    )?
                    else {
                        return Ok(false);
                    };
                    *started = true;
                    if let Some(line) = line {
                        anyhow::ensure!(
                            line.sample.is_none_or(|sample| sample < 16),
                            "RTXp source returned invalid sample slot"
                        );
                        dialogue.begin_player_line(player, line.text, line.sample, line.lifetime);
                    }
                }
                !dialogue.line_active(Some(0))
            }
            Self::SpeakerDialogue(speech) => {
                let [label] = arguments else {
                    anyhow::bail!("RTX requires one argument");
                };
                let input = services
                    .player_dialogue
                    .context("RTX service inputs unavailable")?;
                if input.reset.context("RTX sample-reset input unavailable")? {
                    return Ok(true);
                }
                let first = invocation.elapsed.context("RTX requires elapsed input")? == 0;
                let dialogue = services
                    .dialogue
                    .context("RTX dialogue records unavailable")?;
                let speaker = (input.speaker, &input.speaker_distance);
                match speech.speak(Native::Rtx, label, first, speaker, input.source, dialogue)? {
                    Speak::Complete => true,
                    Speak::Wait => false,
                    Speak::Running { .. } => !dialogue.line_active(speech.active_record()),
                }
            }
            Self::SpeakerAnimation {
                speech,
                index,
                player,
            } => {
                let name = if *player {
                    Native::RTXpAnim
                } else {
                    Native::RtxAnim
                };
                let [label, first_group, second_group, third_group] = arguments else {
                    anyhow::bail!("{name} requires four arguments");
                };
                let input = services
                    .player_dialogue
                    .with_context(|| format!("{name} service inputs unavailable"))?;
                if input
                    .reset
                    .with_context(|| format!("{name} sample-reset input unavailable"))?
                {
                    return Ok(true);
                }
                let first = invocation
                    .elapsed
                    .with_context(|| format!("{name} requires elapsed input"))?
                    == 0;
                let dialogue = services
                    .dialogue
                    .with_context(|| format!("{name} dialogue records unavailable"))?;
                let speaker = if *player {
                    (
                        input
                            .player
                            .with_context(|| format!("{name} selected player unavailable"))?,
                        &input.player_distance,
                    )
                } else {
                    (input.speaker, &input.speaker_distance)
                };
                let fresh =
                    match speech.speak(name, label, first, speaker, input.source, dialogue)? {
                        Speak::Complete => return Ok(true),
                        Speak::Wait => return Ok(false),
                        Speak::Running { fresh } => fresh,
                    };
                // The original stores the third, fourth and second script operands in that order.
                let groups = [
                    second_group.integer()?,
                    third_group.integer()?,
                    first_group.integer()?,
                ];
                let missing =
                    || format!("{name} requires prepared animation and explicit nominal ticks");
                let complete = if *player && !input.player_is_receiver {
                    let call = services.player_animation.as_mut().with_context(missing)?;
                    talk_cycle(call, dialogue, speech, index, groups, fresh)?
                } else {
                    let call = animation.as_mut().with_context(missing)?;
                    talk_cycle(call, dialogue, speech, index, groups, fresh)?
                };
                if !*player && let Some(activation) = services.activation {
                    activation.speech_animation_cycle(complete);
                }
                complete
            }
            Self::Dialogue => {
                anyhow::ensure!(arguments.is_empty(), "WaitOnDialog requires no arguments");
                !services
                    .dialogue
                    .context("WaitOnDialog records unavailable")?
                    .waiting()
            }
            Self::Menu => {
                anyhow::ensure!(arguments.is_empty(), "menuProc requires no arguments");
                let menu = services.menu.context("menuProc menu state unavailable")?;
                *menu.active = 1;
                let input = menu
                    .input
                    .context("menuProc controls, clock and delay settings unavailable")?;
                let running = menu.state.step(input);
                if !running {
                    *menu.active = 0;
                }
                !running
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
