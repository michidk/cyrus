//! Explicitly activated group playback. The game scheduler owns eligibility and timing.
use super::{AnimationPose, AnimationRequest, Animations, FrameSequence};
use bevy::prelude::*;

/// An activated forward group on one placed GLB node, bound to its source world asset.
/// This component does not activate actors, run SOUP, or select a default group after End.
#[derive(Component)]
pub struct AnimationPlayback {
    source: Handle<Animations>,
    actor: String,
    sequence: FrameSequence,
}

impl AnimationPlayback {
    /// Creates playback for a group already selected by the caller's actor scheduler.
    pub fn new(
        source: Handle<Animations>,
        animations: &Animations,
        actor: &str,
        group: u16,
        loops: u8,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            source,
            actor: actor.into(),
            sequence: animations.sequence(actor, group, loops)?,
        })
    }
}

/// One eligible simulation update's whole nominal ticks, including zero.
/// These are original timing inputs, not rendered frames or elapsed milliseconds.
#[derive(Message)]
pub struct AnimationTick {
    /// Whole tick increment derived by the game clock.
    pub ticks: u32,
}

pub(super) fn advance(
    mut commands: Commands,
    request: Res<AnimationRequest>,
    assets: Res<Assets<Animations>>,
    mut ticks: MessageReader<AnimationTick>,
    mut nodes: Query<(Entity, &mut AnimationPlayback)>,
) {
    for (entity, playback) in &nodes {
        if playback.source != request.0 {
            commands
                .entity(entity)
                .remove::<(AnimationPlayback, AnimationPose)>();
        }
    }
    for tick in ticks.read() {
        for (entity, mut playback) in &mut nodes {
            if playback.source != request.0 {
                continue;
            }
            let Some(animations) = assets.get(&playback.source) else {
                continue;
            };
            let result = playback
                .sequence
                .advance(tick.ticks, false)
                .and_then(|reference| {
                    reference
                        .map(|reference| animations.resolve_frame(&playback.actor, reference))
                        .transpose()
                });
            match result {
                Ok(Some(frame)) => {
                    commands.entity(entity).insert(AnimationPose {
                        model: frame.model,
                        frame: frame.frame,
                    });
                }
                Ok(None) => {
                    commands.entity(entity).remove::<AnimationPlayback>();
                }
                Err(error) => {
                    error!(
                        "world {} actor {}: {error:#}",
                        animations.world, playback.actor
                    );
                    commands.entity(entity).remove::<AnimationPlayback>();
                }
            }
        }
    }
}
