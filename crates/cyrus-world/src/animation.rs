//! Retained animation inputs; playback timing and actor activation are not inferred.
use bevy::prelude::*;

mod actor;
mod playback;
mod sequence;
pub use actor::{ActorAnimation, ActorAnimationCheckpoint, ActorAnimationSnapshot};
pub use playback::{AnimationPlayback, AnimationTick};
pub use sequence::{AnimationSound, FrameSequence};

use cyrus_assets::animation::AnimationLoader;
pub use cyrus_assets::animation::{AnimationModel, Animations, ModelFrame};
/// Holds the current world's animation data alive until another world is requested.
#[derive(Resource, Default)]
pub struct AnimationRequest(pub Handle<Animations>);

/// Loads animation assets on native and WASM without inventing a playback clock.
pub struct AnimationPlugin;
impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Animations>()
            .init_asset_loader::<AnimationLoader>()
            .init_resource::<AnimationRequest>()
            .add_message::<AnimationTick>()
            .add_systems(
                Update,
                playback::advance
                    .in_set(AnimationSystems)
                    .after(crate::lifecycle::WorldChange),
            );
    }
}

/// An explicit model frame on a GLB model node. This component supplies no clock or activation.
#[derive(Component, Clone, PartialEq, Eq)]
pub struct AnimationPose {
    /// Uppercase RAAN model key; placed GLB node names identify instances, not models.
    pub model: String,
    /// Frame-table index, including base frame zero.
    pub frame: usize,
}

/// Explicit animation updates, ordered by the application against its consumers.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnimationSystems;
