//! One-shot acquisition after the renderer's Update material bindings.
use bevy::prelude::*;
use cyrus_control::Response;
use serde_json::json;
use std::sync::mpsc::Sender;

#[derive(Resource)]
pub(super) struct PendingLightImage {
    pub id: u64,
    pub name: String,
    pub primitive: usize,
    pub reply: Sender<Response>,
}

pub(super) fn respond(world: &mut World) {
    let Some(pending) = world.remove_resource::<PendingLightImage>() else {
        return;
    };
    let result = (|| -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(
            world
                .get_resource::<State<cyrus_game::AppState>>()
                .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame),
            "bound light observation requires an active world"
        );
        let image = cyrus_render::fog::BoundSceneLightImage::capture_named_primitive(
            world,
            &pending.name,
            pending.primitive,
        )?;
        let frame = world
            .get_resource::<bevy::diagnostic::FrameCount>()
            .ok_or_else(|| anyhow::anyhow!("bound light observation: frame count absent"))?
            .0;
        Ok(
            json!({"frame": frame, "name": pending.name, "primitive": pending.primitive, "bound_light_image": image}),
        )
    })();
    let response = match result {
        Ok(value) => Response::ok(pending.id, value),
        Err(error) => Response::err(pending.id, format!("{error:#}")),
    };
    let _ = pending.reply.send(response);
}
