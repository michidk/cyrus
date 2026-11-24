//! Public runtime admission of installed original continuations.
#![cfg(feature = "original-save-import")]
use anyhow::{Context, Result, ensure};
use bevy::prelude::{
    App, AssetApp, AssetPlugin, AssetServer, Assets, AudioSource, IntoScheduleConfigs,
    MinimalPlugins, Name, State, Transform, Visibility,
};
use cyrus_assets::{animation::Animations, hud::HudAsset, install::GameInstall, script::Scripts};
use cyrus_audio::sound::{Panned, RestoredDialogueAssets, SoundPlugin, dialogue::DialogueClip};
use cyrus_game::{
    preferences::{PreferenceKey, PreferenceStore, Preferences},
    random::EngineRandom,
};
use cyrus_script::{
    player::{
        Player, data::PlayerScene, input::PlayerToggles, placement::*, runtime::PlayerRuntime,
    },
    script::*,
};
use cyrus_session::*;
use rgpre::import::savegame::{SaveGame, SaveSlot, load_save_slot};
use std::{collections::BTreeMap, path::Path};
mod original_runtime_support;
use original_runtime_support::*;

struct NoWrites;
impl PreferenceStore for NoWrites {
    fn load(&self, _: PreferenceKey) -> Result<Option<String>> {
        Ok(None)
    }
    fn save(&self, _: PreferenceKey, _: &str) -> Result<()> {
        anyhow::bail!("import wrote preferences")
    }
}

#[test]
fn retained_runtime_import_preserves_late_rejection_and_reaches_first_service_update() -> Result<()>
{
    let (Some(root), Some(slots)) = (
        std::env::var_os("REDGUARD_DIR"),
        std::env::var_os("REDGUARD_ORIGINAL_SAVE_DIR"),
    ) else {
        eprintln!("SKIP retained runtime admission: install and slots not supplied");
        return Ok(());
    };
    let install = GameInstall::locate(Some(Path::new(&root)))?;
    let files = cyrus_assets::native::Files::open(&install)?;
    let exported = cyrus_assets::game::prepare(&files, "hud/overlay.hud")?;
    let hud: HudAsset = serde_json::from_slice(&exported["hud/overlay.hud"])?;
    let preferences = Preferences::new(NoWrites);
    let mut paths = std::fs::read_dir(slots)?.collect::<std::io::Result<Vec<_>>>()?;
    paths.sort_by_key(|entry| entry.path());
    ensure!(!paths.is_empty(), "retained slots absent");
    for path in paths {
        if path.path().is_dir() {
            verify_slot(
                &load_save_slot(path.path())?,
                &files,
                &install,
                &hud,
                &preferences,
            )?;
        }
    }
    Ok(())
}
fn verify_slot(
    slot: &SaveSlot,
    files: &cyrus_assets::native::Files,
    install: &GameInstall,
    hud: &HudAsset,
    preferences: &Preferences,
) -> Result<()> {
    let installed = InstalledSources::load(slot, files, install)?;
    let sources = installed.sources();
    let allocations = installed.allocations();
    let id = slot.game.metadata.world_id;
    let HostFixture {
        mut app,
        scene,
        animation,
        loaded_worlds,
    } = prepare_host(
        installed.prior(id)?,
        &sources,
        &installed.player_bytes,
        hud,
        preferences,
        id,
    )?;
    let snapshot = RuntimeSessionSnapshot::capture(app.world(), &sources, hud)?;
    let prepare = |snapshot: RuntimeSessionSnapshot| {
        snapshot.prepare_original(
            LogicalSessionRestoreInputs {
                sources: &sources,
                last_generation: 20,
                hud,
                preferences,
                bios_tick: 100,
            },
            OriginalSessionImport {
                slot,
                sources: &sources,
                allocations: &allocations,
                hud,
                preferences,
            },
        )
    };
    let admission = prepare(snapshot.clone())?;
    let retained =
        SessionWorldAssets::from_loaded(admission.snapshot(), loaded_worlds, BTreeMap::new())?;
    AdmissionCompletion {
        sources: &sources,
        hud,
        preferences,
        scene: &scene,
        animation: &animation,
        retained: &retained,
    }
    .verify(&mut app, snapshot, prepare)
}
