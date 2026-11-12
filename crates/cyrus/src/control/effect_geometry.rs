//! Bounded read-only transient geometry acquisition after presentation publication.
use bevy::prelude::*;
use cyrus_control::Response;
use std::sync::mpsc::Sender;

#[derive(Resource)]
pub(super) struct PendingEffectGeometry {
    pub id: u64,
    pub after_generation: Option<u64>,
    pub reply: Sender<Response>,
    pub deadline: std::time::Instant,
}

pub(super) fn respond(world: &mut World) {
    let Some(pending) = world.get_resource::<PendingEffectGeometry>() else {
        return;
    };
    let generation = world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .and_then(|actors| actors.current.as_ref())
        .map(|current| current.generation());
    let ready = world
        .get_resource::<cyrus_render::ReadyScene>()
        .zip(world.get_resource::<cyrus_world::lifecycle::PreparedWorld>())
        .is_some_and(|(ready, prepared)| ready.0 == prepared.animations);
    let revisit_pending = world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .is_some_and(cyrus_script::script::ScriptActors::has_pending_revisit);
    let admitted = generation.is_some_and(|generation| {
        world
            .get_resource::<cyrus_script::script::WorldClockSample>()
            .and_then(|sample| sample.presentation_step(generation))
            .is_some()
    });
    let waiting = pending.after_generation.is_some_and(|old| {
        generation.is_none_or(|new| new == old) || !ready || revisit_pending || !admitted
    });
    if waiting && std::time::Instant::now() < pending.deadline {
        return;
    }
    let pending = world.remove_resource::<PendingEffectGeometry>().unwrap();
    let result = if waiting {
        Err(anyhow::anyhow!(
            "effect geometry: replacement generation deadline expired"
        ))
    } else {
        cyrus_render::fog::inspect_effect_geometry(world)
    };
    let response = match result {
        Ok(geometry) => Response::ok(
            pending.id,
            serde_json::json!({
                "frame": world.get_resource::<bevy::diagnostic::FrameCount>().map(|frame| frame.0),
                "geometry": geometry,
            }),
        ),
        Err(error) => Response::err(pending.id, format!("{error:#}")),
    };
    let _ = pending.reply.send(response);
}
