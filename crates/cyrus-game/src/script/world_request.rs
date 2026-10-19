//! Consumes the `LoadWorld` session request raised by scripts.
use super::ScriptActors;
use crate::player::placement::MarkerEntry;
use bevy::prelude::*;
use cyrus_world::lifecycle::{ReloadScene, SceneRequest};

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

/// Runs after the ordinary script step so the request is acted on after the current script step.
/// Only `load_world` is taken; the other session requests belong to their own consumers.
pub(super) fn consume(mut actors: ResMut<ScriptActors>, mut commands: Commands) {
    let Some(world) = actors.current.as_mut() else {
        return;
    };
    let Some(request) = world.globals_mut().request_mut().load_world.take() else {
        return;
    };
    match destination(request) {
        Ok((scene, marker)) => {
            commands.insert_resource(marker);
            commands.insert_resource(scene);
            commands.insert_resource(ReloadScene);
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
