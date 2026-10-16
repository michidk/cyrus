//! TCP control server (see `cyrus-control`). A thread accepts connections and
//! forwards each request into the ECS; one system answers them in frame order.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use cyrus_assets::loading::Preloaded;
use cyrus_control::server::Pending;
use cyrus_control::{Command, InputAction, Response};
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use cyrus_game::player::Player;
use cyrus_world::lifecycle::CurrentScene;
use cyrus_world::lifecycle::SceneRequest;
use serde_json::json;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

pub struct ControlPlugin;

/// Bind address; set before the plugin builds. Defaults to `cyrus_control::DEFAULT_ADDR`.
#[derive(Resource, Clone, Debug)]
pub struct ControlAddr(pub String);

#[derive(Resource)]
struct Inbox(Mutex<Receiver<Pending>>);

/// Keys tapped this frame, released next frame.
#[derive(Resource, Default)]
struct TapRelease(Vec<KeyCode>);

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        let addr = app
            .world()
            .get_resource::<ControlAddr>()
            .map(|a| a.0.clone())
            .unwrap_or_else(|| cyrus_control::DEFAULT_ADDR.into());
        let rx = match cyrus_control::server::listen(&addr) {
            Ok(rx) => {
                info!("control: listening on {addr}");
                rx
            }
            Err(error) => {
                error!("control: cannot bind {addr}: {error}");
                std::sync::mpsc::channel().1
            }
        };
        app.insert_resource(Inbox(Mutex::new(rx)))
            .add_message::<cyrus_audio::sound::SoundRequest>()
            .init_resource::<TapRelease>()
            .init_resource::<crate::session::NativeSessionLoad>()
            .add_systems(
                PreUpdate,
                (crate::session::poll, release_taps, handle)
                    .chain()
                    .after(bevy::input::InputSystems)
                    .before(cyrus_console::ConsoleInput),
            );
    }
}

fn release_taps(mut taps: ResMut<TapRelease>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    for key in taps.0.drain(..) {
        keys.release(key);
    }
}

/// Audio state for `handle`, grouped because Bevy systems take at most 16 parameters.
#[derive(bevy::ecs::system::SystemParam)]
struct Audio<'w> {
    requests: MessageWriter<'w, cyrus_audio::sound::SoundRequest>,
    effects: Option<Res<'w, cyrus_audio::sound::SoundEffects>>,
    speech: Option<Res<'w, cyrus_audio::sound::Speech>>,
    music: Option<Res<'w, cyrus_audio::music::Music>>,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Inspection<'w, 's> {
    preloaded: Option<Res<'w, Preloaded>>,
    textures: Option<ResMut<'w, cyrus_render::fog::TextureAnimations>>,
    hud: Option<Res<'w, cyrus_render::hud::HudState>>,
    script_hud: Option<Res<'w, cyrus_game::script::ScriptHud>>,
    logbook: Option<Res<'w, cyrus_game::logbook::LogbookScreen>>,
    scripts: Option<Res<'w, cyrus_game::script::ScriptStatus>>,
    script_actors: Option<ResMut<'w, cyrus_game::script::ScriptActors>>,
    random: ResMut<'w, cyrus_game::random::EngineRandom>,
    cheats: ResMut<'w, cyrus_game::cheats::Cheats>,
    console: Res<'w, cyrus_console::Console>,
    activation: Option<Res<'w, cyrus_game::script::ActivationInputs>>,
    prepared: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    ready_scene: Option<Res<'w, cyrus_render::ReadyScene>>,
    animations: Res<'w, Assets<cyrus_assets::animation::Animations>>,
    script_nodes: Query<'w, 's, (Entity, &'static Name)>,
    binding_nodes: Query<'w, 's, (Entity, &'static cyrus_game::script::ScriptBinding)>,
    script_sounds: Option<ResMut<'w, Messages<cyrus_world::sound::SoundCue>>>,
    player_poses: Query<'w, 's, &'static cyrus_game::player::placement::PlayerPose, With<Player>>,
}

// Control dispatch joins input, session state and presentation resources.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle(
    mut commands: Commands,
    inbox: Res<Inbox>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut taps: ResMut<TapRelease>,
    (mut fidelity, mut sky_scroll): (
        ResMut<Fidelity>,
        Option<ResMut<cyrus_render::environment::SkyScrollSync>>,
    ),
    mut scene_request: ResMut<SceneRequest>,
    current: Res<CurrentScene>,
    (state, mut next, mut session, mut native_load, install): (
        Res<State<cyrus_game::AppState>>,
        ResMut<NextState<cyrus_game::AppState>>,
        Option<ResMut<cyrus_session::SessionBoundary>>,
        ResMut<crate::session::NativeSessionLoad>,
        Option<Res<cyrus_assets::install::GameInstall>>,
    ),
    (mut startup, menu): (
        Option<ResMut<cyrus_startup::StartupPresentation>>,
        Option<Res<cyrus_startup::MenuState>>,
    ),
    Inspection {
        preloaded,
        mut textures,
        hud,
        script_hud,
        logbook,
        scripts,
        mut script_actors,
        mut random,
        mut cheats,
        console,
        activation,
        prepared,
        ready_scene,
        animations,
        script_nodes,
        binding_nodes,
        mut script_sounds,
        player_poses,
    }: Inspection,
    frames: Res<bevy::diagnostic::FrameCount>,
    (input, flycam): (
        Res<cyrus_game::player::input::PlayerInput>,
        Option<Res<cyrus_render::camera::FlycamSettings>>,
    ),
    camera: Query<&Transform, (With<cyrus_render::camera::ViewCamera>, Without<Player>)>,
    mut player: Query<&mut Transform, With<Player>>,
    mut exit: MessageWriter<AppExit>,
    mut audio: Audio,
) {
    let rx = inbox.0.lock().unwrap();
    let mut script_bindings: std::collections::BTreeMap<_, _> = binding_nodes
        .iter()
        .map(|(entity, binding)| (entity, binding.actor))
        .collect();
    while let Ok(Pending { request, reply }) = rx.try_recv() {
        let id = request.id;
        // Polling and per-step script commands arrive by the thousand; never log them.
        if !matches!(
            request.cmd,
            Command::Ping
                | Command::State
                | Command::Scripts { .. }
                | Command::BindScriptAnimation { .. }
                | Command::StepScript { .. }
        ) {
            debug!("control #{id}: {:?}", request.cmd);
        }
        let result = match request.cmd {
            Command::Ping => Ok(json!("pong")),
            Command::Cheat { name, value } => cheats.execute(&name, value)
                .map(|effect| json!({"effect":effect,"cheats":cheats.inspect()}))
                .map_err(|error| format!("{error:#}")),
            Command::Console { open, line } => {
                if *state.get() != cyrus_game::AppState::InGame {
                    Err("console requires an active world".into())
                } else {
                    commands.queue(move |world: &mut World| {
                        if let Some(open) = open { world.resource_mut::<cyrus_console::Console>().open = open; }
                        if let Some(line) = line { let _ = cyrus_console::submit(world, &line, cyrus_console::Source::Interactive); }
                        let result = json!({"console": world.resource::<cyrus_console::Console>(), "cheats": world.resource::<cyrus_game::cheats::Cheats>().inspect()});
                        let _ = reply.send(Response::ok(id, result));
                    });
                    // Later requests must observe this command after its world mutation.
                    break;
                }
            }
            Command::SessionLoad { path } => install.as_ref()
                .ok_or_else(|| anyhow::anyhow!("native session install unavailable"))
                .and_then(|install| native_load.start(path, (**install).clone()))
                .map(|()| json!({"pending":true}))
                .map_err(|error| format!("{error:#}")),
            Command::SessionLoadResult { acknowledge } => {
                let completion = native_load.completion();
                if acknowledge && completion.is_none() {
                    Err("native session load has no terminal result".into())
                } else {
                    if acknowledge { let _ = native_load.acknowledge(); }
                    match completion {
                        None => Ok(json!({"pending":true})),
                        Some(Ok(())) => Ok(json!({"pending":false,"committed":true})),
                        Some(Err(error)) => Err(error),
                    }
                }
            },
            Command::SessionCapture | Command::SessionCaptureResult if native_load.busy() =>
                Err("native session load owns the pending boundary".into()),
            Command::SessionCapture => session.as_mut()
                .ok_or_else(|| anyhow::anyhow!("session boundary unavailable"))
                .and_then(|boundary| boundary.capture_live())
                .map(|()| json!({"pending":true}))
                .map_err(|error| format!("{error:#}")),
            Command::SessionCaptureResult => (|| -> anyhow::Result<serde_json::Value> {
                let boundary = session.as_mut().ok_or_else(|| anyhow::anyhow!("session boundary unavailable"))?;
                match boundary.take_result() {
                    None => Ok(json!({"pending":true})),
                    Some(Err(error)) => Err(error),
                    Some(Ok(cyrus_session::SessionBoundaryResult::Captured(snapshot))) => {
                        let document = snapshot.encode_document()?;
                        Ok(json!({"pending":false,"bytes":document.len(),"document":String::from_utf8(document)?}))
                    }
                    Some(Ok(cyrus_session::SessionBoundaryResult::Committed)) => Ok(json!({"pending":false,"committed":true})),
                }
            })().map_err(|error| format!("{error:#}")),
            Command::State => {
                let scene_ready = ready_scene.as_ref().is_some_and(|ready| {
                    prepared.as_ref().is_some_and(|world| ready.0 == world.animations)
                });
                let transform = player.single().ok().map(|t| {
                    let (yaw, pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
                    json!({"position": t.translation.to_array(), "yaw": yaw, "pitch": pitch})
                });
                let preload = preloaded.as_ref().map(|p| {
                    json!({"parsed": p.parsed, "failed": p.failed.len(), "by_type": p.by_type, "unsupported": p.unsupported, "report": "cyrus-config/preload-report.json"})
                });
                Ok(
                    json!({"textures": textures.as_ref().map(|textures| textures.snapshot()), "state": format!("{:?}", state.get()), "menu": menu.as_deref(), "startup": startup.as_deref(), "hud": hud.as_deref(), "logbook": logbook.as_deref(), "script_hud": script_hud.as_ref().map(|hud| hud.inspect()), "cheats": cheats.inspect(), "console": *console, "scripts": scripts.as_deref(), "scene": current.name, "scene_ready": scene_ready, "player": transform, "fidelity": *fidelity, "frame": frames.0, "preload": preload, "player_input": *input, "music_track": audio.music.as_ref().and_then(|m| m.track), "flycam": {"enabled": flycam.as_ref().is_some_and(|f| f.enabled), "position": camera.single().ok().map(|t| t.translation.to_array()), "rotation": camera.single().ok().map(|t| t.rotation.to_array())}}),
                )
            }
            Command::Screenshot { path } => {
                let path = std::path::PathBuf::from(path);
                match path.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())) {
                    Ok(()) => {
                        commands
                            .spawn(Screenshot::primary_window())
                            .observe(save_to_disk(path.clone()));
                        Ok(json!({"path": path}))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Command::Teleport { x, y, z } => match player.single_mut() {
                Ok(mut t) => {
                    t.translation = Vec3::new(x, y, z);
                    Ok(json!({"position": [x, y, z]}))
                }
                Err(_) => Err("no player".into()),
            },
            Command::Input { key, action } => match parse_key(&key) {
                Some(code) => {
                    match action {
                        InputAction::Press => keys.press(code),
                        InputAction::Release => keys.release(code),
                        InputAction::Tap => {
                            keys.press(code);
                            taps.0.push(code);
                        }
                    }
                    Ok(json!({"key": key, "action": action}))
                }
                None => Err(format!("unknown key {key}")),
            },
            Command::SetFidelity { area, mode } => {
                match (
                    serde_json::from_value::<Area>(json!(area)),
                    serde_json::from_value::<Mode>(json!(mode)),
                ) {
                    (Ok(area), Ok(mode)) => {
                        fidelity.set(area, mode);
                        Ok(json!(*fidelity))
                    }
                    _ => Err(
                        "area must be player_controller|camera|water|fog|sky|lighting, mode faithful|modern"
                            .into(),
                    ),
                }
            }
            Command::SetSkyScroll { direction, offset } => match sky_scroll.as_deref_mut() {
                Some(reference) => reference
                    .set(direction, offset)
                    .map(|()| json!({"direction": direction, "offset": offset}))
                    .map_err(|error| error.to_string()),
                None => Err("sky renderer is unavailable".into()),
            },
            Command::HoldTextureFrame { world, image, frame } => {
                if *state.get() != cyrus_game::AppState::InGame || world != scene_request.0 || !ready_scene.as_ref().is_some_and(|ready| {
                    prepared.as_ref().is_some_and(|world| ready.0 == world.animations)
                }) {
                    Err("texture capture requires the requested ready world".into())
                } else if let Some(textures) = textures.as_deref_mut() {
                    textures.hold(&image, frame)
                        .map(|()| json!({"textures": textures.snapshot()}))
                        .map_err(|error| error.to_string())
                } else {
                    Err("texture renderer is unavailable".into())
                }
            }
            Command::LoadScene { .. } if *state.get() == cyrus_game::AppState::SelectingInstall => {
                Err("select and prepare the game files before loading a scene".into())
            }
            Command::LoadScene { name } => {
                scene_request.0 = name.clone();
                if *state.get() != cyrus_game::AppState::InGame {
                    next.set(cyrus_game::AppState::InGame);
                }
                Ok(json!({"scene": name}))
            }
            Command::SkipStartup => {
                if startup.is_some() && *state.get() != cyrus_game::AppState::InGame {
                    if let Some(startup) = startup.as_mut() {
                        startup.skip = true;
                    }
                    Ok(json!({"state": format!("{:?}", state.get()), "destination": "Menu"}))
                } else {
                    Err("startup presentation is not active".into())
                }
            }
            Command::PlaySound { effect, position } => {
                match audio
                    .effects
                    .as_ref()
                    .and_then(|e| e.0.get(effect)?.as_ref())
                {
                    Some(loaded) => {
                        audio
                            .requests
                            .write(cyrus_audio::sound::SoundRequest::Play {
                                effect,
                                position: position.map(Vec3::from_array),
                            });
                        Ok(json!({"effect": effect, "looping": loaded.looping}))
                    }
                    None => Err(format!("sound effect {effect} is not loaded")),
                }
            }
            Command::PlaySpeech { label } => {
                if audio
                    .speech
                    .as_ref()
                    .is_some_and(|s| s.0.contains_key(&label))
                {
                    audio
                        .requests
                        .write(cyrus_audio::sound::SoundRequest::Speech(label.clone()));
                    Ok(json!({"label": label}))
                } else {
                    Err(format!("voice clip {label} is not loaded"))
                }
            }
            Command::StopSounds => {
                audio
                    .requests
                    .write(cyrus_audio::sound::SoundRequest::StopAll);
                Ok(json!("stopped"))
            }
            Command::Scripts { offset, limit } => {
                match script_actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                {
                    Some(world) => Ok(world.inspect(offset, limit)),
                    None => Err("script world is not prepared".into()),
                }
            }
            Command::BindScriptAnimation {
                generation,
                index,
                default_group,
                node,
            } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                let source = &prepared
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("world is not prepared"))?
                    .animations;
                let data = animations
                    .get(source)
                    .ok_or_else(|| anyhow::anyhow!("animation data is not loaded"))?;
                let entity = if let Some(name) = node {
                    let matches: Vec<_> = script_nodes
                        .iter()
                        .filter(|(_, candidate)| candidate.as_str() == name)
                        .collect();
                    anyhow::ensure!(
                        matches.len() == 1,
                        "scene node {name:?}: expected one match, found {}",
                        matches.len()
                    );
                    anyhow::ensure!(
                        script_bindings.get(&matches[0].0).is_none_or(|bound| bound.generation != generation),
                        "scene node {name:?} already has a script binding"
                    );
                    Some(matches[0].0)
                } else {
                    None
                };
                let actor = cyrus_game::script::ScriptActorId { generation, index };
                world.bind_animation(actor, data, default_group)?;
                if let Some(entity) = entity {
                    script_bindings.insert(entity, actor);
                    commands
                        .entity(entity)
                        .remove::<(
                            cyrus_game::script::ScriptedPlayback,
                            cyrus_world::animation::AnimationPlayback,
                            cyrus_world::animation::AnimationPose,
                        )>()
                        .insert(cyrus_game::script::ScriptBinding {
                            actor,
                            source: source.clone(),
                        });
                }
                Ok(world.inspect(index, 1))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::StepScript {
                generation,
                index,
                stage,
                frame_scale,
                nominal_ticks,
                distance_override,
                fallback_position,
            } => (|| -> anyhow::Result<_> {
                use cyrus_game::script::{ScriptActorId, ScriptStage, Thread};
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                let id = ScriptActorId { generation, index };
                anyhow::ensure!(world.id(index)? == id, "stale script actor generation");
                match stage {
                    cyrus_control::ScriptStage::PrepareSpatial => world.prepare_spatial()?,
                    cyrus_control::ScriptStage::Snapshot => world.snapshot_spatial(id)?,
                    cyrus_control::ScriptStage::AxisLocks => world.restore_axis_locks(id)?,
                    cyrus_control::ScriptStage::Transform => world.refresh_transform(id)?,
                    cyrus_control::ScriptStage::Families => world.refresh_families()?,
                    _ => {},
                }
                let stage = match stage {
                    cyrus_control::ScriptStage::PrepareSpatial | cyrus_control::ScriptStage::Snapshot
                    | cyrus_control::ScriptStage::AxisLocks | cyrus_control::ScriptStage::Transform | cyrus_control::ScriptStage::Families => return Ok(world.inspect(index, 1)),
                    cyrus_control::ScriptStage::Animation => ScriptStage::Animation,
                    cyrus_control::ScriptStage::OrdinaryMain => ScriptStage::OrdinaryMain,
                    cyrus_control::ScriptStage::OrdinaryHook => ScriptStage::OrdinaryHook,
                    cyrus_control::ScriptStage::NativeHook => ScriptStage::NativeHook,
                    cyrus_control::ScriptStage::Entry => ScriptStage::Entry,
                    cyrus_control::ScriptStage::Tasks => ScriptStage::Tasks,
                    cyrus_control::ScriptStage::Main => ScriptStage::Body(Thread::Main),
                    cyrus_control::ScriptStage::Interrupt => ScriptStage::Body(Thread::Interrupt),
                };
                let mut captured_sounds = Vec::new();
                let mut emit = |actor: ScriptActorId, sound| -> anyhow::Result<()> {
                    use cyrus_game::script::ScriptSound;
                    let (sound, surface, position) = match sound {
                        ScriptSound::Animation { sound, surface, position } => (sound, surface, position),
                        sound => {
                            let event = match &sound {
                                ScriptSound::Flat { effect, volume, rate_offset_hz } => json!({"type":"flat", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz}),
                                ScriptSound::Positional { effect, volume, rate_offset_hz, position } => json!({"type":"positional", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz, "position":position}),
                                ScriptSound::StopEffect { effect, owned } => json!({"type":"stop_effect", "effect":effect, "owned":owned}),
                                ScriptSound::StopOwned => json!({"type":"stop_owned"}),
                                ScriptSound::StopAll => json!({"type":"stop_all"}),
                                ScriptSound::AnimationState(state) => json!({"type":"animation_state", "state":format!("{state:?}")}),
                                ScriptSound::Animation { .. } => unreachable!(),
                            };
                            captured_sounds.push(json!({"actor":actor, "sound":event}));
                            if let Some(sounds) = script_sounds.as_mut() {
                                sounds.write(cyrus_world::sound::SoundCue::Script {
                                    generation: actor.generation, index: actor.index, sound,
                                });
                            }
                            return Ok(());
                        }
                    };
                    let mut nodes = script_bindings.iter().filter(|(_, bound)| **bound == actor);
                    let Some((entity, _)) = nodes.next() else {
                        use cyrus_world::animation::AnimationSound;
                        let event = match sound {
                            AnimationSound::Play { effect, rate_offset_hz } =>
                                json!({"type":"play", "effect":effect, "rate_offset_hz":rate_offset_hz}),
                            AnimationSound::Stop => json!({"type":"stop"}),
                            AnimationSound::Forget => json!({"type":"forget"}),
                        };
                        captured_sounds.push(json!({"actor":actor, "sound":event}));
                        return Ok(());
                    };
                    let entity = *entity;
                    anyhow::ensure!(
                        nodes.next().is_none(),
                        "script sound receiver has multiple scene bindings"
                    );
                    script_sounds
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("script sound consumer unavailable"))?
                        .write(cyrus_world::sound::SoundCue::Animation {
                            actor: entity,
                            sound,
                            surface,
                            position,
                        });
                    Ok(())
                };
                let mut poses = player_poses.iter();
                let selected = poses.next();
                anyhow::ensure!(poses.next().is_none(), "selected-player query has multiple player poses");
                let player = selected.map(|pose| -> anyhow::Result<_> {
                    let mut position = [0; 3];
                    for (axis, coordinate) in pose.position.into_iter().enumerate() {
                        position[axis] = i32::try_from(coordinate).map_err(|_| anyhow::anyhow!("player query coordinate axis {axis} outside Q8 i32 range"))?;
                    }
                    Ok(cyrus_game::script::PlayerQuery { position, heading: pose.heading })
                }).transpose()?;
                let inputs = cyrus_game::script::ScriptInputs {
                    hud: script_hud.as_deref(),
                    activation: activation.as_deref(),
                    nominal_ticks, frame_scale,
                    player_query: Some(cyrus_game::script::QueryInputs {
                        player, distance_override: distance_override.or_else(|| cheats.distance_override().ok()), fallback_position, linked_position: None,
                    }),
                    ..Default::default()
                };
                world.advance(id, stage, inputs, Some(&mut emit), Some(&mut random), 100_000)?;
                let mut result = world.inspect(index, 1);
                result["captured_sounds"] = json!(captured_sounds);
                Ok(result)
            })()
            .map_err(|error| format!("{error:#}")),
            Command::Quit => {
                exit.write(AppExit::Success);
                Ok(json!("bye"))
            }
        };
        let _ = reply.send(match result {
            Ok(v) => Response::ok(id, v),
            Err(e) => {
                debug!("control #{id}: rejected: {e}");
                Response::err(id, e)
            }
        });
    }
}

/// `KeyCode` is a plain enum; its `Debug` names are the public key names.
fn parse_key(name: &str) -> Option<KeyCode> {
    serde_json::from_value(json!(name)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_control::Request;
    use std::sync::mpsc::channel;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            bevy::input::InputPlugin,
        ))
        .init_state::<cyrus_game::AppState>()
        .init_resource::<Assets<cyrus_assets::animation::Animations>>()
        .init_resource::<Fidelity>()
        .init_resource::<cyrus_game::random::EngineRandom>()
        .init_resource::<cyrus_game::cheats::Cheats>()
        .add_plugins(cyrus_game::console::GameConsolePlugin)
        .init_resource::<CurrentScene>()
        .init_resource::<bevy::diagnostic::FrameCount>()
        .init_resource::<cyrus_game::player::input::PlayerInput>()
        .init_resource::<cyrus_render::camera::FlycamSettings>()
        .insert_resource(SceneRequest(String::new()))
        .insert_resource(ControlAddr("127.0.0.1:0".into()))
        .add_plugins(ControlPlugin);
        app
    }

    #[test]
    fn console_reply_contains_dispatched_output_and_validation_errors() {
        let mut app = app();
        app.insert_state(cyrus_game::AppState::InGame);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        for (id, line, expected) in [(1, "commands", "console [con]"), (2, "é", "ASCII")] {
            let (reply, result) = channel();
            tx.send(Pending {
                request: Request {
                    id,
                    cmd: Command::Console {
                        open: Some(true),
                        line: Some(line.into()),
                    },
                },
                reply: reply.clone(),
            })
            .unwrap();
            tx.send(Pending {
                request: Request {
                    id: id + 100,
                    cmd: Command::State,
                },
                reply,
            })
            .unwrap();
            app.update();
            let response = result.try_recv().unwrap();
            assert!(response.ok);
            assert!(serde_json::to_string(&response).unwrap().contains(expected));
            assert!(result.try_recv().is_err());
            app.update();
            let state = result.try_recv().unwrap().result.unwrap();
            assert_eq!(state["console"]["open"], true);
            assert!(
                state["console"]["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|line| { line["text"].as_str().unwrap().contains(expected) })
            );
        }
    }

    #[test]
    fn scene_requests_cannot_bypass_install_selection() {
        let mut app = app();
        app.insert_state(cyrus_game::AppState::SelectingInstall);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, result) = channel();
        tx.send(Pending {
            request: Request {
                id: 1,
                cmd: Command::LoadScene {
                    name: "START".into(),
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(!result.try_recv().unwrap().ok);
        assert_eq!(
            *app.world().resource::<State<cyrus_game::AppState>>().get(),
            cyrus_game::AppState::SelectingInstall
        );
        assert!(app.world().resource::<SceneRequest>().0.is_empty());
    }

    #[test]
    fn control_taps_survive_keyboard_update_and_release_next_frame() {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, result) = channel();
        tx.send(Pending {
            request: Request {
                id: 1,
                cmd: Command::Input {
                    key: "Enter".into(),
                    action: InputAction::Tap,
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(result.try_recv().unwrap().ok);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Enter)
        );
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.pressed(KeyCode::Enter));
        assert!(keys.just_released(KeyCode::Enter));
    }

    fn script_app() -> App {
        use cyrus_assets::{animation::Animations, script::Scripts};
        use cyrus_game::script::{ScriptActors, ScriptWorld};
        let mut app = app();
        let dialogue = json!({"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"});
        let mut script_json = json!({
            "version":15,"dialogue":dialogue,"player_distance_override":false,"updates":{"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false},"clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},"actor_pool_capacity":10,"content_id":"","world":17,
            "definition":{"functions":[
                {"name":"NullFunction","param_count":0,"kind":"function"},
                {"name":"PushAnimation","param_count":2,"kind":"task"}
            ],"flags":[],"references":[],"attributes":[]},
            "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
                "namespace":"ACTOR","instance_variables":[[]],"location_offsets":[],"attributes":{},"native_hook":null,
                "program":{"version":2,"entry":0,"steps":{
                    "0":{"next":1,"operation":{"type":"call","call":{
                        "function_id":1,"kind":"task","target":{"type":"me"},
                        "function":"PushAnimation","arguments":[{"type":"integer","value":7},{"type":"integer","value":1}]
                    }}},
                    "1":{"next":2,"operation":{"type":"end","target":0}}
                }}
            }},
            "instances":[{"object_id":0,"actor":"ACTOR","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}],
            "unsupported":{}
        });
        script_json["combat_attacks"] = json!([]);
        let mut scripts: Scripts = serde_json::from_value(script_json).unwrap();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut actors = ScriptActors::default();
        actors.current = Some(ScriptWorld::prepare(&scripts, 1).unwrap());
        let data: Animations = serde_json::from_value(json!({
            "version":2,"world":17,"sections":{},"models":{},"unsupported":{},
            "metadata":{"script_content_id":scripts.content_id,"actors":[{
                "script_name":"ACTOR","animation_groups":[
                    {"group_index":1,"anim_type":0,"anim_id":1,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":4,"sound_param":-17,"volume_shift":-1},
                        {"opcode":0,"frame_reference":1},{"opcode":2,"value":0},{"opcode":1}]}
                ]
            }]}
        })).unwrap();
        let animations = app
            .world_mut()
            .resource_mut::<Assets<Animations>>()
            .add(data);
        app.insert_resource(actors)
            .insert_resource(cyrus_world::lifecycle::PreparedWorld {
                requested_name: "TEST".into(),
                world: Some(17),
                animations,
                scripts: default(),
                player_path: "test.player".into(),
                scene_path: "test.glb".into(),
                environment_path: "test.environment".into(),
            });
        app
    }

    fn run_commands(app: &mut App, commands: Vec<Command>) -> Vec<serde_json::Value> {
        let (tx, rx) = channel();
        let (reply, results) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let count = commands.len();
        for (id, cmd) in commands.into_iter().enumerate() {
            tx.send(Pending {
                request: Request { id: id as u64, cmd },
                reply: reply.clone(),
            })
            .unwrap();
        }
        app.update();
        (0..count)
            .map(|_| {
                let response = results.try_recv().unwrap();
                assert!(response.ok, "{:?}", response.error);
                response.result.unwrap()
            })
            .collect()
    }

    #[test]
    fn capture_readiness_requires_the_current_world_to_finish_spawning() {
        let mut app = script_app();
        app.insert_state(cyrus_game::AppState::InGame);
        let state = run_commands(&mut app, vec![Command::State]).remove(0);
        assert_eq!(state["state"], "InGame");
        assert_eq!(state["scene_ready"], false);

        let current = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>()
            .animations
            .clone();
        app.insert_resource(cyrus_render::ReadyScene(current));
        let state = run_commands(&mut app, vec![Command::State]).remove(0);
        assert_eq!(state["scene_ready"], true);

        app.world_mut()
            .resource_mut::<cyrus_world::lifecycle::PreparedWorld>()
            .animations = default();
        let state = run_commands(&mut app, vec![Command::State]).remove(0);
        assert_eq!(state["scene_ready"], false);
    }

    #[test]
    fn cheat_command_supplies_player_distance_state_to_script_steps() {
        use cyrus_game::cheats::{CheatStore, Cheats};
        struct Memory;
        impl CheatStore for Memory {
            fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
                Ok(cyrus_assets::cheats::empty())
            }
            fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let mut app = app();
        let dialogue = json!({"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"});
        let mut script_json = json!({
            "version":15,"dialogue":dialogue,"player_distance_override":false,"updates":{"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false},
            "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},
            "actor_pool_capacity":10,"content_id":"","world":17,
            "definition":{"functions":[
                {"name":"NullFunction","param_count":0,"kind":"function"},
                {"name":"PlayerDistance","param_count":0,"kind":"function"}
            ],"flags":[],"references":[],"attributes":[]},
            "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
                "namespace":"ACTOR","instance_variables":[[0]],"location_offsets":[],"attributes":{},"native_hook":null,
                "program":{"version":2,"entry":0,"variables":[0],"steps":{
                    "0":{"next":1,"operation":{"type":"assign","destination":{"type":"local","index":0},
                        "formula":{"first":{"type":"call","call":{"function_id":1,"kind":"function","target":{"type":"me"},"function":"PlayerDistance","arguments":[]}},"rest":[]}}},
                    "1":{"next":2,"operation":{"type":"end","target":0}}
                }}
            }},
            "instances":[{"object_id":0,"actor":"ACTOR","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}],"unsupported":{}
        });
        script_json["combat_attacks"] = json!([]);
        let mut scripts: cyrus_assets::script::Scripts =
            serde_json::from_value(script_json).unwrap();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut actors = cyrus_game::script::ScriptActors::default();
        actors.current = Some(cyrus_game::script::ScriptWorld::prepare(&scripts, 1).unwrap());
        app.insert_resource(actors)
            .insert_resource(Cheats::new(Memory));
        let step = |stage| Command::StepScript {
            generation: 1,
            index: 0,
            stage,
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            distance_override: None,
            fallback_position: None,
        };
        let responses = run_commands(
            &mut app,
            vec![
                Command::Cheat {
                    name: "neversaydie".into(),
                    value: Some(7),
                },
                step(cyrus_control::ScriptStage::PrepareSpatial),
                step(cyrus_control::ScriptStage::Main),
                Command::State,
            ],
        );
        assert_eq!(responses[0]["effect"], "player_distance");
        assert_eq!(responses[2]["actors"][0]["locals"], json!([9_999_999]));
        assert_eq!(responses[3]["cheats"]["ready"], true);
        assert_eq!(responses[3]["cheats"]["entries"][10]["value"], 7);
    }

    fn bind_and_step(node: Option<&str>) -> Vec<Command> {
        vec![
            Command::BindScriptAnimation {
                generation: 1,
                index: 0,
                default_group: 1,
                node: node.map(str::to_owned),
            },
            Command::StepScript {
                generation: 1,
                index: 0,
                stage: cyrus_control::ScriptStage::Main,
                frame_scale: None,
                nominal_ticks: Some(0),
                distance_override: None,
                fallback_position: None,
            },
        ]
    }

    #[test]
    fn node_less_script_sound_is_captured_without_poisoning_the_actor() {
        let mut app = script_app();
        let responses = run_commands(&mut app, bind_and_step(None));
        assert_eq!(
            responses[1]["captured_sounds"],
            json!([{
                "actor":{"generation":1,"index":0},
                "sound":{"type":"play","effect":-17,"rate_offset_hz":-64}
            }])
        );
        let response = run_commands(
            &mut app,
            vec![Command::StepScript {
                generation: 1,
                index: 0,
                stage: cyrus_control::ScriptStage::Tasks,
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                distance_override: None,
                fallback_position: None,
            }],
        );
        assert_eq!(response[0]["captured_sounds"], json!([]));
        let actors = app.world().resource::<cyrus_game::script::ScriptActors>();
        assert!(
            actors.current.as_ref().unwrap().actors()[0]
                .failure()
                .is_none()
        );
    }

    #[test]
    fn script_binding_routes_sound_in_the_same_command_drain() {
        use cyrus_world::{animation::AnimationSound, sound::SoundCue};
        let mut app = script_app();
        app.add_message::<SoundCue>();
        let source = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>()
            .animations
            .clone();
        let entity = app
            .world_mut()
            .spawn((
                Name::new("actor-node"),
                cyrus_game::script::ScriptBinding {
                    actor: cyrus_game::script::ScriptActorId {
                        generation: 0,
                        index: 0,
                    },
                    source,
                },
                cyrus_world::animation::AnimationPose {
                    model: "OLD".into(),
                    frame: 9,
                },
            ))
            .id();
        let responses = run_commands(&mut app, bind_and_step(Some("actor-node")));
        assert_eq!(responses[1]["captured_sounds"], json!([]));
        assert!(
            app.world()
                .get::<cyrus_world::animation::AnimationPose>(entity)
                .is_none()
        );
        let cues: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<SoundCue>>()
            .drain()
            .collect();
        assert_eq!(cues.len(), 1);
        assert!(matches!(cues[0], SoundCue::Animation {
            actor,
            sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(0)
        } if actor == entity));
        assert_eq!(
            app.world()
                .get::<cyrus_game::script::ScriptBinding>(entity)
                .unwrap()
                .actor
                .index,
            0
        );
    }

    #[test]
    fn animation_sound_uses_unnamed_player_bindings() {
        use cyrus_world::{animation::AnimationSound, sound::SoundCue};
        let mut app = script_app();
        app.add_message::<SoundCue>();
        let entity = app.world_mut().spawn(Name::new("player")).id();
        let mut commands = bind_and_step(Some("player"));
        let step = commands.pop().unwrap();
        run_commands(&mut app, commands);
        app.world_mut().entity_mut(entity).remove::<Name>();
        let responses = run_commands(&mut app, vec![step]);
        assert_eq!(responses[0]["captured_sounds"], json!([]));
        let cues: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<SoundCue>>()
            .drain()
            .collect();
        assert_eq!(cues.len(), 1);
        assert!(matches!(cues[0], SoundCue::Animation {
            actor, sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(0)
        } if actor == entity));
    }
}
