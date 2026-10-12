//! Game-owned SOUP hosts; stages remain explicit until production timing is established.
mod activation;
pub use activation::{
    ActivationActor, ActivationAnimation, ActivationInputs, ActivationOverlay, ActivationState,
};
mod animation;
mod attributes;
mod flags;
mod playback;
mod presentation;
mod registry;
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
    ScriptActor, ScriptActorId, ScriptActorSource, ScriptActors, ScriptInputs, ScriptStage,
    ScriptWorld,
};
pub use spatial::{ActorSpatial, PlayerQuery, Pose, QueryInputs};
pub use status::ScriptStatus;

pub struct ScriptPlugin;
impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptStatus>()
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
                (presentation::bind_scene, presentation::apply)
                    .chain()
                    .in_set(cyrus_world::animation::AnimationSystems)
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
