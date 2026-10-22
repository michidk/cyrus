//! Logical script instances retain authored object order and reject stale world identities.
use cyrus_script::program::{Call, CallKind, Operation};
mod activation;
mod attack;
mod camera;
mod camera_drive;
pub use camera_drive::{CameraDrive, CameraFrameState, PublishedCamera};
mod death_exit;
mod effects;
mod health;
pub use death_exit::WorldLoopOutcome;
pub use effects::{ActorEvent, EnvironmentalEffect, EnvironmentalEffects};
pub use health::{SelectedDeathState, SelectedHealthState};
pub(super) mod body;
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
mod follow;
pub(super) mod ground;
pub(super) mod hazard;
mod item_use;
mod late;
mod ledge;
mod light;
mod lineup;
mod linked;
pub(super) mod model;
pub(super) mod obstacle;
pub(super) mod ordinary;
pub(super) mod overlap;
mod persistence;
mod point;
mod point_tasks;
mod probe;
mod projectile;
mod quick_keys;
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
use cyrus_script::{Argument, CallResult, CodeOrigin, Host, Route, Runtime, Target, Thread};
use cyrus_world::lifecycle::PreparedWorld;
use std::collections::BTreeMap;
use std::sync::Arc;
pub use weapon::{CombatEntryInputs, WeaponInputs};

/// Runtime identity is independent of render nodes and original object-name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ScriptActorId {
    pub generation: u64,
    pub index: usize,
}

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
    pub activation: Option<&'a super::ActivationInputs>,
    /// Held installed Activate binding for live logical-actor activation.
    pub activation_use_held: Option<bool>,
    /// Quick sword and quick health keys for the ordinary update's key handler.
    pub quick_keys: Option<super::QuickKeys>,
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
#[derive(Debug, Default, serde::Serialize)]
pub struct ActorControlState {
    pub navigation: super::ActorNavigation,
    pub point_command: super::PointCommandState,
    /// Runtime script flags, separate from installed attribute bytes.
    pub script_control: u8,
    /// Lifecycle selector: combat entry uses six; IsDead tests eight.
    pub death_selector: u8,
    /// Companion AI mode byte, independent of the lifecycle death notification.
    pub death_response: u8,
    /// Motion/weapon state, independent of the combat/death lifecycle selector.
    pub living_state: u8,
    /// Allocator-zero companion motion byte; weapon cleanup clears it with draw/sheath state.
    pub motion_companion: u8,
    /// Allocator-zeroed runtime motion bytes, independent of animation groups.
    pub sliding_state: u8,
    pub falling_state: u8,
    pub queued_motion: u8,
    pub motion_direction: u8,
    /// Retained movement-helper classification, distinct from displacement direction.
    pub motion_classification: u8,
    /// Allocator-zero directional-state bytes; low bits gate surface assignment.
    pub directional_state: [u8; 2],
    /// Allocator-zero action-transition counter; state-17 reset assigns six.
    pub action_transition_counter: u8,
    /// Packed left/right/forward/backward counters, two bits each; directional motion saturates its field.
    pub movement_flags: u8,
    /// Retained integer motion components; obstacle interruption only clears them here.
    pub motion_vector: [i32; 3],
    /// Original fall origin in Q8 and byte countdown, allocated as zero.
    pub fall_origin_y: i32,
    pub fall_timer: u8,
    /// Allocator-zero runtime jump selector, separate from living and torch state.
    pub jump_selector: u8,
    /// Validity of the separately retained stable local position.
    pub stable_position_valid: bool,
    /// Separately retained stable local Q8 coordinates, allocated as zero.
    pub stable_local_position: [i32; 3],
    /// Accumulated sliding distance in Q8, cleared by ordinary jump entry.
    pub sliding_distance: i32,
    /// Ordered body response marks; cleared only by their owning update stage.
    pub body_response_flags: u8,
    pub collision_transition_flags: u8,
    /// Blocked-step byte followed by the remaining auxiliary response byte; both clear at wrapper cleanup.
    pub collision_response_auxiliary: [u8; 2],
    /// Allocator-zero collision-response age, incremented before clearing prior marks.
    pub collision_response_age: u8,
    /// Original low-word overlap contact identifier; zero also denotes a static placement.
    pub overlap_contact_identifier: u16,
    /// Kind of the last ground-response record the selected player's ordinary support query chose:
    /// 0 none, 0x20 a model or actor face, 0x40 terrain. `PlayerOnScape` reads it.
    pub ground_kind: u8,
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
    pub source: ScriptActorSource,
    /// Signed low word used by namespace lookup; absent for separate map object paths.
    pub runtime_identifier: Option<i16>,
    pub namespace: Option<String>,
    native_hook: Option<Call>,
    combat_taunts: Vec<CombatTaunt>,
    runtime: Option<Runtime>,
    unavailable: Option<String>,
    attributes: Vec<u8>,
    control: Option<ActorControlState>,
    surface: Option<u8>,
    model: model::ModelState,
    collision: Option<ground::OrdinaryCollisionShape>,
    player_movement: Option<player::PlayerMovement>,
    light: Option<light::LightState>,
    flat: Option<flat::FlatState>,
    pub(super) location_offsets: Vec<[i32; 3]>,
    tasks: super::tasks::NativeTasks,
    native_commands: [super::NativeCommandState; 6],
    animation: Option<ScriptAnimation>,
    animation_failure: Option<String>,
    update_failure: Option<String>,
}
/// What starting a shell script needs from its installed definition: the shared program, its
/// code origin and a copy of the initial local variables.
struct ShellTemplate {
    program: Arc<cyrus_script::program::Program>,
    code_origin: u32,
    locals: Vec<i32>,
    namespace: Option<String>,
    native_hook: Option<Call>,
    combat_taunts: Vec<CombatTaunt>,
    location_offsets: Vec<[i32; 3]>,
    /// Installed attribute defaults by index, as an ordinary actor of this script starts with.
    attributes: Vec<u8>,
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
        actor.surface = Some(0);
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
            source: ScriptActorSource::Shell(name.to_owned()),
            runtime_identifier: None,
            namespace: template.and_then(|template| template.namespace.clone()),
            native_hook: template.and_then(|template| template.native_hook.clone()),
            combat_taunts: template
                .map(|template| template.combat_taunts.clone())
                .unwrap_or_default(),
            runtime,
            unavailable,
            attributes: vec![0; 256],
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
            .is_some_and(|control| control.falling_state != 0);
        if let Some(animation) = self.animation.as_mut() {
            animation.playback.set_motion_active(active);
        }
    }
    fn ordinary_collision_role(&self) -> bool {
        matches!(self.source, ScriptActorSource::Player)
            || self
                .placement()
                .is_some_and(|placement| placement.object_type == 1)
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
    pub fn attributes(&self) -> &[u8] {
        &self.attributes
    }
    /// Retained ordinary-map/player allocation state; separate records have no persistent control.
    pub fn control_state(&self) -> Option<&ActorControlState> {
        self.control.as_ref()
    }
    pub(super) fn bound_animation_group(&self) -> Option<Option<u16>> {
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
    texture_frames: flat::TextureFrames,
    non_engaged_spacing: Option<i32>,
    combat_clank_effects: [Option<i32>; 2],
    node_timer: Option<i32>,
    attribute_names: Vec<String>,
    reference_names: Vec<String>,
    names: BTreeMap<i16, usize>,
    actor_lists: Option<super::order::ActorLists>,
    markers: Option<super::markers::Markers>,
    walk_nodes: Option<super::navigation::WalkNodeReservations>,
    next_combat_index: i32,
    ordinary_prepared: bool,
    /// The model candidate list of the latest ordinary candidate gather. The original keeps one
    /// global list, so a receiver that gathers none reads its predecessor's; transient, never saved.
    retained_candidates: cyrus_collision::candidates::CollisionCandidates,
    model_admission: Option<model::AdmissionState>,
    map_updates: Vec<usize>,
    /// Shared surface-query veto; absent until an assigning query establishes it.
    surface_veto: Option<bool>,
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
    /// The selected player's placement raised by `MovePlayerAboveMe`, applied at the end of the
    /// frame loop.
    pending_player_placement: Option<([i32; 3], u16)>,
    effects: Option<EnvironmentalEffects>,
}
impl ScriptWorld {
    /// Index of the selected player (CYRUS, or GREMLIN while the swap state is set).
    pub(super) fn selected_player_index(&self) -> Option<usize> {
        selected_player_index(&self.actors, &self.globals)
    }

    /// The selected player: CYRUS, or GREMLIN while the swap state is set. Hosts bind their player
    /// entity, controller and camera to this actor rather than to a fixed role.
    /// Test hook: write one attribute byte of a logical actor.
    #[doc(hidden)]
    pub fn set_attribute_for_test(&mut self, id: ScriptActorId, attribute: usize, value: u8) {
        self.actors[id.index].attributes[attribute] = value;
    }

    pub fn selected_player(&self) -> Option<ScriptActorId> {
        self.id(self.selected_player_index()?).ok()
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
        code: i32,
    ) -> Result<()> {
        if code == 0 {
            return Ok(());
        }
        self.resolve(sender)?;
        let index = self.resolve(receiver)?;
        self.globals.combat_messages.send(
            self.actors[index].attributes[125],
            super::CombatMessage {
                sender,
                receiver: Some(receiver),
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
        self.globals
            .combat_messages
            .peek(receiver.attributes[125], actor, receiver.message_read)
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
            receiver.attributes[125],
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
        mut random: Option<&mut crate::random::EngineRandom>,
        sounds: &mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            self.actors[index].placement().is_some()
                || matches!(self.actors[index].source, ScriptActorSource::Player),
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
        let selected = matches!(actor.source, ScriptActorSource::Player);
        if selected
            && self
                .globals
                .selected_combat_inhibition
                .context("selected-player combat entry inhibition unavailable")?
                != 0
        {
            return Ok(());
        }
        if !selected && control.death_selector == 8 {
            return Ok(());
        }
        if !selected
            && actor.attributes[116] == 0
            && control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.flags & 4 != 0 && combat.target.is_some())
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
        self.actors[index].attributes[1] = self.actors[index].attributes[119];
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
                .death_selector
                == 6
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
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("admitted control");
        let combat = control.combat.as_mut().expect("admitted combat");
        if selected {
            anyhow::ensure!(
                combat.decision.attack_state == 0,
                "selected-player combat nonzero maintenance phase unsupported"
            );
            if combat.action != 0 {
                combat.action = -1;
                combat.retained_action_indices = [-1; 2];
            }
            // Both maintenance helpers with action -1 are inert in phase zero.
        }
        if !player_weapon_transition {
            if combat.cooldown != 0 {
                combat.cooldown = combat.cooldown.saturating_sub(ticks);
            } else if combat.action != -1 {
                if (0..=6).contains(&combat.action) {
                    if self.combat_attack_window(id)? {
                        self.actors[index]
                            .control
                            .as_mut()
                            .expect("admitted control")
                            .combat
                            .as_mut()
                            .expect("admitted combat")
                            .flags |= 0x20;
                        self.process_combat_attack_candidates(
                            id,
                            inputs,
                            random.context("attack candidate random owner unavailable")?,
                            sounds,
                        )?;
                    }
                } else {
                    combat.action = -1;
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
                        attributes: (0..256)
                            .map(|index| {
                                let key = scripts
                                    .definition
                                    .attributes
                                    .get(index)
                                    .filter(|name| !name.is_empty())
                                    .cloned()
                                    .unwrap_or_else(|| format!("attr_{index}"));
                                actor.attributes.get(&key).copied().unwrap_or(0)
                            })
                            .collect(),
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
                ScriptActorSource::Map(placement) if !matches!(placement.object_type, 2 | 6) => {
                    Some(ActorControlState::default())
                }
                ScriptActorSource::Player | ScriptActorSource::AlternatePlayer => {
                    Some(ActorControlState::default())
                }
                _ => None,
            };
            let separate = matches!(&source, ScriptActorSource::Map(placement) if matches!(placement.object_type, 2 | 6));
            let light = match &source {
                ScriptActorSource::Map(placement) if placement.object_type == 6 => {
                    Some(light::LightState::authored(placement))
                }
                // A type-four object gets the same record with the placement's intensity and
                // radius; its colour is never initialised, so it stays zero.
                ScriptActorSource::Map(placement) if placement.object_type == 4 => {
                    Some(light::LightState {
                        rgb: [0; 3],
                        ..light::LightState::authored(placement)
                    })
                }
                _ => None,
            };
            ScriptActor {
                light,
                flat: None,
                message_read: 0,
                surface: (control.is_some()
                    || matches!(
                        &source,
                        ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
                    ))
                .then_some(0),
                source,
                control,
                runtime_identifier,
                namespace: metadata.and_then(|actor| actor.namespace.clone()),
                native_hook: metadata.and_then(|actor| actor.native_hook.clone()),
                combat_taunts: metadata
                    .map(|actor| actor.combat_taunts.clone())
                    .unwrap_or_default(),
                runtime,
                unavailable,
                location_offsets: metadata
                    .map(|actor| actor.location_offsets.clone())
                    .unwrap_or_default(),
                tasks: Default::default(),
                native_commands: Default::default(),
                animation: None,
                model: Default::default(),
                collision: None,
                player_movement: None,
                animation_failure: None,
                update_failure: None,
                attributes: (0..256)
                    .map(|index| {
                        (!separate)
                            .then_some(metadata)
                            .flatten()
                            .and_then(|actor| {
                                let key = scripts
                                    .definition
                                    .attributes
                                    .get(index)
                                    .filter(|name| !name.is_empty())
                                    .cloned()
                                    .unwrap_or_else(|| format!("attr_{index}"));
                                actor.attributes.get(&key).copied()
                            })
                            .unwrap_or(0)
                    })
                    .collect(),
            }
        };
        let mut counter = scripts.actor_pool_capacity.wrapping_add(3);
        let mut actors = Vec::new();
        for (index, placement) in scripts.instances.iter().enumerate() {
            let identifier = if matches!(placement.object_type, 2 | 6) {
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
                    actors[player.index].attributes[105],
                )
                .with_context(|| format!("world {} camera initialization", scripts.world))
            })
            .transpose()?;
        Ok(Self {
            map_updates: Vec::new(),
            updates: scripts.updates,
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
            texture_frames: scripts.texture_frames.clone(),
            non_engaged_spacing: scripts.non_engaged_spacing,
            combat_clank_effects: scripts.combat_clank_effects,
            node_timer: scripts.node_timer,
            attribute_names: scripts.definition.attributes.clone(),
            reference_names: scripts.definition.references.clone(),
            names,
            actor_lists: None,
            markers: None,
            walk_nodes: None,
            next_combat_index: 0,
            ordinary_prepared: false,
            retained_candidates: Default::default(),
            surface_veto: None,
            camera,
            camera_glide: scripts.camera.glide.into(),
            camera_limits: scripts.camera.limits,
            camera_settings: scripts.camera.clone(),
            standing_height: None,
            gxa_overlay: None,
            pending_player_placement: None,
            effects: None,
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

    /// Consume movement registrations before regional selection, in first-registration order.
    /// View coordinates use Q8 units. The caller supplies the original VM map-exclusion state.
    /// Excluded passes consume registrations without changing cell membership.
    pub fn flush_map_updates(&mut self, view: [i32; 3], excluded: bool) -> Result<()> {
        if self.map_updates.is_empty() {
            return Ok(());
        }
        if self.actor_lists.is_none() {
            self.actor_lists = Some(super::order::ActorLists::new(&self.actors)?);
        }
        let pending = std::mem::take(&mut self.map_updates);
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
    /// attribute-113 actors on its outer border. The selection is retained until
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
            .death_selector
            == 8)
    }

    fn query_position(&self, id: ScriptActorId) -> Result<[i32; 3]> {
        let index = self.resolve(id)?;
        let actor = self.spatial(id)?;
        if self.actors[index].attributes[26] != 0 {
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
        trigger: u8,
        random: &mut crate::random::EngineRandom,
    ) -> Result<Option<&CombatTaunt>> {
        let index = self.resolve(id)?;
        anyhow::ensure!(
            matches!(trigger, 0 | 2 | 6 | 10 | 11),
            "world {} instance {index}: combat taunt type {trigger} requires timed selection",
            self.world
        );
        Ok(select_combat_taunt(&self.actors[index], trigger, random))
    }

    /// Read stored actor surface state; unsupported object paths have no established value.
    pub fn actor_surface(&self, id: ScriptActorId) -> Result<u8> {
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
    pub fn set_actor_surface(&mut self, id: ScriptActorId, surface: u8) -> Result<()> {
        let index = self.resolve(id)?;
        self.actors[index].surface = Some(surface);
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
        default: u16,
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
        let contact_profile = if self.actors[index].attributes[14] != 0 {
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
            placement.object_type == 1,
            "world {} instance {index}: animation initialization requires map object type 1",
            self.world
        );
        anyhow::ensure!(
            data.actor(actor.name())?["model_load_path"].as_str() == Some("animation_table"),
            "world {} instance {index}: actor does not use an animation model table",
            self.world
        );
        let result = cyrus_world::animation::ActorAnimation::initialize(
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
        );
        match result {
            Ok((playback, frame)) => {
                actor
                    .control
                    .as_mut()
                    .expect("type-one map actor has allocation state")
                    .combat_death_default = (actor.attributes[14] != 0).then_some(0);
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
        let group = if control.death_selector == 8 {
            if let Some(default) = control.combat_death_default {
                default as u16
            } else if animation.playback.has_group(123) {
                123
            } else {
                124
            }
        } else if control.rope_attached {
            79
        } else if control.living_state == 15 {
            7
        } else {
            let (holding, drawing, sheathing) = super::activation::weapon_predicates(
                control.living_state,
                Some(super::ActivationAnimation {
                    group: animation.playback.current_group(),
                }),
                control.combat_death_default.is_some(),
                control.weapon_drawn,
                actor.attributes[110],
                item_system_ready,
                false,
            );
            if sheathing {
                u16::from(actor.attributes[44])
            } else if drawing || holding {
                8
            } else if control.torch_burning != 0 {
                5
            } else {
                u16::from(actor.attributes[44])
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
        let contact_profile = if self.actors[index].attributes[14] != 0 {
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
            let mut health = super::SelectedHealthState::initialized(actor.attributes[3]);
            health.retain_death_position(captured);
            self.globals.selected_health = Some(health);
        }
        control.combat_death_default = (actor.attributes[14] != 0).then_some(0);
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
            matches!(self.actors[index].source, ScriptActorSource::Player),
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
    pub fn advance(
        &mut self,
        id: ScriptActorId,
        stage: ScriptStage,
        inputs: ScriptInputs<'_>,
        sounds: Option<&mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
        random: Option<&mut crate::random::EngineRandom>,
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
                        .is_some_and(|placement| matches!(placement.object_type, 2 | 6)))
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
                    self.actors[selected].attributes[105],
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
            matches!(self.actors[player].source, ScriptActorSource::Player),
            "activation selected-player switching unsupported"
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
        self.activation.overlay(
            inputs,
            &self.actors[inputs.player.index],
            ticks,
            cycle_held,
            override_ticks,
            self.globals.lockout(),
        )
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
        Ok(actor.index)
    }
    /// Bounded diagnostics carry logical state, never installed bytecode.
    pub fn inspect(&self, offset: usize, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "world": self.world, "generation": self.generation, "content_id": self.content_id, "total": self.actors.len(),
            "activation": self.activation, "globals": self.globals,
            "actors": self.actors.iter().enumerate().skip(offset).take(limit.min(64)).map(|(index, actor)| serde_json::json!({
                "id": ScriptActorId { generation: self.generation, index }, "name": actor.name(),
                "source": actor.source.label(), "runtime_identifier": actor.runtime_identifier,
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
                "tasks": actor.runtime.as_ref().map(|runtime| runtime.active_tasks()),
                "failure": actor.failure(),
            })).collect::<Vec<_>>()
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
        actors[player].attributes[117] = u8::from(lockout == 1);
    }
    if lockout != 1 {
        for actor in actors
            .iter_mut()
            .filter(|actor| actor.runtime_identifier.is_some())
        {
            actor.attributes[117] = 0;
        }
        activation.clear_prompt_override();
    }
}

impl ScriptActors {
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

    /// Apply New Game's lockout cleanup and combat-queue reset before world replacement.
    /// Ordinary world reloads retain globals and must not call this operation.
    pub fn begin_new_game(&mut self) {
        self.world_loop_outcome = None;
        self.retained_flags = None;
        if let Some(world) = self.current.as_mut() {
            let player = world.selected_player_index();
            let lockout = world.globals.lockout_player(0);
            apply_player_lockout(&mut world.actors, &mut world.activation, player, lockout);
            world.globals.combat_messages.reset();
        } else {
            self.globals.lockout_player(0);
            self.globals.combat_messages.reset();
        }
        self.discard_current_on_release = self.current.is_some();
        self.visited.clear();
    }

    fn release_world(&mut self) {
        self.restored_generation = None;
        if let Some(mut world) = self.current.take() {
            self.globals = std::mem::take(&mut world.globals);
            if !self.discard_current_on_release {
                self.retained_flags = Some(world.snapshot_flags());
                // An unfinished revisit must not overwrite its retained actor state.
                self.visited.entry(world.world).or_insert(world);
            }
            self.discard_current_on_release = false;
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
        actors.release_world();
        actors.error = None;
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
        world.prepare_markers(animation_data)?;
        world.prepare_navigation(animation_data)?;
        if clock.is_none() {
            commands.insert_resource(crate::clock::SimulationClock::new(scripts.clock)?);
        }
        Ok(world)
    })();
    match result {
        Ok(mut world) => {
            actors.retained_flags = None;
            world.globals = std::mem::take(&mut actors.globals);
            world.globals.sound_maintenance.enter_world();
            world.globals.world_loop_frame = 0;
            world.globals.group_sync = Default::default();
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
    native_source_program: Option<std::sync::Arc<cyrus_script::program::Program>>,
    registry: &'a mut ScriptWorld,
    world: u32,
    animation_system: bool,
    task_system: bool,
    inputs: ScriptInputs<'input>,
    index: usize,
    generation: u64,
    sounds: Option<&'sound mut dyn FnMut(ScriptActorId, super::ScriptSound) -> Result<()>>,
    random: Option<&'random mut crate::random::EngineRandom>,
    ordinary_actors: Option<&'a [ScriptActorId]>,
    walk_map_count: Option<usize>,
}
impl<'input> GameHost<'_, '_, 'input, '_> {
    fn native_task_call(
        &mut self,
        (function, mode): (&str, super::tasks::NativeTaskMode),
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<Option<CallResult>> {
        let kind = mode.kind();
        if matches!(mode, super::tasks::NativeTaskMode::Script(_))
            && super::tasks::NativeTasks::is_lineup_function(function)
        {
            return self.lineup_call(function, kind, arguments, invocation);
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
        let operation =
            if kind != CallKind::Function && arguments.len() == 1 && arguments[0].integer().is_ok()
            {
                super::tasks::LocomotionOperation::named(function)
            } else {
                None
            };
        let locomotion_admission = self.locomotion_probes(operation)?;
        let locomotion_state = self.locomotion_state();
        let player_face_requested = function.eq_ignore_ascii_case("PlayerFaceObj");
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
        let (actor, player_actor) = match player_slot.and_then(|player| {
            self.registry
                .actors
                .get_disjoint_mut([self.index, player.index])
                .ok()
        }) {
            Some([actor, player]) => (actor, Some(player)),
            None => (&mut self.registry.actors[self.index], None),
        };
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
                    slave: actor.attributes[26] != 0,
                    actor_index: self.index,
                    map_updates: &mut self.registry.map_updates,
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    turn: self.registry.globals.turn.delta(frame_scale),
                    nominal_ticks,
                },
            );
        let menu_input = menu_input(&self.inputs, self.registry.globals.dialogue.as_ref());
        let services = super::tasks::NativeServices {
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
                channel: actor.attributes[super::group_sync::CHANNEL_ATTRIBUTE],
            }),
            frame_wait: Some(frame_wait),
            locomotion: Some(super::tasks::LocomotionCall {
                state: locomotion_state,
                admission: locomotion_admission,
                animation_system: self.animation_system,
                motion: actor
                    .control
                    .as_mut()
                    .map(|control| super::tasks::MotionCall {
                        direction: &mut control.motion_direction,
                        counters: &mut control.movement_flags,
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
            point: None,
            gxa_overlay: Some(&mut self.registry.gxa_overlay),
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
            super::tasks::NativeTaskMode::SynchronousMovement => {
                actor.tasks.call_synchronous_movement(
                    function,
                    arguments,
                    invocation,
                    &mut animation,
                    &mut spatial_call,
                    services,
                )
            }
        };
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
                || combat.attacker_selection_state != 1
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
            .actor_weapon_predicates(index, "IsHoldingWeapon")
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
        if attributes.len() < 0x35 {
            return Err(format!(
                "camera target actor {index}: attribute block too short"
            ));
        }
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
            self.actor_weapon_predicates(index, "IsHoldingWeapon")
                .map(|(holding, _, _)| holding)
                .map_err(describe)
        } else {
            Ok(false)
        };
        Ok(super::tasks::CameraTarget {
            actor: id,
            camera_move: attributes[0x2d],
            pan_type: attributes[0x2e],
            offsets: std::array::from_fn(|byte| attributes[0x2f + byte]),
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
            .actor_weapon_predicates(index, "IsHoldingWeapon")
            .map_err(describe)?;
        let combat_mode = control
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags & 4 != 0);
        Ok(super::tasks::PlayerFaceState {
            position: pose.position,
            angles: pose.angles,
            speed: super::alignment::speed(
                animation.playback.current_group(),
                Some(&actor.attributes[..]),
            ),
            rate_attribute: actor.attributes[if holding_weapon || combat_mode {
                21
            } else {
                15
            }],
            holding_weapon,
            torch: control.torch_burning != 0,
            response_present: control.body_response_flags != 0
                || control.collision_transition_flags != 0
                || control.collision_response_auxiliary != [0, 0],
            response_age: control.collision_response_age,
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
            refresh = spatial.local.angles != angles;
            spatial.local.angles = angles;
        }
        if let Some(position) = outcome.position {
            spatial.local.position = position;
            if !self.registry.map_updates.contains(&index) {
                self.registry.map_updates.push(index);
            }
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
    fn sheath_sword(&mut self, function: &str, id: ScriptActorId) -> Result<()> {
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
            .receiver_weapon_predicates("MoveBackward")
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
                .is_some_and(|combat| combat.flags & 4 != 0),
            surface: actor.surface.unwrap_or(0),
            torch_burning: control.torch_burning != 0,
            step_adjustment: actor.attributes[11],
            step_attribute: actor.attributes[9],
            combat_step_attribute: actor.attributes[18],
            strafe_step_attribute: actor.attributes[83],
            strafe_turn_attribute: actor.attributes[84],
            lateral_step_attribute: actor.attributes[10],
            forward_step_attribute: actor.attributes[8],
            forward_combat_step_attribute: actor.attributes[17],
            walk_step_attribute: actor.attributes[12],
            walk_combat_step_attribute: actor.attributes[19],
            walk_backward_step_attribute: actor.attributes[13],
            walk_backward_combat_step_attribute: actor.attributes[20],
            move_selected: control
                .combat
                .as_ref()
                .is_some_and(|combat| combat.action != -1),
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
        let walking = matches!(operation, LocomotionOperation::WalkForward)
            && !matches!(state.surface, 2 | 4);
        admission.walk_forward_mode = walking;
        let walking_backward = matches!(operation, LocomotionOperation::WalkBackward)
            && !matches!(state.surface, 2 | 4);
        admission.walk_backward_mode = walking_backward;
        admission.forward_admitted = Some(super::locomotion::forward_admitted(&state));
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
                .is_some_and(|control| control.death_selector == 7)
                && actor.attributes[1] & 1 == 0
            {
                admission.forward_step_clear = Some(true);
                return Ok(admission);
            }
        }
        let Some(geometry) = self.inputs.locomotion_geometry else {
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
                        self.registry
                            .player_step_clear(id, geometry.ground, false)?
                    } else {
                        self.registry.ordinary_step_clear(
                            id,
                            hazard::OrdinaryStepDirection::Backward,
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
                    super::locomotion::StrafeAdmission::OutsideCombat { .. }
                        | super::locomotion::StrafeAdmission::DeepWater { .. }
                ) {
                    let direction = match strafe {
                        super::locomotion::Strafe::Left => hazard::OrdinaryStepDirection::Left,
                        super::locomotion::Strafe::Right => hazard::OrdinaryStepDirection::Right,
                    };
                    let mut missing_sound =
                        |_, _| anyhow::bail!("strafe collision sound handler unavailable");
                    let sounds = self.sounds.as_deref_mut().unwrap_or(&mut missing_sound);
                    admission.lateral_step_clear = Some(self.registry.ordinary_step_clear(
                        id,
                        direction,
                        geometry.ground,
                        geometry.environment,
                        sounds,
                    )?);
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
            LocomotionOperation::Forward | LocomotionOperation::WalkForward => {
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
                            self.registry.player_step_clear(id, geometry.ground, true)?
                        } else {
                            self.registry.walk_forward_step_clear(
                                id,
                                geometry.ground,
                                geometry.environment,
                                sounds,
                            )?
                        });
                    } else {
                        if super::locomotion::forward_plan(&state)
                            == super::locomotion::ForwardPlan::NeedsEdgeProbe
                        {
                            admission.edge_probe_clear = Some(self.registry.edge_probe_clear(
                                id,
                                super::locomotion::FORWARD_MOTION_DIRECTION,
                                geometry.ground.scene,
                            )?);
                        }
                        if (control.movement_flags >> 4) & 3 < 3 {
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
            actors[player.index].attributes[23] = weapon;
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
                None => anyhow::bail!("world actor {}: animation sound handler absent", id.index),
            }
        };
        let animation = self.registry.actors[self.index]
            .animation
            .as_mut()
            .context("actor animation not prepared")?;
        let frame = animation.playback.advance_with_sounds(ticks, &mut emit)?;
        animation.frame = frame.or(animation.frame);
        let frame = animation.playback.request_with_sounds(
            animation.playback.default_group(),
            0,
            ticks,
            &mut emit,
        )?;
        animation.frame = frame.or(animation.frame);
        Ok(())
    }

    /// Receiver weapon predicates as (holding, drawing, sheathing); all false while the
    /// inventory or the receiver's animation is not ready.
    /// `IsCombatCapable`, and `PlayerArmed` when `armed`, which also needs the drawn weapon
    /// not to be sheathing. The original's transition lock and world-active globals are
    /// **Inferred** clear and set respectively; cyrus has no transition state yet.
    fn combat_capable(&self, index: usize, armed: bool) -> Result<bool> {
        let actor = &self.registry.actors[index];
        if actor.attributes[110] != 0 {
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
        let sheathing =
            control.living_state == 21 || animation.playback.current_group() == Some(129);
        Ok(control.weapon_drawn && !sheathing)
    }

    fn receiver_weapon_predicates(&self, function: &str) -> Result<(bool, bool, bool)> {
        self.actor_weapon_predicates(self.index, function)
    }

    fn actor_weapon_predicates(&self, index: usize, function: &str) -> Result<(bool, bool, bool)> {
        let actor = &self.registry.actors[index];
        if function.eq_ignore_ascii_case("IsHoldingWeapon") && actor.attributes[110] != 0 {
            return Ok((true, false, false));
        }
        let ready = self
            .inputs
            .hud
            .with_context(|| format!("{function}: inventory owner unavailable"))?
            .inventory_ready()?;
        if !ready || actor.animation.is_none() {
            return Ok((false, false, false));
        }
        let shell = self
            .inputs
            .shell_execution
            .with_context(|| format!("{function}: shell context unavailable"))?;
        let control = actor
            .control
            .as_ref()
            .with_context(|| format!("{function}: receiver control unavailable"))?;
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
            actor.attributes[110],
            ready,
            shell,
        ))
    }

    fn property_name(&self, target: &Target, reference: u16) -> Result<&str> {
        self.registry
            .reference_names
            .get(usize::from(reference))
            .map(String::as_str)
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

    fn target_index(&self, target: &Target) -> Result<usize> {
        let index = match target {
            Target::Me => return Ok(self.index),
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
            // Object encoding 0x0A names an actor through a local variable.  The
            // variable stores the actor's signed 16-bit runtime identifier; the
            // registry already owns the authoritative identifier-to-slot map.
            Target::Variable(value) => i16::try_from(*value)
                .ok()
                .and_then(|identifier| self.registry.names.get(&identifier).copied()),
        };
        index.with_context(|| format!("object target {target:?}: receiver absent"))
    }
}
enum PoseProperty {
    Position(usize),
    Angle(usize),
}

/// Script properties that name the actor bytes +0x1b0 through +0x1b4, in order: the companion
/// motion, sliding, falling, queued-motion and living-state bytes. Inferred from the property
/// table's byte offsets; the names (`jump_flag` for the falling byte) do not describe the bytes.
fn motion_property<'a>(control: &'a mut ActorControlState, name: &str) -> Option<&'a mut u8> {
    Some(match name.to_ascii_lowercase().as_str() {
        "slide_flag" => &mut control.motion_companion,
        "fall_flag" => &mut control.sliding_state,
        "jump_flag" => &mut control.falling_state,
        "extra_flag" => &mut control.queued_motion,
        "extra_time" => &mut control.living_state,
        _ => return None,
    })
}

fn pose_property(name: &str) -> Option<PoseProperty> {
    let find = |names: [&str; 3]| names.iter().position(|n| name.eq_ignore_ascii_case(n));
    find(["pos.vx", "pos.vy", "pos.vz"])
        .map(PoseProperty::Position)
        .or_else(|| find(["angle.vx", "angle.vy", "angle.vz"]).map(PoseProperty::Angle))
}

impl Host for GameHost<'_, '_, '_, '_> {
    fn establish_native_source(
        &mut self,
        _: cyrus_script::CodeOrigin,
        program: std::sync::Arc<cyrus_script::program::Program>,
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

    fn read_property(&mut self, target: &Target, reference: u16) -> Result<i32> {
        let name = self.property_name(target, reference)?;
        if name.eq_ignore_ascii_case("anim.current") {
            let index = self.property_receiver(target)?;
            let animation = self.registry.actors[index]
                .animation
                .as_ref()
                .context("anim.current animation owner unavailable")?;
            return Ok(animation
                .playback
                .current_group()
                .map_or(-1, |group| i32::from(group as i16)));
        }
        if name.eq_ignore_ascii_case("torch_timer") {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("torch_timer control owner unavailable")?;
            return Ok(i32::from(control.torch_burning));
        }
        if motion_property(&mut ActorControlState::default(), name).is_some() {
            let name = name.to_owned();
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(i32::from(
                *motion_property(control, &name).expect("named motion byte"),
            ));
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
            PoseProperty::Angle(axis) => i32::from(spatial.local.angles[axis]),
        })
    }

    fn write_property(&mut self, target: &Target, reference: u16, value: i32) -> Result<()> {
        let name = self.property_name(target, reference)?;
        if name.eq_ignore_ascii_case("torch_timer") {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .context("torch_timer control owner unavailable")?;
            control.torch_burning = value as u8;
            return Ok(());
        }
        if motion_property(&mut ActorControlState::default(), name).is_some() {
            let name = name.to_owned();
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            *motion_property(control, &name).expect("named motion byte") = value as u8;
            return Ok(());
        }
        let name = name.to_owned();
        let pose = pose_property(&name)
            .with_context(|| format!("property {target:?}.{name} write unsupported"))?;
        let index = self.property_receiver(target)?;
        let local = &mut self
            .registry
            .spatial
            .as_mut()
            .with_context(|| format!("property {target:?}.{reference}: spatial owner unavailable"))?
            .actor_mut(index)?
            .local;
        match pose {
            PoseProperty::Position(axis) => local.position[axis] = value,
            // The local angle field is stored wrapped; a wider assigned value has no owner yet.
            PoseProperty::Angle(axis) => {
                local.angles[axis] = u16::try_from(value)
                    .ok()
                    .filter(|angle| *angle < 2048)
                    .with_context(|| {
                        format!(
                            "property {target:?}.{name}: angle {value} outside 0..2048 unsupported"
                        )
                    })?;
            }
        }
        Ok(())
    }

    fn prepare_native_command(&mut self, site: cyrus_script::NativeCallSite) {
        self.registry.globals.native_command.prepare(site);
    }
    fn load_native_command(&mut self, slot: usize) {
        self.registry.globals.native_command =
            self.registry.actors[self.index].native_commands[slot];
        self.registry.globals.native_command_return = 0;
    }
    fn save_native_command(&mut self, slot: usize, script_return: &mut u8) {
        self.registry.actors[self.index].native_commands[slot] =
            self.registry.globals.native_command;
        *script_return = self.registry.globals.native_command_return;
    }
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        flags: &mut [i32],
    ) -> Result<CallResult> {
        anyhow::ensure!(
            target == &Target::Me,
            "inspection target {target:?} unsupported"
        );
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        // The original's function table keeps only each handler, not the declared kind, so a task
        // or multitask call of a function native runs the handler once and a zero return completes
        // the task at once. Only natives whose handler is shown to return zero take this form.
        if kind != CallKind::Function
            && ["FlatAnimate", "FlatSound"]
                .iter()
                .any(|name| function.eq_ignore_ascii_case(name))
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
        if function.eq_ignore_ascii_case("PlayerLineUp") {
            // Unknown until the native stores this invocation's return value.
            self.registry.globals.lineup_result = None;
        }
        if kind == CallKind::Function
            && [
                "Light",
                "LightRadius",
                "LightIntensity",
                "LightOffset",
                "LightFlicker",
                "FlickerLight",
                "LightFlickerOff",
                "LightSize",
                "LightSizeOff",
            ]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let name = function.to_ascii_lowercase();
            let expected = match name.as_str() {
                "light" | "lightflicker" | "lightsize" => 2,
                "lightoffset" => 3,
                "lightflickeroff" | "lightsizeoff" => 0,
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
            match name.as_str() {
                "light" => {
                    light.enabled = true;
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.starting_intensity = values[1] as i16;
                    light.intensity = values[1] as i16;
                }
                "lightradius" => {
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                "lightintensity" => {
                    light.starting_intensity = values[0] as i16;
                    light.intensity = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                "lightoffset" => {
                    light.offset = [values[0] as i16, values[1] as i16, values[2] as i16]
                }
                "lightflicker" => light.flicker = [values[0] as u16, 0, values[1] as u16],
                "lightsize" => light.size = [values[0] as u16, 0, values[1] as u16],
                "lightsizeoff" => light.size[0] = 0,
                "flickerlight" => light.flicker = [values[0] as u16, 0, 1],
                _ => light.flicker[0] = 0,
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && [
                "Flat",
                "FlatSetTexture",
                "FlatOff",
                "FlatOffset",
                "FlatAnimate",
                "FlatStop",
                "FlatLikeStatic",
            ]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
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
            && ["menuNew", "menuAddItem", "menuSelection"]
                .iter()
                .any(|name| function.eq_ignore_ascii_case(name))
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let name = function.to_ascii_lowercase();
            let expected = if name == "menuadditem" { 3 } else { 0 };
            anyhow::ensure!(values.len() == expected, "{function}: wrong argument count");
            let globals = &mut self.registry.globals;
            match name.as_str() {
                "menunew" => globals.menu.clear(),
                "menuselection" => return Ok(CallResult::Complete(globals.menu.selection())),
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("LightOff") {
            anyhow::ensure!(arguments.is_empty(), "LightOff requires no arguments");
            let actor = &mut self.registry.actors[self.index];
            if let Some(light) = &mut actor.light {
                light.locked = false;
                light.enabled = false;
                light.inactive_ticks = 0;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("ResetGroupSync") {
            anyhow::ensure!(arguments.is_empty(), "ResetGroupSync requires no arguments");
            // The native clears this actor's posted value.  The channel update retains the
            // released byte; the next world-loop update observes this member as unposted.
            self.registry.globals.group_sync.reset(self.index);
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("beginCombat") {
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
            && (function.eq_ignore_ascii_case("ShowItem")
                || function.eq_ignore_ascii_case("ShowItemNoRtx"))
        {
            // ShowItem(item, text, first, second) ignores its text argument here; ShowItemNoRtx
            // has no text argument. Both leave the viewer request pending.
            let rtx = function.eq_ignore_ascii_case("ShowItem");
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("AddItem") {
            let [item, amount] = arguments else {
                anyhow::bail!("AddItem requires two arguments")
            };
            let item =
                u16::try_from(item.integer()?).context("AddItem item ID outside u16 range")?;
            self.add_session_item(item, amount.integer()?)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function
            && (function.eq_ignore_ascii_case("LightTorch")
                || function.eq_ignore_ascii_case("UnlightTorch"))
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
            if function.eq_ignore_ascii_case("LightTorch") {
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
            && (function.eq_ignore_ascii_case("shoot")
                || function.eq_ignore_ascii_case("shootPlayer"))
        {
            let player_form = function.eq_ignore_ascii_case("shootPlayer");
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
                local.angles[0].wrapping_neg() & 2047,
                local.angles[1],
                local.angles[2].wrapping_neg() & 2047,
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
                let elevation = i32::from(
                    super::spatial::query_bearing(horizontal, unit[1]).wrapping_neg() & 2047,
                );
                [
                    (elevation.wrapping_sub(0x200) & 0x7ff) as u16,
                    super::spatial::query_bearing(difference[0], difference[2]),
                    0,
                ]
            } else {
                [
                    (arguments[4].integer()? & 2047) as u16,
                    (arguments[5].integer()? & 2047) as u16,
                    (arguments[6].integer()? & 2047) as u16,
                ]
            };
            self.registry.create_object(id, name, position, angles)?;
            return Ok(CallResult::Complete(0));
        }
        // GolemSteam: once every few updates, a puff of steam at one of the receiver's four weapon
        // attachment vertices (a grip or a scabbard point), in the alternating effect variants.
        if kind == CallKind::Function && function.eq_ignore_ascii_case("GolemSteam") {
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
            let rotated = self.attachment_offset("GolemSteam", field)?;
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("DragonBreath") {
            anyhow::ensure!(arguments.is_empty(), "DragonBreath requires no arguments");
            return Ok(CallResult::Complete(i32::from(self.breathe()?)));
        }
        // AddFlatEffect(x, y, z, spread, descriptor, speed) builds a type-one environmental effect
        // at a Q8 point. Only the installed form is supported: zero spread and speed one.
        if kind == CallKind::Function && function.eq_ignore_ascii_case("AddFlatEffect") {
            let [x, y, z, spread, descriptor, speed] = arguments else {
                anyhow::bail!("AddFlatEffect requires six arguments")
            };
            anyhow::ensure!(
                spread.integer()? == 0 && speed.integer()? == 1,
                "AddFlatEffect: nonzero spread or a speed other than one unsupported"
            );
            self.registry.spawn_environment_effect(
                descriptor.integer()? as u32,
                [x.integer()?, y.integer()?, z.integer()?],
            )?;
            return Ok(CallResult::Complete(0));
        }
        // OpenShell(name) starts the named shell script; it runs from the next update.
        if kind == CallKind::Function && function.eq_ignore_ascii_case("OpenShell") {
            let [Argument::String(name)] = arguments else {
                anyhow::bail!("OpenShell requires one script name")
            };
            self.registry.spawn_shell(name)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("UseItem") {
            let [item] = arguments else {
                anyhow::bail!("UseItem requires one argument")
            };
            let item =
                u16::try_from(item.integer()?).context("UseItem item ID outside u16 range")?;
            self.use_item(item)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("RemoveItem") {
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
        // chargeWeapon(1) swaps the player's weapon item 1 for item 75 (the charged weapon) and any
        // other operand swaps back: add the new item, record it as the weapon attribute, empty the
        // old item and select the new one. A closed inventory skips the adds and removals, and the
        // selection then fails as the original's does.
        if kind == CallKind::Function && function.eq_ignore_ascii_case("chargeWeapon") {
            let [charge] = arguments else {
                anyhow::bail!("chargeWeapon requires one argument")
            };
            let (take, drop) = if charge.integer()? as i16 == 1 {
                (75, 1)
            } else {
                (1, 75)
            };
            self.add_session_item(take, 1)?;
            self.registry.actors[self.index].attributes[23] = take as u8;
            let hud = self
                .inputs
                .hud
                .context("chargeWeapon inventory host unavailable")?;
            hud.remove_item(drop)?;
            hud.select_item(take)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("ActiveItem") {
            anyhow::ensure!(arguments.is_empty(), "ActiveItem requires no arguments");
            let selected = self
                .inputs
                .hud
                .context("ActiveItem inventory host unavailable")?
                .active_item()?;
            return Ok(CallResult::Complete(i32::from(selected)));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("HandItem") {
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("LockoutPlayer") {
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
                && (function.eq_ignore_ascii_case("Activate")
                    || function.eq_ignore_ascii_case("TorchActivate"))
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
        if function.eq_ignore_ascii_case("KillMyTasks") {
            anyhow::ensure!(arguments.is_empty(), "KillMyTasks requires no arguments");
            return Ok(CallResult::CompleteAfterClearingTasks(0));
        }
        if function.eq_ignore_ascii_case("ResetRotation") {
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
        if function.eq_ignore_ascii_case("ResetPosition") {
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
        if function.eq_ignore_ascii_case("MovePlayerAboveMe") {
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
            self.registry.pending_player_placement = Some((position, (yaw & 0x7ff) as u16));
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("cameraController") {
            anyhow::ensure!(
                arguments.is_empty(),
                "cameraController requires no arguments"
            );
            let service = self
                .inputs
                .camera_update
                .context("cameraController: camera update service unavailable")?;
            self.registry.run_camera_controller(service)?;
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("XResetCamera") {
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
        if function.eq_ignore_ascii_case("ReloadRotation") {
            anyhow::ensure!(arguments.is_empty(), "ReloadRotation requires no arguments");
            self.registry
                .spatial
                .as_mut()
                .context("ReloadRotation: spatial state not prepared")?
                .actor_mut(self.index)?
                .reset_rotation([false; 3])?;
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("Resurrect") {
            anyhow::ensure!(arguments.is_empty(), "Resurrect requires no arguments");
            self.registry.release_held_walk_nodes(id)?;
            let resurrected = {
                let actor = &mut self.registry.actors[self.index];
                let control = actor
                    .control
                    .as_mut()
                    .context("Resurrect: receiver lifecycle unavailable")?;
                if control.killed || control.death_selector == 8 {
                    control.killed = false;
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
        if function.eq_ignore_ascii_case("KillMe") {
            anyhow::ensure!(arguments.is_empty(), "KillMe requires no arguments");
            self.registry.release_held_walk_nodes(id)?;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .context("KillMe: receiver lifecycle unavailable")?;
            control.killed = true;
            control.death_selector = 8;
            self.registry.actors[self.index].model.current = None;
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id, super::ScriptSound::StopOwned
            )?;
            // The VM cancels the receiver's runtime task slots after this function returns, while
            // its runtime is still borrowed by the invocation.
            return Ok(CallResult::CompleteAfterClearingTasks(0));
        }
        if function.eq_ignore_ascii_case("HideMe") || function.eq_ignore_ascii_case("ShowMe") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| {
                    format!("{function}: receiver visibility lifecycle unavailable")
                })?;
            control.hidden = function.eq_ignore_ascii_case("HideMe");
            if control.hidden {
                self.registry.actors[self.index].model.current = None;
            } else if self.registry.actors[self.index]
                .model
                .static_radius
                .is_some()
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
        if [
            "IsHoldingWeapon",
            "IsDrawingSword",
            "IsSheathingSword",
            "IsDrawingOrSheathing",
        ]
        .iter()
        .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let (holding, drawing, sheathing) = self.receiver_weapon_predicates(function)?;
            let result = if function.eq_ignore_ascii_case("IsHoldingWeapon") {
                holding
            } else if function.eq_ignore_ascii_case("IsDrawingSword") {
                drawing
            } else if function.eq_ignore_ascii_case("IsDrawingOrSheathing") {
                drawing || sheathing
            } else {
                sheathing
            };
            return Ok(CallResult::Complete(i32::from(result)));
        }
        if function.eq_ignore_ascii_case("SetHealth")
            || function.eq_ignore_ascii_case("AdjustHealth")
        {
            let set = function.eq_ignore_ascii_case("SetHealth");
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
            if attributes[120] != 0 {
                return Ok(CallResult::Complete(0));
            }
            if attributes[97] != 0
                && !self.receiver_weapon_predicates("IsHoldingWeapon")?.0
                && self.registry.actors[self.index]
                    .control
                    .as_ref()
                    .and_then(|control| control.combat.as_ref())
                    .is_none_or(|combat| combat.flags & 4 == 0)
            {
                return Ok(CallResult::Complete(0));
            }
            if selected {
                let id = self.registry.id(self.index)?;
                let change = arguments[0]
                    .integer()?
                    .wrapping_sub(i32::from(self.registry.actors[self.index].attributes[3]));
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
            let attributes = &mut self.registry.actors[self.index].attributes;
            let value = arguments[0].integer()?;
            let delta = if set {
                value.wrapping_sub(i32::from(attributes[3]))
            } else {
                value
            };
            let health = i32::from(attributes[3]).wrapping_add(delta);
            let damage = set || arguments[1].integer()? == 1;
            anyhow::ensure!(
                health > 0 || !damage,
                "{function}: death sequence unavailable"
            );
            attributes[3] = health.clamp(0, 255) as u8;
            return Ok(CallResult::Complete(0));
        }
        // PlayerMoved compares the selected player's position with its snapshot taken at the
        // ordinary actor-update boundary.
        if function.eq_ignore_ascii_case("PlayerMoved") {
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
        // PlayerTurned, PlayerTurnedDirection and PlayerMovementSpeed read the selected player; the
        // original also returns zero for an actor without its model handle, assumed present here.
        if [
            "PlayerTurned",
            "PlayerTurnedDirection",
            "PlayerMovementSpeed",
        ]
        .iter()
        .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .with_context(|| format!("{function}: selected player unavailable"))?;
            let index = self.registry.resolve(player)?;
            if function.eq_ignore_ascii_case("PlayerMovementSpeed") {
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
            let value = if function.eq_ignore_ascii_case("PlayerTurned") {
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
        if function.eq_ignore_ascii_case("PlayerOnScape") {
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
                control.ground_kind == super::registry::ground::TERRAIN_KIND,
            )));
        }
        // PlayerHanging: the player is in living state 15 on the ledge the receiver names. The
        // ledge identity belongs to the ledge-entry controller, so state 15 stays unsupported.
        if function.eq_ignore_ascii_case("PlayerHanging") {
            anyhow::ensure!(arguments.is_empty(), "PlayerHanging requires no arguments");
            let player = self
                .selected_player()
                .context("PlayerHanging: selected player unavailable")?;
            let index = self.registry.resolve(player)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("PlayerHanging: selected-player control unavailable")?;
            anyhow::ensure!(
                control.living_state != 15,
                "PlayerHanging: ledge hang identity unavailable"
            );
            return Ok(CallResult::Complete(0));
        }
        // PlayerStatus classifies the selected player; the motion bytes are the inferred
        // actor bytes +0x1b1 through +0x1b3 and the living state +0x1b4.
        if function.eq_ignore_ascii_case("PlayerStatus") {
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
            )));
        }
        // The dialogue and topic-menu mode words. DialogBegin also clears one retained byte whose
        // owner is not yet identified; that write is not modelled.
        if let Some((dialogue, begin)) = [
            ("DialogBegin", true, true),
            ("DialogEnd", true, false),
            ("MenuBegin", false, true),
            ("MenuEnd", false, false),
        ]
        .into_iter()
        .find_map(|(name, dialogue, begin)| {
            function
                .eq_ignore_ascii_case(name)
                .then_some((dialogue, begin))
        }) {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            match (dialogue, begin) {
                (true, begin) => self.registry.globals.set_dialogue_state(begin),
                (false, true) => self.registry.globals.begin_topic_menu(),
                (false, false) => self.registry.globals.end_topic_menu(),
            }
            return Ok(CallResult::Complete(0));
        }
        // IsWithinMap is constant true. LoadWorld, WonGame, PushPlayer and PopPlayer only raise
        // a request that the original's main loop consumes; the host takes it from the globals.
        if function.eq_ignore_ascii_case("IsWithinMap") {
            return Ok(CallResult::Complete(1));
        }
        // MyId is the actor's signed 16-bit runtime identifier (pool index plus three).
        if function.eq_ignore_ascii_case("MyId") {
            return Ok(CallResult::Complete(i32::from(
                self.registry.actors[self.index]
                    .runtime_identifier
                    .context("MyId: runtime identifier unavailable")?,
            )));
        }
        // Animal ignores its arguments and returns zero.
        if function.eq_ignore_ascii_case("Animal") {
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("SpeedScale") {
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("MainloopExit") {
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("updatePlayerViewAttrib") {
            anyhow::ensure!(
                arguments.is_empty(),
                "updatePlayerViewAttrib requires no arguments"
            );
            return Ok(CallResult::CompleteWithoutValue);
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("QueueMovie") {
            let [movie] = arguments else {
                anyhow::bail!("QueueMovie requires one argument")
            };
            self.registry.globals.queue_movie(movie.integer()?);
            return Ok(CallResult::Complete(0));
        }
        if let Some(enabled) = [("EnableNodeMaps", true), ("DisableNodeMaps", false)]
            .into_iter()
            .find_map(|(name, enabled)| function.eq_ignore_ascii_case(name).then_some(enabled))
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            self.registry.set_all_walk_maps_enabled(enabled)?;
            return Ok(CallResult::Complete(0));
        }
        // HoldAnimation, PauseAnimation and ReleaseAnimation write the receiver's animation wait
        // byte: 255 holds, a pause value counts nominal ticks, release clears only a hold.
        if kind == CallKind::Function
            && ["HoldAnimation", "PauseAnimation", "ReleaseAnimation"]
                .iter()
                .any(|name| function.eq_ignore_ascii_case(name))
        {
            let wait = match (arguments, function.to_ascii_lowercase().as_str()) {
                ([], "holdanimation") => Some(u8::MAX),
                ([value], "pauseanimation") => Some(value.integer()? as u8),
                ([], "releaseanimation") => None,
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
        if function.eq_ignore_ascii_case("LoadWorld") {
            let [world, a, b] = arguments else {
                anyhow::bail!("LoadWorld requires three arguments")
            };
            self.registry.globals.request_mut().load_world =
                Some([world.integer()?, a.integer()?, b.integer()?]);
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("WonGame") {
            self.registry.globals.request_mut().won_game = true;
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("PushPlayer") || function.eq_ignore_ascii_case("PopPlayer")
        {
            let request = self.registry.globals.request_mut();
            if function.eq_ignore_ascii_case("PushPlayer") {
                request.push_player = true;
            } else {
                request.pop_player = true;
            }
            return Ok(CallResult::Complete(0));
        }
        // IsMaster and IsSlave test attributes 25 and 26; TrueFunction ignores both arguments.
        if function.eq_ignore_ascii_case("IsMaster") || function.eq_ignore_ascii_case("IsSlave") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let attribute = if function.eq_ignore_ascii_case("IsMaster") {
                25
            } else {
                26
            };
            return Ok(CallResult::Complete(i32::from(
                self.registry.actors[self.index].attributes[attribute] != 0,
            )));
        }
        if function.eq_ignore_ascii_case("TrueFunction") {
            anyhow::ensure!(arguments.len() == 2, "TrueFunction: wrong argument count");
            return Ok(CallResult::Complete(1));
        }
        // SetAIType and SetAIMode store the low byte of their argument in the bytes that
        // MyAIType and MyAIMode read back.
        if function.eq_ignore_ascii_case("SetAIType") || function.eq_ignore_ascii_case("SetAIMode")
        {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()? as u8;
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver control state unavailable"))?;
            if function.eq_ignore_ascii_case("SetAIType") {
                control.death_selector = value;
            } else {
                control.death_response = value;
            }
            return Ok(CallResult::Complete(0));
        }
        // `attr_health` is attribute 3; Strength and Armor read the combat record.
        if [
            "Health",
            "PlayerHealth",
            "Strength",
            "Armor",
            "ReplenishHealth",
        ]
        .iter()
        .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let lower = function.to_ascii_lowercase();
            let index = if lower == "playerhealth" {
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
            let value = match lower.as_str() {
                "health" | "playerhealth" => i32::from(actor.attributes[3]),
                "replenishhealth" => {
                    if actor.attributes[3] >= 100 {
                        0
                    } else {
                        if selected {
                            self.inputs
                                .hud
                                .context("ReplenishHealth selected-player HUD owner unavailable")?
                                .request_replenish_health_feedback();
                        }
                        actor.attributes[3] = 100;
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
                    if lower == "strength" {
                        combat.strength
                    } else {
                        combat.armor
                    }
                }
            };
            return Ok(CallResult::Complete(value));
        }
        if function.eq_ignore_ascii_case("IsCombatCapable")
            || function.eq_ignore_ascii_case("PlayerArmed")
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let armed = function.eq_ignore_ascii_case("PlayerArmed");
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
        if function.eq_ignore_ascii_case("DisableHook")
            || function.eq_ignore_ascii_case("EnableHook")
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let control = self.registry.actors[self.index]
                .control
                .as_mut()
                .with_context(|| format!("{function}: receiver script-control unavailable"))?;
            // Bit 0x02 of the script-control byte suppresses the native hook.
            if function.eq_ignore_ascii_case("DisableHook") {
                control.script_control |= 2;
            } else {
                control.script_control &= !2;
            }
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("SetPotion") || function.eq_ignore_ascii_case("GetPotion")
        {
            let set = function.eq_ignore_ascii_case("SetPotion");
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
        if function.eq_ignore_ascii_case("AdjustStrength")
            || function.eq_ignore_ascii_case("AdjustArmor")
        {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let delta = arguments[0].integer()?;
            let combat = self.registry.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            let value = if function.eq_ignore_ascii_case("AdjustStrength") {
                &mut combat.strength
            } else {
                &mut combat.armor
            };
            *value = value.wrapping_add(delta);
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("setStrength") || function.eq_ignore_ascii_case("setArmor")
        {
            anyhow::ensure!(arguments.len() == 1, "{function}: wrong argument count");
            let value = arguments[0].integer()?;
            let combat = self.registry.actors[self.index]
                .control
                .as_mut()
                .and_then(|control| control.combat.as_mut())
                .with_context(|| format!("{function}: receiver combat record unavailable"))?;
            if function.eq_ignore_ascii_case("setStrength") {
                combat.strength = value;
            } else {
                combat.armor = value;
            }
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("PlayAnimation") {
            let [group, modifier] = arguments else {
                anyhow::bail!("PlayAnimation requires two arguments")
            };
            let group = u16::try_from(group.integer()?)
                .context("PlayAnimation group outside supported range")?;
            anyhow::ensure!(
                group <= i16::MAX as u16,
                "PlayAnimation signed group unsupported"
            );
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
                .request_with_sounds(group, modifier, ticks, &mut emit)?;
            animation.frame = frame.or(animation.frame);
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("GetFramePassed") {
            anyhow::ensure!(arguments.is_empty(), "GetFramePassed requires no arguments");
            let ticks = self
                .inputs
                .nominal_ticks
                .context("GetFramePassed requires nominal_ticks input")?;
            return Ok(CallResult::Complete(
                i32::try_from(ticks).context("GetFramePassed ticks exceed the signed range")?,
            ));
        }
        if function.eq_ignore_ascii_case("InRectangle") {
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
        // DrawSword is the weapon draw admission: it enters state 20 when every gate allows it
        // and otherwise leaves the actor unchanged. Its final repositioning step is disabled in
        // the shipped game.
        if function.eq_ignore_ascii_case("DrawSword") {
            anyhow::ensure!(arguments.is_empty(), "DrawSword requires no arguments");
            self.draw_sword(id)?;
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("SheathSword") {
            anyhow::ensure!(arguments.is_empty(), "SheathSword requires no arguments");
            self.sheath_sword(function, id)?;
            return Ok(CallResult::Complete(0));
        }
        // endCombat runs SheathSword's action, then unconditionally resets the lifecycle selector
        // and mode bytes, re-enables the hook, leaves combat mode and clears the close-distance
        // pass counter and the coordination selection state.
        if function.eq_ignore_ascii_case("endCombat") {
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
        if function.eq_ignore_ascii_case("isFighting") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(self.is_fighting()?)));
        }
        if function.eq_ignore_ascii_case("isDead") {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            return Ok(CallResult::Complete(i32::from(
                self.actor_is_dead(self.index)?,
            )));
        }
        if function.eq_ignore_ascii_case("Rnd") || function.eq_ignore_ascii_case("QuickRnd") {
            let [operand] = arguments else {
                anyhow::bail!("{function} requires one argument")
            };
            let operand = operand.integer()?;
            let random = self
                .random
                .as_deref_mut()
                .with_context(|| format!("{function}: shared engine random stream unavailable"))?;
            let value = if function.eq_ignore_ascii_case("Rnd") {
                random.rnd(operand)?
            } else {
                random.quick_rnd(operand)
            };
            return Ok(CallResult::Complete(value));
        }
        // Enable*/Disable* clear and set one bit of the script-control byte: 0x01 script,
        // 0x04 tasks, 0x08 object, 0x10 combat (0x02 is the hook, handled above).
        let control_bit = [
            ("Script", 0x01),
            ("Tasks", 0x04),
            ("Object", 0x08),
            ("Combat", 0x10),
        ]
        .into_iter()
        .find_map(|(suffix, bit)| {
            let enable = function.eq_ignore_ascii_case(&format!("Enable{suffix}"));
            (enable || function.eq_ignore_ascii_case(&format!("Disable{suffix}")))
                .then_some((enable, bit))
        });
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
        if function.eq_ignore_ascii_case("Sound") {
            let [effect, volume, rate] = arguments else {
                anyhow::bail!("Sound requires three arguments")
            };
            let spatial = self
                .registry
                .spatial
                .as_ref()
                .context("Sound receiver pose unavailable")?
                .actor(self.index)?;
            let position = if spatial.parent.is_some() {
                spatial
                    .effective
                    .context("Sound attached receiver pose unavailable")?
                    .position
            } else {
                spatial.linked_position.unwrap_or(spatial.local.position)
            };
            self.sounds
                .as_mut()
                .context("script sound handler absent")?(
                id,
                super::ScriptSound::Positional {
                    effect: effect.integer()?,
                    volume: volume.integer()?,
                    rate_offset_hz: rate.integer()?,
                    position,
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
                "Wander",
                "MoveByAxis",
                "MoveAlongAxis",
                "WanderToLocation",
                "sMoveByAxis",
                "MoveObjectAxis",
                "MoveToLocation",
                "RotateByAxis",
                "sRotateByAxis",
                "RotateToAxis",
                "Move",
                "Rotate",
                "sMove",
                "sRotate",
                "sMoveAlongAxis",
                "MoveMarker",
                "MoveLocation",
                "MovePosition",
                "MoveNodeMarker",
                "MoveNodeLocation",
                "MoveNodePosition",
            ]
            .iter()
            .any(|name| function.eq_ignore_ascii_case(name))
        {
            anyhow::bail!("{function}: separate movement/relocation owner unavailable");
        }
        if function.eq_ignore_ascii_case("Wander") {
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
            self.registry.actors[self.index].attributes[94] = 1;
            return Ok(CallResult::Complete(result));
        }
        if function.eq_ignore_ascii_case("AtNodeMarker") {
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("Offset") {
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
        if kind == CallKind::Function && function.eq_ignore_ascii_case("UnOffset") {
            anyhow::ensure!(arguments.is_empty(), "UnOffset requires no arguments");
            self.registry.unoffset_actor(self.index)?;
            return Ok(CallResult::Complete(0));
        }
        if kind == CallKind::Function && function.eq_ignore_ascii_case("PlayerOffset") {
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
        if function.eq_ignore_ascii_case("AtNodeLocation") {
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
            let target = self.receiver_location("AtNodeLocation", location.integer()?)?;
            return Ok(CallResult::Complete(i32::from(
                point_tasks::within_location(position, target, total_height, distance.integer()?)?,
            )));
        }
        // PlayerCollide holds while the selected player stands on the receiver or its body touched
        // the receiver; PlayerCollideStatic only reads the contact list.
        if function.eq_ignore_ascii_case("PlayerCollide")
            || function.eq_ignore_ascii_case("PlayerCollideStatic")
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let player = self
                .selected_player()
                .with_context(|| format!("{function} selected player unavailable"))?;
            let receiver = ScriptActorId {
                generation: self.generation,
                index: self.index,
            };
            let contacts = &self.registry.globals.player_contacts;
            let touching = if function.eq_ignore_ascii_case("PlayerCollide") {
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
        if function.eq_ignore_ascii_case("CollidePlayerWeapon") {
            anyhow::ensure!(arguments.len() == 4, "{function} requires four arguments");
            let player = self
                .selected_player()
                .context("CollidePlayerWeapon selected player unavailable")?;
            let player_index = self.registry.resolve(player)?;
            let (holding, _, _) = self.actor_weapon_predicates(player_index, "IsHoldingWeapon")?;
            let actor = &self.registry.actors[player_index];
            let swinging = actor
                .control
                .as_ref()
                .and_then(|control| control.combat.as_ref())
                .is_some_and(|combat| combat.decision.attack_state == 1);
            if !holding || actor.attributes[105] == 1 || !swinging {
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
        if function.eq_ignore_ascii_case("PlayerStand") {
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
        // Surface classes 1..=4 are the water classes, 2 and 4 the deep ones, 5 lava.
        let surface_test: Option<fn(u8) -> bool> = match function.to_ascii_lowercase().as_str() {
            "isindeepwater" => Some(|surface| matches!(surface, 2 | 4)),
            "isinwater" => Some(|surface| (1..=4).contains(&surface)),
            "isinlava" => Some(|surface| surface == 5),
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
        // notification byte; IsOnEdge holds for living states 10 through 19.
        let control_test: Option<fn(&ActorControlState) -> i32> =
            match function.to_ascii_lowercase().as_str() {
                "myaitype" => Some(|control| i32::from(control.death_selector)),
                "myaimode" => Some(|control| i32::from(control.death_response)),
                "isonedge" => Some(|control| i32::from((10..=19).contains(&control.living_state))),
                // The original also answers 0 while its transition lock (global word read by
                // dozens of routines) is set; cyrus has no transition state, so it is clear.
                "isinair" => Some(|control| {
                    i32::from(
                        control.sliding_state != 0
                            || control.falling_state != 0
                            || control.queued_motion != 0
                            || (27..=29).contains(&control.living_state),
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
        if function.eq_ignore_ascii_case("PrintSingleParm") {
            let [value] = arguments else {
                anyhow::bail!("{function} requires one argument");
            };
            let value = value.integer()?;
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry.globals.print_line(format!("{name} {value}"));
            return Ok(CallResult::Complete(0));
        }
        if function.eq_ignore_ascii_case("PlayerInSight") {
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
        if function.eq_ignore_ascii_case("DistanceToLocation") {
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
        if function.eq_ignore_ascii_case("PlayerDistance")
            || function.eq_ignore_ascii_case("AcuratePlayerDistance")
            || function.eq_ignore_ascii_case("PlayerFacing")
            || function.eq_ignore_ascii_case("PlayerLooking")
            || function.eq_ignore_ascii_case("PlayerArc")
            || function.eq_ignore_ascii_case("PlayerArctan")
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
        if function.eq_ignore_ascii_case("SetAttribute")
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
                    .context("SetAttribute 44: dead actor without a combat record unsupported")?;
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
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        let id = ScriptActorId {
            generation: self.generation,
            index: self.index,
        };
        if self.registry.actors[self.index].tasks.is_lineup_task(token) {
            return self.lineup_poll(token, arguments, invocation);
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
        let (actor, player_actor) = match player_slot.and_then(|player| {
            self.registry
                .actors
                .get_disjoint_mut([self.index, player.index])
                .ok()
        }) {
            Some([actor, player]) => (actor, Some(player)),
            None => (&mut self.registry.actors[self.index], None),
        };
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
                    slave: actor.attributes[26] != 0,
                    actor_index: self.index,
                    map_updates: &mut self.registry.map_updates,
                    position: &mut spatial.local.position,
                    angles: &mut spatial.local.angles,
                    frame_scale,
                    turn: self.registry.globals.turn.delta(frame_scale),
                    nominal_ticks,
                },
            );
        let menu_input = menu_input(&self.inputs, self.registry.globals.dialogue.as_ref());
        let result = actor.tasks.poll(
            token,
            arguments,
            invocation,
            &mut animation,
            &mut spatial_call,
            super::tasks::NativeServices {
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
                    channel: actor.attributes[super::group_sync::CHANNEL_ATTRIBUTE],
                }),
                frame_wait: Some(frame_wait),
                locomotion: Some(super::tasks::LocomotionCall {
                    state: locomotion_state,
                    admission: locomotion_admission,
                    animation_system: self.animation_system,
                    motion: actor
                        .control
                        .as_mut()
                        .map(|control| super::tasks::MotionCall {
                            direction: &mut control.motion_direction,
                            counters: &mut control.movement_flags,
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
                point: None,
                gxa_overlay: Some(&mut self.registry.gxa_overlay),
            },
        );
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
        site: cyrus_script::NativeCallSite,
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
        let attributes = &self.registry.actors[self.index].attributes;
        anyhow::ensure!(
            attributes[30] == 0,
            "user synchronization channels unsupported"
        );
        // End clears the actor's pending group synchronization state.
        if attributes[super::group_sync::CHANNEL_ATTRIBUTE] != 0 {
            self.registry.globals.group_sync.reset(self.index);
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
    actors.release_world();
    actors.requested = None;
    actors.error = None;
    *status = super::ScriptStatus::NotRequested;
}

fn select_combat_taunt<'a>(
    actor: &'a ScriptActor,
    trigger: u8,
    random: &mut crate::random::EngineRandom,
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
        if selected.is_some() || requested != 6 {
            return selected;
        }
        requested = 2;
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
                source: ScriptActorSource::Map(Instance {
                    object_id: index as u32,
                    actor: format!("ACTOR{index}"),
                    object_type: 0,
                    active: 0,
                    model_name: String::new(),
                    model_mode: 0,
                    light_intensity: 0,
                    light_radius: 0,
                    light_rgb: [0; 3],
                    map_position: [0; 3],
                    angles: [0; 3],
                }),
                runtime_identifier: None,
                namespace: None,
                native_hook: None,
                combat_taunts: vec![],
                runtime: None,
                unavailable: None,
                attributes: vec![0; 256],
                control: Some(ActorControlState {
                    combat: (index != 0).then_some(super::super::CombatControlState {
                        target: Some(receiver),
                        attacker_selection_state: state,
                        ..Default::default()
                    }),
                    death_selector: if dead { 8 } else { 0 },
                    ..Default::default()
                }),
                surface: Some(0),
                model: Default::default(),
                collision: None,
                player_movement: None,
                location_offsets: vec![],
                tasks: Default::default(),
                native_commands: Default::default(),
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
        let definition = cyrus_script::program::Definition {
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
                fall_death_ticks: 144,
                fall_landing: None,
                dummy_model_manager: false,
                task_system: true,
                animation_system: true,
                disable_slaves: false,
                disable_master_slaves: false,
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
            texture_frames: Default::default(),
            non_engaged_spacing: None,
            combat_clank_effects: [None; 2],
            node_timer: None,
            attribute_names: Vec::new(),
            reference_names: definition.references,
            names: BTreeMap::new(),
            actor_lists: None,
            markers: None,
            walk_nodes: None,
            next_combat_index: 0,
            ordinary_prepared: false,
            retained_candidates: Default::default(),
            model_admission: None,
            map_updates: Vec::new(),
            surface_veto: None,
            camera: None,
            camera_glide: Default::default(),
            camera_limits: Default::default(),
            camera_settings: Default::default(),
            standing_height: None,
            gxa_overlay: None,
            pending_player_placement: None,
            effects: None,
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
            source,
            runtime_identifier: None,
            namespace: None,
            native_hook: None,
            combat_taunts: vec![],
            runtime: None,
            unavailable: None,
            attributes: vec![0; 256],
            control: Some(ActorControlState::default()),
            surface: Some(0),
            model: Default::default(),
            collision: None,
            player_movement: None,
            location_offsets: vec![],
            tasks: Default::default(),
            native_commands: Default::default(),
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
            object_type: 0,
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            light_intensity: 0,
            light_radius: 0,
            light_rgb: [0; 3],
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
                object_type: 0,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
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
}
