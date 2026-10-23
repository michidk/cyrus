//! Script preparation diagnostics distinguish loaded programs from executable gameplay.
use bevy::prelude::*;
use cyrus_assets::script::Scripts;
use cyrus_world::lifecycle::PreparedWorld;
use serde::Serialize;
use std::collections::BTreeMap;

/// Failures retained after an explicitly attached playback component is removed.
/// Consumers may drain these records; recording does not retry or advance playback.
#[derive(Resource, Default)]
pub struct PlaybackDiagnostics(pub Vec<PlaybackDiagnostic>);

#[derive(Debug, Clone, Serialize)]
pub struct PlaybackDiagnostic {
    pub world: u32,
    pub actor: String,
    /// Animation advancement, script advancement, or frame resolution.
    pub stage: String,
    pub reason: String,
}

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
        /// Actor flat records the world's drawing pass would submit now.
        actor_flats: usize,
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
            .filter(|actor| actor.failure().is_some())
            .count();
        *status = ScriptStatus::Loaded {
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            programs: scripts.actors.len(),
            instances: runtime_actors.len(),
            prepared,
            waiting,
            failed,
            unsupported: scripts.unsupported.len(),
            actor_flats: world.actor_flat_draws().map_or(0, |draws| draws.len()),
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
