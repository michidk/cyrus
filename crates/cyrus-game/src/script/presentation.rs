//! Presentation bindings read poses from the authoritative logical script actor.
use super::{ScriptActorId, ScriptActors};
use bevy::prelude::*;
use cyrus_assets::animation::Animations;
use cyrus_world::animation::{AnimationPose, AnimationRequest};
use cyrus_world::sound::SoundSurface;

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

#[derive(Component)]
pub(super) struct StandaloneRole;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct RoleBindings<'w, 's> {
    players: Query<'w, 's, (Entity, Option<&'static ScriptBinding>), With<crate::player::Player>>,
    roles: Query<'w, 's, (Entity, Option<&'static ScriptBinding>), With<StandaloneRole>>,
}

pub(super) fn bind_scene(
    mut commands: Commands,
    actors: Res<ScriptActors>,
    request: Res<AnimationRequest>,
    mut roots: Query<(Entity, &mut ScriptScene)>,
    children: Query<&Children>,
    nodes: Query<&Name, (Without<ScriptBinding>, Without<SceneActorBound>)>,
    role_bindings: RoleBindings,
) {
    let RoleBindings { players, roles } = role_bindings;
    let Some(world) = &actors.current else {
        for (entity, _) in &roles {
            commands.entity(entity).despawn();
        }
        return;
    };
    for (entity, binding) in &roles {
        if binding.is_none_or(|binding| {
            binding.actor.generation != world.generation() || binding.source != request.0
        }) {
            commands.entity(entity).despawn();
        }
    }
    if let Ok((player, binding)) = players.single() {
        for (index, actor) in world.actors().iter().enumerate() {
            let id = ScriptActorId {
                generation: world.generation(),
                index,
            };
            match actor.source {
                super::ScriptActorSource::Player => {
                    if binding
                        .is_none_or(|binding| binding.actor != id || binding.source != request.0)
                    {
                        commands.entity(player).insert(ScriptBinding {
                            actor: id,
                            source: request.0.clone(),
                        });
                    }
                }
                super::ScriptActorSource::AlternatePlayer
                    if !roles.iter().any(|(_, binding)| {
                        binding.is_some_and(|binding| {
                            binding.actor == id && binding.source == request.0
                        })
                    }) =>
                {
                    commands.spawn((
                        StandaloneRole,
                        cyrus_world::sound::PlayerAnimationSource,
                        ScriptBinding {
                            actor: id,
                            source: request.0.clone(),
                        },
                        Transform::default(),
                        ChildOf(player),
                    ));
                }
                _ => {}
            }
        }
    }
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

type PresentationNode = (
    Entity,
    &'static ScriptBinding,
    Option<&'static AnimationPose>,
    Option<&'static SoundSurface>,
    Option<&'static mut crate::player::placement::PlayerPose>,
    Option<&'static mut Transform>,
);

pub(super) fn apply(
    mut commands: Commands,
    actors: Res<ScriptActors>,
    request: Res<AnimationRequest>,
    assets: Res<Assets<Animations>>,
    mut nodes: Query<PresentationNode>,
) {
    for (entity, binding, pose, surface, player_pose, transform) in &mut nodes {
        let world = actors.current.as_ref().filter(|world| {
            world.generation() == binding.actor.generation && binding.source == request.0
        });
        let Some(world) = world else {
            commands
                .entity(entity)
                .remove::<(ScriptBinding, AnimationPose, SoundSurface)>();
            continue;
        };
        if world
            .actors()
            .get(binding.actor.index)
            .is_some_and(|actor| matches!(actor.source, super::ScriptActorSource::Player))
            && let (Some(mut player_pose), Some(mut transform), Ok(spatial)) =
                (player_pose, transform, world.spatial(binding.actor))
        {
            let position = spatial.local.position.map(i64::from);
            let delta = std::array::from_fn::<_, 3, _>(|axis| {
                (position[axis] - player_pose.position[axis]) as f32
                    / (256.0 * crate::ENGINE_UNIT_SCALE)
            });
            transform.translation += Vec3::new(-delta[0], -delta[1], delta[2]);
            player_pose.position = position;
            player_pose.heading = spatial.local.angles[1];
        }
        match world.actor_surface(binding.actor).ok().map(SoundSurface) {
            Some(next) if surface != Some(&next) => {
                commands.entity(entity).insert(next);
            }
            None if surface.is_some() => {
                commands.entity(entity).remove::<SoundSurface>();
            }
            _ => {}
        }
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
                    .remove::<(ScriptBinding, AnimationPose, SoundSurface)>();
            }
        }
    }
}

/// Validated actor-to-entity attachment for the complete session publication.
/// The authoritative actor remains in ScriptWorld; this supplies no execution eligibility.
pub struct ScriptEntityBinding {
    binding: ScriptBinding,
    map: bool,
    alternate: bool,
}
impl super::ScriptWorld {
    pub fn prepare_entity_binding(
        &self,
        actor: ScriptActorId,
        source: Handle<Animations>,
    ) -> anyhow::Result<ScriptEntityBinding> {
        anyhow::ensure!(
            self.id(actor.index)? == actor,
            "restored presentation actor generation differs"
        );
        let actor_source = &self.actors()[actor.index].source;
        Ok(ScriptEntityBinding {
            binding: ScriptBinding { actor, source },
            map: matches!(actor_source, super::ScriptActorSource::Map(_)),
            alternate: matches!(actor_source, super::ScriptActorSource::AlternatePlayer),
        })
    }
}
impl ScriptEntityBinding {
    /// Queue after all target entities and the selected player have been validated.
    pub fn queue(self, commands: &mut Commands, entity: Entity, selected_player: Entity) {
        let mut target = commands.entity(entity);
        target.insert(self.binding);
        if self.map {
            target.insert(SceneActorBound);
        }
        if self.alternate {
            target.insert((
                StandaloneRole,
                cyrus_world::sound::PlayerAnimationSource,
                ChildOf(selected_player),
            ));
        }
    }
}
