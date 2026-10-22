//! Fresh actor targets with explicit ownership of cancellation cleanup.
use crate::RuntimeSessionSnapshot;
use anyhow::{Context, Result, ensure};
use bevy::prelude::{Children, Component, Entity, Name, Visibility, World};
use cyrus_render::persistence::SceneCandidate;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Component)]
pub(super) struct StagedSessionEntity;

/// Map targets belong to the caller's staged scene; this owner never deletes them.
/// Standalone targets remain hidden and unbound until the aggregate commits.
pub struct SessionEntityStage {
    bindings: Vec<(u64, Entity)>,
    player: Entity,
    owned: Vec<Entity>,
}

impl SessionEntityStage {
    /// Match every saved map actor to exactly one prepared node before allocating
    /// standalone targets. Validation failure changes no live or staged entity.
    pub fn prepare(
        snapshot: &RuntimeSessionSnapshot,
        scene: Option<&SceneCandidate>,
        world: &mut World,
    ) -> Result<Self> {
        ensure!(
            snapshot.sky.is_some() == scene.is_some(),
            "session entity staging rendered scene census differs"
        );
        let mut nodes: BTreeMap<String, Vec<Entity>> = BTreeMap::new();
        if let Some(scene) = scene {
            scene.validate(world)?;
            let mut pending = vec![scene.root()];
            let mut seen = BTreeSet::new();
            while let Some(entity) = pending.pop() {
                ensure!(
                    seen.insert(entity),
                    "session staging scene hierarchy repeats an entity"
                );
                if let Some(name) = world.get::<Name>(entity) {
                    nodes
                        .entry(name.as_str().to_owned())
                        .or_default()
                        .push(entity);
                }
                if let Some(children) = world.get::<Children>(entity) {
                    pending.extend(children.iter());
                }
            }
        }
        let mut saved = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut choices = Vec::new();
        let mut players = 0;
        for binding in snapshot.entity_bindings() {
            ensure!(
                saved.insert(binding.saved_entity()),
                "session staging saved entity duplicated"
            );
            let name = match binding.role() {
                "map" => Some(crate::entities::map_node_name(
                    binding.actor().index,
                    binding.name(),
                )),
                // The host player entity is bound to the selected role: CYRUS, or GREMLIN while
                // the swap state is set. The other role has a standalone entity.
                "player" | "alternate_player" => {
                    if binding.saved_entity() == snapshot.player.entity {
                        players += 1;
                    }
                    None
                }
                "camera" => None,
                role => anyhow::bail!(
                    "session staging actor {}: unsupported role {role}",
                    binding.name()
                ),
            };
            let target = if scene.is_some()
                && let Some(name) = &name
            {
                let matches = nodes
                    .get(name)
                    .with_context(|| format!("session staging map node {name} absent"))?;
                ensure!(
                    matches.len() == 1,
                    "session staging map node {name} is ambiguous"
                );
                let target = matches[0];
                crate::entities::validate_retirement_ancestry(world, target)?;
                ensure!(targets.insert(target), "session staging map targets alias");
                Some(target)
            } else {
                None
            };
            choices.push((binding.saved_entity(), name, target));
        }
        ensure!(
            players == 1,
            "session staging requires exactly one selected player"
        );
        for source in snapshot.sample_playback().sources() {
            ensure!(
                saved.insert(source.saved_entity),
                "session staging sample playback entity duplicates another target"
            );
            choices.push((source.saved_entity, None, None));
        }
        let mut bindings = Vec::with_capacity(choices.len());
        let mut owned = Vec::new();
        let mut player = None;
        for (saved, name, target) in choices {
            let entity = target.unwrap_or_else(|| {
                let mut entity = world.spawn((StagedSessionEntity, Visibility::Hidden));
                if let Some(name) = name {
                    entity.insert(Name::new(name));
                }
                let id = entity.id();
                owned.push(id);
                id
            });
            if saved == snapshot.player.entity {
                player = Some(entity);
            }
            bindings.push((saved, entity));
        }
        Ok(Self {
            bindings,
            player: player.expect("validated selected player"),
            owned,
        })
    }

    pub fn bindings(&self) -> &[(u64, Entity)] {
        &self.bindings
    }
    pub fn player(&self) -> Entity {
        self.player
    }

    /// Remove only still-unpublished standalone targets from this stage. Commit
    /// removes the ownership marker, so cancellation after commit preserves them.
    /// The caller separately owns cancellation of prepared scene and asset work.
    pub fn discard(self, world: &mut World) {
        for entity in self.owned {
            if world.get::<StagedSessionEntity>(entity).is_some() {
                world.despawn(entity);
            }
        }
    }
}
