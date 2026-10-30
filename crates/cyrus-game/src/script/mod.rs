//! Game-owned SOUP hosts and supported camera-centered ordinary actor updates.
//!
//! [`ScriptWorld`] retains logical actors; [`ScriptActors`] owns the session and
//! active world. [`ScriptInputs`] lends host state and services to each call.
//! Explicit operations do not establish automatic actor admission. [`run_ordinary`]
//! composes supported stages using a [`ScriptView`] and the shared clock/random
//! owners; required unavailable services fail with world/actor context.

//!
//! # Ownership and integration
//!
//! [`ScriptWorld`] owns generation-checked logical actor identities, authored actor
//! order, mutable script/control state, spatial relationships, native task slots
//! and walk-node reservations. [`ScriptActors`] owns the active session and world.
//! [`ScriptBinding`] and [`ScriptScene`] connect logical state to presentation.
//! [`ScriptStatus`] and retained [`PlaybackDiagnostics`] expose preparation and runtime
//! failures separately.
//!
//! [`ScriptInputs`] lends timing, selected-player state, geometry and host services
//! to explicit operations. [`run_ordinary`] composes the supported automatic stages
//! using [`ScriptView`] and shared session owners. Calling an explicit operation
//! does not establish automatic scheduler admission or complete gameplay support.
//! Missing required ownership and unsupported branches return contextual errors.
//!
//! Animation actor events execute synchronously inside each world-owned playback
//! callback, using the shared camera and environmental-effect pool. The audio host
//! receives sounds and transition decisions. Earlier event effects remain applied
//! if a later command or world phase fails; ordinary traversal never batches events.
//!
//! [`ModelPreparation`] captures model geometry and stored positions before
//! [`SceneSubmission`] and [`OrdinaryUpdates`]. Both [`OrdinaryPhase`] calls borrow
//! the same host services; only simulation advances ordinary actors. Effect hosts
//! use [`WorldClockSample::presentation_step`] so pending model preparation cannot
//! admit an effect update. Surfaces 5 and 11 support model-only displacement and
//! fatal health routing; other water displacement paths remain unsupported.
//!
//! Normal interpreter entry assigns the session native source before entry/task
//! admission. Alternate entry retains it. Native strings use that source's program,
//! while routed locals and properties remain receiver-owned. Snapshots rebind the
//! source actor identity; legacy unknown or unavailable world owners do not fall
//! back to the receiver. World-reset source lifetime remains unsupported. See
//! [preservation's retained-source contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#retained-health-source-context).
//!
//! Selected death-owning health publishes actor/display health before notification,
//! retains captured pose and numeric death progress/mode, and consumes the retained
//! source override. Death control starts the retained group before wrapping progress
//! accumulation and bypasses idle/movement. Default selection applies the retained
//! override before inventory queries and preserves matching playback cursors. Rope
//! cleanup, environmental spawning, last-save/menu dispatch and restart remain
//! explicit gaps. End-of-frame exit arbitration retains the independent falling
//! timeout and selected animation/progress completion before win requests. See
//! [preservation's exit ordering](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#world-loop-death-exit-ordering).
//! The caller supplies raw session death suppression; this does not establish
//! complete cheat or death support. See
//! [preservation's health ordering](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#selected-player-death-owning-health-mode).
//!
//! The implementation is grouped by responsibility:
//!
//! - [Registry](registry.rs), [ordinary scheduler](registry/ordinary.rs)
//!   and [tasks](tasks.rs) own actor dispatch and native continuation.
//! - [Spatial state](spatial.rs), [navigation](navigation.rs)
//!   and [point movement](registry/point.rs) own poses, reservations and
//!   supported route/direct walking phases. Collision and movement services live
//!   in the corresponding registry modules.
//! - [Combat state](combat.rs), [decision rules](decision.rs),
//!   [defence](registry/defence.rs), [attacks](registry/attack.rs),
//!   [spacing](registry/spacing.rs) and [perception](registry/sight.rs)
//!   separate retained state, deterministic rules and live receiver services.
//! - [Dialogue](dialogue.rs), [HUD](hud.rs),
//!   [inventory](inventory.rs) and [journal](journal.rs) own
//!   shared gameplay records; hosts supply playback, drawing and storage.
//! - [Persistence](registry/persistence.rs) validates snapshot candidates
//!   before replacing accepted logical state. Component snapshots do not enable
//!   player-facing save/load or original SAV import.
//! - [Camera state](camera.rs) prepares logical poses; the session host
//!   must supply live bindings, geometry and publication.

mod activation;
/// Equipped weapon poses and retained combat endpoints.
pub mod attachment;
pub use activation::{
    ActivationActor, ActivationAnimation, ActivationInputs, ActivationOverlay, ActivationState,
    GameplayGlobals, HostRequests, ItemViewerRequest,
};
mod animation;
mod attributes;
/// Logical camera preparation; installed initialization and live publication are host-owned.
pub mod camera;
mod combat;
pub mod decision;
pub use combat::{
    CombatControlState, CombatDecision, CombatMessage, CombatMessages, CombatTauntPlayback,
    CombatTauntSource, select_timed_combat_taunt,
};
mod dialogue;
pub use dialogue::{
    DialogueEffects, DialogueLine, DialoguePlayback, DialogueSnapshot, DialogueSource,
    DialogueState,
};
mod flags;
pub use flags::GlobalFlagsSnapshot;
mod hud;
pub use registry::{
    ActorEvent, ActorFlatDraw, AnimationData, CameraDrive, CameraFrameState, EnvironmentalEffect,
    EnvironmentalEffects, PublishedCamera, SelectedDeathState, SelectedHealthState,
};
mod inventory;
mod journal;
pub use hud::{CompassUse, ScriptHud, ScriptHudSnapshot};
pub use inventory::{Inventory, InventorySnapshot, InventoryView};
pub use journal::{Journal, JournalSnapshot};
pub mod locomotion;
mod maintenance;
mod markers;
mod menu;
pub use maintenance::{SoundMaintenance, SoundResetStage};
pub use menu::{MenuInput, MenuItem, MenuState};
pub use registry::body::{OrdinaryBodyQuery, OrdinaryResidualQuery};
pub use registry::bounds::{BoundsSegment, ordinary_bounds_segments, ordinary_bounds_separation};
pub use registry::falling::{
    OrdinaryDirectionalMotion, OrdinaryFallEntry, OrdinaryFallingRequest, OrdinaryLandingDecision,
    OrdinaryLandingSettings, ordinary_directional_motion, ordinary_fall_entry,
    ordinary_fall_origin, ordinary_falling_request, ordinary_health_value,
    ordinary_landing_decision, ordinary_landing_request,
};
pub use registry::ground::{OrdinaryCollisionShape, OrdinaryGroundQuery, OrdinaryStableProbe};
pub use registry::hazard::OrdinaryStepDirection;
pub use registry::model::{
    ModelPresentation, ModelSupportOwners, OrdinaryCollisionQuery, StableRecoveryQuery,
};
pub use registry::obstacle::{OrdinaryForwardHit, OrdinaryObstacleQuery, ordinary_forward_hit};
pub use registry::overlap::{OrdinaryOverlapMotion, OrdinaryOverlapPass, ordinary_overlap_motion};
pub use registry::player::PlayerControllerInput;
pub use registry::prevalidation::{OrdinaryGroundPrevalidation, ordinary_ground_prevalidation};
pub use registry::queued::{OrdinaryQueuedAction, OrdinaryQueuedInputs, ordinary_queued_action};
pub use registry::recovery::{OrdinaryRollbackSnapshot, OrdinaryStableRecoveryInputs};
pub use registry::sight::OrdinaryCombatSample;
pub use registry::slope::{
    OrdinarySlide, OrdinarySlopeInputs, OrdinarySlopeTurn, ordinary_slide, ordinary_slope_turn,
};
pub use registry::spacing::{
    OrdinaryForwardAmountInputs, OrdinaryMovement, OrdinarySpacingInput, OrdinarySpacingNeighbors,
};
pub use registry::transition::{
    ObstacleEnvironment, ObstacleFallStart, ObstacleTransition, ObstacleTransitionInput,
    ordinary_obstacle_transition,
};
pub use registry::vertical::{
    OrdinaryVerticalInputs, OrdinaryVerticalState, ordinary_falling_vertical,
    ordinary_queued_vertical, ordinary_vertical_factor,
};
pub use registry::{
    OrdinaryCombatBranch, OrdinaryCombatCoordinationInput, WalkRouteExclusion, WorldLoopOutcome,
};
mod navigation;
pub use navigation::{ActorNavigation, PointCommandState, WalkNodeSlot, WalkRouteState};
mod order;
mod playback;
mod quick_keys;
pub use quick_keys::{QuickKeys, ScriptKeys};
mod presentation;
mod registry;
pub use registry::ordinary::{
    ModelPreparation, OrdinaryContext, OrdinaryInputs, OrdinaryPhase, OrdinarySoundExecutor,
    OrdinaryUpdates, ScriptView, WorldClockSample, run_ordinary,
};
mod sound;
mod spatial;
pub use sound::ScriptSound;
/// Pure player-alignment arithmetic: `PlayerFaceObj`'s facing step and `PlayerLineUp`'s helpers.
pub mod alignment;
/// `BounceObject`'s launch state machine.
pub mod bounce;
mod group_sync;
/// The player-alignment movers of `PlayerPickupLineUp` and its fallback.
pub mod lineup;
/// `PlayerStatus` classification.
pub mod player_status;
mod status;
mod tasks;
pub use group_sync::GroupSync;
mod world_request;
pub use animation::AnimationScript;
use bevy::prelude::*;
pub use cyrus_script::Thread;
pub use playback::ScriptedPlayback;
pub use presentation::{ScriptBinding, ScriptEntityBinding, ScriptScene};
pub use registry::{
    ActorControlState, CombatEntryInputs, LocomotionGeometry, LogicalSessionCandidate,
    LogicalSessionRestoreInputs, LogicalSessionRestorePlan, LogicalSessionSnapshot,
    PlayerCollisionSource, ScriptActor, ScriptActorId, ScriptActorSource, ScriptActors,
    ScriptInputs, ScriptStage, ScriptWorld, WeaponInputs, WorldRestoreContext, WorldSnapshot,
    WorldSnapshotSources,
};
pub use spatial::{ActorSpatial, PlayerQuery, Pose, QueryInputs, rounded_distance};
pub use status::{PlaybackDiagnostic, PlaybackDiagnostics, ScriptStatus};
pub use tasks::{
    GxaOverlay, NativeCommandState, NativeTaskBindings, NativeTasks, NativeTasksSnapshot,
};
pub use world_request::request_world;
pub use world_request::{
    CreditsPlaying, DeathBookEntry, DeathDispatch, EndingEntry, LastGameEntry, LastSaveSelection,
    ScriptMovieEntry, WorldLoopExit,
};

/// Retain scene submissions before player/object updates mutate their source state.
/// Environmental effects follow preservation's SOUP world-loop submission order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneSubmission;

pub struct ScriptPlugin;
impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Update,
            SceneSubmission
                .after(ModelPreparation)
                .before(crate::player::runtime::step)
                .before(OrdinaryUpdates),
        );
        app.init_resource::<CameraDrive>()
            .init_resource::<CameraFrameState>()
            .init_resource::<ScriptStatus>()
            .init_resource::<PlaybackDiagnostics>()
            .init_resource::<crate::random::EngineRandom>()
            .init_resource::<ScriptActors>()
            .add_systems(
                Update,
                (registry::prepare, status::update)
                    .chain()
                    .before(crate::player::runtime::step)
                    .after(cyrus_world::lifecycle::WorldChange)
                    .run_if(resource_exists::<cyrus_world::lifecycle::PreparedWorld>),
            )
            .add_systems(
                Update,
                registry::clear_unprepared
                    .after(cyrus_world::lifecycle::WorldChange)
                    .before(crate::player::runtime::step),
            )
            .add_systems(
                Update,
                crate::clock::enter_world
                    .after(registry::prepare)
                    .after(registry::clear_unprepared)
                    .before(crate::player::runtime::step),
            )
            .add_systems(
                Update,
                registry::model::prepare_resources
                    .after(registry::prepare)
                    .before(registry::ordinary::sample_clock)
                    .before(OrdinaryUpdates)
                    .run_if(resource_exists::<cyrus_world::lifecycle::PreparedWorld>),
            )
            .init_resource::<QuickKeys>()
            .init_resource::<ScriptKeys>()
            .add_systems(
                Update,
                quick_keys::sample
                    .after(crate::player::input::InputSample)
                    .before(OrdinaryUpdates),
            )
            .add_systems(
                Update,
                registry::ordinary::sample_clock
                    .after(crate::clock::enter_world)
                    .before(crate::player::input::InputSample)
                    .before(OrdinaryUpdates),
            )
            .add_systems(
                Update,
                registry::ordinary::prepare_models
                    .in_set(ModelPreparation)
                    .after(registry::ordinary::sample_clock)
                    .after(presentation::bind_scene),
            )
            .add_systems(
                Update,
                registry::ordinary::step
                    .in_set(OrdinaryUpdates)
                    .after(crate::player::runtime::step)
                    .after(crate::clock::enter_world)
                    .after(crate::player::input::InputSample)
                    .after(presentation::bind_scene)
                    .before(presentation::apply),
            )
            .add_systems(Last, world_request::consume.in_set(WorldLoopExit))
            .add_systems(
                Update,
                presentation::bind_scene
                    .before(ModelPreparation)
                    .after(registry::prepare)
                    .after(registry::clear_unprepared)
                    .run_if(resource_exists::<cyrus_world::animation::AnimationRequest>),
            )
            .add_systems(
                Update,
                presentation::apply
                    .before(cyrus_world::animation::AnimationSystems)
                    .after(OrdinaryUpdates)
                    .after(crate::player::runtime::step)
                    .run_if(resource_exists::<cyrus_world::animation::AnimationRequest>),
            )
            .add_systems(
                Update,
                presentation::apply_ropes
                    .after(OrdinaryUpdates)
                    .run_if(resource_exists::<cyrus_world::animation::AnimationRequest>),
            )
            .add_systems(
                Update,
                playback::advance
                    .in_set(cyrus_world::animation::AnimationSystems)
                    .after(cyrus_world::lifecycle::WorldChange)
                    .run_if(resource_exists::<cyrus_world::animation::AnimationRequest>),
            );
    }
}
