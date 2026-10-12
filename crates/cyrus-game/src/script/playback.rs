//! Explicit script stages feed shared animation poses and synchronous sound intents.
use super::AnimationScript;
use bevy::prelude::*;
use cyrus_assets::animation::Animations;
use cyrus_world::{
    animation::{ActorAnimation, AnimationPose, AnimationRequest, AnimationTick},
    sound::SoundCue,
};

/// A caller attaches authored playback only after establishing its eligibility.
#[derive(Component)]
pub struct ScriptedPlayback {
    source: Handle<Animations>,
    actor: String,
    animation: ActorAnimation,
    script: AnimationScript,
    default: u16,
}
impl ScriptedPlayback {
    pub fn new(
        source: Handle<Animations>,
        data: &Animations,
        actor: &str,
        default: u16,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            source,
            actor: actor.into(),
            animation: ActorAnimation::from_asset(data, actor, default)?,
            script: AnimationScript::from_asset(data, actor)?,
            default,
        })
    }
}
pub(super) fn advance(
    mut commands: Commands,
    request: Res<AnimationRequest>,
    assets: Res<Assets<Animations>>,
    mut ticks: MessageReader<AnimationTick>,
    mut sounds: Option<ResMut<Messages<SoundCue>>>,
    mut nodes: Query<(Entity, &mut ScriptedPlayback)>,
) {
    for (entity, playback) in &nodes {
        if playback.source != request.0 {
            commands
                .entity(entity)
                .remove::<(ScriptedPlayback, AnimationPose)>();
        }
    }
    for tick in ticks.read() {
        for (entity, mut playback) in &mut nodes {
            if playback.source != request.0 {
                continue;
            }
            let Some(data) = assets.get(&playback.source) else {
                continue;
            };
            let ScriptedPlayback {
                animation,
                script,
                default,
                ..
            } = &mut *playback;
            let result = (|| {
                if let Some(sounds) = &mut sounds {
                    let mut emit = |sound| {
                        sounds.write(SoundCue::Animation {
                            actor: entity,
                            sound,
                        });
                        Ok(())
                    };
                    let frame = animation.advance_with_sounds(tick.ticks, &mut emit)?;
                    let frame = animation
                        .request_with_sounds(*default, 0, tick.ticks, &mut emit)?
                        .or(frame);
                    Ok(script
                        .advance_with_sounds(animation, tick.ticks, &mut emit)?
                        .or(frame))
                } else {
                    let frame = animation.advance(tick.ticks)?;
                    let frame = animation.request(*default, 0, tick.ticks)?.or(frame);
                    Ok(script.advance(animation, tick.ticks)?.or(frame))
                }
            })();
            let result: anyhow::Result<_> = result.and_then(|reference: Option<u32>| {
                reference
                    .map(|reference| data.resolve_frame(&playback.actor, reference))
                    .transpose()
            });
            match result {
                Ok(Some(frame)) => {
                    commands.entity(entity).insert(AnimationPose {
                        model: frame.model,
                        frame: frame.frame,
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    log::error!("world {} actor {}: {error:#}", data.world, playback.actor);
                    commands.entity(entity).remove::<ScriptedPlayback>();
                }
            }
        }
    }
}
