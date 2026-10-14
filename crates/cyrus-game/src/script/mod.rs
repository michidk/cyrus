//! Game-owned SOUP hosts and supported camera-centered ordinary actor updates.
mod activation;
pub use activation::{
    ActivationActor, ActivationAnimation, ActivationInputs, ActivationOverlay, ActivationState,
    GameplayGlobals,
};
mod animation;
mod attributes;
mod combat;
pub use combat::{CombatControlState, CombatMessage, CombatMessages, CombatTauntPlayback};
mod dialogue;
pub use dialogue::{
    DialogueEffects, DialogueLine, DialoguePlayback, DialogueSource, DialogueState,
};
mod flags;
mod hud;
pub use hud::{CompassUse, ScriptHud};
mod maintenance;
mod markers;
pub use maintenance::{SoundMaintenance, SoundResetStage};
pub use registry::body::{OrdinaryBodyQuery, OrdinaryResidualQuery};
pub use registry::bounds::{BoundsSegment, ordinary_bounds_segments, ordinary_bounds_separation};
pub use registry::falling::{
    OrdinaryDirectionalMotion, OrdinaryFallEntry, OrdinaryFallingRequest, OrdinaryLandingDecision,
    OrdinaryLandingSettings, ordinary_directional_motion, ordinary_fall_entry,
    ordinary_fall_origin, ordinary_falling_request, ordinary_health_value,
    ordinary_landing_decision, ordinary_landing_request,
};
pub use registry::ground::{OrdinaryCollisionShape, OrdinaryGroundQuery, OrdinaryStableProbe};
pub use registry::model::{
    ModelPresentation, ModelSupportOwners, OrdinaryCollisionQuery, StableRecoveryQuery,
};
pub use registry::obstacle::{OrdinaryForwardHit, OrdinaryObstacleQuery, ordinary_forward_hit};
pub use registry::overlap::{OrdinaryOverlapMotion, OrdinaryOverlapPass, ordinary_overlap_motion};
pub use registry::prevalidation::{OrdinaryGroundPrevalidation, ordinary_ground_prevalidation};
pub use registry::queued::{OrdinaryQueuedAction, OrdinaryQueuedInputs, ordinary_queued_action};
pub use registry::recovery::{OrdinaryRollbackSnapshot, OrdinaryStableRecoveryInputs};
pub use registry::slope::{
    OrdinarySlide, OrdinarySlopeInputs, OrdinarySlopeTurn, ordinary_slide, ordinary_slope_turn,
};
pub use registry::transition::{
    ObstacleEnvironment, ObstacleFallStart, ObstacleTransition, ObstacleTransitionInput,
    ordinary_obstacle_transition,
};
pub use registry::vertical::{
    OrdinaryVerticalInputs, OrdinaryVerticalState, ordinary_falling_vertical,
    ordinary_queued_vertical, ordinary_vertical_factor,
};
mod navigation;
mod order;
mod playback;
mod presentation;
mod registry;
pub use registry::ordinary::{
    OrdinaryContext, OrdinaryInputs, OrdinarySoundExecutor, OrdinaryUpdates, ScriptView,
    run_ordinary,
};
mod sound;
mod spatial;
pub use sound::ScriptSound;
mod status;
mod tasks;
pub use animation::AnimationScript;
use bevy::prelude::*;
pub use cyrus_script::Thread;
pub use playback::ScriptedPlayback;
pub use presentation::{ScriptBinding, ScriptScene};
pub use registry::{
    ActorControlState, ScriptActor, ScriptActorId, ScriptActorSource, ScriptActors, ScriptInputs,
    ScriptStage, ScriptWorld,
};
pub use spatial::{ActorSpatial, PlayerQuery, Pose, QueryInputs, rounded_distance};
pub use status::{PlaybackDiagnostic, PlaybackDiagnostics, ScriptStatus};

pub struct ScriptPlugin;
impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptStatus>()
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
            .add_systems(
                Update,
                registry::ordinary::sample_clock
                    .after(crate::clock::enter_world)
                    .before(crate::player::input::InputSample)
                    .before(OrdinaryUpdates),
            )
            .add_systems(
                Update,
                registry::ordinary::step
                    .in_set(OrdinaryUpdates)
                    .after(crate::clock::enter_world)
                    .after(crate::player::input::InputSample)
                    .after(presentation::bind_scene)
                    .before(presentation::apply),
            )
            .add_systems(
                Update,
                (presentation::bind_scene, presentation::apply)
                    .chain()
                    .before(cyrus_world::animation::AnimationSystems)
                    .after(crate::player::runtime::step)
                    .after(registry::prepare)
                    .after(registry::clear_unprepared)
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
