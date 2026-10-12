//! Script preparation diagnostics distinguish loaded programs from executable gameplay.
use bevy::prelude::*;
use cyrus_assets::script::Scripts;
use cyrus_world::lifecycle::PreparedWorld;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Resource, Default, Debug, Serialize)]
#[serde(tag = "state")]
pub enum ScriptStatus {
    #[default]
    NotRequested,
    Loading,
    Loaded {
        world: u32,
        content_id: String,
        programs: usize,
        instances: usize,
        prepared: usize,
        waiting: usize,
        failed: usize,
        unsupported: usize,
        examples: BTreeMap<String, String>,
    },
    Failed {
        error: String,
    },
}

pub(super) fn update(
    prepared: Res<PreparedWorld>,
    assets: Res<Assets<Scripts>>,
    server: Res<AssetServer>,
    mut status: ResMut<ScriptStatus>,
    actors: Res<super::ScriptActors>,
) {
    if let Some(error) = &actors.error {
        *status = ScriptStatus::Failed {
            error: error.clone(),
        };
        return;
    }
    if let Some(scripts) = assets.get(&prepared.scripts) {
        let Some(world) = &actors.current else {
            *status = match server.get_load_state(prepared.animations.id()) {
                Some(bevy::asset::LoadState::Failed(error)) => ScriptStatus::Failed {
                    error: format!(
                        "world {} animation metadata: {error}",
                        prepared.requested_name
                    ),
                },
                _ => ScriptStatus::Loading,
            };
            return;
        };
        let runtime_actors = world.actors();
        let prepared = runtime_actors
            .iter()
            .filter(|actor| actor.runtime().is_some())
            .count();
        let waiting = runtime_actors
            .iter()
            .filter(|actor| {
                actor.runtime().is_some_and(|runtime| {
                    runtime.waiting(cyrus_script::Thread::Main).is_some()
                        || runtime.waiting(cyrus_script::Thread::Interrupt).is_some()
                })
            })
            .count();
        let failed = runtime_actors
            .iter()
            .filter(|actor| {
                actor
                    .runtime()
                    .is_some_and(|runtime| runtime.failure().is_some())
            })
            .count();
        *status = ScriptStatus::Loaded {
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            programs: scripts.actors.len(),
            instances: scripts.instances.len(),
            prepared,
            waiting,
            failed,
            unsupported: scripts.unsupported.len(),
            examples: scripts
                .unsupported
                .iter()
                .take(16)
                .map(|(name, reason)| (name.clone(), reason.clone()))
                .collect(),
        };
    } else {
        *status = match server.get_load_state(prepared.scripts.id()) {
            Some(bevy::asset::LoadState::Failed(error)) => ScriptStatus::Failed {
                error: format!("world {} script metadata: {error}", prepared.requested_name),
            },
            _ => ScriptStatus::Loading,
        };
    }
}
