//! Actor-owned native task tokens share waits and animation requests.
use anyhow::{Context, Result};
use cyrus_script::program::CallKind;
use cyrus_script::{Argument, CallResult, Invocation};
use cyrus_world::animation::{ActorAnimation, AnimationSound};
use std::collections::BTreeMap;
mod persistence;
pub use persistence::{NativeTaskBindings, NativeTasksSnapshot};

/// Generic native execution state shared by successive actors and task slots.
/// Raw command bits preserve preservation's four-word load/save contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeCommandState {
    pub continuation: u32,
    /// Unsigned shared point-navigation pause in nominal ticks.
    pub pause_ticks: u32,
    pub point_value: u32,
    pub point_word: u16,
    pub point_phase: u8,
    pub point_counter: u8,
}
impl NativeCommandState {
    pub(super) fn prepare(&mut self, site: cyrus_script::NativeCallSite) {
        match site {
            cyrus_script::NativeCallSite::Hook => {}
            cyrus_script::NativeCallSite::ObjectFunctionValue => {
                *self = Self {
                    point_value: self.point_value,
                    ..Self::default()
                };
            }
            cyrus_script::NativeCallSite::Statement(_)
            | cyrus_script::NativeCallSite::FunctionValue => {
                *self = Self::default();
            }
        }
    }
}

/// One explicit invocation's animation and synchronous sound destination.
pub(super) struct AnimationCall<'a, 'sound> {
    pub animation: &'a mut ActorAnimation,
    pub ticks: u32,
    pub frame: &'a mut Option<u32>,
    pub sounds: Option<&'sound mut dyn FnMut(AnimationSound) -> Result<()>>,
}

/// Current receiver pose and shared timing inputs for spatial native tasks.
pub(super) struct SpatialCall<'a> {
    pub position: &'a mut [i32; 3],
    pub angles: &'a mut [u16; 3],
    pub frame_scale: u32,
    /// Held-turn amount at `frame_scale` from the shared accumulators; negative turns left.
    pub turn: i64,
    pub nominal_ticks: u32,
    pub matrix: cyrus_world::transform::Matrix,
    pub slave: bool,
    pub locations: &'a [[i32; 3]],
    pub effective_position: Option<[i32; 3]>,
    /// Selected player's local position, when the caller supplies the player query.
    pub player_position: Option<[i32; 3]>,
    pub actor_index: usize,
    pub map_updates: &'a mut Vec<usize>,
}

/// Explicit playback service and receiver state for one native ambient invocation.
pub(super) struct AmbientCall<'a> {
    pub driver: Option<&'a dyn cyrus_world::sound::AmbientSoundDriver>,
    pub reset: Option<bool>,
    pub position: Option<[i32; 3]>,
    pub excluded: Option<bool>,
    /// The receiver's killed flag, which ends an `AmbientRtx` task.
    pub killed: Option<bool>,
    pub generation: u64,
    pub actor_index: usize,
}

/// Shared services sampled for each native task invocation.
#[derive(Default)]
pub(super) struct NativeServices<'a> {
    pub ambient: Option<&'a AmbientCall<'a>>,
    pub frame_wait: Option<FrameWaitCall>,
    pub dialogue: Option<&'a mut super::DialogueState>,
    pub player_dialogue: Option<PlayerDialogueCall<'a>>,
    /// The selected player's animation, for `RTXpAnim`. Absent when the player is the receiver,
    /// whose own animation then serves, or when the player has no prepared animation.
    pub player_animation: Option<AnimationCall<'a, 'a>>,
    pub group_sync: Option<GroupSyncCall<'a>>,
    pub locomotion: Option<LocomotionCall<'a>>,
    pub camera: Option<CameraCall<'a>>,
}

/// Session camera, the selected player's slot request and the selected player's pose for
/// `showPlayer` and `showPlayerPan`. A failed read surfaces only when the task uses it.
pub(super) struct CameraCall<'a> {
    pub state: &'a mut super::camera::CameraState,
    pub requested_slot: Result<Option<u8>, String>,
    pub bound_pose: Result<([i32; 3], [u16; 3]), String>,
    /// The receiver as a camera target, for the `showObj*` and `lookObj` natives.
    pub receiver: Result<CameraTarget, String>,
    /// The selected player as a camera target, for the `showCyrus*` natives.
    pub selected_player: Result<CameraTarget, String>,
    /// The owner's pose update for the location natives; absent means they cannot start.
    pub update: Option<super::camera::CameraUpdateHandle<'a>>,
}

/// An actor a camera native points the camera at: its camera attributes and pose, plus the
/// camera-wide reads the same natives need. Fallible reads surface only when used.
pub(super) struct CameraTarget {
    pub actor: super::ScriptActorId,
    pub camera_move: u8,
    pub pan_type: u8,
    /// Target x/y/z, then camera x/y/z attribute bytes.
    pub offsets: [u8; 6],
    /// The actor's local pose, to which the record binds.
    pub pose: Result<([i32; 3], [u16; 3]), String>,
    /// The position the currently selected record is bound to (`showObjWorldPan`).
    pub selected_bound_position: Result<[i32; 3], String>,
    /// The holding-weapon test that guards the `showCyrus*` natives.
    pub holding_weapon: Result<bool, String>,
}

/// Receiver state sampled for locomotion tasks. Fallible reads surface only when used.
pub(super) struct LocomotionCall<'a> {
    pub state: Result<super::locomotion::BackwardState, String>,
    pub admission: LocomotionAdmission,
    pub animation_system: bool,
    pub motion: Option<MotionCall<'a>>,
    pub called: &'a mut u8,
}

/// Admission sampled before collision writes; animation and movement state is read afterwards.
#[derive(Default)]
pub(super) struct LocomotionAdmission {
    pub blocked_ahead_30: Option<bool>,
    pub forward_step_clear: Option<bool>,
    /// Lateral step check of an ordinary `MoveLeft` or `MoveRight`.
    pub lateral_step_clear: Option<bool>,
    pub walk_forward_mode: bool,
    pub forward_admitted: Option<bool>,
}

pub(super) struct MotionCall<'a> {
    pub direction: &'a mut u8,
    pub counters: &'a mut u8,
}

/// Read-only animation state, sampled independently of playback tick availability.
pub(super) struct FrameWaitCall {
    pub receiver: Result<(i32, i32, u8), String>,
    pub player: Result<Option<(i32, i32, u8)>, String>,
}

/// The session's group channels and the receiver's channel (attribute 29) for `SyncWithGroup`.
pub(super) struct GroupSyncCall<'a> {
    pub state: &'a mut super::GroupSync,
    pub actor_index: usize,
    pub channel: u8,
}

/// Inputs shared by the `RTXp` and `RTX` dialogue tasks.
pub(super) struct PlayerDialogueCall<'a> {
    pub source: Option<&'a dyn super::DialogueSource>,
    pub player: Option<super::ScriptActorId>,
    pub reset: Option<bool>,
    /// The scripted actor, owner of an `RTX` record.
    pub speaker: super::ScriptActorId,
    /// `RTX` distance to the selected player; a failed read surfaces only when `RTX` uses it.
    pub speaker_distance: Result<i32, String>,
    /// The same distance measured for the selected player, which `RTXpAnim` gates on.
    pub player_distance: Result<i32, String>,
    /// The scripted actor is the selected player, so `RTXpAnim` animates the receiver.
    pub player_is_receiver: bool,
}

/// Progress of an `RTX`/`rtxAnim` label lookup. The original looks up once, on the first invocation;
/// a host that prepares the entry asynchronously keeps the task retrying until the lookup resolves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Lookup {
    #[default]
    Idle,
    Pending,
    Done,
}

/// Private state of an `RTX` or `rtxAnim` line, which the original keeps in the task's words.
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Speech {
    /// Retained dialogue record: zero means none (record zero is the player's), -1 means the
    /// record table was full and reads as inactive.
    record: i32,
    /// The entry's sample slot, retained so a distant speaker can still cut it after the record ends.
    sample: Option<u8>,
    lookup: Lookup,
}

/// What an `RTX`-family invocation does after the shared distance and lookup steps.
enum Speak {
    Complete,
    /// The host is still preparing the entry.
    Wait,
    /// `fresh` is true on the invocation that created the line.
    Running {
        fresh: bool,
    },
}

impl Speech {
    fn active_record(&self) -> Option<usize> {
        usize::try_from(self.record).ok()
    }

    /// Shared `RTX`/`rtxAnim` steps: a speaker beyond `dialog_max_distance` completes (cutting a line it
    /// already has); otherwise the label is looked up once and one ordinary record is created for it.
    fn speak(
        &mut self,
        name: &str,
        label: &Argument,
        first: bool,
        speaker: (super::ScriptActorId, &Result<i32, String>),
        source: Option<&dyn super::DialogueSource>,
        dialogue: &mut super::DialogueState,
    ) -> Result<Speak> {
        let (speaker, distance) = speaker;
        let source = source.with_context(|| format!("{name} source service unavailable"))?;
        let distance = distance
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        if *distance > dialogue.settings().max_distance {
            if self.record != 0 {
                if let Some(sample) = self.sample {
                    source.stop_sample(sample)?;
                }
                dialogue.clear_sample_lines(self.sample);
            }
            return Ok(Speak::Complete);
        }
        let attempt = match self.lookup {
            Lookup::Idle => first,
            Lookup::Pending => true,
            Lookup::Done => false,
        };
        if !attempt {
            return Ok(Speak::Running { fresh: false });
        }
        let std::task::Poll::Ready(line) = source.speaker_line(
            label.integer()?.to_le_bytes(),
            speaker,
            dialogue.settings().max_text_width,
        )?
        else {
            self.lookup = Lookup::Pending;
            return Ok(Speak::Wait);
        };
        self.lookup = Lookup::Done;
        let Some(line) = line else {
            // A failed lookup keeps record zero, so the task then waits on the player's line.
            return Ok(Speak::Running { fresh: false });
        };
        anyhow::ensure!(
            line.sample.is_none_or(|slot| slot < 16),
            "{name} source returned invalid sample slot"
        );
        self.sample = line.sample;
        self.record = dialogue
            .begin_line(speaker, line.text, line.sample, line.lifetime)
            .map_or(-1, |index| index as i32);
        Ok(Speak::Running { fresh: true })
    }
}

/// `rtxAnim` ordinary group request: modifier zero, then the finite loop countdown set to one.
fn request_talk_group(input: &mut AnimationCall<'_, '_>, group: i32) -> Result<()> {
    let group = u16::try_from(group)
        .ok()
        .filter(|group| *group <= i16::MAX as u16)
        .context("rtxAnim/RTXpAnim group outside supported signed range")?;
    let frame = match &mut input.sounds {
        Some(sounds) => input
            .animation
            .request_with_sounds(group, 0, input.ticks, *sounds)?,
        None => input.animation.request(group, 0, input.ticks)?,
    };
    *input.frame = frame.or(*input.frame);
    if input.animation.current_group().is_some() {
        input.animation.set_loop_count(1)?;
    }
    Ok(())
}

/// Animation half of `rtxAnim`/`RTXpAnim`; returns whether the task completes. The first group is
/// requested when the line is created, then each invocation advances the cycle while the line is active
/// and the group last requested is not current.
fn talk_cycle(
    call: &mut AnimationCall<'_, '_>,
    dialogue: &super::DialogueState,
    speech: &Speech,
    index: &mut u8,
    groups: [i32; 3],
    fresh: bool,
) -> Result<bool> {
    let current = |call: &AnimationCall<'_, '_>| call.animation.current_group().map(i32::from);
    if fresh {
        *index = 0;
        request_talk_group(call, groups[0])?;
    }
    let line_active = dialogue.line_active(speech.active_record());
    if current(call) != Some(groups[usize::from(*index)]) && line_active {
        *index = (*index + 1) % 3;
        request_talk_group(call, groups[usize::from(*index)])?;
    }
    // Ends only when the line is over and the last requested group is no longer current.
    Ok(!(dialogue.line_active(speech.active_record())
        || current(call) == Some(groups[usize::from(*index)])))
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
enum Task {
    Dialogue,
    AnimationFrame {
        player: bool,
    },
    PlayerDialogue {
        started: bool,
    },
    /// `SyncWithGroup`.
    GroupSync,
    /// `RTX`.
    SpeakerDialogue(Speech),
    /// `rtxAnim` and `RTXpAnim`: `RTX` plus a cycle through three animation groups while the line
    /// is active; `index` selects the group last requested. `player` makes the selected player the
    /// speaker and the animated actor.
    SpeakerAnimation {
        speech: Speech,
        index: u8,
        player: bool,
    },
    Ambient {
        channel: Option<u8>,
    },
    /// `AmbientRtx`: the RTX clip's sample slot, once its lookup has started it.
    AmbientRtx {
        slot: Option<u8>,
        lookup: Lookup,
    },
    ShowPlayer {
        counter: u32,
    },
    ShowPlayerPan {
        counter: u32,
        pan: Option<super::camera::PanState>,
    },
    /// `showObjPan`, `showObjPanLoc`, `showObjWorldPan` and the `showCyrus` pans.
    ObjectPan {
        command: super::camera::ObjectPanCommand,
        counter: u32,
        pan: Option<super::camera::ObjectPanState>,
    },
    /// `showObj`, `showObjLoc`, `showObjWorld`, `showCyrus` and `showCyrusLoc`.
    ShowLocation {
        command: super::camera::LocationCommand,
        counter: u32,
    },
    /// `lookObj`.
    Look {
        counter: u32,
        look: Option<super::camera::LookState>,
    },
    /// `showObjRot`, which the original leaves empty.
    ObjectRotation,
    Movement {
        target: i32,
    },
    ObjectMovement {
        target: i32,
        traveled: i32,
    },
    ScaledMovement {
        origin: i32,
    },
    Rotation {
        target: i32,
        accumulator: i32,
    },
    AbsoluteRotation {
        movement: i32,
        accumulator: i32,
    },
    ScaledRotation {
        origin: i32,
    },
    ScaledRelativeRotation {
        origin: i32,
    },
    FacePlayer,
    Backward,
    Strafe(super::locomotion::Strafe),
    Forward,
    WalkForward,
    Location,
    Duration,
    Tasks,
    Blocking,
    Animation {
        group: u16,
        started: bool,
    },
    /// `PushControlAnimation`: `PushAnimation` that also applies the held ground turn.
    ControlAnimation {
        group: u16,
        started: bool,
    },
    /// `WanderToLocation`: walks to a location at the speed fixed on the first call.
    WanderLocation {
        started: bool,
        speed: i32,
    },
    /// `MoveAlongAxis`: the delta, spread over the duration and rotated by the receiver's
    /// orientation; `target` is the delta retained at elapsed zero, `accumulated` the sum so far.
    AxisMovement {
        target: i32,
        accumulated: i32,
    },
}
impl Task {
    fn complete(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        mut services: NativeServices<'_>,
    ) -> Result<bool> {
        let ambient = services.ambient;
        let turning = matches!(self, Self::ControlAnimation { .. });
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
            Self::GroupSync => {
                let [group] = arguments else {
                    anyhow::bail!("SyncWithGroup requires one argument");
                };
                let input = services
                    .group_sync
                    .context("SyncWithGroup channel state unavailable")?;
                // Without a channel the actor never waits.
                input.channel == 0
                    || input
                        .state
                        .post(input.actor_index, input.channel, group.integer()?)
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
                match speech.speak("RTX", label, first, speaker, input.source, dialogue)? {
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
                let name = if *player { "RTXpAnim" } else { "rtxAnim" };
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
                if *player && !input.player_is_receiver {
                    let call = services.player_animation.as_mut().with_context(missing)?;
                    talk_cycle(call, dialogue, speech, index, groups, fresh)?
                } else {
                    let call = animation.as_mut().with_context(missing)?;
                    talk_cycle(call, dialogue, speech, index, groups, fresh)?
                }
            }
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
            Self::ShowPlayer { counter } => {
                anyhow::ensure!(arguments.is_empty(), "showPlayer requires no arguments");
                let elapsed = invocation
                    .elapsed
                    .context("showPlayer requires elapsed input")?;
                let camera = services
                    .camera
                    .context("showPlayer camera session unavailable")?;
                let requested = camera.requested_slot.map_err(anyhow::Error::msg)?;
                !camera.state.show_player(requested, counter, elapsed)?
            }
            Self::ShowPlayerPan { counter, pan } => {
                let [duration] = arguments else {
                    anyhow::bail!("showPlayerPan requires one argument");
                };
                let elapsed = invocation
                    .elapsed
                    .context("showPlayerPan requires elapsed input")?;
                let camera = services
                    .camera
                    .context("showPlayerPan camera session unavailable")?;
                let requested_slot = camera.requested_slot.map_err(anyhow::Error::msg)?;
                let (bound_position, bound_angles) =
                    camera.bound_pose.map_err(anyhow::Error::msg)?;
                !camera.state.show_player_pan(
                    &super::camera::PanInputs {
                        duration: duration.integer()?,
                        requested_slot,
                        bound_position,
                        bound_angles,
                    },
                    pan,
                    counter,
                    elapsed,
                )?
            }
            Self::ObjectPan {
                command,
                counter,
                pan,
            } => {
                let name = command.name();
                let [x, y, z, duration] = arguments else {
                    anyhow::bail!("{name} requires four arguments");
                };
                let elapsed = invocation
                    .elapsed
                    .context(format!("{name} requires elapsed input"))?;
                let camera = services
                    .camera
                    .context(format!("{name} camera session unavailable"))?;
                let target = if command.targets_selected_player() {
                    camera.selected_player
                } else {
                    camera.receiver
                }
                .map_err(anyhow::Error::msg)?;
                if command.targets_selected_player()
                    && target.holding_weapon.map_err(anyhow::Error::msg)?
                {
                    return Ok(true);
                }
                let (bound_position, bound_angles) = target.pose.map_err(anyhow::Error::msg)?;
                let inputs = super::camera::ObjectPanInputs {
                    location: super::camera::LocationInputs {
                        frame: command.frame(),
                        actor: target.actor,
                        camera_move: target.camera_move,
                        attributes: target.offsets,
                        arguments: [x.integer()?, y.integer()?, z.integer()?],
                        bound_position,
                        bound_angles,
                    },
                    pan_type: target.pan_type,
                    duration: duration.integer()?,
                };
                !if command.world() {
                    camera.state.show_object_world_pan(
                        &inputs,
                        target.selected_bound_position.map_err(anyhow::Error::msg)?,
                        pan,
                        counter,
                        elapsed,
                    )?
                } else {
                    camera
                        .state
                        .show_object_pan(&inputs, pan, counter, elapsed)?
                }
            }
            Self::ShowLocation { command, counter } => {
                let name = command.name();
                let [x, y, z] = arguments else {
                    anyhow::bail!("{name} requires three arguments");
                };
                let elapsed = invocation
                    .elapsed
                    .context(format!("{name} requires elapsed input"))?;
                let camera = services
                    .camera
                    .context(format!("{name} camera session unavailable"))?;
                let target = if command.targets_selected_player() {
                    camera.selected_player
                } else {
                    camera.receiver
                }
                .map_err(anyhow::Error::msg)?;
                if command.targets_selected_player()
                    && target.holding_weapon.map_err(anyhow::Error::msg)?
                {
                    return Ok(true);
                }
                let (bound_position, bound_angles) = target.pose.map_err(anyhow::Error::msg)?;
                let arguments = [x.integer()?, y.integer()?, z.integer()?];
                // Only the first invocation runs the pose update; later ones never reach it.
                let update = camera.update;
                let mut run = |state: &mut super::camera::CameraState| match update {
                    Some(service) => service.update(state, bound_position, bound_angles),
                    None => anyhow::bail!("{name} camera update service unavailable"),
                };
                !if command.world() {
                    camera.state.show_world_location(
                        &super::camera::WorldLocationInputs {
                            actor: target.actor,
                            camera_move: target.camera_move,
                            attributes: target.offsets,
                            position: arguments,
                            bound_position,
                            actor_position: bound_position,
                            bound_angles,
                        },
                        counter,
                        elapsed,
                        &mut run,
                    )?
                } else {
                    camera.state.show_location(
                        &super::camera::LocationInputs {
                            frame: command.frame(),
                            actor: target.actor,
                            camera_move: target.camera_move,
                            attributes: target.offsets,
                            arguments,
                            bound_position,
                            bound_angles,
                        },
                        counter,
                        elapsed,
                        &mut run,
                    )?
                }
            }
            Self::Look { counter, look } => {
                let [duration] = arguments else {
                    anyhow::bail!("lookObj requires one argument");
                };
                let elapsed = invocation
                    .elapsed
                    .context("lookObj requires elapsed input")?;
                let camera = services
                    .camera
                    .context("lookObj camera session unavailable")?;
                let target = camera.receiver.map_err(anyhow::Error::msg)?;
                let (bound_position, bound_angles) = target.pose.map_err(anyhow::Error::msg)?;
                !camera.state.look_at(
                    &super::camera::LookInputs {
                        actor: target.actor,
                        camera_move: target.camera_move,
                        attributes: target.offsets,
                        bound_position,
                        bound_angles,
                        duration: duration.integer()?,
                    },
                    look,
                    counter,
                    elapsed,
                )?
            }
            Self::ObjectRotation => {
                anyhow::ensure!(arguments.len() == 4, "showObjRot requires four arguments");
                true
            }
            Self::Dialogue => {
                anyhow::ensure!(arguments.is_empty(), "WaitOnDialog requires no arguments");
                !services
                    .dialogue
                    .context("WaitOnDialog records unavailable")?
                    .waiting()
            }
            Self::Ambient { channel } => {
                let [effect, volume, rate] = arguments else {
                    anyhow::bail!("AmbientSound requires three arguments")
                };
                let elapsed = invocation
                    .elapsed
                    .context("AmbientSound requires elapsed input")?;
                let input = ambient.context("AmbientSound playback service unavailable")?;
                let reset = input
                    .reset
                    .context("AmbientSound sample-reset input unavailable")?;
                let driver = input
                    .driver
                    .context("AmbientSound playback service unavailable")?;
                let position = input
                    .position
                    .context("AmbientSound receiver pose unavailable")?;
                let excluded = input
                    .excluded
                    .context("AmbientSound receiver exclusion state unavailable")?;
                if elapsed == 0 || reset {
                    *channel = None;
                }
                let updated = driver
                    .update(cyrus_world::sound::AmbientSoundRequest {
                        generation: input.generation,
                        actor_index: input.actor_index,
                        channel: *channel,
                        effect: effect.integer()?,
                        volume: volume.integer()?,
                        rate_offset_hz: rate.integer()?,
                        position,
                        object_update_excluded: excluded,
                    })
                    .map_err(anyhow::Error::msg)?;
                anyhow::ensure!(
                    updated.channel.is_none_or(|slot| slot < 16),
                    "AmbientSound playback returned an invalid sample slot"
                );
                *channel = updated.channel;
                !updated.active
            }
            Self::AmbientRtx { slot, lookup } => {
                let [label, volume, _] = arguments else {
                    anyhow::bail!("AmbientRtx requires three arguments")
                };
                let elapsed = invocation
                    .elapsed
                    .context("AmbientRtx requires elapsed input")?;
                let input = ambient.context("AmbientRtx playback inputs unavailable")?;
                let dialogue_input = services
                    .player_dialogue
                    .context("AmbientRtx service inputs unavailable")?;
                let source = dialogue_input
                    .source
                    .context("AmbientRtx source service unavailable")?;
                let attempt = match *lookup {
                    Lookup::Idle => elapsed == 0,
                    Lookup::Pending => true,
                    Lookup::Done => false,
                };
                if attempt {
                    let width = services
                        .dialogue
                        .context("AmbientRtx dialogue settings unavailable")?
                        .settings()
                        .max_text_width;
                    let std::task::Poll::Ready(line) = source.speaker_line(
                        label.integer()?.to_le_bytes(),
                        dialogue_input.speaker,
                        width,
                    )?
                    else {
                        *lookup = Lookup::Pending;
                        return Ok(false);
                    };
                    *lookup = Lookup::Done;
                    // No dialogue line is created: only the clip's sample matters.
                    *slot = line.and_then(|line| line.sample);
                    anyhow::ensure!(
                        slot.is_none_or(|slot| slot < 16),
                        "AmbientRtx source returned invalid sample slot"
                    );
                }
                let Some(slot) = *slot else {
                    // A missing or text-only entry has no clip, so there is nothing to keep playing.
                    return Ok(true);
                };
                let position = input
                    .position
                    .context("AmbientRtx receiver pose unavailable")?;
                let killed = input
                    .killed
                    .context("AmbientRtx receiver killed state unavailable")?;
                !source.update_ambient_rtx(slot, position, volume.integer()?, killed)?
            }
            Self::FacePlayer => {
                let [duration] = arguments else {
                    anyhow::bail!("FacePlayer requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .context("FacePlayer requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("FacePlayer requires receiver pose")?;
                let player = input
                    .player_position
                    .context("FacePlayer selected-player position unavailable")?;
                let bearing = i32::from(super::spatial::query_bearing(
                    player[0].wrapping_sub(input.position[0]),
                    player[2].wrapping_sub(input.position[2]),
                ));
                let heading = i32::from(input.angles[1]);
                let mut delta = heading.wrapping_neg().wrapping_sub(bearing) & 0x7ff;
                if delta == 0 {
                    return Ok(true);
                }
                let remaining = elapsed.wrapping_sub(duration).wrapping_abs().max(1);
                let backwards = delta > 0x400;
                if backwards {
                    delta = (-delta - 0x400) & 0x3ff;
                }
                let step = ((delta << 8) / remaining) >> 8;
                if step == 0 {
                    return Ok(true);
                }
                let heading = if backwards {
                    heading - step
                } else {
                    heading + step
                };
                input.angles[1] = (heading & 0x7ff) as u16;
                elapsed >= duration.wrapping_sub(1)
            }
            Self::Backward => {
                use super::locomotion::{
                    BACKWARD_MOTION_DIRECTION, BackwardAdmission, backward_admission,
                    backward_translation, continues,
                };
                let [duration] = arguments else {
                    anyhow::bail!("MoveBackward requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .context("MoveBackward requires elapsed input")?;
                let motion = services
                    .locomotion
                    .context("MoveBackward locomotion state unavailable")?;
                let state = motion.state.map_err(anyhow::Error::msg)?;
                let mut probe_needed = false;
                let admission = backward_admission(state, || {
                    probe_needed = true;
                    motion.admission.blocked_ahead_30.unwrap_or(false)
                });
                anyhow::ensure!(
                    !(probe_needed && motion.admission.blocked_ahead_30.is_none()),
                    "MoveBackward blocked-ahead probe input unavailable"
                );
                if admission == BackwardAdmission::Refused {
                    return Ok(true);
                }
                let receiver = motion
                    .motion
                    .context("MoveBackward motion state unavailable")?;
                *receiver.direction = BACKWARD_MOTION_DIRECTION;
                let BackwardAdmission::Proceed { group, step } = admission else {
                    return Ok(true);
                };
                let input = animation.as_mut().context(
                    "MoveBackward requires prepared animation and explicit nominal ticks",
                )?;
                let spatial = spatial
                    .as_mut()
                    .context("MoveBackward requires receiver pose and frame_scale")?;
                if input.animation.has_group(group) {
                    let frame = match &mut input.sounds {
                        Some(sounds) => {
                            input
                                .animation
                                .request_with_sounds(group, 0, input.ticks, *sounds)?
                        }
                        None => input.animation.request(group, 0, input.ticks)?,
                    };
                    *input.frame = frame.or(*input.frame);
                    if motion.animation_system && input.animation.current_group() != Some(group) {
                        return Ok(!continues(duration, elapsed));
                    }
                    *motion.called |= 8;
                    let distance = super::locomotion::initial_step_distance(
                        receiver.counters,
                        6,
                        state.step_adjustment,
                        i32::from(step) << 8,
                    );
                    if let Some((heading, distance)) = backward_translation(
                        spatial.angles[1],
                        distance,
                        spatial.frame_scale as i32,
                        input.animation.translation_suppressed(),
                    ) {
                        *spatial.position = cyrus_world::transform::translate_forward(
                            *spatial.position,
                            heading,
                            distance,
                        )?;
                    }
                }
                !continues(duration, elapsed)
            }
            Self::Forward | Self::WalkForward => {
                let walking = matches!(self, Self::WalkForward);
                let name = if walking {
                    "WalkForward"
                } else {
                    "MoveForward"
                };
                use super::locomotion::{
                    FORWARD_MOTION_DIRECTION, ForwardPlan, continues, forward_admitted,
                    forward_plan, forward_translation,
                };
                let [duration] = arguments else {
                    anyhow::bail!("{name} requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .with_context(|| format!("{name} requires elapsed input"))?;
                let motion = services
                    .locomotion
                    .with_context(|| format!("{name} locomotion state unavailable"))?;
                let state = motion.state.map_err(anyhow::Error::msg)?;
                if !motion
                    .admission
                    .forward_admitted
                    .unwrap_or_else(|| forward_admitted(&state))
                {
                    return Ok(true);
                }
                let receiver = motion
                    .motion
                    .with_context(|| format!("{name} motion state unavailable"))?;
                let walking = walking && motion.admission.walk_forward_mode;
                if (walking || (*receiver.counters >> 4) & 3 < 3)
                    && !motion
                        .admission
                        .forward_step_clear
                        .with_context(|| format!("{name} forward step check input unavailable"))?
                {
                    return Ok(true);
                }
                *receiver.direction = if walking { 6 } else { FORWARD_MOTION_DIRECTION };
                let (group, step) = if walking {
                    if state.holding_weapon || state.combat_mode {
                        (31, state.walk_combat_step_attribute)
                    } else {
                        (
                            if state.torch_burning { 21 } else { 20 },
                            state.walk_step_attribute,
                        )
                    }
                } else {
                    match forward_plan(&state) {
                        ForwardPlan::Done => return Ok(true),
                        ForwardPlan::NeedsEdgeProbe => anyhow::bail!(
                            "MoveForward in deep water unsupported: edge probe input unavailable"
                        ),
                        ForwardPlan::Proceed { group, step } => (group, step),
                    }
                };
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                let spatial = spatial
                    .as_mut()
                    .with_context(|| format!("{name} requires receiver pose and frame_scale"))?;
                if input.animation.has_group(group) {
                    let frame = match &mut input.sounds {
                        Some(sounds) => {
                            input
                                .animation
                                .request_with_sounds(group, 0, input.ticks, *sounds)?
                        }
                        None => input.animation.request(group, 0, input.ticks)?,
                    };
                    *input.frame = frame.or(*input.frame);
                    if motion.animation_system && input.animation.current_group() != Some(group) {
                        return Ok(!continues(duration, elapsed));
                    }
                    *motion.called |= 4;
                    let distance = super::locomotion::initial_step_distance(
                        receiver.counters,
                        4,
                        state.step_adjustment,
                        if walking && step == 0 {
                            i32::from(state.forward_step_attribute / 2)
                        } else {
                            i32::from(step) << 8
                        },
                    );
                    if let Some((heading, distance)) = forward_translation(
                        spatial.angles[1],
                        distance,
                        spatial.frame_scale as i32,
                        input.animation.translation_suppressed(),
                    ) {
                        *spatial.position = cyrus_world::transform::translate_forward(
                            *spatial.position,
                            heading,
                            distance,
                        )?;
                    }
                }
                !continues(duration, elapsed)
            }
            Self::Strafe(strafe) => {
                use super::locomotion::{
                    StrafeAdmission, continues, strafe_admission, strafe_motion,
                };
                let name = match strafe {
                    super::locomotion::Strafe::Left => "MoveLeft",
                    super::locomotion::Strafe::Right => "MoveRight",
                };
                let [duration] = arguments else {
                    anyhow::bail!("{name} requires one argument")
                };
                let duration = duration.integer()?;
                let elapsed = invocation
                    .elapsed
                    .with_context(|| format!("{name} requires elapsed input"))?;
                let motion = services
                    .locomotion
                    .with_context(|| format!("{name} locomotion state unavailable"))?;
                let state = motion.state.map_err(anyhow::Error::msg)?;
                let admission = strafe_admission(state, *strafe);
                if admission == StrafeAdmission::Refused {
                    return Ok(true);
                }
                let receiver = motion
                    .motion
                    .with_context(|| format!("{name} motion state unavailable"))?;
                *receiver.direction = strafe.motion_direction();
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                let spatial = spatial
                    .as_mut()
                    .with_context(|| format!("{name} requires receiver pose and frame_scale"))?;
                let (group, step, turn) = match admission {
                    StrafeAdmission::Combat { group, step, turn } => (group, step, turn),
                    StrafeAdmission::OutsideCombat { group } => {
                        if !motion.admission.lateral_step_clear.with_context(|| {
                            format!("{name} lateral step check input unavailable")
                        })? {
                            return Ok(true);
                        }
                        if input.animation.has_group(group) {
                            let frame = match &mut input.sounds {
                                Some(sounds) => input.animation.request_with_sounds(
                                    group,
                                    0,
                                    input.ticks,
                                    *sounds,
                                )?,
                                None => input.animation.request(group, 0, input.ticks)?,
                            };
                            *input.frame = frame.or(*input.frame);
                        }
                        if motion.animation_system
                            && !input
                                .animation
                                .current_group()
                                .is_some_and(|current| strafe.ordinary_groups().contains(&current))
                        {
                            return Ok(!continues(duration, elapsed));
                        }
                        *motion.called |= strafe.called_flag();
                        let distance = super::locomotion::initial_step_distance(
                            receiver.counters,
                            strafe.counter_shift(),
                            state.step_adjustment,
                            i32::from(state.lateral_step_attribute) << 8,
                        );
                        if let Some((heading, distance)) = super::locomotion::strafe_translation(
                            *strafe,
                            spatial.angles[1],
                            distance,
                            spatial.frame_scale as i32,
                            input.animation.translation_suppressed(),
                        ) {
                            *spatial.position = cyrus_world::transform::translate_forward(
                                *spatial.position,
                                heading,
                                distance,
                            )?;
                        }
                        return Ok(!continues(duration, elapsed));
                    }
                    StrafeAdmission::Unsupported => anyhow::bail!(
                        "{name} in deep water or hanging at an edge unsupported: edge probe and hanging-edge test"
                    ),
                    StrafeAdmission::Refused => unreachable!("refusal returned above"),
                };
                let already_strafing = input.animation.current_group().is_some_and(|current| {
                    super::locomotion::Strafe::COMBAT_GROUPS.contains(&current)
                });
                if already_strafing || !input.animation.has_group(group) {
                    return Ok(true);
                }
                let frame = match &mut input.sounds {
                    Some(sounds) => {
                        input
                            .animation
                            .request_with_sounds(group, 0, input.ticks, *sounds)?
                    }
                    None => input.animation.request(group, 0, input.ticks)?,
                };
                *input.frame = frame.or(*input.frame);
                *motion.called |= strafe.called_flag();
                let distance = super::locomotion::initial_step_distance(
                    receiver.counters,
                    strafe.counter_shift(),
                    state.step_adjustment,
                    i32::from(step) << 8,
                );
                let (angle, movement) = strafe_motion(
                    *strafe,
                    spatial.angles[1],
                    turn,
                    distance,
                    spatial.frame_scale as i32,
                    input.animation.translation_suppressed(),
                );
                spatial.angles[1] = angle;
                if let Some((heading, distance)) = movement {
                    *spatial.position = cyrus_world::transform::translate_forward(
                        *spatial.position,
                        heading,
                        distance,
                    )?;
                }
                !continues(duration, elapsed)
            }
            Self::WanderLocation { started, speed } => {
                let [location, duration] = arguments else {
                    anyhow::bail!("WanderToLocation requires two arguments")
                };
                let (location, duration) = (location.integer()?, duration.integer()?);
                let input = spatial
                    .as_mut()
                    .context("WanderToLocation requires receiver pose and frame_scale")?;
                let count = i32::try_from(input.locations.len())
                    .context("WanderToLocation location count exceeds signed range")?;
                let target = if location < 0 || location > count {
                    input
                        .locations
                        .first()
                        .context("WanderToLocation home unavailable")?
                } else {
                    input.locations.get(location as usize).context("WanderToLocation selector equals location count: original out-of-range access unsupported")?
                };
                let source = if input.slave {
                    input
                        .effective_position
                        .context("WanderToLocation attached receiver pose unavailable")?
                } else {
                    *input.position
                };
                let differences =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(source[axis]));
                // Distance subtracts in the opposite order; wrapping makes this significant at MIN.
                let distance = super::spatial::rounded_distance(std::array::from_fn(|axis| {
                    source[axis].wrapping_sub(target[axis])
                }))?;
                if !*started {
                    *speed = distance
                        .checked_div(duration)
                        .context("WanderToLocation division invalid")?;
                    *started = true;
                }
                let direction = super::spatial::normalized_direction(differences);
                let step = speed.wrapping_mul(input.frame_scale as i32) >> 8;
                if !input.map_updates.contains(&input.actor_index) {
                    input.map_updates.push(input.actor_index);
                }
                if distance > step {
                    for (axis, component) in direction.into_iter().enumerate() {
                        input.position[axis] = input.position[axis]
                            .wrapping_add(((component >> 8).wrapping_mul(step)) >> 8);
                    }
                    false
                } else {
                    *input.position = *target;
                    true
                }
            }
            Self::AxisMovement {
                target,
                accumulated,
            } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("MoveAlongAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let elapsed = invocation
                    .elapsed
                    .context("MoveAlongAxis requires explicit elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveAlongAxis requires spatial state and frame_scale")?;
                if elapsed == 0 {
                    *target = delta;
                    *accumulated = 0;
                }
                let step = (delta
                    .checked_div(duration)
                    .context("MoveAlongAxis division overflow")?
                    >> 8)
                    .wrapping_mul(input.frame_scale as i32);
                *accumulated = accumulated.wrapping_add(step);
                if (step < 0 && *accumulated <= *target) || (step > 0 && *accumulated >= *target) {
                    return Ok(true);
                }
                let mut offset = [0; 3];
                offset[match axis {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }] = step;
                let rotated = input.matrix.transform(offset)?;
                for (coordinate, amount) in input.position.iter_mut().zip(rotated) {
                    *coordinate = coordinate.wrapping_add(amount);
                }
                if !input.map_updates.contains(&input.actor_index) {
                    input.map_updates.push(input.actor_index);
                }
                !(duration.wrapping_sub(1) > elapsed)
            }
            Self::Location => {
                let [location, duration] = arguments else {
                    anyhow::bail!("MoveToLocation requires two arguments")
                };
                let (location, duration) = (location.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("MoveToLocation requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveToLocation requires receiver pose and frame_scale")?;
                let count = i32::try_from(input.locations.len())
                    .context("MoveToLocation location count exceeds signed range")?;
                let target = if location < 0 || location > count {
                    input
                        .locations
                        .first()
                        .context("MoveToLocation home unavailable")?
                } else {
                    input.locations.get(location as usize).context("MoveToLocation selector equals location count: original out-of-range access unsupported")?
                };
                if elapsed > duration {
                    *input.position = *target;
                    if !input.map_updates.contains(&input.actor_index) {
                        input.map_updates.push(input.actor_index);
                    }
                    return Ok(true);
                }
                let source = if input.slave {
                    input
                        .effective_position
                        .context("MoveToLocation attached receiver pose unavailable")?
                } else {
                    *input.position
                };
                let differences =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(source[axis]));
                let direction = super::spatial::normalized_direction(differences);
                // Distance subtracts in the opposite order; wrapping makes this significant at MIN.
                let distance = super::spatial::rounded_distance(std::array::from_fn(|axis| {
                    source[axis].wrapping_sub(target[axis])
                }))?;
                let divisor = duration.wrapping_add(1).wrapping_sub(elapsed);
                let movement = distance
                    .checked_div(divisor)
                    .context("MoveToLocation division invalid")?
                    .wrapping_mul(input.frame_scale as i32)
                    >> 8;
                for (axis, component) in direction.into_iter().enumerate() {
                    input.position[axis] = input.position[axis]
                        .wrapping_add(((component >> 8).wrapping_mul(movement)) >> 8);
                }
                if !input.map_updates.contains(&input.actor_index) {
                    input.map_updates.push(input.actor_index);
                }
                false
            }
            Self::ObjectMovement { target, traveled } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("MoveObjectAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let elapsed = invocation
                    .elapsed
                    .context("MoveObjectAxis requires explicit elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("MoveObjectAxis requires receiver pose and frame_scale")?;
                if elapsed == 0 {
                    *target = delta;
                    *traveled = 0;
                }
                let step = (delta
                    .checked_div(duration)
                    .context("MoveObjectAxis division overflow")?
                    >> 8)
                    .wrapping_mul(input.frame_scale as i32);
                *traveled = traveled.wrapping_add(step);
                if (step < 0 && *traveled <= *target) || (step > 0 && *traveled >= *target) {
                    return Ok(true);
                }
                if axis == 2 && !input.slave && input.angles[0] == 0 && input.angles[2] == 0 {
                    *input.position = cyrus_world::transform::translate_forward(
                        *input.position,
                        input.angles[1],
                        step,
                    )?;
                } else {
                    let mut displacement = [0; 3];
                    if (0..=2).contains(&axis) {
                        displacement[axis as usize] = step;
                    }
                    let transformed = input.matrix.transform(displacement)?;
                    *input.position =
                        std::array::from_fn(|i| input.position[i].wrapping_add(transformed[i]));
                }
                if !input.map_updates.contains(&input.actor_index) {
                    input.map_updates.push(input.actor_index);
                }
                elapsed >= duration.wrapping_sub(1)
            }
            Self::Movement { target } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("MoveByAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let input = spatial
                    .as_mut()
                    .context("MoveByAxis requires spatial state and frame_scale")?;
                let elapsed = invocation
                    .elapsed
                    .context("MoveByAxis requires explicit elapsed input")?;
                let step = (delta
                    .checked_div(duration)
                    .context("MoveByAxis division overflow")?
                    >> 8)
                    .wrapping_mul(input.frame_scale as i32);
                let coordinate = &mut input.position[match axis {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }];
                if *target == 0 {
                    *target = coordinate.wrapping_add(delta);
                }
                let next = coordinate.wrapping_add(step);
                if (step < 0 && next <= *target)
                    || (step > 0 && next >= *target)
                    || elapsed >= duration.wrapping_sub(1)
                {
                    *coordinate = *target;
                    true
                } else {
                    *coordinate = next;
                    false
                }
            }
            Self::ScaledMovement { origin } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("sMoveByAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sMoveByAxis requires explicit elapsed input")?;
                let distance = delta
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("sMoveByAxis division by zero or overflow")?
                    .wrapping_mul(elapsed)
                    >> 8;
                let complete = elapsed >= duration.wrapping_sub(1);
                if (0..=2).contains(&axis) {
                    let input = spatial
                        .as_mut()
                        .context("sMoveByAxis requires spatial state")?;
                    let coordinate = &mut input.position[axis as usize];
                    if *origin == 0 && elapsed == 0 {
                        *origin = *coordinate;
                    }
                    *coordinate = origin.wrapping_add(if complete { delta } else { distance });
                }
                complete
            }
            Self::Rotation {
                target,
                accumulator,
            } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("RotateByAxis requires three arguments")
                };
                let (delta, duration) = (delta.integer()?, duration.integer()?);
                if delta == 0 || duration == 0 {
                    return Ok(true);
                }
                let input = spatial.as_mut().context(
                    "RotateByAxis requires spatial state, frame_scale and nominal_ticks",
                )?;
                let axis = match axis.integer()? {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                };
                let current = i32::from(input.angles[axis]);
                let elapsed = invocation
                    .elapsed
                    .context("RotateByAxis requires explicit elapsed input")?;
                let quotient = delta
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("RotateByAxis fixed-point division overflow")?;
                if *target == 0 && elapsed == 0 {
                    *target = (current.wrapping_add(delta) & 2047) << 8;
                    *accumulator = current << 8;
                }
                let mut step = quotient.wrapping_mul(input.frame_scale as i32) >> 8;
                if step == 0 && input.nominal_ticks != 0 {
                    step = input.nominal_ticks as i32;
                    if *target < 0 {
                        step = step.wrapping_neg();
                    }
                }
                let difference = ((current - (*target >> 8) + 1024) & 2047) - 1024;
                if difference.abs() <= (step >> 8).abs() {
                    input.angles[axis] = (*target >> 8) as u16;
                    true
                } else {
                    *accumulator = accumulator.wrapping_add(step);
                    input.angles[axis] = ((*accumulator >> 8) & 2047) as u16;
                    false
                }
            }
            Self::AbsoluteRotation {
                movement,
                accumulator,
            } => {
                let [axis, target, duration] = arguments else {
                    anyhow::bail!("RotateToAxis requires three arguments")
                };
                let duration = duration.integer()?;
                if duration == 0 {
                    return Ok(true);
                }
                let target = target.integer()? & 2047;
                let axis = match axis.integer()? {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                };
                let input = spatial
                    .as_mut()
                    .context("RotateToAxis requires spatial state")?;
                let current = i32::from(input.angles[axis]);
                let difference = (current & 2047) - target;
                let remaining = if difference < -1024 {
                    -(difference & 2047)
                } else if difference > 1024 {
                    2048 - difference
                } else {
                    -difference
                };
                let elapsed = invocation
                    .elapsed
                    .context("RotateToAxis requires elapsed input")?;
                if *movement == 0 && elapsed == 0 {
                    *movement = remaining << 8;
                    *accumulator = current << 8;
                }
                let mut step = movement
                    .checked_div(duration)
                    .context("RotateToAxis fixed-point division overflow")?
                    .wrapping_mul(input.frame_scale as i32)
                    >> 8;
                if step == 0 && input.nominal_ticks != 0 {
                    step = (input.nominal_ticks as i32).wrapping_shl(8);
                    if *movement < 0 {
                        step = step.wrapping_neg();
                    }
                }
                if remaining.abs() <= (step >> 8).abs() {
                    input.angles[axis] = target as u16;
                    true
                } else {
                    *accumulator = accumulator.wrapping_add(step);
                    input.angles[axis] = ((*accumulator >> 8) & 2047) as u16;
                    false
                }
            }
            Self::ScaledRotation { origin } => {
                let [axis, target, duration] = arguments else {
                    anyhow::bail!("sRotateToAxis requires three arguments")
                };
                let (axis, target, duration) =
                    (axis.integer()?, target.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sRotateToAxis requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("sRotateToAxis requires spatial state")?;
                if elapsed == 0 && (0..=2).contains(&axis) {
                    *origin = i32::from(input.angles[axis as usize]);
                }
                let difference = target.wrapping_sub(*origin);
                let movement = if difference < -1024 {
                    difference & 2047
                } else if difference > 1024 {
                    -(difference & 1023)
                } else {
                    difference
                };
                let end = duration.wrapping_sub(1);
                let offset = movement
                    .wrapping_shl(8)
                    .checked_div(end)
                    .context("sRotateToAxis duration causes division by zero or overflow")?
                    .wrapping_mul(elapsed)
                    >> 8;
                let complete = elapsed >= end;
                if (0..=2).contains(&axis) {
                    input.angles[axis as usize] = if complete {
                        (target & 2047) as u16
                    } else {
                        (origin.wrapping_add(offset) & 2047) as u16
                    };
                }
                complete
            }
            Self::ScaledRelativeRotation { origin } => {
                let [axis, delta, duration] = arguments else {
                    anyhow::bail!("sRotateByAxis requires three arguments")
                };
                let (axis, delta, duration) =
                    (axis.integer()?, delta.integer()?, duration.integer()?);
                let elapsed = invocation
                    .elapsed
                    .context("sRotateByAxis requires elapsed input")?;
                let input = spatial
                    .as_mut()
                    .context("sRotateByAxis requires spatial state")?;
                if elapsed == 0 && (0..=2).contains(&axis) {
                    *origin = i32::from(input.angles[axis as usize]);
                }
                let magnitude = delta
                    .wrapping_abs()
                    .wrapping_shl(8)
                    .checked_div(duration)
                    .context("sRotateByAxis duration causes division by zero or overflow")?
                    .wrapping_mul(elapsed)
                    >> 8;
                let offset = if delta < 0 {
                    magnitude.wrapping_neg()
                } else {
                    magnitude
                };
                let complete = elapsed >= duration.wrapping_sub(1);
                if (0..=2).contains(&axis) {
                    input.angles[axis as usize] =
                        (origin.wrapping_add(if complete { delta } else { offset }) & 2047) as u16;
                }
                complete
            }
            Self::Duration => {
                let [duration] = arguments else {
                    anyhow::bail!("Wait requires one argument")
                };
                invocation
                    .elapsed
                    .context("Wait requires explicit elapsed input")?
                    >= duration.integer()?.wrapping_sub(1)
            }
            Self::Tasks | Self::Blocking => {
                anyhow::ensure!(
                    arguments.is_empty(),
                    "task-count wait requires no arguments"
                );
                match self {
                    Self::Tasks => invocation.unpaused_tasks <= 1,
                    _ => invocation.unpaused_blocking_tasks <= 1,
                }
            }
            Self::Animation { group, started } | Self::ControlAnimation { group, started } => {
                let name = if turning {
                    "PushControlAnimation"
                } else {
                    "PushAnimation"
                };
                let [requested, count] = arguments else {
                    anyhow::bail!("{name} requires two arguments")
                };
                let requested = u16::try_from(requested.integer()?)
                    .with_context(|| format!("{name} group outside supported range"))?;
                anyhow::ensure!(
                    requested <= i16::MAX as u16,
                    "{name} signed group unsupported"
                );
                let count = count.integer()? as u8;
                let input = animation.as_mut().with_context(|| {
                    format!("{name} requires prepared animation and explicit nominal ticks")
                })?;
                *group = requested;
                if *started {
                    let running = input.animation.current_group() == Some(requested);
                    if turning && running {
                        let pose = spatial
                            .as_mut()
                            .context("PushControlAnimation requires receiver pose")?;
                        pose.angles[1] = crate::player::turn::turned(pose.angles[1], pose.turn);
                    }
                    !running
                } else {
                    if input.animation.current_group() == Some(requested) {
                        input.animation.set_loop_count(count)?;
                        *started = true;
                    } else {
                        let frame = match &mut input.sounds {
                            Some(sounds) => input.animation.request_with_sounds(
                                requested,
                                0,
                                input.ticks,
                                *sounds,
                            )?,
                            None => input.animation.request(requested, 0, input.ticks)?,
                        };
                        *input.frame = frame.or(*input.frame);
                    }
                    false
                }
            }
        })
    }
}

/// Actor-owned native continuations with replacement-only persistence APIs.
/// Execution and polling remain internal to the game host.
#[derive(Default, Debug)]
pub struct NativeTasks {
    next: u64,
    tasks: BTreeMap<u64, Task>,
}
/// Locomotion operation whose geometry must be sampled for the actual receiver.
#[derive(Clone, Copy)]
pub(super) enum LocomotionOperation {
    Backward,
    Forward,
    WalkForward,
    Strafe(super::locomotion::Strafe),
}
impl LocomotionOperation {
    pub(super) fn named(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("MoveBackward") {
            Some(Self::Backward)
        } else if name.eq_ignore_ascii_case("MoveForward") {
            Some(Self::Forward)
        } else if name.eq_ignore_ascii_case("WalkForward") {
            Some(Self::WalkForward)
        } else if name.eq_ignore_ascii_case("MoveLeft") {
            Some(Self::Strafe(super::locomotion::Strafe::Left))
        } else if name.eq_ignore_ascii_case("MoveRight") {
            Some(Self::Strafe(super::locomotion::Strafe::Right))
        } else {
            None
        }
    }
}
/// Internal execution retains script task timing while allowing immediate movement.
#[derive(Clone, Copy)]
pub(super) enum NativeTaskMode {
    Script(CallKind),
    SynchronousMovement,
}
impl NativeTaskMode {
    pub(super) fn kind(self) -> CallKind {
        match self {
            Self::Script(kind) => kind,
            Self::SynchronousMovement => CallKind::Task,
        }
    }
}

impl NativeTasks {
    pub(super) fn locomotion_operation(&self, token: u64) -> Option<LocomotionOperation> {
        match self.tasks.get(&token)? {
            Task::Backward => Some(LocomotionOperation::Backward),
            Task::Forward => Some(LocomotionOperation::Forward),
            Task::WalkForward => Some(LocomotionOperation::WalkForward),
            _ => None,
        }
    }
    pub(super) fn call(
        &mut self,
        (function, kind): (&str, CallKind),
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<Option<CallResult>> {
        self.invoke(
            (function, NativeTaskMode::Script(kind)),
            arguments,
            invocation,
            animation,
            spatial,
            services,
        )
    }

    pub(super) fn call_synchronous_movement(
        &mut self,
        function: &str,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<Option<CallResult>> {
        anyhow::ensure!(
            LocomotionOperation::named(function).is_some()
                || function.eq_ignore_ascii_case("MoveLeft")
                || function.eq_ignore_ascii_case("MoveRight"),
            "synchronous native {function} is not a movement operation"
        );
        self.invoke(
            (function, NativeTaskMode::SynchronousMovement),
            arguments,
            invocation,
            animation,
            spatial,
            services,
        )
    }

    fn invoke(
        &mut self,
        (function, mode): (&str, NativeTaskMode),
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<Option<CallResult>> {
        let kind = mode.kind();
        let mut task = if function.eq_ignore_ascii_case("WaitOnDialog") {
            anyhow::ensure!(
                kind == CallKind::Task,
                "WaitOnDialog requires task encoding"
            );
            Task::Dialogue
        } else if function.eq_ignore_ascii_case("RTXp") {
            anyhow::ensure!(kind == CallKind::Task, "RTXp requires task encoding");
            Task::PlayerDialogue { started: false }
        } else if function.eq_ignore_ascii_case("SyncWithGroup") {
            anyhow::ensure!(
                kind == CallKind::Task,
                "SyncWithGroup requires task encoding"
            );
            Task::GroupSync
        } else if function.eq_ignore_ascii_case("RTX") {
            anyhow::ensure!(kind == CallKind::Task, "RTX requires task encoding");
            Task::SpeakerDialogue(Speech::default())
        } else if function.eq_ignore_ascii_case("rtxAnim")
            || function.eq_ignore_ascii_case("RTXpAnim")
        {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::SpeakerAnimation {
                speech: Speech::default(),
                index: 0,
                player: function.eq_ignore_ascii_case("RTXpAnim"),
            }
        } else if function.eq_ignore_ascii_case("AmbientRtx") {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "AmbientRtx requires multitask encoding"
            );
            Task::AmbientRtx {
                slot: None,
                lookup: Lookup::Idle,
            }
        } else if function.eq_ignore_ascii_case("AmbientSound") {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "AmbientSound requires multitask encoding"
            );
            Task::Ambient { channel: None }
        } else if function.eq_ignore_ascii_case("showPlayer") {
            anyhow::ensure!(kind == CallKind::Task, "showPlayer requires task encoding");
            Task::ShowPlayer { counter: 0 }
        } else if function.eq_ignore_ascii_case("showPlayerPan") {
            anyhow::ensure!(
                kind == CallKind::Task,
                "showPlayerPan requires task encoding"
            );
            Task::ShowPlayerPan {
                counter: 0,
                pan: None,
            }
        } else if let Some(command) = super::camera::ObjectPanCommand::named(function) {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::ObjectPan {
                command,
                counter: 0,
                pan: None,
            }
        } else if let Some(command) = super::camera::LocationCommand::named(function) {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::ShowLocation {
                command,
                counter: 0,
            }
        } else if function.eq_ignore_ascii_case("lookObj") {
            anyhow::ensure!(kind == CallKind::Task, "lookObj requires task encoding");
            Task::Look {
                counter: 0,
                look: None,
            }
        } else if function.eq_ignore_ascii_case("showObjRot") {
            anyhow::ensure!(kind == CallKind::Task, "showObjRot requires task encoding");
            Task::ObjectRotation
        } else if function.eq_ignore_ascii_case("RotateByAxis") {
            Task::Rotation {
                target: 0,
                accumulator: 0,
            }
        } else if function.eq_ignore_ascii_case("RotateToAxis") {
            Task::AbsoluteRotation {
                movement: 0,
                accumulator: 0,
            }
        } else if function.eq_ignore_ascii_case("sRotateToAxis") {
            Task::ScaledRotation { origin: 0 }
        } else if function.eq_ignore_ascii_case("sRotateByAxis") {
            Task::ScaledRelativeRotation { origin: 0 }
        } else if function.eq_ignore_ascii_case("FacePlayer") {
            Task::FacePlayer
        } else if function.eq_ignore_ascii_case("MoveBackward") {
            Task::Backward
        } else if function.eq_ignore_ascii_case("MoveForward") {
            Task::Forward
        } else if function.eq_ignore_ascii_case("WalkForward") {
            Task::WalkForward
        } else if function.eq_ignore_ascii_case("MoveLeft") {
            Task::Strafe(super::locomotion::Strafe::Left)
        } else if function.eq_ignore_ascii_case("MoveRight") {
            Task::Strafe(super::locomotion::Strafe::Right)
        } else if function.eq_ignore_ascii_case("MoveObjectAxis") {
            Task::ObjectMovement {
                target: 0,
                traveled: 0,
            }
        } else if function.eq_ignore_ascii_case("MoveByAxis") {
            Task::Movement { target: 0 }
        } else if function.eq_ignore_ascii_case("sMoveByAxis") {
            Task::ScaledMovement { origin: 0 }
        } else if function.eq_ignore_ascii_case("WanderToLocation") {
            Task::WanderLocation {
                started: false,
                speed: 0,
            }
        } else if function.eq_ignore_ascii_case("MoveAlongAxis") {
            Task::AxisMovement {
                target: 0,
                accumulated: 0,
            }
        } else if function.eq_ignore_ascii_case("MoveToLocation") {
            Task::Location
        } else if function.eq_ignore_ascii_case("Wait") {
            Task::Duration
        } else if function.eq_ignore_ascii_case("WaitOnTasks") {
            Task::Tasks
        } else if function.eq_ignore_ascii_case("WaitNonMulti") {
            Task::Blocking
        } else if function.eq_ignore_ascii_case("WaitAnimFrame")
            || function.eq_ignore_ascii_case("WaitPlayerAnimFrame")
        {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::AnimationFrame {
                player: function.eq_ignore_ascii_case("WaitPlayerAnimFrame"),
            }
        } else if function.eq_ignore_ascii_case("PushAnimation") {
            Task::Animation {
                group: 0,
                started: false,
            }
        } else if function.eq_ignore_ascii_case("PushControlAnimation") {
            Task::ControlAnimation {
                group: 0,
                started: false,
            }
        } else {
            return Ok(None);
        };
        anyhow::ensure!(
            kind != CallKind::Function,
            "function-encoded {function} timing unsupported"
        );
        let next = match mode {
            NativeTaskMode::Script(_) => Some(
                self.next
                    .checked_add(1)
                    .context("native task identity exhausted")?,
            ),
            NativeTaskMode::SynchronousMovement => None,
        };
        let complete = task.complete(arguments, invocation, animation, spatial, services)?;
        if let Some(next) = next {
            // Initial script completion still allocates and yields until a later task pass.
            self.next = next;
            self.tasks.insert(next, task);
            Ok(Some(CallResult::Pending(next)))
        } else {
            Ok(Some(CallResult::Complete(i32::from(!complete))))
        }
    }

    pub(super) fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        let task = self
            .tasks
            .get_mut(&token)
            .with_context(|| format!("native task {token} absent"))?;
        let completed = task.complete(arguments, invocation, animation, spatial, services)?;
        if completed {
            self.tasks.remove(&token);
        }
        Ok(completed)
    }
    /// Whether `token` is a camera continuation (`showPlayer`, `showPlayerPan`, an object pan or
    /// `lookObj`), which needs the camera session.
    pub(super) fn is_camera_task(&self, token: u64) -> bool {
        matches!(
            self.tasks.get(&token),
            Some(
                Task::ShowPlayer { .. }
                    | Task::ShowPlayerPan { .. }
                    | Task::ObjectPan { .. }
                    | Task::ShowLocation { .. }
                    | Task::Look { .. }
            )
        )
    }

    /// Whether the native named `function` is a camera task that needs the camera session.
    pub(super) fn camera_function(function: &str) -> bool {
        function.eq_ignore_ascii_case("showPlayer")
            || function.eq_ignore_ascii_case("showPlayerPan")
            || function.eq_ignore_ascii_case("lookObj")
            || super::camera::ObjectPanCommand::named(function).is_some()
            || super::camera::LocationCommand::named(function).is_some()
    }
    pub(super) fn cancel(&mut self, token: u64) -> Result<()> {
        self.tasks
            .remove(&token)
            .with_context(|| format!("native task {token} absent"))?;
        Ok(())
    }
    pub(super) fn animation_group(&self) -> Option<u16> {
        self.tasks.values().find_map(|task| match task {
            Task::Animation { group, .. } => Some(*group),
            _ => None,
        })
    }
}

#[cfg(test)]
mod frame_wait_tests {
    use super::*;

    #[test]
    fn frame_wait_poll_uses_full_arguments_and_current_before_pending() {
        for (current, pending, progress, group, threshold, expected) in [
            (7, -1, 0, 7, 1, false),
            (7, -1, 1, 7, 1, true),
            (7, -1, 255, 7, 256, false),
            (7, -1, 0, 7, i32::MIN, true),
            (7, -1, 255, 7, i32::MAX, false),
            (8, 7, 255, 7, 0, false),
            (8, 9, 0, 7, 255, true),
            (7, 7, 1, 7, 1, true),
            (-32768, -1, 1, -32768, 1, true),
            (-32768, -1, 0, 32768, 1, true),
            (-1, -1, 0, 65535, 1, true),
        ] {
            for player in [false, true] {
                let mut tasks = NativeTasks::default();
                let function = if player {
                    "WaitPlayerAnimFrame"
                } else {
                    "WaitAnimFrame"
                };
                let inputs = || NativeServices {
                    frame_wait: Some(FrameWaitCall {
                        receiver: if player {
                            Err("caller must not be read".into())
                        } else {
                            Ok((current, pending, progress))
                        },
                        player: if player {
                            Ok(Some((current, pending, progress)))
                        } else {
                            Err("player must not be read".into())
                        },
                    }),
                    ..Default::default()
                };
                let invocation = Invocation {
                    elapsed: None,
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                };
                let args = [Argument::Integer(group), Argument::Integer(threshold)];
                let Some(CallResult::Pending(token)) = tasks
                    .call(
                        (function, CallKind::Task),
                        &args,
                        invocation,
                        &mut None,
                        &mut None,
                        inputs(),
                    )
                    .unwrap()
                else {
                    panic!("task must allocate continuation")
                };
                assert_eq!(
                    tasks
                        .poll(token, &args, invocation, &mut None, &mut None, inputs())
                        .unwrap(),
                    expected,
                    "{function}: current={current}, pending={pending}, progress={progress}, group={group}, threshold={threshold}"
                );
            }
        }
    }
}
