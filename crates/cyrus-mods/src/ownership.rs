//! Loaded guest incarnations shared by asynchronous gameplay and visual hooks.
use crate::Catalog;
use bevy::prelude::*;
use cyrus_script::script::ScriptActors;
use std::collections::{BTreeMap, HashMap};
use wasvy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Reflect)]
pub(crate) struct Owner {
    pub(crate) instance: Entity,
    pub(crate) revision: u64,
}

#[derive(Resource, Default)]
pub(crate) struct Owners {
    pub(crate) active: BTreeMap<String, Owner>,
    pub(crate) world: Option<(u32, u64)>,
}

pub(crate) fn publish(
    mut owners: ResMut<Owners>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    mods: Query<&wasvy::prelude::Mod>,
    mut events: MessageReader<AssetEvent<ModAsset>>,
    mut revisions: Local<HashMap<AssetId<ModAsset>, u64>>,
) {
    for event in events.read() {
        if let AssetEvent::LoadedWithDependencies { id } = event {
            *revisions.entry(*id).or_default() += 1;
        }
    }
    owners.world = actors
        .as_deref()
        .and_then(|actors| actors.current.as_ref())
        .map(|world| (world.world, world.generation()));
    owners.active = catalog
        .entries
        .iter()
        .filter(|entry| entry.active && !entry.gone)
        .filter_map(|entry| {
            let instance = entry.instance?;
            let asset = mods.get(instance).ok()?.asset().id();
            Some((
                entry.id.clone(),
                Owner {
                    instance,
                    revision: revisions.get(&asset).copied().unwrap_or_default(),
                },
            ))
        })
        .collect();
}
