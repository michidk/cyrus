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
            .add_message::<cyrus_render::sound::SoundRequest>()
            .init_resource::<TapRelease>()
            .add_systems(
                PreUpdate,
                (release_taps, handle)
                    .chain()
                    .after(bevy::input::InputSystems),
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
    requests: MessageWriter<'w, cyrus_render::sound::SoundRequest>,
    effects: Option<Res<'w, cyrus_render::sound::SoundEffects>>,
    speech: Option<Res<'w, cyrus_render::sound::Speech>>,
    music: Option<Res<'w, cyrus_render::music::Music>>,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Inspection<'w> {
    preloaded: Option<Res<'w, Preloaded>>,
    hud: Option<Res<'w, cyrus_render::hud::HudState>>,
    scripts: Option<Res<'w, cyrus_game::script::ScriptStatus>>,
    script_actors: Option<ResMut<'w, cyrus_game::script::ScriptActors>>,
}

#[allow(clippy::too_many_arguments)]
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
    (state, mut next): (
        Res<State<cyrus_game::AppState>>,
        ResMut<NextState<cyrus_game::AppState>>,
    ),
    (startup, menu): (
        Option<Res<cyrus_render::startup::StartupPresentation>>,
        Option<Res<cyrus_render::startup::MenuState>>,
    ),
    Inspection {
        preloaded,
        hud,
        scripts,
        mut script_actors,
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
    while let Ok(Pending { request, reply }) = rx.try_recv() {
        let id = request.id;
        let result = match request.cmd {
            Command::Ping => Ok(json!("pong")),
            Command::State => {
                let transform = player.single().ok().map(|t| {
                    let (yaw, pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
                    json!({"position": t.translation.to_array(), "yaw": yaw, "pitch": pitch})
                });
                let preload = preloaded.as_ref().map(|p| {
                    json!({"parsed": p.parsed, "failed": p.failed.len(), "by_type": p.by_type, "unsupported": p.unsupported, "report": "cyrus-config/preload-report.json"})
                });
                Ok(
                    json!({"state": format!("{:?}", state.get()), "menu": menu.as_deref(), "hud": hud.as_deref(), "scripts": scripts.as_deref(), "scene": current.name, "player": transform, "fidelity": *fidelity, "frame": frames.0, "preload": preload, "player_input": *input, "music_track": audio.music.as_ref().and_then(|m| m.track), "flycam": {"enabled": flycam.as_ref().is_some_and(|f| f.enabled), "position": camera.single().ok().map(|t| t.translation.to_array())}}),
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
            Command::LoadScene { name } => {
                scene_request.0 = name.clone();
                if *state.get() != cyrus_game::AppState::InGame {
                    next.set(cyrus_game::AppState::InGame);
                }
                Ok(json!({"scene": name}))
            }
            Command::SkipStartup => {
                if startup.is_some() && *state.get() != cyrus_game::AppState::InGame {
                    if *state.get() != cyrus_game::AppState::Menu {
                        next.set(cyrus_game::AppState::Menu);
                    }
                    Ok(json!({"state": "Menu"}))
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
                            .write(cyrus_render::sound::SoundRequest::Play {
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
                        .write(cyrus_render::sound::SoundRequest::Speech(label.clone()));
                    Ok(json!({"label": label}))
                } else {
                    Err(format!("voice clip {label} is not loaded"))
                }
            }
            Command::StopSounds => {
                audio
                    .requests
                    .write(cyrus_render::sound::SoundRequest::StopAll);
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
            Command::StepScript {
                generation,
                index,
                stage,
            } => {
                match script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                {
                    Some(world) => {
                        let id = cyrus_game::script::ScriptActorId { generation, index };
                        let result = match stage {
                            cyrus_control::ScriptStage::Tasks => world.step_tasks(id),
                            cyrus_control::ScriptStage::Main => {
                                world.step(id, cyrus_game::script::Thread::Main, 100_000)
                            }
                            cyrus_control::ScriptStage::Interrupt => {
                                world.step(id, cyrus_game::script::Thread::Interrupt, 100_000)
                            }
                        };
                        result
                            .map(|()| world.inspect(index, 1))
                            .map_err(|error| format!("{error:#}"))
                    }
                    None => Err("script world is not prepared".into()),
                }
            }
            Command::Quit => {
                exit.write(AppExit::Success);
                Ok(json!("bye"))
            }
        };
        let _ = reply.send(match result {
            Ok(v) => Response::ok(id, v),
            Err(e) => Response::err(id, e),
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

    #[test]
    fn control_taps_survive_keyboard_update_and_release_next_frame() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            bevy::input::InputPlugin,
        ))
        .init_state::<cyrus_game::AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<CurrentScene>()
        .init_resource::<bevy::diagnostic::FrameCount>()
        .init_resource::<cyrus_game::player::input::PlayerInput>()
        .init_resource::<cyrus_render::camera::FlycamSettings>()
        .insert_resource(SceneRequest(String::new()))
        .insert_resource(ControlAddr("127.0.0.1:0".into()))
        .add_plugins(ControlPlugin);
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
}
