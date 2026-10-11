//! Explicitly activated group playback. The game scheduler owns eligibility and timing.
use super::sequence::SoundDispatcher;
use super::{
    ActorAnimation, AnimationPose, AnimationRequest, AnimationScript, Animations, FrameSequence,
};
use crate::sound::SoundCue;
use bevy::prelude::*;

/// Explicit playback on one placed GLB node, bound to its source world asset.
/// The caller supplies eligibility and nominal ticks; no world activation is inferred.
#[derive(Component)]
pub struct AnimationPlayback {
    source: Handle<Animations>,
    actor: String,
    playback: Playback,
}

enum Playback {
    Group(FrameSequence),
    Scripted {
        animation: ActorAnimation,
        script: AnimationScript,
        default: u16,
    },
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
            playback: Playback::Group(animations.sequence(actor, group, loops)?),
        })
    }

    /// Connects a supported authored main program to mesh playback with a caller-selected
    /// default group. Each supplied tick runs ordinary advancement/default request, then
    /// the script stage. Actor movement, state selection and interrupt work are unsupported.
    pub fn new_scripted(
        source: Handle<Animations>,
        animations: &Animations,
        actor: &str,
        default: u16,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            source,
            actor: actor.into(),
            playback: Playback::Scripted {
                animation: animations.actor_animation(actor, default)?,
                script: animations.animation_script(actor)?,
                default,
            },
        })
    }
}

impl Playback {
    fn advance(
        &mut self,
        ticks: u32,
        sounds: &mut SoundDispatcher<'_>,
    ) -> anyhow::Result<Option<u32>> {
        match self {
            Self::Group(sequence) => sequence.advance_with_sounds(ticks, false, sounds),
            Self::Scripted {
                animation,
                script,
                default,
            } => {
                let frame = animation.advance_dispatch(ticks, sounds)?;
                let frame = animation
                    .request_dispatch(*default, 0, ticks, sounds)?
                    .or(frame);
                Ok(script.advance_dispatch(animation, ticks, sounds)?.or(frame))
            }
        }
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
    mut sound_cues: Option<ResMut<Messages<SoundCue>>>,
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
            let result = if let Some(cues) = &mut sound_cues {
                let mut emit = |sound| {
                    cues.write(SoundCue::Animation {
                        actor: entity,
                        sound,
                    });
                    Ok(())
                };
                playback.playback.advance(tick.ticks, &mut Some(&mut emit))
            } else {
                playback.playback.advance(tick.ticks, &mut None)
            };
            let result = result.and_then(|reference| {
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
                    if matches!(&playback.playback, Playback::Group(sequence) if sequence.is_finished())
                    {
                        commands.entity(entity).remove::<AnimationPlayback>();
                    }
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
