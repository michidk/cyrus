//! Startup preload: scan the install and parse every known file with the
//! preservation crate while a progress bar shows what is being read. Files that
//! fail to parse are counted, not fatal; the list is the format-coverage report.

use bevy::prelude::*;
use cyrus_assets::install::GameInstall;
use cyrus_game::AppState;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Loading), start)
            .add_systems(Update, poll.run_if(in_state(AppState::Loading)))
            .add_systems(OnExit(AppState::Loading), teardown);
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

fn teardown(mut commands: Commands, ui: Query<Entity, With<LoadingUi>>) {
    for e in &ui {
        commands.entity(e).despawn();
    }
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
