//! Actor-backed entity census used to prepare map nodes and standalone roles.
use anyhow::{Context, Result, ensure};
use bevy::prelude::{
    Assets, Commands, Entity, Handle, Name, Quat, Transform, Vec3, Visibility, World,
};
use cyrus_audio::persistence::AudioRestoreBindings;
use cyrus_game::script::{
    ScriptActorId, ScriptActorSource, ScriptActors, ScriptBinding, ScriptEntityBinding, ScriptWorld,
};
use cyrus_world::animation::{AnimationPose, Animations};
use cyrus_world::sound::SoundSurface;
use serde::{Deserialize, Serialize};

/// Saved presentation owner hint. Entity IDs are never reused directly on load.
#[derive(Clone, Serialize, Deserialize)]
pub struct EntityBindingSnapshot {
    entity: u64,
    actor: (u64, usize),
    name: String,
    role: String,
    displayed: Option<(String, usize)>,
    surface: Option<u8>,
    transform: Option<[u32; 10]>,
    visibility: Option<u8>,
}
impl EntityBindingSnapshot {
    pub fn saved_entity(&self) -> u64 {
        self.entity
    }
    pub fn actor(&self) -> ScriptActorId {
        ScriptActorId {
            generation: self.actor.0,
            index: self.actor.1,
        }
    }
    /// Authored actor name; exported map-node indices have a minimum width of three digits.
    pub fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn displayed(&self) -> Option<(&str, usize)> {
        self.displayed
            .as_ref()
            .map(|(model, frame)| (model.as_str(), *frame))
    }
    pub fn role(&self) -> &str {
        &self.role
    }
}
/// Matches the scene exporter's minimum-width map instance index.
pub(crate) fn map_node_name(index: usize, name: &str) -> String {
    format!("B_{index:03}_{name}")
}

fn role(source: &ScriptActorSource) -> &'static str {
    match source {
        ScriptActorSource::Map(_) => "map",
        ScriptActorSource::Player => "player",
        ScriptActorSource::AlternatePlayer => "alternate_player",
        ScriptActorSource::Camera => "camera",
    }
}

pub(super) fn capture(world: &World) -> Result<Vec<EntityBindingSnapshot>> {
    let mut query = world
        .try_query::<(Entity, &ScriptBinding)>()
        .context("session actor bindings absent")?;
    let current = super::resource::<ScriptActors>(world, "script actors")?
        .current
        .as_ref()
        .context("entity census current world absent")?;
    let animation =
        super::resource::<cyrus_world::animation::AnimationRequest>(world, "animation source")?;
    let mut saved = Vec::new();
    for (entity, binding) in query.iter(world) {
        ensure!(
            binding.actor.generation == current.generation() && binding.source == animation.0,
            "entity {entity:?}: stale actor/source binding at snapshot boundary"
        );
        let actor = current
            .actors()
            .get(binding.actor.index)
            .context("entity census actor absent")?;
        saved.push(EntityBindingSnapshot {
            entity: entity.to_bits(),
            actor: (binding.actor.generation, binding.actor.index),
            name: actor.name().into(),
            role: role(&actor.source).into(),
            displayed: world
                .get::<AnimationPose>(entity)
                .map(|pose| (pose.model.clone(), pose.frame)),
            surface: world.get::<SoundSurface>(entity).map(|surface| surface.0),
            transform: if matches!(actor.source, ScriptActorSource::Player) {
                None
            } else {
                world.get::<Transform>(entity).map(|pose| {
                    let values = [
                        pose.translation.x,
                        pose.translation.y,
                        pose.translation.z,
                        pose.rotation.x,
                        pose.rotation.y,
                        pose.rotation.z,
                        pose.rotation.w,
                        pose.scale.x,
                        pose.scale.y,
                        pose.scale.z,
                    ];
                    values.map(f32::to_bits)
                })
            },
            visibility: world.get::<Visibility>(entity).map(|v| match v {
                Visibility::Inherited => 0,
                Visibility::Hidden => 1,
                Visibility::Visible => 2,
            }),
        });
    }
    saved.sort_by_key(|entry| entry.entity);
    Ok(saved)
}

/// Prepared attachment; queued together with actor resources and service replacements.
pub struct EntityBindingCandidate {
    pub entity: Entity,
    binding: ScriptEntityBinding,
    displayed: Option<AnimationPose>,
    surface: Option<SoundSurface>,
    transform: Option<Transform>,
    visibility: Option<Visibility>,
    player: bool,
    map: bool,
}
impl EntityBindingCandidate {
    pub(crate) fn is_map(&self) -> bool {
        self.map
    }
    /// Recheck staging immediately before the complete session publication.
    pub fn validate_publication(&self, world: &World) -> Result<()> {
        ensure!(
            world.get_entity(self.entity).is_ok(),
            "session entity publication target {:?} disappeared",
            self.entity
        );
        ensure!(
            world.get::<ScriptBinding>(self.entity).is_none()
                && world
                    .get::<cyrus_game::player::Player>(self.entity)
                    .is_none(),
            "session entity publication target {:?} already has a live owner",
            self.entity
        );
        validate_retirement_ancestry(world, self.entity)?;
        Ok(())
    }

    pub fn queue(self, commands: &mut Commands, player: Entity) {
        self.binding.queue(commands, self.entity, player);
        let mut target = commands.entity(self.entity);
        target.remove::<crate::staging::StagedSessionEntity>();
        if let Some(pose) = self.displayed {
            target.insert(pose);
        } else {
            target.remove::<AnimationPose>();
        }
        if let Some(surface) = self.surface {
            target.insert(surface);
        } else {
            target.remove::<SoundSurface>();
        }
        if !self.player {
            if let Some(pose) = self.transform {
                target.insert(pose);
            } else {
                target.remove::<Transform>();
            }
        }
        if let Some(visibility) = self.visibility {
            target.insert(visibility);
        } else {
            target.remove::<Visibility>();
        }
    }
}

pub(super) fn restore(
    saved: Vec<EntityBindingSnapshot>,
    bindings: &AudioRestoreBindings,
    current: &ScriptWorld,
    animation: &Handle<Animations>,
    live: &World,
) -> Result<Vec<EntityBindingCandidate>> {
    let mut seen = std::collections::BTreeSet::new();
    saved
        .into_iter()
        .map(|entry| {
            ensure!(seen.insert(entry.entity), "saved entity binding duplicated");
            let entity = bindings.entity(entry.entity)?;
            ensure!(
                live.get_entity(entity).is_ok(),
                "replacement bound entity {entity:?} absent"
            );
            let actor = bindings.actor(entry.actor())?;
            ensure!(
                actor.generation == current.generation(),
                "replacement presentation actor belongs to inactive world"
            );
            let owner = current
                .actors()
                .get(actor.index)
                .context("replacement entity actor absent")?;
            ensure!(
                owner.name() == entry.name && role(&owner.source) == entry.role,
                "replacement entity {entity:?}: actor name/role differs"
            );
            if matches!(owner.source, ScriptActorSource::Map(_)) {
                let expected = map_node_name(actor.index, owner.name());
                ensure!(
                    live.get::<Name>(entity)
                        .is_some_and(|name| name.as_str() == expected),
                    "replacement map entity {entity:?}: expected node {expected}"
                );
            }
            let player = matches!(owner.source, ScriptActorSource::Player);
            ensure!(
                !player || entry.transform.is_none(),
                "selected-player transform must belong to placement snapshot"
            );
            let transform = entry
                .transform
                .map(|bits| {
                    let [x, y, z, qx, qy, qz, qw, sx, sy, sz] = bits.map(f32::from_bits);
                    ensure!(
                        bits.map(f32::from_bits)
                            .iter()
                            .all(|value| value.is_finite()),
                        "restored entity transform is nonfinite"
                    );
                    let rotation = Quat::from_xyzw(qx, qy, qz, qw);
                    ensure!(
                        rotation.is_normalized(),
                        "restored entity rotation is not normalized"
                    );
                    Ok::<_, anyhow::Error>(Transform {
                        translation: Vec3::new(x, y, z),
                        rotation,
                        scale: Vec3::new(sx, sy, sz),
                    })
                })
                .transpose()?;
            let displayed = entry
                .displayed
                .map(|(model, frame)| {
                    let data = super::resource::<Assets<Animations>>(live, "animation assets")?
                        .get(animation)
                        .context("restored entity animation asset absent")?;
                    ensure!(
                        data.models
                            .get(&model)
                            .is_some_and(|data| frame < data.positions.len()),
                        "restored entity model {model} frame {frame} absent"
                    );
                    Ok::<_, anyhow::Error>(AnimationPose { model, frame })
                })
                .transpose()?;
            let visibility = entry
                .visibility
                .map(|v| match v {
                    0 => Ok(Visibility::Inherited),
                    1 => Ok(Visibility::Hidden),
                    2 => Ok(Visibility::Visible),
                    _ => anyhow::bail!("restored entity visibility outside supported range"),
                })
                .transpose()?;
            Ok(EntityBindingCandidate {
                entity,
                binding: current.prepare_entity_binding(actor, animation.clone())?,
                displayed,
                surface: entry.surface.map(SoundSurface),
                transform,
                visibility,
                player,
                map: matches!(owner.source, ScriptActorSource::Map(_)),
            })
        })
        .collect()
}

pub(super) fn validate_retirement_ancestry(world: &World, target: Entity) -> Result<()> {
    let mut cursor = Some(target);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(entity) = cursor {
        ensure!(
            seen.insert(entity),
            "session entity hierarchy contains a cycle"
        );
        ensure!(
            world.get_entity(entity).is_ok(),
            "session target hierarchy entity absent"
        );
        ensure!(
            world.get::<ScriptBinding>(entity).is_none()
                && world
                    .get::<cyrus_render::camera::ViewCamera>(entity)
                    .is_none()
                && world
                    .get::<cyrus_game::script::ScriptedPlayback>(entity)
                    .is_none()
                && world.get::<cyrus_game::player::Player>(entity).is_none()
                && world
                    .get::<cyrus_audio::sound::PlayingSound>(entity)
                    .is_none()
                && world
                    .get::<cyrus_audio::music::MusicPlayer>(entity)
                    .is_none(),
            "session target {target:?} belongs to a retiring owner"
        );
        cursor = world
            .get::<bevy::prelude::ChildOf>(entity)
            .map(bevy::prelude::ChildOf::parent);
    }
    Ok(())
}
