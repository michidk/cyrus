//! Native off-live document preparation; shared boundary owns validation and commit.
use anyhow::{Context, Result, ensure};
use bevy::prelude::{AssetServer, Resource, World};
use cyrus_assets::{hud::HudAsset, install::GameInstall};
use cyrus_session::{
    RuntimeSessionSnapshot, SessionBoundary, SessionBoundaryResult, SessionLoadAssets,
    SessionWorldAssets,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Mutex, mpsc},
};

#[derive(Resource, Default)]
pub(crate) struct NativeSessionLoad {
    worker: Option<Mutex<mpsc::Receiver<Result<PreparedLoad>>>>,
    submitted: bool,
    result: Option<Result<()>>,
    path: String,
}

struct PreparedLoad {
    snapshot: RuntimeSessionSnapshot,
    worlds: BTreeMap<u32, String>,
    hud: HudAsset,
    samples: BTreeMap<String, String>,
}

impl NativeSessionLoad {
    pub(crate) fn start(&mut self, path: String, install: GameInstall) -> Result<()> {
        ensure!(
            self.worker.is_none() && !self.submitted && self.result.is_none(),
            "native session load already pending or result unread"
        );
        let requested_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("prepare session load".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = prepare(Path::new(&path), &install)
                    .with_context(|| format!("loading session {path}"));
                let _ = sender.send(result);
            })
            .context("starting native session preparation worker")?;
        self.worker = Some(Mutex::new(receiver));
        self.path = requested_path;
        Ok(())
    }

    pub(crate) fn busy(&self) -> bool {
        self.worker.is_some() || self.submitted
    }

    pub(crate) fn completion(&self) -> Option<std::result::Result<(), String>> {
        self.result.as_ref().map(|result| {
            result
                .as_ref()
                .map(|()| ())
                .map_err(|error| format!("loading session {}: {error:#}", self.path))
        })
    }

    pub(crate) fn acknowledge(&mut self) -> Result<()> {
        ensure!(
            self.result.is_some(),
            "native session load has no terminal result"
        );
        self.result = None;
        self.path.clear();
        Ok(())
    }
}

pub(crate) fn poll(world: &mut World) {
    let Some(mut state) = world.remove_resource::<NativeSessionLoad>() else {
        return;
    };
    if state.submitted {
        if let Some(result) = world.resource_mut::<SessionBoundary>().take_result() {
            state.submitted = false;
            state.result = Some(result.and_then(|result| match result {
                SessionBoundaryResult::Committed => Ok(()),
                SessionBoundaryResult::Captured(_) => {
                    anyhow::bail!("native load received a capture result")
                }
            }));
        }
    } else if let Some(worker) = &state.worker {
        let result = match worker.lock().unwrap().try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "native session preparation worker disconnected"
            ))),
        };
        if let Some(result) = result {
            state.worker = None;
            match result.and_then(|prepared| submit(prepared, world)) {
                Ok(()) => state.submitted = true,
                Err(error) => state.result = Some(Err(error)),
            }
        }
    }
    world.insert_resource(state);
}

fn prepare(path: &Path, install: &GameInstall) -> Result<PreparedLoad> {
    let bytes = crate::storage::read(path)?.context("session document does not exist")?;
    let snapshot = RuntimeSessionSnapshot::decode_document(&bytes)?;
    let current_name = snapshot.current_scene_name();
    ensure!(
        cyrus_assets::scene::world_names(install)?
            .iter()
            .any(|name| name.eq_ignore_ascii_case(current_name)),
        "session current scene {current_name:?} is not an installed selection"
    );
    ensure!(
        cyrus_assets::scene::world_index(install, current_name)? == snapshot.current_world(),
        "session current scene/world identity differs"
    );
    let cache = crate::settings::Settings::cache_dir();
    let mut worlds = BTreeMap::new();
    for id in snapshot.world_ids() {
        let name = if id == snapshot.current_world() {
            current_name.to_ascii_uppercase()
        } else {
            id.to_string()
        };
        ensure!(
            cyrus_assets::scene::world_index(install, &name)? == id,
            "session world {id}: installed identity differs"
        );
        cyrus_assets::scene::export_scene(install, &name, &cache.join(format!("{name}.glb")))?;
        worlds.insert(id, name);
    }
    cyrus_assets::hud_export::export_hud(install, &cache)?;
    cyrus_assets::sound_export::export(install, &cache)?;
    let mut audio_paths = Vec::new();
    let bank: cyrus_assets::sound::SoundBank =
        serde_json::from_slice(&std::fs::read(cache.join("sounds/bank.sounds"))?)?;
    for index in 0..bank.effects.len() {
        audio_paths.push(format!("sounds/{index:03}.wav"));
    }
    for label in snapshot.dialogue().required_labels() {
        cyrus_assets::sound::dialogue::export::export(install, &cache, label)?;
        let key = label
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let entry: cyrus_assets::sound::DialogueEntry = serde_json::from_slice(&std::fs::read(
            cache.join(format!("dialogue/{key}.dialogue")),
        )?)?;
        if let Some(voice) = entry.voice {
            audio_paths.push(voice.audio);
        }
    }
    if let Some(track) = snapshot.music().track() {
        let cue = cyrus_assets::music::cue_path(install).context("session CD image absent")?;
        let bytes = cyrus_assets::music::read_track(&cue, track).map_err(anyhow::Error::msg)?;
        std::fs::create_dir_all(cache.join("music"))?;
        std::fs::write(cache.join(format!("music/{track:02}.wav")), bytes)?;
    }
    let wanted = snapshot
        .sample_playback()
        .sources()
        .map(|source| source.source_identity.to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut samples = BTreeMap::new();
    for path in audio_paths {
        let bytes =
            std::fs::read(cache.join(&path)).with_context(|| format!("session audio {path}"))?;
        let identity = blake3::hash(&bytes).to_hex().to_string();
        if wanted.contains(&identity) {
            samples.insert(identity, path);
        }
    }
    ensure!(
        samples.len() == wanted.len(),
        "session sample source not found in prepared installed effects/dialogue"
    );
    let hud = serde_json::from_slice(&std::fs::read(cache.join("hud/overlay.hud"))?)?;
    Ok(PreparedLoad {
        snapshot,
        worlds,
        hud,
        samples,
    })
}

fn submit(prepared: PreparedLoad, world: &mut World) -> Result<()> {
    let server = world
        .get_resource::<AssetServer>()
        .context("native session asset server absent")?;
    let worlds = prepared
        .worlds
        .into_iter()
        .map(|(id, name)| {
            let mut prepared = cyrus_world::lifecycle::PreparedWorld::from_cache(name, server);
            prepared.world = Some(id);
            (id, prepared)
        })
        .collect();
    let assets = SessionLoadAssets {
        sources: SessionWorldAssets::request(&prepared.snapshot, worlds, server)?,
        hud: server.load("cache://hud/overlay.hud"),
        hud_images: prepared
            .snapshot
            .has_hud_presentation()
            .then(|| cyrus_render::hud::HudPresentationAssets::request(&prepared.hud, server)),
        samples: prepared
            .samples
            .into_iter()
            .map(|(identity, path)| (identity, server.load(format!("cache://{path}"))))
            .collect(),
        dialogue: prepared
            .snapshot
            .dialogue()
            .required_labels()
            .into_iter()
            .map(|label| {
                let key = label
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                (
                    label,
                    server.load(format!("cache://dialogue/{key}.dialogue")),
                )
            })
            .collect(),
        music: prepared
            .snapshot
            .music()
            .track()
            .map(|track| server.load(format!("cache://music/{track:02}.wav"))),
    };
    world
        .get_resource_mut::<SessionBoundary>()
        .context("native session boundary absent")?
        .load(prepared.snapshot, assets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Name;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn malformed_document_finishes_with_path_context_without_replacing_live_state() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "cyrus-session-load-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        let path = root.join("malformed.cyrus");
        std::fs::write(&path, b"not a session document")?;
        let install = GameInstall {
            root: root.join("absent-install"),
        };
        let mut world = World::new();
        world.init_resource::<NativeSessionLoad>();
        world.init_resource::<SessionBoundary>();
        let live = world.spawn(Name::new("retained live session")).id();
        world
            .resource_mut::<NativeSessionLoad>()
            .start(path.display().to_string(), install.clone())?;
        assert!(
            world
                .resource_mut::<NativeSessionLoad>()
                .start(path.display().to_string(), install.clone())
                .is_err()
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while world.resource::<NativeSessionLoad>().busy() {
            ensure!(
                std::time::Instant::now() < deadline,
                "native load worker did not finish"
            );
            poll(&mut world);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            world
                .resource_mut::<NativeSessionLoad>()
                .start(path.display().to_string(), install)
                .is_err()
        );
        let first = world.resource::<NativeSessionLoad>().completion().unwrap();
        assert_eq!(
            world.resource::<NativeSessionLoad>().completion(),
            Some(first.clone())
        );
        let error = first.unwrap_err();
        world.resource_mut::<NativeSessionLoad>().acknowledge()?;
        assert!(world.resource::<NativeSessionLoad>().completion().is_none());
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(error.contains("format/version unsupported"), "{error}");
        assert_eq!(
            world.get::<Name>(live).unwrap().as_str(),
            "retained live session"
        );
        assert!(
            world
                .resource_mut::<SessionBoundary>()
                .take_result()
                .is_none()
        );
        assert_eq!(std::fs::read(&path)?, b"not a session document");
        std::fs::remove_file(path)?;
        std::fs::remove_dir(root)?;
        Ok(())
    }
}
