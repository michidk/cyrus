//! Startup preload: scan the install and parse every known file with the
//! preservation crate while a progress bar shows what is being read. Files that
//! fail to parse are counted, not fatal; the list is the format-coverage report.

use crate::{AppState, install::GameInstall};
use bevy::prelude::*;
use cyrus_app::LoadingProgress;
use rgpre::import::{FileType, registry};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(cyrus_app::LoadingOverlayPlugin)
            .add_systems(OnEnter(AppState::Loading), start)
            .add_systems(Update, poll.run_if(in_state(AppState::Loading)))
            .add_systems(OnExit(AppState::Loading), teardown);
    }
}

#[derive(Debug, Clone)]
pub enum Progress {
    File {
        done: usize,
        total: usize,
        name: String,
    },
    Done(Box<Preloaded>),
}

/// Result of the preload, kept for the rest of the session and written to
/// `cyrus-config/preload-report.json`: the format-coverage report.
#[derive(Resource, Debug, Clone, Default, serde::Serialize)]
pub struct Preloaded {
    pub parsed: usize,
    pub failed: Vec<(String, String)>,
    /// Per file type (display name): parsed and failed counts.
    pub by_type: std::collections::BTreeMap<String, TypeCount>,
    /// Extensions in the install that no parser claims, with file counts.
    pub unsupported: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct TypeCount {
    pub parsed: usize,
    pub failed: usize,
}

impl Preloaded {
    pub fn report_path() -> std::path::PathBuf {
        std::path::Path::new(crate::settings::CONFIG_DIR).join("preload-report.json")
    }
    pub fn write_report(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(crate::settings::CONFIG_DIR)?;
        std::fs::write(
            Self::report_path(),
            serde_json::to_string_pretty(self).unwrap_or_default(),
        )
    }
}

#[derive(Resource)]
struct Channel(Arc<Mutex<Receiver<Progress>>>);

fn start(
    mut commands: Commands,
    install: Res<GameInstall>,
    mut progress: ResMut<LoadingProgress>,
    startup: Option<Res<crate::startup::StartupPresentation>>,
) {
    let (tx, rx) = channel();
    let root = install.root.clone();
    std::thread::spawn(move || preload(&root, &tx));
    commands.insert_resource(Channel(Arc::new(Mutex::new(rx))));
    *progress = LoadingProgress {
        visible: startup.is_none(),
        label: "Reading install...".into(),
        fraction: None,
    };
}

fn poll(
    mut commands: Commands,
    channel: Res<Channel>,
    startup: Option<Res<crate::startup::StartupPresentation>>,
    mut progress: ResMut<LoadingProgress>,
    mut next: ResMut<NextState<AppState>>,
) {
    let rx = channel.0.lock().unwrap();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Progress::File { done, total, name } => {
                progress.fraction = Some(done as f32 / total.max(1) as f32);
                progress.label = format!("{name} ({done}/{total})");
            }
            Progress::Done(preloaded) => {
                progress.visible = false;
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
                if let Err(e) = preloaded.write_report() {
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

fn teardown(mut commands: Commands, mut progress: ResMut<LoadingProgress>) {
    progress.visible = false;
    commands.remove_resource::<Channel>();
}

fn preload(root: &std::path::Path, tx: &Sender<Progress>) {
    let registry = match registry::scan_dir(root) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Progress::Done(Box::new(Preloaded {
                failed: vec![("scan".into(), e.to_string())],
                ..Default::default()
            })));
            return;
        }
    };
    let mut entries: Vec<_> = registry.files.values().collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let total = entries.len();
    let mut failed = Vec::new();
    let mut by_type = std::collections::BTreeMap::<String, TypeCount>::new();
    for (i, entry) in entries.iter().enumerate() {
        let _ = tx.send(Progress::File {
            done: i,
            total,
            name: entry.name.clone(),
        });
        let count = by_type
            .entry(entry.file_type.display_name().to_string())
            .or_default();
        match parse_one(&entry.path, entry.file_type) {
            Ok(()) => count.parsed += 1,
            Err(e) => {
                count.failed += 1;
                failed.push((entry.name.clone(), e));
            }
        }
    }
    let _ = tx.send(Progress::File {
        done: total,
        total,
        name: "done".into(),
    });
    let _ = tx.send(Progress::Done(Box::new(Preloaded {
        parsed: total - failed.len(),
        failed,
        by_type,
        unsupported: unsupported_extensions(root),
    })));
}

/// Files under the install whose extension no parser claims, counted per extension.
pub fn unsupported_extensions(root: &std::path::Path) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() || FileType::from_path(entry.path()).is_some() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        *counts.entry(ext).or_insert(0) += 1;
    }
    counts
}

/// One parse per file type. Returns the error text so coverage gaps are visible in the log.
pub fn parse_one(path: &std::path::Path, file_type: FileType) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    use rgpre::import::*;
    let r: Result<(), String> = match file_type {
        FileType::Bsi => bsi::parse_bsi_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Cht => cht::parse_cht_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Col => palette::Palette::parse(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Model3d | FileType::Model3dc => model3d::parse_3d_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Rob => rob::parse_rob_with_models(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Rgm => rgm::parse_rgm_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Pvo => pvo::parse_pvo_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Wld => wld::parse_wld_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Fnt => fnt::parse_fnt(&data).map(drop).map_err(|e| e.to_string()),
        FileType::Gxa => gxa::parse_gxa_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Sfx => sfx::parse_sfx_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Rtx => rtx::parse_rtx_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
    };
    r
}
