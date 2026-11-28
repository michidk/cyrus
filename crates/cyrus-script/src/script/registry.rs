//! Logical script instances retain authored object order and reject stale world identities.
#[cfg(feature = "test-support")]
pub(super) mod test_support;
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
mod native;
mod property;
mod script_control;
pub use camera_drive::{CameraDrive, CameraFrameSnapshot, CameraFrameState, PublishedCamera};
pub use script_control::ScriptControl;
mod damage;
mod lifecycle;
pub use damage::DamageEvent;
pub use lifecycle::{ActorPresence, ActorSnapshot, DeathEvent};
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
#[cfg(feature = "original-save-import")]
mod original_actor;
#[cfg(feature = "original-save-import")]
mod original_camera;
#[cfg(feature = "original-save-import")]
mod original_execution;
#[cfg(feature = "original-save-import")]
pub use original_execution::OriginalExecutionOwners;
#[cfg(feature = "original-save-import")]
mod original_animation;
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
mod authored_pool;
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
use std::collections::{BTreeMap, BTreeSet};
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
    /// Shared drno state: the entire ordinary update is skipped for every actor except the selected
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
    /// Optional recorded decision-entry service; ordinary gameplay uses the raw sample.
    pub decision_clock: Option<&'a dyn super::replay_clock::DecisionClock>,
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
    pub script_control: ScriptControl,
    /// The DEF `ai_type` lifecycle selector.
    pub ai_type: super::AiType,
    /// The DEF `ai_mode` byte, independent of the `ai_type` lifecycle selector.
    pub ai_mode: crate::script::AiMode,
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
    #[serde(rename = "sliding_state")]
    pub slide_flag: u8,
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
    /// The `rope_wait` countdown byte, allocator-zero: a rope release sets it to 24, ordinary updates
    /// count it down and the rope grab skips the released rope while it is nonzero. The weapon
    /// routines retain it.
    #[serde(rename = "jump_selector")]
    pub rope_wait: u8,
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
    pool_offsets: Option<cyrus_assets::script::ScriptPoolOffsets>,
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
            pool_offsets: None,
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
/// The most calls the watch log keeps before it drops new ones.
pub const NATIVE_LOG_LIMIT: usize = 4096;

/// One call to a watched native: the function, the script name of the actor that made it and its
/// integer arguments in order (actor and string arguments are left out).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCall {
    pub function: Native,
    pub actor: String,
    pub integers: Vec<i32>,
}

/// One actor as an observer sees it (`ScriptWorld::actor_facts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFact<'a> {
    /// The actor's script name.
    pub name: &'a str,
    /// Position in Q8 map units (256 to a map unit), original axes; none before spatial state.
    pub position: Option<[i32; 3]>,
    /// The `attr_health` byte.
    pub health: u8,
    /// A script hid it (`HideMe`), so it is not drawn.
    pub hidden: bool,
}

pub struct ScriptWorld {
    pub world: u32,
    pub content_id: String,
    generation: u64,
    authored_pool: Option<authored_pool::AuthoredPool>,
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
    /// A mod's jump scales for the selected player; not saved.
    player_tuning: crate::player::input::PlayerTuning,
    /// What a mod asks of the selected player this update; not saved.
    player_requests: crate::player::input::PlayerRequests,
    /// The selected player's animation pace this update and the fraction of a tick carried
    /// between updates; not saved.
    player_tempo: (f32, f32),
    /// Natives a mod watches and the calls made to them since the log was last taken; not saved.
    native_watch: BTreeSet<Native>,
    native_calls: Vec<NativeCall>,
    damage: damage::DamageLog,
    defence_pacing: std::collections::BTreeMap<usize, defence::DefencePacingObservation>,
    lifecycle: lifecycle::LifecycleLog,
    weapons: BTreeMap<u16, cyrus_assets::script::weapon::Weapon>,
    torches: BTreeMap<u16, cyrus_assets::script::weapon::Torch>,
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
    selected_wall_contacts: std::collections::VecDeque<u64>,
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
    pending_surface_transitions: Vec<SurfaceTransition>,
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

    /// Whether the `yeahbaby` Insert step holds the camera limiter suspended.
    pub fn yeahbaby_insert_active(&self) -> bool {
        self.insert_camera_flag
    }

    /// Whether the Home toggle of the yeahbaby ground bypass is set.
    pub fn ground_bypass_toggle(&self) -> bool {
        self.ground_bypass_toggle
    }

    /// Watch calls to these natives: each call is logged for [`Self::take_native_calls`] until the
    /// log holds [`NATIVE_LOG_LIMIT`] entries. An empty set stops watching.
    pub fn watch_natives(&mut self, natives: BTreeSet<Native>) {
        self.native_watch = natives;
        if self.native_watch.is_empty() {
            self.native_calls.clear();
        }
    }

    /// The watched native calls made since the last call, oldest first.
    pub fn take_native_calls(&mut self) -> Vec<NativeCall> {
        std::mem::take(&mut self.native_calls)
    }

    /// A mod's jump scales for the selected player. A change waits until the player is on the
    /// ground, so a jump or fall already under way keeps one height throughout.
    pub fn set_player_tuning(&mut self, tuning: crate::player::input::PlayerTuning) {
        let airborne = self
            .selected_player_index()
            .and_then(|index| self.actors[index].control.as_ref())
            .is_some_and(|control| {
                control.queued_motion != JumpMotion::NONE
                    || control.falling_state != JumpMotion::NONE
            });
        if !airborne {
            self.player_tuning = tuning;
        }
    }

    /// What a mod asks of the selected player this update; see [`crate::player::input::PlayerRequests`].
    pub fn set_player_requests(&mut self, requests: crate::player::input::PlayerRequests) {
        // A NaN, negative or absent pace is the original's.
        self.player_tempo.0 = requests
            .tempo
            .filter(|tempo| tempo.is_finite() && *tempo > 0.0)
            .map_or(1.0, |tempo| tempo.min(8.0));
        self.player_requests = requests;
    }

    /// `attr_jump_height` of an actor, scaled by a mod's tuning for the selected player. Jump and
    /// fall arithmetic read it through here so they agree for the length of a jump.
    /// The selected player's `attr_jump_height` as the jump and fall arithmetic read it, after a
    /// mod's tuning.
    pub fn selected_jump_height(&self) -> Option<u8> {
        self.selected_player_index()
            .map(|index| self.jump_height_of(index))
    }

    pub(super) fn jump_height_of(&self, index: usize) -> u8 {
        let base = self.actors[index].attributes[Attribute::JumpHeight];
        if self.selected_player_index() == Some(index) {
            (f32::from(base) * self.player_tuning.jump_height)
                .round()
                .clamp(0.0, 255.0) as u8
        } else {
            base
        }
    }

    pub fn selected_player(&self) -> Option<ScriptActorId> {
        self.id(self.selected_player_index()?).ok()
    }

    /// Whether the actor is a map cell actor outside the current view window. The world draw
    /// traversal walks the same window list as the ordinary update, so such an actor is not drawn.
    pub fn outside_view_window(&self, actor: ScriptActorId) -> bool {
        actor.generation == self.generation
            && self
                .actor_lists
                .as_ref()
                .is_some_and(|lists| lists.outside_window(actor.index))
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

    /// Publish the current logical camera to the synchronous sample owner, never its render glide.
    pub fn publish_sound_listener(&self, driver: &dyn cyrus_world::sound::AmbientSoundDriver) {
        driver.publish_listener(self.camera.as_ref().map(|camera| {
            cyrus_world::sound::ListenerPose {
                position: camera.current.position,
                yaw: cyrus_world::heading::wrap(camera.current.angles[1] as u16),
            }
        }));
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
        let control = self.actors[index]
            .control
            .as_ref()
            .context("ordinary combat control unavailable")?;
        if control.combat.is_none() {
            return Ok(());
        }
        let inputs = self.recorded_combat_inputs(id, inputs, random.as_deref())?;
        let actor = &self.actors[index];
        let control = actor.control.as_ref().expect("admitted combat control");
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
        Self::prepare_with_shells(scripts, generation, &scripts.shells)
    }

    fn prepare_with_shells(scripts: &Scripts, generation: u64, shells: &[String]) -> Result<Self> {
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
                            index: usize|
         -> Result<ScriptActor> {
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
                ScriptActorSource::Player
                | ScriptActorSource::AlternatePlayer
                | ScriptActorSource::Camera => Some(ActorControlState::default()),
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
            let pool_offsets = metadata
                .and_then(|actor| actor.pool_offsets)
                .map(|offsets| {
                    let ordinal = allocations
                        .get(name)
                        .copied()
                        .and_then(|n| n.checked_sub(1))
                        .context("script pool instance allocation unavailable")?;
                    offsets.for_instance(
                        ordinal,
                        metadata.map_or(0, |actor| actor.program.variables.len()),
                    )
                })
                .transpose()
                .with_context(|| format!("{context}: pool binding"))?;
            Ok(ScriptActor {
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
                pool_offsets,
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
            })
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
            )?);
        }
        // Shell scripts exist before the standalone roles; the original prepends each record to its
        // list, so they run newest first (see `update_shells`).
        for name in shells {
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
                actors.push(allocate(source, Some(identifier), index)?);
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
            authored_pool: authored_pool::AuthoredPool::initialized(scripts).ok(),
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
            player_tuning: Default::default(),
            player_requests: Default::default(),
            player_tempo: (1.0, 0.0),
            native_watch: BTreeSet::new(),
            native_calls: Vec::new(),
            damage: Default::default(),
            defence_pacing: Default::default(),
            lifecycle: Default::default(),
            weapons: scripts.weapons.clone(),
            torches: scripts.torches.clone(),
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
            selected_wall_contacts: Default::default(),
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

    /// One rope-system step: publish incoming geometry, service amplitude (playing swing sounds
    /// at the base, owned by the first attached actor), then advance swing and attachment climbs.
    pub(super) fn update_ropes(
        &mut self,
        frame_scale: i32,
        ticks: i32,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let view = self
            .actor_lists
            .as_ref()
            .and_then(|lists| lists.rope_view());
        for rope_index in 0..self.ropes.len() {
            let rope = &mut self.ropes[rope_index];
            if !Self::rope_in_retained_window(rope, view) {
                continue;
            }
            rope.clamp_nodes();
            rope.publish_geometry();
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
            if !Self::rope_in_retained_window(rope, view) {
                continue;
            }
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
        let published_camera = self.camera.as_ref().and_then(|camera| {
            self.spatial(camera.camera_actor).ok()?;
            Some(camera.current.position)
        });
        self.actor_lists
            .as_mut()
            .expect("actor lists prepared")
            .select(&self.actors, self.generation, view, published_camera)
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
    /// slave group reads its retained group-angle table even when no current master visits it.
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
        let master_angles = spatial.family_angles(group);
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
        anyhow::ensure!(
            actor.control.is_some(),
            "mark dead: actor {index} has no control state"
        );
        self.set_actor_ai_type(index, AiType::DEAD);
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
            .slide_flag = u8::from(sliding);
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
        let selected = self.selected_player_index() == Some(index);
        let spatial = self
            .spatial
            .as_mut()
            .context("spatial state not prepared")?
            .actor_mut(index)?;
        if selected {
            spatial.refresh_player_transform()
        } else {
            spatial.refresh_transform()
        }
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
        let result = if self.updates.animation_system {
            crate::player::placement::initial_role_animation(data, actor.name(), ticks, &mut emit)
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
        if matches!(stage, ScriptStage::SeparateMain | ScriptStage::SeparateHook) {
            self.retain_separate_native_source(index);
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
            if control
                .script_control
                .contains(ScriptControl::HOOK_DISABLED)
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
            if control
                .script_control
                .contains(ScriptControl::SCRIPT_DISABLED)
            {
                return self.finish_ordinary_script_response(id, nominal_ticks);
            }
            control
                .script_control
                .contains(ScriptControl::TASKS_DISABLED)
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
        host.registry.refresh_retained_source_locals(index);
        if matches!(stage, ScriptStage::SeparateMain | ScriptStage::SeparateHook) {
            host.registry.retain_separate_native_source(index);
        }
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

        // Deferred probes belong to the outgoing selection, never its replacement.
        // Re-selecting the same role preserves its pending work.
        if self.selected_player_index() != Some(selected) {
            self.pending_surface_transitions.clear();
        }
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
                "combat_pacing": self.defence_pacing.get(&index),
                "spatial": self.spatial.as_ref().and_then(|spatial| spatial.actor(index).ok()),
                "animation": actor.animation.as_ref().map(|animation| serde_json::json!({
                    "current_group": animation.playback.current_group(),
                    "default_group": animation.playback.default_group(),
                    "command_frame": animation.playback.record_field(cyrus_world::animation::PlaybackField::Frame).ok(),
                    "speed_count": animation.playback.record_field(cyrus_world::animation::PlaybackField::SpeedCount).ok(),
                    "real_frame": animation.playback.record_field(cyrus_world::animation::PlaybackField::RealFrame).ok(),
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
    /// What a read-only observer such as a mod may know of each actor, in actor order.
    pub fn actor_facts(&self) -> impl Iterator<Item = ActorFact<'_>> + '_ {
        self.actors
            .iter()
            .enumerate()
            .map(|(index, actor)| ActorFact {
                name: actor.name(),
                position: self
                    .spatial
                    .as_ref()
                    .and_then(|spatial| spatial.actor(index).ok())
                    .map(|pose| pose.local.position),
                health: actor.attributes[Attribute::Health],
                hidden: actor.control.as_ref().is_some_and(|control| control.hidden),
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

    /// Apply validated original-save scalar state to an off-live initialized world.
    /// The caller discards the candidate on error; continuations are not restored.
    #[cfg(feature = "original-save-import")]
    pub(super) fn restore_original_player(
        &mut self,
        position: [i32; 3],
        heading: u16,
        health: u8,
        flags: super::GlobalFlagsSnapshot,
    ) -> Result<()> {
        let player = self
            .selected_player()
            .context("original save: selected player unavailable")?;
        anyhow::ensure!(
            matches!(self.actors[player.index].source, ScriptActorSource::Player),
            "original save: native selected player is transformed; base player required"
        );
        let spatial = self
            .spatial(player)
            .context("original save: player spatial state unavailable")?;
        anyhow::ensure!(
            spatial.parent.is_none() && spatial.link.is_none(),
            "original save: native linked-player coordinate continuation is unsupported"
        );
        self.restore_flags(flags)?;
        self.place_selected_player(position, heading)?;
        self.set_model_support(player, super::ModelSupportOwners::default())?;
        let index = self.resolve(player)?;
        self.actors[index].attributes[Attribute::Health] = health;
        self.globals.selected_health = Some(super::SelectedHealthState::initialized(health));
        Ok(())
    }

    #[cfg(feature = "original-save-import")]
    pub(super) fn restore_original_animations(
        &mut self,
        saved: &[super::original_save::OriginalActorPlayback],
        data: &Animations,
    ) -> Result<()> {
        use cyrus_world::animation::OriginalPlaybackRestore;
        for record in saved {
            let id = if let Some(identity) = record.identity {
                let mut matches = self
                    .actors
                    .iter()
                    .enumerate()
                    .filter(|(_, actor)| actor.runtime_identifier == Some(identity));
                let Some((index, _)) = matches.next() else {
                    log::warn!(
                        "original save world {} actor {identity}: animation owner absent; native baseline retained",
                        self.world
                    );
                    continue;
                };
                anyhow::ensure!(
                    matches.next().is_none(),
                    "original save world {} actor {identity}: ambiguous animation owner",
                    self.world
                );
                self.id(index)?
            } else {
                self.selected_player()
                    .context("original animation import: player missing")?
            };
            let index = self.resolve(id)?;
            if record.model_frame != record.last_model_frame
                || record.model_handle != record.last_model_handle
            {
                log::warn!(
                    "original save world {} actor {}: distinct last model/frame binding unsupported; native baseline retained",
                    self.world,
                    self.actors[index].name()
                );
                continue;
            }
            if self.actors[index].animation.is_none() {
                self.bind_animation(id, data, record.default)?;
            }
            let animation = self.actors[index].animation.as_mut().unwrap();
            let result = animation.playback.restore_original(&record.playback)?;
            let OriginalPlaybackRestore::Restored { frame } = result else {
                log::warn!(
                    "original save world {} actor {}: selected pose outside ShowFrame unsupported; native baseline retained",
                    self.world,
                    self.actors[index].name()
                );
                continue;
            };
            if let Some(reference) = frame {
                let resolved = match self.actors[index].source {
                    ScriptActorSource::Map(_) => data.resolve_map_frame(index, reference)?,
                    _ => data.resolve_frame(self.actors[index].name(), reference)?,
                };
                anyhow::ensure!(
                    i32::try_from(resolved.frame)? == i32::from(record.model_frame),
                    "original save world {} actor {}: saved model frame {} differs from installed animation selection {}",
                    self.world,
                    self.actors[index].name(),
                    record.model_frame,
                    resolved.frame
                );
            } else {
                anyhow::ensure!(
                    record.model_frame == -1,
                    "original save world {} actor {}: retained pose without a ShowFrame binding is unsupported",
                    self.world,
                    self.actors[index].name()
                );
            }
            // Ordinary living-default selection reads the actor attribute each update,
            // independently of the playback record's stored default. Restore both so
            // its first selection does not replace imported playback with the baseline.
            if let Some(group) = record.panic_group {
                self.actors[index].attributes[Attribute::PanicGroup] = group;
            }
            let animation = self.actors[index].animation.as_mut().unwrap();
            animation.playback.set_stored_default(record.default);
            animation.playback.set_last_frame(frame);
            animation.frame = frame;
        }
        Ok(())
    }

    /// Restore an unanimated support owner before publishing an imported candidate.
    #[cfg(feature = "original-save-import")]
    pub(super) fn restore_original_support(
        &mut self,
        saved: super::original_save::OriginalSupport,
    ) -> Result<()> {
        let mut matches = self
            .actors
            .iter()
            .enumerate()
            .filter(|(_, actor)| actor.runtime_identifier == Some(saved.identity));
        let (index, actor) = matches
            .next()
            .context("original save support: initialized actor missing")?;
        anyhow::ensure!(
            matches.next().is_none(),
            "original save support: ambiguous initialized actor {}",
            saved.identity
        );
        let normalize = |name: &str| name.trim_end_matches(".3").to_ascii_uppercase();
        anyhow::ensure!(
            actor
                .model
                .static_model
                .as_deref()
                .is_some_and(|name| normalize(name) == normalize(&saved.model))
                && actor.model.static_radius.is_some(),
            "original save support {}: initialized static model does not match {}",
            saved.identity,
            saved.model
        );
        anyhow::ensure!(
            actor.native_hook.is_none(),
            "original save support {}: native hook unsupported",
            saved.identity
        );
        let actor = &mut self.actors[index];
        if let Some(runtime) = actor.runtime.as_mut() {
            runtime.discard_execution_with(|token| actor.tasks.cancel(token))?;
        }
        let id = self.id(index)?;
        let spatial = self.spatial(id)?;
        anyhow::ensure!(
            spatial.parent.is_none() && spatial.link.is_none(),
            "original save support {}: native attachment unsupported",
            saved.identity
        );
        let spatial = self
            .spatial
            .as_mut()
            .context("original save support: spatial state missing")?
            .actor_mut(index)?;
        spatial.relocate(saved.position, (saved.angles[1] & 2047) as u16);
        spatial.local.angles = saved.angles;
        spatial.local.matrix = cyrus_world::transform::Matrix::orientation(
            saved.angles.map(|angle| (angle & 2047) as u16),
        );
        spatial.angle_history = saved.angles;
        spatial.effective = Some(spatial.local);
        spatial.previous_effective_position = Some(saved.position);
        spatial.previous_effective_angles = Some(saved.angles);
        let pose = spatial.local;
        self.actors[index].attributes = saved.attributes.into();
        self.actors[index].model.show_static(pose);
        self.map_updates.register(index)?;
        let player = self
            .selected_player()
            .context("original save support: player missing")?;
        self.flush_map_updates(self.spatial(player)?.local.position, false)?;
        self.set_model_support(
            player,
            super::ModelSupportOwners {
                actor: Some(id),
                static_placement: None,
            },
        )?;
        Ok(())
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
    /// Retained game main-loop counter, including its saved continuation.
    /// Unlike diagnostic ordinary updates, restoring a world does not restart it.
    pub fn world_loop_frame(&self) -> u32 {
        self.globals.world_loop_frame
    }

    /// Ordinary updates executed by this world since it was created or restored.
    pub fn ordinary_updates(&self) -> u64 {
        self.ordinary_updates
    }
    /// Last ordinary update with an admitted selected-player primary wall contact.
    /// Observation only; absent in a new/restored world and independent of later actors' queries.
    pub fn selected_wall_contact_update(&self) -> Option<u64> {
        self.selected_wall_contacts.back().copied()
    }
    /// Earliest retained wall contact after the caller's arm update. Rejects lost history.
    pub fn first_selected_wall_contact_after(&self, after: u64) -> Result<Option<u64>> {
        anyhow::ensure!(
            self.selected_wall_contacts.len() < 64
                || self
                    .selected_wall_contacts
                    .front()
                    .is_none_or(|first| after >= *first),
            "world {}: requested wall contact history expired",
            self.world
        );
        Ok(self
            .selected_wall_contacts
            .iter()
            .copied()
            .find(|update| *update > after))
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
impl ScriptActors {
    /// Whether the active world is a published saved continuation, rather than a
    /// fresh world that still needs its initial ordinary updates. Diagnostic update
    /// counters restart at zero on load and cannot distinguish those two cases.
    pub fn current_is_restored(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|world| self.restored_generation == Some(world.generation()))
    }

    /// Restore a no-world checkpoint after rejected preliminary world entry.
    /// Preserve the generation high-water mark so retired actor bindings cannot
    /// alias the next world even when the checkpoint predates that entry.
    pub fn restore_empty_checkpoint(&mut self, mut checkpoint: Self) -> Result<()> {
        anyhow::ensure!(
            checkpoint.current.is_none(),
            "startup rollback checkpoint contains a current world"
        );
        checkpoint.generation = checkpoint.generation.max(self.generation);
        checkpoint.requested = None;
        *self = checkpoint;
        Ok(())
    }
}

#[cfg(test)]
mod startup_checkpoint_tests {
    use super::*;
    #[test]
    fn restoring_empty_checkpoint_preserves_retired_generation_high_water() {
        let mut retired = ScriptActors {
            generation: 73,
            ..Default::default()
        };
        retired
            .restore_empty_checkpoint(ScriptActors::default())
            .unwrap();
        assert!(retired.current.is_none());
        assert_eq!(retired.generation, 73);
        assert!(retired.requested.is_none());
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
                dialogue.clear_world_lines();
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
        if self.error.is_none() {
            log::error!(
                "script updates stopped in world {:?}: {error}",
                self.current.as_ref().map(|world| world.world)
            );
            self.error = Some(error);
        }
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
            super::activation::NativeSource::Separate => {
                anyhow::bail!(
                    "{native}: shared separate temporary source has no registry actor identity"
                )
            }
        }
    }

    fn separate_native_source(
        &self,
        native: &str,
    ) -> Result<&super::activation::SeparateNativeSource> {
        self.registry
            .globals
            .separate_native_source
            .as_ref()
            .with_context(|| format!("{native}: retained separate native source contents absent"))
    }

    fn native_source_local(&self, native: &str) -> Result<super::spatial::Pose> {
        if matches!(
            self.registry.globals.native_source,
            super::activation::NativeSource::Separate
        ) {
            if let Some(index) = self.registry.active_separate_native_source() {
                return Ok(self.registry.spatial(self.registry.id(index)?)?.local);
            }
            return self
                .separate_native_source(native)?
                .local
                .as_ref()
                .map(super::activation::NativeSourcePose::pose)
                .with_context(|| format!("{native}: retained separate source pose unavailable"));
        }
        Ok(self
            .registry
            .spatial(self.native_source_actor(native)?)?
            .local)
    }

    fn native_source_position(&self, native: &str) -> Result<[i32; 3]> {
        if matches!(
            self.registry.globals.native_source,
            super::activation::NativeSource::Separate
        ) {
            if let Some(index) = self.registry.active_separate_native_source() {
                return self.registry.query_position(self.registry.id(index)?);
            }
            return self
                .separate_native_source(native)?
                .position
                .with_context(|| {
                    format!("{native}: retained separate source position unavailable")
                });
        }
        self.registry
            .query_position(self.native_source_actor(native)?)
    }

    fn native_source_attribute(&self, native: &str, attribute: Attribute) -> Result<u8> {
        if matches!(
            self.registry.globals.native_source,
            super::activation::NativeSource::Separate
        ) {
            if let Some(index) = self.registry.active_separate_native_source() {
                return Ok(self.registry.actors[index].attributes[attribute]);
            }
            return Ok(self.separate_native_source(native)?.attributes[attribute]);
        }
        Ok(self.registry.actors[self.native_source_actor(native)?.index].attributes[attribute])
    }

    fn native_source_model_position(&self, native: &str) -> Result<[f32; 3]> {
        let position = if matches!(
            self.registry.globals.native_source,
            super::activation::NativeSource::Separate
        ) {
            if let Some(index) = self.registry.active_separate_native_source() {
                self.registry.actors[index].model.position
            } else {
                self.separate_native_source(native)?
                    .model_position
                    .map(|position| position.map(f32::from_bits))
            }
        } else {
            self.registry.actors[self.native_source_actor(native)?.index]
                .model
                .position
        };
        position.with_context(|| format!("{native}: source stored model position unavailable"))
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
            .is_some_and(|control| {
                control
                    .script_control
                    .contains(ScriptControl::HOOK_DISABLED)
            }))
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
            activation: Some(&mut self.registry.activation),
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
        let camera_updated = !published.is_empty();
        self.registry.apply_camera_publication(published)?;
        if camera_updated && let Some(driver) = self.inputs.ambient_sound {
            self.registry.publish_sound_listener(driver);
        }
        Ok(())
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
        let position = self.registry.spatial.as_ref().and_then(|world| {
            let spatial = world.actor(self.index).ok()?;
            if spatial.parent.is_some() {
                Some(spatial.effective?.position)
            } else {
                Some(spatial.linked_position.unwrap_or(spatial.local.position))
            }
        });
        super::tasks::AmbientCall {
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
            control.script_control.insert(ScriptControl::HOOK_DISABLED);
        } else {
            control.script_control.remove(ScriptControl::HOOK_DISABLED);
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
            slide_flag: control.slide_flag,
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
        // A mod's tempo scales the selected player's animation ticks, carrying the remainder.
        let ticks = if self.registry.selected_player_index() == Some(self.index) {
            let (tempo, carry) = &mut self.registry.player_tempo;
            let scaled = ticks as f32 * *tempo + *carry;
            let whole = scaled.floor();
            *carry = scaled - whole;
            whole as u32
        } else {
            ticks
        };
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
            self.registry.step_player(
                id,
                function,
                self.inputs,
                &mut **sounds,
                self.random.as_deref_mut(),
            )?;
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
/// A selected-player surface update: previous, returned and stored surfaces at the update,
/// plus the splash effect placement and sound position. Deferred dispatch retains query-time values.
#[derive(Clone, Copy, Debug)]
pub(super) struct SurfaceTransition {
    pub previous: Surface,
    pub returned: Surface,
    pub stored: Surface,
    /// Queued probes capture their position; immediate handlers sample only if they emit a sound.
    pub sound_position: Option<[i32; 3]>,
    pub effect: SurfaceEffectPlacement,
    pub animation: SurfaceAnimationRequest,
}

/// Deferred transitions retain the request selected by the probe's motion and torch state.
#[derive(Clone, Copy, Debug)]
pub(super) enum SurfaceAnimationRequest {
    Immediate,
    Captured(Option<AnimationGroup>),
}

/// Immediate transitions resolve a support height against live actor/camera poses; deferred
/// queries capture the final Q8 effect point, including the camera-relative offset.
#[derive(Clone, Copy, Debug)]
pub(super) enum SurfaceEffectPlacement {
    Immediate { height: Option<i32> },
    Captured { position: Option<[i32; 3]> },
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
        let before = attributes[Attribute::Health];
        let sum = i32::from(before).wrapping_add(amount);
        attributes[Attribute::Health] = sum.clamp(0, 255) as u8;
        self.registry.observe_health_loss(self.index, before);
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
        self.actor_task_slot_property(target, reference)
    }

    fn take_actor_result(&mut self) -> Option<ScriptActorId> {
        self.registry.globals.actor_result.take()
    }

    fn establish_native_source(
        &mut self,
        _: cyrus_soup::CodeOrigin,
        program: std::sync::Arc<cyrus_soup::program::Program>,
    ) {
        self.registry.globals.native_source = if self.registry.actors[self.index]
            .placement()
            .is_some_and(|placement| placement.object_type.has_separate_record())
        {
            super::activation::NativeSource::Separate
        } else {
            super::activation::NativeSource::Actor {
                generation: self.generation,
                index: self.index,
            }
        };
        self.native_source_program = Some(program);
    }

    fn native_string_parameter(&mut self, index: u32, _: &[String]) -> Result<String> {
        if matches!(
            self.registry.globals.native_source,
            super::activation::NativeSource::Separate
        ) {
            return self
                .separate_native_source("native string parameter")?
                .strings
                .get(index as usize)
                .cloned()
                .with_context(|| {
                    format!(
                        "world {} actor {}: retained separate source string {index} absent",
                        self.world, self.index
                    )
                });
        }
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
        self.read_script_value(target, reference)
    }

    fn read_property(&mut self, target: &Target, reference: u16) -> Result<i32> {
        self.read_actor_property(target, reference)
    }

    fn write_property(&mut self, target: &Target, reference: u16, value: i32) -> Result<()> {
        self.write_actor_property(target, reference, value)
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
        if self.registry.native_watch.contains(&function)
            && self.registry.native_calls.len() < NATIVE_LOG_LIMIT
        {
            let call = NativeCall {
                function,
                actor: self.registry.actors[self.index].name().to_owned(),
                integers: arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Integer(value) => Some(*value),
                        _ => None,
                    })
                    .collect(),
            };
            self.registry.native_calls.push(call);
        }
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
        if let Some(result) =
            self.call_action(target, kind, function, arguments, invocation, flags)?
        {
            return Ok(result);
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
        if let Some(result) = self.call_delete_broadcast(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_jump(kind, function, arguments, id)? {
            return Ok(result);
        }
        self.prepare_lineup_call(function);
        if let Some(result) = self.call_effects(kind, function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_menu(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_reset_sync(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_begin_combat(kind, function, arguments, id)? {
            return Ok(result);
        }
        if let Some(result) = self.call_inventory(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_shoot(kind, function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_input(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_shell(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_activation(kind, function, arguments, id)? {
            return Ok(result);
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
        if let Some(result) = self.call_clear_tasks(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_spatial_transform(function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_camera(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_reload_rotation(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_lifecycle(function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_model_visibility(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_weapon_state(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_change_health(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_player_motion(function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_dialogue(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_map_status(function)? {
            return Ok(result);
        }
        if let Some(result) = self.call_messages(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_animal(function)? {
            return Ok(result);
        }
        if let Some(result) = self.call_world_control(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_node_map_control(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_animation_control(kind, function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_world_request(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_family_membership(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_coordination(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_ai_control(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_health_query(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_combat_query(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_script_control(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_health_parameters(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_animation_playback(kind, function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_distance_from_start(function, arguments, id)? {
            return Ok(result);
        }
        if let Some(result) = self.call_shell_context(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_spatial_region(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_combat_control(function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_dead_query(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_random(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_sound(function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_wander(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_node_marker(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_attachment_and_turn(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_player_placement(kind, function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_node_position(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_reload_position(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_node_location(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_player_contact(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_surface(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_lifecycle_query(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_diagnostics(function, arguments)? {
            return Ok(result);
        }

        if let Some(result) = self.call_player_sight(function, arguments)? {
            return Ok(result);
        }
        if let Some(result) = self.call_spatial_distance(function, arguments, id)? {
            return Ok(result);
        }

        if let Some(result) = self.call_flags(function, arguments, flags)? {
            return Ok(result);
        }
        if let Some(result) = self.call_attributes(function, arguments)? {
            return Ok(result);
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
                activation: Some(&mut self.registry.activation),
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
        self.registry.refresh_retained_source_locals(index);
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
                pool_offsets: None,
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
            authored_pool: None,
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
            player_tuning: Default::default(),
            player_requests: Default::default(),
            player_tempo: (1.0, 0.0),
            native_watch: BTreeSet::new(),
            native_calls: Vec::new(),
            damage: Default::default(),
            defence_pacing: Default::default(),
            lifecycle: Default::default(),
            weapons: BTreeMap::new(),
            torches: Default::default(),
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
            selected_wall_contacts: Default::default(),
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
            pool_offsets: None,
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

#[cfg(feature = "original-save-import")]
mod original_presentation;

#[cfg(feature = "original-save-import")]
mod original_family;
#[cfg(feature = "original-save-import")]
mod original_world;

#[cfg(feature = "original-save-import")]
mod original_combat;
#[cfg(feature = "original-save-import")]
mod original_composition;
#[cfg(feature = "original-save-import")]
pub use original_composition::OriginalWorldAllocations;

#[cfg(feature = "original-save-import")]
mod original_session;
#[cfg(feature = "original-save-import")]
pub use original_session::OriginalSessionImport;

#[cfg(feature = "original-save-import")]
mod original_export;
