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

/// World scene root whose exported map nodes may observe logical actors.
/// Once associated, a root cannot be reused for a later world generation.
#[derive(Component)]
pub struct ScriptScene {
    source: Handle<Animations>,
    generation: Option<u64>,
}
impl ScriptScene {
    pub fn new(source: Handle<Animations>) -> Self {
        Self {
            source,
            generation: None,
        }
    }
}

#[derive(Component)]
pub(super) struct SceneActorBound;

pub(super) fn bind_scene(
    mut commands: Commands,
    actors: Res<ScriptActors>,
    request: Res<AnimationRequest>,
    mut roots: Query<(Entity, &mut ScriptScene)>,
    children: Query<&Children>,
    nodes: Query<&Name, (Without<ScriptBinding>, Without<SceneActorBound>)>,
) {
    let Some(world) = &actors.current else {
        return;
    };
    for (root, mut scene) in &mut roots {
        if scene.source != request.0 {
            continue;
        }
        let generation = *scene.generation.get_or_insert(world.generation());
        if generation != world.generation() {
            continue;
        }
        for entity in children.iter_descendants(root) {
            let Ok(name) = nodes.get(entity) else {
                continue;
            };
            let Some((index, actor_name)) = name
                .as_str()
                .strip_prefix("B_")
                .and_then(|name| name.split_once('_'))
            else {
                continue;
            };
            let Ok(index) = index.parse::<usize>() else {
                continue;
            };
            let Some(actor) = world.actors().get(index) else {
                continue;
            };
            if actor.placement().is_some() && actor.name() == actor_name {
                commands.entity(entity).insert((
                    SceneActorBound,
                    ScriptBinding {
                        actor: super::ScriptActorId { generation, index },
                        source: scene.source.clone(),
                    },
                ));
            }
        }
    }
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
