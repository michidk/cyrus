//! Focused control handlers. The parent drain owns response order and barriers.
use super::*;

mod session;
use session::Sessions;
mod scene;
use scene::Scene;
mod rendering;
use rendering::Rendering;
mod gameplay;
mod script_stage;
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
        let outcome = controls.dispatch(
            request.cmd,
            Reply {
                id,
                reply: reply.clone(),
            },
            &mut commands,
            &mut exit,
            &mut script_bindings,
        );
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

impl Controls<'_, '_> {
    fn dispatch(
        &mut self,
        command: Command,
        reply: Reply,
        commands: &mut Commands,
        exit: &mut MessageWriter<AppExit>,
        script_bindings: &mut std::collections::BTreeMap<
            Entity,
            cyrus_script::script::ScriptActorId,
        >,
    ) -> Outcome {
        match CommandGroup::from(&command) {
            CommandGroup::Ping => Outcome::Reply(Ok(json!("pong"))),
            CommandGroup::State => Outcome::Reply(self.inspection.state(
                &self.scene,
                &self.rendering,
                &self.gameplay,
                &self.audio,
                &self.acquisition,
            )),
            CommandGroup::Quit => {
                exit.write(AppExit::Success);
                Outcome::Reply(Ok(json!("bye")))
            }
            CommandGroup::Session => self.session.handle(command, reply, commands),
            CommandGroup::Audio => self.audio.handle(command, reply, commands),
            CommandGroup::Scene => self.scene.handle(command, commands),
            CommandGroup::Rendering => self.rendering.handle(command, commands, &self.scene),
            CommandGroup::Gameplay => self.gameplay.handle(
                command,
                reply,
                commands,
                &self.scene,
                &self.acquisition,
                script_bindings,
            ),
            CommandGroup::Acquisition => {
                self.acquisition
                    .handle(command, reply, commands, &mut self.scene, &self.gameplay)
            }
        }
    }
}

enum CommandGroup {
    Ping,
    State,
    Quit,
    Session,
    Audio,
    Scene,
    Rendering,
    Gameplay,
    Acquisition,
}
impl From<&Command> for CommandGroup {
    fn from(command: &Command) -> Self {
        match command {
            Command::Ping => Self::Ping,
            Command::State => Self::State,
            Command::Quit => Self::Quit,
            Command::SessionSlots { .. }
            | Command::SessionSlotsResult { .. }
            | Command::SessionLoad { .. }
            | Command::SessionLoadResult { .. }
            | Command::SessionCapture
            | Command::SessionCaptureResult => Self::Session,
            Command::PlaySound { .. }
            | Command::PlaySpeech { .. }
            | Command::StopSounds
            | Command::ConfigureDigitalOutput { .. }
            | Command::ServiceDigitalOutput { .. } => Self::Audio,
            Command::LoadScene { .. } | Command::SkipStartup => Self::Scene,
            Command::Screenshot { .. }
            | Command::Teleport { .. }
            | Command::Flycam { .. }
            | Command::RayTracing { .. }
            | Command::RayTracingReference { .. }
            | Command::SetFidelity { .. }
            | Command::SetDayNight { .. }
            | Command::SetModernEffect { .. }
            | Command::SetSkyScroll { .. }
            | Command::HoldTextureFrame { .. } => Self::Rendering,
            Command::Cheat { .. }
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
            | Command::StepScript { .. } => Self::Gameplay,
            Command::RenderedActor { .. }
            | Command::EffectGeometry { .. }
            | Command::BoundLightImage { .. }
            | Command::CapturePresentationFrame { .. }
            | Command::CaptureGameplayFrame { .. }
            | Command::TracePlayer { .. }
            | Command::ScheduleInput { .. }
            | Command::ReplayFrame { .. }
            | Command::SetReplayClockOrigin { .. }
            | Command::AcquireInput { .. }
            | Command::Input { .. } => Self::Acquisition,
        }
    }
}
