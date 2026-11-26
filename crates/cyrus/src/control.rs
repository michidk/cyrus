//! TCP control server (see `cyrus-control`). A thread accepts connections and
//! forwards each request into the ECS; one system answers them in frame order.

mod commands;
use commands::handle;

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
/// examine view run freely so readiness checks can complete. Restored continuations
/// are held immediately even though their observation counter restarts at zero: the view closes on real-time ticks,
/// which a frozen step never delivers, and it pauses ordinary updates itself. Faithful gameplay is unchanged: the gate exists
/// only when a replay step is configured.
pub(crate) fn replay_gate_open(
    schedule_pending: bool,
    modal_suspended: bool,
    in_game: bool,
    world_updates: Option<u64>,
    dialogue_pending: bool,
    restored: bool,
) -> bool {
    let started = restored || world_updates.is_some_and(|updates| updates >= 2);
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
        inputs
            .actors
            .as_deref()
            .is_some_and(|actors| actors.current_is_restored()),
    );
    *strategy = bevy::time::TimeUpdateStrategy::ManualDuration(if open {
        scheduler.host_step().unwrap_or(step.0)
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

/// `KeyCode` is a plain enum; its `Debug` names are the public key names.
fn parse_key(name: &str) -> Option<KeyCode> {
    serde_json::from_value(json!(name)).ok()
}

#[cfg(test)]
mod tests;
