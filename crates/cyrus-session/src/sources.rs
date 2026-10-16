//! Host-resolved world sources retained independently of live world-entry resources.
use crate::RuntimeSessionSnapshot;
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::LoadState,
    prelude::{AssetServer, Assets, Handle, Resource, World},
};
use cyrus_assets::{animation::Animations, script::Scripts};
use cyrus_game::{player::data::PlayerScene, script::WorldSnapshotSources};
use cyrus_world::lifecycle::PreparedWorld;
use std::collections::{BTreeMap, BTreeSet};

/// Retained cache references for worlds observed at a settled gameplay boundary.
/// These are source assets only, not dormant gameplay or audio services.
#[derive(Resource, Default)]
pub struct SessionSourceCatalog {
    worlds: BTreeMap<u32, (PreparedWorld, Handle<PlayerScene>)>,
}

impl SessionSourceCatalog {
    /// Retain host-prepared sources, including visited worlds prepared for a load.
    /// Decoded identity and saved content are still validated during capture/plan.
    pub fn retain(
        &mut self,
        world: u32,
        prepared: PreparedWorld,
        player: Handle<PlayerScene>,
    ) -> Result<()> {
        ensure!(
            prepared.world.is_none_or(|id| id == world),
            "session source catalog world {world}: prepared identity differs"
        );
        self.worlds.insert(world, (prepared, player));
        Ok(())
    }

    pub fn for_capture(&self, world: &World) -> Result<SessionWorldAssets> {
        let actors = world
            .get_resource::<cyrus_game::script::ScriptActors>()
            .context("session capture registry absent")?;
        let current = actors
            .current
            .as_ref()
            .context("session capture current world absent")?
            .world;
        let prepared = std::iter::once(current)
            .chain(actors.visited_worlds().keys().copied())
            .map(|id| {
                self.worlds
                    .get(&id)
                    .cloned()
                    .with_context(|| format!("session capture world {id}: retained sources absent"))
                    .map(|prepared| (id, prepared))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(SessionWorldAssets {
            current,
            worlds: prepared,
        })
    }
}

pub(crate) fn retain_settled_sources(world: &mut World) {
    let Some(current) = world
        .get_resource::<cyrus_game::script::ScriptActors>()
        .and_then(|actors| actors.current.as_ref())
        .map(|current| current.world)
    else {
        return;
    };
    let Some(prepared) = world.get_resource::<PreparedWorld>() else {
        return;
    };
    let Some(scene) = world.get_resource::<cyrus_world::lifecycle::CurrentScene>() else {
        return;
    };
    let Some(request) = world.get_resource::<cyrus_world::lifecycle::SceneRequest>() else {
        return;
    };
    if scene.world != Some(current)
        || !request.0.eq_ignore_ascii_case(&scene.name)
        || !prepared.requested_name.eq_ignore_ascii_case(&scene.name)
        || world.contains_resource::<cyrus_world::lifecycle::WorldPreparationPending>()
    {
        return;
    }
    let Some(scripts) = world
        .get_resource::<Assets<Scripts>>()
        .and_then(|assets| assets.get(&prepared.scripts))
    else {
        return;
    };
    if scripts.world != current || prepared.world.is_some_and(|id| id != current) {
        return;
    }
    let Some(player) = world.get_resource::<cyrus_game::player::data::PlayerSceneRequest>() else {
        return;
    };
    let Some(decoded) = world
        .get_resource::<Assets<PlayerScene>>()
        .and_then(|assets| assets.get(&player.0))
    else {
        return;
    };
    if decoded.world != current {
        return;
    }
    let player = player.0.clone();
    let prepared = prepared.clone();
    world
        .resource_mut::<SessionSourceCatalog>()
        .worlds
        .insert(current, (prepared, player));
}

/// Keeps all current and visited world sidecars alive during off-live preparation.
/// Hosts resolve IDs against their installed world catalog and prepare cache files
/// first. Construction loads player sidecars but publishes no lifecycle resource.
#[derive(Clone)]
pub struct SessionWorldAssets {
    current: u32,
    worlds: BTreeMap<u32, (PreparedWorld, Handle<PlayerScene>)>,
}

impl SessionWorldAssets {
    /// Installed cache references for every represented world, without live entry.
    pub fn prepared_worlds(&self) -> impl Iterator<Item = (u32, &PreparedWorld)> {
        self.worlds
            .iter()
            .map(|(&id, (prepared, _))| (id, prepared))
    }
    /// Adopt already loaded current and visited handles without requesting assets.
    /// Decoded content is checked against the logical plan during assembly.
    pub fn from_loaded(
        snapshot: &RuntimeSessionSnapshot,
        worlds: BTreeMap<u32, (PreparedWorld, Handle<PlayerScene>)>,
    ) -> Result<Self> {
        ensure!(
            worlds.keys().copied().collect::<BTreeSet<_>>()
                == snapshot.world_ids().collect::<BTreeSet<_>>(),
            "session loaded sources must match the saved world census"
        );
        for (&id, (prepared, _)) in &worlds {
            ensure!(
                prepared.world.is_none_or(|world| world == id),
                "session loaded source world {id}: prepared identity differs"
            );
        }
        Ok(Self {
            current: snapshot.current_world(),
            worlds,
        })
    }

    pub(crate) fn into_catalog(self) -> SessionSourceCatalog {
        SessionSourceCatalog {
            worlds: self.worlds,
        }
    }

    pub fn request(
        snapshot: &RuntimeSessionSnapshot,
        prepared: BTreeMap<u32, PreparedWorld>,
        assets: &AssetServer,
    ) -> Result<Self> {
        Self::load(
            snapshot.current_world(),
            snapshot.world_ids().collect(),
            prepared,
            assets,
        )
    }

    /// Prepare capture sources without requiring an existing saved document.
    pub fn for_capture(
        actors: &cyrus_game::script::ScriptActors,
        prepared: BTreeMap<u32, PreparedWorld>,
        assets: &AssetServer,
    ) -> Result<Self> {
        let current = actors
            .current
            .as_ref()
            .context("session capture source: current world absent")?
            .world;
        let ids = std::iter::once(current)
            .chain(actors.visited_worlds().keys().copied())
            .collect();
        Self::load(current, ids, prepared, assets)
    }

    fn load(
        current: u32,
        ids: Vec<u32>,
        prepared: BTreeMap<u32, PreparedWorld>,
        assets: &AssetServer,
    ) -> Result<Self> {
        let unique: BTreeSet<_> = ids.iter().copied().collect();
        ensure!(
            ids.len() == unique.len() && unique.contains(&current),
            "session source request has duplicate worlds or no current world"
        );
        ensure!(
            prepared.keys().copied().collect::<BTreeSet<_>>() == unique,
            "session source request must prepare exactly the saved world census"
        );
        for (&id, world) in &prepared {
            ensure!(
                world.world.is_none_or(|resolved| resolved == id),
                "session source world {id}: host resolved another world"
            );
        }
        Ok(Self {
            current,
            worlds: prepared
                .into_iter()
                .map(|(id, world)| {
                    let player = assets.load(world.player_path.clone());
                    (id, (world, player))
                })
                .collect(),
        })
    }

    /// `None` means at least one source is still loading. Failures identify its world
    /// and owner. Complete results still require the snapshot's exact source checks.
    pub fn sources<'a>(
        &self,
        world: &'a World,
    ) -> Result<Option<BTreeMap<u32, WorldSnapshotSources<'a>>>> {
        let server = world
            .get_resource::<AssetServer>()
            .context("session source asset server absent")?;
        let scripts = world
            .get_resource::<Assets<Scripts>>()
            .context("session script assets absent")?;
        let animations = world
            .get_resource::<Assets<Animations>>()
            .context("session animation assets absent")?;
        let players = world
            .get_resource::<Assets<PlayerScene>>()
            .context("session player assets absent")?;
        let mut result = BTreeMap::new();
        let mut pending = false;
        for (&id, (prepared, player)) in &self.worlds {
            for (owner, handle) in [
                ("scripts", prepared.scripts.id().untyped()),
                ("animations", prepared.animations.id().untyped()),
                ("player", player.id().untyped()),
            ] {
                if let Some(LoadState::Failed(error)) = server.get_load_state(handle) {
                    anyhow::bail!("session source world {id} {owner}: {error}");
                }
            }
            let (Some(scripts), Some(animations), Some(player)) = (
                scripts.get(&prepared.scripts),
                animations.get(&prepared.animations),
                players.get(player),
            ) else {
                pending = true;
                continue;
            };
            ensure!(
                scripts.world == id && animations.world == id && player.world == id,
                "session source world {id}: decoded owner identity differs"
            );
            result.insert(
                id,
                WorldSnapshotSources {
                    player: Some(player),
                    scripts,
                    animations,
                    collision: &player.collision,
                },
            );
        }
        Ok((!pending).then_some(result))
    }

    /// Validated-source handles for current-world entity and service preparation.
    /// Readiness and installed compatibility are established separately by sources/plan.
    pub fn current(&self) -> (&PreparedWorld, &Handle<PlayerScene>) {
        let (prepared, player) = &self.worlds[&self.current];
        (prepared, player)
    }

    /// Transfer prepared handles after all source borrows and candidate construction.
    /// This does not publish or initialize any world.
    pub fn into_worlds(self) -> BTreeMap<u32, (PreparedWorld, Handle<PlayerScene>)> {
        self.worlds
    }
}

/// Content accepted by the logical plan, rechecked against retained asset handles.
pub(crate) fn identities(
    sources: &BTreeMap<u32, WorldSnapshotSources<'_>>,
) -> Result<BTreeMap<u32, String>> {
    sources
        .iter()
        .map(|(&id, source)| {
            ensure!(
                source.player.is_some(),
                "session source world {id}: player absent"
            );
            Ok((id, source.identity()?))
        })
        .collect()
}

pub(crate) fn validate_retained(
    assets: &SessionWorldAssets,
    expected: &BTreeMap<u32, String>,
    world: &World,
) -> Result<()> {
    let sources = assets
        .sources(world)?
        .context("session retained sources still loading")?;
    ensure!(
        identities(&sources)? == *expected,
        "session retained source content differs from validated logical plan"
    );
    Ok(())
}
