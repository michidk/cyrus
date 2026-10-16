//! Explicit host-prepared identity bindings shared by restored audio owners.
use anyhow::{Context, Result, ensure};
use bevy::prelude::Entity;
use cyrus_game::script::ScriptActorId;
use std::collections::{BTreeMap, BTreeSet};

/// Complete supplied actor and presentation bindings for off-live audio reconstruction.
/// The host owns entity existence and prepares all required assets before using these
/// bindings. Missing owners fail lookup; construction never creates entities or plays audio.
pub struct AudioRestoreBindings {
    actors: BTreeMap<(u64, usize), ScriptActorId>,
    generations: BTreeMap<u64, u64>,
    entities: BTreeMap<u64, Entity>,
}
impl AudioRestoreBindings {
    /// Actor bindings come from the validated logical session plan. Entity bindings
    /// name the host's separately prepared presentation candidate, including visited owners.
    pub fn new(
        actors: &[(ScriptActorId, ScriptActorId)],
        entities: &[(u64, Entity)],
    ) -> Result<Self> {
        let mut result = Self {
            actors: BTreeMap::new(),
            generations: BTreeMap::new(),
            entities: BTreeMap::new(),
        };
        let mut actor_destinations = BTreeSet::new();
        let mut generation_destinations = BTreeMap::new();
        for &(old, new) in actors {
            ensure!(
                result
                    .actors
                    .insert((old.generation, old.index), new)
                    .is_none(),
                "audio binding actor {}/{} duplicated",
                old.generation,
                old.index
            );
            ensure!(
                actor_destinations.insert((new.generation, new.index)),
                "audio replacement actor {}/{} aliased",
                new.generation,
                new.index
            );
            ensure!(
                result
                    .generations
                    .insert(old.generation, new.generation)
                    .is_none_or(|previous| previous == new.generation),
                "audio saved generation {} maps to multiple worlds",
                old.generation
            );
            ensure!(
                generation_destinations
                    .insert(new.generation, old.generation)
                    .is_none_or(|previous| previous == old.generation),
                "audio replacement generation {} aliases saved worlds",
                new.generation
            );
        }
        let mut entity_destinations = BTreeSet::new();
        for &(old, new) in entities {
            ensure!(
                result.entities.insert(old, new).is_none(),
                "audio saved entity {old} duplicated"
            );
            ensure!(
                entity_destinations.insert(new.to_bits()),
                "audio replacement entity {new:?} aliased"
            );
        }
        Ok(result)
    }

    /// Resolve a saved logical actor without substituting a current-world default.
    pub fn actor(&self, old: ScriptActorId) -> Result<ScriptActorId> {
        self.actors
            .get(&(old.generation, old.index))
            .copied()
            .with_context(|| {
                format!(
                    "audio saved actor {}/{} binding absent",
                    old.generation, old.index
                )
            })
    }

    /// Resolve a retained service world generation from the complete actor census.
    pub fn generation(&self, old: u64) -> Result<u64> {
        self.generations
            .get(&old)
            .copied()
            .with_context(|| format!("audio saved generation {old} binding absent"))
    }

    /// Resolve a saved presentation owner to its host-prepared entity.
    pub fn entity(&self, old: u64) -> Result<Entity> {
        self.entities
            .get(&old)
            .copied()
            .with_context(|| format!("audio saved entity {old} binding absent"))
    }
}
