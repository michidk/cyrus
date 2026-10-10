//! Startup preload: scan the install and parse every known file with the
//! preservation crate while a progress bar shows what is being read. Files that
//! fail to parse are counted, not fatal; the list is the format-coverage report.

use crate::{AppState, install::GameInstall};
use bevy::prelude::*;
use rgpre::import::{FileType, registry};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Loading), start)
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

#[derive(Component)]
struct LoadingUi;
#[derive(Component)]
struct Bar;
#[derive(Component)]
struct Label;

fn start(
    mut commands: Commands,
    install: Res<GameInstall>,
    startup: Option<Res<crate::startup::StartupPresentation>>,
) {
    let (tx, rx) = channel();
    let root = install.root.clone();
    std::thread::spawn(move || preload(&root, &tx));
    commands.insert_resource(Channel(Arc::new(Mutex::new(rx))));
    if startup.is_some() {
        return;
    }
    commands.spawn((
        LoadingUi,
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(12),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        children![
            (
                Label,
                Text::new("Reading install..."),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                }
            ),
            (
                Node {
                    width: px(400),
                    height: px(16),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BorderColor::all(Color::WHITE),
                children![(
                    Bar,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::WHITE)
                )],
            ),
        ],
    ));
}

fn poll(
    mut commands: Commands,
    channel: Res<Channel>,
    startup: Option<Res<crate::startup::StartupPresentation>>,
    mut bar: Query<&mut Node, With<Bar>>,
    mut label: Query<&mut Text, With<Label>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let rx = channel.0.lock().unwrap();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Progress::File { done, total, name } => {
                if let Ok(mut node) = bar.single_mut() {
                    node.width = percent(100.0 * done as f32 / total.max(1) as f32);
                }
                if let Ok(mut text) = label.single_mut() {
                    text.0 = format!("{name} ({done}/{total})");
                }
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

fn teardown(mut commands: Commands, ui: Query<Entity, With<LoadingUi>>) {
    for e in &ui {
        commands.entity(e).despawn();
    }
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
