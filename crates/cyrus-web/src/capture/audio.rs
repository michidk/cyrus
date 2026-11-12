//! Opt-in installed audio starts through the ordinary shared sample adapters.
use bevy::prelude::*;
use cyrus_audio::sound::{SoundRequest, Speech, dialogue::DialogueClip};
use wasm_bindgen::JsValue;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u32,
    generation: u64,
    index: usize,
    label: String,
    effect: Option<i32>,
}
/// Native control-port equivalents for opt-in lifecycle PCM acceptance.
#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum SampleRequest {
    Play {
        id: u32,
        effect: usize,
    },
    Stop {
        id: u32,
    },
    /// Re-read the installed bank through the ordinary asset reader and admission owner.
    ReloadBank {
        id: u32,
    },
    ScriptPlay {
        id: u32,
        generation: u64,
        index: usize,
        effect: i32,
    },
    Animation {
        id: u32,
        generation: u64,
        index: usize,
        command: AnimationCommand,
    },
    Inspect {
        id: u32,
    },
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AnimationCommand {
    Play {
        effect: i16,
        #[serde(default)]
        rate_offset_hz: i32,
    },
    Stop,
    Forget,
}

#[derive(Resource, Default)]
struct Pending(Option<(Request, Handle<DialogueClip>)>);

pub(super) fn register(app: &mut App) {
    app.init_resource::<Pending>().add_systems(
        Update,
        respond.run_if(in_state(cyrus_game::AppState::InGame)),
    );
}

fn respond(world: &mut World) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let result = (|| -> anyhow::Result<()> {
        let sample_key = JsValue::from_str("__cyrusSampleRequest");
        let value = js_sys::Reflect::get(&window, &sample_key)
            .map_err(|e| anyhow::anyhow!("sample request: {e:?}"))?;
        if !value.is_null() && !value.is_undefined() {
            js_sys::Reflect::set(&window, &sample_key, &JsValue::NULL)
                .map_err(|e| anyhow::anyhow!("clearing sample request: {e:?}"))?;
            let request: SampleRequest = serde_json::from_str(
                &value
                    .as_string()
                    .ok_or_else(|| anyhow::anyhow!("sample request must be JSON text"))?,
            )?;
            let id = match request {
                SampleRequest::ReloadBank { id } => {
                    use cyrus_audio::{SoundBank, SoundBankHandle};
                    anyhow::ensure!(
                        !world.contains_resource::<SoundBankHandle>(),
                        "bank admission already pending"
                    );
                    let server = world.resource::<AssetServer>();
                    let handle = server.load::<SoundBank>("cache://sounds/bank.sounds");
                    server.reload("cache://sounds/bank.sounds");
                    world
                        .resource_mut::<Assets<SoundBank>>()
                        .remove(handle.id());
                    world.insert_resource(SoundBankHandle(handle));
                    id
                }
                SampleRequest::ScriptPlay {
                    id,
                    generation,
                    index,
                    effect,
                } => {
                    let actors = world.resource::<cyrus_script::script::ScriptActors>();
                    let current = actors
                        .current
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("script world absent"))?;
                    anyhow::ensure!(
                        current.generation() == generation
                            && !current.inspect(index, 1)["actors"]
                                .as_array()
                                .is_none_or(Vec::is_empty),
                        "script owner absent"
                    );
                    world.write_message(cyrus_world::sound::SoundCue::Script {
                        generation,
                        index,
                        sound: cyrus_world::sound::ScriptSound::Flat {
                            effect,
                            volume: 0,
                            rate_offset_hz: 0,
                        },
                    });
                    id
                }
                SampleRequest::Animation {
                    id,
                    generation,
                    index,
                    command,
                } => {
                    let mut bindings =
                        world.query::<(Entity, &cyrus_script::script::ScriptBinding)>();
                    let nodes = bindings
                        .iter(world)
                        .filter(|(_, binding)| {
                            binding.actor.generation == generation && binding.actor.index == index
                        })
                        .map(|(entity, _)| entity)
                        .collect::<Vec<_>>();
                    anyhow::ensure!(nodes.len() == 1, "animation emitter binding must be unique");
                    let sound = match command {
                        AnimationCommand::Play {
                            effect,
                            rate_offset_hz,
                        } => cyrus_world::animation::AnimationSound::Play {
                            effect,
                            rate_offset_hz,
                        },
                        AnimationCommand::Stop => cyrus_world::animation::AnimationSound::Stop,
                        AnimationCommand::Forget => cyrus_world::animation::AnimationSound::Forget,
                    };
                    let actors = world.resource::<cyrus_script::script::ScriptActors>();
                    let current = actors
                        .current
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("animation world absent"))?;
                    anyhow::ensure!(
                        current.generation() == generation,
                        "animation generation expired"
                    );
                    let row = current.inspect(index, 1);
                    let position = row["actors"][0]["spatial"]["effective"]["position"]
                        .as_array()
                        .and_then(|p| {
                            p.iter()
                                .map(|v| i32::try_from(v.as_i64()?).ok())
                                .collect::<Option<Vec<_>>>()
                        })
                        .and_then(|p| p.try_into().ok());
                    anyhow::ensure!(
                        position.is_some(),
                        "animation effective position unavailable"
                    );
                    world.write_message(cyrus_world::sound::SoundCue::Animation {
                        actor: nodes[0],
                        sound,
                        surface: Some(cyrus_assets::surface::Surface::UNKNOWN),
                        position,
                    });
                    id
                }
                SampleRequest::Inspect { id } => id,
                SampleRequest::Play { id, effect } => {
                    world.write_message(SoundRequest::Play {
                        effect,
                        position: None,
                    });
                    id
                }
                SampleRequest::Stop { id } => {
                    world.write_message(SoundRequest::StopAll);
                    id
                }
            };
            let playing = world
                .query_filtered::<Entity, With<cyrus_audio::sound::PlayingSound>>()
                .iter(world)
                .count();
            let selected = world.get_resource::<cyrus_script::script::DeathDispatch>().and_then(|dispatch| dispatch.last_save.as_ref()).map(|selected| serde_json::json!({"slot":selected.slot,"description":selected.description}));
            let response = serde_json::json!({"id":id,"playing":playing,"bank_pending":world.contains_resource::<cyrus_audio::SoundBankHandle>(),"selected":selected});
            js_sys::Reflect::set(
                &window,
                &"__cyrusSampleInspection".into(),
                &js_sys::JSON::parse(&response.to_string())
                    .map_err(|e| anyhow::anyhow!("sample inspection: {e:?}"))?,
            )
            .map_err(|e| anyhow::anyhow!("sample inspection: {e:?}"))?;
            js_sys::Reflect::set(
                &window,
                &"__cyrusSampleResult".into(),
                &JsValue::from_f64(f64::from(id)),
            )
            .map_err(|e| anyhow::anyhow!("sample response: {e:?}"))?;
        }
        let stop = js_sys::Reflect::get(&window, &"__cyrusVoiceStopRequest".into())
            .map_err(|e| anyhow::anyhow!("voice stop request: {e:?}"))?;
        if !stop.is_null() && !stop.is_undefined() {
            js_sys::Reflect::set(&window, &"__cyrusVoiceStopRequest".into(), &JsValue::NULL)
                .map_err(|e| anyhow::anyhow!("voice stop: {e:?}"))?;
            world.init_resource::<cyrus_script::item_viewer::ExamineScreen>();
            world
                .resource_mut::<cyrus_script::item_viewer::ExamineScreen>()
                .speech_stop = true;
            js_sys::Reflect::set(&window, &"__cyrusVoiceStopResult".into(), &stop)
                .map_err(|e| anyhow::anyhow!("voice stop response: {e:?}"))?;
        }

        let value = js_sys::Reflect::get(&window, &"__cyrusAudioRequest".into())
            .map_err(|e| anyhow::anyhow!("audio request: {e:?}"))?;
        if !value.is_null() && !value.is_undefined() {
            js_sys::Reflect::set(&window, &"__cyrusAudioRequest".into(), &JsValue::NULL)
                .map_err(|e| anyhow::anyhow!("clearing audio request: {e:?}"))?;
            anyhow::ensure!(
                world.resource::<Pending>().0.is_none(),
                "audio request already pending"
            );
            let mut request: Request = serde_json::from_str(
                &value
                    .as_string()
                    .ok_or_else(|| anyhow::anyhow!("audio request must be JSON text"))?,
            )?;
            anyhow::ensure!(
                request.label.is_ascii() && request.label.len() == 4,
                "audio label must have four ASCII bytes"
            );
            request.label.make_ascii_lowercase();
            let key: String = request
                .label
                .bytes()
                .map(|b| format!("{:02x}", b.to_ascii_lowercase()))
                .collect();
            let handle = world
                .resource::<AssetServer>()
                .load(format!("cache://dialogue/{key}.dialogue"));
            world.resource_mut::<Pending>().0 = Some((request, handle));
        }
        let Some((request, handle)) = world.resource_mut::<Pending>().0.take() else {
            return Ok(());
        };
        let id = request.id;
        let started = (|| -> anyhow::Result<bool> {
            let Some(clip) = world
                .resource::<Assets<DialogueClip>>()
                .get(&handle)
                .cloned()
            else {
                if let Some(bevy::asset::LoadState::Failed(error)) =
                    world.resource::<AssetServer>().get_load_state(handle.id())
                {
                    anyhow::bail!("audio dialogue {}: {error}", request.label);
                }
                return Ok(false);
            };
            let voice =
                clip.entry.voice.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("audio dialogue {} has no voice", request.label)
                })?;
            let audio = clip
                .audio
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("audio voice dependency absent"))?;
            if !world.resource::<Assets<AudioSource>>().contains(audio) {
                return Ok(false);
            }
            let actors = world.resource::<cyrus_script::script::ScriptActors>();
            let current = actors
                .current
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("audio script world absent"))?;
            anyhow::ensure!(
                current.generation() == request.generation,
                "audio generation changed"
            );
            anyhow::ensure!(
                !current.inspect(request.index, 1)["actors"]
                    .as_array()
                    .is_none_or(Vec::is_empty),
                "audio actor absent"
            );
            if let Some(effect) = request.effect {
                let metadata = usize::try_from(effect)
                    .ok()
                    .and_then(|i| {
                        world
                            .resource::<cyrus_audio::sound::SoundEffects>()
                            .0
                            .get(i)
                    })
                    .and_then(Option::as_ref)
                    .ok_or_else(|| anyhow::anyhow!("audio effect {effect} unavailable"))?;
                anyhow::ensure!(
                    metadata.repetitions.is_infinite(),
                    "audio acceptance requires an installed looping effect"
                );
            }
            world.resource_mut::<Speech>().0.insert(
                request.label.clone(),
                cyrus_assets::sound::VoiceClip {
                    audio: audio.clone(),
                    bits_per_sample: voice.bits_per_sample,
                    base_volume: voice.base_volume,
                    seconds: f64::from(voice.lifetime) / 256.0,
                    text: clip.entry.text,
                },
            );
            world.write_message(SoundRequest::Voice(request.label.clone()));
            if let Some(effect) = request.effect {
                world.write_message(cyrus_world::sound::SoundCue::Script {
                    generation: request.generation,
                    index: request.index,
                    sound: cyrus_world::sound::ScriptSound::Flat {
                        effect,
                        volume: 0,
                        rate_offset_hz: 0,
                    },
                });
            }
            Ok(true)
        })();
        let response = match started {
            Ok(false) => {
                world.resource_mut::<Pending>().0 = Some((request, handle));
                serde_json::json!({"id": id, "pending": true})
            }
            Ok(true) => serde_json::json!({"id": id, "started": true}),
            Err(error) => serde_json::json!({"id": id, "error": format!("{error:#}")}),
        };
        js_sys::Reflect::set(
            &window,
            &"__cyrusAudioResult".into(),
            &js_sys::JSON::parse(&response.to_string())
                .map_err(|e| anyhow::anyhow!("audio response: {e:?}"))?,
        )
        .map_err(|e| anyhow::anyhow!("audio response: {e:?}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser audio acceptance: {error:#}");
    }
}
