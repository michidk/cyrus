//! End-of-frame world exits and host-owned dispatch of retained script requests.
use super::{ScriptActors, WorldLoopOutcome};
use crate::player::placement::MarkerEntry;
use bevy::prelude::*;
use cyrus_world::lifecycle::{ReloadScene, SceneRequest};

/// End-of-frame exit arbitration precedes any replacement-session publication.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldLoopExit;

/// Destination of a `LoadWorld(world, marker, heading)` call. The heading is masked to 0 through
/// 2047 as preservation documents; the world index is validated against WORLD.INI by the scene
/// preparation that consumes the [`SceneRequest`], which reports failure as `SceneLoadError`.
fn destination(request: [i32; 3]) -> anyhow::Result<(SceneRequest, MarkerEntry)> {
    let [world, marker, heading] = request;
    anyhow::ensure!(
        (0..100).contains(&world),
        "LoadWorld: world {world} outside 0 through 99"
    );
    Ok((
        SceneRequest(world.to_string()),
        MarkerEntry {
            marker: usize::try_from(marker)
                .map_err(|_| anyhow::anyhow!("LoadWorld: negative marker {marker}"))?,
            heading: heading & 2047,
        },
    ))
}

/// Publishes a world load: the marker and heading first, then the request, plus a reload marker so a
/// request naming the current world is prepared again. Shared by the script native and the console.
pub(crate) fn request_world(commands: &mut Commands, request: [i32; 3]) -> anyhow::Result<()> {
    let (scene, marker) = destination(request)?;
    commands.insert_resource(marker);
    commands.insert_resource(scene);
    commands.insert_resource(ReloadScene);
    Ok(())
}

/// Runs after actor and presentation work. A pending world destination takes
/// priority without advancing the falling timer. Death dispatch remains explicit
/// until the last-save/menu owner is available; it never starts a fresh world.
pub(super) fn consume(
    mut actors: ResMut<ScriptActors>,
    mut commands: Commands,
    clock: Option<Res<crate::clock::SimulationClock>>,
    state: Option<Res<State<crate::AppState>>>,
    mut checked: Local<Option<(u64, u32, u32)>>,
) {
    if actors.error.is_some() || state.is_some_and(|state| *state.get() != crate::AppState::InGame)
    {
        return;
    }
    let Some(world) = actors.current.as_mut() else {
        return;
    };
    let identity = (
        world.generation(),
        world.world,
        world.globals().world_loop_frame,
    );
    let transition = world.globals_mut().request_mut().load_world;
    if *checked == Some(identity) && transition.is_none() {
        return;
    }
    let first = checked.is_none();
    *checked = Some(identity);
    if first && identity.2 == 0 && transition.is_none() {
        return;
    }
    // World preparation can precede the standalone-player initializer.
    if transition.is_none() && world.globals().selected_health().is_none() {
        return;
    }
    let result = if transition.is_some() {
        world.finish_world_loop(0)
    } else {
        anyhow::Context::context(clock.as_ref(), "world-loop clock unavailable")
            .and_then(|clock| world.finish_world_loop(clock.step().nominal_ticks))
    };
    let world_id = world.world;
    match result {
        Ok(WorldLoopOutcome::Continue) => {
            actors.world_loop_outcome = None;
        }
        Ok(WorldLoopOutcome::WorldTransition(destination)) => {
            world.globals_mut().request_mut().load_world = None;
            if let Err(error) = request_world(&mut commands, destination) {
                actors.report_runtime_error(format!("{error:#}"));
            }
        }
        Ok(outcome) => {
            actors.world_loop_outcome = Some(outcome);
            let service = match outcome {
                WorldLoopOutcome::Death => "last-save/menu dispatcher",
                _ => "win dispatcher",
            };
            actors.report_runtime_error(format!(
                "world {world_id} {outcome:?} world-loop outcome: {service} unavailable"
            ));
        }
        Err(error) => actors.report_runtime_error(format!("{error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_masks_heading_and_bounds_the_world() {
        let (scene, marker) = destination([7, 11, 2048 + 5]).unwrap();
        assert_eq!(scene.0, "7");
        assert_eq!(
            marker,
            MarkerEntry {
                marker: 11,
                heading: 5
            }
        );
        assert_eq!(destination([0, 0, -1]).unwrap().1.heading, 2047);
        assert!(destination([100, 0, 0]).is_err());
        assert!(destination([-1, 0, 0]).is_err());
        assert!(destination([1, -1, 0]).is_err());
    }
}
