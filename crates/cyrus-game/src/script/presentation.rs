//! Presentation bindings read poses from the authoritative logical script actor.
use super::{ScriptActorId, ScriptActors};
use bevy::prelude::*;
use cyrus_assets::animation::Animations;
use cyrus_world::animation::{AnimationPose, AnimationRequest};

/// An explicitly selected scene node observes one actor in one prepared world.
/// This binding grants neither script nor animation update eligibility.
#[derive(Component)]
pub struct ScriptBinding {
    pub actor: ScriptActorId,
    pub source: Handle<Animations>,
}

pub(super) fn apply(
    mut commands: Commands,
    actors: Res<ScriptActors>,
    request: Res<AnimationRequest>,
    assets: Res<Assets<Animations>>,
    nodes: Query<(Entity, &ScriptBinding, Option<&AnimationPose>)>,
) {
    for (entity, binding, pose) in &nodes {
        let world = actors.current.as_ref().filter(|world| {
            world.generation() == binding.actor.generation && binding.source == request.0
        });
        let Some(world) = world else {
            commands
                .entity(entity)
                .remove::<(ScriptBinding, AnimationPose)>();
            continue;
        };
        let Some(data) = assets.get(&binding.source) else {
            continue;
        };
        let result = world.animation_frame(binding.actor).and_then(|frame| {
            frame
                .map(|frame| {
                    let actor = &world.actors()[binding.actor.index];
                    if actor.placement().is_some() {
                        data.resolve_map_frame(binding.actor.index, frame)
                    } else {
                        data.resolve_frame(actor.name(), frame)
                    }
                })
                .transpose()
        });
        match result {
            Ok(Some(frame)) => {
                let next = AnimationPose {
                    model: frame.model,
                    frame: frame.frame,
                };
                if pose != Some(&next) {
                    commands.entity(entity).insert(next);
                }
            }
            Ok(None) => {
                if pose.is_some() {
                    commands.entity(entity).remove::<AnimationPose>();
                }
            }
            Err(error) => {
                log::error!(
                    "world {} script instance {} pose: {error:#}",
                    world.world,
                    binding.actor.index
                );
                commands
                    .entity(entity)
                    .remove::<(ScriptBinding, AnimationPose)>();
            }
        }
    }
}
