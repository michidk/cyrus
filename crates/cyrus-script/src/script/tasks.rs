//! Actor-owned native task tokens share waits and animation requests.
use super::locomotion::MotionDirection;
use crate::script::LivingState;
use anyhow::{Context, Result};
use cyrus_soup::Native;
use cyrus_soup::program::CallKind;
use cyrus_soup::{AnimationGroup, Argument, CallResult, Invocation};
use cyrus_world::animation::{ActorAnimation, AnimationSound};
use cyrus_world::heading::wrap;
use std::collections::BTreeMap;
mod completion;
mod persistence;
pub use persistence::{NativeTaskBindings, NativeTasksSnapshot};

/// Generic native execution state shared by successive actors and task slots.
/// Four words belong to task slots; the separate elapsed/point value stays shared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeCommandState {
    pub continuation: u32,
    /// Second native continuation word; `FxPhase` uses it as the direction.
    #[serde(default)]
    pub continuation_direction: u32,
    /// Unsigned shared point-navigation pause in nominal ticks.
    pub pause_ticks: u32,
    pub point_value: u32,
    pub point_word: u16,
    pub point_phase: super::PointPhase,
    pub point_counter: u8,
}
/// Four native words retained by a task slot, excluding the shared elapsed/point value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct NativeTaskCommandState {
    continuation: u32,
    #[serde(default)]
    continuation_direction: u32,
    pause_ticks: u32,
    point_word: u16,
    point_phase: super::PointPhase,
    point_counter: u8,
}
/// One of the four 32-bit words a task slot retains, in the order the actor record keeps them
/// (`task_variable`, `task_variable2`, `task_variable3`, `task_variable4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskWord {
    /// First native continuation word (`AnimateGXA`'s loaded resource).
    Continuation,
    /// Pause countdown in nominal ticks.
    PauseTicks,
    /// Second native continuation word (`FxPhase`'s direction, `AnimateGXA`'s progress).
    ContinuationDirection,
    /// Command word in the low sixteen bits, phase byte, then counter byte.
    PointCommand,
}
impl NativeTaskCommandState {
    pub(super) fn word(self, word: TaskWord) -> u32 {
        match word {
            TaskWord::Continuation => self.continuation,
            TaskWord::PauseTicks => self.pause_ticks,
            TaskWord::ContinuationDirection => self.continuation_direction,
            TaskWord::PointCommand => {
                u32::from(self.point_word)
                    | u32::from(self.point_phase.byte()) << 16
                    | u32::from(self.point_counter) << 24
            }
        }
    }

    pub(super) fn set_word(&mut self, word: TaskWord, value: u32) {
        match word {
            TaskWord::Continuation => self.continuation = value,
            TaskWord::PauseTicks => self.pause_ticks = value,
            TaskWord::ContinuationDirection => self.continuation_direction = value,
            TaskWord::PointCommand => {
                self.point_word = value as u16;
                self.point_phase = super::PointPhase::from_byte((value >> 16) as u8);
                self.point_counter = (value >> 24) as u8;
            }
        }
    }

    pub(super) fn load(self, current: &mut NativeCommandState) {
        current.continuation = self.continuation;
        current.continuation_direction = self.continuation_direction;
        current.pause_ticks = self.pause_ticks;
        current.point_word = self.point_word;
        current.point_phase = self.point_phase;
        current.point_counter = self.point_counter;
    }
}
impl NativeCommandState {
    pub(super) fn task_state(self) -> NativeTaskCommandState {
        NativeTaskCommandState {
            continuation: self.continuation,
            continuation_direction: self.continuation_direction,
            pause_ticks: self.pause_ticks,
            point_word: self.point_word,
            point_phase: self.point_phase,
            point_counter: self.point_counter,
        }
    }

    pub(super) fn prepare(&mut self, site: cyrus_soup::NativeCallSite) {
        match site {
            cyrus_soup::NativeCallSite::Hook => {}
            cyrus_soup::NativeCallSite::ObjectFunctionValue => {
                *self = Self {
                    point_value: self.point_value,
                    ..Self::default()
                };
            }
            cyrus_soup::NativeCallSite::Statement(_)
            | cyrus_soup::NativeCallSite::FunctionValue => {
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
    pub angles: &'a mut [i32; 3],
    pub frame_scale: u32,
    /// Held-turn amount at `frame_scale` from the shared accumulators; negative turns left.
    pub turn: i64,
    pub nominal_ticks: u32,
    /// The shared tick counter `FaceLocation` compares its deadline with.
    pub tick: Option<u32>,
    pub matrix: cyrus_world::transform::Matrix,
    pub slave: bool,
    pub locations: &'a [[i32; 3]],
    /// The world's marker table, for `MoveToMarker`.
    pub markers: Option<&'a super::markers::Markers>,
    pub effective_position: Option<[i32; 3]>,
    /// Selected player's local position, when the caller supplies the player query.
    pub player_position: Option<[i32; 3]>,
    pub actor_index: usize,
    pub map_updates: &'a mut super::map_updates::MapUpdates,
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

/// The shared scripted menu for `menuProc`.
pub(super) struct MenuCall<'a> {
    pub state: &'a mut super::menu::MenuState,
    /// The menu-active word that `menuBegin` raises and `menuEnd` clears.
    pub active: &'a mut u16,
    pub input: Option<super::menu::MenuInput>,
}

/// Shared services sampled for each native task invocation.
type LightEffect<'a> = dyn FnMut(Native, &[Argument]) -> Result<bool> + 'a;

#[derive(Default)]
pub(super) struct NativeServices<'a> {
    pub animation_requests_enabled: bool,
    pub light_effect: Option<&'a mut LightEffect<'a>>,
    pub ambient: Option<&'a AmbientCall<'a>>,
    pub frame_wait: Option<FrameWaitCall>,
    pub dialogue: Option<&'a mut super::DialogueState>,
    pub activation: Option<&'a mut super::ActivationState>,
    pub menu: Option<MenuCall<'a>>,
    pub player_dialogue: Option<PlayerDialogueCall<'a>>,
    /// The selected player's animation, for `RTXpAnim`. Absent when the player is the receiver,
    /// whose own animation then serves, or when the player has no prepared animation.
    pub player_animation: Option<AnimationCall<'a, 'a>>,
    pub group_sync: Option<GroupSyncCall<'a>>,
    pub locomotion: Option<LocomotionCall<'a>>,
    pub camera: Option<CameraCall<'a>>,
    pub player_face: Option<PlayerFaceCall<'a>>,
    /// The selected player's alignment owner, for the lineup natives.
    pub lineup: Option<&'a mut (dyn super::lineup::LineupHost + 'a)>,
    /// The receiver's live state, for `BounceObject`.
    pub bounce: Option<&'a mut (dyn super::bounce::BouncePort + 'a)>,
    /// The receiver's facing and point-walking owner, for the `Face*` and `MoveNode*` tasks.
    pub point: Option<&'a mut (dyn PointHost + 'a)>,
    /// The image `ViewGXA` has the host draw over the world.
    pub gxa_overlay: Option<&'a mut Option<GxaOverlay>>,
    /// The task-slot state `AnimateGXA` keeps its loaded image and progress in.
    pub gxa_animation: Option<GxaAnimation<'a>>,
    /// The alignment flag (mask 2 of the flag byte) of `WaitLineUp`'s argument actor.
    pub alignment_flag: Option<bool>,
}

/// An installed GXA image frame drawn centered over the world frame while a `ViewGXA` task runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct GxaOverlay {
    /// The script's file name, relative to the install's game directory.
    pub name: String,
    pub frame: i32,
    /// `AnimateGXA`'s second image slot, as an offset from `frame`: the host draws frame
    /// `frame + offset` when it is below the file's frame count and `frame + 1` otherwise.
    /// `None` leaves the second slot clear, as `ViewGXA` does.
    pub second_offset: Option<i32>,
}

/// `AnimateGXA`'s task-slot words and the whole nominal-tick increment of this update.
pub(super) struct GxaAnimation<'a> {
    /// The loaded-image word of the task slot's native state: zero until the image is loaded.
    /// The original keeps a resource pointer here; only whether it is set is observable.
    pub resource: &'a mut u32,
    /// The animation progress word of the task slot's native state.
    pub progress: &'a mut i32,
    pub ticks: u32,
}

/// What `FaceAngle` and `FacePos` turn the receiver toward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FaceTarget {
    /// A heading in 2048ths of a turn.
    Angle(i32),
    /// A local Q8 X/Z point.
    Point([i32; 2]),
    /// An actor's local X/Z, read on every step (`FaceObject`).
    Actor(cyrus_soup::ActorReference),
}

/// Which point wrapper a move task steps: the node wrapper claims walk nodes along routes, the
/// direct wrapper walks straight at the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PointWrapper {
    Node,
    Direct,
}

/// What the `Move*` point tasks walk toward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MoveTarget {
    /// A local Q8 position.
    Position([i32; 3]),
    /// A marker index; the receiver's Y offset applies to its position.
    Marker(i32),
    /// A location index of the receiver.
    Location(i32),
}

/// The receiver's facing and point-walking owner: one `FaceAngle`/`FacePos` turn step or one
/// ordinary point-wrapper step per invocation.
pub(super) trait PointHost {
    /// One GotoLocation arrival/selector refresh, nonzero while pending.
    fn goto_location(&mut self, location: i32) -> Result<i32>;
    /// One point-wrapper step toward `target` within a Q8 arrival tolerance; the wrapper's
    /// return, nonzero while the walk continues.
    fn move_to(&mut self, wrapper: PointWrapper, target: MoveTarget, tolerance: i32)
    -> Result<i32>;

    /// The signed pre-turn difference, or zero when the receiver cannot turn, has arrived or its
    /// turn finished in this step.
    fn face(&mut self, target: FaceTarget) -> Result<i32>;

    /// One `PlayerOffsettask` step: place the selected player relative to the receiver and give it
    /// the receiver's orientation.
    fn player_offset(&mut self, offset: [i32; 3]) -> Result<()>;

    /// One persistent PlayerTilt orientation update; `reversed` selects PlayerTiltRevx's targets.
    fn player_tilt(
        &mut self,
        angle: i32,
        strength: i32,
        divisor: i32,
        reversed: bool,
    ) -> Result<()>;

    /// One `DragonBreathTask` update: true while the receiver keeps breathing.
    fn dragon_breath(&mut self) -> Result<bool>;

    /// One `projectileAI` update: true while the receiver keeps flying.
    fn projectile(&mut self) -> Result<bool>;

    /// The receiver's and the selected player's query positions, for `TestTask`.
    fn follow_positions(&mut self) -> Result<([i32; 3], [i32; 3])>;
}

/// The selected player's state for `PlayerFaceObj`, read before the call.
pub(super) struct PlayerFaceState {
    pub position: [i32; 3],
    pub angles: [u16; 3],
    /// [`super::alignment::speed`] of the player's current group.
    pub speed: u8,
    pub rate_attribute: u8,
    pub holding_weapon: bool,
    pub torch: bool,
    /// Whether the packed collision-response word is nonzero.
    pub response_present: bool,
    pub response_age: u8,
}

/// What `PlayerFaceObj` changed on the selected player; its owner applies it after the call.
#[derive(Default)]
pub(super) struct PlayerFaceOutcome {
    pub position: Option<[i32; 3]>,
    pub angles: Option<[u16; 3]>,
    /// The player's alignment flag (script-control bit 2, hook disabled) after the call.
    pub aligning: bool,
    /// Value for the native return-state byte, when the call sets it.
    pub return_state: Option<u8>,
}

/// The selected player's state and the destination of the call's changes. A failed read surfaces
/// only when the task uses it.
pub(super) struct PlayerFaceCall<'a> {
    pub state: Result<PlayerFaceState, String>,
    pub outcome: &'a mut PlayerFaceOutcome,
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
    /// Nonplayer deep-water edge probe of `MoveForward` and the ordinary strafes.
    pub edge_probe_clear: Option<bool>,
    /// Lateral step check of an ordinary `MoveLeft` or `MoveRight`.
    pub lateral_step_clear: Option<bool>,
    pub walk_forward_mode: bool,
    /// `WalkBackward` outside deep water; in deep water it is `MoveBackward`.
    pub walk_backward_mode: bool,
    /// `WalkBackward`'s step check.
    pub backward_step_clear: Option<bool>,
    pub forward_admitted: Option<bool>,
}

pub(super) struct MotionCall<'a> {
    /// Retained movement-helper classification, independent of displacement direction.
    pub classification: &'a mut MotionDirection,
    pub direction: &'a mut MotionDirection,
    pub counters: &'a mut u8,
}

/// Read-only animation state, sampled independently of playback tick availability.
pub(super) struct FrameWaitCall {
    pub receiver: Result<(i32, i32, u8), String>,
    pub player: Result<Option<(i32, i32, u8)>, String>,
}

/// The session's group channels and the receiver's channel (`attr_group`) for `SyncWithGroup`.
pub(super) struct GroupSyncCall<'a> {
    pub state: &'a mut super::GroupSync,
    pub actor_index: usize,
    pub channel: u8,
    pub user_channel: u8,
    pub user_has_active: bool,
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
        name: Native,
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
        .map(AnimationGroup::from_index)
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

/// Preserve the engine's signed sixteen-bit animation-group representation while
/// retaining the script's full integer operand for later comparisons.
fn animation_group_bits(group: i32) -> Option<AnimationGroup> {
    i16::try_from(group)
        .ok()
        .map(|value| AnimationGroup::from_index(value as u16))
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
    RadiusFlicker,
    ColorPhase,
    ColorFlicker,
    /// The installed `multitask PlayAnimation(10, 0)` request retains its slot across main loops.
    AnimationRequest,
    Dialogue,
    /// `MoveToMarker`: a timed straight move to a marker.
    Marker,
    /// `menuProc`: the shared scripted menu's highlight and selection.
    Menu,
    AnimationFrame {
        player: bool,
    },
    PlayerDialogue {
        started: bool,
    },
    /// `SyncWithGroup`.
    GroupSync,
    UserSync,
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
    /// `lookCyrus`: `lookObj` with the selected player as the target.
    LookCyrus {
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
    /// `PlayerFaceTalk(distance)`: aligns the selected player in front of the receiver.
    PlayerFaceTalk,
    /// `ObjectLineUp(angle, x_offset, z_offset)`: walks the receiver to the source actor's frame.
    ObjectLineUp,
    /// `ObjectParallelLineUp(angle, x_offset, z_offset)`: the plain mover on the receiver, toward
    /// the source actor's frame with unrotated offsets.
    ObjectParallelLineUp,
    /// `LineUp(angle, x_offset, z_offset)`: the close mover on the receiver toward the reference
    /// actor retained from the first invocation's shared native-command source.
    ReceiverLineUp {
        fallback: bool,
        reference: Option<u32>,
    },
    /// `ObjectPickupLineUp(angle, x_offset, z_offset)`: the pickup mover on the receiver, toward
    /// the shared source actor's position plus unrotated offsets.
    ObjectPickupLineUp {
        fallback: bool,
    },
    /// `FaceLocation(location, deadline)`: turns toward a location table entry by the shared tick
    /// counter's distance to the deadline.
    FaceLocation,
    /// `FacePlayerInertia`: `FacePlayer` whose step shrinks by the elapsed input.
    FacePlayerInertia,
    /// `Move(x, y, z, duration)`: adds each axis's operand times the nominal tick count to the
    /// position on every invocation until the elapsed input reaches `duration - 1`.
    Move,
    /// `Rotate(x, y, z, duration)`: adds each axis's angle step to the angles on every
    /// invocation until the elapsed input reaches `duration - 1`.
    Rotate,
    /// `sMoveAlongAxis(axis, delta, duration)`: each invocation adds the per-frame step along an
    /// axis, rotated by the receiver's angles, until the elapsed input reaches `duration - 1`.
    ScaledAxisMovement,
    /// `sMove(x, y, z, duration)`: `Move` with each operand divided by the duration instead of
    /// scaled by the tick count.
    ScaledMove,
    /// `sRotate(x, y, z, duration)`: the same step as `Rotate`.
    ScaledRotate,
    /// `PlayerOffsettask(x, y, z)`: every invocation places the selected player at the receiver's
    /// position plus the offset and gives it the receiver's orientation; it never completes.
    PlayerOffset,
    /// `PlayerTilt`: recomputes receiver tilt from the selected player each invocation.
    PlayerTilt,
    /// `PlayerTiltRevx`: `PlayerTilt` with the reversed pitch and roll targets.
    PlayerTiltRevx,
    /// `DragonBreathTask()`: the `DragonBreath` update every invocation, completing when it returns zero.
    DragonBreath,
    /// `projectileAI()`: one projectile flight step every invocation, completing when it ends.
    Projectile,
    /// `MoveMarker(marker, tolerance)`: the direct point wrapper toward a marker.
    MoveMarker,
    /// `MoveLocation(location, tolerance)`: the direct point wrapper toward a receiver location.
    MoveLocation,
    /// `GotoLocation(location)`: arrival snap or retained AI location/selector refresh.
    GotoLocation,
    /// `TestTask(value)` (named for what it does): follows the selected player with the node walk; never completes.
    FollowPlayer,
    /// `MovePosition(x, y, z, tolerance)`: the direct point wrapper toward a Q8 position.
    MovePosition,
    /// `MoveNodeMarker(marker, tolerance)`: the point wrapper toward a marker.
    MoveNodeMarker,
    /// `MoveNodeLocation(location, tolerance)`: the point wrapper toward a receiver location.
    MoveNodeLocation,
    /// `MoveNodePosition(x, y, z, tolerance)`: the point wrapper toward a Q8 position.
    MoveNodePosition,
    /// `FaceAngle(angle)`: one turn step per invocation toward a heading, completing on arrival.
    FaceAngle,
    /// `FacePos(x, z)`: `FaceAngle` toward a point, snapping onto it within the facing threshold.
    FacePos,
    /// `FaceObject(target)`: `FacePos` toward the target actor's local X/Z.
    FaceObject,
    Backward,
    /// `WalkBackward`: `MoveBackward` outside deep water with its own groups, step attributes and
    /// step check.
    WalkBackward,
    Strafe(super::locomotion::Strafe),
    Forward,
    WalkForward,
    /// The selected player's attack lunge: `MoveForward`'s step with `attr_fight_lunge_speed` and no group request.
    Lunge,
    Location,
    Duration,
    /// `ViewGXA(name, frame, count)`: an installed image over the world for `count` counter ticks.
    ViewGxa,
    /// `AnimateGXA(name, first, cycle, count)`: two cycling images over the world for `count`
    /// counter ticks, advanced by the shared progress.
    AnimateGxa,
    /// `WaitLineUp(actor)`: active while the argument actor's alignment flag is set.
    WaitLineUp,
    Tasks,
    Blocking,
    Animation {
        /// Full script operand; comparisons use the current group's signed 16-bit value.
        group: i32,
        started: bool,
    },
    /// `PushControlAnimation`: `PushAnimation` that also applies the held ground turn.
    ControlAnimation {
        /// Full script operand; comparisons use the current group's signed 16-bit value.
        group: i32,
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
    /// `PlayerFaceObj`: turns the selected player toward the receiver.
    PlayerFace,
    /// `PlayerPickupLineUp`: the pickup mover; `fallback` is its retained switch to the plain mover.
    PickupLineUp {
        fallback: bool,
    },
    /// `PlayerParallelLineUp`: the plain mover.
    ParallelLineUp,
    /// `BounceObject`: the launch state machine and its retained first-invocation flag.
    Bounce(super::bounce::BounceState),
    /// `PlayerLineUp`: the distance mode chosen on the first invocation, the fallback flag and
    /// the close mode's retained reference actor index.
    LineUp {
        initialized: bool,
        approach: bool,
        fallback: bool,
        reference: Option<u32>,
    },
}
impl Task {
    fn complete(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        match self {
            Self::RadiusFlicker | Self::ColorPhase | Self::ColorFlicker => {
                self.complete_effects(arguments, services)
            }
            Self::PlayerDialogue { .. }
            | Self::SpeakerDialogue(..)
            | Self::SpeakerAnimation { .. }
            | Self::Dialogue
            | Self::Menu => self.complete_dialogue(arguments, invocation, animation, services),
            Self::GroupSync
            | Self::UserSync
            | Self::Duration
            | Self::WaitLineUp
            | Self::Tasks
            | Self::Blocking => self.complete_waits(arguments, invocation, services),
            Self::AnimationFrame { .. }
            | Self::AnimationRequest
            | Self::Animation { .. }
            | Self::ControlAnimation { .. } => {
                self.complete_animation(arguments, animation, spatial, services)
            }
            Self::ShowPlayer { .. }
            | Self::ShowPlayerPan { .. }
            | Self::ObjectPan { .. }
            | Self::ShowLocation { .. }
            | Self::Look { .. }
            | Self::LookCyrus { .. }
            | Self::ObjectRotation => self.complete_camera(arguments, invocation, services),
            Self::Ambient { .. } | Self::AmbientRtx { .. } => {
                self.complete_ambient(arguments, invocation, services)
            }
            Self::ScaledAxisMovement
            | Self::Move
            | Self::Rotate
            | Self::ScaledMove
            | Self::ScaledRotate
            | Self::AxisMovement { .. }
            | Self::Marker
            | Self::Location
            | Self::ObjectMovement { .. }
            | Self::Movement { .. }
            | Self::ScaledMovement { .. } => {
                self.complete_translation(arguments, invocation, spatial)
            }
            Self::GotoLocation
            | Self::MoveMarker
            | Self::MoveLocation
            | Self::MovePosition
            | Self::MoveNodeMarker
            | Self::MoveNodeLocation
            | Self::MoveNodePosition
            | Self::FollowPlayer
            | Self::PlayerOffset
            | Self::PlayerTilt
            | Self::PlayerTiltRevx
            | Self::DragonBreath
            | Self::Projectile
            | Self::FaceAngle
            | Self::FacePos
            | Self::FaceObject
            | Self::FacePlayer
            | Self::FacePlayerInertia
            | Self::FaceLocation
            | Self::WanderLocation { .. } => {
                self.complete_point(arguments, invocation, spatial, services)
            }
            Self::Bounce(..)
            | Self::PickupLineUp { .. }
            | Self::ParallelLineUp
            | Self::LineUp { .. }
            | Self::ObjectPickupLineUp { .. }
            | Self::ReceiverLineUp { .. }
            | Self::ObjectParallelLineUp
            | Self::ObjectLineUp
            | Self::PlayerFaceTalk
            | Self::PlayerFace => self.complete_alignment(arguments, invocation, spatial, services),
            Self::Backward
            | Self::WalkBackward
            | Self::Forward
            | Self::WalkForward
            | Self::Lunge
            | Self::Strafe(..) => {
                self.complete_locomotion(arguments, invocation, animation, spatial, services)
            }
            Self::Rotation { .. }
            | Self::AbsoluteRotation { .. }
            | Self::ScaledRotation { .. }
            | Self::ScaledRelativeRotation { .. } => {
                self.complete_rotation(arguments, invocation, spatial)
            }
            Self::ViewGxa | Self::AnimateGxa => {
                self.complete_overlay(arguments, invocation, services)
            }
        }
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LocomotionOperation {
    Backward,
    WalkBackward,
    Forward,
    WalkForward,
    /// The selected player's attack lunge: a forward step without an animation request.
    Lunge,
    Strafe(super::locomotion::Strafe),
}
impl LocomotionOperation {
    pub(super) fn named(name: Native) -> Option<Self> {
        if name == Native::MoveBackward {
            Some(Self::Backward)
        } else if name == Native::WalkBackward {
            Some(Self::WalkBackward)
        } else if name == Native::MoveForward {
            Some(Self::Forward)
        } else if name == Native::WalkForward {
            Some(Self::WalkForward)
        } else if name == Native::MoveLeft {
            Some(Self::Strafe(super::locomotion::Strafe::Left))
        } else if name == Native::MoveRight {
            Some(Self::Strafe(super::locomotion::Strafe::Right))
        } else {
            None
        }
    }

    /// The native whose handler runs the step; the lunge runs `MoveForward`'s forward step.
    pub(super) fn native(self) -> Native {
        match self {
            Self::Backward => Native::MoveBackward,
            Self::WalkBackward => Native::WalkBackward,
            Self::Forward | Self::Lunge => Native::MoveForward,
            Self::WalkForward => Native::WalkForward,
            Self::Strafe(super::locomotion::Strafe::Left) => Native::MoveLeft,
            Self::Strafe(super::locomotion::Strafe::Right) => Native::MoveRight,
        }
    }

    fn task(self) -> Task {
        match self {
            Self::Backward => Task::Backward,
            Self::WalkBackward => Task::WalkBackward,
            Self::Forward => Task::Forward,
            Self::Lunge => Task::Lunge,
            Self::WalkForward => Task::WalkForward,
            Self::Strafe(strafe) => Task::Strafe(strafe),
        }
    }
}
/// Internal execution retains script task timing while allowing immediate movement.
#[derive(Clone, Copy)]
pub(super) enum NativeTaskMode {
    Script(CallKind),
    SynchronousMovement(LocomotionOperation),
}
impl NativeTaskMode {
    pub(super) fn kind(self) -> CallKind {
        match self {
            Self::Script(kind) => kind,
            Self::SynchronousMovement(_) => CallKind::Task,
        }
    }
}

impl NativeTasks {
    /// Whether any native task record is live, which the original keeps as a word on the actor.
    pub(super) fn is_active(&self) -> bool {
        !self.tasks.is_empty()
    }
    pub(super) fn locomotion_operation(&self, token: u64) -> Option<LocomotionOperation> {
        match self.tasks.get(&token)? {
            Task::Backward => Some(LocomotionOperation::Backward),
            Task::WalkBackward => Some(LocomotionOperation::WalkBackward),
            Task::Forward => Some(LocomotionOperation::Forward),
            Task::Lunge => Some(LocomotionOperation::Lunge),
            Task::WalkForward => Some(LocomotionOperation::WalkForward),
            _ => None,
        }
    }
    pub(super) fn call(
        &mut self,
        (function, kind): (Native, CallKind),
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
        operation: LocomotionOperation,
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<Option<CallResult>> {
        self.invoke(
            (
                operation.native(),
                NativeTaskMode::SynchronousMovement(operation),
            ),
            arguments,
            invocation,
            animation,
            spatial,
            services,
        )
    }

    fn invoke(
        &mut self,
        (function, mode): (Native, NativeTaskMode),
        arguments: &[Argument],
        invocation: Invocation,
        animation: &mut Option<AnimationCall<'_, '_>>,
        spatial: &mut Option<SpatialCall<'_>>,
        services: NativeServices<'_>,
    ) -> Result<Option<CallResult>> {
        let kind = mode.kind();
        let mut task = if let NativeTaskMode::SynchronousMovement(operation) = mode {
            operation.task()
        } else if function == Native::WaitOnDialog {
            anyhow::ensure!(
                kind == CallKind::Task,
                "WaitOnDialog requires task encoding"
            );
            Task::Dialogue
        } else if function == Native::MenuProc {
            anyhow::ensure!(kind == CallKind::Task, "menuProc requires task encoding");
            Task::Menu
        } else if function == Native::RTXp {
            anyhow::ensure!(
                matches!(kind, CallKind::Task | CallKind::Multitask),
                "RTXp requires task or multitask encoding"
            );
            Task::PlayerDialogue { started: false }
        } else if function == Native::SyncWithUser {
            anyhow::ensure!(
                kind == CallKind::Task,
                "SyncWithUser requires task encoding"
            );
            Task::UserSync
        } else if function == Native::SyncWithGroup {
            anyhow::ensure!(
                kind == CallKind::Task,
                "SyncWithGroup requires task encoding"
            );
            Task::GroupSync
        } else if function == Native::Rtx {
            anyhow::ensure!(
                matches!(kind, CallKind::Task | CallKind::Multitask),
                "RTX requires task or multitask encoding"
            );
            Task::SpeakerDialogue(Speech::default())
        } else if function == Native::RtxAnim || function == Native::RTXpAnim {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::SpeakerAnimation {
                speech: Speech::default(),
                index: 0,
                player: function == Native::RTXpAnim,
            }
        } else if function == Native::AmbientRtx {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "AmbientRtx requires multitask encoding"
            );
            Task::AmbientRtx {
                slot: None,
                lookup: Lookup::Idle,
            }
        } else if function == Native::AmbientSound {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "AmbientSound requires multitask encoding"
            );
            Task::Ambient { channel: None }
        } else if function == Native::ShowPlayer {
            anyhow::ensure!(kind == CallKind::Task, "showPlayer requires task encoding");
            Task::ShowPlayer { counter: 0 }
        } else if function == Native::ShowPlayerPan {
            anyhow::ensure!(
                kind == CallKind::Task,
                "showPlayerPan requires task encoding"
            );
            Task::ShowPlayerPan {
                counter: 0,
                pan: None,
            }
        } else if let Some(command) = super::camera::ObjectPanCommand::named(function) {
            anyhow::ensure!(
                matches!(kind, CallKind::Task | CallKind::Multitask),
                "{function} requires task or multitask encoding"
            );
            Task::ObjectPan {
                command,
                counter: 0,
                pan: None,
            }
        } else if let Some(command) = super::camera::LocationCommand::named(function) {
            anyhow::ensure!(
                matches!(kind, CallKind::Task | CallKind::Multitask),
                "{function} requires task or multitask encoding"
            );
            Task::ShowLocation {
                command,
                counter: 0,
            }
        } else if function == Native::LookObj {
            anyhow::ensure!(kind == CallKind::Task, "lookObj requires task encoding");
            Task::Look {
                counter: 0,
                look: None,
            }
        } else if function == Native::LookCyrus {
            anyhow::ensure!(kind == CallKind::Task, "lookCyrus requires task encoding");
            Task::LookCyrus {
                counter: 0,
                look: None,
            }
        } else if function == Native::ShowObjRot {
            anyhow::ensure!(kind == CallKind::Task, "showObjRot requires task encoding");
            Task::ObjectRotation
        } else if function == Native::RotateByAxis {
            Task::Rotation {
                target: 0,
                accumulator: 0,
            }
        } else if function == Native::RotateToAxis {
            Task::AbsoluteRotation {
                movement: 0,
                accumulator: 0,
            }
        } else if function == Native::SRotateToAxis {
            Task::ScaledRotation { origin: 0 }
        } else if function == Native::SRotateByAxis {
            Task::ScaledRelativeRotation { origin: 0 }
        } else if function == Native::PlayerFaceObj {
            anyhow::ensure!(
                kind == CallKind::Task,
                "PlayerFaceObj requires task encoding"
            );
            Task::PlayerFace
        } else if function == Native::PlayerPickupLineUp {
            anyhow::ensure!(
                kind == CallKind::Task,
                "PlayerPickupLineUp requires task encoding"
            );
            Task::PickupLineUp { fallback: false }
        } else if function == Native::ObjectPickupLineUp {
            anyhow::ensure!(
                kind == CallKind::Task,
                "ObjectPickupLineUp requires task encoding"
            );
            Task::ObjectPickupLineUp { fallback: false }
        } else if function == Native::ObjectLineUp {
            anyhow::ensure!(
                kind == CallKind::Task,
                "ObjectLineUp requires task encoding"
            );
            Task::ObjectLineUp
        } else if function == Native::PlayerFaceTalk {
            anyhow::ensure!(
                kind == CallKind::Task,
                "PlayerFaceTalk requires task encoding"
            );
            Task::PlayerFaceTalk
        } else if function == Native::PlayerLineUp {
            anyhow::ensure!(
                kind == CallKind::Task,
                "PlayerLineUp requires task encoding"
            );
            Task::LineUp {
                initialized: false,
                approach: false,
                fallback: false,
                reference: None,
            }
        } else if function == Native::LineUp {
            anyhow::ensure!(kind == CallKind::Task, "LineUp requires task encoding");
            Task::ReceiverLineUp {
                fallback: false,
                reference: None,
            }
        } else if function == Native::ObjectParallelLineUp {
            anyhow::ensure!(
                kind == CallKind::Task,
                "ObjectParallelLineUp requires task encoding"
            );
            Task::ObjectParallelLineUp
        } else if function == Native::BounceObject {
            anyhow::ensure!(
                kind == CallKind::Task,
                "BounceObject requires task encoding"
            );
            Task::Bounce(super::bounce::BounceState::default())
        } else if function == Native::PlayerParallelLineUp {
            anyhow::ensure!(
                kind == CallKind::Task,
                "PlayerParallelLineUp requires task encoding"
            );
            Task::ParallelLineUp
        } else if function == Native::FxFlickerDim {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "FxFlickerDim requires multitask encoding"
            );
            Task::ColorFlicker
        } else if function == Native::FxPhase {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "FxPhase requires multitask encoding"
            );
            Task::ColorPhase
        } else if function == Native::FxFlickerOnOff {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "FxFlickerOnOff requires multitask encoding"
            );
            Task::RadiusFlicker
        } else if function == Native::Move {
            Task::Move
        } else if function == Native::Rotate {
            Task::Rotate
        } else if function == Native::SMoveAlongAxis {
            Task::ScaledAxisMovement
        } else if function == Native::SMove {
            Task::ScaledMove
        } else if function == Native::SRotate {
            Task::ScaledRotate
        } else if function == Native::PlayerOffsettask {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "PlayerOffsettask requires multitask encoding"
            );
            Task::PlayerOffset
        } else if function == Native::PlayerTilt {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "PlayerTilt requires multitask encoding"
            );
            Task::PlayerTilt
        } else if function == Native::PlayerTiltRevx {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "PlayerTiltRevx requires multitask encoding"
            );
            Task::PlayerTiltRevx
        } else if function == Native::DragonBreathTask {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "DragonBreathTask requires multitask encoding"
            );
            Task::DragonBreath
        } else if function == Native::ProjectileAI {
            Task::Projectile
        } else if function == Native::MoveMarker {
            Task::MoveMarker
        } else if function == Native::GotoLocation {
            anyhow::ensure!(
                kind == CallKind::Task,
                "GotoLocation requires task encoding"
            );
            Task::GotoLocation
        } else if function == Native::MoveLocation {
            Task::MoveLocation
        } else if function == Native::TestTask {
            Task::FollowPlayer
        } else if function == Native::MovePosition {
            Task::MovePosition
        } else if function == Native::MoveNodeMarker {
            Task::MoveNodeMarker
        } else if function == Native::MoveNodeLocation {
            Task::MoveNodeLocation
        } else if function == Native::MoveNodePosition {
            Task::MoveNodePosition
        } else if function == Native::FaceAngle {
            Task::FaceAngle
        } else if function == Native::FacePos {
            Task::FacePos
        } else if function == Native::FaceObject {
            Task::FaceObject
        } else if function == Native::FacePlayer {
            Task::FacePlayer
        } else if function == Native::FacePlayerInertia {
            Task::FacePlayerInertia
        } else if function == Native::FaceLocation {
            Task::FaceLocation
        } else if function == Native::MoveBackward {
            Task::Backward
        } else if function == Native::WalkBackward {
            Task::WalkBackward
        } else if function == Native::MoveForward {
            Task::Forward
        } else if function == Native::WalkForward {
            Task::WalkForward
        } else if function == Native::MoveLeft {
            Task::Strafe(super::locomotion::Strafe::Left)
        } else if function == Native::MoveRight {
            Task::Strafe(super::locomotion::Strafe::Right)
        } else if function == Native::MoveObjectAxis {
            Task::ObjectMovement {
                target: 0,
                traveled: 0,
            }
        } else if function == Native::MoveByAxis {
            Task::Movement { target: 0 }
        } else if function == Native::SMoveByAxis {
            Task::ScaledMovement { origin: 0 }
        } else if function == Native::WanderToLocation {
            Task::WanderLocation {
                started: false,
                speed: 0,
            }
        } else if function == Native::MoveAlongAxis {
            Task::AxisMovement {
                target: 0,
                accumulated: 0,
            }
        } else if function == Native::MoveToMarker {
            Task::Marker
        } else if function == Native::MoveToLocation {
            Task::Location
        } else if function == Native::Wait {
            Task::Duration
        } else if function == Native::ViewGXA {
            Task::ViewGxa
        } else if function == Native::AnimateGXA {
            Task::AnimateGxa
        } else if function == Native::WaitLineUp {
            Task::WaitLineUp
        } else if function == Native::WaitOnTasks {
            Task::Tasks
        } else if function == Native::WaitNonMulti {
            Task::Blocking
        } else if function == Native::WaitAnimFrame || function == Native::WaitPlayerAnimFrame {
            anyhow::ensure!(kind == CallKind::Task, "{function} requires task encoding");
            Task::AnimationFrame {
                player: function == Native::WaitPlayerAnimFrame,
            }
        } else if function == Native::PlayAnimation && kind != CallKind::Function {
            anyhow::ensure!(
                kind == CallKind::Multitask,
                "PlayAnimation requires function or installed multitask encoding"
            );
            Task::AnimationRequest
        } else if function == Native::PushAnimation {
            Task::Animation {
                group: 0,
                started: false,
            }
        } else if function == Native::PushControlAnimation {
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
            NativeTaskMode::SynchronousMovement(_) => None,
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
    /// Whether `token` is a `PlayerFaceObj` continuation, which needs the selected player's state.
    /// Whether `function` is a lineup native, which needs the selected player's alignment owner.
    pub(super) fn is_lineup_function(function: Native) -> bool {
        [
            Native::PlayerPickupLineUp,
            Native::PlayerParallelLineUp,
            Native::PlayerLineUp,
            Native::PlayerFaceTalk,
            Native::LineUp,
            Native::ObjectParallelLineUp,
            Native::ObjectLineUp,
            Native::ObjectPickupLineUp,
        ]
        .contains(&function)
    }

    /// Whether `token` is a `WaitLineUp` continuation, which needs its argument actor's flag.
    pub(super) fn is_wait_lineup_task(&self, token: u64) -> bool {
        matches!(self.tasks.get(&token), Some(Task::WaitLineUp))
    }

    /// Whether `token` is a lineup continuation.
    pub(super) fn is_lineup_task(&self, token: u64) -> bool {
        matches!(
            self.tasks.get(&token),
            Some(
                Task::PickupLineUp { .. }
                    | Task::ParallelLineUp
                    | Task::LineUp { .. }
                    | Task::PlayerFaceTalk
                    | Task::ReceiverLineUp { .. }
                    | Task::ObjectParallelLineUp
                    | Task::ObjectLineUp
                    | Task::ObjectPickupLineUp { .. }
            )
        )
    }

    /// Whether `token` is a `PlayerFaceTalk` continuation, whose receiver may be the player.
    pub(super) fn is_face_talk_task(&self, token: u64) -> bool {
        matches!(self.tasks.get(&token), Some(Task::PlayerFaceTalk))
    }

    /// Whether `token` is an `ObjectLineUp` continuation, whose mover is the receiver itself.
    pub(super) fn is_object_lineup_task(&self, token: u64) -> bool {
        matches!(
            self.tasks.get(&token),
            Some(
                Task::ReceiverLineUp { .. }
                    | Task::ObjectParallelLineUp
                    | Task::ObjectLineUp
                    | Task::ObjectPickupLineUp { .. }
            )
        )
    }

    /// Whether `function` is `BounceObject`, which needs the receiver's live state.
    pub(super) fn is_bounce_function(function: Native) -> bool {
        function == Native::BounceObject
    }

    /// Whether `token` is a `BounceObject` continuation.
    pub(super) fn is_bounce_task(&self, token: u64) -> bool {
        matches!(self.tasks.get(&token), Some(Task::Bounce(_)))
    }

    /// Whether `function` is a facing task, which needs the receiver's facing owner.
    pub(super) fn is_point_function(function: Native) -> bool {
        [
            Native::FaceAngle,
            Native::FacePos,
            Native::FaceObject,
            Native::PlayerOffsettask,
            Native::PlayerTilt,
            Native::PlayerTiltRevx,
            Native::DragonBreathTask,
            Native::ProjectileAI,
            Native::MoveMarker,
            Native::MoveLocation,
            Native::GotoLocation,
            Native::MovePosition,
            Native::TestTask,
            Native::MoveNodeMarker,
            Native::MoveNodeLocation,
            Native::MoveNodePosition,
        ]
        .contains(&function)
    }

    /// Whether `token` is a facing continuation.
    pub(super) fn is_point_task(&self, token: u64) -> bool {
        matches!(
            self.tasks.get(&token),
            Some(
                Task::FaceAngle
                    | Task::FacePos
                    | Task::FaceObject
                    | Task::PlayerTilt
                    | Task::PlayerTiltRevx
                    | Task::PlayerOffset
                    | Task::DragonBreath
                    | Task::Projectile
                    | Task::MoveMarker
                    | Task::MoveLocation
                    | Task::GotoLocation
                    | Task::MovePosition
                    | Task::FollowPlayer
                    | Task::MoveNodeMarker
                    | Task::MoveNodeLocation
                    | Task::MoveNodePosition
            )
        )
    }

    pub(super) fn is_player_face_task(&self, token: u64) -> bool {
        matches!(self.tasks.get(&token), Some(Task::PlayerFace))
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
                    | Task::LookCyrus { .. }
            )
        )
    }

    /// Whether the native named `function` is a camera task that needs the camera session.
    pub(super) fn camera_function(function: Native) -> bool {
        function == Native::ShowPlayer
            || function == Native::ShowPlayerPan
            || function == Native::LookObj
            || function == Native::LookCyrus
            || super::camera::ObjectPanCommand::named(function).is_some()
            || super::camera::LocationCommand::named(function).is_some()
    }
    pub(super) fn cancel(&mut self, token: u64) -> Result<()> {
        self.tasks
            .remove(&token)
            .with_context(|| format!("native task {token} absent"))?;
        Ok(())
    }
    pub(super) fn animation_group(&self) -> Option<AnimationGroup> {
        self.tasks.values().find_map(|task| match task {
            Task::Animation { group, .. } => animation_group_bits(*group),
            _ => None,
        })
    }
}

#[cfg(test)]
mod animation_group_tests {
    use super::animation_group_bits;
    use cyrus_soup::AnimationGroup;

    #[test]
    fn signed_group_operands_keep_their_bit_pattern() {
        assert_eq!(
            animation_group_bits(-1),
            Some(AnimationGroup::from_index(u16::MAX))
        );
        assert_eq!(
            animation_group_bits(-32768),
            Some(AnimationGroup::from_index(0x8000))
        );
        assert_eq!(
            animation_group_bits(32767),
            Some(AnimationGroup::from_index(0x7fff))
        );
        assert_eq!(animation_group_bits(32768), None);
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
                    Native::WaitPlayerAnimFrame
                } else {
                    Native::WaitAnimFrame
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

#[cfg(test)]
mod view_gxa_tests {
    use super::*;

    #[test]
    fn view_gxa_shows_its_frame_until_the_count_elapses_then_clears_it() {
        let mut tasks = NativeTasks::default();
        let mut overlay = None;
        let args = [
            Argument::String("system\\stmap01.gxa".into()),
            Argument::Integer(2),
            Argument::Integer(3),
        ];
        let step = |elapsed| Invocation {
            elapsed: Some(elapsed),
            unpaused_tasks: 0,
            unpaused_blocking_tasks: 0,
        };
        let Some(CallResult::Pending(token)) = tasks
            .call(
                (Native::ViewGXA, CallKind::Task),
                &args,
                step(0),
                &mut None,
                &mut None,
                NativeServices {
                    gxa_overlay: Some(&mut overlay),
                    ..Default::default()
                },
            )
            .unwrap()
        else {
            panic!("ViewGXA must allocate a continuation")
        };
        let shown = Some(GxaOverlay {
            name: "system\\stmap01.gxa".into(),
            frame: 2,
            second_offset: None,
        });
        assert_eq!(overlay, shown);
        for (elapsed, done) in [(1, false), (2, true)] {
            let completed = tasks
                .poll(
                    token,
                    &args,
                    step(elapsed),
                    &mut None,
                    &mut None,
                    NativeServices {
                        gxa_overlay: Some(&mut overlay),
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(completed, done);
            assert_eq!(overlay, if done { None } else { shown.clone() });
        }
    }
}

#[cfg(test)]
mod animate_gxa_tests {
    use super::*;

    fn run(
        tasks: &mut NativeTasks,
        token: Option<u64>,
        args: &[Argument],
        elapsed: u32,
        overlay: &mut Option<GxaOverlay>,
        slot: &mut (u32, i32),
        ticks: u32,
    ) -> anyhow::Result<(Option<u64>, bool)> {
        let invocation = Invocation {
            elapsed: Some(elapsed as i32),
            unpaused_tasks: 0,
            unpaused_blocking_tasks: 0,
        };
        let services = NativeServices {
            gxa_overlay: Some(overlay),
            gxa_animation: Some(GxaAnimation {
                resource: &mut slot.0,
                progress: &mut slot.1,
                ticks,
            }),
            ..Default::default()
        };
        match token {
            None => match tasks.call(
                (Native::AnimateGXA, CallKind::Task),
                args,
                invocation,
                &mut None,
                &mut None,
                services,
            )? {
                Some(CallResult::Pending(token)) => Ok((Some(token), false)),
                _ => Ok((None, true)),
            },
            Some(token) => Ok((
                Some(token),
                tasks.poll(token, args, invocation, &mut None, &mut None, services)?,
            )),
        }
    }

    #[test]
    fn animate_gxa_cycles_its_second_slot_and_clears_both_when_the_count_elapses() {
        let mut tasks = NativeTasks::default();
        let (mut overlay, mut slot) = (None, (0, 99));
        let args = [
            Argument::String("system\\stmap01.gxa".into()),
            Argument::Integer(4),
            Argument::Integer(5),
            Argument::Integer(5),
        ];
        // The first invocation restarts the shared progress, then shows frame 4 with the second
        // slot at progress 0 plus one; the update adds the three ticks.
        let (token, done) = run(&mut tasks, None, &args, 0, &mut overlay, &mut slot, 3).unwrap();
        assert!(!done);
        let shown = |offset| {
            Some(GxaOverlay {
                name: "system\\stmap01.gxa".into(),
                frame: 4,
                second_offset: Some(offset),
            })
        };
        assert_eq!(overlay, shown(1));
        assert_eq!(slot, (1, 3), "the load marks the image resident");
        // Progress 3 plus 3 ticks is 6, above the cycle of 5, so it wraps to 1.
        let (_, done) = run(&mut tasks, token, &args, 1, &mut overlay, &mut slot, 3).unwrap();
        assert!(!done);
        assert_eq!(
            overlay,
            shown(4),
            "the slot used the progress before the update"
        );
        assert_eq!(slot.1, 1);
        // Equality with the cycle is kept: 1 + 4 is 5, not above 5.
        let (_, done) = run(&mut tasks, token, &args, 2, &mut overlay, &mut slot, 4).unwrap();
        assert!(!done);
        assert_eq!(slot.1, 5);
        // At counter count - 1 the image is released: both slots clear and the task completes.
        let (_, done) = run(&mut tasks, token, &args, 4, &mut overlay, &mut slot, 3).unwrap();
        assert!(done);
        assert_eq!(overlay, None);
        assert_eq!(
            slot,
            (0, 5),
            "release unloads the image and leaves the progress alone"
        );
    }

    #[test]
    fn animate_gxa_rejects_a_cycle_that_would_never_end_and_a_missing_owner() {
        let mut tasks = NativeTasks::default();
        let (mut overlay, mut slot) = (None, (0, 0));
        let bad = [
            Argument::String("system\\stmap01.gxa".into()),
            Argument::Integer(0),
            Argument::Integer(0),
            Argument::Integer(9),
        ];
        let error = run(&mut tasks, None, &bad, 0, &mut overlay, &mut slot, 1).unwrap_err();
        assert!(
            format!("{error:#}").contains("cycle 0 must be positive"),
            "{error:#}"
        );
        let error = tasks
            .call(
                (Native::AnimateGXA, CallKind::Task),
                &bad,
                Invocation {
                    elapsed: Some(0),
                    unpaused_tasks: 0,
                    unpaused_blocking_tasks: 0,
                },
                &mut None,
                &mut None,
                NativeServices::default(),
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("AnimateGXA overlay owner unavailable"));
        // A later invocation whose image was never loaded would dereference nothing.
        let good = [
            bad[0].clone(),
            Argument::Integer(0),
            Argument::Integer(4),
            Argument::Integer(9),
        ];
        let (token, _) = run(&mut tasks, None, &good, 0, &mut overlay, &mut slot, 1).unwrap();
        slot.0 = 0;
        let error = run(&mut tasks, token, &good, 3, &mut overlay, &mut slot, 1).unwrap_err();
        assert!(format!("{error:#}").contains("before its image was loaded"));
    }
}
