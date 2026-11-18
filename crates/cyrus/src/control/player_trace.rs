//! Atomic logical placement and bounded observations of completed ordinary updates.
use super::*;
use cyrus_script::{clock::SimulationClock, script::ScriptActors};
use std::sync::mpsc::Sender;

#[derive(Resource)]
pub(super) struct PendingTrace {
    id: u64,
    reply: Sender<Response>,
    generation: u64,
    player: cyrus_script::script::ScriptActorId,
    first: u64,
    previous: u64,
    updates: u16,
    previous_scale: u64,
    normal_frame_rate: u32,
    observations: Vec<serde_json::Value>,
    deadline: std::time::Instant,
}

fn snapshot(
    world: &cyrus_script::script::ScriptWorld,
    clock: &SimulationClock,
    first: u64,
) -> anyhow::Result<serde_json::Value> {
    let player = world
        .selected_player()
        .ok_or_else(|| anyhow::anyhow!("player trace: selected player absent"))?;
    Ok(json!({
        "after_updates": world.ordinary_updates() - first,
        "update": world.ordinary_updates(), "generation": world.generation(),
        "elapsed_scale": clock.elapsed_frame_scale(),
        "game_seconds": clock.game_seconds(),
        "normal_frame_rate": clock.settings().normal_frame_rate,
        "player": world.inspect(player.index, 1)["actors"][0],
    }))
}

pub(super) fn start(
    world: &mut World,
    id: u64,
    reply: Sender<Response>,
    position: [i32; 3],
    heading: u16,
    updates: u16,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=255).contains(&updates),
        "player trace: updates must be 1..=255"
    );
    anyhow::ensure!(
        !world.contains_resource::<PendingTrace>(),
        "player trace already pending"
    );
    anyhow::ensure!(
        !world.resource::<input_schedule::InputScheduler>().pending(),
        "player trace: input schedule pending"
    );
    anyhow::ensure!(
        world.contains_resource::<SimulationClock>(),
        "player trace: game clock absent"
    );
    let (generation, player, first) = {
        let mut actors = world
            .get_resource_mut::<ScriptActors>()
            .ok_or_else(|| anyhow::anyhow!("player trace: script world absent"))?;
        let current = actors
            .current
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("player trace: script world absent"))?;
        current.place_selected_player(position.map(|v| v.wrapping_mul(256)), heading)?;
        (
            current.generation(),
            current
                .selected_player()
                .ok_or_else(|| anyhow::anyhow!("player trace: selected player absent"))?,
            current.ordinary_updates(),
        )
    };
    let current = world.resource::<ScriptActors>().current.as_ref().unwrap();
    let initial = snapshot(current, world.resource::<SimulationClock>(), first)?;
    world.insert_resource(PendingTrace {
        id,
        reply,
        generation,
        player,
        first,
        previous: first,
        updates,
        previous_scale: world.resource::<SimulationClock>().elapsed_frame_scale(),
        normal_frame_rate: world
            .resource::<SimulationClock>()
            .settings()
            .normal_frame_rate,
        observations: vec![initial],
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
    });
    Ok(())
}

pub(super) fn respond(world: &mut World) {
    let Some(mut pending) = world.remove_resource::<PendingTrace>() else {
        return;
    };
    let result = (|| -> anyhow::Result<bool> {
        anyhow::ensure!(
            std::time::Instant::now() < pending.deadline,
            "player trace: deadline expired"
        );
        let current = world
            .get_resource::<ScriptActors>()
            .and_then(|actors| actors.current.as_ref())
            .ok_or_else(|| anyhow::anyhow!("player trace: script world retired"))?;
        anyhow::ensure!(
            current.generation() == pending.generation
                && current.selected_player() == Some(pending.player),
            "player trace: generation or selected player changed"
        );
        let update = current.ordinary_updates();
        if update == pending.previous {
            return Ok(false);
        }
        anyhow::ensure!(
            pending.previous.checked_add(1) == Some(update),
            "player trace: ordinary update skipped or restarted"
        );
        let clock = world
            .get_resource::<SimulationClock>()
            .ok_or_else(|| anyhow::anyhow!("player trace: game clock absent"))?;
        anyhow::ensure!(
            clock.elapsed_frame_scale() > pending.previous_scale
                && clock.settings().normal_frame_rate == pending.normal_frame_rate,
            "player trace: clock did not advance or rate changed"
        );
        pending
            .observations
            .push(snapshot(current, clock, pending.first)?);
        pending.previous = update;
        pending.previous_scale = clock.elapsed_frame_scale();
        Ok(update - pending.first == u64::from(pending.updates))
    })();
    if matches!(result, Ok(false)) {
        world.insert_resource(pending);
        return;
    }
    let response = match result {
        Ok(_) => Response::ok(pending.id, json!({"observations":pending.observations})),
        Err(error) => Response::err(
            pending.id,
            format!("{error:#}; observations: {}", json!(pending.observations)),
        ),
    };
    let _ = pending.reply.send(response);
}
