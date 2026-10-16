//! Settled world identity and prepared host references for coordinated replacement.
use anyhow::{Context, Result, ensure};
use bevy::prelude::{Assets, World};
use cyrus_assets::script::Scripts;
use cyrus_world::lifecycle::{
    CurrentScene, PreparedWorld, SceneLoadError, SceneRequest, WorldPreparationPending,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct LifecycleSnapshot {
    version: u32,
    world: u32,
    request: String,
    pub(super) current_name: String,
    scripts: String,
    error: Option<String>,
}

/// Prepared host references validated against saved identity. Publication is owned
/// by the complete session commit, not by world-entry initialization.
pub struct LifecycleCandidate {
    pub prepared: PreparedWorld,
    pub current: CurrentScene,
    pub request: SceneRequest,
    pub error: Option<SceneLoadError>,
}

fn identity(scripts: &Scripts) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(scripts)?)
        .to_hex()
        .to_string())
}

impl LifecycleSnapshot {
    pub(super) fn capture(world: &World, id: u32, scripts: &Scripts) -> Result<Self> {
        ensure!(
            !world.contains_resource::<WorldPreparationPending>(),
            "session capture: world preparation is in flight"
        );
        let current = super::resource::<CurrentScene>(world, "current scene")?;
        let request = super::resource::<SceneRequest>(world, "scene request")?;
        let prepared = super::resource::<PreparedWorld>(world, "prepared world")?;
        ensure!(
            current.world == Some(id) && prepared.world.is_none_or(|world| world == id),
            "session capture: lifecycle world differs from logical world {id}"
        );
        ensure!(
            request.0.eq_ignore_ascii_case(&current.name)
                && prepared.requested_name.eq_ignore_ascii_case(&current.name),
            "session capture: scene request is not settled"
        );
        let actual = super::resource::<Assets<Scripts>>(world, "script assets")?
            .get(&prepared.scripts)
            .context("session capture: prepared scripts absent")?;
        let scripts = identity(scripts)?;
        ensure!(
            identity(actual)? == scripts,
            "session capture: prepared scripts differ from validated source"
        );
        Ok(Self {
            version: 1,
            world: id,
            request: request.0.clone(),
            current_name: current.name.clone(),
            scripts,
            error: world
                .get_resource::<SceneLoadError>()
                .map(|error| error.0.clone()),
        })
    }

    pub(super) fn restore(
        self,
        prepared: PreparedWorld,
        world: &World,
    ) -> Result<LifecycleCandidate> {
        ensure!(
            self.version == 1,
            "session lifecycle snapshot version unsupported"
        );
        ensure!(
            self.request.eq_ignore_ascii_case(&self.current_name)
                && prepared
                    .requested_name
                    .eq_ignore_ascii_case(&self.current_name)
                && prepared.world.is_none_or(|id| id == self.world),
            "session lifecycle replacement request/world differs"
        );
        let scripts = super::resource::<Assets<Scripts>>(world, "prepared scripts")?
            .get(&prepared.scripts)
            .context("session lifecycle replacement scripts absent")?;
        ensure!(
            scripts.world == self.world && identity(scripts)? == self.scripts,
            "session lifecycle replacement script source differs"
        );
        Ok(LifecycleCandidate {
            prepared,
            current: CurrentScene {
                name: self.current_name,
                world: Some(self.world),
            },
            request: SceneRequest(self.request),
            error: self.error.map(SceneLoadError),
        })
    }
}
