//! One actor's logical and rendered attachment ownership after the frame update.
use bevy::prelude::*;
use cyrus_control::Response;

use serde_json::json;
use std::sync::mpsc::Sender;

#[derive(Resource)]
pub(super) struct PendingRenderedActor {
    pub id: u64,
    pub index: usize,
    pub after_generation: Option<u64>,
    pub deadline: std::time::Instant,
    pub reply: Sender<Response>,
}

pub(super) fn respond(world: &mut World) {
    let Some(pending) = world.get_resource::<PendingRenderedActor>() else {
        return;
    };
    let generation = world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .and_then(|actors| actors.current.as_ref())
        .map(|actors| actors.generation());
    let in_game = world
        .get_resource::<State<cyrus_game::AppState>>()
        .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame);
    let scene_ready = world
        .get_resource::<cyrus_render::ReadyScene>()
        .zip(world.get_resource::<cyrus_world::lifecycle::PreparedWorld>())
        .is_some_and(|(ready, prepared)| ready.0 == prepared.animations);
    let revisit_pending = world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .is_some_and(cyrus_script::script::ScriptActors::has_pending_revisit);
    let model_pending = world
        .try_query::<(&cyrus_script::script::ScriptBinding, &Children)>()
        .is_some_and(|mut query| {
            query.iter(world).any(|(binding, children)| {
                binding.actor.index == pending.index
                    && Some(binding.actor.generation) == generation
                    && children.iter().any(|child| {
                        world.get::<WorldAssetRoot>(child).is_some()
                            && world
                                .get::<bevy::world_serialization::WorldInstance>(child)
                                .is_none_or(|instance| {
                                    world
                                .get_resource::<bevy::world_serialization::WorldInstanceSpawner>()
                                .is_none_or(|spawner| !spawner.instance_is_ready(**instance))
                                })
                    })
            })
        });
    let waiting = pending.after_generation.is_some_and(|old| {
        generation.is_none()
            || generation == Some(old)
            || !in_game
            || !scene_ready
            || revisit_pending
            || model_pending
    });
    // A frame's mesh replacement finishes asynchronously after its instance spawns.
    // Keep the same exact unsettled-frame rejection used by public capture pending.
    let mesh_pending = pending.after_generation.is_some()
        && !waiting
        && world
            .get_resource::<cyrus_script::script::ScriptActors>()
            .and_then(|actors| actors.current.as_ref())
            .and_then(|actors| actors.id(pending.index).ok())
            .is_some_and(|actor| {
                cyrus_render::inspect_actor_model(world, actor).is_err_and(|error| {
                    error.to_string()
                        == "displayed model mesh has not settled to the selected frame"
                })
            });
    let waiting = waiting || mesh_pending;
    if waiting && std::time::Instant::now() < pending.deadline {
        return;
    }
    let pending = world
        .remove_resource::<PendingRenderedActor>()
        .expect("inspected pending request");
    let result = (|| -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(
            world
                .get_resource::<State<cyrus_game::AppState>>()
                .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame),
            "rendered actor observation requires an active world"
        );
        anyhow::ensure!(
            !waiting,
            "rendered actor replacement generation wait expired"
        );
        let actors = world
            .get_resource::<cyrus_script::script::ScriptActors>()
            .and_then(|actors| actors.current.as_ref())
            .ok_or_else(|| anyhow::anyhow!("rendered actor: current world absent"))?;
        let actor = actors.id(pending.index)?;
        let model = cyrus_render::inspect_actor_model(world, actor)?;
        let frame = world
            .get_resource::<bevy::diagnostic::FrameCount>()
            .ok_or_else(|| anyhow::anyhow!("rendered actor observation: frame count absent"))?
            .0;
        let seconds = world
            .get_resource::<cyrus_script::clock::SimulationClock>()
            .ok_or_else(|| anyhow::anyhow!("rendered actor observation: game clock absent"))?
            .game_seconds();
        Ok(json!({"frame": frame, "game_seconds": seconds, "rendered_actor": model}))
    })();
    let response = match result {
        Ok(value) => Response::ok(pending.id, value),
        Err(error) => Response::err(pending.id, format!("{error:#}")),
    };
    let _ = pending.reply.send(response);
}
