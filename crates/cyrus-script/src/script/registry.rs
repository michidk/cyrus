//! Logical script instances retain authored object order and reject stale world identities.
use super::locomotion::MotionDirection;
use crate::script::AiType;
use crate::script::CombatControlState;
use crate::script::CombatFlags;
use crate::script::JumpMotion;
use crate::script::LivingState;
use crate::script::locomotion::StepDirection;
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::TauntType;
use cyrus_assets::surface::Surface;
use cyrus_soup::Native;
use cyrus_soup::program::{Call, CallKind, Operation};
use cyrus_soup::{AnimationGroup, Attribute, Attributes, Property};
use cyrus_world::heading::wrap;
mod activation;
mod actor_bounce;
mod ai_progress;
mod attack;
mod attack_input;
mod camera;
mod camera_drive;
pub use camera_drive::{CameraDrive, CameraFrameState, PublishedCamera};
mod death_exit;
mod effects;
mod health;
pub use death_exit::WorldLoopOutcome;
pub use effects::{ActorEvent, EnvironmentalEffect, EnvironmentalEffects};
pub use health::{SelectedDeathState, SelectedHealthState};
pub(super) mod avoid;
pub(super) mod body;
mod bounce;
pub(super) mod bounds;
mod combat_decision;
mod contact;
mod coordination;
mod counter;
mod defence;
mod dialogue;
mod direct_point;
mod dragon;
pub(super) mod falling;
mod flat;
pub use flat::ActorFlatDraw;
mod follow;
pub(super) mod ground;
pub use ground::GroundKind;
pub(super) mod hazard;
mod item_use;
mod late;
mod leap;
mod ledge;
mod ledge_states;
pub use ledge_states::LedgeTiming;
mod light;
mod map_marker;
pub use map_marker::SavedPlacement;
mod lineup;
mod linked;
pub(super) mod model;
pub(super) mod obstacle;
pub(super) mod ordinary;
pub(super) mod overlap;
mod persistence;
mod point;
mod point_tasks;
mod prelight;
pub use prelight::ActorPrelight;
mod probe;
mod projectile;
mod quick_keys;
mod rope_host;
mod terrain_sink;
mod torch;
pub use persistence::{
    LogicalSessionCandidate, LogicalSessionRestoreInputs, LogicalSessionRestorePlan,
    LogicalSessionSnapshot, WorldRestoreContext, WorldSnapshot, WorldSnapshotSources,
};
pub(super) mod player;
pub(super) mod prevalidation;
pub(super) mod queued;
pub(super) mod recovery;
mod separate;
pub(super) mod sight;
pub(super) mod slope;
pub(super) mod spacing;
pub(super) mod transition;
pub(super) mod vertical;
mod weapon;
use anyhow::{Context, Result};
use bevy::prelude::{Assets, Commands, DetectChanges, Handle, Res, ResMut, Resource};
pub use coordination::{OrdinaryCombatBranch, OrdinaryCombatCoordinationInput};
use cyrus_assets::{
    animation::Animations,
    script::{CombatTaunt, Instance, Scripts},
};
use cyrus_soup::{Argument, CallResult, CodeOrigin, Host, Route, Runtime, Target, Thread};
use cyrus_world::lifecycle::PreparedWorld;
use std::collections::BTreeMap;
use std::sync::Arc;
pub use weapon::{CombatEntryInputs, WeaponInputs};

/// Runtime identity is independent of render nodes and original object-name lookup.
pub use cyrus_soup::ActorReference as ScriptActorId;

/// Index of the selected player: CYRUS, or GREMLIN while the swap state is set. The standalone player
/// roles are the only candidates, so a world without the selected role has no selected player.
pub(super) fn selected_player_index(
    actors: &[ScriptActor],
    globals: &super::GameplayGlobals,
) -> Option<usize> {
    actors.iter().position(|actor| match actor.source {
        ScriptActorSource::Player => !globals.player_swapped(),
        ScriptActorSource::AlternatePlayer => globals.player_swapped(),
        _ => false,
    })
}

/// Re-assert the original ordinary-update exclusion for the two standalone player roles.
/// The selected role follows the retained swap bit; the other role remains allocated but does not
/// enter ordinary actor updates. This is repeated at every world-loop admission because scripts
/// may change control flags while a swap is retained.
pub(super) fn reassert_player_update_exclusion(
    actors: &mut [ScriptActor],
    globals: &super::GameplayGlobals,
) {
    for actor in actors {
        let excluded = match &actor.source {
            ScriptActorSource::Player => globals.player_swapped(),
            ScriptActorSource::AlternatePlayer => !globals.player_swapped(),
            _ => continue,
        };
        if let Some(control) = actor.control.as_mut() {
            control.object_update_excluded = excluded;
        }
    }
}

/// Player and camera instances are independent of authored map placements.
#[derive(Debug, Clone)]
pub enum ScriptActorSource {
    Map(Instance),
    Player,
    AlternatePlayer,
    Camera,
    /// A shell script (`DEBUG`, `COMMON` or the world's own): a script context without a map
    /// object, run each update on zeroed temporary attributes until `KillMe`.
    Shell(String),
    /// An object created by a native (`shoot`, `shootPlayer`, `DragonBreath`) from an installed
    /// script, with no map placement. It keeps its attributes and control state, runs a script
    /// update every iteration and ends at `KillMe`.
    Created(String),
}
impl ScriptActorSource {
    fn name(&self) -> &str {
        match self {
            Self::Map(placement) => &placement.actor,
            Self::Player => "CYRUS",
            Self::AlternatePlayer => "GREMLIN",
            Self::Camera => "CAMERA",
            Self::Shell(name) | Self::Created(name) => name,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Map(_) => "map",
            Self::Player => "player",
            Self::AlternatePlayer => "alternate_player",
            Self::Camera => "camera",
            Self::Shell(_) => "shell",
            Self::Created(_) => "created",
        }
    }
}

/// Explicit stages preserve separate animation, task and script-body invocations.
#[derive(Debug, Clone, Copy)]
pub enum ScriptStage {
    /// Caller has admitted the ordinary main stage; actor script-control flags still apply.
    OrdinaryMain,
    /// Separate-record main/task stage, without ordinary control/collision gates.
    SeparateMain,
    /// Separate-record native entry, gated only by pending unpaused tasks.
    SeparateHook,
    /// Caller has admitted this actor; task-system and hook-suppression gates still apply.
    OrdinaryHook,
    NativeHook,
    Animation,
    Entry,
    Tasks,
    Body(Thread),
}

/// An established active route-admission filter around the selected player's local position.
/// Distance uses map units. None at the call site means the shared exclusion switch is off.
#[derive(Debug, Clone, Copy)]
pub struct WalkRouteExclusion {
    pub selected_player: ScriptActorId,
    pub minimum_distance: i32,
}

/// Receiver-bound geometry retained by the ordinary wrapper for native locomotion.
/// Routed calls must supply a matching prepared query; candidate rebinding is unsupported.
#[derive(Clone, Copy)]
pub struct LocomotionGeometry<'a> {
    pub ground: ground::OrdinaryGroundQuery<'a>,
    pub environment: transition::ObstacleEnvironment,
}
impl std::fmt::Debug for LocomotionGeometry<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocomotionGeometry")
            .field("environment", &self.environment)
            .finish_non_exhaustive()
    }
}

/// Controls, clock and settings for one `menuProc` step: up and down are the first two bindings
/// (forward and backward) and select is the fourth action binding. Elapsed BIOS ticks shorten the
/// repeat delay only in updates that advance at least one nominal tick.
fn menu_input(
    inputs: &ScriptInputs<'_>,
    dialogue: Option<&super::DialogueState>,
) -> Option<super::menu::MenuInput> {
    let held = inputs.player_controller?.held;
    Some(super::menu::MenuInput {
        up: held.forward,
        down: held.backward,
        select: held.action,
        tick: inputs.combat_clock_sample?,
        elapsed_delay: inputs.nominal_ticks? != 0,
        traverse_delay: i32::try_from(dialogue?.settings().menu_traverse_delay).ok()?,
    })
}

/// Borrowed installed animation data; opaque in debug output because the asset is large.
#[derive(Clone, Copy)]
pub struct AnimationData<'a>(pub &'a Animations);
impl std::fmt::Debug for AnimationData<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AnimationData")
    }
}

/// Inputs supplied by the owning scheduler; missing values never imply a clock tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptInputs<'a> {
    pub hud: Option<&'a super::ScriptHud>,
    /// Caller-established shell context for item operations; absence is unknown.
    pub shell_execution: Option<bool>,
    /// Shared drevil state; absence remains unknown when fatal health needs it.
    pub death_suppressed: Option<bool>,
    /// Shared goldfinger state: the selected player's burning torch is held at a full counter.
    /// Absence is the cheat being off.
    pub torch_retained: Option<bool>,
    /// Shared drno state: the ordinary main stage is skipped for every actor except the selected
    /// player and the camera actor. Absence is the cheat being off.
    pub actor_update_skipped: Option<bool>,
    /// Shared `task` state: whether the task scheduling system runs. Absence keeps the installed
    /// `SYSTEM.INI` switch.
    pub task_system: Option<bool>,
    /// Shared `animation` state tested for the value one: the movement natives wait for their
    /// requested group. Absence keeps the installed `SYSTEM.INI` switch.
    pub animation_system: Option<bool>,
    /// Shared `animation` state tested for nonzero: whether the animation system runs. Absence
    /// follows `animation_system`.
    pub animation_running: Option<bool>,
    /// Shared yeahbaby state: the selected player's ground-support probe is bypassed while a
    /// vertical debug key is held or the Home toggle is set. Absence is the cheat being off.
    pub ground_bypass: Option<bool>,
    /// Shared magiccarpet state (the value 1): the selected player's probe clears its falling
    /// byte and ends. Absence is the cheat being off.
    pub magic_carpet: Option<bool>,
    /// Shared moonraker state: the selected player skips the forward obstacle probes and the actor
    /// overlap pass of the late collision stage. Absence is the cheat being off.
    pub player_collision_skipped: Option<bool>,
    pub activation: Option<&'a super::ActivationInputs>,
    /// Held installed Activate binding for live logical-actor activation.
    pub activation_use_held: Option<bool>,
    /// Quick sword and quick health keys for the ordinary update's key handler.
    pub quick_keys: Option<super::QuickKeys>,
    /// Held script keys for `InKey` and `SpacePressed`; absence is unknown.
    pub script_keys: Option<super::ScriptKeys>,
    /// Installed animation data for natives that read model vertices; the ordinary update supplies it.
    pub animations: Option<AnimationData<'a>>,
    /// Raw current BIOS-clock sample for combat pacing, independent of nominal ticks.
    pub combat_clock_sample: Option<u32>,
    pub nominal_ticks: Option<u32>,
    pub frame_scale: Option<u32>,
    /// Whole finer-counter increments and the measured FPS of this update's clock step.
    pub fine_ticks: Option<u32>,
    pub step_fps: Option<u32>,
    pub player_query: Option<super::QueryInputs>,
    /// Explicit selected-player keyboard admission for the ordinary traversal.
    pub player_controller: Option<player::PlayerControllerInput<'a>>,
    pub ambient_sound: Option<&'a dyn cyrus_world::sound::AmbientSoundDriver>,
    /// Caller-admitted weapon and taunt gates for synchronous beginCombat.
    pub combat_entry: Option<CombatEntryInputs>,
    pub combat_taunts: Option<&'a dyn super::CombatTauntSource>,
    pub dialogue_source: Option<&'a dyn super::DialogueSource>,
    /// Explicit shared sample-reset flag from the owning engine update.
    pub sound_reset: Option<bool>,
    /// Explicit inspection input for MoveBackward combat stance without live geometry.
    pub blocked_ahead_30: Option<bool>,
    /// Explicit inspection input for MoveForward without live geometry.
    pub forward_step_clear: Option<bool>,
    /// Explicit inspection input for an ordinary MoveLeft or MoveRight without live geometry.
    pub lateral_step_clear: Option<bool>,
    /// Live receiver-bound queries take precedence over caller-sampled checks.
    pub locomotion_geometry: Option<LocomotionGeometry<'a>>,
    /// The camera pose update the location natives run in their first invocation.
    pub camera_update: Option<super::camera::CameraUpdateHandle<'a>>,
    /// What preparing collision geometry bound to the selected player needs, for natives that
    /// move the player from another actor's script.
    pub player_collision: Option<PlayerCollisionSource<'a>>,
}

/// Matching world geometry and traversal from which a native prepares the selected player's own
/// collision query.
#[derive(Clone, Copy)]
pub struct PlayerCollisionSource<'a> {
    pub scene: &'a crate::player::data::PlayerScene,
    pub animations: &'a cyrus_assets::animation::Animations,
    pub view: [i32; 2],
    pub actors: &'a [ScriptActorId],
}
impl std::fmt::Debug for PlayerCollisionSource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerCollisionSource")
            .field("view", &self.view)
            .finish_non_exhaustive()
    }
}

pub(crate) struct ScriptAnimation {
    pub(crate) playback: cyrus_world::animation::ActorAnimation,
    pub(crate) frame: Option<u32>,
}

/// Allocated ordinary-map and player control state, separate from animation playback.
/// Allocation and combat construction follow preservation's engine/animation.md.
/// Player role transitions and death/removal transitions remain unsupported.
/// The actor's directional action words: the DEF `action_flag` (pending) and `action` (accepted)
/// properties. `SetAction` sets pending bits; a late update promotes them into the accepted word
/// (preservation `docs/engine/SOUPPROPS.md`). Both keep all 32 bits a script writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct ActionWords {
    pub pending: u32,
    pub accepted: u32,
}

impl ActionWords {
    /// DEF `action_jump_right`: accepted, it selects the running jump motion.
    pub const JUMP_RIGHT: u32 = 1;
    /// DEF `action_jump_left`: accepted, it selects the left-lead running jump motion.
    pub const JUMP_LEFT: u32 = 2;
    /// Both directional jump bits.
    pub const JUMPS: u32 = Self::JUMP_RIGHT | Self::JUMP_LEFT;

    /// Both words, pending first.
    pub fn iter(&self) -> impl Iterator<Item = u32> {
        [self.pending, self.accepted].into_iter()
    }

    /// Both words, pending first.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut u32> {
        [&mut self.pending, &mut self.accepted].into_iter()
    }
}

impl From<[u32; 2]> for ActionWords {
    fn from([pending, accepted]: [u32; 2]) -> Self {
        Self { pending, accepted }
    }
}

impl PartialEq<[u32; 2]> for ActionWords {
    fn eq(&self, words: &[u32; 2]) -> bool {
        <[u32; 2]>::from(*self) == *words
    }
}

impl From<ActionWords> for [u32; 2] {
    fn from(words: ActionWords) -> Self {
        [words.pending, words.accepted]
    }
}

/// The receiver's AI scratch fields scripts read and write by property: `ai_var1` (the signed
/// location word `GotoLocation` stores), `ai_var2` and `ai_length`. Allocation zeroes the record and
/// then sets the first variable to minus one; reads of the word fields sign-extend them. The node
/// fields (`node`, `dnode`, `fdnode`) belong to [`super::ActorNavigation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiRecord {
    /// `ai_var1`: the location word `GotoLocation` retains independently of AI mode and delay.
    pub var1: i16,
    pub var2: i16,
    pub length: i32,
}
impl Default for AiRecord {
    fn default() -> Self {
        Self {
            var1: -1,
            var2: 0,
            length: 0,
        }
    }
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ActorControlState {
    pub navigation: super::ActorNavigation,
    pub point_command: super::PointCommandState,
    /// Runtime script flags, separate from installed attribute bytes.
    pub script_control: u8,
    /// The DEF `ai_type` lifecycle selector.
    pub ai_type: super::AiType,
    /// The DEF `ai_mode` byte, independent of the `ai_type` lifecycle selector.
    pub ai_mode: u8,
    /// Receiver-owned unsigned AI delay, retained independently of mode and ordinary countdown.
    pub ai_delay: u8,
    /// The AI scratch fields scripts read and write by property.
    pub ai: AiRecord,
    /// Motion/weapon state, independent of the combat/death lifecycle selector.
    pub living_state: LivingState,
    /// Receiver-owned ordinary countdown; zero and 255 do not decrement.
    pub extra_time: u8,
    /// The `surface_time`, `jump_time`, `jump_height` and `anchor` bytes: no engine routine reads
    /// or writes them, so each only retains what a script stores (allocator zero otherwise).
    pub surface_time: u8,
    pub jump_time: u8,
    pub jump_height: u8,
    pub anchor: u8,
    /// Allocator-zeroed runtime motion bytes, independent of animation groups.
    pub sliding_state: u8,
    pub falling_state: JumpMotion,
    pub queued_motion: JumpMotion,
    pub motion_direction: MotionDirection,
    /// Retained movement-helper classification, distinct from displacement direction.
    pub motion_classification: MotionDirection,
    /// The pending (`action_flag`) and accepted (`action`) directional action words.
    pub actions: ActionWords,
    /// Allocator-zero action-transition counter; state-17 reset assigns six.
    pub action_transition_counter: u8,
    /// Ticks since the directional-state pending byte began, promoting it after 36.
    pub jump_window: u8,
    /// The DEF `move_counter` byte: a two-bit counter per [`StepDirection`](super::locomotion::StepDirection).
    pub move_counter: u8,
    /// The `move` triple: retained integer motion components. A rope release stores the speed
    /// queued motion 24 turns into a horizontal move in the last one (the installed jump-off speed
    /// plus the node's measured speed, negative for a backward jump); falls and obstacle
    /// interruption clear them.
    pub motion_vector: [i32; 3],
    /// The `ov` vector: the held ledge point in Q8. The ledge entry stores the selected record's point
    /// here and its heading in the second word of `move`; the ledge states read both back.
    pub ledge_point: [i32; 3],
    /// The `hand_pos` triple: the retained attachment point in Q8 that the hand and hilt model
    /// helpers (the Display natives and the equipped-weapon placements) store.
    pub hand_point: [i32; 3],
    /// The `hand_angle` triple: the placed model's pitch, heading and roll, stored by the same
    /// helpers. Plain dwords: a script write is not masked.
    pub hand_angles: [i32; 3],
    /// Original fall origin in Q8 and byte countdown, allocated as zero.
    pub fall_origin_y: i32,
    pub fall_timer: u8,
    /// The `rope_wait` byte (+0x1b0), allocator-zero: a rope release sets it to 24, ordinary updates
    /// count it down and the rope grab skips the released rope while it is nonzero. The weapon
    /// routines retain it.
    pub jump_selector: u8,
    /// Validity of the separately retained stable local position.
    pub stable_position_valid: bool,
    /// Separately retained stable local Q8 coordinates, allocated as zero.
    pub stable_local_position: [i32; 3],
    /// Accumulated sliding distance in Q8, cleared by ordinary jump entry.
    pub sliding_distance: i32,
    /// Collision-response marks (`last_collide`); the script wrapper clears them after its work.
    pub last_collide: super::CollisionFlags,
    /// Allocator-zero collision-response age (`last_collide_count`), advanced before the wrapper
    /// clears the marks.
    pub last_collide_count: u8,
    /// Original low-word overlap contact identifier; zero also denotes a static placement.
    pub overlap_contact_identifier: u16,
    /// Kind of the last ground-response record the selected player's ordinary support query chose.
    /// `PlayerOnScape` reads it.
    pub ground_kind: ground::GroundKind,
    pub rope_attached: bool,
    pub torch_burning: u8,
    /// The counter saved when a torch is put out, restored when it is lit again; zero starts afresh.
    pub torch_saved: u8,
    pub weapon_drawn: bool,
    /// Low byte assigned by admitted HandItem; None leaves initial attachment state unestablished.
    pub held_item: Option<u8>,
    pub object_update_excluded: bool,
    /// Presentation hidden state; this does not exclude script or animation updates.
    pub hidden: bool,
    /// The `invisible` byte: the body is not drawn while scripts, animation and collision go on.
    pub invisible: bool,
    /// The signed RAEX `scabbard0`/`scabbard1` words a script rewrote through `scabbard[0]` and
    /// `scabbard[1]`; None keeps the installed value the loader copied into the actor.
    pub scabbard_override: [Option<i16>; 2],
    /// The signed RAEX `grip0`/`grip1` words a script rewrote through `grip[0]` and `grip[1]`.
    pub grip_override: [Option<i16>; 2],
    /// The `yangle_change` word the model refresh stores: the heading change over the last
    /// ordinary update (the angle minus the update-start history, as a signed word).
    pub yangle_change: i16,
    /// Set by `KillMe`, cleared by `Resurrect`; independent of the death selector.
    pub killed: bool,
    /// Actor index of the object that created this one (`shoot`, `shootPlayer`, `DragonBreath`),
    /// the parent `projectileAI` measures range from and sends hits for.
    pub creator: Option<usize>,
    pub combat_death_default: Option<i32>,
    pub combat: Option<super::CombatControlState>,
}

pub struct ScriptActor {
    /// Allocator-zero class-message read cursor, independent of combat allocation.
    message_read: u8,
    /// Allocator-zero broadcast selection, independent of the class-message cursor.
    broadcast_index: u8,
    pub source: ScriptActorSource,
    /// Signed low word used by namespace lookup; absent for separate map object paths.
    pub runtime_identifier: Option<i16>,
    pub namespace: Option<String>,
    native_hook: Option<Call>,
    combat_taunts: Vec<CombatTaunt>,
    runtime: Option<Runtime>,
    /// Created-record cleanup completed; runtime may also be absent during an active call.
    created_retired: bool,
    unavailable: Option<String>,
    attributes: Attributes,
    control: Option<ActorControlState>,
    surface: Option<cyrus_assets::surface::Surface>,
    model: model::ModelState,
    collision: Option<ground::OrdinaryCollisionShape>,
    player_movement: Option<player::PlayerMovement>,
    light: Option<light::LightState>,
    flat: Option<flat::FlatState>,
    pub(super) location_offsets: Vec<[i32; 3]>,
    tasks: super::tasks::NativeTasks,
    native_commands: [super::tasks::NativeTaskCommandState; 6],
    /// Stored actor default while no playback allocation is prepared.
    unprepared_animation_default: i16,
    animation: Option<ScriptAnimation>,
    animation_failure: Option<String>,
    update_failure: Option<String>,
}
/// What starting a shell script needs from its installed definition: the shared program, its
/// code origin and a copy of the initial local variables.
struct ShellTemplate {
    program: Arc<cyrus_soup::program::Program>,
    code_origin: u32,
    locals: Vec<i32>,
    namespace: Option<String>,
    native_hook: Option<Call>,
    combat_taunts: Vec<CombatTaunt>,
    location_offsets: Vec<[i32; 3]>,
    /// Installed attribute defaults, as an ordinary actor of this script starts with.
    attributes: Attributes,
}

impl ScriptActor {
    /// An object created after preparation: attributes from the installed defaults, control state
    /// and surface allocated as for an ordinary actor, the creator recorded by the caller.
    fn created(world: u32, index: usize, name: &str, template: &ShellTemplate) -> Self {
        let mut actor = Self::shell(world, index, name, Some(template));
        actor.source = ScriptActorSource::Created(name.to_owned());
        // Post-load objects use standalone records. The original leaves their runtime
        // identifier at zero instead of allocating an actor-pool slot.
        actor.runtime_identifier = Some(0);
        actor.attributes = template.attributes.clone();
        actor.control = Some(ActorControlState::default());
        actor.surface = Some(Surface::UNKNOWN);
        actor
    }

    /// A shell actor: no map object, zeroed temporary attributes and no persistent control.
    fn shell(world: u32, index: usize, name: &str, template: Option<&ShellTemplate>) -> Self {
        let context = format!("world {world} instance {index} actor {name} script");
        let (runtime, unavailable) = match template {
            Some(template) => match Runtime::from_shared(
                Arc::clone(&template.program),
                CodeOrigin(template.code_origin),
                template.locals.clone(),
                context.clone(),
            ) {
                Ok(runtime) => (Some(runtime), None),
                Err(error) => (None, Some(format!("{context}: {error:#}"))),
            },
            None => (None, Some(format!("{context}: program absent"))),
        };
        Self {
            message_read: 0,
            broadcast_index: 0,
            source: ScriptActorSource::Shell(name.to_owned()),
            runtime_identifier: None,
            namespace: template.and_then(|template| template.namespace.clone()),
            native_hook: template.and_then(|template| template.native_hook.clone()),
            combat_taunts: template
                .map(|template| template.combat_taunts.clone())
                .unwrap_or_default(),
            runtime,
            created_retired: false,
            unavailable,
            attributes: Attributes::default(),
            control: None,
            surface: None,
            model: Default::default(),
            collision: None,
            player_movement: None,
            light: None,
            flat: None,
            location_offsets: template
                .map(|template| template.location_offsets.clone())
                .unwrap_or_default(),
            tasks: Default::default(),
            native_commands: Default::default(),
            unprepared_animation_default: 0,
            animation: None,
            animation_failure: None,
            update_failure: None,
        }
    }

    pub fn name(&self) -> &str {
        self.source.name()
    }

    /// Mirror the falling byte into the animation, whose mask-4 `Transition` commands read it.
    /// Call after every write to the byte and before an ordinary update steps the animation.
    fn sync_falling_motion(&mut self) {
        let active = self
            .control
            .as_ref()
            .is_some_and(|control| control.falling_state != JumpMotion::NONE);
        if let Some(animation) = self.animation.as_mut() {
            animation.playback.set_motion_active(active);
        }
    }
    fn ordinary_collision_role(&self) -> bool {
        matches!(
            self.source,
            ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
        ) || self
            .placement()
            .is_some_and(|placement| placement.object_type == PlacementType::MODEL)
    }

    pub fn placement(&self) -> Option<&Instance> {
        match &self.source {
            ScriptActorSource::Map(placement) => Some(placement),
            _ => None,
        }
    }
    pub fn runtime(&self) -> Option<&Runtime> {
        self.runtime.as_ref()
    }
    /// Prepared attribute bytes, including immediate script writes and backup copies.
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }
    /// Retained ordinary-map/player allocation state; separate records have no persistent control.
    pub fn control_state(&self) -> Option<&ActorControlState> {
        self.control.as_ref()
    }
    /// The actor this one stands on, as its ground or landing response left it.
    pub fn support_actor(&self) -> Option<ScriptActorId> {
        self.model.support.actor
    }
    pub(super) fn bound_animation_group(&self) -> Option<Option<AnimationGroup>> {
        self.animation
            .as_ref()
            .map(|animation| animation.playback.current_group())
    }
    /// Sticky script or animation failure; failed effects are never retried.
    pub fn failure(&self) -> Option<&str> {
        self.update_failure
            .as_deref()
            .or_else(|| self.animation_failure())
            .or_else(|| self.runtime.as_ref().and_then(Runtime::failure))
    }
    /// Retained animation construction or advancement failure, separate from SOUP errors.
    pub fn animation_failure(&self) -> Option<&str> {
        self.animation_failure.as_deref().or_else(|| {
            self.animation
                .as_ref()
                .and_then(|animation| animation.playback.failure())
        })
    }
    /// Whether a caller has initialized or explicitly bound animation playback.
    pub fn animation_initialized(&self) -> bool {
        self.animation.is_some()
    }
    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
}

/// Logical world state shared by inspection and camera-centered ordinary updates.
pub struct ScriptWorld {
    pub world: u32,
    pub content_id: String,
    generation: u64,
    flags: Vec<i32>,
    flag_state: super::flags::Flags,
    updates: cyrus_assets::script::UpdateSettings,
    /// The `animation` state's value-one test, which the movement natives' wait on the requested
    /// group uses; `None` follows `updates.animation_system` (the nonzero test).
    animation_strict: Option<bool>,
    player_distance_override: bool,
    actors: Vec<ScriptActor>,
    /// Installed script definitions a shell can be started from after preparation.
    shell_templates: BTreeMap<String, ShellTemplate>,
    spatial: Option<super::spatial::SpatialWorld>,
    activation: super::ActivationState,
    globals: super::GameplayGlobals,
    task_calls: BTreeMap<(CodeOrigin, u32), Call>,
    /// While a call routed to another actor runs, the actor that made it: the original's current
    /// script actor, which `Offset` attaches. Transient.
    routed_caller: Option<usize>,
    weapons: BTreeMap<u16, cyrus_assets::script::weapon::Weapon>,
    combat_attacks: Vec<cyrus_assets::script::combat::Attack>,
    combat_defenses: Vec<cyrus_assets::script::combat::Defense>,
    in_combat_threshold: Option<i32>,
    lineup_distance: Option<i32>,
    view_distance: Option<i32>,
    /// Letters whose key state word `PressKey` cleared and that have not been released since.
    /// Input state, not saved gameplay state.
    consumed_letters: u32,
    texture_frames: flat::TextureFrames,
    non_engaged_spacing: Option<i32>,
    combat_clank_effects: [Option<i32>; 2],
    node_timer: Option<i32>,
    references: Vec<Property>,
    names: BTreeMap<i16, usize>,
    actor_lists: Option<super::order::ActorLists>,
    markers: Option<super::markers::Markers>,
    walk_nodes: Option<super::navigation::WalkNodeReservations>,
    next_combat_index: i32,
    ordinary_prepared: bool,
    /// Ordinary updates this logical world has executed. Observation only: never saved, and
    /// zero again after a restore.
    ordinary_updates: u64,
    actor_prelighting: Option<BTreeMap<usize, ActorPrelight>>,
    /// The model candidate list of the latest ordinary candidate gather. The original keeps one
    /// global list, so a receiver that gathers none reads its predecessor's; transient, never saved.
    retained_candidates: cyrus_collision::candidates::CollisionCandidates,
    /// Frame transient models the hand and hilt display natives placed; emptied at the start of
    /// every ordinary update (the original's per-frame pool reset), never saved.
    display_attachments: Vec<super::attachment::DisplayAttachment>,
    model_admission: Option<model::AdmissionState>,
    map_updates: super::map_updates::MapUpdates,
    /// Shared surface-query veto; absent until an assigning query establishes it.
    surface_veto: Option<bool>,
    /// Water and lava transitions the selected player's forward probes noticed, in query order;
    /// the caller that owns the random stream and sound sink runs them right after the probe.
    pending_surface_transitions: Vec<PendingSurfaceTransition>,
    /// Whether the actor now updating was rolled back this update (the original's shared changed
    /// flag): the stable-position or water rollback restored its pose. Cleared as each ordinary
    /// update starts; never saved.
    rolled_back: bool,
    /// The session flag the Home key toggles for the yeahbaby ground bypass; it is read by the
    /// ground probe only while the cheat is on and is not reset by turning the cheat off.
    ground_bypass_toggle: bool,
    /// The state word of the `yeahbaby` Insert step: set when the camera limiter was suspended on
    /// the first held update, cleared when it is restored on release.
    insert_camera_flag: bool,
    /// Camera session owned by the CAMERA instance; absent when the world has no
    /// selected player or CAMERA actor.
    camera: Option<super::camera::CameraState>,
    /// Installed render-camera glide factors.
    camera_glide: super::camera::GlideSettings,
    /// Installed shared camera limits.
    camera_limits: cyrus_assets::script::CameraLimits,
    /// Installed `[camera]` values, kept to reload record zero on a camera reset.
    camera_settings: cyrus_assets::script::CameraSettings,
    /// Installed `[system]` `standing_height`, learned when collision shapes are prepared.
    standing_height: Option<i32>,
    /// The image the running `ViewGXA` task has the host draw over the world frame.
    gxa_overlay: Option<super::tasks::GxaOverlay>,
    /// `AnimateGXA`'s shared animation progress. Not saved: a restored world restarts the cycle.
    /// The selected player's placement raised by `MovePlayerAboveMe`, applied at the end of the
    /// frame loop.
    pending_player_placement: Option<([i32; 3], u16)>,
    effects: Option<EnvironmentalEffects>,
    /// Retained ropes of the installed `MPRP` records; rebuilt on preparation and restored from
    /// the world's [`WorldSnapshot`] when a session replacement is loaded.
    ropes: Vec<cyrus_game::rope::Rope>,
    /// The selected player's rope handling state; not saved.
    player_rope: rope_host::PlayerRope,
    /// Modern mouse camera: the heading the follow camera reads for the selected player in
    /// place of its own. Transient, never saved.
    follow_heading: Option<u16>,
}
impl ScriptWorld {
    /// Cancel native VM continuations and actor-owned task records before disposal.
    fn cancel_native_tasks(&mut self) -> Result<()> {
        for (index, actor) in self.actors.iter_mut().enumerate() {
            if let Some(runtime) = actor.runtime.as_mut() {
                runtime
                    .cancel_tasks_with(|token| actor.tasks.cancel(token))
                    .with_context(|| format!("world {} actor {index}", self.world))?;
            }
        }
        Ok(())
    }

    /// Index of the selected player (CYRUS, or GREMLIN while the swap state is set).
    pub(super) fn selected_player_index(&self) -> Option<usize> {
        selected_player_index(&self.actors, &self.globals)
    }

    /// The selected player: CYRUS, or GREMLIN while the swap state is set. Hosts bind their player
    /// entity, controller and camera to this actor rather than to a fixed role.
    /// Test hook: write one attribute byte of a logical actor.
    #[doc(hidden)]
    pub fn set_attribute_for_test(&mut self, id: ScriptActorId, attribute: Attribute, value: u8) {
        self.actors[id.index].attributes[attribute] = value;
    }

    /// Test hook: set the actor that owns the pending activation prompt.
    #[doc(hidden)]
    pub fn set_activation_owner_for_test(&mut self, owner: Option<ScriptActorId>) {
        self.activation.set_owner_for_test(owner);
    }

    /// Test hook: attach or release a logical actor on a rope without running the attach routine.
    #[doc(hidden)]
    pub fn set_rope_attached_for_test(&mut self, id: ScriptActorId, attached: bool) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .rope_attached = attached;
    }

    /// Whether the `yeahbaby` Insert step holds the camera limiter suspended.
    pub fn yeahbaby_insert_active(&self) -> bool {
        self.insert_camera_flag
    }

    /// Whether the Home toggle of the yeahbaby ground bypass is set.
    pub fn ground_bypass_toggle(&self) -> bool {
        self.ground_bypass_toggle
    }

    /// Test hook: set a logical actor's queued motion.
    #[doc(hidden)]
    pub fn set_queued_motion_for_test(&mut self, id: ScriptActorId, motion: JumpMotion) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .queued_motion = motion;
    }

    /// Test hook: set a logical actor's pending and accepted directional action words.
    #[doc(hidden)]
    pub fn set_action_words_for_test(&mut self, id: ScriptActorId, pending: u32, accepted: u32) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .actions = ActionWords { pending, accepted };
    }

    /// Test hook: give a logical actor a cached link position, as an `Offset` link does.
    #[doc(hidden)]
    pub fn set_link_position_for_test(&mut self, id: ScriptActorId, position: [i32; 3]) {
        self.spatial
            .as_mut()
            .expect("test spatial state")
            .actor_mut(id.index)
            .expect("test actor spatial state")
            .set_link_cache(position);
    }

    /// Test hook: give a logical actor a default combat record.
    #[doc(hidden)]
    pub fn set_combat_record_for_test(&mut self, id: ScriptActorId) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .combat = Some(CombatControlState::default());
    }

    /// Test hook: set a logical actor's collision-response word.
    #[doc(hidden)]
    pub fn set_last_collide_for_test(&mut self, id: ScriptActorId, bits: u32) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .last_collide = crate::script::CollisionFlags::from_bits(bits);
    }

    /// Test hook: set a logical actor's living state.
    #[doc(hidden)]
    pub fn set_living_state_for_test(&mut self, id: ScriptActorId, state: LivingState) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .living_state = state;
    }

    /// Test hook: run the bounce handler's pre-support part for one actor; true when the probe
    /// ends there.
    #[doc(hidden)]
    pub fn bounce_handler_prefix_for_test(&mut self, id: ScriptActorId) -> Result<bool> {
        self.bounce_handler_prefix(id)
    }

    /// Test hook: put a logical actor into a falling state with its retained fall origin (Q8 Y).
    #[doc(hidden)]
    pub fn set_falling_for_test(&mut self, id: ScriptActorId, state: JumpMotion, origin_y: i32) {
        let control = self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state");
        control.falling_state = state;
        control.fall_origin_y = origin_y;
    }

    pub fn selected_player(&self) -> Option<ScriptActorId> {
        self.id(self.selected_player_index()?).ok()
    }

    /// Console `item add`: one acquisition through the session inventory, as `AddItem` does. An
    /// empty-selection repair assigns the selected player's weapon attribute.
    pub fn add_player_item(&mut self, hud: &super::ScriptHud, item: u16) -> Result<()> {
        anyhow::ensure!(hud.inventory_ready()?, "item add: inventory unavailable");
        let player = self
            .selected_player()
            .context("item add: no selected player")?;
        let actors = &mut self.actors;
        hud.add_item(item, 1, &mut |weapon| {
            actors[player.index].attributes[Attribute::Weapon] = weapon;
            Ok(())
        })
    }

    /// Return the selected player's state for the engine-owned torch sound.
    /// The sound host supplies the positional mixer and remembers its channel;
    /// this method only exposes the authoritative logical actor state.
    pub fn selected_player_torch_sound(&self) -> Result<Option<(ScriptActorId, bool, [i32; 3])>> {
        let Some(id) = self.selected_player() else {
            return Ok(None);
        };
        let index = self.resolve(id)?;
        let burning = self.actors[index]
            .control
            .as_ref()
            .context("selected player torch state unavailable")?
            .torch_burning
            != 0;
        Ok(Some((id, burning, self.query_position(id)?)))
    }

    /// The image a running `ViewGXA` task has the host draw over the world frame, if any.
    pub fn gxa_overlay(&self) -> Option<&super::tasks::GxaOverlay> {
        self.gxa_overlay.as_ref()
    }

    /// Published camera inputs at this exact scheduler point, before render interpolation.
    pub fn sound_listener(&self) -> Option<cyrus_world::sound::SoundListener> {
        self.camera
            .as_ref()
            .map(|camera| cyrus_world::sound::SoundListener {
                position: camera.current.position,
                heading: cyrus_world::heading::wrap(camera.current.angles[1]) as u16,
            })
    }

    /// Retained camera session, when the world has both a player and a CAMERA instance.
    pub fn camera(&self) -> Option<&super::camera::CameraState> {
        self.camera.as_ref()
    }

    /// Resolve a retained zero-based actor attack slot against the shared installed table.
    /// Empty authored slots return `None`; missing initialization or definitions are errors.
    pub fn combat_attack(
        &self,
        id: ScriptActorId,
        slot: usize,
    ) -> Result<Option<&cyrus_assets::script::combat::Attack>> {
        let actor = &self.actors[self.resolve(id)?];
        let context = || format!("world {} actor {} attack slot {slot}", self.world, id.index);
        let combat = actor
            .control
            .as_ref()
            .and_then(|control| control.combat.as_ref())
            .with_context(|| format!("{}: combat uninitialized", context()))?;
        let index = combat
            .attack_indices
            .get(slot)
            .with_context(|| format!("{}: slot outside authored list", context()))?;
        index
            .map(|index| {
                self.combat_attacks
                    .get(usize::from(index))
                    .with_context(|| {
                        format!("{}: attack{index:02} definition unavailable", context())
                    })
            })
            .transpose()
    }

    /// Prepare a defence pick from this receiver's authored attack slot, not its
    /// separate four-entry defence table. Omitted first-motion inputs reject explicitly.
    pub fn combat_defence_candidate(
        &self,
        id: ScriptActorId,
        slot: usize,
    ) -> Result<Option<super::decision::DefenceCandidate>> {
        let Some(attack) = self.combat_attack(id, slot)? else {
            return Ok(None);
        };
        let context = || {
            format!(
                "world {} actor {} defence pick attack slot {slot}",
                self.world, id.index
            )
        };
        let motion = attack
            .motions
            .first()
            .with_context(|| format!("{}: first motion unavailable", context()))?;
        Ok(Some(super::decision::DefenceCandidate {
            min_range: motion.min_range,
            max_range: motion.max_range,
            near_timing: motion.first_collide_frame,
            counter_attack: motion
                .defense
                .with_context(|| format!("{}: counter-attack reference unavailable", context()))?,
        }))
    }

    /// Send an addressed combat message using the receiver's authored class.
    /// A zero code has no effect; nonzero messages require live logical identities.
    pub fn send_combat_message(
        &mut self,
        sender: ScriptActorId,
        receiver: ScriptActorId,
        code: super::CombatMessageCode,
    ) -> Result<()> {
        if code == super::CombatMessageCode::NONE {
            return Ok(());
        }
        self.resolve(sender)?;
        let index = self.resolve(receiver)?;
        self.globals.combat_messages.send(
            self.actors[index].attributes[Attribute::MessGroup],
            super::CombatMessage {
                sender,
                receiver: Some(receiver),
                code,
            },
        );
        Ok(())
    }

    /// `SendBroadcast`: append an entry with no recipient to channel zero, the queue every
    /// receiver's broadcast selection reads. A zero value has no effect.
    pub fn send_broadcast(
        &mut self,
        sender: ScriptActorId,
        code: super::CombatMessageCode,
    ) -> Result<()> {
        if code == super::CombatMessageCode::NONE {
            return Ok(());
        }
        self.resolve(sender)?;
        self.globals.combat_messages.send(
            0,
            super::CombatMessage {
                sender,
                receiver: None,
                code,
            },
        );
        Ok(())
    }

    /// Peek one addressed message without changing its retained read cursor.
    pub fn peek_combat_message(
        &self,
        actor: ScriptActorId,
    ) -> Result<Option<super::CombatMessage>> {
        let index = self.resolve(actor)?;
        let receiver = &self.actors[index];
        self.globals.combat_messages.peek(
            receiver.attributes[Attribute::MessGroup],
            actor,
            receiver.message_read,
        )
    }

    /// The entry the actor's retained class cursor selects; see
    /// [`super::CombatMessages::selected_entry`].
    pub fn selected_message_entry(
        &self,
        actor: ScriptActorId,
    ) -> Result<Option<Option<ScriptActorId>>> {
        let index = self.resolve(actor)?;
        let receiver = &self.actors[index];
        self.globals.combat_messages.selected_entry(
            receiver.attributes[Attribute::MessGroup],
            receiver.message_read,
        )
    }

    /// The sender of the message the actor's retained class cursor selects, if any.
    pub fn selected_message_sender(&self, actor: ScriptActorId) -> Result<Option<ScriptActorId>> {
        Ok(self.selected_message_entry(actor)?.flatten())
    }

    /// The broadcast entry the actor's retained selection picks in channel zero. The outer `None`
    /// is no selection; the inner `None` is a selected slot never written (null sender). The
    /// reference is returned as stored, not checked against the live world.
    pub fn selected_broadcast_entry(
        &self,
        actor: ScriptActorId,
    ) -> Result<Option<Option<ScriptActorId>>> {
        let index = self.resolve(actor)?;
        self.globals
            .combat_messages
            .selected_entry(0, self.actors[index].broadcast_index)
    }

    /// The sender of the selected broadcast entry, if any.
    pub fn selected_broadcast_sender(&self, actor: ScriptActorId) -> Result<Option<ScriptActorId>> {
        Ok(self.selected_broadcast_entry(actor)?.flatten())
    }

    /// `ReplySender(value)`: unless the value is zero, nothing is selected, or the selected sender
    /// is null or the receiver itself, write one entry (sender the receiver, recipient the
    /// selected message's sender, the value) on the receiver's own class channel. The receiver's
    /// selection is unchanged.
    pub fn reply_to_message_sender(&mut self, actor: ScriptActorId, value: i32) -> Result<()> {
        if value == 0 {
            return Ok(());
        }
        let Some(Some(sender)) = self.selected_message_entry(actor)? else {
            return Ok(());
        };
        if sender == actor {
            return Ok(());
        }
        let class = self.actors[self.resolve(actor)?].attributes[Attribute::MessGroup];
        self.globals.combat_messages.send(
            class,
            super::CombatMessage {
                sender: actor,
                receiver: Some(sender),
                code: value.into(),
            },
        );
        Ok(())
    }

    /// `ReplyBroadcastSender(value)`: unless the value is zero, no broadcast is selected, or the
    /// selected sender is null or the receiver itself, write one entry on channel zero (sender the
    /// receiver, recipient the selected broadcast's sender, the value). The receiver's own
    /// broadcast selection is unchanged.
    pub fn reply_to_broadcast_sender(&mut self, actor: ScriptActorId, value: i32) -> Result<()> {
        if value == 0 {
            return Ok(());
        }
        let Some(Some(sender)) = self.selected_broadcast_entry(actor)? else {
            return Ok(());
        };
        if sender == actor {
            return Ok(());
        }
        self.globals.combat_messages.send(
            0,
            super::CombatMessage {
                sender: actor,
                receiver: Some(sender),
                code: value.into(),
            },
        );
        Ok(())
    }

    /// `ReceiveMessage`: see [`super::CombatMessages::receive`].
    pub fn receive_message(&mut self, actor: ScriptActorId) -> Result<i32> {
        let index = self.resolve(actor)?;
        let class = self.actors[index].attributes[Attribute::MessGroup];
        let receiver = &mut self.actors[index];
        self.globals
            .combat_messages
            .receive(class, actor, &mut receiver.message_read)
    }

    /// `ReceiveBroadcast`: the `ReceiveMessage` skip and admission rule over channel zero, driven by
    /// the actor's broadcast selection instead of its class-message cursor.
    pub fn receive_broadcast(&mut self, actor: ScriptActorId) -> Result<i32> {
        let index = self.resolve(actor)?;
        let receiver = &mut self.actors[index];
        self.globals
            .combat_messages
            .receive(0, actor, &mut receiver.broadcast_index)
    }

    /// `DeleteMessage`: move the actor's selection to the next entry of its class queue. With no
    /// selection (cursor at the write index) nothing changes. Otherwise the cursor increments and
    /// wraps to zero at the queue's entry count; no entry is released.
    pub fn advance_message_selection(&mut self, actor: ScriptActorId) -> Result<()> {
        let index = self.resolve(actor)?;
        let class = self.actors[index].attributes[Attribute::MessGroup];
        let read = self.actors[index].message_read;
        if self
            .globals
            .combat_messages
            .selected_entry(class, read)?
            .is_none()
        {
            return Ok(());
        }
        self.actors[index].message_read = super::CombatMessages::next_selection(read);
        Ok(())
    }

    /// Consume one addressed message using the actor's retained class cursor.
    /// This is message transport; combat effects belong to the maintenance stage.
    pub fn consume_combat_message(
        &mut self,
        actor: ScriptActorId,
    ) -> Result<Option<super::CombatMessage>> {
        let index = self.resolve(actor)?;
        let receiver = &mut self.actors[index];
        self.globals.combat_messages.consume(
            receiver.attributes[Attribute::MessGroup],
            actor,
            &mut receiver.message_read,
        )
    }

    /// Maintain an admitted ordinary combat record using shared nominal ticks.
    /// Perception and nonplayer decisions use live query owners and synchronous services.
    /// Damage, selected-player phases and remaining attack maintenance retain guards.
    /// The owner supplies the selected player's drawing/sheathing and shell gates,
    /// and the session random stream for routed point-facing delays.
    pub fn maintain_map_combat(
        &mut self,
        id: ScriptActorId,
        ticks: u32,
        player_weapon_transition: bool,
        inputs: ScriptInputs<'_>,
        mut random: Option<&mut cyrus_game::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            self.actors[index].placement().is_some() || self.selected_player_index() == Some(index),
            "ordinary combat requires a map actor or selected player"
        );
        let shell_execution = inputs
            .shell_execution
            .or_else(|| inputs.activation.map(|input| input.shell_execution))
            .unwrap_or(false);
        if shell_execution {
            return Ok(());
        }
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("ordinary combat control unavailable")?;
        if control.combat.is_none() {
            return Ok(());
        }
        let selected = self.selected_player_index() == Some(index);
        if selected
            && self
                .globals
                .selected_combat_inhibition
                .context("selected-player combat entry inhibition unavailable")?
                != 0
        {
            return Ok(());
        }
        if !selected && control.ai_type == AiType::DEAD {
            return Ok(());
        }
        if !selected
            && actor.attributes[Attribute::NoNodeCombat] == 0
            && control.combat.as_ref().is_some_and(|combat| {
                combat.flags.contains(CombatFlags::STANCE) && combat.target.is_some()
            })
        {
            let player = self
                .selected_player_index()
                .context("perception selected player unavailable")?;
            self.maintain_combat_perception(
                id,
                self.id(player)?,
                ticks,
                inputs,
                random.as_deref_mut(),
                sounds,
            )?;
        }
        self.actors[index].attributes[Attribute::Collide] =
            self.actors[index].attributes[Attribute::ReservedCollide];
        let selected_player = self.selected_player_index().map(|index| ScriptActorId {
            generation: self.generation,
            index,
        });
        self.process_combat_message_with_services(
            id,
            selected_player,
            inputs,
            random.as_deref_mut(),
            sounds,
        )?;
        if !selected
            && self.actors[index]
                .control
                .as_ref()
                .expect("admitted control")
                .ai_type
                == AiType::COMBAT
        {
            self.run_ordinary_combat_decision(
                id,
                selected_player.context("combat decision selected player unavailable")?,
                inputs,
                random
                    .as_deref_mut()
                    .context("combat decision random owner unavailable")?,
                sounds,
            )?;
        }
        if selected {
            let phase = self.actors[index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .map(|combat| combat.decision.attack_state)
                .expect("admitted combat");
            if phase.is_idle() {
                let combat = self.actors[index]
                    .control
                    .as_mut()
                    .and_then(|control| control.combat.as_mut())
                    .expect("admitted combat");
                if combat.action != CombatControlState::SINGLE_ACTION {
                    combat.action = CombatControlState::NO_ACTION;
                    combat.retained_action_indices = [-1; 2];
                }
                // Both maintenance helpers with action -1 are inert in phase zero.
            } else {
                // The defence executor, then the attack executor, both cleaning up.
                let clock = inputs
                    .combat_clock_sample
                    .context("selected combat BIOS-clock sample unavailable")?
                    as i32;
                let random = random
                    .as_deref_mut()
                    .context("selected combat random owner unavailable")?;
                if !self.execute_combat_defence(id, -1, clock, inputs, random, sounds)? {
                    self.execute_combat_attack(id, -1, inputs, sounds)?;
                }
            }
        }
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("admitted control");
        let combat = control.combat.as_mut().expect("admitted combat");
        if !player_weapon_transition {
            if combat.cooldown != 0 {
                combat.cooldown = combat.cooldown.saturating_sub(ticks);
            } else if combat.action != CombatControlState::NO_ACTION {
                if (0..=6).contains(&combat.action) {
                    if self.combat_attack_window(id)? {
                        self.actors[index]
                            .control
                            .as_mut()
                            .expect("admitted control")
                            .combat
                            .as_mut()
                            .expect("admitted combat")
                            .flags
                            .insert(CombatFlags::CONTACT_TESTED);
                        self.process_combat_attack_candidates(
                            id,
                            inputs,
                            random.context("attack candidate random owner unavailable")?,
                            sounds,
                        )?;
                    }
                } else {
                    combat.action = CombatControlState::NO_ACTION;
                }
            }
        }
        Ok(())
    }

    pub fn prepare(scripts: &Scripts, generation: u64) -> Result<Self> {
        scripts.validate()?;
        let programs: BTreeMap<_, _> = scripts
            .actors
            .iter()
            .map(|(name, actor)| (name.as_str(), Arc::new(actor.program.clone())))
            .collect();
        let mut task_calls = BTreeMap::new();
        for actor in scripts.actors.values() {
            for (&instruction, step) in &actor.program.steps {
                if let Operation::Call { call } | Operation::ObjectCall { call } = &step.operation {
                    let key = (CodeOrigin(actor.code_origin), instruction);
                    if let Some(existing) = task_calls.insert(key, call.clone()) {
                        anyhow::ensure!(
                            existing == *call,
                            "world {}: aliased script call differs at origin {} instruction {instruction}",
                            scripts.world,
                            actor.code_origin
                        );
                    }
                }
            }
        }
        let shell_templates: BTreeMap<String, ShellTemplate> = scripts
            .actors
            .iter()
            .filter_map(|(name, actor)| {
                Some((
                    name.clone(),
                    ShellTemplate {
                        program: Arc::clone(&programs[name.as_str()]),
                        code_origin: actor.code_origin,
                        locals: actor.instance_variables.first()?.clone(),
                        namespace: actor.namespace.clone(),
                        native_hook: actor.native_hook.clone(),
                        combat_taunts: actor.combat_taunts.clone(),
                        location_offsets: actor.location_offsets.clone(),
                        attributes: actor.attributes.clone(),
                    },
                ))
            })
            .collect();
        let mut allocations = BTreeMap::<String, usize>::new();
        let mut allocate = |source: ScriptActorSource,
                            runtime_identifier: Option<i16>,
                            index: usize| {
            let name = source.name();
            let context = format!(
                "world {} instance {index} actor {name} script",
                scripts.world
            );
            let metadata = scripts.actors.get(name);
            let result = (|| {
                let actor = metadata.context("program absent")?;
                let allocated = allocations.entry(name.to_owned()).or_default();
                let locals = actor
                    .instance_variables
                    .get(*allocated)
                    .context("authored instance allocation exceeded")?
                    .clone();
                *allocated += 1;
                Runtime::from_shared(
                    Arc::clone(&programs[name]),
                    CodeOrigin(actor.code_origin),
                    locals,
                    context.clone(),
                )
            })();
            let (runtime, unavailable) = match result {
                Ok(runtime) => (Some(runtime), None),
                Err(error) => (None, Some(format!("{context}: {error:#}"))),
            };
            let control = match &source {
                ScriptActorSource::Map(placement)
                    if !placement.object_type.has_separate_record() =>
                {
                    Some(ActorControlState::default())
                }
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer => {
                    Some(ActorControlState::default())
                }
                _ => None,
            };
            let separate = matches!(&source, ScriptActorSource::Map(placement) if placement.object_type.has_separate_record());
            let light = match &source {
                ScriptActorSource::Map(placement)
                    if placement.object_type == PlacementType::COLOURED_LIGHT =>
                {
                    Some(light::LightState::authored(placement))
                }
                // A type-four object gets the same record with the placement's intensity and
                // radius; its starting colour is unavailable to scripted dimming.
                ScriptActorSource::Map(placement)
                    if placement.object_type == PlacementType::LIGHT =>
                {
                    Some(light::LightState {
                        rgb: [0; 3],
                        ..light::LightState::authored(placement)
                    })
                }
                _ => None,
            };
            // A type-three placement gets an enabled flat record showing the placement's texture.
            let flat = match &source {
                ScriptActorSource::Map(placement)
                    if placement.object_type == PlacementType::FLAT =>
                {
                    Some(flat::FlatState::placed(placement.texture as u16))
                }
                _ => None,
            };
            ScriptActor {
                light,
                flat,
                message_read: 0,
                broadcast_index: 0,
                surface: (control.is_some()
                    || matches!(
                        &source,
                        ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                    ))
                .then_some(Surface::UNKNOWN),
                source,
                control,
                runtime_identifier,
                namespace: metadata.and_then(|actor| actor.namespace.clone()),
                native_hook: metadata.and_then(|actor| actor.native_hook.clone()),
                combat_taunts: metadata
                    .map(|actor| actor.combat_taunts.clone())
                    .unwrap_or_default(),
                runtime,
                created_retired: false,
                unavailable,
                location_offsets: metadata
                    .map(|actor| actor.location_offsets.clone())
                    .unwrap_or_default(),
                tasks: Default::default(),
                native_commands: Default::default(),
                unprepared_animation_default: 0,
                animation: None,
                model: Default::default(),
                collision: None,
                player_movement: None,
                animation_failure: None,
                update_failure: None,
                attributes: (!separate)
                    .then_some(metadata)
                    .flatten()
                    .map(|actor| actor.attributes.clone())
                    .unwrap_or_default(),
            }
        };
        let mut counter = scripts.actor_pool_capacity.wrapping_add(3);
        let mut actors = Vec::new();
        for (index, placement) in scripts.instances.iter().enumerate() {
            let identifier = if placement.object_type.has_separate_record() {
                None
            } else {
                if placement.active != 0 {
                    counter = counter.wrapping_add(1);
                }
                counter = counter.wrapping_add(1);
                Some(counter as i16)
            };
            actors.push(allocate(
                ScriptActorSource::Map(placement.clone()),
                identifier,
                index,
            ));
        }
        // Shell scripts exist before the standalone roles; the original prepends each record to its
        // list, so they run newest first (see `update_shells`).
        for name in &scripts.shells {
            let index = actors.len();
            actors.push(ScriptActor::shell(
                scripts.world,
                index,
                name,
                shell_templates.get(name),
            ));
        }
        for (name, source, identifier) in [
            ("CYRUS", ScriptActorSource::Player, 1),
            ("GREMLIN", ScriptActorSource::AlternatePlayer, 1),
            ("CAMERA", ScriptActorSource::Camera, 2),
        ] {
            if scripts.actors.contains_key(name) {
                let index = actors.len();
                actors.push(allocate(source, Some(identifier), index));
            }
        }
        let mut names = BTreeMap::new();
        for (index, actor) in actors.iter().enumerate() {
            if let Some(identifier) = actor
                .runtime_identifier
                .filter(|_| actor.namespace.is_some())
            {
                names.entry(identifier).or_insert(index);
            }
        }
        let mut globals = super::GameplayGlobals::default();
        globals.prepare_dialogue(&scripts.dialogue)?;
        globals.group_sync.register_actors(&actors);
        let actor_id = |source: fn(&ScriptActorSource) -> bool| {
            actors
                .iter()
                .position(|actor| source(&actor.source))
                .map(|index| ScriptActorId { generation, index })
        };
        let camera = actor_id(|source| matches!(source, ScriptActorSource::Player))
            .zip(actor_id(|source| {
                matches!(source, ScriptActorSource::Camera)
            }))
            .map(|(player, camera)| {
                super::camera::CameraState::new(
                    &scripts.camera,
                    player,
                    camera,
                    actors[player.index].attributes[Attribute::PlayerType],
                )
                .with_context(|| format!("world {} camera initialization", scripts.world))
            })
            .transpose()?;
        Ok(Self {
            map_updates: Default::default(),
            updates: scripts.updates,
            animation_strict: None,
            player_distance_override: scripts.player_distance_override,
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            generation,
            flag_state: super::flags::Flags::prepare(&scripts.definition)
                .with_context(|| format!("world {} global flag initialization", scripts.world))?,
            flags: scripts
                .definition
                .flags
                .iter()
                .map(|flag| flag.initial_value)
                .collect(),
            actors,
            shell_templates,
            task_calls,
            routed_caller: None,
            weapons: scripts.weapons.clone(),
            combat_attacks: scripts.combat_attacks.clone(),
            combat_defenses: scripts.combat_defenses.clone(),
            in_combat_threshold: scripts.in_combat_threshold,
            lineup_distance: scripts.lineup_distance,
            view_distance: scripts.view_distance,
            consumed_letters: 0,
            texture_frames: scripts.texture_frames.clone(),
            non_engaged_spacing: scripts.non_engaged_spacing,
            combat_clank_effects: scripts.combat_clank_effects,
            node_timer: scripts.node_timer,
            references: scripts.definition.references.clone(),
            names,
            actor_lists: None,
            markers: None,
            walk_nodes: None,
            next_combat_index: 0,
            ordinary_prepared: false,
            ordinary_updates: 0,
            actor_prelighting: None,
            retained_candidates: Default::default(),
            display_attachments: Vec::new(),
            surface_veto: None,
            pending_surface_transitions: Vec::new(),
            rolled_back: false,
            ground_bypass_toggle: false,
            insert_camera_flag: false,
            camera,
            camera_glide: scripts.camera.glide.into(),
            camera_limits: scripts.camera.limits,
            camera_settings: scripts.camera.clone(),
            standing_height: None,
            gxa_overlay: None,
            pending_player_placement: None,
            effects: None,
            ropes: Vec::new(),
            player_rope: Default::default(),
            follow_heading: None,
            model_admission: None,
            spatial: None,
            activation: Default::default(),
            globals,
        })
    }
    /// Prepare ordinary-world markers from the matching sidecar before script execution.
    /// Preparation is atomic and cannot replace marker state within a world generation.
    pub fn prepare_markers(&mut self, data: &Animations) -> Result<()> {
        data.require_version()?;
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} marker/script cache generation mismatch; re-export the world",
            self.world
        );
        anyhow::ensure!(
            self.markers.is_none(),
            "world {} markers already prepared",
            self.world
        );
        self.markers = Some(
            super::markers::Markers::prepare(&data.metadata)
                .with_context(|| format!("world {} marker preparation", self.world))?,
        );
        Ok(())
    }

    /// Build the world's ropes from the exported `MPRP` records. Only the shipped link models
    /// (`ROPELINK`, `CHAINLNK`, both 0.8 world units tall) are supported; another model, or more
    /// than 255 ropes, is a contextual error. A cache without the records has no ropes.
    pub fn prepare_ropes(&mut self, data: &Animations) -> Result<()> {
        anyhow::ensure!(
            self.ropes.is_empty(),
            "world {} ropes already prepared",
            self.world
        );
        let Some(records) = data.metadata["ropes"].as_array() else {
            return Ok(());
        };
        let load = self
            .updates
            .ropes_load
            .context("world rope preparation requires SYSTEM.INI [system] ropes_load")?;
        if !load {
            return Ok(());
        }
        anyhow::ensure!(
            records.len() <= cyrus_game::rope::MAX_ROPES,
            "world {}: {} ropes exceed the engine's {}",
            self.world,
            records.len(),
            cyrus_game::rope::MAX_ROPES
        );
        for (index, value) in records.iter().enumerate() {
            let record = cyrus_game::rope::RopeRecord::from_metadata(value)
                .with_context(|| format!("world {} rope {index}", self.world))?;
            let model = value["rope_model"].as_str().unwrap_or_default();
            anyhow::ensure!(
                ["ROPELINK", "CHAINLNK"]
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(model.trim_end_matches(".3D"))),
                "world {} rope {index}: link model {model:?} has no recovered height",
                self.world
            );
            self.ropes.push(cyrus_game::rope::Rope::new(&record, 0.8));
        }
        Ok(())
    }

    /// The world's ropes, in construction order (the engine lists the newest first).
    pub fn ropes(&self) -> &[cyrus_game::rope::Rope] {
        &self.ropes
    }

    /// One rope-system step at its place in the world loop: each rope's amplitude service (which
    /// plays the swing sound at the rope's base, owned by its first attached actor), then its swing
    /// step and attachment climbs.
    pub(super) fn update_ropes(
        &mut self,
        frame_scale: i32,
        ticks: i32,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        for rope_index in 0..self.ropes.len() {
            let rope = &mut self.ropes[rope_index];
            rope.clamp_nodes();
            let sum = rope.step_sum();
            let owner = rope.attachments.first().map(|attachment| attachment.actor);
            let base = rope.position;
            if let (Some(sound), Some(owner)) = (rope.amplitude_service(sum), owner) {
                let engine = cyrus_game::rope::engine_position(base);
                sounds(
                    self.id(owner)?,
                    super::ScriptSound::Positional {
                        effect: sound.effect,
                        volume: 0,
                        rate_offset_hz: 0,
                        position: engine.map(|value| (value * 256.0).round() as i32),
                        start_index: Some(6),
                    },
                )?;
            }
        }
        let actors = &self.actors;
        for rope in &mut self.ropes {
            rope.update(
                frame_scale,
                ticks,
                |actor| {
                    actors
                        .get(actor)
                        .and_then(|actor| actor.animation.as_ref())
                        .and_then(|animation| animation.playback.current_group())
                        .map_or(0, i32::from)
                },
                // Only the selected player computes the limit; other attachments step freely.
                |rope, actor, node| match rope.down_limit {
                    Some((owner, limit)) if owner == actor => limit,
                    _ => node,
                },
            );
        }
        Ok(())
    }

    /// Load cleared reservations and associate ordinary placements from authored
    /// local coordinates. Routed movement remains unavailable; an absent table fails.
    pub fn prepare_navigation(&mut self, data: &Animations) -> Result<()> {
        data.require_version()?;
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} navigation/script cache generation mismatch; re-export the world",
            self.world
        );
        anyhow::ensure!(
            self.walk_nodes.is_none(),
            "world {} navigation already prepared",
            self.world
        );
        let mut nodes = super::navigation::WalkNodeReservations::prepare(
            &data.metadata["walk_node_maps"],
            self.world,
        )?;
        let associations = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| -> Result<_> {
                let Some(placement) = actor.placement().filter(|_| actor.control.is_some()) else {
                    return Ok(None);
                };
                let mut position = [0; 3];
                for (axis, &coordinate) in placement.map_position.iter().enumerate() {
                    position[axis] = coordinate.checked_mul(256).and_then(|value| i32::try_from(value).ok())
                        .with_context(|| format!("world {} actor {index} navigation position axis {axis}: outside Q8 i32 range", self.world))? >> 8;
                }
                Ok(Some(nodes.nearest(position)))
            })
            .collect::<Result<Vec<_>>>()?;
        for (actor, association) in self.actors.iter_mut().zip(associations) {
            if let Some(selected) = association
                && let Some(control) = actor.control.as_mut()
            {
                let state = &mut control.navigation;
                state.map = selected.map(|(map, _)| map);
                state.current = selected.map_or(-1, |(_, node)| node as i32);
            }
        }
        self.walk_nodes = Some(nodes);
        Ok(())
    }

    /// Find a free walk node using the query pose and authored positive Y extent.
    /// Returns loaded map/node indices and retains the shared winning map, clearing
    /// it on a miss. Loaded order, Manhattan distance and early return follow
    /// preservation's SOUP nearest-free-node contract; this does not reserve a node.
    pub fn nearest_free_walk_node(
        &mut self,
        actor: ScriptActorId,
    ) -> Result<Option<(usize, usize)>> {
        let result = (|| -> Result<_> {
            let position = self.query_position(actor)?;
            let height = self.ordinary_collision_shape(actor)?.positive_extent[1];
            Ok(self
                .walk_nodes
                .as_mut()
                .context("walk-node navigation not prepared")?
                .nearest_free(position, height))
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} nearest free walk node",
                self.world, actor.index
            )
        })
    }

    /// Find a point destination on this actor's current enabled map.
    /// Coordinates are Q8 and arithmetic-shifted to map units. Reservations do
    /// not filter candidates. Preservation's strict 4096 Manhattan bound and
    /// first improving distance below twelve apply. Retains the shared selected
    /// map on a hit and clears it on a miss without changing actor navigation.
    pub fn nearest_current_walk_node(
        &mut self,
        actor: ScriptActorId,
        destination: [i32; 3],
    ) -> Result<Option<usize>> {
        let result: Result<Option<usize>> = (|| {
            let index = self.resolve(actor)?;
            let map = self.actors[index]
                .control
                .as_ref()
                .context("actor navigation unavailable")?
                .navigation
                .map;
            Ok(self
                .walk_nodes
                .as_mut()
                .context("walk-node navigation not prepared")?
                .nearest_on_map(map, destination.map(|value| value >> 8)))
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} current-map point destination",
                self.world, actor.index
            )
        })
    }

    /// Find the next hop from the actor's held current node to a destination index.
    /// Preserves authored traversal, live source-node reservations and shared link flags.
    /// This selects a hop without reserving it, moving the actor or changing its navigation.
    /// The optional player filter must come from an established active exclusion switch.
    pub fn next_walk_node(
        &mut self,
        actor: ScriptActorId,
        destination: i32,
        exclusion: Option<WalkRouteExclusion>,
    ) -> Result<i32> {
        let result = (|| {
            let index = self.resolve(actor)?;
            let receiver = &self.actors[index];
            let navigation = receiver
                .control
                .as_ref()
                .context("actor navigation unavailable")?
                .navigation;
            anyhow::ensure!(
                navigation.current >= -1,
                "route current {} below unset sentinel",
                navigation.current
            );
            if navigation.current == -1 {
                return Ok(-1);
            }
            if navigation.current == destination {
                return Ok(destination);
            }
            if navigation.map.is_none() {
                return Ok(-1);
            }
            let identifier = receiver
                .runtime_identifier
                .context("route actor identifier unavailable")? as u8;
            // Retain a failed player sample until a qualifying link actually reads it.
            let player = exclusion.map(|input| {
                (
                    self.spatial(input.selected_player)
                        .map(|actor| actor.local.position.map(|v| v >> 8))
                        .map_err(|error| format!("route selected player: {error:#}")),
                    input.minimum_distance,
                )
            });
            self.walk_nodes
                .as_mut()
                .context("walk-node navigation not prepared")?
                .next_hop(
                    &navigation,
                    destination,
                    identifier,
                    &mut self.globals.walk_route,
                    |position| {
                        let Some((player, threshold)) = &player else {
                            return Ok(true);
                        };
                        let player = player
                            .as_ref()
                            .map_err(|error| anyhow::anyhow!("{error}"))?;
                        let delta =
                            std::array::from_fn(|axis| position[axis].wrapping_sub(player[axis]));
                        Ok(cyrus_collision::distance::rounded_distance(delta)? >= *threshold)
                    },
                )
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} route destination {destination}",
                self.world, actor.index
            )
        })
    }

    /// Set the retained runtime map flag. Searches remain independent of this flag;
    /// actor map validation releases held nodes when the map is disabled.
    pub fn set_walk_map_enabled(&mut self, map: usize, enabled: bool) -> Result<()> {
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .set_enabled(map, enabled)
            .with_context(|| format!("world {} map {map} navigation enablement", self.world))
    }

    /// DisableNodeMaps and EnableNodeMaps: set the runtime flag of every loaded map.
    pub fn set_all_walk_maps_enabled(&mut self, enabled: bool) -> Result<()> {
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .set_all_enabled(enabled);
        Ok(())
    }

    /// Validate the receiver's held map/indices against live map state.
    /// Invalid or absent maps clear indices; disabled maps retain the destination
    /// until the next absent-map validation, following preservation's SOUP rules.
    pub fn validate_walk_navigation(&mut self, actor: ScriptActorId) -> Result<bool> {
        let result = (|| {
            let index = self.resolve(actor)?;
            let receiver = &mut self.actors[index];
            let identifier = receiver
                .runtime_identifier
                .context("actor identifier unavailable")?;
            let state = &mut receiver
                .control
                .as_mut()
                .context("actor navigation unavailable")?
                .navigation;
            self.walk_nodes
                .as_mut()
                .context("walk-node navigation not prepared")?
                .validate(state, identifier)
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} validate navigation",
                self.world, actor.index
            )
        })
    }

    /// Release both held indices in the selected slot's order, then attempt a claim.
    /// Current assignment retains the old next index as history. Rejected claims
    /// leave both held indices unset while preserving the map and destination.
    pub fn assign_walk_node(
        &mut self,
        actor: ScriptActorId,
        node: i32,
        slot: super::WalkNodeSlot,
    ) -> Result<bool> {
        let result = (|| {
            let index = self.resolve(actor)?;
            let receiver = &mut self.actors[index];
            let identifier = receiver
                .runtime_identifier
                .context("actor identifier unavailable")?;
            let state = &mut receiver
                .control
                .as_mut()
                .context("actor navigation unavailable")?
                .navigation;
            self.walk_nodes
                .as_mut()
                .context("walk-node navigation not prepared")?
                .assign(state, identifier, node, slot)
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} assign {slot:?} node {node}",
                self.world, actor.index
            )
        })
    }

    /// Release held next/current nodes only when their reservation bytes match.
    /// Clears both indices, including foreign-owned/out-of-range nodes, while
    /// retaining the map and final destination. An absent map requires no table.
    pub fn release_held_walk_nodes(&mut self, actor: ScriptActorId) -> Result<()> {
        let result = (|| {
            let index = self.resolve(actor)?;
            let receiver = &mut self.actors[index];
            let state = &mut receiver
                .control
                .as_mut()
                .context("actor navigation unavailable")?
                .navigation;
            if state.map.is_none() {
                state.current = -1;
                state.next = -1;
                return Ok(());
            }
            let identifier = receiver
                .runtime_identifier
                .context("actor identifier unavailable")?;
            self.walk_nodes
                .as_mut()
                .context("walk-node navigation not prepared")?
                .release_held(state, identifier)
        })();
        result.with_context(|| {
            format!(
                "world {} actor {} release held navigation",
                self.world, actor.index
            )
        })
    }

    /// Acquire a loaded WDNM reservation using the actor identifier's low byte.
    /// A foreign owner returns false; stale actors and invalid map/node indices fail.
    pub fn reserve_walk_node(
        &mut self,
        actor: ScriptActorId,
        map: usize,
        node: usize,
    ) -> Result<bool> {
        let index = self.resolve(actor)?;
        let identifier = self.actors[index]
            .runtime_identifier
            .context("walk-node actor identifier unavailable")?;
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .acquire(map, node, identifier)
            .with_context(|| {
                format!(
                    "world {} actor {} reserve navigation",
                    self.world, actor.index
                )
            })
    }

    /// Release only a reservation whose byte still matches this actor's identifier.
    pub fn release_walk_node(
        &mut self,
        actor: ScriptActorId,
        map: usize,
        node: usize,
    ) -> Result<()> {
        let index = self.resolve(actor)?;
        let identifier = self.actors[index]
            .runtime_identifier
            .context("walk-node actor identifier unavailable")?;
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .release(map, node, identifier)
            .with_context(|| {
                format!(
                    "world {} actor {} release navigation",
                    self.world, actor.index
                )
            })
    }

    /// Complete the navigation stage of sound maintenance across every loaded map.
    pub fn clear_walk_node_reservations(&mut self) -> Result<()> {
        self.walk_nodes
            .as_mut()
            .context("walk-node navigation not prepared")?
            .reset();
        Ok(())
    }

    /// Actors registered as moved and awaiting regional selection, in registration order.
    pub fn pending_map_updates(&self) -> &[usize] {
        self.map_updates.as_slice()
    }

    /// Consume movement registrations before regional selection, in first-registration order.
    /// View coordinates use Q8 units. The caller supplies the original VM map-exclusion state.
    /// Excluded passes consume registrations without changing cell membership.
    pub fn flush_map_updates(&mut self, view: [i32; 3], excluded: bool) -> Result<()> {
        self.consume_map_updates(view, excluded)
    }

    fn consume_map_updates(&mut self, view: [i32; 3], excluded: bool) -> Result<()> {
        if self.map_updates.is_empty() {
            return Ok(());
        }
        if self.actor_lists.is_none() {
            self.actor_lists = Some(super::order::ActorLists::new(&self.actors)?);
        }
        let pending = self.map_updates.take();
        if excluded {
            return Ok(());
        }
        let spatial = self
            .spatial
            .as_ref()
            .context("deferred movement spatial state unavailable")?;
        let lists = self.actor_lists.as_mut().expect("actor lists prepared");
        for index in pending {
            let actor = &self.actors[index];
            if actor
                .placement()
                .is_none_or(|placement| placement.active != 0)
            {
                continue;
            }
            let pose = spatial.actor(index)?;
            lists
                .relocate(index, pose.local.position, pose.linked_position, view)
                .with_context(|| {
                    format!(
                        "world {} instance {index} actor {} deferred relocation",
                        self.world,
                        actor.name()
                    )
                })?;
        }
        Ok(())
    }

    /// Normal fresh-world update-list membership, before runtime actor exclusions.
    /// View coordinates use integer map units. Cell and persistent lists retain
    /// reverse creation order; the 18-cell window omits its corners and excludes
    /// `attr_short_mapping` actors on its outer border. The selection is retained until
    /// the starting cell changes, following preservation's `engine/animation.md`.
    /// Dynamic allocation, removal and cell migration require separate lifecycle support.
    pub fn ordinary_actors(&mut self, view: [i32; 2]) -> Result<&[ScriptActorId]> {
        if self.actor_lists.is_none() {
            self.actor_lists = Some(
                super::order::ActorLists::new(&self.actors)
                    .with_context(|| format!("world {} ordinary actor lists", self.world))?,
            );
        }
        self.actor_lists
            .as_mut()
            .expect("actor lists prepared")
            .select(&self.actors, self.generation, view)
            .with_context(|| format!("world {} ordinary actor list", self.world))
    }

    /// Prepare ordinary map poses and unique attachment families transactionally.
    /// Separate map records retain position with cleared temporary orientation;
    /// standalone roles still require their own spatial owners.
    pub fn prepare_spatial(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.spatial.is_none(),
            "world {} spatial state already prepared",
            self.world
        );
        self.spatial = Some(
            super::spatial::SpatialWorld::prepare(&self.actors)
                .with_context(|| format!("world {} spatial preparation", self.world))?,
        );
        Ok(())
    }

    fn query_is_dead(
        &self,
        id: ScriptActorId,
        selected_player: Option<ScriptActorId>,
    ) -> Result<bool> {
        let index = self.resolve(id)?;
        if selected_player == Some(id)
            && self
                .globals
                .player_hit_inhibited()
                .context("IsDead: selected-player hit inhibition unavailable")?
        {
            return Ok(true);
        }
        Ok(self.actors[index]
            .control
            .as_ref()
            .context("IsDead: actor death lifecycle unavailable")?
            .ai_type
            == AiType::DEAD)
    }

    /// `FixSlaveAngle(axis, angle)`: a slave sets one local angle component so that, added to its
    /// master group's angle, the component equals `angle` modulo 2048. The group angle table the
    /// original reads holds, per master group, the master's accumulated angles from the latest
    /// family update, which is the master's effective pose here. Axis 0 is pitch, 1 yaw and any
    /// other value roll; nothing else changes (the other components, the effective pose and the
    /// matrix wait for the next family refresh). A receiver without a slave group is unchanged. A
    /// slave group whose master is not the prepared parent has no established table entry and is
    /// reported instead of read as zero.
    fn fix_slave_angle(&mut self, id: ScriptActorId, axis: i32, angle: i32) -> Result<()> {
        let index = self.resolve(id)?;
        let group = self.actors[index].attributes[Attribute::Slave];
        if group == 0 {
            return Ok(());
        }
        let spatial = self
            .spatial
            .as_ref()
            .context("FixSlaveAngle: spatial state not prepared")?;
        let master = spatial
            .actor(index)?
            .parent
            .filter(|&parent| self.actors[parent].attributes[Attribute::Master] == group)
            .with_context(|| {
                format!(
                    "FixSlaveAngle: slave group {group} has no prepared master; its group angle table entry is not established"
                )
            })?;
        let master_angles = spatial.actor(master)?.query_pose()?.angles;
        let component = match axis {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let local = angle.wrapping_sub(master_angles[component]) & 2047;
        self.spatial
            .as_mut()
            .context("FixSlaveAngle: spatial state not prepared")?
            .actor_mut(index)?
            .local
            .angles[component] = local;
        Ok(())
    }

    /// The previous-position counterpart of [`Self::query_position`]: the previous attached
    /// position with `attr_slave`, else the link record's previous cached position, else the
    /// previous local position.
    pub(super) fn query_previous_position(&self, id: ScriptActorId) -> Result<[i32; 3]> {
        let index = self.resolve(id)?;
        let actor = self.spatial(id)?;
        if self.actors[index].attributes[Attribute::Slave] != 0 {
            actor
                .previous_effective_position
                .context("attached query previous effective position unavailable")
        } else if actor.linked_position.is_some() {
            Ok(actor.link_previous)
        } else {
            Ok(actor.previous_local_position)
        }
    }

    /// The position the ground and support probes read, as the original's query-position selector
    /// picks it: the composed attached position with `attr_slave`, else the linked record's cached
    /// position, else the local position.
    pub(super) fn query_position(&self, id: ScriptActorId) -> Result<[i32; 3]> {
        let index = self.resolve(id)?;
        let actor = self.spatial(id)?;
        if self.actors[index].attributes[Attribute::Slave] != 0 {
            Ok(actor
                .effective
                .context("query attached position unavailable")?
                .position)
        } else {
            Ok(actor.linked_position.unwrap_or(actor.local.position))
        }
    }

    /// Initialize a standalone player role's authoritative pose once per world generation.
    /// The caller supplies established entry coordinates in Q8 and wrapped angles.
    /// This does not settle ground, initialize animation, or run the controller.
    pub fn initialize_player_spatial(
        &mut self,
        id: ScriptActorId,
        position: [i32; 3],
        angles: [u16; 3],
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        anyhow::ensure!(
            matches!(
                actor.source,
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
            ),
            "world {} instance {index}: spatial player initialization requires a player role",
            self.world
        );
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .initialize_player(index, actor, position, angles)
            .with_context(|| {
                format!(
                    "world {} instance {index}: player spatial initialization",
                    self.world
                )
            })
    }

    /// Select an ordered combat taunt for an already admitted invocation.
    /// Supports begin-combat, hurt, death and opponent-unarmed types, following
    /// preservation config/combat-ini.md. Each matching nonzero label consumes
    /// one shared random draw. Death alone falls back to hurt when none matched.
    /// The caller owns eligibility and prepared presentation dependencies; this
    /// does not enter combat or admit the generic timed selection path.
    pub fn select_ordered_combat_taunt(
        &self,
        id: ScriptActorId,
        trigger: TauntType,
        random: &mut cyrus_game::random::EngineRandom,
    ) -> Result<Option<&CombatTaunt>> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            matches!(
                trigger,
                TauntType::BEGIN_COMBAT
                    | TauntType::HURT_BY_OPPONENT
                    | TauntType::DEATH
                    | TauntType::UNARMED_MALE_OPPONENT
                    | TauntType::UNARMED_FEMALE_OPPONENT
            ),
            "world {} instance {index}: combat taunt type {trigger} requires timed selection",
            self.world
        );
        Ok(select_combat_taunt(&self.actors[index], trigger, random))
    }

    /// Read stored actor surface state; unsupported object paths have no established value.
    pub fn actor_surface(&self, id: ScriptActorId) -> Result<Surface> {
        let index = self.resolve(id)?;
        self.actors[index].surface.with_context(|| {
            format!(
                "world {} instance {index} actor {} surface state unavailable",
                self.world,
                self.actors[index].name()
            )
        })
    }

    /// Current selected-player item gates from the logical actor, never presentation defaults.
    /// Player switching and prompt lineup scheduling remain separate unsupported services.
    /// Acceptance-run tool for the control port's `set_attribute`: write one attribute byte of the
    /// actor at `index`, for example a health of zero to reach a script's death branch without
    /// playing the fight.
    pub fn set_actor_attribute(&mut self, index: usize, attribute: usize, value: u8) -> Result<()> {
        let actor = self
            .actors
            .get_mut(index)
            .with_context(|| format!("set attribute: no actor {index}"))?;
        *actor
            .attributes
            .bytes_mut()
            .get_mut(attribute)
            .with_context(|| format!("set attribute: no attribute {attribute}"))? = value;
        Ok(())
    }

    /// Acceptance-run tool for the control port's `mark_dead`: set the actor's health to zero and
    /// its lifecycle selector to eight, the state `IsDead` reads, without the death animation or
    /// sounds, so a script's death branch can be reached without playing the fight.
    pub fn mark_actor_dead(&mut self, index: usize) -> Result<()> {
        let actor = self
            .actors
            .get_mut(index)
            .with_context(|| format!("mark dead: no actor {index}"))?;
        actor.attributes[Attribute::Health] = 0;
        actor
            .control
            .as_mut()
            .with_context(|| format!("mark dead: actor {index} has no control state"))?
            .ai_type = AiType::DEAD;
        Ok(())
    }

    /// What the island map screen reads when it opens.
    pub fn island_map_world(&self) -> crate::island_map::IslandMapWorld {
        let player = self.selected_player_index();
        crate::island_map::IslandMapWorld {
            world: self.world,
            player: player
                .and_then(|index| self.id(index).ok())
                .and_then(|id| self.spatial(id).ok())
                .map(|spatial| spatial.local.position),
            lockout: self.globals.lockout(),
            player_type: player.map(|index| self.actors[index].attributes[Attribute::PlayerType]),
        }
    }

    pub fn player_activation_state(&self) -> Result<(ScriptActorId, super::ActivationActor)> {
        let index = self
            .selected_player_index()
            .context("selected player absent")?;
        let actor = &self.actors[index];
        let control = actor
            .control
            .as_ref()
            .context("selected player control state unavailable")?;
        let id = self.id(index)?;
        Ok((
            id,
            super::ActivationActor {
                state: control.living_state,
                on_rope: control.rope_attached,
                water_state: self.actor_surface(id)?,
                torch_burning: control.torch_burning,
                weapon_assigned: control.combat_death_default.is_some(),
                weapon_drawn: control.weapon_drawn,
                animation: actor
                    .bound_animation_group()
                    .map(|group| super::ActivationAnimation { group }),
            },
        ))
    }

    /// Commit the actor driver's stored surface, including assignments before a failed transition.
    /// SOUP water predicates consume this value without querying terrain again.
    pub fn set_actor_surface(
        &mut self,
        id: ScriptActorId,
        surface: cyrus_assets::surface::Surface,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        self.actors[index].surface = Some(surface);
        Ok(())
    }

    /// Acceptance-run and test tool: put the actor in or out of the slope response's slide, whose
    /// only writer is the ordinary slope response.
    pub fn set_actor_sliding(&mut self, id: ScriptActorId, sliding: bool) -> Result<()> {
        let index = self.resolve(id)?;
        self.actors[index]
            .control
            .as_mut()
            .with_context(|| format!("set sliding: actor {index} has no control state"))?
            .sliding_state = u8::from(sliding);
        Ok(())
    }

    /// Authoritative local and composed pose for this generation's actor.
    pub fn spatial(&self, id: ScriptActorId) -> Result<&super::ActorSpatial> {
        let index = self.resolve(id)?;
        self.spatial
            .as_ref()
            .context("spatial state not prepared")?
            .actor(index)
    }

    /// Snapshot local position and angles at the explicit ordinary actor update boundary.
    pub fn snapshot_spatial(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .snapshot();
        self.refresh_linked_position(index)
    }

    /// Restore the current attribute-controlled axes after this actor's movement/script work.
    /// Matrix and family refresh remain separate later stages.
    pub fn restore_axis_locks(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .restore_axis_locks(&self.actors[index].attributes)
            .with_context(|| format!("world {} instance {index} axis locks", self.world))
    }

    /// Refresh an unattached actor matrix without consuming its angle snapshot.
    pub fn refresh_transform(&mut self, id: ScriptActorId) -> Result<()> {
        let index = self.resolve(id)?;
        self.spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?
            .refresh_transform()
    }

    /// Explicit family pass, publishing attached model transforms even without geometry.
    /// Retains root model snapshots and selected geometry, radius and animation.
    /// Does not infer timing or actor update eligibility.
    pub fn refresh_families(&mut self) -> Result<()> {
        let spatial = self
            .spatial
            .as_mut()
            .context("spatial state not prepared")?;
        spatial
            .refresh_families()
            .with_context(|| format!("world {} attachment refresh", self.world))?;
        for (index, actor) in self.actors.iter_mut().enumerate() {
            if !spatial.is_initialized(index)? {
                continue;
            }
            let state = spatial.actor(index)?;
            if state.parent.is_none() {
                continue;
            }
            let pose = state
                .effective
                .context("refreshed family pose unavailable")?;
            let position = pose.position.map(|value| (value >> 8) as f32);
            actor.model.position = Some(position);
            if let Some(current) = actor.model.current.as_mut() {
                current.pose = pose;
                current.position = position;
            }
        }
        Ok(())
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Prepares one actor's animation without granting update eligibility.
    /// The default group is supplied by an established caller, not inferred from placement.
    pub fn bind_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        default: AnimationGroup,
    ) -> Result<()> {
        let index = self.validate_animation_binding(id, data)?;
        let playback = cyrus_world::animation::ActorAnimation::from_asset(
            data,
            self.actors[index].name(),
            default,
        )?;
        self.actors[index].animation = Some(ScriptAnimation {
            playback,
            frame: None,
        });
        Ok(())
    }

    /// Initialize an ordinary placed animation-table model using shared nominal ticks.
    /// This does not grant update eligibility or initialize standalone player roles.
    /// Actor events execute against this world's camera/effect owners at their command;
    /// the callback receives sounds and transitions. Failures are sticky because
    /// already-dispatched effects cannot be rolled back.
    pub fn initialize_map_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.validate_animation_binding(id, data)?;
        let position = animation_sound_position(self.spatial.as_ref(), index);
        let contact_profile = if self.actors[index].attributes[Attribute::Combat] != 0 {
            data.combat_contact_profile(self.actors[index].name())?
        } else {
            Default::default()
        };
        let events = effects::ActorEventContext::capture(self, id);
        let camera = &mut self.camera;
        let effects = &mut self.effects;
        let actor = &mut self.actors[index];
        let placement = actor
            .placement()
            .context("animation initialization requires a map actor")?;
        anyhow::ensure!(
            placement.object_type == PlacementType::MODEL,
            "world {} instance {index}: animation initialization requires map object type 1",
            self.world
        );
        anyhow::ensure!(
            data.actor(actor.name())?["model_load_path"].as_str() == Some("animation_table"),
            "world {} instance {index}: actor does not use an animation model table",
            self.world
        );
        // With the animation system off the request routine returns before touching the record,
        // so the two initial panic requests leave it with no current or pending group and no frame.
        let result = if self.updates.animation_system {
            cyrus_world::animation::ActorAnimation::initialize(
                data,
                actor.name(),
                ticks,
                &mut |sound| {
                    if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                        return events.dispatch(flags, camera.as_mut(), effects);
                    }
                    sounds(
                        id,
                        super::ScriptSound::Animation {
                            sound,
                            surface: actor.surface,
                            position,
                        },
                    )
                },
            )
        } else {
            cyrus_world::animation::ActorAnimation::from_asset(
                data,
                actor.name(),
                AnimationGroup::PANIC,
            )
            .map(|playback| (playback, None))
        };
        match result {
            Ok((playback, frame)) => {
                actor
                    .control
                    .as_mut()
                    .expect("type-one map actor has allocation state")
                    .combat_death_default = (actor.attributes[Attribute::Combat] != 0).then_some(0);
                let control = actor.control.as_mut().expect("allocated map control");
                control.combat = super::CombatControlState::initialize(
                    &actor.attributes,
                    control.combat.as_ref(),
                    &mut self.next_combat_index,
                    &self.combat_attacks,
                    contact_profile,
                )
                .with_context(|| {
                    format!("world {} instance {index} combat construction", self.world)
                })?;
                actor.animation = Some(ScriptAnimation { playback, frame });
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "world {} instance {index} animation initialization: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message)
            }
        }
    }

    /// Select a nonplayer map actor's ordinary default using its owned state.
    /// Item readiness and shared nominal ticks come from the invoking game stage.
    /// This is separate from advancement and never grants update eligibility.
    pub fn select_map_animation_default(
        &mut self,
        id: ScriptActorId,
        item_system_ready: bool,
        ticks: u32,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        let selected = self.selected_player_index() == Some(index);
        // The selected player's deep-water default also runs the weapon cleanup first.
        if selected
            && self.actors[index].failure().is_none()
            && self.actors[index]
                .control
                .as_ref()
                .is_some_and(|control| control.ai_type != AiType::DEAD)
            && self.actors[index]
                .surface
                .is_some_and(Surface::is_deep_water)
        {
            self.cleanup_actor_weapon(id, id, ticks, sounds)?;
        }
        let events = effects::ActorEventContext::capture(self, id);
        let camera = &mut self.camera;
        let effects = &mut self.effects;
        let actor = &mut self.actors[index];
        if let Some(error) = actor.failure() {
            anyhow::bail!("{error}");
        }
        let control = actor.control.as_ref().with_context(|| {
            format!(
                "world {} instance {index}: ordinary map control state unavailable",
                self.world
            )
        })?;
        let animation = actor.animation.as_mut().with_context(|| {
            format!(
                "world {} instance {index}: animation not prepared",
                self.world
            )
        })?;
        let group = if control.ai_type == AiType::DEAD {
            if let Some(default) = control.combat_death_default {
                AnimationGroup::from_index(default as u16)
            } else if animation
                .playback
                .has_group(AnimationGroup::DEATH_FIGHT_STAB_END)
            {
                AnimationGroup::DEATH_FIGHT_STAB_END
            } else {
                AnimationGroup::DEATH_FIGHT_HARD_END
            }
        } else if selected && actor.surface.is_some_and(Surface::is_deep_water) {
            // The selected player in deep water treads water after the weapon cleanup above.
            AnimationGroup::SWIM_IDLE
        } else if control.rope_attached {
            AnimationGroup::ROPE_PANIC
        } else if control.living_state == LivingState::HANGING {
            AnimationGroup::IDLE_LEDGE
        } else {
            let (holding, drawing, sheathing) = super::activation::weapon_predicates(
                control.living_state,
                Some(super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
                control.combat_death_default.is_some(),
                control.weapon_drawn,
                actor.attributes[Attribute::HoldingWeapon],
                item_system_ready,
                false,
            );
            if sheathing {
                AnimationGroup::from_index(actor.attributes[Attribute::PanicGroup].into())
            } else if drawing || holding {
                AnimationGroup::FIGHT_PANIC
            } else if control.torch_burning != 0 {
                AnimationGroup::IDLE_TORCH
            } else {
                AnimationGroup::from_index(actor.attributes[Attribute::PanicGroup].into())
            }
        };
        let position = animation_sound_position(self.spatial.as_ref(), index);
        let surface = actor.surface;
        let result = animation
            .playback
            .select_default_with_sounds(group, ticks, &mut |sound| {
                if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                    return events.dispatch(flags, camera.as_mut(), effects);
                }
                sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                )
            });
        match result {
            Ok(frame) => {
                animation.frame = frame.or(animation.frame);
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "world {} instance {index} default selection: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message)
            }
        }
    }

    fn validate_animation_binding(&self, id: ScriptActorId, data: &Animations) -> Result<usize> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            data.world == self.world,
            "world {} animation data belongs to world {}",
            self.world,
            data.world
        );
        anyhow::ensure!(
            data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} animation/script cache generation mismatch; re-export the world",
            self.world
        );
        let actor = &self.actors[index];
        if let Some(error) = actor.failure() {
            anyhow::bail!("{error}");
        }
        anyhow::ensure!(
            actor.animation.is_none(),
            "world {} instance {index}: animation already prepared",
            self.world
        );
        Ok(index)
    }
    /// Construct a standalone player role's animation and combat association once.
    /// The caller owns entry position/surface snapshots for synchronous sounds and
    /// supplies the existing nominal ticks. Actor events execute against this world's
    /// current logical pose and camera/effect owners. This does not advance controls.
    pub fn initialize_player_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.validate_animation_binding(id, data)?;
        let contact_profile = if self.actors[index].attributes[Attribute::Combat] != 0 {
            data.combat_contact_profile(self.actors[index].name())?
        } else {
            Default::default()
        };
        let events = effects::ActorEventContext::capture(self, id);
        let camera = &mut self.camera;
        let effects = &mut self.effects;
        let actor = &mut self.actors[index];
        anyhow::ensure!(
            matches!(
                actor.source,
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
            ),
            "world {} instance {index}: player initialization requires a standalone player role",
            self.world
        );
        let control = actor
            .control
            .as_mut()
            .expect("player role has allocation state");
        self.globals.initialize_player_controller();
        if matches!(actor.source, ScriptActorSource::Player) {
            let captured = self
                .globals
                .selected_health
                .as_ref()
                .and_then(|health| health.death())
                .and_then(|death| death.local_position());
            let mut health =
                super::SelectedHealthState::initialized(actor.attributes[Attribute::Health]);
            health.retain_death_position(captured);
            self.globals.selected_health = Some(health);
        }
        control.combat_death_default = (actor.attributes[Attribute::Combat] != 0).then_some(0);
        control.combat = super::CombatControlState::initialize(
            &actor.attributes,
            control.combat.as_ref(),
            &mut self.next_combat_index,
            &self.combat_attacks,
            contact_profile,
        )
        .with_context(|| {
            format!(
                "world {} instance {index} player combat construction",
                self.world
            )
        })?;
        let mut emit = |sound| {
            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                return events.dispatch(flags, camera.as_mut(), effects);
            }
            sounds(sound)
        };
        match crate::player::placement::initial_role_animation(data, actor.name(), ticks, &mut emit)
        {
            Ok((playback, frame)) => {
                actor.animation = Some(ScriptAnimation { playback, frame });
                if matches!(actor.source, ScriptActorSource::AlternatePlayer) {
                    actor
                        .control
                        .as_mut()
                        .expect("player role has allocation state")
                        .hidden = true;
                }
                Ok(())
            }
            Err(error) => {
                let message = format!(
                    "world {} instance {index} player initialization: {error:#}",
                    self.world
                );
                actor.animation_failure = Some(message.clone());
                anyhow::bail!(message)
            }
        }
    }

    /// The admitted player stage and SOUP share this playback and selected file frame.
    pub(crate) fn player_animation(
        &mut self,
        id: ScriptActorId,
        data: &Animations,
        ticks: u32,
        sounds: &mut dyn FnMut(cyrus_world::animation::AnimationSound) -> Result<()>,
    ) -> Result<&mut ScriptAnimation> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            self.selected_player_index() == Some(index),
            "world {} instance {index}: grounded update requires the logical player",
            self.world
        );
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} player animation/script cache generation mismatch; re-export the world",
            self.world
        );
        if let Some(error) = self.actors[index].failure() {
            anyhow::bail!("world {} player: {error}", self.world);
        }
        if self.actors[index].animation.is_none() {
            self.initialize_player_animation(id, data, ticks, sounds)?;
        }
        Ok(self.actors[index]
            .animation
            .as_mut()
            .expect("player animation initialized"))
    }

    /// Last selected file frame; resolve through this world's animation asset for presentation.
    pub fn animation_frame(&self, id: ScriptActorId) -> Result<Option<u32>> {
        let index = self.resolve(id)?;
        Ok(self.actors[index]
            .animation
            .as_ref()
            .and_then(|animation| animation.frame))
    }
    /// Deterministic inspection of one body. This does not establish actor eligibility
    /// or assign the shared native source. Use an eligible Entry or normal main stage
    /// first when the body needs native string parameters.
    pub fn step(&mut self, id: ScriptActorId, thread: Thread, budget: usize) -> Result<()> {
        self.advance(
            id,
            ScriptStage::Body(thread),
            ScriptInputs::default(),
            None,
            None,
            budget,
        )
    }
    /// Explicit task timing only. Missing scale fails before changing any actor state.
    pub fn step_tasks(&mut self, id: ScriptActorId, frame_scale: Option<u32>) -> Result<()> {
        self.resolve(id)?;
        let frame_scale = frame_scale.with_context(|| {
            format!(
                "world {} instance {} task stage requires frame_scale",
                self.world, id.index
            )
        })?;
        self.advance(
            id,
            ScriptStage::Tasks,
            ScriptInputs {
                frame_scale: Some(frame_scale),
                ..Default::default()
            },
            None,
            None,
            0,
        )
    }
    /// Invokes one explicit stage against the actor's shared native services.
    /// Worlds with any animation-bound actor require nominal ticks, including zero. Sound effects
    /// are dispatched synchronously with the receiving actor's identity.
    /// Random functions require the caller's shared engine stream; they never seed actor state.
    /// Standalone ordinary stages skip killed records, including retained world records.
    /// Explicit body and routed calls keep their own invocation semantics.
    /// The live `task` and `animation` cheat states replace the installed `SYSTEM.INI` switches the
    /// update gates read; absent inputs leave the installed values.
    pub(super) fn apply_system_switches(&mut self, inputs: &ScriptInputs<'_>) {
        if let Some(task) = inputs.task_system {
            self.updates.task_system = task;
        }
        // `animation_system` is the value-one state and `animation_running` the nonzero state;
        // a host supplying only one gives both the same answer.
        if let Some(running) = inputs.animation_running.or(inputs.animation_system) {
            self.updates.animation_system = running;
        }
        if let Some(strict) = inputs.animation_system {
            self.animation_strict = Some(strict);
        }
    }

    /// Test hook: replace an actor's local angles without refreshing its pose.
    #[doc(hidden)]
    pub fn set_local_angles_for_test(&mut self, id: ScriptActorId, angles: [i32; 3]) {
        self.spatial
            .as_mut()
            .expect("spatial state prepared")
            .actor_mut(id.index)
            .expect("actor spatial state")
            .local
            .angles = angles;
    }

    /// Test hook: [`Self::animation_waits`].
    #[doc(hidden)]
    pub fn animation_waits_for_test(&self) -> bool {
        self.animation_waits()
    }

    /// Whether the movement natives wait for their requested group to become current: the
    /// original tests the `animation` state for exactly one there, but for nonzero elsewhere.
    fn animation_waits(&self) -> bool {
        self.animation_strict
            .unwrap_or(self.updates.animation_system)
    }

    pub fn advance(
        &mut self,
        id: ScriptActorId,
        stage: ScriptStage,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
        random: Option<&mut cyrus_game::random::EngineRandom>,
        budget: usize,
    ) -> Result<()> {
        self.apply_system_switches(&inputs);
        let index = self.resolve(id)?;
        // Killed map records survive revisits, but never re-enter ordinary script stages.
        if matches!(stage, ScriptStage::OrdinaryMain | ScriptStage::OrdinaryHook)
            && self.actors[index]
                .control
                .as_ref()
                .is_some_and(|control| control.killed)
        {
            return Ok(());
        }
        self.advance_admitted(id, stage, inputs, sounds, random, budget)
    }

    // The outer traversal checks killed state once, before the hook and main continuation.
    pub(super) fn advance_admitted(
        &mut self,
        id: ScriptActorId,
        stage: ScriptStage,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
        random: Option<&mut cyrus_game::random::EngineRandom>,
        budget: usize,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        if let Some(activation) = inputs.activation {
            self.validate_activation(activation)?;
        }
        if matches!(stage, ScriptStage::SeparateMain | ScriptStage::SeparateHook) {
            anyhow::ensure!(
                (matches!(self.actors[index].source, ScriptActorSource::Shell(_))
                    || self.actors[index]
                        .placement()
                        .is_some_and(|placement| placement.object_type.has_separate_record()))
                    && self.actors[index].control.is_some(),
                "separate stage requires admitted type-two or type-six scratch state"
            );
        }
        let nominal_ticks = inputs.nominal_ticks;
        if matches!(
            stage,
            ScriptStage::Tasks | ScriptStage::OrdinaryMain | ScriptStage::SeparateMain
        ) {
            anyhow::ensure!(
                inputs.frame_scale.is_some(),
                "task stage requires frame_scale"
            );
        }
        if self.spatial.is_some() {
            anyhow::ensure!(
                inputs.frame_scale.is_some() && nominal_ticks.is_some(),
                "spatial script stages require frame_scale and nominal_ticks"
            );
        }
        if let Some(query) = inputs.player_query {
            anyhow::ensure!(
                query.linked_position.is_none(),
                "script query linked-position ownership must be established per receiver"
            );
        }
        if let Some(error) = self.actors[index].failure() {
            anyhow::bail!("{error}");
        }
        if self.actors.iter().any(|actor| actor.animation.is_some())
            || matches!(stage, ScriptStage::Animation)
        {
            anyhow::ensure!(
                nominal_ticks.is_some(),
                "world {} instance {index}: animation requires nominal_ticks",
                self.world
            );
        }
        // With drno on, every actor except the selected player and the camera actor skips both
        // its task update and its response timers (preservation `engine/cheats.md`), so nothing
        // runs here, including the response finish.
        if matches!(stage, ScriptStage::OrdinaryMain)
            && inputs.actor_update_skipped.unwrap_or(false)
            && self.selected_player() != Some(id)
            && self.camera.as_ref().map(|camera| camera.camera_actor) != Some(id)
        {
            return Ok(());
        }
        if matches!(stage, ScriptStage::OrdinaryMain) && !self.updates.task_system {
            return self.finish_ordinary_script_response(id, nominal_ticks);
        }
        if matches!(stage, ScriptStage::OrdinaryHook) {
            let actor = &self.actors[index];
            let control = actor.control.as_ref().with_context(|| {
                format!(
                    "world {} instance {index}: script-control lifecycle unavailable",
                    self.world
                )
            })?;
            if control.script_control & 2 != 0
                || (self.updates.task_system
                    && actor
                        .runtime
                        .as_ref()
                        .is_some_and(Runtime::has_unpaused_task))
            {
                return Ok(());
            }
        }
        if matches!(stage, ScriptStage::SeparateHook)
            && self.updates.task_system
            && self.actors[index]
                .runtime
                .as_ref()
                .is_some_and(Runtime::has_unpaused_task)
        {
            return Ok(());
        }
        let bypass_tasks = if matches!(stage, ScriptStage::OrdinaryMain) {
            let control = self.actors[index].control.as_ref().with_context(|| {
                format!(
                    "world {} instance {index}: script-control lifecycle unavailable",
                    self.world
                )
            })?;
            if control.script_control & 1 != 0 {
                return self.finish_ordinary_script_response(id, nominal_ticks);
            }
            control.script_control & 4 != 0
        } else {
            false
        };
        let ordinary_actors = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.retained_selected())
            .map(<[_]>::to_vec);
        let native_source_program = match self.globals.native_source {
            super::activation::NativeSource::Actor { generation, index }
                if generation == self.generation =>
            {
                self.actors
                    .get(index)
                    .and_then(|actor| actor.runtime.as_ref())
                    .map(Runtime::shared_program)
            }
            _ => None,
        };
        let mut host = GameHost {
            native_source_program,
            world: self.world,
            animation_system: self.updates.animation_system,
            task_system: self.updates.task_system,
            index,
            generation: self.generation,
            ordinary_actors: ordinary_actors.as_deref(),
            walk_map_count: self.walk_nodes.as_ref().map(|nodes| nodes.map_count()),
            registry: self,
            sounds,
            random,
            inputs,
        };
        if matches!(stage, ScriptStage::Animation) {
            anyhow::ensure!(
                host.registry.actors[index].animation.is_some(),
                "world {} instance {index}: actor animation not prepared",
                host.world
            );
            return host.advance_animation().map_err(|error| {
                let message = format!(
                    "world {} instance {index} animation stage: {error:#}",
                    host.world
                );
                host.registry.actors[index].animation_failure = Some(message.clone());
                anyhow::anyhow!(message)
            });
        }
        let native_hook = host.registry.actors[index].native_hook.clone();
        let actor = &mut host.registry.actors[index];
        let mut runtime = actor.runtime.take().with_context(|| {
            actor
                .unavailable
                .clone()
                .unwrap_or_else(|| "program absent".into())
        })?;
        let mut flags = std::mem::take(&mut host.registry.flags);
        let result = match stage {
            ScriptStage::OrdinaryMain | ScriptStage::SeparateMain => runtime.invoke_main_stage(
                &mut host,
                &mut flags,
                inputs.frame_scale.unwrap(),
                budget,
                bypass_tasks,
            ),
            ScriptStage::NativeHook | ScriptStage::OrdinaryHook | ScriptStage::SeparateHook => {
                match native_hook {
                    Some(call) => runtime
                        .invoke_native_hook(&call, &mut host, &mut flags)
                        .map(|_| ()),
                    None => Ok(()),
                }
            }
            ScriptStage::Entry => runtime.invoke_entry(&mut host, &mut flags, budget),
            ScriptStage::Body(thread) => runtime.invoke_body(thread, &mut host, &mut flags, budget),
            ScriptStage::Tasks => {
                runtime.invoke_tasks_timed(&mut host, &mut flags, inputs.frame_scale.unwrap())
            }
            ScriptStage::Animation => unreachable!(),
        };
        host.registry.flags = flags;
        host.registry.actors[index].runtime = Some(runtime);
        result?;
        if matches!(stage, ScriptStage::OrdinaryMain) {
            self.finish_ordinary_script_response(id, nominal_ticks)?;
        }
        Ok(())
    }
    /// Gameplay-global state shared by native calls and activation presentation.
    pub fn globals(&self) -> &super::GameplayGlobals {
        &self.globals
    }
    /// Borrow the topic-selection owner state without changing actor identity.
    pub fn globals_mut(&mut self) -> &mut super::GameplayGlobals {
        &mut self.globals
    }

    /// Atomically select CYRUS or GREMLIN for host-owned player binding.
    ///
    /// The original swaps two standalone player records, then reloads camera record zero from
    /// the selected role's installed camera defaults.  This method owns only that logical-world
    /// portion of the transaction: a host must rebind its entity, controller and presentation
    /// after the returned identity is published.  Both roles are required even when selecting
    /// the currently active one, so a partial world cannot silently enter a swapped state.
    pub fn select_player(&mut self, swapped: bool) -> Result<ScriptActorId> {
        let player = self
            .actors
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .context("selected-player swap: CYRUS role unavailable")?;
        let alternate = self
            .actors
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::AlternatePlayer))
            .context("selected-player swap: GREMLIN role unavailable")?;
        let selected = if swapped { alternate } else { player };
        let selected_id = self.id(selected)?;

        // Build the camera mutation before changing the selector.  A malformed camera record
        // therefore leaves both the selector and camera state untouched.
        let next_camera = self
            .camera
            .as_ref()
            .map(|camera| {
                let mut next = camera.clone();
                next.reload_player_defaults(
                    &self.camera_settings,
                    selected_id,
                    self.actors[selected].attributes[Attribute::PlayerType],
                )?;
                Ok::<_, anyhow::Error>(next)
            })
            .transpose()?;

        self.globals.player_swapped = swapped;
        if let Some(camera) = next_camera {
            self.camera = Some(camera);
        }
        Ok(selected_id)
    }

    /// Shared ownership and pending text; fresh world preparation resets both.
    pub fn activation(&self) -> &super::ActivationState {
        &self.activation
    }

    fn validate_activation(&self, inputs: &super::ActivationInputs) -> Result<()> {
        let player = self.resolve(inputs.player)?;
        anyhow::ensure!(
            self.selected_player_index() == Some(player),
            "activation player is not the selected player"
        );
        for &index in inputs.actors.keys() {
            self.id(index)?;
        }
        Ok(())
    }

    /// Advance one explicit overlay update using the same world-owned prompt as SOUP.
    /// The caller supplies nominal ticks and the selected player's current conditions.
    pub fn activation_overlay(
        &mut self,
        inputs: &super::ActivationInputs,
        ticks: u32,
        cycle_held: bool,
        override_ticks: u32,
    ) -> Result<super::ActivationOverlay> {
        self.validate_activation(inputs)?;
        let overlay = self.activation.overlay(
            inputs,
            &self.actors[inputs.player.index],
            ticks,
            cycle_held,
            override_ticks,
            self.globals.lockout(),
        )?;
        self.activation.record_shown(&overlay);
        Ok(overlay)
    }

    fn resolve(&self, actor: ScriptActorId) -> Result<usize> {
        anyhow::ensure!(
            actor.generation == self.generation,
            "stale script actor generation {} in world {} generation {}",
            actor.generation,
            self.world,
            self.generation
        );
        anyhow::ensure!(
            actor.index < self.actors.len(),
            "world {} instance {} absent",
            self.world,
            actor.index
        );
        let receiver = &self.actors[actor.index];
        anyhow::ensure!(
            !receiver.created_retired,
            "world {} retired created receiver {} {} is unsupported",
            self.world,
            actor.index,
            receiver.name()
        );
        Ok(actor.index)
    }
    /// Bounded diagnostics carry logical state, never installed bytecode.
    pub fn inspect(&self, offset: usize, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "world": self.world, "generation": self.generation, "content_id": self.content_id, "total": self.actors.len(),
            "activation": self.activation, "globals": self.globals,
            "actors": self.actors.iter().enumerate().skip(offset).take(limit.min(64)).map(|(index, actor)| serde_json::json!({
                "id": ScriptActorId { generation: self.generation, index }, "name": actor.name(),
                "source": actor.source.label(), "runtime_identifier": actor.runtime_identifier, "broadcast_index": actor.broadcast_index,
                "object_id": actor.placement().map(|placement| placement.object_id), "unavailable": actor.unavailable,
                "main": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Main)),
                "interrupt": actor.runtime.as_ref().and_then(|runtime| runtime.program_counter(Thread::Interrupt)),
                "locals": actor.runtime.as_ref().map(|runtime| runtime.variables()),
                "attributes": actor.attributes,
                "control": actor.control, "surface": actor.surface,
                "spatial": self.spatial.as_ref().and_then(|spatial| spatial.actor(index).ok()),
                "animation": actor.animation.as_ref().map(|animation| serde_json::json!({
                    "current_group": animation.playback.current_group(),
                    "pending_group": animation.playback.pending_group(),
                    "frame": animation.frame,
                })),
                "weapon": actor.model.weapon.as_ref().map(|weapon| serde_json::json!({
                    "item": weapon.item, "model": weapon.model, "position": weapon.geometry.position,
                    "retained_point": weapon.geometry.retained_point,
                    "retained_angles": weapon.geometry.retained_angles, "endpoints": weapon.endpoints,
                })),
                "tasks": actor.runtime.as_ref().map(|runtime| runtime.active_tasks()),
                "task_slots": actor.runtime.as_ref().map(|runtime| {
                    runtime
                        .task_slots()
                        .into_iter()
                        .map(|(instruction, object_call, blocking, paused, counter)| {
                            serde_json::json!({
                                "instruction": instruction,
                                "object_call": object_call,
                                "blocking": blocking,
                                "paused": paused,
                                "counter": counter,
                            })
                        })
                        .collect::<Vec<_>>()
                }),
                "failure": actor.failure(),
            })).collect::<Vec<_>>()
        })
    }
    /// Bounded identity, health and position diagnostics without VM/task serialization.
    pub fn inspect_summary(&self, offset: usize, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "world": self.world,
            "generation": self.generation,
            "total": self.actors.len(),
            "actors": self.actors.iter().enumerate().skip(offset).take(limit.min(64)).map(|(index, actor)| {
                let position = self.spatial.as_ref().and_then(|spatial| spatial.actor(index).ok()).map(|pose| pose.local.position);
                serde_json::json!({
                    "id": ScriptActorId { generation: self.generation, index },
                    "name": actor.name(),
                    "object_id": actor.placement().map(|placement| placement.object_id),
                    "health": actor.attributes[Attribute::Health],
                    "position": position,
                })
            }).collect::<Vec<_>>()
        })
    }
    pub fn flags(&self) -> &[i32] {
        &self.flags
    }
    /// Capture ordered global values and latches without running scripts.
    /// The snapshot includes installed definitions for compatibility validation.
    pub fn snapshot_flags(&self) -> super::GlobalFlagsSnapshot {
        self.flag_state.snapshot(&self.flags)
    }

    /// Validate all flag definitions before replacing values and latches together.
    /// No native operations, initialization or effects run during restoration.
    /// World/actor restoration and cross-world retention remain host-owned.
    pub fn restore_flags(&mut self, snapshot: super::GlobalFlagsSnapshot) -> Result<()> {
        self.flag_state
            .restore(&mut self.flags, snapshot)
            .with_context(|| format!("world {} global flag restoration", self.world))
    }

    /// Read the global flag's latch without applying its native set/reset operation.
    pub fn global_flag_is_set(&self, index: usize) -> Result<bool> {
        anyhow::ensure!(
            index < self.flags.len(),
            "world {}: global flag {index} absent",
            self.world
        );
        Ok(self.flag_state.is_set(index))
    }
    /// Assign a raw value and optionally mark the latch, as the console does.
    /// A false `mark_set` preserves the latch; it does not clear it.
    pub fn assign_global_flag(&mut self, index: usize, value: i32, mark_set: bool) -> Result<()> {
        let slot = self
            .flags
            .get_mut(index)
            .with_context(|| format!("world {}: global flag {index} absent", self.world))?;
        *slot = value;
        if mark_set {
            self.flag_state.mark_set(index);
        }
        Ok(())
    }
    pub fn actors(&self) -> &[ScriptActor] {
        &self.actors
    }
    /// Ordinary updates executed by this world since it was created or restored.
    pub fn ordinary_updates(&self) -> u64 {
        self.ordinary_updates
    }
    /// Handle for an indexed record, including records retained for diagnostics after cleanup.
    /// Operations on a retired created receiver return an unsupported error.
    pub fn id(&self, index: usize) -> Result<ScriptActorId> {
        anyhow::ensure!(
            index < self.actors.len(),
            "world {} instance {index} absent",
            self.world
        );
        Ok(ScriptActorId {
            generation: self.generation,
            index,
        })
    }
    pub fn invoke(
        &mut self,
        actor: ScriptActorId,
        thread: Thread,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<()> {
        self.resolve(actor)?;
        let actor = self
            .actors
            .get_mut(actor.index)
            .context("script instance absent")?;
        let runtime = actor.runtime.as_mut().with_context(|| {
            actor
                .unavailable
                .clone()
                .unwrap_or_else(|| "program absent".into())
        })?;
        runtime.invoke(thread, host, &mut self.flags, budget)
    }
}

/// Session owner. Default construction establishes fresh-session globals;
/// replacing its current world preserves them rather than starting a new session.
#[derive(Resource)]
pub struct ScriptActors {
    /// Retained terminal loop result; unavailable dispatch is reported without resuming simulation.
    pub world_loop_outcome: Option<WorldLoopOutcome>,
    pub current: Option<ScriptWorld>,
    visited: BTreeMap<u32, ScriptWorld>,
    discard_current_on_release: bool,
    /// Global SOUP flags survive ordinary world replacement. New Game clears this snapshot
    /// before releasing the prior world, matching the session initializer's fresh definitions.
    retained_flags: Option<super::GlobalFlagsSnapshot>,
    /// The selected player's health (`attr_health`) when the last world was released. The original
    /// retains it outside the world, copies it into both player records at world entry and resets
    /// it to 100 at New Game (preservation engine/menu.md).
    retained_health: Option<u8>,
    pub(crate) restored_generation: Option<u64>,
    generation: u64,
    requested: Option<Handle<Scripts>>,
    globals: super::GameplayGlobals,
    pub(super) error: Option<String>,
}
impl Default for ScriptActors {
    fn default() -> Self {
        Self {
            world_loop_outcome: None,
            current: None,
            visited: BTreeMap::new(),
            discard_current_on_release: false,
            retained_flags: None,
            retained_health: None,
            restored_generation: None,
            generation: 0,
            requested: None,
            globals: super::GameplayGlobals::new_session(),
            error: None,
        }
    }
}
/// Animation commands capture the receiver's own local Q8 position.
/// Unlike script Sound, they do not substitute linked or attached positions.
fn animation_sound_position(
    spatial: Option<&super::spatial::SpatialWorld>,
    index: usize,
) -> Option<[i32; 3]> {
    Some(spatial?.actor(index).ok()?.local.position)
}

fn apply_player_lockout(
    actors: &mut [ScriptActor],
    activation: &mut super::ActivationState,
    player: Option<usize>,
    lockout: u16,
) {
    if let Some(player) = player {
        actors[player].attributes[Attribute::LockPos] = u8::from(lockout == 1);
    }
    if lockout != 1 {
        for actor in actors
            .iter_mut()
            .filter(|actor| actor.runtime_identifier.is_some())
        {
            actor.attributes[Attribute::LockPos] = 0;
        }
        activation.clear_prompt_override();
    }
}

impl ScriptActors {
    /// SYSTEM.INI `[debug] final_version` of the current world, which enables the gameplay book
    /// shortcuts (preservation engine/menu.md).
    pub fn final_version(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|world| world.updates.final_version)
    }

    /// Run after death confirmation or the book returns, before loading another game. Clear only
    /// hit inhibition, retaining the override group, death progress and falling timer.
    /// A replacement session must not apply this operation to its restored state.
    pub fn finish_death_book(&mut self) -> Result<()> {
        let globals = self
            .current
            .as_mut()
            .map_or(&mut self.globals, |world| &mut world.globals);
        globals
            .selected_health
            .as_mut()
            .context("death book return: selected health unavailable")?
            .set_hit_inhibited(false);
        self.world_loop_outcome = None;
        Ok(())
    }

    /// Apply New Game's lockout cleanup, combat-queue reset and selected-player reset (CYRUS selected,
    /// as the session initializer clears the swap state) before world replacement.
    /// Ordinary world reloads retain globals and must not call this operation.
    pub fn begin_new_game(&mut self) {
        self.world_loop_outcome = None;
        self.retained_flags = None;
        self.retained_health = None;
        if let Some(world) = self.current.as_mut() {
            let player = world.selected_player_index();
            let lockout = world.globals.lockout_player(0);
            apply_player_lockout(&mut world.actors, &mut world.activation, player, lockout);
            world.globals.combat_messages.reset();
            world.globals.player_swapped = false;
        } else {
            self.globals.lockout_player(0);
            self.globals.combat_messages.reset();
            self.globals.player_swapped = false;
        }
        self.discard_current_on_release = self.current.is_some();
        self.visited.clear();
    }

    fn release_world(&mut self) {
        self.restored_generation = None;
        if let Some(mut world) = self.current.take() {
            let teardown_error = if self.discard_current_on_release {
                world
                    .cancel_native_tasks()
                    .err()
                    .map(|error| format!("world {} native task teardown: {error:#}", world.world))
            } else {
                None
            };
            self.globals = std::mem::take(&mut world.globals);
            // The original's world teardown clears every dialogue record, so a line active at the
            // switch must not keep a speaker from the released world.
            if let Some(dialogue) = self.globals.dialogue.as_mut() {
                dialogue.reset_active_lines();
            }
            if !self.discard_current_on_release {
                self.retained_flags = Some(world.snapshot_flags());
                // shortcut: read at release; the original mirrors it at each health change, so a raw
                // SetAttribute(3) from a script persists here but not there. Upgrade with evidence.
                self.retained_health = world
                    .selected_player_index()
                    .map(|player| world.actors[player].attributes[Attribute::Health]);
                // An unfinished revisit must not overwrite its retained actor state.
                self.visited.entry(world.world).or_insert(world);
            }
            self.discard_current_on_release = false;
            if let Some(error) = teardown_error {
                self.error.get_or_insert(error);
            }
        }
    }

    /// Record a host failure without replacing the first failure in this world.
    /// World preparation clears it alongside the logical registry.
    pub fn report_runtime_error(&mut self, error: String) {
        self.error.get_or_insert(error);
    }
}

pub(super) fn prepare(
    prepared: Res<PreparedWorld>,
    assets: Res<Assets<Scripts>>,
    animations: Res<Assets<Animations>>,
    mut actors: ResMut<ScriptActors>,
    clock: Option<Res<crate::clock::SimulationClock>>,
    mut commands: Commands,
) {
    // A replacement registry adopts its already validated world without entry work.
    if actors.restored_generation.is_some() && actors.requested.is_none() {
        let (Some(scripts), Some(animation_data)) = (
            assets.get(&prepared.scripts),
            animations.get(&prepared.animations),
        ) else {
            return;
        };
        let compatible = actors.current.as_ref().is_some_and(|world| {
            world.world == scripts.world
                && world.content_id == scripts.content_id
                && prepared.world.is_none_or(|id| id == world.world)
                && animation_data.world == world.world
                && animation_data.metadata["script_content_id"].as_str()
                    == Some(world.content_id.as_str())
        });
        if compatible {
            actors.requested = Some(prepared.scripts.clone());
        } else {
            actors.report_runtime_error(
                "restored registry does not match prepared world assets".into(),
            );
        }
        return;
    }
    if prepared.is_changed() {
        actors.error = None;
        actors.release_world();
        actors.requested = Some(prepared.scripts.clone());
        let Some(generation) = actors.generation.checked_add(1) else {
            actors.error = Some("script world generation exhausted".into());
            actors.requested = None;
            return;
        };
        actors.generation = generation;
    }
    if actors.current.is_some() || actors.requested.as_ref() != Some(&prepared.scripts) {
        return;
    }
    let Some(scripts) = assets.get(&prepared.scripts) else {
        return;
    };
    let Some(animation_data) = animations.get(&prepared.animations) else {
        return;
    };
    let expected_world = prepared.world.unwrap_or(animation_data.world);
    let result = (|| {
        anyhow::ensure!(
            expected_world == scripts.world && animation_data.world == scripts.world,
            "world {} script identity differs from prepared world {}",
            scripts.world,
            expected_world
        );
        anyhow::ensure!(
            animation_data.metadata["script_content_id"].as_str() == Some(&scripts.content_id),
            "world {} animation/script cache generation mismatch; re-export the world",
            scripts.world
        );
        if let Some(clock) = &clock {
            anyhow::ensure!(
                clock.settings() == scripts.clock,
                "world {} clock settings differ from the active game's installed settings",
                scripts.world
            );
        }
        actors.globals.prepare_dialogue(&scripts.dialogue)?;
        let mut world = ScriptWorld::prepare(scripts, actors.generation)?;
        if let Some(snapshot) = actors.retained_flags.clone() {
            world.restore_flags(snapshot)?;
        }
        if let Some(health) = actors.retained_health {
            for actor in world.actors.iter_mut().filter(|actor| {
                matches!(
                    actor.source,
                    ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                )
            }) {
                actor.attributes[Attribute::Health] = health;
            }
        }
        world.prepare_markers(animation_data)?;
        world.prepare_navigation(animation_data)?;
        world.prepare_ropes(animation_data)?;
        if clock.is_none() {
            commands.insert_resource(crate::clock::SimulationClock::new(scripts.clock)?);
        }
        Ok(world)
    })();
    let result = result.and_then(|mut world| {
        world.globals = std::mem::take(&mut actors.globals);
        // A retained swap state rebinds the world's camera to the selected role, as the swap
        // itself does; the fresh camera starts bound to CYRUS.
        let has_alternate = world
            .actors
            .iter()
            .any(|actor| matches!(actor.source, ScriptActorSource::AlternatePlayer));
        if world.globals.player_swapped()
            && has_alternate
            && let Err(error) = world.select_player(true)
        {
            actors.globals = std::mem::take(&mut world.globals);
            return Err(error);
        }
        Ok(world)
    });
    match result {
        Ok(mut world) => {
            actors.retained_flags = None;
            actors.retained_health = None;
            // A retained swap state excludes the other role before the first update, including
            // the model presentation that runs ahead of the first world-loop admission.
            reassert_player_update_exclusion(&mut world.actors, &world.globals);
            world.globals.sound_maintenance.enter_world();
            world.globals.world_loop_frame = 0;
            world.globals.group_sync = Default::default();
            world.globals.group_sync.register_actors(&world.actors);
            actors.world_loop_outcome = None;
            actors.current = Some(world);
        }
        Err(error) => {
            actors.error = Some(format!(
                "world {} script preparation: {error:#}",
                scripts.world
            ));
            actors.requested = None;
        }
    }
}

struct GameHost<'a, 'sound, 'input, 'random> {
    native_source_program: Option<std::sync::Arc<cyrus_soup::program::Program>>,
    registry: &'a mut ScriptWorld,
    world: u32,
    animation_system: bool,
    task_system: bool,
    inputs: ScriptInputs<'input>,
    index: usize,
    generation: u64,
    sounds: Option<&'sound mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
    random: Option<&'random mut cyrus_game::random::EngineRandom>,
    ordinary_actors: Option<&'a [ScriptActorId]>,
    walk_map_count: Option<usize>,
}
impl<'input> GameHost<'_, '_, 'input, '_> {
    /// The selected player's step check, then the water and lava transitions its support query
    /// returned (the original's surface handler runs inside that query).
    fn player_step_clear_with_transition(
        &mut self,
        id: ScriptActorId,
        ground: ground::OrdinaryGroundQuery<'_>,
        direction: StepDirection,
    ) -> Result<bool> {
        let clear = self.registry.player_step_clear(id, ground, direction);
        if !self.registry.pending_surface_transitions.is_empty() {
            let ticks = self.inputs.nominal_ticks.unwrap_or(0);
            let random = self
                .random
                .as_deref_mut()
                .context("surface transition random owner unavailable")?;
            let sounds = self
                .sounds
                .as_deref_mut()
                .context("surface transition sound handler unavailable")?;
            self.registry
                .flush_surface_transitions(id, ticks, random, sounds)?;
        }
        clear
    }

    /// The shared native-command source: the actor whose ordinary interpreter entry ran last. It is
    /// the original's shared source actor reference, read by `MoveAboveMe` and `PointAt`
    /// independently of their receiver. Unknown before any such entry, and a source retained from
    /// a replaced world names a departed actor; both are explicit errors.
    fn native_source_actor(&self, native: &str) -> Result<ScriptActorId> {
        match self.registry.globals.native_source {
            super::activation::NativeSource::Actor { generation, index }
                if generation == self.generation =>
            {
                self.registry.id(index)
            }
            super::activation::NativeSource::Actor { .. } => {
                anyhow::bail!("{native}: the retained native source belongs to a replaced world")
            }
            super::activation::NativeSource::Unknown => {
                anyhow::bail!("{native}: no interpreter entry has established the native source")
            }
        }
    }

    /// Mask 2 of the flag byte of `WaitLineUp`'s argument actor. An actor without a control
    /// record never had the flag set, since only a lineup sets it, so it reads clear.
    fn argument_alignment_flag(&self, arguments: &[Argument]) -> Result<bool> {
        let [Argument::Actor(actor)] = arguments else {
            anyhow::bail!("WaitLineUp requires one actor reference")
        };
        let index = self
            .registry
            .resolve(*actor)
            .context("WaitLineUp argument actor")?;
        Ok(self.registry.actors[index]
            .control
            .as_ref()
            .is_some_and(|control| control.script_control & 2 != 0))
    }

    fn native_task_call(
        &mut self,
        (function, mode): (Native, super::tasks::NativeTaskMode),
        arguments: &[Argument],
        invocation: cyrus_soup::Invocation,
    ) -> Result<Option<CallResult>> {
        let kind = mode.kind();
        let alignment_flag = (function == Native::WaitLineUp)
            .then(|| self.argument_alignment_flag(arguments))
            .transpose()?;
        if matches!(mode, super::tasks::NativeTaskMode::Script(_))
            && super::tasks::NativeTasks::is_lineup_function(function)
        {
            return self.lineup_call(function, kind, arguments, invocation);
        }
        if matches!(mode, super::tasks::NativeTaskMode::Script(_))
            && super::tasks::NativeTasks::is_bounce_function(function)
        {
            return self.bounce_call(function, kind, arguments, invocation);
        }
        if matches!(mode, super::tasks::NativeTaskMode::Script(_))
            && super::tasks::NativeTasks::is_point_function(function)
        {
            return self.point_call(function, kind, arguments, invocation);
        }
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let ambient = self.ambient_call();
        let player_dialogue = self.player_dialogue_call();
        let frame_wait = self.frame_wait_call();
        let operation = match mode {
            super::tasks::NativeTaskMode::SynchronousMovement(operation) => Some(operation),
            super::tasks::NativeTaskMode::Script(_)
                if kind != CallKind::Function
                    && arguments.len() == 1
                    && arguments[0].integer().is_ok() =>
            {
                super::tasks::LocomotionOperation::named(function)
            }
            super::tasks::NativeTaskMode::Script(_) => None,
        };
        let locomotion_admission = self.locomotion_probes(operation)?;
        let locomotion_state = self.locomotion_state();
        let player_face_requested = function == Native::PlayerFaceObj;
        let player_face = player_face_requested.then(|| self.player_face_state());
        let mut player_face_outcome = super::tasks::PlayerFaceOutcome::default();
        let camera_update_handle = self.inputs.camera_update;
        let camera_inputs = super::tasks::NativeTasks::camera_function(function).then(|| {
            (
                self.camera_slot_request(),
                self.camera_bound_pose(),
                self.camera_targets(),
            )
        });
        let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
        let surface = self.registry.actors[self.index].surface;
        // RTXpAnim animates the selected player, who emits animation sounds under their own identity.
        let player_slot = self.selected_player().filter(|player| {
            player.generation == self.generation
                && player.index != self.index
                && player.index < self.registry.actors.len()
        });
        let player_sound = player_slot.map(|player| {
            (
                player,
                animation_sound_position(self.registry.spatial.as_ref(), player.index),
                self.registry.actors[player.index].surface,
            )
        });
        let animation_waits = self.registry.animation_waits();
        let actor_events = effects::ActorEventContext::capture(self.registry, id);
        let player_events =
            player_slot.map(|player| effects::ActorEventContext::capture(self.registry, player));
        // Camera tasks own the camera instead of animation playback; animation tasks
        // share these local dispatch borrows between receiver and selected-player cursors.
        let (event_camera, task_camera) = if camera_inputs.is_some() {
            (None, self.registry.camera.as_mut())
        } else {
            (self.registry.camera.as_mut(), None)
        };
        let event_services = std::cell::RefCell::new((event_camera, &mut self.registry.effects));
        let sound_handler = std::cell::RefCell::new(&mut self.sounds);
        let mut emit = |sound| {
            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                let (camera, effects) = &mut *event_services.borrow_mut();
                return actor_events.dispatch(flags, camera.as_deref_mut(), effects);
            }
            match &mut **sound_handler.borrow_mut() {
                Some(sounds) => sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                ),
                None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
            }
        };
        let mut emit_player = |sound| {
            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                let context = player_events
                    .as_ref()
                    .context("selected player actor event unavailable")?;
                let (camera, effects) = &mut *event_services.borrow_mut();
                return context.dispatch(flags, camera.as_deref_mut(), effects);
            }
            match (&mut **sound_handler.borrow_mut(), player_sound) {
                (Some(sounds), Some((player, position, surface))) => sounds(
                    player,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                ),
                _ => anyhow::bail!("selected player animation sound handler absent"),
            }
        };
        let user_channel = self.registry.actors[self.index].attributes[Attribute::User];
        let user_has_active = self
            .registry
            .globals
            .group_sync
            .user_has_active(&self.registry.actors, user_channel);
        let (actor, player_actor) = match player_slot.and_then(|player| {
            self.registry
                .actors
                .get_disjoint_mut([self.index, player.index])
                .ok()
        }) {
            Some([actor, player]) => (actor, Some(player)),
            None => (&mut self.registry.actors[self.index], None),
        };
        let light_position = actor
            .placement()
            .map(|placement| placement.map_position.map(|value| value as i32))
            .unwrap_or_default();
        let mut animation =
            actor
                .animation
                .as_mut()
                .zip(self.inputs.nominal_ticks)
                .map(|(animation, ticks)| super::tasks::AnimationCall {
                    animation: &mut animation.playback,
                    ticks,
                    frame: &mut animation.frame,
                    sounds: Some(&mut emit),
                });
        let player_animation = player_actor
            .and_then(|player| player.animation.as_mut())
            .zip(self.inputs.nominal_ticks)
            .map(|(animation, ticks)| super::tasks::AnimationCall {
                animation: &mut animation.playback,
                ticks,
                frame: &mut animation.frame,
                sounds: Some(&mut emit_player),
            });
        let player_position = self
            .inputs
            .player_query
            .as_ref()
            .and_then(|query| query.player.as_ref())
            .map(|player| player.position);
        let mut spatial_call = self
            .registry
            .spatial
            .as_mut()
            .and_then(|spatial| spatial.actor_mut(self.index).ok())
            .zip(self.inputs.frame_scale.zip(self.inputs.nominal_ticks))
            .map(
                |(spatial, (frame_scale, nominal_ticks))| super::tasks::SpatialCall {
                    matrix: spatial.local.matrix,
                    locations: &spatial.locations,
                    markers: self.registry.markers.as_ref(),
                    effective_position: spatial.effective.map(|pose| pose.position),
                    player_position,
                    slave: actor.attributes[Attribute::Slave] != 0,
                    actor_index: self.index,
                    map_updates: &mut self.registry.map_updates,
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    turn: self.registry.globals.turn.delta(frame_scale),
                    nominal_ticks,
                    tick: self.inputs.combat_clock_sample,
                },
            );
        let menu_input = menu_input(&self.inputs, self.registry.globals.dialogue.as_ref());
        let mut gxa_words = {
            let command = &self.registry.globals.native_command;
            (command.continuation, command.continuation_direction as i32)
        };
        let gxa_before = gxa_words;
        let mut light_effect = |function, arguments: &[Argument]| {
            light::LightState::effect(
                &mut actor.light,
                light_position,
                function,
                arguments,
                &mut self.registry.globals.native_command,
                self.inputs.nominal_ticks,
                self.random.as_deref_mut(),
            )
        };
        let services = super::tasks::NativeServices {
            light_effect: Some(&mut light_effect),
            animation_requests_enabled: self.animation_system
                && !actor
                    .control
                    .as_ref()
                    .is_some_and(|control| control.object_update_excluded),
            ambient: Some(&ambient),
            dialogue: self.registry.globals.dialogue.as_mut(),
            menu: Some(super::tasks::MenuCall {
                state: &mut self.registry.globals.menu,
                active: &mut self.registry.globals.menu_state,
                input: menu_input,
            }),
            player_dialogue: Some(player_dialogue),
            player_animation,
            group_sync: Some(super::tasks::GroupSyncCall {
                state: &mut self.registry.globals.group_sync,
                actor_index: self.index,
                channel: actor.attributes[Attribute::Group],
                user_channel,
                user_has_active,
            }),
            frame_wait: Some(frame_wait),
            locomotion: Some(super::tasks::LocomotionCall {
                state: locomotion_state,
                admission: locomotion_admission,
                animation_system: animation_waits,
                motion: actor
                    .control
                    .as_mut()
                    .map(|control| super::tasks::MotionCall {
                        classification: &mut control.motion_classification,
                        direction: &mut control.motion_direction,
                        counters: &mut control.move_counter,
                    }),
                called: &mut self.registry.globals.locomotion_called,
            }),
            camera: camera_inputs.and_then(
                |(requested_slot, bound_pose, (receiver, selected_player))| {
                    task_camera.map(|state| super::tasks::CameraCall {
                        state,
                        requested_slot,
                        bound_pose,
                        receiver,
                        selected_player,
                        update: camera_update_handle,
                    })
                },
            ),
            player_face: player_face.map(|state| super::tasks::PlayerFaceCall {
                state,
                outcome: &mut player_face_outcome,
            }),
            lineup: None,
            bounce: None,
            point: None,
            gxa_overlay: Some(&mut self.registry.gxa_overlay),
            alignment_flag,
            gxa_animation: self
                .inputs
                .nominal_ticks
                .map(|ticks| super::tasks::GxaAnimation {
                    resource: &mut gxa_words.0,
                    progress: &mut gxa_words.1,
                    ticks,
                }),
        };
        let result = match mode {
            super::tasks::NativeTaskMode::Script(kind) => actor.tasks.call(
                (function, kind),
                arguments,
                invocation,
                &mut animation,
                &mut spatial_call,
                services,
            ),
            super::tasks::NativeTaskMode::SynchronousMovement(operation) => {
                actor.tasks.call_synchronous_movement(
                    operation,
                    arguments,
                    invocation,
                    &mut animation,
                    &mut spatial_call,
                    services,
                )
            }
        };
        if gxa_words != gxa_before {
            let command = &mut self.registry.globals.native_command;
            command.continuation = gxa_words.0;
            command.continuation_direction = gxa_words.1 as u32;
        }
        self.finish_camera_update(result.is_ok())?;
        if result.is_ok() && player_face_requested {
            self.apply_player_face(player_face_outcome)?;
        }
        result
    }

    /// Apply the logical camera actor positions the location natives' pose update published, once
    /// the native succeeded; a failed native discards them.
    fn finish_camera_update(&mut self, succeeded: bool) -> Result<()> {
        let Some(service) = self.inputs.camera_update else {
            return Ok(());
        };
        let published = service.take_published();
        if !succeeded {
            return Ok(());
        }
        self.registry.apply_camera_publication(published)
    }

    fn actor_is_dead(&self, index: usize) -> Result<bool> {
        self.registry
            .query_is_dead(self.registry.id(index)?, self.selected_player())
    }

    fn fighting_position(&self, index: usize) -> Result<[i32; 3]> {
        self.registry.query_position(self.registry.id(index)?)
    }

    fn is_fighting(&self) -> Result<bool> {
        let traversal = self
            .ordinary_actors
            .context("isFighting: ordinary traversal unavailable")?;
        let receiver = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let mut nearest = None;
        let mut bound = 65535;
        for &candidate in traversal {
            anyhow::ensure!(
                candidate.generation == self.generation
                    && candidate.index < self.registry.actors.len(),
                "isFighting: stale ordinary actor {candidate:?}"
            );
            if candidate == receiver {
                continue;
            }
            let Some(combat) = self.registry.actors[candidate.index]
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
            else {
                continue;
            };
            if combat.target != Some(receiver)
                || combat.attacker_selection_state != CombatControlState::COORDINATION_ACTIVE
                || self.actor_is_dead(candidate.index)?
            {
                continue;
            }
            let origin = self.fighting_position(self.index)?;
            let position = self.fighting_position(candidate.index)?;
            let differences = std::array::from_fn(|axis| position[axis].wrapping_sub(origin[axis]));
            let distance = cyrus_collision::distance::rounded_distance(differences)?;
            if distance < bound {
                bound = distance;
                nearest = Some(differences);
            }
        }
        let Some([x, _, z]) = nearest else {
            return Ok(false);
        };
        let threshold = self
            .registry
            .in_combat_threshold
            .context("isFighting: installed combat threshold unavailable")?;
        Ok(cyrus_collision::distance::rounded_distance([x, 0, z])? < threshold)
    }

    fn ambient_call(&self) -> super::tasks::AmbientCall<'input> {
        let actor = &self.registry.actors[self.index];
        let position = self
            .registry
            .query_position(ScriptActorId {
                generation: self.generation,
                index: self.index,
            })
            .ok();
        super::tasks::AmbientCall {
            listener: self.registry.sound_listener(),
            driver: self.inputs.ambient_sound,
            reset: self.inputs.sound_reset,
            position,
            excluded: actor
                .control
                .as_ref()
                .map(|control| control.object_update_excluded),
            killed: actor.control.as_ref().map(|control| control.killed),
            generation: self.generation,
            actor_index: self.index,
        }
    }

    fn selected_player(&self) -> Option<ScriptActorId> {
        self.inputs
            .activation
            .map(|inputs| inputs.player)
            .or_else(|| {
                self.registry
                    .selected_player_index()
                    .map(|index| ScriptActorId {
                        generation: self.generation,
                        index,
                    })
            })
    }

    /// Camera slot `showPlayer` requests for the selected player; fallible reads
    /// surface only when the task uses them.
    fn camera_slot_request(&self) -> Result<Option<u8>, String> {
        let describe = |error: anyhow::Error| format!("{error:#}");
        let player = self
            .selected_player()
            .ok_or("showPlayer selected player unavailable")?;
        let index = self.registry.resolve(player).map_err(describe)?;
        let control = self.registry.actors[index]
            .control
            .as_ref()
            .ok_or("showPlayer selected-player control unavailable")?;
        let (holding_weapon, _, _) = self
            .actor_weapon_predicates(index, Native::IsHoldingWeapon)
            .map_err(describe)?;
        Ok(super::camera::player_slot(
            holding_weapon,
            false,
            control.living_state,
            control.rope_attached,
        ))
    }

    /// The receiver and the selected player as camera native targets.
    fn camera_targets(
        &self,
    ) -> (
        Result<super::tasks::CameraTarget, String>,
        Result<super::tasks::CameraTarget, String>,
    ) {
        let receiver = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        (
            self.camera_target(Some(receiver), false),
            self.camera_target(self.selected_player(), true),
        )
    }

    /// An actor as a camera native's target: its camera attributes, the pose the camera binds to
    /// (`camera_binding_pose`) and the camera-wide reads the natives share. Fallible reads
    /// surface only when used.
    fn camera_target(
        &self,
        actor: Option<ScriptActorId>,
        guard_weapon: bool,
    ) -> Result<super::tasks::CameraTarget, String> {
        let describe = |error: anyhow::Error| format!("{error:#}");
        let id = actor.ok_or("camera target actor unavailable")?;
        let index = self.registry.resolve(id).map_err(describe)?;
        let attributes = &self.registry.actors[index].attributes;
        let pose = self.registry.camera_binding_pose(index).map_err(describe);
        let selected_bound_position = (|| {
            let camera = self
                .registry
                .camera
                .as_ref()
                .context("camera session unavailable")?;
            let bound = self
                .registry
                .resolve(camera.records[camera.selected.index()].actor)?;
            Ok(self.registry.camera_binding_pose(bound)?.0)
        })()
        .map_err(describe);
        let holding_weapon = if guard_weapon {
            self.actor_weapon_predicates(index, Native::IsHoldingWeapon)
                .map(|(holding, _, _)| holding)
                .map_err(describe)
        } else {
            Ok(false)
        };
        Ok(super::tasks::CameraTarget {
            actor: id,
            camera_move: attributes[Attribute::CameraMove],
            pan_type: attributes[Attribute::CameraPanType],
            offsets: [
                Attribute::CameraTargetOffsetX,
                Attribute::CameraTargetOffsetY,
                Attribute::CameraTargetOffsetZ,
                Attribute::CameraOffsetX,
                Attribute::CameraOffsetY,
                Attribute::CameraOffsetZ,
            ]
            .map(|attribute| attributes[attribute]),
            pose,
            selected_bound_position,
            holding_weapon,
        })
    }

    /// The selected player's pose to which the camera binds (`camera_binding_pose`); fallible
    /// reads surface only when used.
    fn camera_bound_pose(&self) -> Result<([i32; 3], [u16; 3]), String> {
        let describe = |error: anyhow::Error| format!("{error:#}");
        let player = self
            .selected_player()
            .ok_or("camera bound selected player unavailable")?;
        let index = self.registry.resolve(player).map_err(describe)?;
        self.registry.camera_binding_pose(index).map_err(describe)
    }

    /// The selected player's state for `PlayerFaceObj`; fallible reads surface only when the task
    /// uses them.
    fn player_face_state(&self) -> Result<super::tasks::PlayerFaceState, String> {
        let describe = |error: anyhow::Error| format!("{error:#}");
        let player = self
            .selected_player()
            .ok_or("PlayerFaceObj selected player unavailable")?;
        let index = self.registry.resolve(player).map_err(describe)?;
        if index == self.index {
            return Err("PlayerFaceObj receiver is the selected player".into());
        }
        let actor = &self.registry.actors[index];
        let control = actor
            .control
            .as_ref()
            .ok_or("PlayerFaceObj selected-player control unavailable")?;
        let animation = actor
            .animation
            .as_ref()
            .ok_or("PlayerFaceObj selected-player animation unavailable")?;
        let pose = self
            .registry
            .spatial
            .as_ref()
            .ok_or("PlayerFaceObj spatial state not prepared")?
            .actor(index)
            .map_err(describe)?
            .local;
        let (holding_weapon, _, _) = self
            .actor_weapon_predicates(index, Native::IsHoldingWeapon)
            .map_err(describe)?;
        let combat_mode = control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags.contains(CombatFlags::STANCE));
        Ok(super::tasks::PlayerFaceState {
            position: pose.position,
            angles: pose.wrapped_angles(),
            speed: super::alignment::speed(
                animation.playback.current_group(),
                Some(&actor.attributes),
            ),
            rate_attribute: actor.attributes[if holding_weapon || combat_mode {
                Attribute::FightTurnSpeed
            } else {
                Attribute::TurnSpeed
            }],
            holding_weapon,
            torch: control.torch_burning != 0,
            response_present: !control.last_collide.is_empty(),
            response_age: control.last_collide_count,
        })
    }

    /// Apply a successful `PlayerFaceObj` call's changes to the selected player.
    fn apply_player_face(&mut self, outcome: super::tasks::PlayerFaceOutcome) -> Result<()> {
        let player = self
            .selected_player()
            .context("PlayerFaceObj selected player unavailable")?;
        let index = self.registry.resolve(player)?;
        let spatial = self
            .registry
            .spatial
            .as_mut()
            .context("PlayerFaceObj spatial state not prepared")?
            .actor_mut(index)?;
        let mut refresh = false;
        if let Some(angles) = outcome.angles {
            refresh = spatial.local.angles != angles.map(i32::from);
            spatial.local.angles = angles.map(i32::from);
        }
        if let Some(position) = outcome.position {
            spatial.local.position = position;
            self.registry.map_updates.register(index)?;
        }
        if refresh {
            spatial.refresh_transform()?;
        }
        // The alignment flag is the hook-disabled bit `DisableHook` also sets, so the player's
        // controller hook stays suppressed while the player is still being aligned.
        let control = self.registry.actors[index]
            .control
            .as_mut()
            .context("PlayerFaceObj selected-player control unavailable")?;
        if outcome.aligning {
            control.script_control |= 2;
        } else {
            control.script_control &= !2;
        }
        if let Some(state) = outcome.return_state {
            self.registry.globals.native_command_return = state;
        }
        Ok(())
    }

    /// `DrawSword`'s action for the receiver `id`.
    fn draw_sword(&mut self, id: ScriptActorId) -> Result<()> {
        let selected_player = self
            .selected_player()
            .context("DrawSword selected player unavailable")?;
        let weapon = WeaponInputs {
            selected_player,
            item_system_ready: self
                .inputs
                .hud
                .context("DrawSword inventory owner unavailable")?
                .inventory_ready()?,
            shell_execution: self
                .inputs
                .shell_execution
                .context("DrawSword shell context unavailable")?,
        };
        let ticks = self
            .inputs
            .nominal_ticks
            .context("DrawSword nominal ticks unavailable")?;
        let sounds = self
            .sounds
            .as_deref_mut()
            .context("DrawSword sound handler unavailable")?;
        self.registry
            .admit_actor_weapon_draw(id, weapon, ticks, sounds)
    }

    /// `SheathSword`'s action: when the receiver can sheathe it enters the sheathing state. The
    /// selected player also refreshes the HUD health timer and returns to camera slot zero.
    fn sheath_sword(&mut self, function: Native, id: ScriptActorId) -> Result<()> {
        let selected = self.selected_player() == Some(id);
        let (holding, drawing, sheathing) = self.receiver_weapon_predicates(function)?;
        let actor = &mut self.registry.actors[self.index];
        let Some(control) = actor.control.as_mut() else {
            return Ok(());
        };
        let eligible = self.task_system
            && actor.animation.is_some()
            && super::combat::sheath_eligible(control, holding, drawing, sheathing);
        if !eligible {
            return Ok(());
        }
        super::combat::begin_sheathing(control);
        if selected {
            // Preservation refreshes the selected player's health display for 25
            // ticks when sheathing. This is a HUD timer, distinct from the
            // logical candle-show request used by weapon cleanup.
            self.inputs
                .hud
                .context("SheathSword selected-player HUD unavailable")?
                .request_health_feedback();
            if let Some(camera) = self.registry.camera.as_mut() {
                camera.select(0)?;
            }
        }
        Ok(())
    }

    /// Receiver state for locomotion tasks; fallible reads surface only when a task uses them.
    fn locomotion_state(&self) -> Result<super::locomotion::BackwardState, String> {
        let actor = &self.registry.actors[self.index];
        let control = actor
            .control
            .as_ref()
            .ok_or("locomotion control state unavailable")?;
        let (holding_weapon, _, _) = self
            .receiver_weapon_predicates(Native::MoveBackward)
            .map_err(|error| format!("{error:#}"))?;
        Ok(super::locomotion::BackwardState {
            rope_attached: control.rope_attached,
            sliding_state: control.sliding_state,
            falling_state: control.falling_state,
            queued_motion: control.queued_motion,
            living_state: control.living_state,
            holding_weapon,
            combat_mode: control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.flags.contains(CombatFlags::STANCE)),
            surface: actor.surface.unwrap_or_default(),
            torch_burning: control.torch_burning != 0,
            step_adjustment: actor.attributes[Attribute::StepType],
            step_attribute: actor.attributes[Attribute::BackwardSpeed],
            combat_step_attribute: actor.attributes[Attribute::FightBackwardSpeed],
            strafe_step_attribute: actor.attributes[Attribute::FightCircleSpeed],
            strafe_turn_attribute: actor.attributes[Attribute::FightCircleDeltaAngle],
            lateral_step_attribute: actor.attributes[Attribute::SideSpeed],
            forward_step_attribute: actor.attributes[Attribute::ForwardSpeed],
            forward_combat_step_attribute: actor.attributes[Attribute::FightForwardSpeed],
            lunge_step_attribute: actor.attributes[Attribute::FightLungeSpeed],
            walk_step_attribute: actor.attributes[Attribute::ForwardWalk],
            walk_combat_step_attribute: actor.attributes[Attribute::FightForwardWalk],
            walk_backward_step_attribute: actor.attributes[Attribute::BackwardWalk],
            walk_backward_combat_step_attribute: actor.attributes[Attribute::FightBackwardWalk],
            move_selected: control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.action != CombatControlState::NO_ACTION),
            selected_player_mode_one: self.registry.selected_player_index() == Some(self.index)
                && actor.attributes[Attribute::PlayerType] == 1,
        })
    }

    fn locomotion_probes(
        &mut self,
        operation: Option<super::tasks::LocomotionOperation>,
    ) -> Result<super::tasks::LocomotionAdmission> {
        use super::tasks::LocomotionOperation;
        let mut admission = super::tasks::LocomotionAdmission {
            blocked_ahead_30: self.inputs.blocked_ahead_30,
            forward_step_clear: self.inputs.forward_step_clear,
            lateral_step_clear: self.inputs.lateral_step_clear,
            ..Default::default()
        };
        let Some(operation) = operation else {
            return Ok(admission);
        };
        let state = self.locomotion_state().map_err(anyhow::Error::msg)?;
        let walking =
            matches!(operation, LocomotionOperation::WalkForward) && !state.surface.is_deep_water();
        admission.walk_forward_mode = walking;
        let walking_backward = matches!(operation, LocomotionOperation::WalkBackward)
            && !state.surface.is_deep_water();
        admission.walk_backward_mode = walking_backward;
        admission.forward_admitted = Some(super::locomotion::forward_admitted(&state));
        // Backward and lateral classifications precede their live refusal queries.
        // Dry backward walking instead stores only after its support check succeeds.
        let classification = match operation {
            LocomotionOperation::Backward | LocomotionOperation::WalkBackward
                if !walking_backward
                    && super::locomotion::backward_admission(state, || false)
                        != super::locomotion::BackwardAdmission::Refused =>
            {
                Some(MotionDirection::WALK_BACKWARD)
            }
            LocomotionOperation::Strafe(strafe)
                if super::locomotion::strafe_admission(state, strafe)
                    != super::locomotion::StrafeAdmission::Refused =>
            {
                Some(strafe.motion_direction())
            }
            _ => None,
        };
        if let Some(classification) = classification {
            self.registry.actors[self.index]
                .control
                .as_mut()
                .context("locomotion classification control unavailable")?
                .motion_classification = classification;
        }
        // WalkForward has no caller-sampled substitute for its side-effecting query.
        if walking {
            admission.forward_step_clear = None;
        }
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        // The selected player's scripted walking steps run their step checks through the player
        // support query; the controller's forward walk is its own walking stage.
        let player_backward = walking_backward && self.selected_player() == Some(id);
        let player_forward = walking && self.selected_player() == Some(id);
        if (walking && !player_forward) || (walking_backward && !player_backward) {
            anyhow::ensure!(
                self.selected_player() != Some(id)
                    && !matches!(
                        self.registry.actors[self.index].source,
                        ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                    ),
                "world {} instance {}: selected-player walking step unavailable",
                self.world,
                self.index,
            );
        }
        if walking && admission.forward_admitted == Some(true) {
            let actor = &self.registry.actors[self.index];
            if actor
                .control
                .as_ref()
                .is_some_and(|control| control.ai_type == AiType::POINT_NAVIGATION)
                && actor.attributes[Attribute::Collide] & 1 == 0
            {
                admission.forward_step_clear = Some(true);
                return Ok(admission);
            }
        }
        // A routed call made by an actor without a collision query runs on a receiver that has
        // one: prepare the receiver's own query here.
        let routed_query;
        let geometry = if let Some(geometry) = self.inputs.locomotion_geometry {
            geometry
        } else if self.registry.actors[self.index].attributes[Attribute::Collide] != 0
            && self.registry.actors[self.index].ordinary_collision_role()
            && let Some(source) = self.inputs.player_collision
        {
            routed_query = self.registry.prepare_ordinary_collision_query(
                id,
                source.scene,
                source.animations,
                source.view,
                source.actors,
            )?;
            LocomotionGeometry {
                ground: ground::OrdinaryGroundQuery {
                    scene: source.scene,
                    animations: source.animations,
                    prepared: &routed_query,
                    distance: source.scene.settings.post_collide_height,
                    sidedness: false,
                },
                environment: transition::ObstacleEnvironment {
                    selected_player: self.selected_player(),
                    auto_grab: false,
                    item_system_ready: self
                        .inputs
                        .hud
                        .map(super::ScriptHud::inventory_ready)
                        .transpose()?
                        .unwrap_or(false),
                    shell_execution: self.inputs.shell_execution.unwrap_or(false),
                    ticks: self
                        .inputs
                        .nominal_ticks
                        .context("routed locomotion needs nominal ticks")?,
                },
            }
        } else {
            // Without collision attribute bits no collision query is prepared for the actor and
            // every destination test of the walk step passes.
            if matches!(
                operation,
                LocomotionOperation::WalkForward
                    | LocomotionOperation::Forward
                    | LocomotionOperation::Lunge
            ) && admission.forward_admitted == Some(true)
                && admission.forward_step_clear.is_none()
                && self.registry.actors[self.index].attributes[Attribute::Collide] == 0
            {
                admission.forward_step_clear = Some(true);
            }
            return Ok(admission);
        };
        let obstacle = || obstacle::OrdinaryObstacleQuery {
            scene: geometry.ground.scene,
            animations: geometry.ground.animations,
            prepared: geometry.ground.prepared,
            vertical_offset: 0,
            respond: false,
        };
        match operation {
            LocomotionOperation::WalkBackward => {
                if walking_backward && super::locomotion::walk_backward_admitted(&state) {
                    let mut missing_sound =
                        |_, _| anyhow::bail!("WalkBackward collision sound handler unavailable");
                    let sounds = self.sounds.as_deref_mut().unwrap_or(&mut missing_sound);
                    admission.backward_step_clear = Some(if player_backward {
                        self.player_step_clear_with_transition(
                            id,
                            geometry.ground,
                            crate::script::locomotion::StepDirection::Backward,
                        )?
                    } else {
                        self.registry.ordinary_step_clear(
                            id,
                            crate::script::locomotion::StepDirection::Backward,
                            geometry.ground,
                            geometry.environment,
                            sounds,
                        )?
                    });
                }
            }
            LocomotionOperation::Backward => {
                let mut needed = false;
                super::locomotion::backward_admission(state, || {
                    needed = true;
                    false
                });
                if needed {
                    admission.blocked_ahead_30 =
                        Some(self.registry.blocked_ahead(id, 30, obstacle())?);
                }
            }
            LocomotionOperation::Strafe(strafe) => {
                // Only the ordinary dry stance runs the lateral check; combat strafes skip it.
                let strafe_plan = super::locomotion::strafe_admission(state, strafe);
                if matches!(
                    strafe_plan,
                    super::locomotion::StrafeAdmission::Ledge { .. }
                ) {
                    // The hanging player's lateral check is the ledge scan one step to the side.
                    let fine_ticks = self
                        .inputs
                        .fine_ticks
                        .context("ledge sidestep needs the step's fine-tick increment")?;
                    let environment = self
                        .inputs
                        .locomotion_geometry
                        .map(|geometry| (geometry.ground, geometry.environment));
                    if let Some((ground, environment)) = environment {
                        let timing = ledge_states::LedgeTiming {
                            fine_ticks,
                            frame_scale: self.inputs.frame_scale,
                        };
                        admission.lateral_step_clear = Some(self.registry.ledge_lateral_clear(
                            id,
                            strafe == super::locomotion::Strafe::Right,
                            ground,
                            environment,
                            timing,
                        )?);
                    }
                } else if matches!(
                    strafe_plan,
                    super::locomotion::StrafeAdmission::OutsideCombat { .. }
                        | super::locomotion::StrafeAdmission::DeepWater { .. }
                ) {
                    let direction = match strafe {
                        super::locomotion::Strafe::Left => {
                            crate::script::locomotion::StepDirection::Left
                        }
                        super::locomotion::Strafe::Right => {
                            crate::script::locomotion::StepDirection::Right
                        }
                    };
                    let selected = self.selected_player() == Some(id);
                    let mut missing_sound =
                        |_, _| anyhow::bail!("strafe collision sound handler unavailable");
                    let sounds = self.sounds.as_deref_mut().unwrap_or(&mut missing_sound);
                    // The selected player's step check is its support query, as for its walks.
                    admission.lateral_step_clear = Some(if selected {
                        self.player_step_clear_with_transition(id, geometry.ground, direction)?
                    } else {
                        self.registry.ordinary_step_clear(
                            id,
                            direction,
                            geometry.ground,
                            geometry.environment,
                            sounds,
                        )?
                    });
                    if matches!(
                        strafe_plan,
                        super::locomotion::StrafeAdmission::DeepWater { .. }
                    ) && admission.lateral_step_clear == Some(true)
                    {
                        admission.edge_probe_clear = Some(self.registry.edge_probe_clear(
                            id,
                            strafe.motion_direction(),
                            geometry.ground.scene,
                        )?);
                    }
                }
            }
            LocomotionOperation::Forward
            | LocomotionOperation::WalkForward
            | LocomotionOperation::Lunge => {
                if super::locomotion::forward_admitted(&state) {
                    let control = self.registry.actors[self.index]
                        .control
                        .as_ref()
                        .context("locomotion control unavailable")?;
                    if walking {
                        let mut missing_sound =
                            |_, _| anyhow::bail!("WalkForward collision sound handler unavailable");
                        let sounds = self.sounds.as_deref_mut().unwrap_or(&mut missing_sound);
                        admission.forward_step_clear = Some(if player_forward {
                            self.player_step_clear_with_transition(
                                id,
                                geometry.ground,
                                crate::script::locomotion::StepDirection::Forward,
                            )?
                        } else {
                            self.registry.walk_forward_step_clear(
                                id,
                                geometry.ground,
                                geometry.environment,
                                sounds,
                            )?
                        });
                    } else {
                        if operation != LocomotionOperation::Lunge
                            && super::locomotion::forward_plan(&state)
                                == super::locomotion::ForwardPlan::NeedsEdgeProbe
                        {
                            admission.edge_probe_clear = Some(self.registry.edge_probe_clear(
                                id,
                                super::locomotion::MotionDirection::FORWARD,
                                geometry.ground.scene,
                            )?);
                        }
                        if StepDirection::Forward.counter(control.move_counter) < 3 {
                            admission.forward_step_clear = self.registry.forward_step_clear(
                                id,
                                geometry.environment.selected_player,
                                self.inputs
                                    .frame_scale
                                    .context("MoveForward requires frame_scale")?
                                    as i32,
                                Some(obstacle()),
                            )?;
                        }
                    }
                }
            }
        }
        Ok(admission)
    }

    /// `AddItem` over the session inventory: a closed inventory ignores the call, and an empty
    /// selection repair assigns the selected player's weapon attribute.
    fn add_session_item(&mut self, item: u16, amount: i32) -> Result<()> {
        let hud = self
            .inputs
            .hud
            .context("AddItem inventory host unavailable")?;
        if !hud.inventory_ready()? {
            return Ok(());
        }
        let player = self.selected_player();
        let generation = self.generation;
        let actors = &mut self.registry.actors;
        hud.add_item(item, amount, &mut |weapon| {
            let player = player.context("AddItem fallback selected player unavailable")?;
            anyhow::ensure!(
                player.generation == generation && player.index < actors.len(),
                "AddItem fallback selected player identity is stale"
            );
            actors[player.index].attributes[Attribute::Weapon] = weapon;
            Ok(())
        })
    }

    fn frame_wait_call(&self) -> super::tasks::FrameWaitCall {
        let sample = |index: usize| -> Result<(i32, i32, u8)> {
            self.registry.actors[index]
                .animation
                .as_ref()
                .with_context(|| {
                    format!("actor {index}: animation frame wait requires prepared playback")
                })?
                .playback
                .frame_wait_state()
        };
        let player = (|| -> Result<Option<(i32, i32, u8)>> {
            let Some(player) = self.selected_player() else {
                return Ok(None);
            };
            anyhow::ensure!(
                player.generation == self.generation && player.index < self.registry.actors.len(),
                "animation frame wait selected player identity is stale"
            );
            sample(player.index).map(Some)
        })();
        super::tasks::FrameWaitCall {
            receiver: sample(self.index).map_err(|error| format!("{error:#}")),
            player: player.map_err(|error| format!("{error:#}")),
        }
    }

    fn player_dialogue_call(&self) -> super::tasks::PlayerDialogueCall<'input> {
        let distance_of = |index: usize| -> Result<i32, String> {
            (|| -> Result<i32> {
                let mut inputs = self
                    .inputs
                    .player_query
                    .context("player query inputs unavailable")?;
                let spatial = self
                    .registry
                    .spatial
                    .as_ref()
                    .context("spatial state not prepared")?
                    .actor(index)?;
                inputs.linked_position = Some(spatial.linked_position);
                spatial.speaker_distance(inputs)
            })()
            .map_err(|error| format!("RTX speaker distance: {error:#}"))
        };
        let player = self.selected_player();
        super::tasks::PlayerDialogueCall {
            source: self.inputs.dialogue_source,
            player,
            reset: self.inputs.sound_reset,
            speaker: ScriptActorId {
                generation: self.generation,
                index: self.index,
            },
            speaker_distance: distance_of(self.index),
            player_distance: player
                .ok_or_else(|| "RTXpAnim selected player unavailable".to_string())
                .and_then(|player| distance_of(player.index)),
            player_is_receiver: player.is_some_and(|player| player.index == self.index),
        }
    }

    fn advance_animation(&mut self) -> Result<()> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let ticks = self
            .inputs
            .nominal_ticks
            .context("animation requires nominal_ticks")?;
        let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
        let surface = self.registry.actors[self.index].surface;
        let events = effects::ActorEventContext::capture(self.registry, id);
        // A mask-8 transition is decided by the selected player's forward key (preservation's
        // engine/animation.md "Transition command").
        let selected_player = self.registry.selected_player_index() == Some(self.index);
        let forward_held = self
            .inputs
            .player_controller
            .map(|controller| controller.held.forward);
        // Only a taken mask-8 transition needs the weapon predicate, so its error is deferred.
        let armed = (selected_player && forward_held == Some(true))
            .then(|| self.actor_weapon_predicates(self.index, Native::IsHoldingWeapon))
            .map(|predicates| predicates.map(|(holding, _, _)| holding));
        let mut walk_step: Option<Native> = None;
        let camera = &mut self.registry.camera;
        let effects = &mut self.registry.effects;
        let actor = &mut self.registry.actors[self.index];
        let control = &mut actor.control;
        let animation = actor
            .animation
            .as_mut()
            .context("actor animation not prepared")?;
        let mut emit = |sound| {
            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                events.dispatch(flags, camera.as_mut(), effects)?;
                return Ok(cyrus_world::animation::TransitionDecision::Continue);
            }
            if let cyrus_world::animation::AnimationSound::Transition {
                mask: mask @ (1 | 2),
                ..
            } = sound
            {
                if let Some(control) = control.as_mut()
                    && control.actions.pending & ActionWords::JUMPS != 0
                {
                    let clear = if mask == 1 { 2 } else { 1 };
                    control.actions.iter_mut().for_each(|word| *word &= !clear);
                    control.actions.accepted |= control.actions.pending;
                    return Ok(cyrus_world::animation::TransitionDecision::Take);
                }
                return Ok(cyrus_world::animation::TransitionDecision::Continue);
            }
            if selected_player
                && let cyrus_world::animation::AnimationSound::Transition { mask: 8, .. } = sound
            {
                return match (forward_held, &armed) {
                    (Some(true), Some(Err(error))) => {
                        anyhow::bail!("selected-player mask-8 transition: {error:#}")
                    }
                    (Some(true), holding) => {
                        // A held weapon runs the armed walk step (`anim_fight_run`), otherwise the
                        // ordinary forward step.
                        walk_step = Some(if matches!(holding, Some(Ok(true))) {
                            Native::WalkForward
                        } else {
                            Native::MoveForward
                        });
                        Ok(cyrus_world::animation::TransitionDecision::Yield)
                    }
                    (Some(false), _) => Ok(cyrus_world::animation::TransitionDecision::Continue),
                    (None, _) => anyhow::bail!("selected-player forward key state unavailable"),
                };
            }
            match &mut self.sounds {
                Some(sounds) => sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                ),
                None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
            }
            .map(|_| cyrus_world::animation::TransitionDecision::Continue)
        };
        let frame = animation
            .playback
            .advance_with_decisions(ticks, &mut emit)?;
        animation.frame = frame.or(animation.frame);
        let frame = animation.playback.request_with_decisions(
            animation.playback.default_group(),
            0,
            ticks,
            &mut emit,
        )?;
        animation.frame = frame.or(animation.frame);
        if let Some(function) = walk_step {
            // The taken transition ORs the finished ramp into the movement counters and runs
            // one step; the step forces its group itself.
            // The original sets the animation's stop flag before the group request.
            if let Some(animation) = self.registry.actors[self.index].animation.as_mut() {
                animation.playback.mark_exit();
            }
            if let Some(control) = self.registry.actors[self.index].control.as_mut() {
                control.move_counter |= StepDirection::Forward.counter_mask();
            }
            let sounds = self
                .sounds
                .as_mut()
                .context("selected-player mask-8 transition: animation sound handler absent")?;
            self.registry
                .step_player(id, function, self.inputs, &mut **sounds)?;
        }
        Ok(())
    }

    /// Receiver weapon predicates as (holding, drawing, sheathing); all false while the
    /// inventory or the receiver's animation is not ready.
    /// `IsCombatCapable`, and `PlayerArmed` when `armed`, which also needs the drawn weapon
    /// not to be sheathing. The original's transition lock and world-active globals are
    /// **Inferred** clear and set respectively; cyrus has no transition state yet.
    fn combat_capable(&self, index: usize, armed: bool) -> Result<bool> {
        let actor = &self.registry.actors[index];
        if actor.attributes[Attribute::HoldingWeapon] != 0 {
            return Ok(true);
        }
        let (Some(control), Some(animation)) = (actor.control.as_ref(), actor.animation.as_ref())
        else {
            return Ok(false);
        };
        if control.combat.is_none() {
            return Ok(false);
        }
        if !armed {
            return Ok(true);
        }
        let sheathing = control.living_state == LivingState::SHEATHING_SWORD
            || animation.playback.current_group() == Some(AnimationGroup::SHEATH_SWORD);
        Ok(control.weapon_drawn && !sheathing)
    }

    fn receiver_weapon_predicates(&self, function: Native) -> Result<(bool, bool, bool)> {
        self.actor_weapon_predicates(self.index, function)
    }

    fn actor_weapon_predicates(
        &self,
        index: usize,
        function: Native,
    ) -> Result<(bool, bool, bool)> {
        if function == Native::IsHoldingWeapon
            && self.registry.actors[index].attributes[Attribute::HoldingWeapon] != 0
        {
            return Ok((true, false, false));
        }
        self.weapon_predicates(index)
            .with_context(|| function.to_string())
    }

    /// `IsCarryingWeapon`: the carrying attribute answers before the ordinary gates; otherwise the
    /// receiver's selected item must be defined as a weapon, with its quantity untested.
    fn receiver_carrying_weapon(&self) -> Result<bool> {
        let actor = &self.registry.actors[self.index];
        if actor.attributes[Attribute::HoldingWeapon] != 0 {
            return Ok(true);
        }
        let hud = self.inputs.hud.context("inventory owner unavailable")?;
        let shell = self
            .inputs
            .shell_execution
            .context("shell context unavailable")?;
        if !hud.inventory_ready()? || actor.animation.is_none() || shell {
            return Ok(false);
        }
        let selected = actor.attributes[Attribute::Weapon];
        Ok(selected != 0 && hud.defined_weapon(selected)?)
    }

    /// Holding, drawing and sheathing for an actor, without `IsHoldingWeapon`'s attribute test.
    fn weapon_predicates(&self, index: usize) -> Result<(bool, bool, bool)> {
        let actor = &self.registry.actors[index];
        let ready = self
            .inputs
            .hud
            .context("inventory owner unavailable")?
            .inventory_ready()?;
        if !ready || actor.animation.is_none() {
            return Ok((false, false, false));
        }
        let shell = self
            .inputs
            .shell_execution
            .context("shell context unavailable")?;
        let control = actor
            .control
            .as_ref()
            .context("receiver control unavailable")?;
        Ok(super::activation::weapon_predicates(
            control.living_state,
            actor
                .animation
                .as_ref()
                .map(|animation| super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
            control.combat.is_some(),
            control.weapon_drawn,
            actor.attributes[Attribute::HoldingWeapon],
            ready,
            shell,
        ))
    }

    fn property(&self, target: &Target, reference: u16) -> Result<Property> {
        self.registry
            .references
            .get(usize::from(reference))
            .copied()
            .with_context(|| format!("property {target:?}.{reference}: installed reference absent"))
    }

    fn property_receiver(&self, target: &Target) -> Result<usize> {
        if !matches!(target, Target::Player) {
            return self.target_index(target);
        }
        let player = self
            .selected_player()
            .context("Player property selected player unavailable")?;
        anyhow::ensure!(
            player.generation == self.generation && player.index < self.registry.actors.len(),
            "Player property selected player identity is stale"
        );
        Ok(player.index)
    }

    fn target_reference(&self, target: &Target) -> Result<ScriptActorId> {
        let index = match target {
            Target::Me => Some(self.index),
            Target::Named(name) => self.registry.names.values().copied().find(|&index| {
                self.registry.actors[index]
                    .namespace
                    .as_ref()
                    .is_some_and(|namespace| namespace.eq_ignore_ascii_case(name))
            }),
            Target::Player => self.registry.selected_player_index(),
            Target::Camera => self
                .registry
                .actors
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Camera)),
            Target::Actor(reference) => {
                anyhow::ensure!(
                    reference.generation == self.generation
                        && reference.index < self.registry.actors.len(),
                    "object target {target:?}: stale or absent actor reference"
                );
                Some(reference.index)
            }
        };
        let index = index.with_context(|| format!("object target {target:?}: receiver absent"))?;
        self.registry.id(index)
    }

    fn target_index(&self, target: &Target) -> Result<usize> {
        self.registry
            .resolve(self.target_reference(target)?)
            .with_context(|| format!("object target {target:?}"))
    }
}
/// A surface change a selected-player support query returned: the saved previous surface, the
/// returned one, and the splash effect's Q8 height from that query's support.
#[derive(Clone, Copy, Debug)]
pub(super) struct PendingSurfaceTransition {
    pub previous: Surface,
    pub returned: Surface,
    pub effect_height: Option<i32>,
}

/// The actor-record triples a position or angle property names: the local pose, the previous
/// position, the saved start position (location zero) and the angle history.
enum PoseProperty {
    Position(usize),
    Angle(usize),
    LastPosition(usize),
    StartPosition(usize),
    LastAngle(usize),
    StartAngle(usize),
    SlavePosition(usize),
    LastSlavePosition(usize),
    SlaveAngle(usize),
    LastSlaveAngle(usize),
}

/// A signed RAEX attachment word (`grip0`, `grip1`, `scabbard0`, `scabbard1`) of the actor: the
/// installed value the loader copied, unless a script rewrote the word through its property.
pub(super) fn raex_word(
    control: Option<&ActorControlState>,
    metadata: &serde_json::Value,
    field: &str,
) -> Option<i16> {
    let overridden = |control: &ActorControlState| match field {
        "scabbard0" => control.scabbard_override[0],
        "scabbard1" => control.scabbard_override[1],
        "grip0" => control.grip_override[0],
        "grip1" => control.grip_override[1],
        _ => None,
    };
    control.and_then(overridden).or_else(|| {
        metadata["raex"][field]
            .as_i64()
            .and_then(|value| i16::try_from(value).ok())
    })
}

/// Script properties that name actor motion bytes. The engine's property descriptors (preservation
/// engine/SOUPPROPS.md) place `rope_wait` at +0x1b0 (the companion motion byte), `slide_flag` at
/// +0x1b1 (sliding); `fall_flag` (+0x1b2, falling), `jump_flag` (+0x1b3, queued motion) and
/// `extra_flag` (+0x1b4, living state) are typed and converted in the property handlers.
/// `extra_time` is the independent ordinary countdown described by preservation.
/// `last_direction` shares the retained movement classification written by motion producers.
fn motion_property(control: &mut ActorControlState, name: Property) -> Option<&mut u8> {
    Some(match name {
        Property::RopeWait => &mut control.jump_selector,
        Property::ExtraTime => &mut control.extra_time,
        Property::SurfaceTime => &mut control.surface_time,
        Property::JumpTime => &mut control.jump_time,
        Property::JumpHeight => &mut control.jump_height,
        Property::Anchor => &mut control.anchor,
        Property::SlideFlag => &mut control.sliding_state,
        Property::MoveCounter => &mut control.move_counter,
        Property::OriginTime => &mut control.fall_timer,
        Property::ActionTime => &mut control.jump_window,
        Property::ArmsTime => &mut control.action_transition_counter,
        Property::LastCollideCount => &mut control.last_collide_count,
        Property::DisableScript => &mut control.script_control,
        Property::AiMode => &mut control.ai_mode,
        Property::AiDelay => &mut control.ai_delay,
        _ => return None,
    })
}

/// A control-record field a script property names: the word fields read sign-extended and keep the
/// low sixteen bits of a write, the dword fields keep all of it, and the flag byte reads
/// zero-extended (cyrus keeps it as a boolean, so a read gives 0 or 1 and any nonzero write sets it).
enum ControlField<'a> {
    Word(&'a mut i16),
    Dword(&'a mut i32),
    Flag(&'a mut bool),
}
fn control_record_property(
    control: &mut ActorControlState,
    name: Property,
) -> Option<ControlField<'_>> {
    Some(match name {
        Property::AiVar1 => ControlField::Word(&mut control.ai.var1),
        Property::AiVar2 => ControlField::Word(&mut control.ai.var2),
        Property::AiLength => ControlField::Dword(&mut control.ai.length),
        Property::YangleChange => ControlField::Word(&mut control.yangle_change),
        // The node fields are the walk-map owner's held nodes: current, next and final destination.
        Property::Node => ControlField::Dword(&mut control.navigation.current),
        Property::Dnode => ControlField::Dword(&mut control.navigation.next),
        Property::Fdnode => ControlField::Dword(&mut control.navigation.destination),
        // The stable-position record the fall and recovery code restores from.
        // KillMe's finished marker and the weapon-drawn byte of the equipped-weapon routines.
        Property::Dead => ControlField::Flag(&mut control.killed),
        Property::WeaponDrawn => ControlField::Flag(&mut control.weapon_drawn),
        Property::Invisible => ControlField::Flag(&mut control.invisible),
        Property::MoveVx => ControlField::Dword(&mut control.motion_vector[0]),
        Property::MoveVy => ControlField::Dword(&mut control.motion_vector[1]),
        Property::MoveVz => ControlField::Dword(&mut control.motion_vector[2]),
        Property::OvVx => ControlField::Dword(&mut control.ledge_point[0]),
        Property::OvVy => ControlField::Dword(&mut control.ledge_point[1]),
        Property::OvVz => ControlField::Dword(&mut control.ledge_point[2]),
        Property::HandPosVx => ControlField::Dword(&mut control.hand_point[0]),
        Property::HandPosVy => ControlField::Dword(&mut control.hand_point[1]),
        Property::HandPosVz => ControlField::Dword(&mut control.hand_point[2]),
        Property::HandAngleVx => ControlField::Dword(&mut control.hand_angles[0]),
        Property::HandAngleVy => ControlField::Dword(&mut control.hand_angles[1]),
        Property::HandAngleVz => ControlField::Dword(&mut control.hand_angles[2]),
        Property::LastValidFlag => ControlField::Flag(&mut control.stable_position_valid),
        Property::LastValidPosVx => ControlField::Dword(&mut control.stable_local_position[0]),
        Property::LastValidPosVy => ControlField::Dword(&mut control.stable_local_position[1]),
        Property::LastValidPosVz => ControlField::Dword(&mut control.stable_local_position[2]),
        _ => return None,
    })
}

/// The bounds-record triple a `size.psize`, `size.msize` or `size.pos` property names, and the axis.
enum CollisionField {
    Positive(usize),
    Negative(usize),
    Center(usize),
    Total(usize),
}

fn collision_extent_property(name: Property) -> Option<CollisionField> {
    use Property::{
        SizeMsizeVx, SizeMsizeVy, SizeMsizeVz, SizePosVx, SizePosVy, SizePosVz, SizePsizeVx,
        SizePsizeVy, SizePsizeVz, SizeSizeVx, SizeSizeVy, SizeSizeVz,
    };
    let find = |names: [Property; 3]| names.iter().position(|&n| n == name);
    find([SizePsizeVx, SizePsizeVy, SizePsizeVz])
        .map(CollisionField::Positive)
        .or_else(|| find([SizeMsizeVx, SizeMsizeVy, SizeMsizeVz]).map(CollisionField::Negative))
        .or_else(|| find([SizePosVx, SizePosVy, SizePosVz]).map(CollisionField::Center))
        .or_else(|| find([SizeSizeVx, SizeSizeVy, SizeSizeVz]).map(CollisionField::Total))
}

fn pose_property(name: Property) -> Option<PoseProperty> {
    use Property::{
        AngleVx, AngleVy, AngleVz, LastAngleVx, LastAngleVy, LastAngleVz, LastPosVx, LastPosVy,
        LastPosVz, LastSlaveAngleVx, LastSlaveAngleVy, LastSlaveAngleVz, LastSlavePosVx,
        LastSlavePosVy, LastSlavePosVz, PosVx, PosVy, PosVz, SlaveAngleVx, SlaveAngleVy,
        SlaveAngleVz, SlavePosVx, SlavePosVy, SlavePosVz, StartAngleVx, StartAngleVy, StartAngleVz,
        StartPosVx, StartPosVy, StartPosVz,
    };
    let find = |names: [Property; 3]| names.iter().position(|&n| n == name);
    find([PosVx, PosVy, PosVz])
        .map(PoseProperty::Position)
        .or_else(|| find([AngleVx, AngleVy, AngleVz]).map(PoseProperty::Angle))
        .or_else(|| find([LastPosVx, LastPosVy, LastPosVz]).map(PoseProperty::LastPosition))
        .or_else(|| find([StartPosVx, StartPosVy, StartPosVz]).map(PoseProperty::StartPosition))
        .or_else(|| find([LastAngleVx, LastAngleVy, LastAngleVz]).map(PoseProperty::LastAngle))
        .or_else(|| find([StartAngleVx, StartAngleVy, StartAngleVz]).map(PoseProperty::StartAngle))
        .or_else(|| find([SlavePosVx, SlavePosVy, SlavePosVz]).map(PoseProperty::SlavePosition))
        .or_else(|| {
            find([LastSlavePosVx, LastSlavePosVy, LastSlavePosVz])
                .map(PoseProperty::LastSlavePosition)
        })
        .or_else(|| find([SlaveAngleVx, SlaveAngleVy, SlaveAngleVz]).map(PoseProperty::SlaveAngle))
        .or_else(|| {
            find([LastSlaveAngleVx, LastSlaveAngleVy, LastSlaveAngleVz])
                .map(PoseProperty::LastSlaveAngle)
        })
}

/// The animation record field an `anim.*` property names. The other record fields have no
/// recovered owner or contract and stay unsupported.
fn playback_field(name: Property) -> Option<cyrus_world::animation::PlaybackField> {
    use cyrus_world::animation::PlaybackField;
    Some(match name {
        Property::AnimNext => PlaybackField::Next,
        Property::AnimNextSpeed => PlaybackField::NextSpeed,
        Property::AnimFrame => PlaybackField::Frame,
        Property::AnimSpeed => PlaybackField::Speed,
        Property::AnimSpeedCount => PlaybackField::SpeedCount,
        Property::AnimStopFlag => PlaybackField::StopFlag,
        Property::AnimHaltFlag => PlaybackField::HaltFlag,
        Property::AnimHoldFlag => PlaybackField::HoldFlag,
        Property::AnimLoopCount => PlaybackField::LoopCount,
        Property::AnimRepCount => PlaybackField::RepCount,
        Property::AnimPosVx => PlaybackField::Position(0),
        Property::AnimPosVy => PlaybackField::Position(1),
        Property::AnimPosVz => PlaybackField::Position(2),
        Property::AnimAngleVx => PlaybackField::Rotation(0),
        Property::AnimAngleVy => PlaybackField::Rotation(1),
        Property::AnimAngleVz => PlaybackField::Rotation(2),
        Property::AnimRewindFlag => PlaybackField::RewindFlag,
        Property::AnimRealFrame => PlaybackField::RealFrame,
        Property::AnimMaxGroup => PlaybackField::MaxGroup,
        Property::AnimLastAngleVx => PlaybackField::LastRotation(0),
        Property::AnimLastAngleVy => PlaybackField::LastRotation(1),
        Property::AnimLastAngleVz => PlaybackField::LastRotation(2),
        _ => return None,
    })
}

/// The task slot and retained word a `task_variable*` property names.
fn task_word(name: Property) -> Option<(usize, super::tasks::TaskWord)> {
    use super::tasks::TaskWord;
    use Property::*;
    let families = [
        (
            TaskWord::Continuation,
            [
                TaskVariable0,
                TaskVariable1,
                TaskVariable2,
                TaskVariable3,
                TaskVariable4,
                TaskVariable5,
            ],
        ),
        (
            TaskWord::PauseTicks,
            [
                TaskVariable2_0,
                TaskVariable2_1,
                TaskVariable2_2,
                TaskVariable2_3,
                TaskVariable2_4,
                TaskVariable2_5,
            ],
        ),
        (
            TaskWord::ContinuationDirection,
            [
                TaskVariable3_0,
                TaskVariable3_1,
                TaskVariable3_2,
                TaskVariable3_3,
                TaskVariable3_4,
                TaskVariable3_5,
            ],
        ),
        (
            TaskWord::PointCommand,
            [
                TaskVariable4_0,
                TaskVariable4_1,
                TaskVariable4_2,
                TaskVariable4_3,
                TaskVariable4_4,
                TaskVariable4_5,
            ],
        ),
    ];
    families.into_iter().find_map(|(word, names)| {
        names
            .iter()
            .position(|&candidate| candidate == name)
            .map(|slot| (slot, word))
    })
}

/// The task slot field a `task_status`, `task_pause` or `task_timer` property names.
fn task_slot_name(name: Property) -> Option<(usize, cyrus_soup::TaskSlotField)> {
    use Property::*;
    use cyrus_soup::TaskSlotField;
    [
        (
            TaskSlotField::Status,
            [
                TaskStatus0,
                TaskStatus1,
                TaskStatus2,
                TaskStatus3,
                TaskStatus4,
                TaskStatus5,
            ],
        ),
        (
            TaskSlotField::Pause,
            [
                TaskPause0, TaskPause1, TaskPause2, TaskPause3, TaskPause4, TaskPause5,
            ],
        ),
        (
            TaskSlotField::Timer,
            [
                TaskTimer0, TaskTimer1, TaskTimer2, TaskTimer3, TaskTimer4, TaskTimer5,
            ],
        ),
    ]
    .into_iter()
    .find_map(|(field, names)| {
        names
            .iter()
            .position(|&candidate| candidate == name)
            .map(|slot| (slot, field))
    })
}

impl GameHost<'_, '_, '_, '_> {
    /// The nonplayer health services `ZapObject`, `ZapObjectNoKill`, `SetHealth` and `AdjustHealth`
    /// share, after their receiver gates: the health byte takes the wrapped sum clamped to 0..255
    /// and, for the death-owning variant, a result at or below zero ends the receiver with the
    /// recovered death dispatch.
    fn adjust_nonplayer_health(
        &mut self,
        function: Native,
        amount: i32,
        kills: bool,
    ) -> Result<()> {
        let attributes = &mut self.registry.actors[self.index].attributes;
        let sum = i32::from(attributes[Attribute::Health]).wrapping_add(amount);
        attributes[Attribute::Health] = sum.clamp(0, 255) as u8;
        if kills && sum <= 0 {
            let holding = self.receiver_weapon_predicates(Native::IsHoldingWeapon)?.0;
            let ticks = self
                .inputs
                .nominal_ticks
                .with_context(|| format!("{function}: death nominal ticks unavailable"))?;
            let id = self.registry.id(self.index)?;
            let sounds = &mut self.sounds;
            self.registry
                .dispatch_zap_object_death(id, holding, ticks, &mut |actor, sound| {
                    sounds
                        .as_deref_mut()
                        .context("health death sound executor unavailable")?(
                        actor, sound
                    )
                })?;
        }
        Ok(())
    }

    /// `ZapObject`, `ZapObjectNoKill`, `ZapPlayer` and `ZapPlayerNoKill`: health adjustments over
    /// the selected-player services `AdjustHealth` also uses. The player natives take the selected
    /// player whatever their receiver; the object natives gate on the receiver first and use the
    /// player services only for a selected receiver, adjusting any other receiver's health byte
    /// directly. The original's pointer-valued results are not reproduced, so none is returned.
    fn zap(&mut self, function: Native, arguments: &[Argument]) -> Result<CallResult> {
        let [amount] = arguments else {
            anyhow::bail!("{function} requires one argument");
        };
        let amount = amount.integer()?;
        let player = self.selected_player();
        let kills = matches!(function, Native::ZapObject | Native::ZapPlayer);
        let objects = matches!(function, Native::ZapObject | Native::ZapObjectNoKill);
        let receiver_is_player = player.is_some_and(|player| player.index == self.index);
        if objects {
            let attributes = &self.registry.actors[self.index].attributes;
            if attributes[Attribute::Invulnerable] != 0 {
                return Ok(CallResult::CompleteWithoutValue);
            }
            if attributes[Attribute::Npc] != 0
                && !self.receiver_weapon_predicates(Native::IsHoldingWeapon)?.0
                && self.registry.actors[self.index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .is_none_or(|combat| !combat.flags.contains(CombatFlags::STANCE))
            {
                return Ok(CallResult::CompleteWithoutValue);
            }
            if !receiver_is_player {
                self.adjust_nonplayer_health(function, amount, kills)?;
                return Ok(CallResult::CompleteWithoutValue);
            }
        }
        let player = player.with_context(|| format!("{function}: selected player unavailable"))?;
        let id = self.registry.id(player.index)?;
        // The player service takes the native's receiver as the damage source; ZapObject's
        // selected-receiver branch passes none.
        let source = if function == Native::ZapPlayer {
            let receiver = self.registry.id(self.index)?;
            Some(super::activation::NativeSource::Actor {
                generation: receiver.generation,
                index: receiver.index,
            })
        } else {
            None
        };
        let sounds = &mut self.sounds;
        let mut play = |actor, sound| {
            sounds
                .as_deref_mut()
                .context("selected health sound executor unavailable")?(actor, sound)
        };
        if kills {
            self.registry.apply_selected_death_health(
                id,
                amount,
                source,
                self.inputs,
                self.random.as_deref_mut(),
                &mut play,
            )?;
        } else {
            self.registry.apply_selected_additive_health(
                id,
                amount,
                self.inputs,
                self.random.as_deref_mut(),
                &mut play,
            )?;
        }
        Ok(CallResult::CompleteWithoutValue)
    }
}

impl Host for GameHost<'_, '_, '_, '_> {
    fn task_slot_property(
        &mut self,
        target: &Target,
        reference: u16,
    ) -> Result<Option<cyrus_soup::TaskSlotProperty>> {
        let Some((slot, field)) = task_slot_name(self.property(target, reference)?) else {
            return Ok(None);
        };
        Ok(Some(cyrus_soup::TaskSlotProperty {
            slot,
            field,
            own: self.property_receiver(target)? == self.index,
        }))
    }

    fn take_actor_result(&mut self) -> Option<ScriptActorId> {
        self.registry.globals.actor_result.take()
    }

    fn establish_native_source(
        &mut self,
        _: cyrus_soup::CodeOrigin,
        program: std::sync::Arc<cyrus_soup::program::Program>,
    ) {
        self.registry.globals.native_source = super::activation::NativeSource::Actor {
            generation: self.generation,
            index: self.index,
        };
        self.native_source_program = Some(program);
    }

    fn native_string_parameter(&mut self, index: u32, _: &[String]) -> Result<String> {
        self.native_source_program
            .as_ref()
            .with_context(|| {
                format!(
                    "world {} actor {}: retained native source unavailable",
                    self.world, self.index
                )
            })?
            .strings
            .get(index as usize)
            .cloned()
            .with_context(|| {
                format!(
                    "world {} actor {}: retained source string {index} absent",
                    self.world, self.index
                )
            })
    }

    fn read_value(&mut self, target: &Target, reference: u16) -> Result<cyrus_soup::ScriptValue> {
        if self.property(target, reference)? == Property::Address {
            let actor = if matches!(target, Target::Player) {
                self.registry.id(self.property_receiver(target)?)?
            } else {
                self.target_reference(target)?
            };
            anyhow::ensure!(
                self.registry.actors[actor.index]
                    .runtime_identifier
                    .is_some(),
                "property address: world {} actor {} temporary or unallocated reference identity unsupported",
                self.world,
                actor.index
            );
            return Ok(cyrus_soup::ScriptValue::Actor(actor));
        }
        self.read_property(target, reference)
            .map(cyrus_soup::ScriptValue::Integer)
    }

    fn read_property(&mut self, target: &Target, reference: u16) -> Result<i32> {
        let name = self.property(target, reference)?;
        if name == Property::LastCollide {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("last_collide control owner unavailable")?;
            return Ok(i32::from(control.last_collide.bits() as u8));
        }
        if name == Property::Sflags {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .model
                .collision_flags
                .map(i32::from)
                .with_context(|| format!("property {name}: collision metadata unavailable"));
        }
        if name == Property::PanicGroup {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            return Ok(i32::from(
                actor
                    .animation
                    .as_ref()
                    .map_or(actor.unprepared_animation_default, |animation| {
                        animation.playback.default_group().index() as i16
                    }),
            ));
        }
        if let Some(field) = playback_field(name) {
            let index = self.property_receiver(target)?;
            // An actor without an animation association reads zero, as `anim.current` does.
            return self.registry.actors[index]
                .animation
                .as_ref()
                .map_or(Ok(0), |animation| animation.playback.record_field(field))
                .with_context(|| format!("property {name}"));
        }
        if name == Property::Frame || name == Property::LastFrame {
            // The word is the frame index inside the model the selected ShowFrame names, -1 when
            // no frame was selected.
            let index = self.property_receiver(target)?;
            let animation = self.registry.actors[index]
                .animation
                .as_ref()
                .with_context(|| format!("property {name}: actor {index} has no animation"))?;
            let Some(reference) = (if name == Property::Frame {
                animation.frame
            } else {
                animation.playback.last_frame()
            }) else {
                return Ok(-1);
            };
            let data = self
                .inputs
                .animations
                .with_context(|| format!("property {name}: animation data unavailable"))?
                .0;
            let actor = &self.registry.actors[index];
            let resolved = if actor.placement().is_some() {
                data.resolve_map_frame(index, reference)
            } else {
                data.resolve_frame(actor.name(), reference)
            }
            .with_context(|| format!("property {name}"))?;
            return i32::try_from(resolved.frame).with_context(|| format!("property {name}"));
        }
        if name == Property::AnimCurrent {
            let index = self.property_receiver(target)?;
            return Ok(self.registry.actors[index]
                .animation
                .as_ref()
                .map_or(0, |animation| {
                    animation
                        .playback
                        .current_group()
                        .map_or(-1, |group| i32::from(group.index() as i16))
                }));
        }
        if let Some(word) = [Property::Grip0, Property::Grip1]
            .iter()
            .position(|&word| word == name)
        {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            let value = if let Some(value) = control.grip_override[word] {
                value
            } else {
                let data = self
                    .inputs
                    .animations
                    .with_context(|| format!("{name} animation data unavailable"))?
                    .0;
                let field = ["grip0", "grip1"][word];
                raex_word(Some(control), data.actor(actor.name())?, field)
                    .with_context(|| format!("actor {} RAEX {field} unavailable", actor.name()))?
            };
            return Ok(i32::from(value));
        }
        if let Some(word) = [Property::Scabbard0, Property::Scabbard1]
            .iter()
            .position(|&word| word == name)
        {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            let value = if let Some(value) = control.scabbard_override[word] {
                value
            } else {
                let data = self
                    .inputs
                    .animations
                    .with_context(|| format!("{name} animation data unavailable"))?
                    .0;
                let field = ["scabbard0", "scabbard1"][word];
                raex_word(Some(control), data.actor(actor.name())?, field)
                    .with_context(|| format!("actor {} RAEX {field} unavailable", actor.name()))?
            };
            return Ok(i32::from(value));
        }
        if name == Property::UnderSurface {
            let index = self.property_receiver(target)?;
            let id = self.registry.id(index)?;
            return self
                .registry
                .actor_surface(id)
                .map(|surface| i32::from(surface.byte()));
        }
        if matches!(
            name,
            Property::Origin | Property::OriginAccum | Property::ActionFlag | Property::Action
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            return Ok(match name {
                Property::Origin => control.fall_origin_y,
                Property::OriginAccum => control.sliding_distance,
                Property::ActionFlag => control.actions.pending as i32,
                _ => control.actions.accepted as i32,
            });
        }
        if name == Property::TorchTimer {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("torch_timer control owner unavailable")?;
            return Ok(i32::from(control.torch_burning));
        }
        // `rope` holds a record pointer in the original; installed scripts only test it against zero.
        if name == Property::Rope || name == Property::Hide {
            let rope = name == Property::Rope;
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            return Ok(i32::from(if rope {
                control.rope_attached
            } else {
                control.hidden
            }));
        }
        if matches!(
            name,
            Property::AiType
                | Property::ExtraFlag
                | Property::FallFlag
                | Property::JumpFlag
                | Property::LastDirection
                | Property::HandItem
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(i32::from(match name {
                Property::AiType => control.ai_type.byte(),
                // An unestablished held item reads as the allocator's zero.
                Property::HandItem => control.held_item.unwrap_or(0),
                Property::ExtraFlag => control.living_state.byte(),
                Property::FallFlag => control.falling_state.byte(),
                Property::LastDirection => control.motion_classification.byte(),
                _ => control.queued_motion.byte(),
            }));
        }
        if motion_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(i32::from(
                *motion_property(control, name).expect("named motion byte"),
            ));
        }
        // The message and broadcast selections are the actor-record bytes the message natives read.
        if name == Property::MessageIndex || name == Property::BroadcastIndex {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            return Ok(i32::from(if name == Property::MessageIndex {
                actor.message_read
            } else {
                actor.broadcast_index
            }));
        }
        if name == Property::Id {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .runtime_identifier
                .map(i32::from)
                .with_context(|| format!("property {name}: runtime identifier unavailable"));
        }
        if let Some((slot, field)) = task_slot_name(name) {
            let index = self.property_receiver(target)?;
            return Ok(self.registry.actors[index]
                .runtime
                .as_ref()
                .with_context(|| {
                    format!("property {name}: task slots of actor {index} unavailable")
                })?
                .task_slot_field(slot, field));
        }
        if let Some((slot, word)) = task_word(name) {
            let index = self.property_receiver(target)?;
            return Ok(self.registry.actors[index].native_commands[slot].word(word) as i32);
        }
        if control_record_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(
                match control_record_property(control, name).expect("named control field") {
                    ControlField::Word(word) => i32::from(*word),
                    ControlField::Dword(dword) => *dword,
                    ControlField::Flag(flag) => i32::from(*flag),
                },
            );
        }
        if let Some(field) = collision_extent_property(name) {
            let index = self.property_receiver(target)?;
            let shape = self.registry.actors[index]
                .collision
                .as_ref()
                .with_context(|| {
                    format!("property {target:?}.{name}: collision shape not prepared")
                })?;
            return Ok(match field {
                CollisionField::Positive(axis) => shape.positive_extent[axis],
                CollisionField::Negative(axis) => shape.negative_extent[axis],
                CollisionField::Center(axis) => shape.center_offset[axis],
                CollisionField::Total(axis) => shape.total_extent[axis],
            });
        }
        let pose = pose_property(name)
            .with_context(|| format!("property {target:?}.{name} unsupported"))?;
        let index = self.property_receiver(target)?;
        let spatial = self
            .registry
            .spatial
            .as_ref()
            .with_context(|| format!("property {target:?}.{name}: spatial owner unavailable"))?
            .actor(index)?;
        Ok(match pose {
            PoseProperty::Position(axis) => spatial.local.position[axis],
            PoseProperty::Angle(axis) => spatial.local.angles[axis],
            PoseProperty::LastPosition(axis) => spatial.previous_local_position[axis],
            PoseProperty::StartPosition(axis) => spatial
                .locations
                .first()
                .with_context(|| format!("property {name}: saved start position unavailable"))?
                [axis],
            PoseProperty::LastAngle(axis) => spatial.angle_history[axis],
            PoseProperty::StartAngle(axis) => spatial.initial_angles[axis],
            PoseProperty::SlavePosition(axis) => spatial.attached_pose(name)?.position[axis],
            PoseProperty::SlaveAngle(axis) => spatial.attached_pose(name)?.angles[axis],
            PoseProperty::LastSlavePosition(axis) => spatial
                .previous_effective_position
                .with_context(|| format!("property {name}: no family refresh has run"))?[axis],
            PoseProperty::LastSlaveAngle(axis) => spatial
                .previous_effective_angles
                .with_context(|| format!("property {name}: no family refresh has run"))?[axis],
        })
    }

    fn write_property(&mut self, target: &Target, reference: u16, value: i32) -> Result<()> {
        let name = self.property(target, reference)?;
        if name == Property::LastCollide {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .context("last_collide control owner unavailable")?;
            control.last_collide = super::CollisionFlags::from_bits(
                (control.last_collide.bits() & !255) | u32::from(value as u8),
            );
            return Ok(());
        }
        if name == Property::Sflags {
            let index = self.property_receiver(target)?;
            let flags = self.registry.actors[index]
                .model
                .collision_flags
                .as_mut()
                .with_context(|| format!("property {name}: collision metadata unavailable"))?;
            *flags = value as u8;
            return Ok(());
        }
        if let Some(field) = playback_field(name) {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .animation
                .as_mut()
                .with_context(|| format!("property {name}: actor {index} has no animation"))?
                .playback
                .set_record_field(field, value)
                .with_context(|| format!("property {name}"));
        }
        if name == Property::AnimCurrent {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .animation
                .as_mut()
                .with_context(|| format!("property {name}: actor {index} has no animation"))?
                .playback
                .set_current_group(value)
                .with_context(|| format!("property {name}"));
        }
        if name == Property::PanicGroup {
            let index = self.property_receiver(target)?;
            let actor = &mut self.registry.actors[index];
            if let Some(animation) = actor.animation.as_mut() {
                animation
                    .playback
                    .set_stored_default(AnimationGroup::from_index(value as u16));
            } else {
                actor.unprepared_animation_default = value as i16;
            }
            return Ok(());
        }
        if name == Property::UnderSurface {
            let index = self.property_receiver(target)?;
            let id = self.registry.id(index)?;
            self.registry.actor_surface(id)?;
            return self
                .registry
                .set_actor_surface(id, Surface::from_byte(value as u8));
        }
        if matches!(
            name,
            Property::Origin | Property::OriginAccum | Property::ActionFlag | Property::Action
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("{name} control owner unavailable"))?;
            match name {
                Property::Origin => control.fall_origin_y = value,
                Property::OriginAccum => control.sliding_distance = value,
                Property::ActionFlag => control.actions.pending = value as u32,
                _ => control.actions.accepted = value as u32,
            }
            return Ok(());
        }
        if name == Property::TorchTimer {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .context("torch_timer control owner unavailable")?;
            control.torch_burning = value as u8;
            return Ok(());
        }
        if matches!(
            name,
            Property::AiType
                | Property::ExtraFlag
                | Property::FallFlag
                | Property::JumpFlag
                | Property::LastDirection
                | Property::HandItem
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            let byte = value as u8;
            match name {
                Property::HandItem => control.held_item = Some(byte),
                Property::AiType => control.ai_type = super::AiType::from_byte(byte),
                Property::ExtraFlag => control.living_state = LivingState::from_byte(byte),
                Property::FallFlag => control.falling_state = JumpMotion::from_byte(byte),
                Property::LastDirection => {
                    control.motion_classification = MotionDirection::from_byte(byte)
                }
                _ => control.queued_motion = JumpMotion::from_byte(byte),
            }
            return Ok(());
        }
        if motion_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            *motion_property(control, name).expect("named motion byte") = value as u8;
            return Ok(());
        }
        if name == Property::MessageIndex || name == Property::BroadcastIndex {
            let index = self.property_receiver(target)?;
            let actor = &mut self.registry.actors[index];
            if name == Property::MessageIndex {
                actor.message_read = value as u8;
            } else {
                actor.broadcast_index = value as u8;
            }
            return Ok(());
        }
        anyhow::ensure!(
            name != Property::Id,
            "property {name} write unsupported: the runtime identifier is installed identity that name lookup, route ownership and restore validation key on"
        );
        if let Some((slot, field)) = task_slot_name(name) {
            let index = self.property_receiver(target)?;
            self.registry.actors[index]
                .runtime
                .as_mut()
                .with_context(|| {
                    format!("property {name}: task slots of actor {index} unavailable")
                })?
                .set_task_slot_field(slot, field, value);
            return Ok(());
        }
        if let Some((slot, word)) = task_word(name) {
            let index = self.property_receiver(target)?;
            self.registry.actors[index].native_commands[slot].set_word(word, value as u32);
            return Ok(());
        }
        if control_record_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            match control_record_property(control, name).expect("named control field") {
                ControlField::Word(word) => *word = value as i16,
                ControlField::Dword(dword) => *dword = value,
                ControlField::Flag(flag) => *flag = value as u8 != 0,
            }
            return Ok(());
        }
        let scabbard = [Property::Scabbard0, Property::Scabbard1]
            .iter()
            .position(|&word| word == name);
        let grip = [Property::Grip0, Property::Grip1]
            .iter()
            .position(|&word| word == name);
        if name == Property::Hide || scabbard.is_some() || grip.is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            match (scabbard, grip) {
                (Some(word), _) => control.scabbard_override[word] = Some(value as i16),
                (_, Some(word)) => control.grip_override[word] = Some(value as i16),
                _ => control.hidden = value as u8 != 0,
            }
            return Ok(());
        }
        if let Some(field) = collision_extent_property(name) {
            let index = self.property_receiver(target)?;
            let shape = self.registry.actors[index]
                .collision
                .as_mut()
                .with_context(|| {
                    format!("property {target:?}.{name}: collision shape not prepared")
                })?;
            match field {
                CollisionField::Positive(axis) => shape.positive_extent[axis] = value,
                CollisionField::Negative(axis) => shape.negative_extent[axis] = value,
                CollisionField::Center(axis) => shape.center_offset[axis] = value,
                CollisionField::Total(axis) => shape.total_extent[axis] = value,
            }
            return Ok(());
        }
        let pose = pose_property(name)
            .with_context(|| format!("property {target:?}.{name} write unsupported"))?;
        let index = self.property_receiver(target)?;
        let spatial = self
            .registry
            .spatial
            .as_mut()
            .with_context(|| format!("property {target:?}.{reference}: spatial owner unavailable"))?
            .actor_mut(index)?;
        match pose {
            PoseProperty::Position(axis) => spatial.local.position[axis] = value,
            PoseProperty::Angle(axis) => spatial.local.angles[axis] = value,
            PoseProperty::LastPosition(axis) => spatial.previous_local_position[axis] = value,
            PoseProperty::StartPosition(axis) => {
                spatial.locations.first_mut().with_context(|| {
                    format!("property {name}: saved start position unavailable")
                })?[axis] = value
            }
            PoseProperty::LastAngle(axis) => spatial.angle_history[axis] = value,
            PoseProperty::StartAngle(axis) => spatial.initial_angles[axis] = value,
            PoseProperty::SlavePosition(axis) => {
                spatial.attached_pose_mut(name)?.position[axis] = value
            }
            PoseProperty::SlaveAngle(axis) => spatial.attached_pose_mut(name)?.angles[axis] = value,
            PoseProperty::LastSlavePosition(axis) => {
                spatial.previous_effective_position.get_or_insert([0; 3])[axis] = value
            }
            PoseProperty::LastSlaveAngle(axis) => {
                spatial.previous_effective_angles.get_or_insert([0; 3])[axis] = value
            }
        }
        Ok(())
    }

    fn prepare_native_command(&mut self, site: cyrus_soup::NativeCallSite) {
        self.registry.globals.native_command.prepare(site);
    }
    fn load_native_command(&mut self, slot: usize, elapsed: Option<i32>) {
        self.registry.actors[self.index].native_commands[slot]
            .load(&mut self.registry.globals.native_command);
        if let Some(elapsed) = elapsed {
            self.registry.globals.native_command.point_value = elapsed as u32;
        }
        self.registry.globals.native_command_return = 0;
    }
    fn save_native_command(&mut self, slot: usize, script_return: &mut u8) {
        self.registry.actors[self.index].native_commands[slot] =
            self.registry.globals.native_command.task_state();
        *script_return = self.registry.globals.native_command_return;
    }
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        invocation: cyrus_soup::Invocation,
        flags: &mut [i32],
    ) -> Result<CallResult> {
        anyhow::ensure!(
            target == &Target::Me,
            "inspection target {target:?} unsupported"
        );
        // The installed changed-actor list capacity travels with the scene the host supplies.
        if let Some(source) = self.inputs.player_collision {
            self.registry.map_updates.learn_capacity(
                source
                    .scene
                    .settings
                    .max_remap_objects
                    .and_then(|capacity| usize::try_from(capacity).ok()),
            );
        }
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        // The original's function table keeps only each handler, not the declared kind, so a task
        // or multitask call of a function native runs the handler once and a zero return completes
        // the task at once. Only natives whose handler is shown to return zero take this form.
        if [
            Native::SetAction,
            Native::ResetAction,
            Native::ResetAllAction,
        ]
        .contains(&function)
        {
            anyhow::ensure!(
                kind == CallKind::Function,
                "{function}: task completion of a void native is unsupported"
            );
            let expected = usize::from(function != Native::ResetAllAction);
            anyhow::ensure!(
                arguments.len() == expected,
                "{function} requires {expected} arguments"
            );
            let mask = arguments
                .first()
                .map(Argument::integer)
                .transpose()?
                .unwrap_or(0) as u32;
            let actor = &mut self.registry.actors[self.index];
            let control = actor
                .control
                .as_mut()
                .context("action native: control unavailable")?;
            if function == Native::ResetAllAction {
                control.actions = Default::default();
            } else if function == Native::ResetAction {
                control.actions.iter_mut().for_each(|word| *word &= !mask);
            } else {
                if !matches!(mask, 1 | 2) || control.actions.iter().any(|word| word & mask != 0) {
                    return Ok(CallResult::CompleteWithoutValue);
                }
                let animation = actor
                    .animation
                    .as_ref()
                    .context("SetAction: animation unavailable")?;
                let group = animation
                    .playback
                    .current_group()
                    .context("SetAction: current group unavailable")?;
                if (AnimationGroup::FIRST_LAND_GROUP..=AnimationGroup::LAST_LAND_GROUP)
                    .contains(&group)
                    || control.queued_motion != JumpMotion::NONE
                    || control.falling_state != JumpMotion::NONE
                {
                    control
                        .actions
                        .iter_mut()
                        .for_each(|word| *word &= !ActionWords::JUMPS);
                } else {
                    let step_hold = actor.attributes[Attribute::StepType] == 0
                        || StepDirection::Forward.counter(control.move_counter) == 3;
                    if step_hold
                        && matches!(group, AnimationGroup::RUN | AnimationGroup::RUN_TORCH)
                        && !animation.playback.exit_reached()
                    {
                        control.actions.pending |= ActionWords::JUMPS;
                        control.jump_window = 0;
                    } else {
                        self.call(target, kind, Native::Jump, &[], invocation, flags)?;
                    }
                }
            }
            return Ok(CallResult::CompleteWithoutValue);
        }
        if kind != CallKind::Function
            && [
                Native::DeleteBroadcast,
                Native::FlatAnimate,
                Native::FlatSound,
                Native::ResetRotation,
                Native::Jump,
                Native::FxLightSize,
            ]
            .contains(&function)
        {
            return Ok(
                match self.call(
                    target,
                    CallKind::Function,
                    function,
                    arguments,
                    invocation,
                    flags,
                )? {
                    CallResult::Complete(0) => CallResult::CompleteTask,
                    other => other,
                },
            );
        }
        if function == Native::DeleteBroadcast {
            anyhow::ensure!(
                arguments.is_empty(),
                "DeleteBroadcast requires no arguments"
            );
            self.registry
                .globals
                .combat_messages
                .delete_broadcast(&mut self.registry.actors[self.index].broadcast_index);
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::Jump {
            anyhow::ensure!(arguments.is_empty(), "Jump requires no arguments");
            if !self
                .registry
                .script_jump_admitted(id, self.selected_player())?
            {
                return Ok(CallResult::Complete(0));
            }
            let environment = transition::ObstacleEnvironment {
                selected_player: self.selected_player(),
                auto_grab: false,
                item_system_ready: self
                    .inputs
                    .hud
                    .map(super::ScriptHud::inventory_ready)
                    .transpose()?
                    .unwrap_or(false),
                shell_execution: self.inputs.shell_execution.unwrap_or(false),
                ticks: self
                    .inputs
                    .nominal_ticks
                    .context("Jump needs nominal ticks")?,
            };
            let prepared;
            let geometry = if let Some(geometry) = self.inputs.locomotion_geometry {
                Some(geometry)
            } else if environment.selected_player == Some(id)
                && let Some(source) = self.inputs.player_collision
            {
                prepared = self.registry.prepare_ordinary_collision_query(
                    id,
                    source.scene,
                    source.animations,
                    source.view,
                    source.actors,
                )?;
                Some(LocomotionGeometry {
                    ground: ground::OrdinaryGroundQuery {
                        scene: source.scene,
                        animations: source.animations,
                        prepared: &prepared,
                        distance: source.scene.settings.post_collide_height,
                        sidedness: false,
                    },
                    environment,
                })
            } else {
                None
            };
            let timing = ledge_states::LedgeTiming {
                fine_ticks: if environment.selected_player == Some(id) {
                    self.inputs
                        .fine_ticks
                        .context("Jump needs fine ticks for selected-player edges")?
                } else {
                    0
                },
                frame_scale: self.inputs.frame_scale,
            };
            let mut missing_sound = |_, _| anyhow::bail!("Jump sound handler unavailable");
            self.registry.begin_script_jump(
                id,
                geometry,
                environment,
                timing,
                self.sounds.as_deref_mut().unwrap_or(&mut missing_sound),
            )?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::PlayerLineUp {
            // Unknown until the native stores this invocation's return value.
            self.registry.globals.lineup_result = None;
        }
        if kind == CallKind::Function
            && [
                Native::Light,
                Native::LightRadius,
                Native::LightIntensity,
                Native::LightOffset,
                Native::LightFlicker,
                Native::FlickerLight,
                Native::LightFlickerOff,
                Native::LightSize,
                Native::FxLightSize,
                Native::LightSizeOff,
            ]
            .contains(&function)
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let expected = match function {
                Native::Light | Native::LightFlicker | Native::LightSize | Native::FxLightSize => 2,
                Native::LightOffset => 3,
                Native::LightFlickerOff | Native::LightSizeOff => 0,
                _ => 1,
            };
            anyhow::ensure!(values.len() == expected, "{function}: wrong argument count");
            let actor = &mut self.registry.actors[self.index];
            let placement = actor.placement();
            let position = placement
                .map(|placement| placement.map_position.map(|value| value as i32))
                .unwrap_or_default();
            let light = actor
                .light
                .get_or_insert_with(|| light::LightState::dynamic(position));
            // Stored values keep their low sixteen bits, as the original's word stores do.
            match function {
                Native::Light => {
                    light.enabled = true;
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.starting_intensity = values[1] as i16;
                    light.intensity = values[1] as i16;
                }
                Native::LightRadius => {
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                Native::LightIntensity => {
                    light.starting_intensity = values[0] as i16;
                    light.intensity = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                Native::LightOffset => {
                    light.offset = [values[0] as i16, values[1] as i16, values[2] as i16]
                }
                Native::LightFlicker => light.flicker = [values[0] as u16, 0, values[1] as u16],
                Native::LightSize | Native::FxLightSize => {
                    light.size = [values[0] as u16, 0, values[1] as u16]
                }
                Native::LightSizeOff => light.size[0] = 0,
                Native::FlickerLight => light.flicker = [values[0] as u16, 0, 1],
                _ => light.flicker[0] = 0,
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && [
                Native::Flat,
                Native::FlatSetTexture,
                Native::FlatOff,
                Native::FlatOffset,
                Native::FlatAnimate,
                Native::FlatStop,
                Native::FlatLikeStatic,
            ]
            .contains(&function)
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            flat::call(
                &mut self.registry.actors[self.index].flat,
                function,
                &values,
                &self.registry.texture_frames,
            )?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && [Native::MenuNew, Native::MenuAddItem, Native::MenuSelection].contains(&function)
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let expected = if function == Native::MenuAddItem {
                3
            } else {
                0
            };
            anyhow::ensure!(values.len() == expected, "{function}: wrong argument count");
            let globals = &mut self.registry.globals;
            match function {
                Native::MenuNew => globals.menu.clear(),
                Native::MenuSelection => return Ok(CallResult::Complete(globals.menu.selection())),
                _ => {
                    let limit = globals
                        .dialogue
                        .as_ref()
                        .context("menuAddItem: dialogue settings unavailable")?
                        .settings()
                        .max_menu_items as usize;
                    let hud = self
                        .inputs
                        .hud
                        .context("menuAddItem: installed text unavailable")?;
                    // A label without an installed entry adds nothing.
                    let bytes = values[0].to_le_bytes();
                    if let Some(text) = std::str::from_utf8(&bytes)
                        .ok()
                        .map(|label| hud.activation_label(label))
                        .transpose()?
                        .flatten()
                    {
                        let remembered = globals.highlighted_choice().unwrap_or("").to_owned();
                        globals.menu.add(
                            limit,
                            &text,
                            values[0],
                            values[1],
                            values[2],
                            &remembered,
                        );
                    }
                }
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::LightOff {
            anyhow::ensure!(arguments.is_empty(), "LightOff requires no arguments");
            let actor = &mut self.registry.actors[self.index];
            if let Some(light) = &mut actor.light {
                light.locked = false;
                light.enabled = false;
                light.inactive_ticks = 0;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::ResetUserSync {
            anyhow::ensure!(arguments.is_empty(), "ResetUserSync requires no arguments");
            let channel = self.registry.actors[self.index].attributes[Attribute::User];
            self.registry
                .globals
                .group_sync
                .reset_user(self.index, channel);
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::ResetGroupSync {
            anyhow::ensure!(arguments.is_empty(), "ResetGroupSync requires no arguments");
            // The native clears this actor's posted value.  The channel update retains the
            // released byte; the next world-loop update observes this member as unposted.
            self.registry.globals.group_sync.reset_current(
                self.index,
                self.registry.actors[self.index].attributes[Attribute::Group],
            );
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::BeginCombat {
            anyhow::ensure!(arguments.is_empty(), "beginCombat requires no arguments");
            let inputs = match self.inputs.combat_entry {
                Some(inputs) => inputs,
                None => {
                    let selected_player = self
                        .selected_player()
                        .context("beginCombat selected player unavailable")?;
                    anyhow::ensure!(
                        id != selected_player,
                        "beginCombat selected-player death override unavailable"
                    );
                    let actor = &self.registry.actors[self.index];
                    anyhow::ensure!(
                        actor.model.prepared,
                        "beginCombat receiver model state unavailable"
                    );
                    CombatEntryInputs {
                        weapon: WeaponInputs {
                            selected_player,
                            item_system_ready: self
                                .inputs
                                .hud
                                .context("beginCombat inventory owner unavailable")?
                                .inventory_ready()?,
                            shell_execution: self
                                .inputs
                                .shell_execution
                                .context("beginCombat shell context unavailable")?,
                        },
                        nominal_ticks: self
                            .inputs
                            .nominal_ticks
                            .context("beginCombat nominal ticks unavailable")?,
                        taunt_animation_admitted: self.animation_system
                            && actor.animation.is_some()
                            && actor.model.static_radius.is_none(),
                        selected_player_death_override: false,
                    }
                }
            };
            anyhow::ensure!(
                inputs.weapon.selected_player.generation == self.generation
                    && inputs.weapon.selected_player.index < self.registry.actors.len(),
                "beginCombat selected actor belongs to an expired or absent world"
            );
            let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
            let events = effects::ActorEventDispatch {
                context: effects::ActorEventContext::capture(self.registry, id),
                camera: self.registry.camera.as_mut(),
                effects: &mut self.registry.effects,
            };
            let mut receiver = weapon::CombatReceiver::new(
                &mut self.registry.actors[self.index],
                &mut self.registry.globals,
                self.world,
                id,
                position,
            )?
            .with_events(events);
            let mut emit = |actor, sound| {
                self.sounds
                    .as_deref_mut()
                    .context("beginCombat sound emitter unavailable")?(actor, sound)
            };
            receiver.begin(
                inputs,
                self.random.as_deref_mut(),
                self.inputs.combat_taunts,
                &mut emit,
            )?;
            return Ok(CallResult::CompleteWithoutValue);
        }
        if kind == CallKind::Function
            && (function == Native::ShowItem || function == Native::ShowItemNoRtx)
        {
            // ShowItem(item, text, first, second) ignores its text argument here; ShowItemNoRtx
            // has no text argument. Both leave the viewer request pending.
            let rtx = function == Native::ShowItem;
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let (item, words) = match (rtx, values.as_slice()) {
                (true, &[item, _, first, second]) => (item, [first, second]),
                (false, &[item, first, second]) => (item, [first, second]),
                _ => anyhow::bail!("{function} requires {} arguments", if rtx { 4 } else { 3 }),
            };
            let request = &mut self.registry.globals.item_viewer;
            request.pending = true;
            request.no_rtx |= !rtx;
            request.item = item;
            request.words = words;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && (function == Native::DisplayHandModel || function == Native::DisplayHiltModel)
        {
            let [name, x, y, z] = arguments else {
                anyhow::bail!("{function} requires four arguments")
            };
            let Argument::String(name) = name else {
                anyhow::bail!("{function} requires a model name")
            };
            let data = self
                .inputs
                .animations
                .with_context(|| format!("{function} installed animation models unavailable"))?
                .0;
            let id = self.registry.id(self.index)?;
            self.registry.display_model_attachment(
                id,
                data,
                name,
                [x.integer()?, y.integer()?, z.integer()?],
                function == Native::DisplayHiltModel,
            )?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && (function == Native::DisplayHandItem || function == Native::DisplayHiltItem)
        {
            let [item, x, y, z] = arguments else {
                anyhow::bail!("{function} requires four arguments")
            };
            // The ordered item list is searched by unsigned identifier; without a match the
            // original requests a diagnostic before any model work.
            let item = item.integer()? as u32;
            let hud = self
                .inputs
                .hud
                .with_context(|| format!("{function} item list unavailable"))?;
            let model = hud
                .item_hand_model(item)?
                .with_context(|| format!("{function}: item {item} is not in the item list"))?;
            use cyrus_assets::hud::ItemHandModel;
            let model = match model {
                // An item without a hand model returns zero without allocating a record.
                ItemHandModel::Absent => return Ok(CallResult::Complete(0)),
                ItemHandModel::Model(model) => model,
                ItemHandModel::Unexported => anyhow::bail!(
                    "{function}: item {item}'s hand model is not in this HUD export; export the \
                     installed assets again"
                ),
            };
            let data = self
                .inputs
                .animations
                .with_context(|| format!("{function} installed animation models unavailable"))?
                .0;
            let id = self.registry.id(self.index)?;
            self.registry.display_model_attachment(
                id,
                data,
                &model,
                [x.integer()?, y.integer()?, z.integer()?],
                function == Native::DisplayHiltItem,
            )?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::AddItem {
            let [item, amount] = arguments else {
                anyhow::bail!("AddItem requires two arguments")
            };
            let item =
                u16::try_from(item.integer()?).context("AddItem item ID outside u16 range")?;
            self.add_session_item(item, amount.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && (function == Native::LightTorch || function == Native::UnlightTorch)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let inputs = self.torch_inputs(self.index, function)?;
            let ticks = self
                .inputs
                .nominal_ticks
                .with_context(|| format!("{function} nominal ticks unavailable"))?;
            let sounds = self
                .sounds
                .as_deref_mut()
                .with_context(|| format!("{function} sound handler unavailable"))?;
            if function == Native::LightTorch {
                self.registry.light_torch(id, inputs, ticks, sounds)?;
            } else {
                self.registry.unlight_torch(id, inputs, ticks, sounds)?;
            }
            return Ok(CallResult::Complete(0));
        }
        // shoot(name, x, y, z, pitch, yaw, roll) creates an object of the named script at the
        // receiver's position plus the offset rotated by the receiver's orientation, with the
        // given orientation. shootPlayer(name, x, y, z) aims the object at the selected player:
        // yaw is the bearing of the raw difference, pitch the elevation of the difference
        // normalized to length 65536, roll zero. The receiver becomes the object's parent record.
        if kind == CallKind::Function
            && (function == Native::Shoot || function == Native::ShootPlayer)
        {
            let player_form = function == Native::ShootPlayer;
            anyhow::ensure!(
                arguments.len() == if player_form { 4 } else { 7 },
                "{function}: wrong argument count"
            );
            let Argument::String(name) = &arguments[0] else {
                anyhow::bail!("{function} requires a script name")
            };
            let id = self.registry.id(self.index)?;
            let local = self.registry.spatial(id)?.local;
            let offset = [
                arguments[1].integer()?,
                arguments[2].integer()?,
                arguments[3].integer()?,
            ];
            // The original builds this rotation by turning yaw by minus the heading and pitch and
            // roll by the plain angles; actor orientation negates all three, so this is the
            // actor-orientation builder with pitch and roll negated.
            let rotated = cyrus_world::transform::Matrix::orientation([
                wrap(local.angles[0].wrapping_neg()) as u16,
                local.wrapped_angles()[1],
                wrap(local.angles[2].wrapping_neg()) as u16,
            ])
            .transform(offset)?;
            let position =
                std::array::from_fn(|axis| rotated[axis].wrapping_add(local.position[axis]));
            let angles = if player_form {
                let player = self
                    .selected_player()
                    .with_context(|| format!("{function}: selected player unavailable"))?;
                let target = self
                    .registry
                    .spatial(self.registry.id(self.registry.resolve(player)?)?)?
                    .local
                    .position;
                let difference =
                    std::array::from_fn(|axis| target[axis].wrapping_sub(position[axis]));
                let unit = super::spatial::normalized_direction(difference);
                let horizontal =
                    cyrus_collision::distance::rounded_distance([unit[0], 0, unit[2]])?;
                // The original's table bearing is the negation of the shared query bearing.
                let elevation = i32::from(wrap(
                    super::spatial::query_bearing(horizontal, unit[1]).wrapping_neg(),
                ));
                [
                    wrap(elevation.wrapping_sub(0x200)) as u16,
                    super::spatial::query_bearing(difference[0], difference[2]),
                    0,
                ]
            } else {
                [
                    wrap(arguments[4].integer()?) as u16,
                    wrap(arguments[5].integer()?) as u16,
                    wrap(arguments[6].integer()?) as u16,
                ]
            };
            self.registry.create_object(id, name, position, angles)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::SpacePressed {
            anyhow::ensure!(arguments.is_empty(), "SpacePressed requires no arguments");
            let keys = self
                .inputs
                .script_keys
                .context("SpacePressed keyboard sample unavailable")?;
            return Ok(CallResult::Complete(i32::from(keys.space)));
        }
        // PressKey(code): InKey's key and modifier rules, but a success consumes the key's held
        // state until the key is released (a matching latest event still answers one).
        if kind == CallKind::Function && function == Native::PressKey {
            let [code] = arguments else {
                anyhow::bail!("PressKey requires one argument")
            };
            let code = code.integer()? as u32;
            let keys = self
                .inputs
                .script_keys
                .context("PressKey keyboard sample unavailable")?;
            let pressed = keys
                .press_key(code, &mut self.registry.consumed_letters)
                .with_context(|| {
                    format!("PressKey code {code:#x}: key without a recovered scan code")
                })?;
            return Ok(CallResult::Complete(i32::from(pressed)));
        }
        // InKey(code): whether the key of the code's low byte is down, with the modifiers the
        // higher bytes name. Only the debug shell and a few bridge and door scripts call it.
        if kind == CallKind::Function && function == Native::InKey {
            let [code] = arguments else {
                anyhow::bail!("InKey requires one argument")
            };
            let code = code.integer()? as u32;
            let keys = self
                .inputs
                .script_keys
                .context("InKey keyboard sample unavailable")?;
            let down = keys.in_key(code).with_context(|| {
                format!("InKey code {code:#x}: key without a recovered scan code")
            })?;
            return Ok(CallResult::Complete(i32::from(down)));
        }
        // GolemSteam: once every few updates, a puff of steam at one of the receiver's four weapon
        // attachment vertices (a grip or a scabbard point), in the alternating effect variants.
        if kind == CallKind::Function && function == Native::GolemSteam {
            anyhow::ensure!(arguments.is_empty(), "GolemSteam requires no arguments");
            let excluded = self.registry.actors[self.index]
                .control
                .as_ref()
                .is_some_and(|control| control.object_update_excluded);
            if excluded {
                return Ok(CallResult::Complete(0));
            }
            let random = self
                .random
                .as_deref_mut()
                .context("GolemSteam shared engine random stream unavailable")?;
            if random.rnd(4)? != 0 {
                return Ok(CallResult::Complete(0));
            }
            let field = ["grip0", "scabbard1", "grip1", "scabbard0"][random.rnd(4)? as usize];
            let id = self.registry.id(self.index)?;
            let rotated = self.attachment_offset(Native::GolemSteam, field)?;
            let position = self.registry.spatial(id)?.local.position;
            let point = std::array::from_fn(|axis| rotated[axis].wrapping_add(position[axis]));
            let descriptor = if self.registry.globals.world_loop_frame & 1 != 0 {
                0x32480
            } else {
                0x22480
            };
            self.registry.spawn_environment_effect(descriptor, point)?;
            return Ok(CallResult::Complete(0));
        }
        // DragonBreath: the dragon's smoke, fireball and death item swap; `DragonBreathTask` runs the
        // same update as a multitask. The return is one while the dragon keeps breathing.
        if kind == CallKind::Function && function == Native::DragonBreath {
            anyhow::ensure!(arguments.is_empty(), "DragonBreath requires no arguments");
            return Ok(CallResult::Complete(i32::from(self.breathe()?)));
        }
        // AddFlatEffect(x, y, z, spread, descriptor, speed) builds a type-one environmental effect
        // at a Q8 point, scattered by the spread when it is nonzero; the animation speed is retained.
        if kind == CallKind::Function && function == Native::AddFlatEffect {
            let [x, y, z, spread, descriptor, speed] = arguments else {
                anyhow::bail!("AddFlatEffect requires six arguments")
            };
            self.registry.spawn_environment_effect_with_speed(
                descriptor.integer()? as u32,
                [x.integer()?, y.integer()?, z.integer()?],
                speed.integer()?,
                spread.integer()?,
                self.random.as_deref_mut(),
            )?;
            return Ok(CallResult::Complete(0));
        }
        // AddLightEffect(a, b, c, d, e, f, g, h) allocates a type-two light record; it returns
        // the record (one-based slot) or zero when the pool is disabled or full.
        if kind == CallKind::Function && function == Native::AddLightEffect {
            let values = arguments
                .iter()
                .map(Argument::integer)
                .collect::<Result<Vec<_>>>()?;
            let values: [i32; 8] = values
                .try_into()
                .map_err(|_| anyhow::anyhow!("AddLightEffect requires eight arguments"))?;
            return Ok(CallResult::Complete(
                self.registry.add_light_effect(values)?,
            ));
        }
        // CountAiShells(name) counts the running shells of that name; FindAiShell(name) reports
        // whether one runs. Both walk the shell list; the receiver is not read.
        if kind == CallKind::Function
            && (function == Native::CountAiShells || function == Native::FindAiShell)
        {
            let [Argument::String(name)] = arguments else {
                anyhow::bail!("{function} requires one script name")
            };
            let first = function == Native::FindAiShell;
            return Ok(CallResult::Complete(
                self.registry.count_shell_matches(name, first)?,
            ));
        }
        // OpenShell(name) starts the named shell script; it runs from the next update.
        if kind == CallKind::Function && function == Native::OpenShell {
            let [Argument::String(name)] = arguments else {
                anyhow::bail!("OpenShell requires one script name")
            };
            self.registry.spawn_shell(name)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::ItemUsed {
            let [item] = arguments else {
                anyhow::bail!("ItemUsed requires one argument")
            };
            let value = self
                .inputs
                .hud
                .context("ItemUsed inventory host unavailable")?
                .item_used(item.integer()? as u32)?;
            return Ok(CallResult::Complete(value));
        }
        if kind == CallKind::Function && function == Native::UseItem {
            let [item] = arguments else {
                anyhow::bail!("UseItem requires one argument")
            };
            let item =
                u16::try_from(item.integer()?).context("UseItem item ID outside u16 range")?;
            self.use_item(item)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::SubItem {
            let [item, amount] = arguments else {
                anyhow::bail!("SubItem requires two arguments")
            };
            self.inputs
                .hud
                .context("SubItem inventory host unavailable")?
                .subtract_item(item.integer()? as u32, amount.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::PushItem {
            let [item, value] = arguments else {
                anyhow::bail!("PushItem requires two arguments")
            };
            let value = self
                .inputs
                .hud
                .context("PushItem inventory host unavailable")?
                .push_item(item.integer()? as u32, value.integer()?)?;
            return Ok(CallResult::Complete(value));
        }
        if kind == CallKind::Function && function == Native::PushAllItems {
            let [value] = arguments else {
                anyhow::bail!("PushAllItems requires one argument")
            };
            self.inputs
                .hud
                .context("PushAllItems inventory host unavailable")?
                .push_all_items(value.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::PopItem {
            let [item] = arguments else {
                anyhow::bail!("PopItem requires one argument")
            };
            let value = self
                .inputs
                .hud
                .context("PopItem inventory host unavailable")?
                .pop_item(item.integer()? as u32)?;
            return Ok(CallResult::Complete(value));
        }
        if kind == CallKind::Function && function == Native::PopAllItems {
            anyhow::ensure!(arguments.is_empty(), "PopAllItems requires no arguments");
            let hud = self
                .inputs
                .hud
                .context("PopAllItems inventory host unavailable")?;
            let player = self.selected_player();
            let generation = self.generation;
            let actors = &mut self.registry.actors;
            hud.pop_all_items(&mut |weapon| {
                let player = player.context("PopAllItems fallback selected player unavailable")?;
                anyhow::ensure!(
                    player.generation == generation && player.index < actors.len(),
                    "PopAllItems fallback selected player identity is stale"
                );
                actors[player.index].attributes[Attribute::Weapon] = weapon;
                Ok(())
            })?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::RemoveItem {
            let [item] = arguments else {
                anyhow::bail!("RemoveItem requires one argument")
            };
            let item =
                u16::try_from(item.integer()?).context("RemoveItem item ID outside u16 range")?;
            self.inputs
                .hud
                .context("RemoveItem inventory host unavailable")?
                .remove_item(item)?;
            return Ok(CallResult::Complete(0));
        }
        // chargeWeapon(1) swaps the player's sabre for the flaming sabre (the charged weapon) and any
        // other operand swaps back: add the new item, record it as the weapon attribute, empty the
        // old item and select the new one. A closed inventory skips the adds and removals, and the
        // selection then fails as the original's does.
        if kind == CallKind::Function && function == Native::ChargeWeapon {
            let [charge] = arguments else {
                anyhow::bail!("chargeWeapon requires one argument")
            };
            let (take, drop) = if charge.integer()? as i16 == 1 {
                (super::inventory::FLAMING_SABRE, super::inventory::SABRE)
            } else {
                (super::inventory::SABRE, super::inventory::FLAMING_SABRE)
            };
            self.add_session_item(take, 1)?;
            self.registry.actors[self.index].attributes[Attribute::Weapon] = take as u8;
            let hud = self
                .inputs
                .hud
                .context("chargeWeapon inventory host unavailable")?;
            hud.remove_item(drop)?;
            hud.select_item(take)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::ActiveItem {
            anyhow::ensure!(arguments.is_empty(), "ActiveItem requires no arguments");
            let selected = self
                .inputs
                .hud
                .context("ActiveItem inventory host unavailable")?
                .active_item()?;
            return Ok(CallResult::Complete(i32::from(selected)));
        }
        if kind == CallKind::Function && function == Native::HandItem {
            let [item] = arguments else {
                anyhow::bail!("HandItem requires one argument")
            };
            let hud = self
                .inputs
                .hud
                .context("HandItem inventory host unavailable")?;
            if hud.inventory_ready()? {
                self.registry.actors[self.index].hand_item(
                    item.integer()?,
                    hud,
                    self.inputs.shell_execution,
                )?;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && super::ScriptHud::handles(function) {
            let hud = self.inputs.hud.context("script HUD host unavailable")?;
            return Ok(CallResult::Complete(hud.call(function, arguments)?));
        }
        if kind == CallKind::Function && function == Native::LockoutPlayer {
            let [value] = arguments else {
                anyhow::bail!("LockoutPlayer requires one argument")
            };
            let value = value.integer()?;
            let player = self
                .inputs
                .activation
                .map(|inputs| inputs.player.index)
                .or_else(|| self.registry.selected_player_index())
                .context("LockoutPlayer selected player unavailable")?;
            let lockout = self.registry.globals.lockout_player(value);
            apply_player_lockout(
                &mut self.registry.actors,
                &mut self.registry.activation,
                Some(player),
                lockout,
            );
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function {
            let automatic;
            let activation = if self.inputs.activation.is_none()
                && (function == Native::Activate || function == Native::TorchActivate)
                && let Some(use_held) = self.inputs.activation_use_held
            {
                anyhow::ensure!(
                    self.inputs.shell_execution == Some(false),
                    "ordinary activation requires an established non-shell context"
                );
                let hud = self
                    .inputs
                    .hud
                    .context("activation HUD owner unavailable")?;
                automatic = activation::ordinary_inputs(
                    &self.registry.actors,
                    self.generation,
                    &self.registry.globals,
                    hud,
                    use_held,
                    Some((self.index, arguments)),
                )?;
                Some(&automatic)
            } else {
                self.inputs.activation
            };
            let selected_player = activation.map(|inputs| {
                (
                    &self.registry.actors[inputs.player.index],
                    self.registry.globals.lockout().unwrap_or(inputs.lockout),
                )
            });
            if let Some(value) = self.registry.activation.call(
                id,
                function,
                arguments,
                activation,
                &self.registry.actors[self.index].attributes,
                selected_player,
            )? {
                if value != 0 {
                    self.registry.globals.activation_consumed_use = true;
                }
                return Ok(CallResult::Complete(value));
            }
        }
        if let Some(result) = self.native_task_call(
            (function, super::tasks::NativeTaskMode::Script(kind)),
            arguments,
            invocation,
        )? {
            return Ok(result);
        }
        anyhow::ensure!(
            kind == CallKind::Function,
            "native {:?} {function} unsupported by inspection host",
            kind
        );
        // KillMyTasks clears all six of the receiver's task slots at once, without running a handler.
        if function == Native::KillMyTasks {
            anyhow::ensure!(arguments.is_empty(), "KillMyTasks requires no arguments");
            return Ok(CallResult::CompleteAfterClearingTasks(0));
        }
        if function == Native::ResetRotation {
            anyhow::ensure!(
                arguments.len() == 3,
                "ResetRotation requires three arguments"
            );
            let mut axes = [false; 3];
            for (axis, argument) in arguments.iter().enumerate() {
                axes[axis] = argument.integer()? != 0;
            }
            self.registry
                .spatial
                .as_mut()
                .context("ResetRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_rotation(axes)?;
            return Ok(CallResult::Complete(0));
        }
        // SetStartPosition(x, y, z) restores the flagged local-position axes from the saved start and
        // returns zero; unlike ResetPosition it leaves the position history, view origin and
        // transform alone.
        if function == Native::SetStartPosition {
            anyhow::ensure!(
                arguments.len() == 3,
                "SetStartPosition requires three arguments"
            );
            let mut axes = [false; 3];
            for (axis, argument) in arguments.iter().enumerate() {
                axes[axis] = argument.integer()? != 0;
            }
            if axes.contains(&true) {
                self.registry
                    .spatial
                    .as_mut()
                    .context("SetStartPosition: spatial state not prepared")?
                    .actor_mut(self.index)?
                    .restore_start_position(axes)?;
            }
            return Ok(CallResult::Complete(0));
        }
        // FixSlaveAngle(axis, angle) writes one local angle component of a slave from its master
        // group's angle and returns zero.
        if function == Native::FixSlaveAngle {
            let [axis, angle] = arguments else {
                anyhow::bail!("FixSlaveAngle requires two arguments")
            };
            let (axis, angle) = (axis.integer()?, angle.integer()?);
            let id = self.registry.id(self.index)?;
            self.registry.fix_slave_angle(id, axis, angle)?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::AttachMe {
            let [group] = arguments else {
                anyhow::bail!("AttachMe requires one argument")
            };
            let group = group.integer()?;
            let attributes = &self.registry.actors[self.index].attributes;
            if group == 0
                || group == i32::from(attributes[Attribute::Slave])
                || group == i32::from(attributes[Attribute::Master])
            {
                return Ok(CallResult::Complete(0));
            }
            anyhow::ensure!(
                attributes[Attribute::Master] == 0,
                "AttachMe: master receiver {} requires separate ordinary and master family records",
                self.index
            );
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("AttachMe: spatial state not prepared")?;
            if attributes[Attribute::Slave] != 0
                && spatial.actor_mut(self.index)?.detach_from_family()?
            {
                self.registry.actors[self.index].attributes[Attribute::Slave] = 0;
                self.registry.map_updates.register(self.index)?;
            }
            let mut targets =
                self.registry
                    .actors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, actor)| {
                        (i32::from(actor.attributes[Attribute::Master]) == group).then_some(index)
                    });
            if let Some(target) = targets.next() {
                anyhow::ensure!(
                    targets.next().is_none(),
                    "AttachMe: group {group} has multiple masters; family traversal order unavailable"
                );
                let no_rotation =
                    self.registry.actors[self.index].attributes[Attribute::NoSlaveRot] != 0;
                spatial.attach_ordinary(self.index, target, no_rotation)?;
                self.registry.actors[self.index].attributes[Attribute::Slave] = group as u8;
            }
            return Ok(CallResult::Complete(0));
        }
        // DetachMe: an unattached receiver is unchanged; an attached master only loses its slave
        // byte; an attached non-master leaves its family, keeps its effective pose as its local
        // pose, loses its slave byte and registers a map update. A receiver with no family record
        // is unchanged. Returns zero.
        if function == Native::DetachMe {
            anyhow::ensure!(arguments.is_empty(), "DetachMe requires no arguments");
            let attributes = &self.registry.actors[self.index].attributes;
            if attributes[Attribute::Slave] == 0 {
                return Ok(CallResult::Complete(0));
            }
            if attributes[Attribute::Master] != 0 {
                self.registry.actors[self.index].attributes[Attribute::Slave] = 0;
                return Ok(CallResult::Complete(0));
            }
            let detached = self
                .registry
                .spatial
                .as_mut()
                .context("DetachMe: spatial state not prepared")?
                .actor_mut(self.index)?
                .detach_from_family()?;
            if detached {
                self.registry.actors[self.index].attributes[Attribute::Slave] = 0;
                self.registry.map_updates.register(self.index)?;
            }
            return Ok(CallResult::Complete(0));
        }
        // SetPosition(x, y, z) and SetLocation(location) write the local position and its
        // history unchanged, move the selected player's shared reference triple (cyrus's
        // published camera position, **Inferred** as for ResetPosition) by the same displacement
        // and register the actor for the deferred map update; SetRotation(x, y, z) masks and
        // writes the angles and rebuilds the matrix without registering. All return zero.
        // SetObjectLocation(location, x, y, z): the selected location's coordinates are copied into
        // the local position on the axes whose flag is nonzero; the receiver is registered for the
        // deferred map update even when every flag is zero. Returns zero.
        if function == Native::SetObjectLocation {
            let [location, x, y, z] = arguments else {
                anyhow::bail!("SetObjectLocation requires four arguments")
            };
            let flags = [x.integer()?, y.integer()?, z.integer()?];
            self.registry
                .spatial
                .as_mut()
                .context("SetObjectLocation: spatial state not prepared")?
                .actor_mut(self.index)?
                .set_location_axes(location.integer()?, flags)?;
            self.registry.map_updates.register(self.index)?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::SetPosition || function == Native::SetLocation {
            let (name, expected) = if function == Native::SetPosition {
                ("SetPosition", 3)
            } else {
                ("SetLocation", 1)
            };
            anyhow::ensure!(
                arguments.len() == expected,
                "{name} requires {expected} argument{}",
                if expected == 1 { "" } else { "s" }
            );
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .with_context(|| format!("{name}: spatial state not prepared"))?
                .actor_mut(self.index)?;
            let position = if function == Native::SetPosition {
                [
                    arguments[0].integer()?,
                    arguments[1].integer()?,
                    arguments[2].integer()?,
                ]
            } else {
                spatial.location_position(arguments[0].integer()?)?
            };
            let change = spatial.set_position(position);
            if self.registry.selected_player_index() == Some(self.index)
                && let Some(camera) = self.registry.camera.as_mut()
            {
                for (axis, delta) in change.into_iter().enumerate() {
                    camera.current.position[axis] =
                        camera.current.position[axis].wrapping_add(delta);
                }
            }
            self.registry.map_updates.register(self.index)?;
            return Ok(CallResult::Complete(0));
        }
        // PlayerSnap(angle, x_offset, z_offset): the selected player's local X and Z are copied
        // directly from the receiver's. The arguments only feed a temporary rotation whose result
        // the original discards; Y, heading and both history fields stay, nothing is registered
        // and no position-update or collision service runs. Returns zero.
        if function == Native::PlayerSnap {
            anyhow::ensure!(arguments.len() == 3, "PlayerSnap requires 3 arguments");
            for argument in arguments {
                argument.integer()?;
            }
            let player = self
                .registry
                .selected_player_index()
                .context("PlayerSnap: selected player unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("PlayerSnap: spatial state not prepared")?;
            let source = spatial.actor(self.index)?.local.position;
            let target = &mut spatial.actor_mut(player)?.local.position;
            target[0] = source[0];
            target[2] = source[2];
            return Ok(CallResult::Complete(0));
        }
        if function == Native::OffsetLocation {
            let [source, x, y, z] = arguments else {
                anyhow::bail!("OffsetLocation requires four arguments")
            };
            let source = source.actor().context("OffsetLocation source")?;
            let offset = [x.integer()?, y.integer()?, z.integer()?];
            let base = self
                .registry
                .query_position(source)
                .context("OffsetLocation source position")?;
            let receiver = self.registry.id(self.index)?;
            // Receiver resolution precedes transformation even though its result is discarded.
            self.registry
                .query_position(receiver)
                .context("OffsetLocation receiver position")?;
            let transformed = self
                .registry
                .spatial(source)?
                .query_pose()?
                .matrix
                .transform(offset)
                .context("OffsetLocation source matrix")?;
            self.registry
                .spatial
                .as_mut()
                .context("OffsetLocation: spatial state not prepared")?
                .actor_mut(self.index)?
                .local
                .position = std::array::from_fn(|axis| base[axis].wrapping_add(transformed[axis]));
            return Ok(CallResult::Complete(0));
        }
        if function == Native::SetRotation {
            anyhow::ensure!(arguments.len() == 3, "SetRotation requires 3 arguments");
            let angles = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            self.registry
                .spatial
                .as_mut()
                .context("SetRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .set_rotation(angles)?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::ResetPosition {
            anyhow::ensure!(
                arguments.len() == 3,
                "ResetPosition requires three arguments"
            );
            let mut axes = [false; 3];
            for (axis, argument) in arguments.iter().enumerate() {
                axes[axis] = argument.integer()? != 0;
            }
            let change = self
                .registry
                .spatial
                .as_mut()
                .context("ResetPosition: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_position(axes)?;
            // The original moves its view-origin global, which cyrus keeps as the published
            // camera position, by the selected player's change (**Inferred** mapping).
            if self.registry.selected_player_index() == Some(self.index)
                && let Some(camera) = self.registry.camera.as_mut()
            {
                for (axis, delta) in change.into_iter().enumerate() {
                    camera.current.position[axis] =
                        camera.current.position[axis].wrapping_add(delta);
                }
            }
            if self.registry.spatial(id)?.parent.is_none() {
                self.registry.refresh_transform(id)?;
            }
            return Ok(CallResult::Complete(0));
        }
        // PointAt turns the receiver to face the shared native-command source actor: the negated shared query bearing of the truncated stored model coordinate
        // differences, written straight to the receiver's yaw without a matrix refresh.
        if function == Native::PointAt {
            anyhow::ensure!(arguments.is_empty(), "PointAt requires no arguments");
            let source = self.native_source_actor("PointAt")?.index;
            let stored = |index: usize, role: &str| {
                self.registry.actors[index]
                    .model
                    .position
                    .with_context(|| format!("PointAt: {role} stored model position unavailable"))
            };
            let (target, own) = (stored(source, "target")?, stored(self.index, "receiver")?);
            // Extended-precision differences truncate toward zero; an unrepresentable one converts
            // to the integer-indefinite value.
            let truncate = |axis: usize| {
                let difference = (f64::from(target[axis]) - f64::from(own[axis])).trunc();
                if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&difference) {
                    difference as i32
                } else {
                    i32::MIN
                }
            };
            // The original negates its bearing helper's angle; `query_bearing` is that helper negated.
            let yaw = super::spatial::query_bearing(truncate(0), truncate(2));
            self.registry
                .spatial
                .as_mut()
                .context("PointAt: spatial state not prepared")?
                .actor_mut(self.index)?
                .local
                .angles[1] = i32::from(yaw);
            return Ok(CallResult::Complete(0));
        }
        // MoveAboveMe places the receiver relative to the shared native-command source actor: the source
        // position plus the offset turned by the source's matrix, lowered by the receiver's
        // vertical extent and raised by the installed standing height. The selected player is
        // placed through the pending placement; any other receiver is written directly and
        // registered in the changed-actor list.
        if function == Native::MoveAboveMe {
            anyhow::ensure!(arguments.len() == 4, "MoveAboveMe requires four arguments");
            let source = self.native_source_actor("MoveAboveMe")?;
            anyhow::ensure!(
                self.registry.actors[source.index].attributes[Attribute::Slave] == 0,
                "MoveAboveMe world {} actor {}: original diagnostic 137: slave source",
                self.registry.world,
                source.index
            );
            let offset = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            let angle = arguments[3].integer()?;
            let standing_height = self
                .inputs
                .player_collision
                .map(|source| source.scene.settings.standing_height)
                .or(self.registry.standing_height)
                .context("MoveAboveMe: standing height unavailable")?;
            let rotated = self
                .registry
                .spatial(source)?
                .local
                .matrix
                .transform(offset)?;
            let base = self.registry.query_position(source)?;
            let extent = self.registry.ordinary_collision_shape(id)?.positive_extent[1];
            let mut target: [i32; 3] =
                std::array::from_fn(|axis| base[axis].wrapping_add(rotated[axis]));
            target[1] = target[1]
                .wrapping_sub(extent.wrapping_shl(8))
                .wrapping_add(standing_height.wrapping_shl(8));
            if self.registry.selected_player_index() == Some(self.index) {
                self.registry.pending_player_placement = Some((target, wrap(angle) as u16));
            } else {
                let receiver = self
                    .registry
                    .spatial
                    .as_mut()
                    .context("MoveAboveMe: spatial state not prepared")?
                    .actor_mut(self.index)?;
                receiver.local.position = target;
                receiver.local.angles[1] = angle & 2047;
                self.registry.map_updates.register(self.index)?;
            }
            return Ok(CallResult::Complete(0));
        }
        if function == Native::MovePlayerAboveMe {
            anyhow::ensure!(
                arguments.len() == 4,
                "MovePlayerAboveMe requires four arguments"
            );
            let offset = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            let yaw = arguments[3].integer()?;
            let player = self
                .registry
                .selected_player()
                .context("MovePlayerAboveMe: selected player unavailable")?;
            let standing_height = self
                .registry
                .standing_height
                .context("MovePlayerAboveMe: standing height unavailable")?;
            // The offset turns with the receiver, the player stands on the result, and its
            // vertical extent and the standing height lower the stored Y.
            let rotated = self.registry.spatial(id)?.local.matrix.transform(offset)?;
            let base = self.registry.query_position(id)?;
            let extent = self
                .registry
                .ordinary_collision_shape(player)?
                .positive_extent[1];
            let mut position: [i32; 3] =
                std::array::from_fn(|axis| base[axis].wrapping_add(rotated[axis]));
            position[1] = position[1]
                .wrapping_sub(extent.wrapping_shl(8))
                .wrapping_sub(standing_height.wrapping_shl(8));
            self.registry.pending_player_placement = Some((position, wrap(yaw) as u16));
            return Ok(CallResult::Complete(0));
        }
        if function == Native::CameraController {
            anyhow::ensure!(
                arguments.is_empty(),
                "cameraController requires no arguments"
            );
            let service = self
                .inputs
                .camera_update
                .context("cameraController: camera update service unavailable")?;
            self.registry.run_camera_controller(service, self.inputs)?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::XResetCamera {
            anyhow::ensure!(arguments.is_empty(), "XResetCamera requires no arguments");
            let service = self
                .inputs
                .camera_update
                .context("XResetCamera: camera update service unavailable")?;
            let player = self
                .registry
                .selected_player_index()
                .context("XResetCamera: selected player unavailable")?;
            let camera = self
                .registry
                .camera
                .as_ref()
                .context("XResetCamera: camera session unavailable")?
                .camera_actor
                .index;
            anyhow::ensure!(
                self.index != player && self.index != camera,
                "XResetCamera called by the player or camera actor is unsupported"
            );
            let reset = self.registry.reset_camera_session(player, service);
            self.finish_camera_update(reset.is_ok())?;
            reset?;
            let lockout = self.registry.globals.lockout_player(0);
            apply_player_lockout(
                &mut self.registry.actors,
                &mut self.registry.activation,
                Some(player),
                lockout,
            );
            return Ok(CallResult::Complete(0));
        }
        if function == Native::ReloadRotation {
            anyhow::ensure!(arguments.is_empty(), "ReloadRotation requires no arguments");
            self.registry
                .spatial
                .as_mut()
                .context("ReloadRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_rotation([false; 3])?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::Resurrect {
            anyhow::ensure!(arguments.is_empty(), "Resurrect requires no arguments");
            self.registry.release_held_walk_nodes(id)?;
            let resurrected = {
                let actor = &mut self.registry.actors[self.index];
                let control = actor
                    .control
                    .as_mut()
                    .context("Resurrect: receiver lifecycle unavailable")?;
                if control.killed || control.ai_type == AiType::DEAD {
                    control.killed = false;
                    // A selector-6 receiver with combat state first runs the unconditional part
                    // of the end-of-combat reset; the script-control bit stays (it is only
                    // cleared there while the killed flag is still set).
                    if control.ai_type == AiType::COMBAT {
                        let script_control = control.script_control;
                        super::combat::end_combat_reset(control);
                        control.script_control = script_control;
                    }
                    if let Some(profile) =
                        control.combat.as_ref().map(|record| record.contact_profile)
                    {
                        control.combat = super::CombatControlState::initialize(
                            &actor.attributes,
                            control.combat.as_ref(),
                            &mut self.registry.next_combat_index,
                            &self.registry.combat_attacks,
                            profile,
                        )
                        .context("Resurrect: combat reinitialization")?;
                    }
                    true
                } else {
                    false
                }
            };
            // The original consumes one waiting class message as part of the combat
            // resurrection path.  Keep transport separate from combat response: a
            // message for another receiver is skipped by the queue's addressed cursor.
            if resurrected
                && self.registry.actors[self.index]
                    .control
                    .as_ref()
                    .is_some_and(|control| control.combat.is_some())
            {
                let _ = self.registry.consume_combat_message(id)?;
            }
            return Ok(CallResult::Complete(0));
        }
        if function == Native::KillMe {
            anyhow::ensure!(arguments.is_empty(), "KillMe requires no arguments");
            self.registry.release_held_walk_nodes(id)?;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .context("KillMe: receiver lifecycle unavailable")?;
            control.killed = true;
            control.ai_type = AiType::DEAD;
            self.registry.actors[self.index].model.current = None;
            self.registry.actors[self.index].model.weapon = None;
            // The original frees the receiver's light and flat records, so a later call starts new ones.
            self.registry.actors[self.index].light = None;
            self.registry.actors[self.index].flat = None;
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id, super::ScriptSound::StopOwned
            )?;
            // The VM cancels the receiver's runtime task slots after this function returns, while
            // its runtime is still borrowed by the invocation.
            return Ok(CallResult::CompleteAfterClearingTasks(0));
        }
        if function == Native::HideMe || function == Native::ShowMe {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| {
                    format!("{function}: receiver visibility lifecycle unavailable")
                })?;
            control.hidden = function == Native::HideMe;
            if control.hidden {
                self.registry.actors[self.index].model.current = None;
            } else if self.registry.actors[self.index]
                .model
                .static_radius
                .is_some()
                || (self.registry.actors[self.index].model.prepared && {
                    let data = self
                        .inputs
                        .animations
                        .context("ShowMe: model path metadata unavailable")?
                        .0;
                    self.registry.require_model_identity(data)?;
                    matches!(
                        data.actor(self.registry.actors[self.index].name())?["model_load_path"]
                            .as_str(),
                        Some("static")
                    )
                })
            {
                let spatial = self
                    .registry
                    .spatial
                    .as_mut()
                    .context("ShowMe: model spatial state unavailable")?;
                let state = spatial.actor_mut(self.index)?;
                if state.parent.is_none() {
                    state.refresh_transform()?;
                }
                self.registry.actors[self.index]
                    .model
                    .show_static(state.query_pose()?);
            }
            return Ok(CallResult::Complete(0));
        }
        if function == Native::CheckPlayerWeapon {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            // The supplied receiver is replaced by the selected player; a closed item system is a
            // no-op. The original's return register is residual state, not a result.
            let hud = self
                .inputs
                .hud
                .with_context(|| format!("{function}: inventory owner unavailable"))?;
            if hud.inventory_ready()? {
                let player = self
                    .selected_player()
                    .with_context(|| format!("{function}: selected player unavailable"))?;
                let shell = self
                    .inputs
                    .shell_execution
                    .with_context(|| format!("{function}: shell context unavailable"))?;
                let ticks = self
                    .inputs
                    .nominal_ticks
                    .with_context(|| format!("{function}: nominal ticks unavailable"))?;
                let sounds = self
                    .sounds
                    .as_mut()
                    .with_context(|| format!("{function}: script sound handler absent"))?;
                self.registry
                    .check_player_weapon(player, hud, shell, ticks, &mut **sounds)
                    .with_context(|| function.to_string())?;
            }
            return Ok(CallResult::Complete(0));
        }
        if matches!(
            function,
            Native::IsLightingTorch | Native::IsUnlightingTorch | Native::IsLightingOrUnlighting
        ) {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(
                self.receiver_torch_predicate(function)
                    .with_context(|| function.to_string())?,
            )));
        }
        if function == Native::IsCarryingWeapon {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(
                self.receiver_carrying_weapon()
                    .with_context(|| function.to_string())?,
            )));
        }
        if [
            Native::IsHoldingWeapon,
            Native::IsDrawingSword,
            Native::IsSheathingSword,
            Native::IsDrawingOrSheathing,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let (holding, drawing, sheathing) = self.receiver_weapon_predicates(function)?;
            let result = if function == Native::IsHoldingWeapon {
                holding
            } else if function == Native::IsDrawingSword {
                drawing
            } else if function == Native::IsDrawingOrSheathing {
                drawing || sheathing
            } else {
                sheathing
            };
            return Ok(CallResult::Complete(i32::from(result)));
        }
        if [
            Native::ZapObject,
            Native::ZapObjectNoKill,
            Native::ZapPlayer,
            Native::ZapPlayerNoKill,
        ]
        .contains(&function)
        {
            return self.zap(function, arguments);
        }
        if function == Native::SetHealth || function == Native::AdjustHealth {
            let set = function == Native::SetHealth;
            anyhow::ensure!(
                arguments.len() == if set { 1 } else { 2 },
                "{function}: wrong argument count"
            );
            let selected = self
                .selected_player()
                .is_some_and(|player| player.index == self.index);
            if selected {
                if !set {
                    let id = self.registry.id(self.index)?;
                    let sounds = &mut self.sounds;
                    self.registry.apply_selected_health_command(
                        id,
                        arguments[0].integer()?,
                        arguments[1].integer()?,
                        self.inputs,
                        self.random.as_deref_mut(),
                        &mut |actor, sound| {
                            sounds
                                .as_deref_mut()
                                .context("selected health sound executor unavailable")?(
                                actor, sound,
                            )
                        },
                    )?;
                    return Ok(CallResult::Complete(0));
                }
                self.inputs
                    .hud
                    .with_context(|| format!("{function}: selected-player HUD owner unavailable"))?
                    .request_replenish_health_feedback();
            }
            let attributes = &mut self.registry.actors[self.index].attributes;
            // Generic admission follows the native selected-player feedback update.
            if attributes[Attribute::Invulnerable] != 0 {
                return Ok(CallResult::Complete(0));
            }
            if attributes[Attribute::Npc] != 0
                && !self.receiver_weapon_predicates(Native::IsHoldingWeapon)?.0
                && self.registry.actors[self.index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .is_none_or(|combat| !combat.flags.contains(CombatFlags::STANCE))
            {
                return Ok(CallResult::Complete(0));
            }
            if selected {
                let id = self.registry.id(self.index)?;
                let change = arguments[0].integer()?.wrapping_sub(i32::from(
                    self.registry.actors[self.index].attributes[Attribute::Health],
                ));
                let sounds = &mut self.sounds;
                self.registry.apply_selected_death_health(
                    id,
                    change,
                    None,
                    self.inputs,
                    self.random.as_deref_mut(),
                    &mut |actor, sound| {
                        sounds
                            .as_deref_mut()
                            .context("selected death sound executor unavailable")?(
                            actor, sound
                        )
                    },
                )?;
                return Ok(CallResult::Complete(0));
            }
            let value = arguments[0].integer()?;
            let health = i32::from(self.registry.actors[self.index].attributes[Attribute::Health]);
            let delta = if set {
                value.wrapping_sub(health)
            } else {
                value
            };
            let kills = set || arguments[1].integer()? == 1;
            self.adjust_nonplayer_health(function, delta, kills)?;
            return Ok(CallResult::Complete(0));
        }
        // PlayerMoved compares the selected player's position with its snapshot taken at the
        // ordinary actor-update boundary.
        if function == Native::PlayerMoved {
            anyhow::ensure!(arguments.is_empty(), "PlayerMoved requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerMoved: selected player unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(player.index)?;
            return Ok(CallResult::Complete(i32::from(
                spatial.local.position != spatial.previous_local_position,
            )));
        }
        if function == Native::IsBouncing {
            anyhow::ensure!(arguments.is_empty(), "IsBouncing requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_ref()
                .context("IsBouncing receiver control unavailable")?;
            return Ok(CallResult::Complete(i32::from(
                control.living_state.is_bounce(),
            )));
        }
        if function == Native::GetMovementSpeed {
            anyhow::ensure!(
                arguments.is_empty(),
                "GetMovementSpeed requires no arguments"
            );
            return Ok(CallResult::Complete(i32::from(
                self.registry.ordinary_group_speed(self.index),
            )));
        }
        if function == Native::MovePos {
            anyhow::ensure!(arguments.len() == 3, "MovePos requires three arguments");
            let destination = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            return Ok(CallResult::Complete(self.registry.move_position_pending(
                id,
                destination,
                self.inputs,
            )?));
        }
        // PlayerTurned, PlayerTurnedDirection and PlayerMovementSpeed read the selected player; the
        // original also returns zero for an actor without its model handle, assumed present here.
        if [
            Native::PlayerTurned,
            Native::PlayerTurnedDirection,
            Native::PlayerMovementSpeed,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .with_context(|| format!("{function}: selected player unavailable"))?;
            let index = self.registry.resolve(player)?;
            if function == Native::PlayerMovementSpeed {
                let actor = &self.registry.actors[index];
                let animation = actor
                    .animation
                    .as_ref()
                    .context("PlayerMovementSpeed: selected-player animation unavailable")?;
                let value = animation
                    .playback
                    .current_group()
                    .and_then(super::player_status::movement_speed_attribute)
                    .map_or(0, |attribute| i32::from(actor.attributes[attribute]));
                return Ok(CallResult::Complete(value));
            }
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(index)?;
            let value = if function == Native::PlayerTurned {
                super::player_status::player_turned(spatial.local.angles, spatial.angle_history)
            } else {
                super::player_status::player_turned_direction(
                    spatial.local.angles[1],
                    spatial.angle_history[1],
                )
            };
            return Ok(CallResult::Complete(value));
        }
        // PlayerOnScape: the selected player's last ground response was a terrain contact.
        if function == Native::PlayerOnScape {
            anyhow::ensure!(arguments.is_empty(), "PlayerOnScape requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerOnScape: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("PlayerOnScape: selected-player control unavailable")?;
            return Ok(CallResult::Complete(i32::from(
                control.ground_kind == super::registry::ground::GroundKind::TERRAIN,
            )));
        }
        // PlayerHanging reports whether the selected player is hanging from this receiver's
        // model. The ledge transition retains the supporting actor separately from ordinary
        // ground ownership; static ledges therefore return zero. This is the identity contract
        // published by preservation's PlayerHanging native.
        if function == Native::PlayerHanging {
            anyhow::ensure!(arguments.is_empty(), "PlayerHanging requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerHanging: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("PlayerHanging: selected-player control unavailable")?;
            let receiver = self.registry.id(self.index)?;
            let hanging = control.living_state == LivingState::HANGING
                && self.registry.actors[index].model.ledge_support == Some(receiver);
            return Ok(CallResult::Complete(i32::from(hanging)));
        }
        // PlayerStatus classifies the selected player; the motion bytes are the inferred
        // actor bytes +0x1b1 through +0x1b3 and the living state +0x1b4.
        if function == Native::PlayerStatus {
            anyhow::ensure!(arguments.is_empty(), "PlayerStatus requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerStatus: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let actor = &self.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("PlayerStatus: selected-player control unavailable")?;
            let animation = actor
                .animation
                .as_ref()
                .context("PlayerStatus: selected-player animation unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(index)?;
            return Ok(CallResult::Complete(super::player_status::player_status(
                super::player_status::PlayerStatusInput {
                    queued_motion: control.queued_motion,
                    falling_state: control.falling_state,
                    sliding_state: control.sliding_state,
                    living_state: control.living_state,
                    group: animation.playback.current_group(),
                    moved: spatial.local.position != spatial.previous_local_position,
                },
            ) as i32));
        }
        // The dialogue and topic-menu mode words. DialogBegin also empties the remembered topic
        // text, the first byte of the buffer the menu compares new entries against.
        if let Some((dialogue, begin)) = match function {
            Native::DialogBegin => Some((true, true)),
            Native::DialogEnd => Some((true, false)),
            Native::MenuBegin => Some((false, true)),
            Native::MenuEnd => Some((false, false)),
            _ => None,
        } {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            match (dialogue, begin) {
                (true, begin) => {
                    if begin {
                        self.registry.globals.forget_choice();
                    }
                    self.registry.globals.set_dialogue_state(begin)
                }
                (false, true) => self.registry.globals.begin_topic_menu(),
                (false, false) => self.registry.globals.end_topic_menu(),
            }
            return Ok(CallResult::Complete(0));
        }
        // IsWithinMap is constant true. LoadWorld, WonGame, PushPlayer and PopPlayer only raise
        // a request that the original's main loop consumes; the host takes it from the globals.
        if function == Native::IsWithinMap {
            return Ok(CallResult::Complete(1));
        }
        // GetMessageSender returns the sender of the receiver's selected class message as an actor
        // reference, or zero when nothing is selected or the entry has no sender.
        if function == Native::GetMessageSender {
            anyhow::ensure!(
                arguments.is_empty(),
                "GetMessageSender requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(match self.registry.selected_message_sender(receiver)? {
                Some(sender) => {
                    self.registry.globals.actor_result = Some(sender);
                    CallResult::CompleteWithoutValue
                }
                None => CallResult::Complete(0),
            });
        }
        // GetBroadcastSender returns the selected broadcast entry's sender pointer, unchanged, as an
        // actor reference; zero when nothing is selected or the entry has no sender.
        if function == Native::GetBroadcastSender {
            anyhow::ensure!(
                arguments.is_empty(),
                "GetBroadcastSender requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(match self.registry.selected_broadcast_sender(receiver)? {
                Some(sender) => {
                    self.registry.globals.actor_result = Some(sender);
                    CallResult::CompleteWithoutValue
                }
                None => CallResult::Complete(0),
            });
        }
        // GetMessageSenderId returns the selected entry's sender's runtime identifier. The original
        // reads the sender without a null check, so a selected entry without one is an error here.
        if function == Native::GetMessageSenderId || function == Native::GetBroadcastSenderId {
            anyhow::ensure!(arguments.is_empty(), "{function:?} requires no arguments");
            let receiver = self.registry.id(self.index)?;
            let entry = if function == Native::GetMessageSenderId {
                self.registry.selected_message_entry(receiver)?
            } else {
                self.registry.selected_broadcast_entry(receiver)?
            };
            let Some(entry) = entry else {
                return Ok(CallResult::Complete(0));
            };
            let sender =
                entry.with_context(|| format!("{function:?}: selected entry has no sender"))?;
            let index = self
                .registry
                .resolve(sender)
                .with_context(|| format!("{function:?} sender"))?;
            return Ok(CallResult::Complete(i32::from(
                self.registry.actors[index]
                    .runtime_identifier
                    .with_context(|| {
                        format!("{function:?}: sender runtime identifier unavailable")
                    })?,
            )));
        }
        // SendMessage(recipient, value) writes sender (the receiver), recipient and value on the
        // recipient's class channel. The original takes the value from a third operand, so the
        // declared single-argument form passes the receiver token as the value and an integer
        // recipient is read as a pointer; neither is representable and both fail here. A zero
        // value returns before the recipient is read.
        if function == Native::SendMessage {
            let [recipient, value] = arguments else {
                anyhow::bail!(
                    "SendMessage requires a recipient actor reference and a value: the original reads the value from an operand the declared single argument does not supply"
                )
            };
            if value.integer()? == 0 {
                return Ok(CallResult::Complete(0));
            }
            let recipient = recipient.actor().context("SendMessage recipient")?;
            let sender = self.registry.id(self.index)?;
            self.registry
                .send_combat_message(sender, recipient, value.integer()?.into())?;
            return Ok(CallResult::Complete(0));
        }
        // IsMessageWaiting is one when the receiver's selection differs from its channel's
        // no-selection index; it scans nothing and changes nothing.
        if function == Native::IsMessageWaiting {
            anyhow::ensure!(
                arguments.is_empty(),
                "IsMessageWaiting requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(CallResult::Complete(i32::from(
                self.registry.selected_message_entry(receiver)?.is_some(),
            )));
        }
        // SendBroadcast appends the value to channel zero with the receiver as sender and no
        // recipient. A zero value returns before anything is read or written.
        if function == Native::SendBroadcast {
            let [value] = arguments else {
                anyhow::bail!("SendBroadcast requires one value argument")
            };
            let sender = self.registry.id(self.index)?;
            self.registry
                .send_broadcast(sender, value.integer()?.into())?;
            return Ok(CallResult::Complete(0));
        }
        // ReplyBroadcastSender answers the selected broadcast's sender on channel zero.
        if function == Native::ReplyBroadcastSender {
            let [value] = arguments else {
                anyhow::bail!("ReplyBroadcastSender requires one value")
            };
            let receiver = self.registry.id(self.index)?;
            self.registry
                .reply_to_broadcast_sender(receiver, value.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        // IsBroadcastWaiting is one when the receiver's broadcast selection differs from the
        // no-selection index; it scans nothing and changes nothing.
        if function == Native::IsBroadcastWaiting {
            anyhow::ensure!(
                arguments.is_empty(),
                "IsBroadcastWaiting requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(CallResult::Complete(i32::from(
                self.registry.selected_broadcast_entry(receiver)?.is_some(),
            )));
        }
        // ReceiveBroadcast is ReceiveMessage over the broadcast selection and channel zero.
        if function == Native::ReceiveBroadcast {
            anyhow::ensure!(
                arguments.is_empty(),
                "ReceiveBroadcast requires no arguments"
            );
            let receiver = self.registry.id(self.index)?;
            return Ok(CallResult::Complete(
                self.registry.receive_broadcast(receiver)?,
            ));
        }
        // ReplySender sends the value to the sender of the receiver's selected message.
        if function == Native::ReplySender {
            let [value] = arguments else {
                anyhow::bail!("ReplySender requires one value")
            };
            let receiver = self.registry.id(self.index)?;
            self.registry
                .reply_to_message_sender(receiver, value.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        // ReceiveMessage returns the value word of the first entry addressed to the receiver.
        if function == Native::ReceiveMessage {
            anyhow::ensure!(arguments.is_empty(), "ReceiveMessage requires no arguments");
            let receiver = self.registry.id(self.index)?;
            return Ok(CallResult::Complete(
                self.registry.receive_message(receiver)?,
            ));
        }
        // DeleteMessage advances the receiver's message selection and returns zero.
        if function == Native::DeleteMessage {
            anyhow::ensure!(arguments.is_empty(), "DeleteMessage requires no arguments");
            let receiver = self.registry.id(self.index)?;
            self.registry.advance_message_selection(receiver)?;
            return Ok(CallResult::Complete(0));
        }
        // MyId is the actor's signed 16-bit runtime identifier (pool index plus three).
        if function == Native::MyId {
            return Ok(CallResult::Complete(i32::from(
                self.registry.actors[self.index]
                    .runtime_identifier
                    .context("MyId: runtime identifier unavailable")?,
            )));
        }
        // Animal ignores its arguments and returns zero.
        if function == Native::Animal {
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::SpeedScale {
            let [value] = arguments else {
                anyhow::bail!("SpeedScale requires one argument")
            };
            let scale = self
                .inputs
                .frame_scale
                .context("SpeedScale requires frame_scale")?;
            return Ok(CallResult::Complete(
                value.integer()?.wrapping_mul(scale as i32) >> 8,
            ));
        }
        if kind == CallKind::Function && function == Native::MainloopExit {
            let [code] = arguments else {
                anyhow::bail!("MainloopExit requires one argument")
            };
            let code = code.integer()?;
            if code >= 5000 {
                self.registry.globals.request_mut().mainloop_exit = Some(code);
            }
            return Ok(CallResult::Complete(0));
        }
        // updatePlayerViewAttrib is an empty routine in the original, with no portable result.
        if kind == CallKind::Function && function == Native::UpdatePlayerViewAttrib {
            anyhow::ensure!(
                arguments.is_empty(),
                "updatePlayerViewAttrib requires no arguments"
            );
            return Ok(CallResult::CompleteWithoutValue);
        }
        if kind == CallKind::Function && function == Native::QueueMovie {
            let [movie] = arguments else {
                anyhow::bail!("QueueMovie requires one argument")
            };
            self.registry.globals.queue_movie(movie.integer()?);
            return Ok(CallResult::Complete(0));
        }
        if let Some(enabled) = match function {
            Native::EnableNodeMaps => Some(true),
            Native::DisableNodeMaps => Some(false),
            _ => None,
        } {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            self.registry.set_all_walk_maps_enabled(enabled)?;
            return Ok(CallResult::Complete(0));
        }
        // HoldAnimation, PauseAnimation and ReleaseAnimation write the receiver's animation wait
        // byte: 255 holds, a pause value counts nominal ticks, release clears only a hold.
        if kind == CallKind::Function
            && [
                Native::HoldAnimation,
                Native::PauseAnimation,
                Native::ReleaseAnimation,
            ]
            .contains(&function)
        {
            let wait = match (arguments, function) {
                ([], Native::HoldAnimation) => Some(u8::MAX),
                ([value], Native::PauseAnimation) => Some(value.integer()? as u8),
                ([], Native::ReleaseAnimation) => None,
                _ => anyhow::bail!("{function}: wrong argument count"),
            };
            if let Some(animation) = self.registry.actors[self.index].animation.as_mut() {
                match wait {
                    Some(wait) => animation.playback.set_wait(wait)?,
                    None => animation.playback.release_hold()?,
                }
            }
            return Ok(CallResult::Complete(0));
        }
        if function == Native::LoadWorld {
            let [world, a, b] = arguments else {
                anyhow::bail!("LoadWorld requires three arguments")
            };
            self.registry.globals.request_mut().load_world =
                Some([world.integer()?, a.integer()?, b.integer()?]);
            return Ok(CallResult::Complete(0));
        }
        if function == Native::WonGame {
            self.registry.globals.request_mut().won_game = true;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::PushPlayer || function == Native::PopPlayer {
            let request = self.registry.globals.request_mut();
            if function == Native::PushPlayer {
                request.push_player = true;
            } else {
                request.pop_player = true;
            }
            return Ok(CallResult::Complete(0));
        }
        // IsMaster and IsSlave test attr_master and attr_slave; TrueFunction ignores both arguments.
        if function == Native::IsMaster || function == Native::IsSlave {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let attribute = if function == Native::IsMaster {
                Attribute::Master
            } else {
                Attribute::Slave
            };
            return Ok(CallResult::Complete(i32::from(
                self.registry.actors[self.index].attributes[attribute] != 0,
            )));
        }
        // EnableSlaves, DisableSlaves, KillSlaves, HideSlaves, ShowSlaves and DetachSlaves search the
        // family tree for the receiver, but the
        // shared lookup never selects a node. For a finite acyclic family they change nothing,
        // leave the global slave scheduling alone and return zero. Cyclic families are outside
        // the contract.
        if [
            Native::EnableSlaves,
            Native::DisableSlaves,
            Native::KillSlaves,
            Native::HideSlaves,
            Native::ShowSlaves,
            Native::DetachSlaves,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(0));
        }
        // ChangeGroup(group) and ChangeUser(user) move the receiver between the synchronization
        // memberships and store the low eight bits of the argument in its group or user byte. The
        // membership move runs first, so a rejected change leaves the byte untouched. Group-list
        // management is always enabled here. Returns zero.
        if function == Native::ChangeGroup || function == Native::ChangeUser {
            let [value] = arguments else {
                anyhow::bail!("{function} requires one argument")
            };
            let value = value.integer()? as u8;
            let attribute = if function == Native::ChangeGroup {
                Attribute::Group
            } else {
                Attribute::User
            };
            let old = self.registry.actors[self.index].attributes[attribute];
            let sync = &mut self.registry.globals.group_sync;
            if function == Native::ChangeGroup {
                sync.change_group(self.index, old, value)?;
            } else {
                sync.change_user(self.index, old, value)?;
            }
            self.registry.actors[self.index].attributes[attribute] = value;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::TrueFunction {
            anyhow::ensure!(arguments.len() == 2, "TrueFunction: wrong argument count");
            return Ok(CallResult::Complete(1));
        }
        // SetAIType and SetAIMode store the low byte of their argument in the bytes that
        // MyAIType and MyAIMode read back.
        if function == Native::SetAiType || function == Native::SetAiMode {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()? as u8;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            if function == Native::SetAiType {
                control.ai_type = AiType::from_byte(value);
            } else {
                control.ai_mode = value;
            }
            return Ok(CallResult::Complete(0));
        }
        // `attr_health` is `attr_health`; Strength and Armor read the combat record.
        if [
            Native::Health,
            Native::PlayerHealth,
            Native::Strength,
            Native::Armor,
            Native::ReplenishHealth,
        ]
        .contains(&function)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let index = if function == Native::PlayerHealth {
                self.selected_player()
                    .context("PlayerHealth: selected player unavailable")?
                    .index
            } else {
                self.index
            };
            let selected = self
                .selected_player()
                .is_some_and(|player| player.index == index);
            let actor = &mut self.registry.actors[index];
            let value = match function {
                Native::Health | Native::PlayerHealth => {
                    i32::from(actor.attributes[Attribute::Health])
                }
                Native::ReplenishHealth => {
                    if actor.attributes[Attribute::Health] >= 100 {
                        0
                    } else {
                        if selected {
                            self.inputs
                                .hud
                                .context("ReplenishHealth selected-player HUD owner unavailable")?
                                .request_replenish_health_feedback();
                        }
                        actor.attributes[Attribute::Health] = 100;
                        1
                    }
                }
                _ => {
                    let combat = actor
                        .control
                        .as_ref()
                        .and_then(|control| control.combat.as_ref())
                        .with_context(|| {
                            format!("{function}: receiver combat record unavailable")
                        })?;
                    if function == Native::Strength {
                        combat.strength
                    } else {
                        combat.armor
                    }
                }
            };
            return Ok(CallResult::Complete(value));
        }
        if function == Native::IsCombatCapable || function == Native::PlayerArmed {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let armed = function == Native::PlayerArmed;
            let index = if armed {
                self.selected_player()
                    .context("PlayerArmed: selected player unavailable")?
                    .index
            } else {
                self.index
            };
            return Ok(CallResult::Complete(i32::from(
                self.combat_capable(index, armed)?,
            )));
        }
        if function == Native::DisableHook || function == Native::EnableHook {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver script-control unavailable"))?;
            // Bit 0x02 of the script-control byte suppresses the native hook.
            if function == Native::DisableHook {
                control.script_control |= 2;
            } else {
                control.script_control &= !2;
            }
            return Ok(CallResult::Complete(0));
        }
        if function == Native::SetPotion || function == Native::GetPotion {
            let set = function == Native::SetPotion;
            anyhow::ensure!(
                arguments.len() == 1 + usize::from(set),
                "{function}: wrong argument count"
            );
            let slot = usize::try_from(arguments[0].integer()?)
                .ok()
                .filter(|slot| *slot < self.registry.globals.potions.len())
                .with_context(|| format!("{function}: potion index outside 0..4 unsupported"))?;
            if set {
                self.registry.globals.potions[slot] = arguments[1].integer()?;
                return Ok(CallResult::Complete(1));
            }
            return Ok(CallResult::Complete(self.registry.globals.potions[slot]));
        }
        if function == Native::AdjustStrength || function == Native::AdjustArmor {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let delta = arguments[0].integer()?;
            let combat = self.registry.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            let value = if function == Native::AdjustStrength {
                &mut combat.strength
            } else {
                &mut combat.armor
            };
            *value = value.wrapping_add(delta);
            return Ok(CallResult::Complete(0));
        }
        if function == Native::SetStrength || function == Native::SetArmor {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()?;
            let combat = self.registry.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            if function == Native::SetStrength {
                combat.strength = value;
            } else {
                combat.armor = value;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::PlayAnimation {
            let [group, modifier] = arguments else {
                anyhow::bail!("PlayAnimation requires two arguments")
            };
            let group = group.integer()?;
            let group_bits = i16::try_from(group)
                .map(|value| AnimationGroup::from_index(value as u16))
                .context("PlayAnimation group outside supported signed range")?;
            // The modifier is stored as one byte.
            let modifier = modifier.integer()? as u8;
            let excluded = self.registry.actors[self.index]
                .control
                .as_ref()
                .is_some_and(|control| control.object_update_excluded);
            if !self.animation_system
                || excluded
                || self.registry.actors[self.index].animation.is_none()
            {
                return Ok(CallResult::Complete(0));
            }
            let ticks = self
                .inputs
                .nominal_ticks
                .context("PlayAnimation requires nominal_ticks")?;
            let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
            let surface = self.registry.actors[self.index].surface;
            let events = effects::ActorEventContext::capture(self.registry, id);
            let camera = &mut self.registry.camera;
            let effects = &mut self.registry.effects;
            let mut emit = |sound| {
                if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                    return events.dispatch(flags, camera.as_mut(), effects);
                }
                match &mut self.sounds {
                    Some(sounds) => sounds(
                        id,
                        super::ScriptSound::Animation {
                            sound,
                            surface,
                            position,
                        },
                    ),
                    None => {
                        anyhow::bail!("world actor {}: animation sound handler absent", id.index)
                    }
                }
            };
            let animation = self.registry.actors[self.index]
                .animation
                .as_mut()
                .expect("checked animation");
            let frame = animation
                .playback
                .request_with_sounds(group_bits, modifier, ticks, &mut emit)?;
            animation.frame = frame.or(animation.frame);
            return Ok(CallResult::Complete(0));
        }
        if function == Native::GetFramePassed {
            anyhow::ensure!(arguments.is_empty(), "GetFramePassed requires no arguments");
            let ticks = self
                .inputs
                .nominal_ticks
                .context("GetFramePassed requires nominal_ticks input")?;
            return Ok(CallResult::Complete(
                i32::try_from(ticks).context("GetFramePassed ticks exceed the signed range")?,
            ));
        }
        // DistanceFromStart(axis): saved start minus the local position on axis 0..2 in map units;
        // any other axis returns zero before the coordinates are read.
        if function == Native::DistanceFromStart {
            let [axis] = arguments else {
                anyhow::bail!("DistanceFromStart requires one argument")
            };
            let value = match usize::try_from(axis.integer()?) {
                Ok(axis) if axis < 3 => self.registry.spatial(id)?.start_displacement(axis)?,
                _ => 0,
            };
            return Ok(CallResult::Complete(value));
        }
        // IsShellScript returns the shell execution-context word, or zero while no shell script
        // exists. The host models the word as a Boolean; whether the shell list keeps killed
        // shells, and any word value other than zero or one, are not established.
        if function == Native::IsShellScript {
            anyhow::ensure!(arguments.is_empty(), "IsShellScript requires no arguments");
            let context = self
                .inputs
                .shell_execution
                .context("IsShellScript: shell context unavailable")?;
            let has_shell = self
                .registry
                .actors
                .iter()
                .any(|actor| matches!(actor.source, ScriptActorSource::Shell(_)));
            return Ok(CallResult::Complete(i32::from(context && has_shell)));
        }
        // IsWithinView(): the receiver's position (the effective one while it has a slave byte, its
        // local one otherwise) and the shared camera origin, each shifted to map units, go through
        // the shared rounded distance helper with a zero vertical difference; the result is one
        // when the distance is at most the installed view distance. The shared enable word is
        // treated as always set.
        if function == Native::IsWithinView {
            anyhow::ensure!(arguments.is_empty(), "IsWithinView requires no arguments");
            let threshold = self
                .registry
                .view_distance
                .context("IsWithinView: installed view distance unavailable")?;
            let id = self.registry.id(self.index)?;
            let spatial = self.registry.spatial(id)?;
            let position = if self.registry.actors[self.index].attributes[Attribute::Slave] != 0 {
                spatial
                    .effective
                    .context("IsWithinView: attached pose requires an explicit family refresh")?
                    .position
            } else {
                spatial.local.position
            };
            let camera = self
                .registry
                .camera
                .as_ref()
                .context("IsWithinView: camera state unavailable")?
                .current
                .position;
            let distance = cyrus_collision::distance::rounded_distance([
                (camera[0] >> 8).wrapping_sub(position[0] >> 8),
                0,
                (camera[2] >> 8).wrapping_sub(position[2] >> 8),
            ])?;
            return Ok(CallResult::Complete(i32::from(distance <= threshold)));
        }
        // InCircle(x, z, radius): the receiver's query position (attached, linked or local) minus
        // (x, z) with 32-bit wrapping and a zero vertical difference goes through the shared rounded
        // distance helper; the result is one when it is at most the signed radius.
        if function == Native::InCircle {
            let [x, z, radius] = arguments else {
                anyhow::bail!("InCircle requires three arguments")
            };
            let (x, z, radius) = (x.integer()?, z.integer()?, radius.integer()?);
            let position = self.fighting_position(self.index)?;
            let distance = cyrus_collision::distance::rounded_distance([
                position[0].wrapping_sub(x),
                0,
                position[2].wrapping_sub(z),
            ])?;
            return Ok(CallResult::Complete(i32::from(distance <= radius)));
        }
        if function == Native::InRectangle {
            let [x1, z1, x2, z2] = arguments else {
                anyhow::bail!("InRectangle requires four arguments")
            };
            let (x1, z1, x2, z2) = (x1.integer()?, z1.integer()?, x2.integer()?, z2.integer()?);
            let position = self.fighting_position(self.index)?;
            let inside = |value: i32, a: i32, b: i32| (a.min(b)..=a.max(b)).contains(&value);
            return Ok(CallResult::Complete(i32::from(
                inside(position[0], x1, x2) && inside(position[2], z1, z2),
            )));
        }
        // DrawSword is the weapon draw admission: it enters `DRAWING_SWORD` when every gate allows it
        // and otherwise leaves the actor unchanged. Its final repositioning step is disabled in
        // the shipped game.
        if function == Native::DrawSword {
            anyhow::ensure!(arguments.is_empty(), "DrawSword requires no arguments");
            self.draw_sword(id)?;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::SheathSword {
            anyhow::ensure!(arguments.is_empty(), "SheathSword requires no arguments");
            self.sheath_sword(function, id)?;
            return Ok(CallResult::Complete(0));
        }
        // endCombat runs SheathSword's action, then unconditionally resets the lifecycle selector
        // and mode bytes, re-enables the hook, leaves combat mode and clears the close-distance
        // pass counter and the coordination selection state.
        if function == Native::EndCombat {
            anyhow::ensure!(arguments.is_empty(), "endCombat requires no arguments");
            self.sheath_sword(function, id)?;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .context("endCombat: receiver control unavailable")?;
            anyhow::ensure!(
                super::combat::end_combat_reset(control),
                "endCombat: receiver combat record unavailable"
            );
            return Ok(CallResult::Complete(0));
        }
        if function == Native::IsFighting {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(self.is_fighting()?)));
        }
        if function == Native::IsDead {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(
                self.actor_is_dead(self.index)?,
            )));
        }
        if function == Native::Rnd || function == Native::QuickRnd {
            let [operand] = arguments else {
                anyhow::bail!("{function} requires one argument")
            };
            let operand = operand.integer()?;
            let random = self
                .random
                .as_deref_mut()
                .with_context(|| format!("{function}: shared engine random stream unavailable"))?;
            let value = if function == Native::Rnd {
                random.rnd(operand)?
            } else {
                random.quick_rnd(operand)
            };
            return Ok(CallResult::Complete(value));
        }
        // Enable*/Disable* clear and set one bit of the script-control byte: 0x01 script,
        // 0x04 tasks, 0x08 object, 0x10 combat (0x02 is the hook, handled above).
        let control_bit = match function {
            Native::EnableScript => Some((true, 0x01)),
            Native::DisableScript => Some((false, 0x01)),
            Native::EnableTasks => Some((true, 0x04)),
            Native::DisableTasks => Some((false, 0x04)),
            Native::EnableObject => Some((true, 0x08)),
            Native::DisableObject => Some((false, 0x08)),
            Native::EnableCombat => Some((true, 0x10)),
            Native::DisableCombat => Some((false, 0x10)),
            _ => None,
        };
        if let Some((enable, bit)) = control_bit {
            anyhow::ensure!(arguments.is_empty(), "{function}: wrong argument count");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: script-control lifecycle unavailable"))?;
            if enable {
                control.script_control &= !bit;
            } else {
                control.script_control |= bit;
            }
            return Ok(CallResult::Complete(0));
        }
        if function == Native::Sound {
            let [effect, volume, rate] = arguments else {
                anyhow::bail!("Sound requires three arguments")
            };
            let position = self
                .registry
                .query_position(id)
                .context("Sound receiver pose unavailable")?;
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id,
                super::ScriptSound::Positional {
                    effect: effect.integer()?,
                    volume: volume.integer()?,
                    rate_offset_hz: rate.integer()?,
                    position,
                    start_index: None,
                },
            )?;
            return Ok(CallResult::Complete(0));
        }
        if let Some(sound) = super::sound::command(function, arguments)? {
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(id, sound)?;
            return Ok(CallResult::Complete(0));
        }
        if self.registry.actors[self.index]
            .runtime_identifier
            .is_none()
            && [
                Native::Wander,
                Native::MoveByAxis,
                Native::MoveAlongAxis,
                Native::WanderToLocation,
                Native::SMoveByAxis,
                Native::MoveObjectAxis,
                Native::MoveToLocation,
                Native::RotateByAxis,
                Native::SRotateByAxis,
                Native::RotateToAxis,
                Native::Move,
                Native::Rotate,
                Native::SMove,
                Native::SRotate,
                Native::SMoveAlongAxis,
                Native::MoveMarker,
                Native::MoveLocation,
                Native::MovePosition,
                Native::TestTask,
                Native::MoveNodeMarker,
                Native::MoveNodeLocation,
                Native::MoveNodePosition,
            ]
            .contains(&function)
        {
            anyhow::bail!("{function}: separate movement/relocation owner unavailable");
        }
        if function == Native::Wander {
            let [radius, tolerance] = arguments else {
                anyhow::bail!("Wander requires two arguments")
            };
            let radius = radius.integer()?;
            let tolerance = tolerance.integer()?;
            let home = self.registry.actors[self.index]
                .placement()
                .context("Wander home unavailable for standalone actor")?
                .map_position;
            let [x, y, z] = home.map(i32::try_from);
            let home = [x?, y?, z?];
            if self
                .walk_map_count
                .context("Wander navigation not prepared")?
                != 0
            {
                let id = ScriptActorId {
                    generation: self.generation,
                    index: self.index,
                };
                let random = self
                    .random
                    .as_deref_mut()
                    .context("Wander shared engine random stream unavailable")?;
                let sounds = self.sounds.as_mut().map(|sink| {
                    &mut **sink as &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>
                });
                let result = self
                    .registry
                    .wander_through_maps(id, home, radius, tolerance, self.inputs, random, sounds)
                    .with_context(|| format!("Wander through walk-node maps (radius {radius})"))?;
                return Ok(CallResult::Complete(result));
            }
            let frame_scale = self
                .inputs
                .frame_scale
                .context("Wander requires frame_scale")?;
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("Wander spatial state not prepared")?
                .actor_mut(self.index)?;
            let random = self
                .random
                .as_deref_mut()
                .context("Wander shared engine random stream unavailable")?;
            let result =
                super::navigation::wander_without_maps(home, radius, frame_scale, spatial, random)?;
            self.registry.actors[self.index].attributes[Attribute::NodeInUse] = 1;
            return Ok(CallResult::Complete(result));
        }
        if function == Native::AtNodeMarker {
            let [marker, distance] = arguments else {
                anyhow::bail!("AtNodeMarker requires two arguments")
            };
            let position = self
                .registry
                .spatial
                .as_ref()
                .context("AtNodeMarker spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let result = self
                .registry
                .markers
                .as_ref()
                .context("AtNodeMarker markers not prepared")?
                .contains(marker.integer()?, position, distance.integer()?)?;
            return Ok(CallResult::Complete(i32::from(result)));
        }
        // Offset attaches the calling actor (the original's current script actor, which differs from
        // the receiver when the call is routed) to the receiver; UnOffset detaches the receiver.
        // PlayerOffset places the selected player relative to the receiver once.
        if kind == CallKind::Function && function == Native::Offset {
            let [x, y, z] = arguments else {
                anyhow::bail!("Offset requires three arguments")
            };
            let caller = self.registry.routed_caller.unwrap_or(self.index);
            self.registry.offset_actor(
                caller,
                self.index,
                [x.integer()?, y.integer()?, z.integer()?],
            )?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::UnOffset {
            anyhow::ensure!(arguments.is_empty(), "UnOffset requires no arguments");
            self.registry.unoffset_actor(self.index)?;
            return Ok(CallResult::Complete(0));
        }
        // TurnToAngle takes one ordinary turn step toward a heading and returns the signed
        // difference (zero when it arrived). An argument equal to the stored yaw returns first,
        // before any weapon query; the ordinary turn clamps the step to eight itself, so unlike
        // the FaceAngle task there is no zero-turn-rate completion.
        if kind == CallKind::Function && function == Native::TurnToAngle {
            let [angle] = arguments else {
                anyhow::bail!("TurnToAngle requires one argument")
            };
            let angle = angle.integer()?;
            let id = self.registry.id(self.index)?;
            if self.registry.spatial(id)?.local.angles[1] == angle {
                return Ok(CallResult::Complete(0));
            }
            let inputs = self.registry.point_facing_inputs(id, self.inputs)?;
            let sounds = self
                .sounds
                .as_mut()
                .context("TurnToAngle requires synchronous sounds")?;
            // Truncation keeps the heading modulo 2048, which is all the turn reads.
            let difference =
                self.registry
                    .turn_ordinary_angle(id, angle as u16, inputs, &mut **sounds)?;
            return Ok(CallResult::Complete(difference));
        }
        // TurnToPos(x, z) snaps onto a point within the facing threshold (returning zero) or turns
        // one ordinary step toward its bearing and returns the turn's value; the same owner as
        // the FacePos task, without that task's zero-turn-rate completion.
        if kind == CallKind::Function && function == Native::TurnToPos {
            let [x, z] = arguments else {
                anyhow::bail!("TurnToPos requires two arguments")
            };
            let point = [x.integer()?, 0, z.integer()?];
            let id = self.registry.id(self.index)?;
            let inputs = self.registry.point_facing_inputs(id, self.inputs)?;
            let sounds = self
                .sounds
                .as_mut()
                .context("TurnToPos requires synchronous sounds")?;
            let value = self
                .registry
                .turn_ordinary_point(id, point, inputs, &mut **sounds)?;
            return Ok(CallResult::Complete(value));
        }
        if kind == CallKind::Function && function == Native::PlayerForceTalk {
            let [distance] = arguments else {
                anyhow::bail!("PlayerForceTalk requires one argument")
            };
            self.registry
                .force_talk_selected_player(self.index, distance.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function == Native::PlayerOffset {
            let [x, y, z] = arguments else {
                anyhow::bail!("PlayerOffset requires three arguments")
            };
            self.registry.offset_selected_player(
                self.index,
                [x.integer()?, y.integer()?, z.integer()?],
                false,
            )?;
            return Ok(CallResult::Complete(0));
        }
        // AtNodeLocation compares a receiver location with the receiver's own position; the height
        // limit is half the actor's total Y extent (positive plus negative extent).
        // AtNodePosition(x, y, z, distance): the wrapping differences between the point and the
        // receiver's local position (the position AtNodeLocation reads) go through the shared
        // rounded distance helper unshifted, with no vertical adjustment; the signed result is at
        // most the full signed distance. An overflowing length converts to the integer-indefinite
        // value and is compared like any other signed result.
        if function == Native::AtNodePosition {
            let [x, y, z, distance] = arguments else {
                anyhow::bail!("AtNodePosition requires four arguments")
            };
            let point = [x.integer()?, y.integer()?, z.integer()?];
            let position = self
                .registry
                .spatial
                .as_ref()
                .context("AtNodePosition: spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let result = cyrus_collision::distance::rounded_distance_or_indefinite(
                std::array::from_fn(|axis| point[axis].wrapping_sub(position[axis])),
            );
            return Ok(CallResult::Complete(i32::from(
                result <= distance.integer()?,
            )));
        }
        // ReloadPosition(): the local position is copied unchanged into the previous-position
        // history and zero is returned. The original also refreshes the model's single-precision
        // coordinates from the shifted position; cyrus positions models from the local position at
        // publication, so no separate store exists (**Inferred**).
        if function == Native::ReloadPosition {
            anyhow::ensure!(arguments.is_empty(), "ReloadPosition requires no arguments");
            let spatial = self
                .registry
                .spatial
                .as_mut()
                .context("ReloadPosition: spatial state not prepared")?
                .actor_mut(self.index)?;
            spatial.previous_local_position = spatial.local.position;
            return Ok(CallResult::Complete(0));
        }
        if function == Native::AtNodeLocation {
            let [location, distance] = arguments else {
                anyhow::bail!("AtNodeLocation requires two arguments")
            };
            let id = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let shape = self.registry.ordinary_collision_shape(id)?;
            let total_height = shape.positive_extent[1].wrapping_add(shape.negative_extent[1]);
            let position = self
                .registry
                .spatial
                .as_ref()
                .context("AtNodeLocation spatial state not prepared")?
                .actor(self.index)?
                .local
                .position;
            let target = self.receiver_location("node location", location.integer()?)?;
            return Ok(CallResult::Complete(i32::from(
                point_tasks::within_location(position, target, total_height, distance.integer()?)?,
            )));
        }
        // PlayerCollide holds while the selected player stands on the receiver or its body touched
        // the receiver; PlayerCollideStatic only reads the contact list.
        if function == Native::PlayerCollide || function == Native::PlayerCollideStatic {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .with_context(|| format!("{function} selected player unavailable"))?;
            let receiver = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let contacts = &self.registry.globals.player_contacts;
            let touching = if function == Native::PlayerCollide {
                self.registry.actors[self.registry.resolve(player)?]
                    .model
                    .support
                    .actor
                    == Some(receiver)
                    || contacts.touches_actor(self.index)
            } else {
                contacts.touches_static(self.index)
            };
            return Ok(CallResult::Complete(i32::from(touching)));
        }
        // CollidePlayerWeapon(x, y, z, radius): while the selected player swings a drawn weapon,
        // whether the weapon tip is within the radius of the point offset from the receiver.
        if function == Native::CollidePlayerWeapon {
            anyhow::ensure!(arguments.len() == 4, "{function} requires four arguments");
            let player = self
                .selected_player()
                .context("CollidePlayerWeapon selected player unavailable")?;
            let player_index = self.registry.resolve(player)?;
            let (holding, _, _) =
                self.actor_weapon_predicates(player_index, Native::IsHoldingWeapon)?;
            let actor = &self.registry.actors[player_index];
            let swinging = actor
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| {
                    combat.decision.attack_state == super::ExecutorState::DEFENDING
                });
            if !holding || actor.attributes[Attribute::PlayerType] == 1 || !swinging {
                return Ok(CallResult::Complete(0));
            }
            let tip = actor
                .model
                .weapon
                .as_ref()
                .context(
                    "CollidePlayerWeapon: the selected player's weapon endpoints are unavailable",
                )?
                .endpoints[1];
            let receiver = self.registry.id(self.index)?;
            let origin = self.registry.query_position(receiver)?;
            let offsets = [
                arguments[0].integer()?,
                arguments[1].integer()?,
                arguments[2].integer()?,
            ];
            let center = std::array::from_fn(|axis| origin[axis].wrapping_add(offsets[axis]) >> 8);
            return Ok(CallResult::Complete(i32::from(
                cyrus_collision::distance::point_in_sphere(
                    tip,
                    center,
                    arguments[3].integer()? >> 8,
                ),
            )));
        }
        if function == Native::PlayerStand {
            anyhow::ensure!(arguments.is_empty(), "PlayerStand requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerStand selected player unavailable")?;
            anyhow::ensure!(
                player.generation == self.generation && player.index < self.registry.actors.len(),
                "PlayerStand selected player identity is stale"
            );
            let receiver = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            return Ok(CallResult::Complete(i32::from(
                self.registry.actors[player.index].model.support.actor == Some(receiver),
            )));
        }
        let surface_test: Option<fn(Surface) -> bool> = match function {
            Native::IsInDeepWater => Some(|surface| surface.is_deep_water()),
            Native::IsInWater => Some(|surface| surface.is_water()),
            Native::IsInLava => Some(|surface| surface == Surface::LAVA),
            _ => None,
        };
        if let Some(test) = surface_test {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let surface = self.registry.actors[self.index]
                .surface
                .with_context(|| format!("{function} actor surface state unavailable"))?;
            return Ok(CallResult::Complete(i32::from(test(surface))));
        }
        // Receiver lifecycle bytes: MyAIType is the lifecycle selector, MyAIMode the death
        // notification byte; IsOnEdge holds for the ledge living states.
        let control_test: Option<fn(&ActorControlState) -> i32> = match function {
            Native::MyAiType => Some(|control| i32::from(control.ai_type.byte())),
            Native::MyAiMode => Some(|control| i32::from(control.ai_mode)),
            Native::IsOnEdge => Some(|control| i32::from(control.living_state.is_ledge())),
            // The original also answers 0 while its transition lock (global word read by
            // dozens of routines) is set; cyrus has no transition state, so it is clear.
            Native::IsInAir => Some(|control| {
                i32::from(
                    control.sliding_state != 0
                        || control.falling_state != JumpMotion::NONE
                        || control.queued_motion != JumpMotion::NONE
                        || control.living_state.is_actor_bounce(),
                )
            }),
            _ => None,
        };
        if let Some(test) = control_test {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_ref()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            return Ok(CallResult::Complete(test(control)));
        }
        // The Spare0/Spare1 natives ignore every argument, change no actor state, add a diagnostic
        // line and return zero. The original's line carries the receiver's host pointer, which
        // has no portable value, so the receiver's name stands in for it (as PrintSingleParm's
        // line does); the argument count is the call record's, whatever the definition declares.
        if matches!(
            function,
            Native::Spare0Parm0
                | Native::Spare0Parm1
                | Native::Spare0Parm2
                | Native::Spare0Parm3
                | Native::Spare0Parm4
                | Native::Spare0Parm5
                | Native::Spare0Parm6
                | Native::Spare0Parm7
                | Native::Spare1Parm0
                | Native::Spare1Parm1
                | Native::Spare1Parm2
                | Native::Spare1Parm3
                | Native::Spare1Parm4
                | Native::Spare1Parm5
                | Native::Spare1Parm6
                | Native::Spare1Parm7
        ) {
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry
                .globals
                .print_line(format!("{name} Spare function"));
            return Ok(CallResult::Complete(0));
        }
        if function == Native::PrintSingleParm {
            let [value] = arguments else {
                anyhow::bail!("{function} requires one argument");
            };
            let value = value.integer()?;
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry.globals.print_line(format!("{name} {value}"));
            return Ok(CallResult::Complete(0));
        }
        // HeaderHook writes three console lines about the receiver and returns one: a banner, its
        // identifier, object name (the namespace named objects resolve through) and script, and its
        // node and AI record. It changes nothing and
        // does not run the receiver's RAHK hook. `%p` of the original has no portable value, so
        // the pool slot stands in for the record address.
        if function == Native::HeaderHook {
            anyhow::ensure!(arguments.is_empty(), "HeaderHook requires no arguments");
            let actor = &self.registry.actors[self.index];
            let control = actor
                .control
                .as_ref()
                .context("HeaderHook: receiver AI record unavailable")?;
            let address = format!("0x{:08x}", self.index);
            let identifier = actor.runtime_identifier.map_or(0, i32::from);
            let script = actor.name();
            let described = match (actor.namespace.as_deref(), actor.unavailable.is_none()) {
                (None, false) => {
                    format!("Unnamed object (id {identifier}) at {address} no script attached")
                }
                (None, true) => {
                    format!("Unnamed object (id {identifier}) at {address} using \"{script}\"")
                }
                (Some(name), false) => {
                    format!("Object \"{name}\"(id {identifier}) at {address} no script attached")
                }
                (Some(name), true) => {
                    format!("Object \"{name}\"(id {identifier}) at {address} using \"{script}\"")
                }
            };
            let ai = control.ai;
            let record = format!(
                "Node {}, dnode {}, ai_type {}, ai_mode {}, ai_delay {}, ai_length {}, ai_var1 {}, ai_var2 {}",
                control.navigation.current,
                control.navigation.next,
                control.ai_type.byte(),
                control.ai_mode,
                control.ai_delay,
                ai.length,
                ai.var1,
                ai.var2
            );
            let globals = &mut self.registry.globals;
            globals.print_line("\nHeader Hook:".to_owned());
            globals.print_line(described);
            globals.print_line(record);
            return Ok(CallResult::Complete(1));
        }
        // PrintParms and LogParms format the receiver's script name and all eight arguments as signed
        // decimals; PrintParms takes the first free console line, LogParms goes to the diagnostic log.
        if function == Native::PrintParms || function == Native::LogParms {
            anyhow::ensure!(
                arguments.len() == 8,
                "{function} requires eight integer arguments"
            );
            let values = arguments
                .iter()
                .map(|argument| argument.integer().map(|value| value.to_string()))
                .collect::<Result<Vec<_>>>()
                .with_context(|| format!("{function} arguments"))?
                .join(", ");
            let name = self.registry.actors[self.index].name().to_owned();
            if function == Native::PrintParms {
                self.registry.globals.print_line(format!("{name} {values}"));
            } else {
                bevy::log::debug!("  {name}|3 {values}");
            }
            return Ok(CallResult::Complete(0));
        }
        // PrintStringParm is PrintSingleParm's line with the already evaluated string as its text.
        if function == Native::PrintStringParm {
            let [Argument::String(text)] = arguments else {
                anyhow::bail!("{function} requires one string argument");
            };
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry.globals.print_line(format!("{name} {text}"));
            return Ok(CallResult::Complete(0));
        }
        if function == Native::PlayerInSight {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerInSight requires a selected player")?;
            let id = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let visible = if let Some(geometry) = self.inputs.locomotion_geometry {
                self.registry.player_in_sight(
                    id,
                    player,
                    obstacle::OrdinaryObstacleQuery {
                        scene: geometry.ground.scene,
                        animations: geometry.ground.animations,
                        prepared: geometry.ground.prepared,
                        vertical_offset: 0,
                        respond: false,
                    },
                )?
            } else {
                // A receiver without collision gathers no candidates, so the test reads the list
                // the latest gather left.
                let source = self
                    .inputs
                    .player_collision
                    .context("PlayerInSight requires the world's collision geometry")?;
                self.registry.player_in_sight_retained(
                    id,
                    player,
                    source.scene,
                    source.animations,
                )?
            };
            return Ok(CallResult::Complete(i32::from(visible)));
        }
        // AtPos snaps the receiver onto the point when it is strictly within one and a half
        // movement-speed steps, registering it in the changed-actor list after the position stores.
        if function == Native::AtPos {
            let [x, y, z] = arguments else {
                anyhow::bail!("AtPos requires three arguments")
            };
            let point = [x.integer()?, y.integer()?, z.integer()?];
            return Ok(CallResult::Complete(i32::from(
                self.registry.arrive_ordinary_point(id, point)?,
            )));
        }
        if function == Native::DistanceFromLocation {
            let [location] = arguments else {
                anyhow::bail!("DistanceFromLocation requires one argument")
            };
            return Ok(CallResult::Complete(
                self.registry
                    .spatial(id)?
                    .location_distance_from_local(location.integer()?)?,
            ));
        }
        if function == Native::DistanceToLocation {
            let [location] = arguments else {
                anyhow::bail!("DistanceToLocation requires one argument")
            };
            let mut inputs = self
                .inputs
                .player_query
                .context("player query inputs unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(self.index)?;
            inputs.linked_position = Some(spatial.linked_position);
            return Ok(CallResult::Complete(
                spatial.location_distance(location.integer()?, inputs)?,
            ));
        }
        if function == Native::PlayerDistance
            || function == Native::AcuratePlayerDistance
            || function == Native::PlayerFacing
            || function == Native::PlayerLooking
            || function == Native::PlayerArc
            || function == Native::PlayerArcTan
        {
            let mut inputs = self
                .inputs
                .player_query
                .context("player query inputs unavailable")?;
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("spatial state not prepared")?
                .actor(self.index)?;
            inputs.linked_position = Some(spatial.linked_position);
            return Ok(CallResult::Complete(
                spatial
                    .query(function, arguments, inputs)?
                    .context("spatial query absent")?,
            ));
        }
        if let Some(value) = self.registry.flag_state.call(function, arguments, flags)? {
            return Ok(CallResult::Complete(value));
        }
        if function == Native::SetAttribute
            && let [index, value] = arguments
            && index.integer()? == 44
        {
            let id = self.registry.id(self.index)?;
            if self.registry.query_is_dead(id, self.selected_player())? {
                self.registry.release_held_walk_nodes(id)?;
                let actor = &mut self.registry.actors[self.index];
                let default = actor
                    .control
                    .as_mut()
                    .and_then(|control| control.combat_death_default.as_mut())
                    .context("SetAttribute attr_panic_group: dead actor without a combat record unsupported")?;
                *default = value.integer()?;
            }
        }
        let actor = &mut self.registry.actors[self.index];
        if let Some(value) = super::attributes::call(
            function,
            arguments,
            &mut actor.attributes,
            actor.control.as_mut(),
        )? {
            return Ok(CallResult::Complete(value));
        }
        anyhow::bail!(
            "native {:?} {function} unsupported by inspection host",
            kind
        )
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: cyrus_soup::Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        let alignment_flag = if self.registry.actors[self.index]
            .tasks
            .is_wait_lineup_task(token)
        {
            Some(self.argument_alignment_flag(arguments)?)
        } else {
            None
        };
        if self.registry.actors[self.index].tasks.is_lineup_task(token) {
            return self.lineup_poll(token, arguments, invocation);
        }
        if self.registry.actors[self.index].tasks.is_bounce_task(token) {
            return self.bounce_poll(token, arguments, invocation);
        }
        if self.registry.actors[self.index].tasks.is_point_task(token) {
            return self.point_poll(token, arguments, invocation);
        }
        let ambient = self.ambient_call();
        let player_dialogue = self.player_dialogue_call();
        let frame_wait = self.frame_wait_call();
        let operation = self.registry.actors[self.index]
            .tasks
            .locomotion_operation(token);
        let locomotion_admission = self.locomotion_probes(operation)?;
        let locomotion_state = self.locomotion_state();
        let player_face_requested = self.registry.actors[self.index]
            .tasks
            .is_player_face_task(token);
        let player_face = player_face_requested.then(|| self.player_face_state());
        let mut player_face_outcome = super::tasks::PlayerFaceOutcome::default();
        let camera_update_handle = self.inputs.camera_update;
        let camera_inputs = self.registry.actors[self.index]
            .tasks
            .is_camera_task(token)
            .then(|| {
                (
                    self.camera_slot_request(),
                    self.camera_bound_pose(),
                    self.camera_targets(),
                )
            });
        let position = animation_sound_position(self.registry.spatial.as_ref(), self.index);
        let surface = self.registry.actors[self.index].surface;
        // RTXpAnim animates the selected player, who emits animation sounds under their own identity.
        let player_slot = self.selected_player().filter(|player| {
            player.generation == self.generation
                && player.index != self.index
                && player.index < self.registry.actors.len()
        });
        let player_sound = player_slot.map(|player| {
            (
                player,
                animation_sound_position(self.registry.spatial.as_ref(), player.index),
                self.registry.actors[player.index].surface,
            )
        });
        let animation_waits = self.registry.animation_waits();
        let actor_events = effects::ActorEventContext::capture(self.registry, id);
        let player_events =
            player_slot.map(|player| effects::ActorEventContext::capture(self.registry, player));
        // Camera tasks own the camera instead of animation playback; animation tasks
        // share these local dispatch borrows between receiver and selected-player cursors.
        let (event_camera, task_camera) = if camera_inputs.is_some() {
            (None, self.registry.camera.as_mut())
        } else {
            (self.registry.camera.as_mut(), None)
        };
        let event_services = std::cell::RefCell::new((event_camera, &mut self.registry.effects));
        let sound_handler = std::cell::RefCell::new(&mut self.sounds);
        let mut emit = |sound| {
            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                let (camera, effects) = &mut *event_services.borrow_mut();
                return actor_events.dispatch(flags, camera.as_deref_mut(), effects);
            }
            match &mut **sound_handler.borrow_mut() {
                Some(sounds) => sounds(
                    id,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                ),
                None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
            }
        };
        let mut emit_player = |sound| {
            if let cyrus_world::animation::AnimationSound::Event { flags } = sound {
                let context = player_events
                    .as_ref()
                    .context("selected player actor event unavailable")?;
                let (camera, effects) = &mut *event_services.borrow_mut();
                return context.dispatch(flags, camera.as_deref_mut(), effects);
            }
            match (&mut **sound_handler.borrow_mut(), player_sound) {
                (Some(sounds), Some((player, position, surface))) => sounds(
                    player,
                    super::ScriptSound::Animation {
                        sound,
                        surface,
                        position,
                    },
                ),
                _ => anyhow::bail!("selected player animation sound handler absent"),
            }
        };
        let user_channel = self.registry.actors[self.index].attributes[Attribute::User];
        let user_has_active = self
            .registry
            .globals
            .group_sync
            .user_has_active(&self.registry.actors, user_channel);
        let (actor, player_actor) = match player_slot.and_then(|player| {
            self.registry
                .actors
                .get_disjoint_mut([self.index, player.index])
                .ok()
        }) {
            Some([actor, player]) => (actor, Some(player)),
            None => (&mut self.registry.actors[self.index], None),
        };
        let light_position = actor
            .placement()
            .map(|placement| placement.map_position.map(|value| value as i32))
            .unwrap_or_default();
        let mut animation =
            actor
                .animation
                .as_mut()
                .zip(self.inputs.nominal_ticks)
                .map(|(animation, ticks)| super::tasks::AnimationCall {
                    animation: &mut animation.playback,
                    ticks,
                    frame: &mut animation.frame,
                    sounds: Some(&mut emit),
                });
        let player_animation = player_actor
            .and_then(|player| player.animation.as_mut())
            .zip(self.inputs.nominal_ticks)
            .map(|(animation, ticks)| super::tasks::AnimationCall {
                animation: &mut animation.playback,
                ticks,
                frame: &mut animation.frame,
                sounds: Some(&mut emit_player),
            });
        let player_position = self
            .inputs
            .player_query
            .as_ref()
            .and_then(|query| query.player.as_ref())
            .map(|player| player.position);
        let mut spatial_call = self
            .registry
            .spatial
            .as_mut()
            .and_then(|spatial| spatial.actor_mut(self.index).ok())
            .zip(self.inputs.frame_scale.zip(self.inputs.nominal_ticks))
            .map(
                |(spatial, (frame_scale, nominal_ticks))| super::tasks::SpatialCall {
                    matrix: spatial.local.matrix,
                    locations: &spatial.locations,
                    markers: self.registry.markers.as_ref(),
                    effective_position: spatial.effective.map(|pose| pose.position),
                    player_position,
                    slave: actor.attributes[Attribute::Slave] != 0,
                    actor_index: self.index,
                    map_updates: &mut self.registry.map_updates,
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    turn: self.registry.globals.turn.delta(frame_scale),
                    nominal_ticks,
                    tick: self.inputs.combat_clock_sample,
                },
            );
        let menu_input = menu_input(&self.inputs, self.registry.globals.dialogue.as_ref());
        let mut gxa_words = {
            let command = &self.registry.globals.native_command;
            (command.continuation, command.continuation_direction as i32)
        };
        let gxa_before = gxa_words;
        let mut light_effect = |function, arguments: &[Argument]| {
            light::LightState::effect(
                &mut actor.light,
                light_position,
                function,
                arguments,
                &mut self.registry.globals.native_command,
                self.inputs.nominal_ticks,
                self.random.as_deref_mut(),
            )
        };
        let result = actor.tasks.poll(
            token,
            arguments,
            invocation,
            &mut animation,
            &mut spatial_call,
            super::tasks::NativeServices {
                light_effect: Some(&mut light_effect),
                animation_requests_enabled: self.animation_system
                    && !actor
                        .control
                        .as_ref()
                        .is_some_and(|control| control.object_update_excluded),
                ambient: Some(&ambient),
                dialogue: self.registry.globals.dialogue.as_mut(),
                menu: Some(super::tasks::MenuCall {
                    state: &mut self.registry.globals.menu,
                    active: &mut self.registry.globals.menu_state,
                    input: menu_input,
                }),
                player_dialogue: Some(player_dialogue),
                player_animation,
                group_sync: Some(super::tasks::GroupSyncCall {
                    state: &mut self.registry.globals.group_sync,
                    actor_index: self.index,
                    channel: actor.attributes[Attribute::Group],
                    user_channel,
                    user_has_active,
                }),
                frame_wait: Some(frame_wait),
                locomotion: Some(super::tasks::LocomotionCall {
                    state: locomotion_state,
                    admission: locomotion_admission,
                    animation_system: animation_waits,
                    motion: actor
                        .control
                        .as_mut()
                        .map(|control| super::tasks::MotionCall {
                            classification: &mut control.motion_classification,
                            direction: &mut control.motion_direction,
                            counters: &mut control.move_counter,
                        }),
                    called: &mut self.registry.globals.locomotion_called,
                }),
                camera: camera_inputs.and_then(
                    |(requested_slot, bound_pose, (receiver, selected_player))| {
                        task_camera.map(|state| super::tasks::CameraCall {
                            state,
                            requested_slot,
                            bound_pose,
                            receiver,
                            selected_player,
                            update: camera_update_handle,
                        })
                    },
                ),
                player_face: player_face.map(|state| super::tasks::PlayerFaceCall {
                    state,
                    outcome: &mut player_face_outcome,
                }),
                lineup: None,
                bounce: None,
                point: None,
                gxa_overlay: Some(&mut self.registry.gxa_overlay),
                alignment_flag,
                gxa_animation: self
                    .inputs
                    .nominal_ticks
                    .map(|ticks| super::tasks::GxaAnimation {
                        resource: &mut gxa_words.0,
                        progress: &mut gxa_words.1,
                        ticks,
                    }),
            },
        );
        if gxa_words != gxa_before {
            let command = &mut self.registry.globals.native_command;
            command.continuation = gxa_words.0;
            command.continuation_direction = gxa_words.1 as u32;
        }
        self.finish_camera_update(result.is_ok())?;
        if result.is_ok() && player_face_requested {
            self.apply_player_face(player_face_outcome)?;
        }
        result
    }
    fn cancel(&mut self, token: u64) -> Result<()> {
        self.registry.actors[self.index].tasks.cancel(token)
    }
    fn route(
        &mut self,
        target: &Target,
        call: &Call,
        strings: &[String],
        site: cyrus_soup::NativeCallSite,
        flags: &mut [i32],
    ) -> Result<Route> {
        let index = self.target_index(target)?;
        if let Some(error) = self.registry.actors[index].failure() {
            anyhow::bail!("{error}");
        }
        anyhow::ensure!(
            self.registry.actors[index].animation.is_none() || self.inputs.nominal_ticks.is_some(),
            "receiver instance {index}: animation requires nominal_ticks"
        );
        if index == self.index {
            return Ok(Route::Here);
        }
        let actor = &mut self.registry.actors[index];
        let mut runtime = actor.runtime.take().with_context(|| {
            format!(
                "receiver instance {index} {}: {}",
                actor.name(),
                actor
                    .unavailable
                    .as_deref()
                    .unwrap_or("script runtime already active")
            )
        })?;
        let caller = std::mem::replace(&mut self.index, index);
        let outer_caller = self.registry.routed_caller.replace(caller);
        let result = runtime.receive(call, strings, site, self, flags);
        self.registry.routed_caller = outer_caller;
        self.index = caller;
        self.registry.actors[index].runtime = Some(runtime);
        result.map(Route::Complete)
    }
    fn task_call(&mut self, origin: CodeOrigin, instruction: u32) -> Result<Call> {
        self.registry
            .task_calls
            .get(&(origin, instruction))
            .cloned()
            .with_context(|| format!("task source {} instruction {instruction} absent", origin.0))
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        self.registry.activation.release_owner(ScriptActorId {
            generation: self.registry.generation,
            index: self.index,
        });
        let attributes = &self.registry.actors[self.index].attributes;
        self.registry
            .globals
            .group_sync
            .reset_user(self.index, attributes[Attribute::User]);
        // End clears the actor's pending group synchronization state.
        if attributes[Attribute::Group] != 0 {
            self.registry.globals.group_sync.reset_current(
                self.index,
                self.registry.actors[self.index].attributes[Attribute::Group],
            );
        }
        Ok(())
    }
}

pub(super) fn clear_unprepared(
    prepared: Option<Res<PreparedWorld>>,
    mut actors: ResMut<ScriptActors>,
    mut status: ResMut<super::ScriptStatus>,
) {
    if prepared.is_some() {
        return;
    }
    actors.error = None;
    actors.release_world();
    actors.requested = None;
    *status = super::ScriptStatus::NotRequested;
}

fn select_combat_taunt<'a>(
    actor: &'a ScriptActor,
    trigger: TauntType,
    random: &mut cyrus_game::random::EngineRandom,
) -> Option<&'a CombatTaunt> {
    let mut requested = trigger;
    loop {
        let mut selected = None;
        for taunt in &actor.combat_taunts {
            if taunt.trigger != requested || taunt.label == [0; 4] {
                continue;
            }
            selected = Some(taunt);
            if 4 * random.draw() / 32767 == 1 {
                break;
            }
        }
        if selected.is_some() || requested != TauntType::DEATH {
            return selected;
        }
        requested = TauntType::HURT_BY_OPPONENT;
    }
}

#[cfg(test)]
mod fighting_tests {
    use super::*;

    fn query(
        positions: &[[i32; 3]],
        order: &[usize],
        threshold: i32,
        state: i32,
        dead: bool,
    ) -> bool {
        let receiver = ScriptActorId {
            generation: 1,
            index: 0,
        };
        let actors: Vec<_> = (0..=positions.len())
            .map(|index| ScriptActor {
                light: None,
                flat: None,
                message_read: 0,
                broadcast_index: 0,
                source: ScriptActorSource::Map(Instance {
                    object_id: index as u32,
                    actor: format!("ACTOR{index}"),
                    object_type: Default::default(),
                    active: 0,
                    model_name: String::new(),
                    model_mode: 0,
                    light_intensity: 0,
                    light_radius: 0,
                    light_rgb: [0; 3],
                    texture: 0,
                    map_position: [0; 3],
                    angles: [0; 3],
                }),
                runtime_identifier: None,
                namespace: None,
                native_hook: None,
                combat_taunts: vec![],
                runtime: None,
                created_retired: false,
                unavailable: None,
                attributes: Attributes::default(),
                control: Some(ActorControlState {
                    combat: (index != 0).then_some(super::super::CombatControlState {
                        target: Some(receiver),
                        attacker_selection_state: state,
                        ..Default::default()
                    }),
                    ai_type: if dead { AiType::DEAD } else { AiType::NONE },
                    ..Default::default()
                }),
                surface: Some(Surface::UNKNOWN),
                model: Default::default(),
                collision: None,
                player_movement: None,
                location_offsets: vec![],
                tasks: Default::default(),
                native_commands: Default::default(),
                unprepared_animation_default: 0,
                animation: None,
                animation_failure: None,
                update_failure: None,
            })
            .collect();
        let mut spatial = super::super::spatial::SpatialWorld::prepare(&actors).unwrap();
        for (index, &position) in positions.iter().enumerate() {
            spatial.actor_mut(index + 1).unwrap().local.position = position;
        }
        let traversal: Vec<_> = order
            .iter()
            .map(|&index| ScriptActorId {
                generation: 1,
                index,
            })
            .collect();
        let definition = cyrus_soup::program::Definition {
            functions: vec![],
            flags: vec![],
            references: vec![],
            attributes: vec![],
        };
        let mut registry = ScriptWorld {
            world: 1,
            content_id: String::new(),
            generation: 1,
            flags: Vec::new(),
            flag_state: super::super::flags::Flags::prepare(&definition).unwrap(),
            updates: cyrus_assets::script::UpdateSettings {
                rope: None,
                ropes_load: None,
                fall_death_ticks: 144,
                fall_landing: None,
                dummy_model_manager: false,
                task_system: true,
                animation_system: true,
                disable_slaves: false,
                disable_master_slaves: false,
                final_version: false,
            },
            player_distance_override: false,
            actors,
            shell_templates: BTreeMap::new(),
            spatial: Some(spatial),
            activation: Default::default(),
            globals: Default::default(),
            task_calls: BTreeMap::new(),
            routed_caller: None,
            weapons: BTreeMap::new(),
            combat_attacks: Vec::new(),
            combat_defenses: Vec::new(),
            in_combat_threshold: Some(threshold),
            lineup_distance: None,
            view_distance: None,
            consumed_letters: 0,
            texture_frames: Default::default(),
            non_engaged_spacing: None,
            combat_clank_effects: [None; 2],
            node_timer: None,
            references: definition.references,
            names: BTreeMap::new(),
            actor_lists: None,
            markers: None,
            walk_nodes: None,
            next_combat_index: 0,
            ordinary_prepared: false,
            animation_strict: None,
            ordinary_updates: 0,
            actor_prelighting: None,
            retained_candidates: Default::default(),
            display_attachments: Vec::new(),
            model_admission: None,
            map_updates: Default::default(),
            surface_veto: None,
            pending_surface_transitions: Vec::new(),
            rolled_back: false,
            ground_bypass_toggle: false,
            insert_camera_flag: false,
            camera: None,
            camera_glide: Default::default(),
            camera_limits: Default::default(),
            camera_settings: Default::default(),
            standing_height: None,
            gxa_overlay: None,
            pending_player_placement: None,
            effects: None,
            ropes: Vec::new(),
            player_rope: Default::default(),
            follow_heading: None,
        };
        let host = GameHost {
            native_source_program: None,
            registry: &mut registry,
            world: 1,
            animation_system: true,
            task_system: true,
            walk_map_count: None,
            inputs: Default::default(),
            index: 0,
            generation: 1,
            sounds: None,
            random: None,
            ordinary_actors: Some(&traversal),
        };
        host.is_fighting().unwrap()
    }

    #[test]
    fn fighting_selection_preserves_order_and_uses_three_dimensions_before_horizontal_test() {
        let positions = [[0, 10, 0], [10, 0, 0]];
        assert!(query(&positions, &[1, 2], 5, 1, false));
        assert!(!query(&positions, &[2, 1], 5, 1, false));
        assert!(!query(&[[0, 11, 0], [10, 0, 0]], &[1, 2], 5, 1, false));
        assert!(query(&[[0, 9, 0], [10, 0, 0]], &[2, 1], 5, 1, false));
    }

    #[test]
    fn fighting_selection_checks_strict_bounds_exact_state_and_death() {
        assert!(!query(&[[10, 0, 0]], &[1], 10, 1, false));
        assert!(query(&[[10, 0, 0]], &[1], 11, 1, false));
        assert!(!query(&[[65535, 0, 0]], &[1], 100000, 1, false));
        assert!(query(&[[65534, 0, 0]], &[1], 100000, 1, false));
        for state in [0, 2, -1] {
            assert!(!query(&[[1, 0, 0]], &[1], 10, state, false));
        }
        assert!(!query(&[[1, 0, 0]], &[1], 10, 1, true));
        assert!(!query(&[[1, 0, 0]], &[], 10, 1, false));
    }
}

#[cfg(test)]
mod selector_tests {
    use super::*;

    fn actor(source: ScriptActorSource) -> ScriptActor {
        ScriptActor {
            light: None,
            flat: None,
            message_read: 0,
            broadcast_index: 0,
            source,
            runtime_identifier: None,
            namespace: None,
            native_hook: None,
            combat_taunts: vec![],
            runtime: None,
            created_retired: false,
            unavailable: None,
            attributes: Attributes::default(),
            control: Some(ActorControlState::default()),
            surface: Some(Surface::UNKNOWN),
            model: Default::default(),
            collision: None,
            player_movement: None,
            location_offsets: vec![],
            tasks: Default::default(),
            native_commands: Default::default(),
            unprepared_animation_default: 0,
            animation: None,
            animation_failure: None,
            update_failure: None,
        }
    }

    #[test]
    fn selection_follows_the_retained_swap_state_across_snapshots() {
        let map = ScriptActorSource::Map(Instance {
            object_id: 1,
            actor: "ACTOR".into(),
            object_type: Default::default(),
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            light_intensity: 0,
            light_radius: 0,
            light_rgb: [0; 3],
            texture: 0,
            map_position: [0; 3],
            angles: [0; 3],
        });
        // Creation order: ordinary actors, then CYRUS, GREMLIN and CAMERA.
        let actors: Vec<_> = [
            map,
            ScriptActorSource::Player,
            ScriptActorSource::AlternatePlayer,
            ScriptActorSource::Camera,
        ]
        .into_iter()
        .map(actor)
        .collect();
        let mut globals = super::super::GameplayGlobals::new_session();
        assert_eq!(selected_player_index(&actors, &globals), Some(1));
        globals.player_swapped = true;
        assert_eq!(selected_player_index(&actors, &globals), Some(2));
        let snapshot = serde_json::to_value(globals.snapshot()).unwrap();
        assert_eq!(snapshot["player_swapped"], true);
        // Saves written before the swap state existed restore with it clear.
        let mut legacy = snapshot;
        legacy.as_object_mut().unwrap().remove("player_swapped");
        let settings = cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        };
        let restore = |value| {
            super::super::GameplayGlobals::from_snapshot(
                serde_json::from_value(value).unwrap(),
                &settings,
                &[],
                &Default::default(),
            )
            .unwrap()
        };
        assert!(!restore(legacy).player_swapped());
        assert!(restore(serde_json::to_value(globals.snapshot()).unwrap()).player_swapped());
        // A world without the selected role has no selected player.
        assert_eq!(selected_player_index(&actors[..2], &globals), None);
        globals.player_swapped = false;
        assert_eq!(selected_player_index(&actors[..2], &globals), Some(1));
        assert_eq!(selected_player_index(&actors[2..], &globals), None);
    }

    #[test]
    fn ordinary_player_exclusion_follows_the_retained_swap_state() {
        let mut actors = vec![
            actor(ScriptActorSource::Player),
            actor(ScriptActorSource::AlternatePlayer),
            actor(ScriptActorSource::Map(Instance {
                object_id: 1,
                actor: "ACTOR".into(),
                object_type: Default::default(),
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                texture: 0,
                map_position: [0; 3],
                angles: [0; 3],
            })),
        ];
        let mut globals = super::super::GameplayGlobals::new_session();
        reassert_player_update_exclusion(&mut actors, &globals);
        assert!(!actors[0].control_state().unwrap().object_update_excluded);
        assert!(actors[1].control_state().unwrap().object_update_excluded);
        assert!(!actors[2].control_state().unwrap().object_update_excluded);

        globals.player_swapped = true;
        reassert_player_update_exclusion(&mut actors, &globals);
        assert!(actors[0].control_state().unwrap().object_update_excluded);
        assert!(!actors[1].control_state().unwrap().object_update_excluded);
    }

    #[test]
    fn new_game_returns_the_selected_player_to_cyrus() {
        let mut registry = ScriptActors::default();
        registry.globals.player_swapped = true;
        registry.begin_new_game();
        assert!(!registry.globals.player_swapped());
    }
}

#[cfg(test)]
#[path = "registry/lunge_tests.rs"]
mod lunge_tests;
