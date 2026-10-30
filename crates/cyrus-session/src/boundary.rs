//! Exclusive host requests after ordinary frame updates; storage stays host-owned.
use crate::{RuntimeSessionCandidate, RuntimeSessionSnapshot, SessionWorldAssets};
use anyhow::{Context, Result, ensure};
use bevy::prelude::{App, Assets, Handle, IntoScheduleConfigs, Last, Plugin, Resource, World};
use cyrus_assets::hud::HudAsset;

type StagedMetadata = (
    Option<&'static bevy::prelude::WorldAssetRoot>,
    bevy::prelude::Has<cyrus_render::persistence::StagedRenderRoot>,
    bevy::prelude::Has<bevy::prelude::Camera>,
);

/// Read-only staging census. Counts are evaluated only when `snapshot` is called;
/// inspecting does not advance preparation or consume the boundary result.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SessionBoundaryInspection<'w, 's> {
    boundary: Option<bevy::prelude::Res<'w, SessionBoundary>>,
    server: Option<bevy::prelude::Res<'w, bevy::prelude::AssetServer>>,
    staged: bevy::prelude::Query<
        'w,
        's,
        StagedMetadata,
        bevy::prelude::With<crate::staging::StagedSessionEntity>,
    >,
    cameras: bevy::prelude::Query<
        'w,
        's,
        &'static bevy::prelude::Camera,
        bevy::prelude::With<cyrus_render::camera::ViewCamera>,
    >,
}

/// Diagnostic metadata only. Render roots exist after source resolution and before
/// logical planning; their presence does not mean the replacement can publish.
#[derive(Debug, serde::Serialize)]
pub struct SessionBoundarySnapshot {
    pub operation: &'static str,
    pub staged_entities: usize,
    pub staged_render_roots: usize,
    pub staged_cameras: usize,
    pub active_live_cameras: usize,
    pub assets: Vec<SessionStagedAsset>,
}

/// Existing staged handle identity and the asset server's current load state.
#[derive(Debug, serde::Serialize)]
pub struct SessionStagedAsset {
    pub path: Option<String>,
    pub state: String,
}

impl SessionBoundaryInspection<'_, '_> {
    /// Return metadata only for an explicit opt-in. Disabled inspection performs
    /// no entity iteration or asset-state lookup.
    pub fn snapshot(&self, requested: bool) -> Option<SessionBoundarySnapshot> {
        if !requested {
            return None;
        }
        let operation = match self.boundary.as_deref() {
            Some(SessionBoundary {
                request: Some(Request::Load(..)),
                ..
            }) => "load",
            Some(SessionBoundary {
                request: Some(Request::Commit(..)),
                ..
            }) => "commit",
            Some(SessionBoundary {
                request: Some(_), ..
            }) => "capture",
            Some(SessionBoundary {
                result: Some(_), ..
            }) => "result",
            _ => "idle",
        };
        let mut snapshot = SessionBoundarySnapshot {
            operation,
            staged_entities: 0,
            staged_render_roots: 0,
            staged_cameras: 0,
            active_live_cameras: self
                .cameras
                .iter()
                .filter(|camera| camera.is_active)
                .count(),
            assets: Vec::new(),
        };
        for (asset, render_root, camera) in &self.staged {
            snapshot.staged_entities += 1;
            snapshot.staged_render_roots += usize::from(render_root);
            snapshot.staged_cameras += usize::from(camera);
            if let Some(asset) = asset {
                snapshot.assets.push(SessionStagedAsset {
                    path: self
                        .server
                        .as_ref()
                        .and_then(|server| server.get_path(asset.0.id()))
                        .map(|path| path.to_string()),
                    state: self
                        .server
                        .as_ref()
                        .map(|server| format!("{:?}", server.load_state(asset.0.id())))
                        .unwrap_or_else(|| "Unavailable".into()),
                });
            }
        }
        snapshot.assets.sort_by(|a, b| a.path.cmp(&b.path));
        Some(snapshot)
    }
}

enum Request {
    CaptureLive,
    Capture(SessionWorldAssets, Handle<HudAsset>),
    Commit(Box<RuntimeSessionCandidate>),
    Load(Box<crate::loading::SessionLoad>, bool),
}

/// One outstanding operation, including its unread result. Callers prepare sources
/// off-live and consume the result before submitting another operation.
#[derive(Resource, Default)]
pub struct SessionBoundary {
    request: Option<Request>,
    result: Option<Result<SessionBoundaryResult>>,
}

/// Successful end-of-frame operation. Encoding and storage are separate host work.
pub enum SessionBoundaryResult {
    Captured(Box<RuntimeSessionSnapshot>),
    Committed,
}

impl SessionBoundary {
    /// True only without a pending operation or an unread result. Host adapters
    /// must not admit competing work that could consume another owner's result.
    pub fn is_idle(&self) -> bool {
        self.request.is_none() && self.result.is_none()
    }

    /// Capture the current and visited census using retained host source handles.
    /// Missing historical sources reject; no world is initialized to fill a gap.
    pub fn capture_live(&mut self) -> Result<()> {
        self.submit(Request::CaptureLive)
    }
    fn submit(&mut self, request: Request) -> Result<()> {
        ensure!(
            self.request.is_none() && self.result.is_none(),
            "session boundary already has a pending operation or unread result"
        );
        self.request = Some(request);
        Ok(())
    }

    /// Sources and HUD must already be loaded. Lifecycle validation runs at the
    /// boundary. This does not pause decoder threads or establish save eligibility.
    pub fn capture(&mut self, sources: SessionWorldAssets, hud: Handle<HudAsset>) -> Result<()> {
        self.submit(Request::Capture(sources, hud))
    }

    /// Submit a fully prepared replacement. Publication revalidates against the
    /// live world at execution time and rejects stale candidates before mutation.
    pub fn commit(&mut self, candidate: RuntimeSessionCandidate) -> Result<()> {
        self.submit(Request::Commit(Box::new(candidate)))
    }

    /// Prepare host-resolved installed assets over subsequent frames, then validate
    /// and publish once at the exclusive boundary. Async storage reads complete
    /// before submission; pending preparation is not a successful load result.
    pub fn load(
        &mut self,
        snapshot: RuntimeSessionSnapshot,
        assets: crate::SessionLoadAssets,
    ) -> Result<()> {
        self.submit(Request::Load(
            Box::new(crate::loading::SessionLoad::new(snapshot, assets)),
            false,
        ))
    }

    /// Load an original-style saved game. Sample/dialogue services are cleared;
    /// the live CD transport continues and world setup selects a new track only
    /// when the saved world differs. Diagnostic `load` retains exact checkpoints.
    pub fn load_saved_game(
        &mut self,
        snapshot: RuntimeSessionSnapshot,
        assets: crate::SessionLoadAssets,
    ) -> Result<()> {
        self.submit(Request::Load(
            Box::new(crate::loading::SessionLoad::new(snapshot, assets)),
            true,
        ))
    }

    pub fn take_result(&mut self) -> Option<Result<SessionBoundaryResult>> {
        self.result.take()
    }
}

/// Registered by both game hosts. Runs exclusively after Update/PostUpdate and
/// their deferred commands; no simulation system intervenes during publication.
pub struct SessionBoundaryPlugin;

impl Plugin for SessionBoundaryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionBoundary>()
            .init_resource::<crate::SessionSourceCatalog>()
            .add_systems(Last, execute.after(cyrus_game::script::WorldLoopExit));
    }
}

fn execute(world: &mut World) {
    crate::sources::retain_settled_sources(world);
    let Some(request) = world.resource_mut::<SessionBoundary>().request.take() else {
        return;
    };
    let result = match request {
        Request::CaptureLive => {
            cyrus_audio::sound::persistence::SamplePlaybackSnapshot::reclaim_finished(world);
            world
                .resource::<crate::SessionSourceCatalog>()
                .for_capture(world)
                .and_then(|sources| {
                    let sources = sources
                        .sources(world)?
                        .context("session capture sources still loading")?;
                    let hud = cyrus_render::hud::bound_asset(world)
                        .context("session capture bound HUD asset absent")?;
                    RuntimeSessionSnapshot::capture(world, &sources, hud)
                })
                .map(|snapshot| SessionBoundaryResult::Captured(Box::new(snapshot)))
        }
        Request::Capture(sources, hud) => {
            cyrus_audio::sound::persistence::SamplePlaybackSnapshot::reclaim_finished(world);
            capture(world, &sources, &hud)
                .map(|snapshot| SessionBoundaryResult::Captured(Box::new(snapshot)))
        }
        Request::Commit(candidate) => candidate
            .commit(world)
            .map(|()| SessionBoundaryResult::Committed),
        Request::Load(mut load, saved_game) => {
            let result = match load.poll(world) {
                Ok(None) => {
                    world.resource_mut::<SessionBoundary>().request =
                        Some(Request::Load(load, saved_game));
                    return;
                }
                Ok(Some(candidate)) => candidate
                    .publish(world, saved_game)
                    .map(|()| SessionBoundaryResult::Committed),
                Err(error) => Err(error),
            };
            load.discard(world);
            result
        }
    };
    world.resource_mut::<SessionBoundary>().result = Some(result);
}

fn capture(
    world: &World,
    sources: &SessionWorldAssets,
    hud: &Handle<HudAsset>,
) -> Result<RuntimeSessionSnapshot> {
    let sources = sources
        .sources(world)?
        .context("session capture sources still loading")?;
    let hud = world
        .get_resource::<Assets<HudAsset>>()
        .and_then(|assets| assets.get(hud))
        .context("session capture HUD asset absent")?;
    RuntimeSessionSnapshot::capture(world, &sources, hud)
}

#[cfg(test)]
mod inspection_tests {
    use super::*;
    use bevy::ecs::system::SystemState;

    #[test]
    fn inspection_preserves_pending_request_and_unread_rejection() {
        let mut app = App::new();
        app.add_plugins(SessionBoundaryPlugin);
        let mut inspection = SystemState::<SessionBoundaryInspection>::new(app.world_mut());
        assert!(
            inspection
                .get(app.world())
                .unwrap()
                .snapshot(false)
                .is_none()
        );
        assert_eq!(
            inspection
                .get(app.world())
                .unwrap()
                .snapshot(true)
                .unwrap()
                .operation,
            "idle"
        );
        app.world_mut()
            .resource_mut::<SessionBoundary>()
            .capture_live()
            .unwrap();
        assert_eq!(
            inspection
                .get(app.world())
                .unwrap()
                .snapshot(true)
                .unwrap()
                .operation,
            "capture"
        );
        assert!(!app.world().resource::<SessionBoundary>().is_idle());
        app.update();
        assert_eq!(
            inspection
                .get(app.world())
                .unwrap()
                .snapshot(true)
                .unwrap()
                .operation,
            "result"
        );
        assert!(
            app.world_mut()
                .resource_mut::<SessionBoundary>()
                .take_result()
                .unwrap()
                .is_err()
        );
        assert_eq!(
            inspection
                .get(app.world())
                .unwrap()
                .snapshot(true)
                .unwrap()
                .operation,
            "idle"
        );
    }
}
