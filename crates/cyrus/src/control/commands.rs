//! Focused control handlers. The parent drain owns response order and barriers.
use super::*;

mod session;
use session::Sessions;
mod scene;
use scene::Scene;
mod rendering;
use rendering::Rendering;
mod gameplay;
use gameplay::Gameplay;
mod acquisition;
use acquisition::Acquisition;
mod inspection;
use inspection::Inspection;
mod audio;
use audio::Audio;

/// An admitted command either answers now, retains a reply and keeps draining,
/// or stops the drain so deferred world mutations publish before later requests.
enum Outcome {
    Reply(Result<serde_json::Value, String>),
    Continue,
    Barrier,
}
struct Reply {
    id: u64,
    reply: std::sync::mpsc::Sender<Response>,
}
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Controls<'w, 's> {
    session: Sessions<'w>,
    scene: Scene<'w>,
    rendering: Rendering<'w, 's>,
    gameplay: Gameplay<'w, 's>,
    acquisition: Acquisition<'w>,
    inspection: Inspection<'w>,
    audio: Audio<'w, 's>,
}

pub(super) fn handle(
    mut commands: Commands,
    inbox: Res<Inbox>,
    mut controls: Controls,
    mut exit: MessageWriter<AppExit>,
) {
    let rx = inbox.0.lock().unwrap();
    // Deferred binding inserts publish after this drain. Later script commands in
    // the same drain must already see those bindings when routing sound.
    let mut script_bindings: std::collections::BTreeMap<_, _> = controls
        .gameplay
        .binding_nodes
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
        let outcome = match request.cmd {
            Command::Ping => Outcome::Reply(Ok(json!("pong"))),
            Command::State => Outcome::Reply(controls.inspection.state(
                &controls.scene,
                &controls.rendering,
                &controls.gameplay,
                &controls.audio,
                &controls.acquisition,
            )),
            Command::Quit => {
                exit.write(AppExit::Success);
                Outcome::Reply(Ok(json!("bye")))
            }
            command @ (Command::SessionSlots { .. }
            | Command::SessionSlotsResult { .. }
            | Command::SessionLoad { .. }
            | Command::SessionLoadResult { .. }
            | Command::SessionCapture
            | Command::SessionCaptureResult) => controls.session.handle(
                command,
                Reply {
                    id,
                    reply: reply.clone(),
                },
                &mut commands,
            ),
            command @ (Command::PlaySound { .. }
            | Command::PlaySpeech { .. }
            | Command::StopSounds
            | Command::ConfigureDigitalOutput { .. }
            | Command::ServiceDigitalOutput { .. }) => controls.audio.handle(
                command,
                Reply {
                    id,
                    reply: reply.clone(),
                },
                &mut commands,
            ),
            command @ (Command::LoadScene { .. } | Command::SkipStartup) => {
                controls.scene.handle(command)
            }
            command @ (Command::Screenshot { .. }
            | Command::Teleport { .. }
            | Command::Flycam { .. }
            | Command::RayTracing { .. }
            | Command::RayTracingReference { .. }
            | Command::SetFidelity { .. }
            | Command::SetDayNight { .. }
            | Command::SetModernEffect { .. }
            | Command::SetSkyScroll { .. }
            | Command::HoldTextureFrame { .. }) => {
                controls
                    .rendering
                    .handle(command, &mut commands, &controls.scene)
            }
            command @ (Command::Cheat { .. }
            | Command::Console { .. }
            | Command::SetAttribute { .. }
            | Command::MarkDead { .. }
            | Command::SetTorch { .. }
            | Command::ShowItem { .. }
            | Command::ResetCamera
            | Command::SelectItem { .. }
            | Command::PlacePlayer { .. }
            | Command::Scripts { .. }
            | Command::QueueMovie { .. }
            | Command::ScriptLoadWorld { .. }
            | Command::ScriptSummary { .. }
            | Command::BindScriptAnimation { .. }
            | Command::StepScript { .. }) => controls.gameplay.handle(
                command,
                Reply {
                    id,
                    reply: reply.clone(),
                },
                &mut commands,
                &controls.scene,
                &controls.acquisition,
                &mut script_bindings,
            ),
            command @ (Command::RenderedActor { .. }
            | Command::EffectGeometry { .. }
            | Command::BoundLightImage { .. }
            | Command::CapturePresentationFrame { .. }
            | Command::CaptureGameplayFrame { .. }
            | Command::TracePlayer { .. }
            | Command::ScheduleInput { .. }
            | Command::ReplayFrame { .. }
            | Command::AcquireInput { .. }
            | Command::Input { .. }) => controls.acquisition.handle(
                command,
                Reply {
                    id,
                    reply: reply.clone(),
                },
                &mut commands,
                &controls.scene,
                &controls.gameplay,
            ),
        };
        let result = match outcome {
            Outcome::Reply(result) => result,
            Outcome::Continue => continue,
            Outcome::Barrier => break,
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
