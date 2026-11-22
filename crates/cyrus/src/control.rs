//! TCP control server (see `cyrus-control`). A thread accepts connections and
//! forwards each request into the ECS; one system answers them in frame order.

mod effect_geometry;
mod frame_capture;
mod input_schedule;
mod light_image;
mod player_trace;
mod rendered_actor;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use cyrus_assets::loading::Preloaded;
use cyrus_control::server::Pending;
use cyrus_control::{Command, InputAction, Response};
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use cyrus_script::player::Player;
use cyrus_world::heading::wrap;
use cyrus_world::lifecycle::CurrentScene;
use cyrus_world::lifecycle::SceneRequest;
use serde_json::json;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

pub struct ControlPlugin;

/// The fixed simulation step of a `--replay-step-micros` run. Present only in replay mode.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ReplayStep(pub std::time::Duration);

/// A replay world advances only while a scheduled input boundary owns the frame loop.
///
/// Without this the headless frame loop keeps feeding fixed steps between control requests, so
/// the number of ordinary updates, and with it every random draw and AI decision, depended on
/// how long the driver took to answer. Loading, menus, a world's first two updates and an open
/// examine view run freely so readiness checks can complete: the view closes on real-time ticks,
/// which a frozen step never delivers, and it pauses ordinary updates itself. Faithful gameplay is unchanged: the gate exists
/// only when a replay step is configured.
pub(crate) fn replay_gate_open(
    schedule_pending: bool,
    modal_suspended: bool,
    in_game: bool,
    world_updates: Option<u64>,
    dialogue_pending: bool,
) -> bool {
    let started = world_updates.is_some_and(|updates| updates >= 2);
    // A speech line still exporting or loading holds a started world even inside a schedule, so
    // it becomes ready at the same update in every run. An open examine view still wins: it
    // closes on real-time ticks.
    if dialogue_pending && in_game && started && !modal_suspended {
        return false;
    }
    schedule_pending || modal_suspended || !in_game || !started
}

/// What the replay gate reads about the game, grouped because Bevy systems take at most 16
/// parameters and clippy limits a function to seven.
#[derive(bevy::ecs::system::SystemParam)]
struct ReplayGateInputs<'w> {
    state: Option<Res<'w, State<cyrus_game::AppState>>>,
    actors: Option<Res<'w, cyrus_script::script::ScriptActors>>,
    examine: Option<Res<'w, cyrus_script::item_viewer::ExamineScreen>>,
    dialogue: Option<Res<'w, cyrus_audio::sound::ReplayDialoguePending>>,
}

fn replay_gate(
    step: Res<ReplayStep>,
    scheduler: Res<input_schedule::InputScheduler>,
    inputs: ReplayGateInputs,
    mut strategy: ResMut<bevy::time::TimeUpdateStrategy>,
    mut hold: ResMut<cyrus_script::script::ReplayHold>,
) {
    let in_game = inputs
        .state
        .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame);
    let updates = inputs
        .actors
        .as_deref()
        .and_then(|actors| actors.current.as_ref())
        .map(|world| world.ordinary_updates());
    let modal = inputs.examine.is_some_and(|screen| screen.suspended());
    let open = replay_gate_open(
        scheduler.pending(),
        modal,
        in_game,
        updates,
        inputs.dialogue.is_some_and(|pending| pending.0),
    );
    *strategy = bevy::time::TimeUpdateStrategy::ManualDuration(if open {
        step.0
    } else {
        std::time::Duration::ZERO
    });
    // A zero step alone still ran zero-length ordinary updates, so hold them too.
    hold.0 = !open;
}

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
        app.add_systems(
            Last,
            frame_capture::capture.after(bevy::diagnostic::update_frame_count),
        );
        app.init_resource::<frame_capture::FrameCaptures>();
        app.add_systems(PostUpdate, light_image::respond);
        app.add_systems(PostUpdate, player_trace::respond);
        app.add_systems(
            PostUpdate,
            effect_geometry::respond.after(bevy::asset::AssetEventSystems),
        );
        app.add_systems(
            PostUpdate,
            rendered_actor::respond.after(bevy::asset::AssetEventSystems),
        );
        app.insert_resource(Inbox(Mutex::new(rx)))
            .add_message::<cyrus_audio::sound::SoundRequest>()
            .init_resource::<TapRelease>()
            .init_resource::<input_schedule::InputScheduler>()
            .add_systems(
                First,
                replay_gate
                    .before(bevy::time::TimeSystems)
                    .run_if(resource_exists::<ReplayStep>),
            )
            .add_systems(
                Update,
                input_schedule::apply
                    .after(cyrus_script::script::ModelPreparation)
                    .before(cyrus_script::player::input::InputSample)
                    .before(cyrus_script::script::OrdinaryUpdates),
            )
            .init_resource::<crate::session::NativeSessionLoad>()
            .init_resource::<crate::session::slots::SlotStore>()
            .init_resource::<cyrus_startup::LastGameLoader>()
            .add_systems(
                Update,
                crate::session::launch_slot.run_if(resource_exists::<crate::session::LaunchSlot>),
            )
            // Deliver control taps before console capture, then sample F9 after keyboard ownership is resolved.
            .add_systems(
                PreUpdate,
                (release_taps, handle)
                    .chain()
                    .after(bevy::input::InputSystems)
                    .before(cyrus_console::ConsoleInput),
            )
            .add_systems(
                PreUpdate,
                crate::session::poll.after(cyrus_console::ConsoleInput),
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
struct Audio<'w, 's> {
    requests: MessageWriter<'w, cyrus_audio::sound::SoundRequest>,
    effects: Option<Res<'w, cyrus_audio::sound::SoundEffects>>,
    speech: Option<Res<'w, cyrus_audio::sound::Speech>>,
    music: Option<Res<'w, cyrus_audio::music::Music>>,
    music_sink:
        Query<'w, 's, &'static bevy::audio::AudioSink, With<cyrus_audio::music::MusicPlayer>>,
    playing: Query<
        'w,
        's,
        Option<&'static bevy::audio::AudioSink>,
        With<cyrus_audio::sound::PlayingSound>,
    >,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Inspection<'w, 's> {
    preloaded: Option<Res<'w, Preloaded>>,
    textures: Option<ResMut<'w, cyrus_render::fog::TextureAnimations>>,
    hud: Option<Res<'w, cyrus_render::hud::HudState>>,
    script_hud: Option<Res<'w, cyrus_script::script::ScriptHud>>,
    logbook: Option<Res<'w, cyrus_script::logbook::LogbookScreen>>,
    inventory_screen: Option<Res<'w, cyrus_script::inventory_screen::InventoryScreen>>,
    examine: Option<Res<'w, cyrus_script::item_viewer::ExamineScreen>>,
    island_map: Option<Res<'w, cyrus_script::island_map::IslandMapScreen>>,
    scripts: Option<Res<'w, cyrus_script::script::ScriptStatus>>,
    script_actors: Option<ResMut<'w, cyrus_script::script::ScriptActors>>,
    random: ResMut<'w, cyrus_game::random::EngineRandom>,
    cheats: ResMut<'w, cyrus_game::cheats::Cheats>,
    console: Res<'w, cyrus_console::Console>,
    activation: Option<Res<'w, cyrus_script::script::ActivationInputs>>,
    prepared: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    ready_scene: Option<Res<'w, cyrus_render::ReadyScene>>,
    animations: Res<'w, Assets<cyrus_assets::animation::Animations>>,
    script_nodes: Query<'w, 's, (Entity, &'static Name)>,
    binding_nodes: Query<'w, 's, (Entity, &'static cyrus_script::script::ScriptBinding)>,
    script_sounds: Option<ResMut<'w, Messages<cyrus_world::sound::SoundCue>>>,
    player_poses: Query<'w, 's, &'static cyrus_script::player::placement::PlayerPose, With<Player>>,
    marker: Option<ResMut<'w, cyrus_script::player::placement::MarkerEntry>>,
    clock: Option<Res<'w, cyrus_script::clock::SimulationClock>>,
    rendered_actor_pending: Option<Res<'w, rendered_actor::PendingRenderedActor>>,
    effect_geometry_pending: Option<Res<'w, effect_geometry::PendingEffectGeometry>>,
    input_scheduler: ResMut<'w, input_schedule::InputScheduler>,
    frame_captures: ResMut<'w, frame_capture::FrameCaptures>,
    player_trace_pending: Option<Res<'w, player_trace::PendingTrace>>,
}

// Control dispatch joins input, session state and presentation resources.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle(
    mut commands: Commands,
    inbox: Res<Inbox>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut taps: ResMut<TapRelease>,
    (mut fidelity, mut sky_scroll, mut day_night, mut modern_effects): (
        ResMut<Fidelity>,
        Option<ResMut<cyrus_render::environment::SkyScrollSync>>,
        Option<ResMut<cyrus_render::fog::DayNight>>,
        Option<ResMut<cyrus_render::fog::ModernEffects>>,
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
        inventory_screen,
        examine,
        island_map,
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
        mut marker,
        clock,
        rendered_actor_pending,
        effect_geometry_pending,
        mut input_scheduler,
        mut frame_captures,
        player_trace_pending,
    }: Inspection,
    frames: Res<bevy::diagnostic::FrameCount>,
    (input, mut flycam, mut reference, mut ray_tracing): (
        Res<cyrus_script::player::input::PlayerInput>,
        Option<ResMut<cyrus_render::camera::DetachedView>>,
        Option<ResMut<cyrus_render::fog::RayTracingReference>>,
        Option<ResMut<cyrus_render::fog::RayTracing>>,
    ),
    mut camera: Query<&mut Transform, (With<cyrus_render::camera::ViewCamera>, Without<Player>)>,
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
                | Command::ScriptSummary { .. }
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
            Command::SessionSlots { operation } => {
                commands.queue(move |world: &mut World| {
                    let result = crate::session::request_slots(world, operation).map(|()| json!({"pending":true})).map_err(|error| format!("{error:#}"));
                    let _ = reply.send(match result { Ok(value) => Response::ok(id, value), Err(error) => Response::err(id, error) });
                });
                break;
            }
            Command::SessionSlotsResult { acknowledge } => {
                commands.queue(move |world: &mut World| {
                    let result = crate::session::slots_result(world, acknowledge);
                    let _ = reply.send(match result { Ok(value) => Response::ok(id, value), Err(error) => Response::err(id, format!("{error:#}")) });
                });
                break;
            }
            Command::SessionLoad { path } => install.as_ref()
                .ok_or_else(|| anyhow::anyhow!("native session install unavailable"))
                .and_then(|install| {
                    anyhow::ensure!(session.as_ref().is_some_and(|boundary| boundary.is_idle()), "session boundary pending or result unread");
                    native_load.start(path, (**install).clone())
                })
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
            Command::RenderedActor { index, after_generation } => {
                if rendered_actor_pending.is_some() {
                    Err("rendered actor observation already pending".into())
                } else {
                    commands.insert_resource(rendered_actor::PendingRenderedActor {
                        id, index, after_generation, reply,
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(180),
                    });
                    break;
                }
            }
            Command::EffectGeometry { after_generation } => {
                if effect_geometry_pending.is_some() {
                    Err("effect geometry observation already pending".into())
                } else {
                    commands.insert_resource(effect_geometry::PendingEffectGeometry {
                        id, after_generation, reply,
                        deadline: std::time::Instant::now() + std::time::Duration::from_secs(180),
                    });
                    break;
                }
            }
            Command::BoundLightImage { name, primitive } => {
                commands.insert_resource(light_image::PendingLightImage { id, name, primitive, reply });
                // Stop dispatch so later commands cannot change the selected world before capture.
                break;
            }
            Command::State => {
                let scene_ready = ready_scene.as_ref().is_some_and(|ready| {
                    prepared.as_ref().is_some_and(|world| ready.0 == world.animations)
                });
                let transform = player.single().ok().map(|t| {
                    let (yaw, pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
                    json!({"position": t.translation.to_array(), "yaw": yaw, "pitch": pitch})
                });
                let preload = preloaded.as_ref().map(|p| {
                    json!({"parsed": p.parsed, "failed": p.failed.len(), "by_type": p.by_type, "unsupported": p.unsupported, "report": cyrus_config::config_dir().join("preload-report.json").display().to_string()})
                });
                Ok(
                    json!({"random_state": random.state(), "effect_geometry_pending": effect_geometry_pending.is_some(), "rendered_actor_pending": rendered_actor_pending.is_some(), "textures": textures.as_ref().map(|textures| textures.snapshot()), "state": format!("{:?}", state.get()), "menu": menu.as_deref(), "startup": startup.as_deref(), "hud": hud.as_deref(), "logbook": logbook.as_deref(), "inventory_screen": inventory_screen.as_deref(), "examine": examine.as_deref(), "island_map": island_map.as_deref(), "script_hud": script_hud.as_ref().map(|hud| hud.inspect()), "cheats": cheats.inspect(), "console": *console, "scripts": scripts.as_deref(), "scene": current.name, "scene_ready": scene_ready, "player": transform, "fidelity": *fidelity, "frame": frames.0, "clock": clock.as_ref().map(|clock| json!({"game_seconds": clock.game_seconds(), "fps": clock.fps()})), "preload": preload, "player_input": *input, "music_track": audio.music.as_ref().and_then(|m| m.track), "music_paused": audio.music_sink.iter().next().map(bevy::audio::AudioSinkPlayback::is_paused), "sounds": json!({"playing": audio.playing.iter().count(), "paused": audio.playing.iter().flatten().filter(|sink| bevy::audio::AudioSinkPlayback::is_paused(*sink)).count()}), "flycam": {"enabled": flycam.as_ref().is_some_and(|f| f.enabled), "position": camera.single().ok().map(|t| t.translation.to_array()), "rotation": camera.single().ok().map(|t| t.rotation.to_array())}}),
                )
            }
            Command::CapturePresentationFrame { path } => {
                match frame_captures.arm(id, path.into(), None, reply.clone(), None, frame_capture::CaptureKind::Presentation) {
                    Ok(()) => continue,
                    Err(error) => Err(error.to_string()),
                }
            }
            Command::CaptureGameplayFrame { path, pcm_path, boundary } => {
                match frame_captures.arm(id, path.into(), pcm_path.map(Into::into), reply.clone(), boundary, frame_capture::CaptureKind::Gameplay) {
                    Ok(()) => break,
                    Err(error) => Err(format!("{error:#}")),
                }
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
            Command::SetAttribute {
                index,
                attribute,
                value,
            } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.set_actor_attribute(index, attribute, value)?;
                Ok(json!({"index": index, "attribute": attribute, "value": value}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::MarkDead { index } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.mark_actor_dead(index)?;
                Ok(json!({"index": index, "dead": true}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::SetTorch { burning } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.set_selected_torch(burning)?;
                Ok(json!({"burning": burning}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::ShowItem { item, no_text } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.globals_mut().request_item_viewer(item, [0, 0], no_text);
                Ok(json!({"item": item, "no_text": no_text}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::ResetCamera => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.globals_mut().request_camera_reset();
                Ok(json!({"requested": true}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::SelectItem { id } => (|| -> anyhow::Result<_> {
                let hud = script_hud
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("inventory is not prepared"))?;
                hud.select_item(id)?;
                Ok(json!({"selected": id}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::TracePlayer { x, y, z, heading, updates } => {
                commands.queue(move |world: &mut World| {
                    if let Err(error) = player_trace::start(world, id, reply.clone(), [x, y, z], heading, updates) {
                        let _ = reply.send(Response::err(id, format!("{error:#}")));
                    }
                });
                break;
            }
            Command::PlacePlayer { .. } if player_trace_pending.is_some() => Err("player trace owns placement until completion".into()),
            Command::PlacePlayer { x, y, z, heading } => (|| -> anyhow::Result<_> {
                let world = script_actors
                    .as_mut()
                    .and_then(|actors| actors.current.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
                world.place_selected_player(
                    [x, y, z].map(|value| value.wrapping_mul(256)),
                    heading,
                )?;
                // handle runs in PreUpdate: placement is applied before the next ordinary update.
                // An absent clock cannot acknowledge an exact simulation event.
                let player = world.selected_player()
                    .ok_or_else(|| anyhow::anyhow!("placement: selected player unavailable"))?;
                let boundary = clock.as_ref().zip(prepared.as_ref()).map(|(clock, prepared)| {
                    json!({
                        "ready": true, "scene": prepared.requested_name,
                        "frame": world.ordinary_updates(), "generation": world.generation(),
                        "elapsed_scale": clock.elapsed_frame_scale(),
                        "normal_frame_rate": clock.settings().normal_frame_rate,
                        "game_seconds": clock.game_seconds(), "execution": "running",
                        "event": "placement_applied", "phase": "before_ordinary_update",
                        "player": world.inspect(player.index, 1)["actors"][0],
                    })
                });
                Ok(json!({"position": [x, y, z], "heading": wrap(heading),
                    "acknowledged": boundary.is_some(), "boundary": boundary}))
            })()
            .map_err(|error| format!("{error:#}")),
            Command::Flycam {
                enabled,
                position,
                look_at,
            } => match flycam.as_deref_mut() {
                Some(flycam) => {
                    flycam.enabled = enabled;
                    if let Ok(mut view) = camera.single_mut() {
                        if let Some(position) = position {
                            view.translation = Vec3::from_array(position);
                        }
                        if let Some(target) = look_at {
                            view.look_at(Vec3::from_array(target), Vec3::Y);
                        }
                    }
                    Ok(json!({"enabled": enabled}))
                }
                None => Err("no flycam".into()),
            },
            Command::ScheduleInput { events } => {
                let generation = script_actors.as_ref().and_then(|actors| actors.current.as_ref()).map(|world| world.generation());
                match input_scheduler.arm(id, reply.clone(), generation, events, &keys) {
                    Ok(()) => continue,
                    Err(error) => Err(error),
                }
            }
            Command::AcquireInput { acquisition } => {
                let generation = script_actors.as_ref().and_then(|actors| actors.current.as_ref()).map(|world| world.generation());
                match input_scheduler.arm_clock(id, reply.clone(), generation, acquisition, &keys) {
                    Ok(()) => continue,
                    Err(error) => Err(error),
                }
            }
            Command::Input { .. } if input_scheduler.pending() => Err("input schedule owns control input until completion".into()),
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
            Command::RayTracing { enabled } => match ray_tracing.as_mut() {
                Some(ray_tracing) => {
                    ray_tracing.enabled = enabled;
                    Ok(json!({
                        "enabled": enabled,
                        "traced": cyrus_render::fog::ray_traced(&fidelity, ray_tracing),
                        "support": format!("{:?}", ray_tracing.support),
                    }))
                }
                None => Err("this build has no ray tracing (cargo feature `raytracing`)".into()),
            },
            Command::RayTracingReference { enabled } => match reference.as_mut() {
                Some(reference) => {
                    reference.0 = enabled;
                    Ok(json!({"enabled": enabled}))
                }
                None => Err("no renderer is running".into()),
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
                    (Err(error), _) => Err(format!("invalid fidelity area: {error}")),
                    (_, Err(error)) => Err(format!("invalid fidelity mode: {error}")),
                }
            }
            Command::SetDayNight { hour, running } => match (day_night.as_deref_mut(), hour) {
                (None, _) => Err("modern lighting is unavailable".into()),
                (Some(_), Some(hour)) if !(0.0..=24.0).contains(&hour) => {
                    Err("hour must be within 0 to 24".into())
                }
                (Some(cycle), hour) => {
                    cycle.enabled = hour.is_some();
                    cycle.hour = hour.unwrap_or(cycle.hour);
                    cycle.running = running;
                    Ok(json!({"enabled": cycle.enabled, "hour": cycle.hour, "running": cycle.running}))
                }
            },
            Command::SetModernEffect { effect, enabled } => {
                match (
                    modern_effects.as_deref_mut(),
                    serde_json::from_value::<cyrus_render::fog::Effect>(json!(effect)),
                ) {
                    (None, _) => Err("modern lighting is unavailable".into()),
                    (Some(_), Err(_)) => Err(
                        "effect must be shadows|ambient_occlusion|contact_shadows|anti_aliasing|bloom|object_lights|water_gloss|grading|auto_exposure|god_rays|lens_flare|sky|water_reflections|depth_of_field|motion_blur"
                            .into(),
                    ),
                    (Some(effects), Ok(effect)) => {
                        *effects.switch(effect) = enabled;
                        Ok(json!(*effects))
                    }
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
                // A world entered by name starts at its default marker, not a retained one.
                if let Some(marker) = marker.as_mut() {
                    **marker = Default::default();
                }
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
                        Ok(json!({"effect": effect, "looping": loaded.repetitions != cyrus_assets::sound::SampleRepetitions::ONCE}))
                    }
                    None => Err(format!("sound effect {effect} is not loaded")),
                }
            }
            Command::PlaySpeech { label } => {
                if audio
                    .speech
                    .as_ref()
                    .is_some_and(|s| s.get(&label).is_some())
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
                    Some(world) => {
                        let mut observation = world.inspect(offset, limit);
                        observation["flags"] = json!(world.snapshot_flags());
                        observation["player_position_q8"] = json!(world
                            .selected_player()
                            .and_then(|id| world.spatial(id).ok())
                            .and_then(|spatial| spatial.effective.as_ref())
                            .map(|pose| pose.position));
                        Ok(observation)
                    }
                    None => Err("script world is not prepared".into()),
                }
            }
            Command::QueueMovie { movie } => match script_actors
                .as_mut()
                .and_then(|actors| actors.current.as_mut())
            {
                Some(world) => {
                    world.globals_mut().queue_movie(movie);
                    Ok(json!({"queued": movie}))
                }
                None => Err("script world is not prepared".into()),
            },
            Command::ScriptLoadWorld {
                world,
                marker,
                heading,
            } => match script_actors
                .as_mut()
                .and_then(|actors| actors.current.as_mut())
            {
                Some(current) => {
                    current.globals_mut().request_load_world([world, marker, heading]);
                    Ok(json!({"requested": [world, marker, heading]}))
                }
                None => Err("script world is not prepared".into()),
            },
            Command::ScriptSummary { offset, limit } => match script_actors
                .as_ref()
                .and_then(|actors| actors.current.as_ref())
            {
                Some(world) => Ok(world.inspect_summary(offset, limit)),
                None => Err("script world is not prepared".into()),
            },
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
                let actor = cyrus_script::script::ScriptActorId { generation, index };
                world.bind_animation(actor, data, cyrus_soup::AnimationGroup::from_index(default_group))?;
                if let Some(entity) = entity {
                    script_bindings.insert(entity, actor);
                    commands
                        .entity(entity)
                        .remove::<(
                            cyrus_script::script::ScriptedPlayback,
                            cyrus_world::animation::AnimationPlayback,
                            cyrus_world::animation::AnimationPose,
                        )>()
                        .insert(cyrus_script::script::ScriptBinding {
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
                use cyrus_script::script::{ScriptActorId, ScriptStage, Thread};
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
                    use cyrus_script::script::ScriptSound;
                    let (sound, surface, position) = match sound {
                        ScriptSound::Animation { sound, surface, position } => (sound, surface, position),
                        sound => {
                            let event = match &sound {
                                ScriptSound::Flat { effect, volume, rate_offset_hz } => json!({"type":"flat", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz}),
                                ScriptSound::Positional { effect, volume, rate_offset_hz, position, start_index } => json!({"type":"positional", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz, "position":position, "start_index":start_index}),
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
                    anyhow::ensure!(
                        !matches!(
                            sound,
                            cyrus_world::animation::AnimationSound::Event { .. }
                                | cyrus_world::animation::AnimationSound::Transition { .. }
                        ),
                        "animation actor events and transitions are unsupported in the control script stage"
                    );
                    let mut nodes = script_bindings.iter().filter(|(_, bound)| **bound == actor);
                    let Some((entity, _)) = nodes.next() else {
                        use cyrus_world::animation::AnimationSound;
                        let event = match sound {
                            AnimationSound::Play { effect, rate_offset_hz } =>
                                json!({"type":"play", "effect":effect, "rate_offset_hz":rate_offset_hz}),
                            AnimationSound::CheckSample => json!({"type":"check_sample"}),
                            AnimationSound::Stop => json!({"type":"stop"}),
                            AnimationSound::Forget => json!({"type":"forget"}),
                            AnimationSound::Event { .. } | AnimationSound::Transition { .. } => {
                                unreachable!()
                            }
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
                    Ok(cyrus_script::script::PlayerQuery { position, heading: pose.heading })
                }).transpose()?;
                let inputs = cyrus_script::script::ScriptInputs {
                    hud: script_hud.as_deref(),
                    activation: activation.as_deref(),
                    death_suppressed: cheats.death_suppressed().ok(),
                    torch_retained: cheats.torch_retained().ok(),
                    actor_update_skipped: cheats.actor_update_skipped().ok(),
                    ground_bypass: cheats.ground_bypass().ok(),
                    magic_carpet: cheats.magic_carpet().ok(),
                    player_collision_skipped: cheats.player_collision_skipped().ok(),
                    nominal_ticks, frame_scale,
                    player_query: Some(cyrus_script::script::QueryInputs {
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
    #[test]
    fn replay_world_pauses_unless_a_schedule_owns_the_frame_loop() {
        use super::replay_gate_open;
        // An in-game world past its first two updates is held between requests.
        assert!(!replay_gate_open(false, false, true, Some(2), false));
        assert!(!replay_gate_open(false, false, true, Some(900), false));
        // A pending scheduled boundary, startup, loading and a fresh world keep time running.
        assert!(replay_gate_open(true, false, true, Some(900), false));
        assert!(replay_gate_open(false, false, false, Some(900), false));
        assert!(replay_gate_open(false, false, true, Some(1), false));
        assert!(replay_gate_open(false, false, true, Some(0), false));
        assert!(replay_gate_open(false, false, true, None, false));
        // An open examine view closes on real-time ticks, so a held world must keep time running.
        assert!(replay_gate_open(false, true, true, Some(900), false));
        // A speech line still exporting or loading holds a started world even inside a schedule,
        // but never loading, a fresh world, a world-less game or an open examine view.
        assert!(!replay_gate_open(true, false, true, Some(900), true));
        assert!(replay_gate_open(true, false, false, Some(900), true));
        assert!(replay_gate_open(true, false, true, Some(1), true));
        assert!(replay_gate_open(true, false, true, None, true));
        assert!(replay_gate_open(true, true, true, Some(900), true));
    }

    use super::*;
    use cyrus_assets::surface::Surface;
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
        .add_plugins(cyrus_script::console::GameConsolePlugin)
        .init_resource::<CurrentScene>()
        .init_resource::<bevy::diagnostic::FrameCount>()
        .init_resource::<cyrus_script::player::input::PlayerInput>()
        .init_resource::<cyrus_render::camera::DetachedView>()
        .insert_resource(SceneRequest(String::new()))
        .insert_resource(ControlAddr("127.0.0.1:0".into()))
        .add_plugins(ControlPlugin);
        app
    }

    #[test]
    fn public_fidelity_uses_registry_areas_and_reports_current_choices() {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        for area in Area::ALL {
            let name = serde_json::to_value(area).unwrap();
            let (reply, responses) = channel();
            tx.send(Pending {
                request: Request {
                    id: 1,
                    cmd: Command::SetFidelity {
                        area: name.as_str().unwrap().into(),
                        mode: "modern".into(),
                    },
                },
                reply,
            })
            .unwrap();
            app.update();
            let response = responses.try_recv().unwrap();
            assert!(response.error.is_none(), "{response:?}");
            assert_eq!(app.world().resource::<Fidelity>().mode(area), Mode::Modern);
        }
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 2,
                cmd: Command::SetFidelity {
                    area: "player_controller".into(),
                    mode: "modern".into(),
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        let error = responses.try_recv().unwrap().error.unwrap();
        for area in Area::ALL {
            let name = serde_json::to_value(area).unwrap();
            assert!(error.contains(name.as_str().unwrap()), "{error}");
        }
    }

    #[test]
    fn rendered_actor_generation_wait_survives_unready_replacement_scene() {
        let mut app = script_app();
        app.insert_state(cyrus_game::AppState::InGame);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 83,
                cmd: Command::RenderedActor {
                    index: 0,
                    after_generation: Some(0),
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(
            responses.try_recv().is_err(),
            "logical replacement must wait for rendered readiness"
        );
        let animations = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>()
            .animations
            .clone();
        let actor = app
            .world()
            .resource::<cyrus_script::script::ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .id(0)
            .unwrap();
        let owner = app
            .world_mut()
            .spawn((
                Name::new("ACTOR"),
                cyrus_script::script::ScriptBinding {
                    actor,
                    source: Handle::default(),
                },
            ))
            .id();
        let loading_model = app
            .world_mut()
            .spawn((WorldAssetRoot(Handle::default()), ChildOf(owner)))
            .id();
        app.insert_resource(cyrus_render::ReadyScene(animations));
        app.update();
        assert!(
            responses.try_recv().is_err(),
            "selected model instance must settle before observation"
        );
        app.world_mut().despawn(loading_model);
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 83);
        assert_eq!(
            response.error.as_deref(),
            Some("rendered actor observation: game clock absent")
        );
    }

    #[test]
    fn rendered_actor_generation_wait_survives_the_empty_world_handoff() {
        let mut app = app();
        app.insert_state(cyrus_game::AppState::InGame);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 82,
                cmd: Command::RenderedActor {
                    index: 0,
                    after_generation: Some(1),
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(responses.try_recv().is_err());
        app.world_mut()
            .resource_mut::<rendered_actor::PendingRenderedActor>()
            .deadline = std::time::Instant::now();
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 82);
        assert!(response.error.unwrap().contains("generation wait expired"));
    }

    #[test]
    fn rendered_actor_observation_replies_after_update_and_rejects_absent_owners() {
        let mut app = app();
        app.insert_state(cyrus_game::AppState::InGame);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 81,
                cmd: Command::RenderedActor {
                    index: 0,
                    after_generation: None,
                },
            },
            reply,
        })
        .unwrap();
        app.add_systems(
            Update,
            |pending: Option<Res<rendered_actor::PendingRenderedActor>>| {
                assert!(
                    pending.is_some(),
                    "request must wait until presentation updates complete"
                );
            },
        );
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 81);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("current world absent"));
        assert!(
            !app.world()
                .contains_resource::<rendered_actor::PendingRenderedActor>()
        );
    }

    #[test]
    fn bound_light_request_observes_update_and_preserves_later_command_order() {
        let mut app = app();
        app.insert_state(cyrus_game::AppState::InGame);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 42,
                cmd: Command::BoundLightImage {
                    name: "selected-mesh".into(),
                    primitive: 0,
                },
            },
            reply: reply.clone(),
        })
        .unwrap();
        tx.send(Pending {
            request: Request {
                id: 43,
                cmd: Command::Ping,
            },
            reply,
        })
        .unwrap();
        app.add_systems(Update, |mut commands: Commands| {
            commands.spawn(Name::new("selected-mesh"));
            commands.spawn(Name::new("selected-mesh"));
        });
        assert!(responses.try_recv().is_err());
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 42);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("ambiguous"));
        assert!(responses.try_recv().is_err());
        assert!(
            !app.world()
                .contains_resource::<light_image::PendingLightImage>()
        );
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 43);
        assert_eq!(response.result.unwrap(), json!("pong"));
        assert!(responses.try_recv().is_err());
    }

    #[test]
    fn public_schedule_input_rejects_unavailable_world_without_pressing() {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        let request =
            serde_json::from_value(json!({"id":91,"cmd":{"type":"schedule_input","events":[
                {"after_updates":0,"key":"ArrowUp","action":"press"},
                {"after_updates":2,"key":"ArrowUp","action":"release"}
            ]}}))
            .unwrap();
        tx.send(Pending { request, reply }).unwrap();
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 91);
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("no world"));
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::ArrowUp)
        );
    }

    #[test]
    fn public_clock_acquisition_rejects_unavailable_world_without_pressing() {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        let request = serde_json::from_value(json!({"id":92,"cmd":{
            "type":"acquire_input","key":"ArrowUp","endpoints":[1024,1334],"normal_frame_rate":12,"max_updates":8
        }})).unwrap();
        tx.send(Pending { request, reply }).unwrap();
        app.update();
        let response = responses.try_recv().unwrap();
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("no world"));
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::ArrowUp)
        );
    }

    #[test]
    fn effect_geometry_waits_for_an_admitted_update_after_scene_readiness() {
        let mut app = script_app();
        app.insert_state(cyrus_game::AppState::InGame);
        let animations = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>()
            .animations
            .clone();
        app.insert_resource(cyrus_render::ReadyScene(animations));
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 84,
                cmd: Command::EffectGeometry {
                    after_generation: Some(0),
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(
            responses.try_recv().is_err(),
            "asset readiness is not an admitted world update"
        );
        app.world_mut()
            .resource_mut::<effect_geometry::PendingEffectGeometry>()
            .deadline = std::time::Instant::now();
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 84);
        assert!(response.error.unwrap().contains("deadline expired"));
    }

    #[test]
    fn armed_effect_geometry_request_allows_load_commands_and_has_a_deadline() {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        tx.send(Pending {
            request: Request {
                id: 40,
                cmd: Command::EffectGeometry {
                    after_generation: Some(0),
                },
            },
            reply: reply.clone(),
        })
        .unwrap();
        tx.send(Pending {
            request: Request {
                id: 41,
                cmd: Command::Ping,
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(responses.try_recv().is_err());
        app.update();
        assert_eq!(responses.try_recv().unwrap().id, 41);
        assert!(responses.try_recv().is_err());
        app.world_mut()
            .resource_mut::<effect_geometry::PendingEffectGeometry>()
            .deadline = std::time::Instant::now();
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 40);
        assert!(response.error.unwrap().contains("deadline expired"));
        assert!(
            !app.world()
                .contains_resource::<effect_geometry::PendingEffectGeometry>()
        );
    }

    #[test]
    fn effect_geometry_request_is_one_shot_and_keeps_following_commands_ordered() {
        let mut app = app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        for (id, cmd) in [
            (
                42,
                Command::EffectGeometry {
                    after_generation: None,
                },
            ),
            (43, Command::Ping),
        ] {
            tx.send(Pending {
                request: Request { id, cmd },
                reply: reply.clone(),
            })
            .unwrap();
        }
        app.update();
        let response = responses.try_recv().unwrap();
        assert_eq!(response.id, 42);
        assert!(response.error.unwrap().contains("service absent"));
        assert!(responses.try_recv().is_err());
        app.update();
        assert_eq!(responses.try_recv().unwrap().id, 43);
        assert!(
            !app.world()
                .contains_resource::<effect_geometry::PendingEffectGeometry>()
        );
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
        script_app_with_player(false)
    }

    fn script_app_with_player(player: bool) -> App {
        use cyrus_assets::{animation::Animations, script::Scripts};
        use cyrus_script::script::{ScriptActors, ScriptWorld};
        let mut app = app();
        let dialogue = json!({"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"});
        let mut script_json = json!({
            "version":30,"dialogue":dialogue,"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},"clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},"actor_pool_capacity":10,"content_id":"","world":17,
            "definition":{"functions":[
                {"name":"NullFunction","param_count":0,"kind":"function"},
                {"name":"PushAnimation","param_count":2,"kind":"task"}
            ],"flags":[],"references":[],"attributes":[]},
            "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
                "namespace":"ACTOR","instance_variables":[[]],"location_offsets":[],"attributes":vec![0u8; 256],"native_hook":null,
                "program":{"version":4,"entry":0,"steps":{
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
        script_json["weapons"] = json!({});
        script_json["combat_attacks"] = json!([]);
        script_json["combat_clank_effects"] = json!([null, null]);
        script_json["combat_defenses"] = json!([]);
        let mut scripts: Scripts = serde_json::from_value(script_json).unwrap();
        if player {
            let mut actor = scripts.actors["ACTOR"].clone();
            actor.namespace = None;
            scripts.actors.insert("CYRUS".into(), actor);
        }
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

    #[test]
    fn public_script_inspection_exposes_read_only_global_flags() {
        let mut app = script_app();
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, result) = channel();
        tx.send(Pending {
            request: Request {
                id: 1,
                cmd: Command::Scripts {
                    offset: 0,
                    limit: 1,
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        let response = result.try_recv().unwrap();
        assert!(response.ok);
        let value = response.result.unwrap();
        assert_eq!(value["world"], 17);
        assert_eq!(value["flags"], json!({"version": 1, "flags": []}));
        assert!(value["player_position_q8"].is_null());
    }

    #[test]
    fn public_player_trace_records_placement_and_every_ordinary_update() {
        use cyrus_script::{
            clock::SimulationClock,
            script::{OrdinaryInputs, ScriptActors, ScriptInputs},
        };
        let mut app = script_app_with_player(true);
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            world.prepare_spatial().unwrap();
            let player = world.selected_player().unwrap();
            world
                .initialize_player_spatial(player, [0; 3], [0; 3])
                .unwrap();
        }
        let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        clock.advance(0);
        app.insert_resource(clock);
        #[derive(Resource)]
        struct TraceOrdinary(u32);
        app.insert_resource(TraceOrdinary(1));
        app.add_systems(
            Update,
            |run: Res<TraceOrdinary>,
             mut actors: ResMut<ScriptActors>,
             mut clock: ResMut<SimulationClock>,
             animations: Res<Assets<cyrus_assets::animation::Animations>>,
             prepared: Res<cyrus_world::lifecycle::PreparedWorld>,
             mut random: ResMut<cyrus_game::random::EngineRandom>| {
                for _ in 0..run.0 {
                    let world = actors.current.as_mut().unwrap();
                    clock.advance((world.ordinary_updates() as u32 + 1) * 3);
                    world
                        .update_ordinary(
                            animations.get(&prepared.animations).unwrap(),
                            [0; 2],
                            OrdinaryInputs {
                                scripts: ScriptInputs {
                                    nominal_ticks: Some(clock.step().nominal_ticks),
                                    frame_scale: Some(clock.step().frame_scale),
                                    ..default()
                                },
                                scene: None,
                            },
                            &mut random,
                            &mut |_, _| Ok(()),
                            100,
                        )
                        .unwrap();
                    world
                        .place_selected_player(
                            [
                                (10 + world.ordinary_updates() as i32) * 256,
                                20 * 256,
                                30 * 256,
                            ],
                            0,
                        )
                        .unwrap();
                }
            },
        );
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        let request = serde_json::from_value(json!({"id":93,"cmd":{
            "type":"trace_player","x":10,"y":20,"z":30,"heading":0,"updates":3
        }}))
        .unwrap();
        tx.send(Pending {
            request,
            reply: reply.clone(),
        })
        .unwrap();
        app.update();
        assert!(responses.try_recv().is_err());
        app.world_mut().resource_mut::<TraceOrdinary>().0 = 0;
        for _ in 0..2 {
            app.update();
            assert!(
                responses.try_recv().is_err(),
                "paused frames cannot complete the trace"
            );
        }
        app.world_mut().resource_mut::<TraceOrdinary>().0 = 1;
        for _ in 0..1 {
            app.update();
            assert!(
                responses.try_recv().is_err(),
                "trace must wait for all requested updates"
            );
        }
        app.update();
        let response = responses.try_recv().unwrap();
        assert!(response.ok, "{:?}", response.error);
        let result = response.result.unwrap();
        let rows = result["observations"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["after_updates"], index);
            assert_eq!(row["update"], index);
            assert_eq!(
                row["player"]["spatial"]["local"]["position"],
                json!([(10 + index) * 256, 20 * 256, 30 * 256])
            );
            assert!(row["player"]["animation"].is_null() || row["player"]["animation"].is_object());
            assert_eq!(row["player"]["control"]["falling_state"], 0);
            assert_eq!(row["normal_frame_rate"], 12);
            if index > 0 {
                assert!(row["elapsed_scale"].as_u64() > rows[index - 1]["elapsed_scale"].as_u64());
                assert_eq!(
                    row["clock_step"],
                    json!({
                        "frame_scale": 256, "nominal_ticks": 1, "fine_ticks": 8
                    })
                );
            } else {
                assert!(row["clock_step"].is_null());
            }
        }
        assert!(responses.try_recv().is_err());
        app.world_mut().resource_mut::<TraceOrdinary>().0 = 2;
        let request = serde_json::from_value(json!({"id":94,"cmd":{
            "type":"trace_player","x":10,"y":20,"z":30,"updates":3
        }}))
        .unwrap();
        tx.send(Pending { request, reply }).unwrap();
        app.update();
        let rejected = responses.try_recv().unwrap();
        assert!(!rejected.ok);
        assert!(rejected.error.unwrap().contains("ordinary update skipped"));
    }

    #[test]
    fn public_player_trace_rejects_invalid_bounds_and_absent_clock() {
        for (updates, error) in [
            (0, "updates must be 1..=255"),
            (256, "updates must be 1..=255"),
            (1, "game clock absent"),
        ] {
            let mut app = app();
            let (tx, rx) = channel();
            app.insert_resource(Inbox(Mutex::new(rx)));
            let (reply, responses) = channel();
            let request = serde_json::from_value(json!({"id":94,"cmd":{
                "type":"trace_player","x":10,"y":20,"z":30,"updates":updates
            }}))
            .unwrap();
            tx.send(Pending { request, reply }).unwrap();
            app.update();
            let response = responses.try_recv().unwrap();
            assert!(!response.ok);
            assert!(response.error.unwrap().contains(error));
        }
    }

    #[test]
    fn public_player_trace_rejects_world_retirement_and_conflicting_placement() {
        use cyrus_script::{clock::SimulationClock, script::ScriptActors};
        let mut app = script_app_with_player(true);
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            world.prepare_spatial().unwrap();
            let player = world.selected_player().unwrap();
            world
                .initialize_player_spatial(player, [0; 3], [0; 3])
                .unwrap();
        }
        let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        clock.advance(0);
        app.insert_resource(clock);
        let (tx, rx) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        let (reply, responses) = channel();
        for (id, cmd) in [
            (
                95,
                json!({"type":"trace_player","x":10,"y":20,"z":30,"updates":2}),
            ),
            (96, json!({"type":"place_player","x":99,"y":99,"z":99})),
        ] {
            tx.send(Pending {
                request: serde_json::from_value(json!({"id":id,"cmd":cmd})).unwrap(),
                reply: reply.clone(),
            })
            .unwrap();
        }
        app.update();
        assert!(responses.try_recv().is_err());
        app.update();
        let rejected = responses.try_recv().unwrap();
        assert_eq!(rejected.id, 96);
        assert!(rejected.error.unwrap().contains("owns placement"));
        let actors = app.world().resource::<ScriptActors>();
        let world = actors.current.as_ref().unwrap();
        assert_eq!(
            world
                .spatial(world.selected_player().unwrap())
                .unwrap()
                .local
                .position,
            [2560, 5120, 7680]
        );
        app.world_mut().resource_mut::<ScriptActors>().current = None;
        app.update();
        let rejected = responses.try_recv().unwrap();
        assert_eq!(rejected.id, 95);
        let error = rejected.error.unwrap();
        assert!(error.contains("world retired"));
        assert!(error.contains("observations:"));
    }

    #[test]
    fn placement_acknowledges_application_before_later_clock_advance() {
        use cyrus_script::{clock::SimulationClock, script::ScriptActors};
        let mut app = script_app_with_player(true);
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            world.prepare_spatial().unwrap();
            let player = world.selected_player().unwrap();
            world
                .initialize_player_spatial(player, [0; 3], [0; 3])
                .unwrap();
        }
        let placement = || Command::PlacePlayer {
            x: 10,
            y: 20,
            z: 30,
            heading: 40,
        };
        let unavailable = run_commands(&mut app, vec![placement()]).remove(0);
        assert_eq!(unavailable["acknowledged"], false);
        assert!(unavailable["boundary"].is_null());
        let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        clock.advance(0);
        let scale = clock.elapsed_frame_scale();
        app.insert_resource(clock)
            .add_systems(Update, |mut clock: ResMut<SimulationClock>| {
                clock.advance(18);
            });
        let result = run_commands(&mut app, vec![placement()]).remove(0);
        assert_eq!(result["acknowledged"], true);
        let boundary = &result["boundary"];
        assert_eq!(boundary["elapsed_scale"], scale);
        assert_eq!(boundary["phase"], "before_ordinary_update");
        assert_eq!(
            boundary["player"]["spatial"]["local"]["position"],
            json!([2560, 5120, 7680])
        );
        assert!(
            app.world()
                .resource::<SimulationClock>()
                .elapsed_frame_scale()
                > scale
        );
    }

    fn frame_app() -> App {
        use cyrus_script::{clock::SimulationClock, script::ScriptActors};
        let mut app = script_app_with_player(true);
        app.world_mut()
            .spawn((Player, Transform::default(), GlobalTransform::default()));
        app.insert_resource(State::new(cyrus_game::AppState::InGame));
        let animations = app
            .world()
            .resource::<cyrus_world::lifecycle::PreparedWorld>()
            .animations
            .clone();
        app.insert_resource(cyrus_render::ReadyScene(animations));
        app.world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow));
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            world.prepare_spatial().unwrap();
            world
                .initialize_player_spatial(world.selected_player().unwrap(), [0; 3], [0; 3])
                .unwrap();
        }
        let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        clock.advance(0);
        app.insert_resource(clock);
        app
    }

    fn request_frame(
        app: &mut App,
        path: &std::path::Path,
        pcm_path: Option<String>,
    ) -> std::sync::mpsc::Receiver<Response> {
        request_frame_at(app, path, pcm_path, None)
    }

    fn request_frame_at(
        app: &mut App,
        path: &std::path::Path,
        pcm_path: Option<String>,
        boundary: Option<cyrus_control::FrameBoundary>,
    ) -> std::sync::mpsc::Receiver<Response> {
        let (tx, rx) = channel();
        let (reply, results) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        tx.send(Pending {
            request: Request {
                id: 42,
                cmd: Command::CaptureGameplayFrame {
                    path: path.display().to_string(),
                    pcm_path,
                    boundary,
                },
            },
            reply,
        })
        .unwrap();
        app.update();
        results
    }

    #[test]
    fn scheduled_frame_waits_for_the_observed_clock_and_rejects_skipped_clock() {
        use bevy::render::view::screenshot::Screenshot;
        use cyrus_control::FrameBoundary;
        use cyrus_script::{clock::SimulationClock, script::ScriptActors};
        let mut app = frame_app();
        let generation = app
            .world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .generation();
        let scale = app
            .world()
            .resource::<SimulationClock>()
            .elapsed_frame_scale();
        let reply = request_frame_at(
            &mut app,
            std::path::Path::new("scheduled-frame.png"),
            None,
            Some(FrameBoundary::Clock {
                generation,
                elapsed_scale: scale + 256,
            }),
        );
        assert!(reply.try_recv().is_err());
        assert_eq!(
            app.world_mut()
                .query::<&Screenshot>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut()
            .resource_mut::<SimulationClock>()
            .advance(18);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Screenshot>()
                .iter(app.world())
                .count(),
            1
        );
        assert!(reply.try_recv().is_err());

        let mut app = frame_app();
        let reply = request_frame_at(
            &mut app,
            std::path::Path::new("skipped-frame.png"),
            None,
            Some(FrameBoundary::Clock {
                generation,
                elapsed_scale: scale - 1,
            }),
        );
        let response = reply.try_recv().unwrap();
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("clock skipped"));
        assert_eq!(
            app.world_mut()
                .query::<&Screenshot>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn presentation_capture_freezes_loading_state_without_a_ready_world() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::{
            render_resource::{Extent3d, TextureDimension, TextureFormat},
            view::screenshot::{Screenshot, ScreenshotCaptured},
        };
        let mut app = frame_app();
        app.insert_resource(State::new(cyrus_game::AppState::Loading));
        app.world_mut()
            .remove_resource::<cyrus_world::lifecycle::PreparedWorld>();
        app.world_mut()
            .remove_resource::<cyrus_render::ReadyScene>();
        app.world_mut()
            .remove_resource::<cyrus_script::script::ScriptActors>();
        app.world_mut()
            .remove_resource::<cyrus_script::clock::SimulationClock>();
        // An unrelated black UI background must not be reported as a world-entry fade.
        app.world_mut().spawn(BackgroundColor(Color::BLACK));
        let path =
            std::env::temp_dir().join(format!("cyrus-loading-frame-{}.png", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (tx, rx) = channel();
        let (reply, results) = channel();
        app.insert_resource(Inbox(Mutex::new(rx)));
        tx.send(Pending {
            request: Request {
                id: 42,
                cmd: serde_json::from_value(json!({
                    "type": "capture_presentation_frame", "path": path,
                }))
                .unwrap(),
            },
            reply,
        })
        .unwrap();
        app.update();
        assert!(results.try_recv().is_err());
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Screenshot>>()
            .single(app.world())
            .unwrap();
        app.insert_resource(State::new(cyrus_game::AppState::Menu));
        let image = Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[12, 34, 56, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        );
        app.world_mut()
            .trigger(ScreenshotCaptured { entity, image });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let response = loop {
            app.update();
            if let Ok(response) = results.try_recv() {
                break response;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "capture did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(response.ok, "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["boundary"]["state"], "Loading");
        assert_eq!(result["boundary"]["phase"], "render_extraction");
        assert_eq!(result["boundary"]["fade_opacity_256"], json!([]));
        assert!(result["boundary"]["render_frame"].is_u64());
        assert!(result["boundary"].get("player").is_none());
        assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn gameplay_frame_clock_pose_and_pcm_cursor_survive_delayed_readback() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::{
            render_resource::{Extent3d, TextureDimension, TextureFormat},
            view::screenshot::ScreenshotCaptured,
        };
        let mut app = frame_app();
        let path =
            std::env::temp_dir().join(format!("cyrus-owned-frame-{}.png", std::process::id()));
        let raw = path.with_extension("raw");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&raw, [0u8; 8]).unwrap();
        let reply = request_frame(&mut app, &path, Some(raw.display().to_string()));
        assert!(reply.try_recv().is_err(), "GPU completion is asynchronous");
        let busy = request_frame(&mut app, &path.with_extension("busy.png"), None)
            .try_recv()
            .unwrap();
        assert!(!busy.ok);
        assert!(busy.error.unwrap().contains("already pending"));
        app.world_mut()
            .resource_mut::<cyrus_script::clock::SimulationClock>()
            .advance(18);
        app.world_mut()
            .resource_mut::<cyrus_script::script::ScriptActors>()
            .current
            .as_mut()
            .unwrap()
            .place_selected_player([999; 3], 0)
            .unwrap();
        std::fs::write(&raw, [0u8; 40]).unwrap();
        app.world_mut()
            .query_filtered::<&mut Transform, With<Player>>()
            .single_mut(app.world_mut())
            .unwrap()
            .translation = Vec3::splat(9.);
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Screenshot>>()
            .single(app.world())
            .unwrap();
        let image = Image::new_fill(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[12, 34, 56, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        );
        app.world_mut()
            .trigger(ScreenshotCaptured { entity, image });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let response = loop {
            app.update();
            if let Ok(response) = reply.try_recv() {
                break response;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "capture did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(response.ok, "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["boundary"]["elapsed_scale"], 256);
        assert_eq!(result["boundary"]["pcm_delivery_frames"], 2);
        assert_eq!(
            result["boundary"]["player"]["spatial"]["local"]["position"],
            json!([0, 0, 0])
        );
        assert_eq!(result["boundary"]["phase"], "render_extraction");
        assert_eq!(
            result["boundary"]["presentation"]["translation"],
            json!([0., 0., 0.])
        );
        assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(raw).unwrap();
    }

    #[test]
    fn gameplay_frame_rejects_unavailable_world_and_missing_pcm_before_readback() {
        let path =
            std::env::temp_dir().join(format!("cyrus-rejected-frame-{}.png", std::process::id()));
        let mut unavailable = app();
        let response = request_frame(&mut unavailable, &path, None)
            .try_recv()
            .unwrap();
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("active world"));
        let mut ready = frame_app();
        let response = request_frame(
            &mut ready,
            &path,
            Some(path.with_extension("missing").display().to_string()),
        )
        .try_recv()
        .unwrap();
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("PCM"));
        assert!(!path.exists());
    }

    #[test]
    fn speech_command_admits_uppercase_installed_labels() {
        let mut app = app();
        app.init_resource::<cyrus_audio::sound::Speech>()
            .add_message::<cyrus_audio::sound::SoundRequest>();
        app.world_mut()
            .resource_mut::<cyrus_audio::sound::Speech>()
            .0
            .insert(
                "cisw".into(),
                cyrus_assets::sound::VoiceClip {
                    audio: Handle::default(),
                    bits_per_sample: 8,
                    base_volume: 127,
                    seconds: 2.5,
                    text: "description".into(),
                },
            );
        let result = run_commands(
            &mut app,
            vec![Command::PlaySpeech {
                label: "CISW".into(),
            }],
        );
        assert_eq!(result[0]["label"], "CISW");
        let requests = app
            .world_mut()
            .resource_mut::<Messages<cyrus_audio::sound::SoundRequest>>()
            .drain()
            .collect::<Vec<_>>();
        assert!(
            matches!(requests.as_slice(), [cyrus_audio::sound::SoundRequest::Speech(label)] if label == "CISW")
        );
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
        // The engine generator's state is read-only diagnostics for replay determinism checks.
        assert_eq!(state["random_state"], 1);

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
            "version":30,"dialogue":dialogue,"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},
            "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},
            "actor_pool_capacity":10,"content_id":"","world":17,
            "definition":{"functions":[
                {"name":"NullFunction","param_count":0,"kind":"function"},
                {"name":"PlayerDistance","param_count":0,"kind":"function"}
            ],"flags":[],"references":[],"attributes":[]},
            "actors":{"ACTOR":{"header_index":0,"code_origin":0,"instances":1,
                "namespace":"ACTOR","instance_variables":[[0]],"location_offsets":[],"attributes":vec![0u8; 256],"native_hook":null,
                "program":{"version":4,"entry":0,"variables":[0],"steps":{
                    "0":{"next":1,"operation":{"type":"assign","destination":{"type":"local","index":0},
                        "formula":{"first":{"type":"call","call":{"function_id":1,"kind":"function","target":{"type":"me"},"function":"PlayerDistance","arguments":[]}},"rest":[]}}},
                    "1":{"next":2,"operation":{"type":"end","target":0}}
                }}
            }},
            "instances":[{"object_id":0,"actor":"ACTOR","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[0,0,0],"angles":[0,0,0]}],"unsupported":{}
        });
        script_json["weapons"] = json!({});
        script_json["combat_attacks"] = json!([]);
        script_json["combat_clank_effects"] = json!([null, null]);
        script_json["combat_defenses"] = json!([]);
        let mut scripts: cyrus_assets::script::Scripts =
            serde_json::from_value(script_json).unwrap();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut actors = cyrus_script::script::ScriptActors::default();
        actors.current = Some(cyrus_script::script::ScriptWorld::prepare(&scripts, 1).unwrap());
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
        let response = run_commands(
            &mut app,
            vec![Command::StepScript {
                generation: 1,
                index: 0,
                stage: cyrus_control::ScriptStage::Animation,
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                distance_override: None,
                fallback_position: None,
            }],
        );
        assert_eq!(
            response[0]["captured_sounds"],
            json!([{
                "actor":{"generation":1,"index":0},
                "sound":{"type":"check_sample"}
            }])
        );
        let actors = app.world().resource::<cyrus_script::script::ScriptActors>();
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
                cyrus_script::script::ScriptBinding {
                    actor: cyrus_script::script::ScriptActorId {
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
            sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(Surface::UNKNOWN)
        } if actor == entity));
        assert_eq!(
            app.world()
                .get::<cyrus_script::script::ScriptBinding>(entity)
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
            actor, sound: AnimationSound::Play { effect: -17, rate_offset_hz: -64 }, position: None, surface: Some(Surface::UNKNOWN)
        } if actor == entity));
    }
}
