//! Exclusive host requests after ordinary frame updates; storage stays host-owned.
use crate::{RuntimeSessionCandidate, RuntimeSessionSnapshot, SessionWorldAssets};
use anyhow::{Context, Result, ensure};
use bevy::prelude::{App, Assets, Handle, IntoScheduleConfigs, Last, Plugin, Resource, World};
use cyrus_assets::hud::HudAsset;

enum Request {
    CaptureLive,
    Capture(SessionWorldAssets, Handle<HudAsset>),
    Commit(Box<RuntimeSessionCandidate>),
    Load(Box<crate::loading::SessionLoad>),
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
        self.submit(Request::Load(Box::new(crate::loading::SessionLoad::new(
            snapshot, assets,
        ))))
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
        Request::CaptureLive => world
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
            .map(|snapshot| SessionBoundaryResult::Captured(Box::new(snapshot))),
        Request::Capture(sources, hud) => capture(world, &sources, &hud)
            .map(|snapshot| SessionBoundaryResult::Captured(Box::new(snapshot))),
        Request::Commit(candidate) => candidate
            .commit(world)
            .map(|()| SessionBoundaryResult::Committed),
        Request::Load(mut load) => {
            let result = match load.poll(world) {
                Ok(None) => {
                    world.resource_mut::<SessionBoundary>().request = Some(Request::Load(load));
                    return;
                }
                Ok(Some(candidate)) => candidate
                    .commit(world)
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
