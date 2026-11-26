//! Sound admission and digital-output service commands.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Audio<'w, 's> {
    pub(super) requests: MessageWriter<'w, cyrus_audio::sound::SoundRequest>,
    pub(super) effects: Option<Res<'w, cyrus_audio::sound::SoundEffects>>,
    pub(super) speech: Option<Res<'w, cyrus_audio::sound::Speech>>,
    pub(super) music: Option<Res<'w, cyrus_audio::music::Music>>,
    pub(super) music_sink:
        Query<'w, 's, &'static bevy::audio::AudioSink, With<cyrus_audio::music::MusicPlayer>>,
    pub(super) playing: Query<
        'w,
        's,
        Option<&'static bevy::audio::AudioSink>,
        With<cyrus_audio::sound::PlayingSound>,
    >,
}

impl Audio<'_, '_> {
    pub(super) fn handle(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        match command {
            command @ Command::ConfigureDigitalOutput { .. } => {
                self.configure_digital_output(command, request, commands)
            }
            command @ Command::ServiceDigitalOutput { .. } => {
                self.service_digital_output(command, request, commands)
            }
            command @ Command::PlaySound { .. } => self.play_sound(command),
            command @ Command::PlaySpeech { .. } => self.play_speech(command),
            command @ Command::StopSounds => self.stop_sounds(command),
            _ => unreachable!("command routed to the wrong audio handler"),
        }
    }

    fn configure_digital_output(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Reply { id, reply, .. } = request;
        match command {
            Command::ConfigureDigitalOutput {
                frames_per_half,
                sample_rate,
            } => {
                commands.queue(move |world: &mut World| {
                    let result =
                        cyrus_audio::configure_digital_output(world, frames_per_half, sample_rate);
                    let _ = reply.send(match result {
                        Ok(()) => Response::ok(id, json!({"configured":true})),
                        Err(error) => Response::err(id, format!("{error:#}")),
                    });
                });
                Outcome::Barrier
            }
            _ => unreachable!("command routed to the wrong configure_digital_output handler"),
        }
    }

    fn service_digital_output(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Reply { id, reply, .. } = request;
        Outcome::Reply(match command {
            Command::ServiceDigitalOutput { active_half } => {
                let half = match active_half {
                    0 => Some(cyrus_audio::DigitalOutputHalf::First),
                    1 => Some(cyrus_audio::DigitalOutputHalf::Second),
                    _ => None,
                };
                if let Some(half) = half {
                    commands.queue(move |world: &mut World| {
                        let result = cyrus_audio::service_digital_output(world, half);
                        let _ = reply.send(match result {
                            Ok(()) => Response::ok(id, json!({"serviced":true})),
                            Err(error) => Response::err(id, format!("{error:#}")),
                        });
                    });
                    return Outcome::Barrier;
                } else {
                    Err("digital output active half must be zero or one".into())
                }
            }
            _ => unreachable!("command routed to the wrong service_digital_output handler"),
        })
    }

    fn play_sound(&mut self, command: Command) -> Outcome {
        let Self {
            requests, effects, ..
        } = self;
        Outcome::Reply(match command {
            Command::PlaySound { effect, position } => {
                match effects.as_ref().and_then(|e| e.0.get(effect)?.as_ref()) {
                    Some(loaded) => {
                        requests.write(cyrus_audio::sound::SoundRequest::Play {
                            effect,
                            position: position.map(Vec3::from_array),
                        });
                        Ok(
                            json!({"effect": effect, "looping": loaded.repetitions != cyrus_assets::sound::SampleRepetitions::ONCE}),
                        )
                    }
                    None => Err(format!("sound effect {effect} is not loaded")),
                }
            }
            _ => unreachable!("command routed to the wrong play_sound handler"),
        })
    }

    fn play_speech(&mut self, command: Command) -> Outcome {
        let Self {
            requests, speech, ..
        } = self;
        Outcome::Reply(match command {
            Command::PlaySpeech { label } => {
                if speech.as_ref().is_some_and(|s| s.get(&label).is_some()) {
                    requests.write(cyrus_audio::sound::SoundRequest::Speech(label.clone()));
                    Ok(json!({"label": label}))
                } else {
                    Err(format!("voice clip {label} is not loaded"))
                }
            }
            _ => unreachable!("command routed to the wrong play_speech handler"),
        })
    }

    fn stop_sounds(&mut self, command: Command) -> Outcome {
        let Self { requests, .. } = self;
        Outcome::Reply(match command {
            Command::StopSounds => {
                requests.write(cyrus_audio::sound::SoundRequest::StopAll);
                Ok(json!("stopped"))
            }
            _ => unreachable!("command routed to the wrong stop_sounds handler"),
        })
    }
}
