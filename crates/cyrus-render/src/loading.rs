//! Startup preload: scan the install and parse every known file with the
//! preservation crate while a progress bar shows what is being read. Files that
//! fail to parse are counted, not fatal; the list is the format-coverage report.

use bevy::prelude::*;
use cyrus_assets::install::GameInstall;
use cyrus_game::AppState;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

pub struct LoadingPlugin;

/// Required menu/HUD export has completed for this native session.
#[derive(Resource)]
pub struct MenuPrepared;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<cyrus_app::LoadingProgress>()
            .add_systems(
                Update,
                (start, poll).chain().run_if(in_state(AppState::Loading)),
            )
            .add_systems(OnExit(AppState::Loading), teardown);
    }
}

#[derive(Resource)]
struct Channel(Arc<Mutex<Receiver<Progress>>>);

fn start(
    mut commands: Commands,
    install: Res<GameInstall>,
    mut startup: Option<ResMut<crate::startup::StartupPresentation>>,
    existing: Option<Res<Channel>>,
    preloaded: Option<Res<Preloaded>>,
    menu_prepared: Option<Res<MenuPrepared>>,
    mut progress: ResMut<cyrus_app::LoadingProgress>,
) {
    if existing.is_some()
        || preloaded.is_some()
        || menu_prepared.is_some()
        || startup
            .as_ref()
            .is_some_and(|s| !s.title_visible || s.error.is_some())
    {
        return;
    }
    let (tx, rx) = channel();
    let install = install.clone();
    let presentation = startup.is_some();
    // Installed parsers exceed the default worker stack in unoptimized builds.
    let worker = std::thread::Builder::new()
        .name("startup preparation".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            if presentation {
                let cache = std::path::Path::new("cyrus-config/cache");
                let result = cyrus_assets::startup::export_menu(&install, cache)
                    .and_then(|()| cyrus_assets::hud_export::export_hud(&install, cache));
                if let Err(error) = result {
                    let _ = tx.send(Progress::Failed(format!("Startup preparation: {error:#}")));
                    return;
                }
                let _ = tx.send(Progress::MenuReady);
                return;
            }
            preload(&install.root, &tx);
        });
    if let Err(error) = worker {
        let error = format!("Startup preparation worker: {error}");
        error!("{error}");
        if let Some(startup) = startup.as_mut() {
            startup.error = Some(error);
        }
        return;
    }
    commands.insert_resource(Channel(Arc::new(Mutex::new(rx))));
    if startup.is_some() {
        return;
    }
    progress.visible = true;
    progress.label = "Reading install...".into();
    progress.fraction = Some(0.);
}

fn poll(
    mut commands: Commands,
    channel: Option<Res<Channel>>,
    mut startup: Option<ResMut<crate::startup::StartupPresentation>>,
    mut progress: ResMut<cyrus_app::LoadingProgress>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(channel) = channel else {
        return;
    };
    let rx = channel.0.lock().unwrap();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Progress::MenuReady => {
                commands.insert_resource(MenuPrepared);
            }
            Progress::Failed(error) => {
                error!("{error}");
                if let Some(startup) = startup.as_mut() {
                    startup.error = Some(error);
                }
            }
            Progress::File { done, total, name } => {
                progress.fraction = Some(done as f32 / total.max(1) as f32);
                progress.label = format!("{name} ({done}/{total})");
            }
            Progress::Done(preloaded) => {
                info!(
                    "preload: {} parsed, {} failed",
                    preloaded.parsed,
                    preloaded.failed.len()
                );
                for (name, err) in &preloaded.failed {
                    warn!("preload: {name}: {err}");
                }
                for (ext, n) in &preloaded.unsupported {
                    info!("preload: {n} unsupported .{ext} files");
                }
                if let Err(e) = write_report(&preloaded) {
                    warn!("preload: cannot write report: {e}");
                }
                commands.insert_resource(*preloaded);
                if startup.is_none() {
                    next.set(AppState::InGame);
                }
            }
        }
    }
}

fn teardown(mut commands: Commands, mut progress: ResMut<cyrus_app::LoadingProgress>) {
    progress.visible = false;
    commands.remove_resource::<Channel>();
}

pub use cyrus_assets::loading::{Preloaded, Progress, TypeCount, preload};

fn write_report(report: &Preloaded) -> std::io::Result<()> {
    std::fs::create_dir_all("cyrus-config")?;
    std::fs::write(
        "cyrus-config/preload-report.json",
        serde_json::to_string_pretty(report)?,
    )
}
