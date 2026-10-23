//! Native off-live document preparation; shared boundary owns validation and commit.
pub(crate) mod slots;

use anyhow::{Context, Result, ensure};
use bevy::prelude::{AssetServer, Local, Resource, World};
use cyrus_assets::{hud::HudAsset, install::GameInstall};
use cyrus_control::SlotOperation;
use cyrus_session::{
    RuntimeSessionSnapshot, SessionBoundary, SessionBoundaryResult, SessionLoadAssets,
    SessionWorldAssets,
};
use serde_json::{Value, json};
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
    slot_result: Option<Result<Value>>,
    slot_worker: Option<Mutex<mpsc::Receiver<Result<Value>>>>,
    capture: Option<slots::SavePlan>,
    slot_load: bool,
    last_game: bool,
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
            !self.busy() && self.result.is_none() && self.slot_result.is_none(),
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
        self.worker.is_some()
            || self.submitted
            || self.slot_worker.is_some()
            || self.capture.is_some()
    }

    pub(crate) fn completion(&self) -> Option<std::result::Result<(), String>> {
        self.result.as_ref().map(|result| {
            result
                .as_ref()
                .copied()
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

/// `--load-slot`: restore the newest revision of this slot once the startup book is ready.
#[derive(Resource)]
pub(crate) struct LaunchSlot(pub u16);

/// Submits the launch slot through the same host as the control command, then waits for its
/// commit. A failure is fatal: the player asked for that save, not the book.
pub(crate) fn launch_slot(world: &mut World, mut requested: Local<bool>) {
    let Some(&LaunchSlot(slot)) = world.get_resource::<LaunchSlot>() else {
        return;
    };
    if !*requested {
        if !world
            .get_resource::<cyrus_startup::MenuState>()
            .is_some_and(|menu| menu.ready)
        {
            return;
        }
        *requested = true;
        let metadata = world.resource::<slots::SlotStore>().metadata(slot, None);
        let submitted = metadata.and_then(|metadata| {
            request_slots(
                world,
                SlotOperation::Load {
                    slot,
                    revision: metadata.revision,
                },
            )
        });
        if let Err(error) = submitted {
            fail_launch_slot(world, slot, &error);
        }
        return;
    }
    match slots_result(world, false) {
        Ok(value) if value["pending"] == true => {}
        result => {
            let _ = slots_result(world, true);
            world.remove_resource::<LaunchSlot>();
            if let Err(error) = result {
                fail_launch_slot(world, slot, &error);
            }
        }
    }
}

fn fail_launch_slot(world: &mut World, slot: u16, error: &anyhow::Error) {
    bevy::log::error!("--load-slot {slot}: {error:#}");
    world.remove_resource::<LaunchSlot>();
    world.write_message(bevy::app::AppExit::error());
}

pub(crate) fn request_slots(world: &mut World, operation: SlotOperation) -> Result<()> {
    let mut state = world
        .remove_resource::<NativeSessionLoad>()
        .context("native session host absent")?;
    let result = (|| -> Result<()> {
        ensure!(
            !state.busy() && state.result.is_none() && state.slot_result.is_none(),
            "native session operation pending or result unread"
        );
        ensure!(
            world
                .get_resource::<SessionBoundary>()
                .context("native session boundary absent")?
                .is_idle(),
            "session boundary pending or result unread"
        );
        let store = world.resource::<slots::SlotStore>().clone();
        match operation {
            SlotOperation::Create { .. } | SlotOperation::Overwrite { .. } => {
                let plan = store.plan(operation)?;
                retain_selection(world, plan.slot, plan.description.clone())?;
                world.resource_mut::<SessionBoundary>().capture_live()?;
                state.capture = Some(plan);
            }
            SlotOperation::Load { slot, revision } => {
                let metadata = store.metadata(slot, Some(&revision))?;
                let install = world
                    .get_resource::<GameInstall>()
                    .context("native session install unavailable")?
                    .clone();
                retain_selection(world, slot, metadata.description)?;
                start_slot_load(&mut state, store, slot, Some(revision), install)?;
                state.slot_load = true;
            }
            operation => {
                let (sender, receiver) = mpsc::channel();
                std::thread::Builder::new()
                    .name("native save slots".into())
                    .spawn(move || {
                        let result = match operation {
                            SlotOperation::List => store.list().map(|slots| json!({"slots":slots})),
                            SlotOperation::Delete { slot, revision } => store
                                .delete(slot, &revision)
                                .map(|()| json!({"deleted":slot})),
                            _ => unreachable!(),
                        };
                        let _ = sender.send(result);
                    })?;
                state.slot_worker = Some(Mutex::new(receiver));
            }
        }
        Ok(())
    })();
    world.insert_resource(state);
    result
}

fn retain_selection(world: &mut World, slot: u16, description: String) -> Result<()> {
    world
        .get_resource_mut::<cyrus_game::script::DeathDispatch>()
        .context("native save selection: last-save dispatcher unavailable")?
        .last_save = Some(cyrus_game::script::LastSaveSelection { slot, description });
    Ok(())
}

fn start_slot_load(
    state: &mut NativeSessionLoad,
    store: slots::SlotStore,
    slot: u16,
    revision: Option<String>,
    install: GameInstall,
) -> Result<()> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("prepare saved slot".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result = store
                .read(slot, revision.as_deref())
                .and_then(|(bytes, _)| prepare_bytes(&bytes, &install))
                .with_context(|| format!("loading save slot {slot}"));
            let _ = sender.send(result);
        })?;
    state.path = format!("slot {slot}");
    state.worker = Some(Mutex::new(receiver));
    Ok(())
}

pub(crate) fn slots_result(world: &mut World, acknowledge: bool) -> Result<Value> {
    let mut state = world.resource_mut::<NativeSessionLoad>();
    ensure!(
        !acknowledge || state.slot_result.is_some(),
        "native slots have no terminal result"
    );
    let mut result = state
        .slot_result
        .as_ref()
        .map(|result| {
            result
                .as_ref()
                .cloned()
                .map_err(|error| anyhow::anyhow!("{error:#}"))
        })
        .unwrap_or_else(|| Ok(json!({"pending":true})));
    if state.slot_result.is_some()
        && let Ok(value) = &mut result
    {
        value["pending"] = json!(false);
    }
    if acknowledge {
        state.slot_result = None;
    }
    result
}

pub(crate) fn poll(world: &mut World) {
    let Some(mut state) = world.remove_resource::<NativeSessionLoad>() else {
        return;
    };
    if !state.busy()
        && state.result.is_none()
        && state.slot_result.is_none()
        && world
            .get_resource::<SessionBoundary>()
            .is_some_and(SessionBoundary::is_idle)
    {
        let request = world
            .get_resource_mut::<cyrus_startup::LastGameLoader>()
            .and_then(|mut loader| loader.take_request());
        if let Some(save) = request {
            state.last_game = true;
            let result = world
                .get_resource::<GameInstall>()
                .cloned()
                .context("native session install unavailable")
                .and_then(|install| {
                    start_slot_load(
                        &mut state,
                        world.resource::<slots::SlotStore>().clone(),
                        save.slot,
                        None,
                        install,
                    )
                });
            if let Err(error) = result {
                state.result = Some(Err(error));
            }
        }
    }
    if let Some(worker) = &state.slot_worker {
        let result = match worker.lock().unwrap().try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("native slot worker disconnected")))
            }
        };
        if let Some(result) = result {
            state.slot_worker = None;
            state.slot_result = Some(result);
        }
    } else if state.capture.is_some() {
        if let Some(result) = world.resource_mut::<SessionBoundary>().take_result() {
            let plan = state.capture.take().unwrap();
            let result = result.and_then(|result| match result {
                SessionBoundaryResult::Captured(snapshot) => snapshot.encode_document(),
                SessionBoundaryResult::Committed => {
                    anyhow::bail!("native save received commit result")
                }
            });
            match result {
                Err(error) => state.slot_result = Some(Err(error)),
                Ok(document) => {
                    let store = world.resource::<slots::SlotStore>().clone();
                    let (sender, receiver) = mpsc::channel();
                    match std::thread::Builder::new()
                        .name("publish save slot".into())
                        .spawn(move || {
                            let _ = sender.send(
                                store
                                    .publish(plan, &document)
                                    .map(|slot| json!({"slot":slot})),
                            );
                        }) {
                        Ok(_) => state.slot_worker = Some(Mutex::new(receiver)),
                        Err(error) => state.slot_result = Some(Err(error.into())),
                    }
                }
            }
        }
    } else if state.submitted {
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
    if let Some(result) = state.result.take() {
        if state.last_game {
            let completion = result.map_err(|error| format!("{error:#}"));
            let _ = world
                .resource_mut::<cyrus_startup::LastGameLoader>()
                .finish(completion);
            state.last_game = false;
        } else if state.slot_load {
            state.slot_result = Some(result.map(|()| json!({"committed":true})));
            state.slot_load = false;
        } else {
            state.result = Some(result);
        }
    }
    world.insert_resource(state);
}

fn prepare(path: &Path, install: &GameInstall) -> Result<PreparedLoad> {
    let bytes = cyrus_config::read(path)?.context("session document does not exist")?;
    prepare_bytes(&bytes, install)
}

fn prepare_bytes(bytes: &[u8], install: &GameInstall) -> Result<PreparedLoad> {
    let snapshot = RuntimeSessionSnapshot::decode_document(bytes)?;
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
    let cache = cyrus_config::cache_dir();
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
    cyrus_assets::hud_export::export_hud(install, cache)?;
    cyrus_assets::sound_export::export(install, cache)?;
    let mut audio_paths = Vec::new();
    let bank: cyrus_assets::sound::SoundBank =
        serde_json::from_slice(&std::fs::read(cache.join("sounds/bank.sounds"))?)?;
    for index in 0..bank.effects.len() {
        audio_paths.push(format!("sounds/{index:03}.wav"));
    }
    for label in snapshot.dialogue().required_labels() {
        cyrus_assets::sound::dialogue::export::export(install, cache, label)?;
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
    fn slots_reject_competing_owners_and_retain_admitted_selection() -> Result<()> {
        let mut world = World::new();
        world.init_resource::<NativeSessionLoad>();
        world.init_resource::<SessionBoundary>();
        let root = std::env::temp_dir().join(format!(
            "cyrus-slot-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        world.insert_resource(slots::SlotStore { root });
        world.init_resource::<cyrus_game::script::DeathDispatch>();
        world.resource_mut::<SessionBoundary>().capture_live()?;
        assert!(request_slots(&mut world, SlotOperation::List).is_err());
        assert!(
            world
                .resource::<cyrus_game::script::DeathDispatch>()
                .last_save
                .is_none()
        );
        // A fresh boundary models completion of the unrelated owner's operation.
        world.insert_resource(SessionBoundary::default());
        request_slots(
            &mut world,
            SlotOperation::Create {
                description: "Retained description".into(),
            },
        )?;
        let retained = world
            .resource::<cyrus_game::script::DeathDispatch>()
            .last_save
            .as_ref()
            .unwrap();
        assert_eq!(retained.description, "Retained description");
        assert!((2..=299).contains(&retained.slot));
        assert!(request_slots(&mut world, SlotOperation::List).is_err());
        assert!(slots_result(&mut world, true).is_err());
        Ok(())
    }

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
