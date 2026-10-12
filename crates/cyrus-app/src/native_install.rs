//! Shared native install selection, remembered location and desktop folder picker.
use crate::bevy_egui::{EguiContexts, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use anyhow::Context;
use bevy::prelude::{
    App, AppExit, Camera2d, Commands, DefaultPlugins, MessageWriter, PluginGroup, Res, ResMut,
    Resource, Startup, default,
};
use cyrus_assets::install::GameInstall;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) const LOCATION: &str = "cyrus-config/install.json";

fn remembered(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("{}: invalid saved game-files location", path.display()))
            .map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("{}: reading saved game-files location", path.display())),
    }
}

pub(super) fn locate(explicit: Option<&Path>, preferences: &Path) -> anyhow::Result<GameInstall> {
    if let Some(path) = explicit {
        return GameInstall::resolve(path);
    }
    if let Some(path) = remembered(preferences)? {
        return GameInstall::resolve(&path)
            .context("The remembered game-files folder is no longer available");
    }
    GameInstall::locate(None)
}

fn remember(install: &GameInstall, preferences: &Path) -> anyhow::Result<()> {
    let path = install.cache_path(preferences)?;
    let directory = path
        .parent()
        .context("saved game-files location has no parent directory")?;
    std::fs::create_dir_all(directory)
        .with_context(|| format!("{}: creating preferences directory", directory.display()))?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result: anyhow::Result<()> = (|| {
        std::fs::write(&temporary, serde_json::to_vec_pretty(&install.root)?)?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.with_context(|| format!("{}: saving game-files location", path.display()))
}

#[derive(Resource)]
struct Selection {
    path: String,
    error: Option<String>,
    focus: bool,
    rejected: bool,
}
impl Selection {
    fn confirm(&self, preferences: &Path) -> anyhow::Result<GameInstall> {
        let install = GameInstall::resolve(Path::new(&self.path))?;
        remember(&install, preferences)?;
        Ok(install)
    }
}

#[derive(Resource)]
struct SelectedInstall(std::sync::mpsc::Sender<GameInstall>);

pub(super) fn prompt(
    explicit: Option<&Path>,
    title: &str,
    error: anyhow::Error,
) -> anyhow::Result<()> {
    let path = explicit
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os(cyrus_assets::install::ENV_VAR).map(PathBuf::from))
        .or_else(|| remembered(Path::new(LOCATION)).ok().flatten());
    let mut window = crate::window(title);
    let primary = window.primary_window.as_mut().unwrap();
    primary.resolution = (800, 460).into();
    primary.resize_constraints.min_width = 480.;
    primary.resize_constraints.min_height = 320.;
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(window))
        .add_plugins(crate::EguiPlugin)
        .insert_resource(crate::bevy_egui::EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .insert_resource(SelectedInstall(sender))
        .insert_resource(Selection {
            path: path
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            error: Some(format!("{error:#}")),
            focus: true,
            rejected: false,
        })
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn((Camera2d, PrimaryEguiContext));
        })
        .add_systems(EguiPrimaryContextPass, screen);
    app.run();
    // Winit permits one event loop per process; continue in a fresh process.
    if let Ok(install) = receiver.try_recv() {
        let status = std::process::Command::new(std::env::current_exe()?)
            .args(retry_args(std::env::args_os().skip(1), &install.root))
            .status()
            .context("starting the application with the selected game files")?;
        anyhow::ensure!(status.success(), "application exited with {status}");
    }
    Ok(())
}

fn retry_args(args: impl IntoIterator<Item = OsString>, root: &Path) -> Vec<OsString> {
    let mut output = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--install" {
            args.next();
        } else if arg != "--" && !arg.as_encoded_bytes().starts_with(b"--install=") {
            output.push(arg);
        }
    }
    output.extend([OsString::from("--install"), root.as_os_str().to_owned()]);
    output
}

fn screen(
    mut contexts: EguiContexts,
    mut selection: ResMut<Selection>,
    selected: Res<SelectedInstall>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(context) = contexts.ctx_mut() else {
        return;
    };
    let mut viewport = egui::Ui::new(
        context.clone(),
        "install-picker".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(context.viewport_rect()),
    );
    egui::CentralPanel::default().show(&mut viewport, |ui| {
        let top = ((ui.available_height() - 260.) / 2.).max(0.);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(top);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width().min(560.), 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.heading("Game files not found");
                        ui.add_space(8.);
                        ui.label("Choose the folder for your own GOG copy of Redguard.");
                        ui.label("You can choose the game folder, its GOG folder, or a subfolder.");
                        ui.add_space(16.);
                        ui.label("Redguard folder");
                        let mut confirm = false;
                        ui.horizontal(|ui| {
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut selection.path)
                                    .hint_text("Path to your Redguard installation")
                                    .desired_width((ui.available_width() - 85.).max(100.)),
                            );
                            if field.changed() {
                                selection.error = None;
                                selection.rejected = false;
                            }
                            if selection.focus {
                                field.request_focus();
                                selection.focus = false;
                            }
                            confirm = field.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            if ui.button("Browse…").clicked() {
                                let mut dialog =
                                    rfd::FileDialog::new().set_title("Select your Redguard folder");
                                if !selection.path.trim().is_empty() {
                                    dialog = dialog.set_directory(&selection.path);
                                }
                                if let Some(path) = dialog.pick_folder() {
                                    selection.path = path.to_string_lossy().into_owned();
                                    selection.focus = true;
                                    selection.error = None;
                                    selection.rejected = false;
                                }
                            }
                        });
                        ui.add_space(8.);
                        ui.label("We'll remember this folder for the game and asset viewer.");
                        if let Some(error) = &selection.error {
                            egui::CollapsingHeader::new("Details").show(ui, |ui| {
                                ui.colored_label(egui::Color32::LIGHT_RED, error);
                            });
                        }
                        if selection.rejected {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                "Couldn't use this folder. Check the details and try again.",
                            );
                        }
                        ui.add_space(16.);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            confirm |= ui
                                .add_enabled(
                                    !selection.path.trim().is_empty(),
                                    egui::Button::new("Continue"),
                                )
                                .clicked();
                            if ui.button("Cancel").clicked()
                                || ui.input(|input| input.key_pressed(egui::Key::Escape))
                            {
                                exit.write(AppExit::Success);
                            }
                        });
                        if confirm && !selection.path.trim().is_empty() {
                            match selection.confirm(Path::new(LOCATION)) {
                                Ok(install) => {
                                    selected
                                        .0
                                        .send(install)
                                        .expect("launcher receives selected install");
                                    exit.write(AppExit::Success);
                                }
                                Err(error) => {
                                    selection.error = Some(format!("{error:#}"));
                                    selection.rejected = true;
                                }
                            }
                        }
                    },
                );
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "cyrus-install-picker-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn install(&self, name: &str) -> PathBuf {
            let root = self.0.join(name);
            std::fs::create_dir_all(root.join("fxart")).unwrap();
            std::fs::create_dir_all(root.join("maps")).unwrap();
            std::fs::write(root.join("RGFX.EXE"), []).unwrap();
            root.canonicalize().unwrap()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn selection_remembers_resolved_root_and_recovers_after_removal() {
        let scratch = Scratch::new();
        let root = scratch.install("GOG Galaxy/Redguard/Redguard");
        let preferences = scratch.0.join("config/install.json");
        let mut selection = Selection {
            path: scratch.0.join("GOG Galaxy").display().to_string(),
            error: None,
            focus: false,
            rejected: false,
        };
        let accepted = selection.confirm(&preferences).unwrap();
        assert_eq!(accepted.root, root);
        assert_eq!(locate(None, &preferences).unwrap().root, root);
        let other = scratch.install("Other/Redguard");
        assert_eq!(locate(Some(&other), &preferences).unwrap().root, other);
        let saved = std::fs::read(&preferences).unwrap();
        selection.path = scratch.0.join("missing").display().to_string();
        assert!(selection.confirm(&preferences).is_err());
        assert_eq!(std::fs::read(&preferences).unwrap(), saved);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(locate(None, &preferences).is_err());
        selection.path = other.display().to_string();
        selection.confirm(&preferences).unwrap();
        assert_eq!(locate(None, &preferences).unwrap().root, other);
    }

    #[test]
    fn malformed_preferences_are_reported_and_explicit_selection_can_replace_them() {
        let scratch = Scratch::new();
        let preferences = scratch.0.join("install.json");
        std::fs::write(&preferences, "broken").unwrap();
        assert!(
            locate(None, &preferences)
                .unwrap_err()
                .to_string()
                .contains("invalid saved")
        );
        let root = scratch.install("Redguard");
        assert_eq!(locate(Some(&root), &preferences).unwrap().root, root);
        remember(&GameInstall::open(&root).unwrap(), &preferences).unwrap();
        assert_eq!(locate(None, &preferences).unwrap().root, root);
    }

    #[test]
    fn cannot_store_preferences_inside_install_or_accept_a_failed_save() {
        let scratch = Scratch::new();
        let root = scratch.install("Redguard");
        let install = GameInstall::open(&root).unwrap();
        assert!(remember(&install, &root.join("install.json")).is_err());
        assert!(!root.join("install.json").exists());
        let blocked = scratch.0.join("blocked");
        std::fs::write(&blocked, "keep this file").unwrap();
        let selection = Selection {
            path: root.display().to_string(),
            error: None,
            focus: false,
            rejected: false,
        };
        assert!(selection.confirm(&blocked.join("install.json")).is_err());
        assert_eq!(std::fs::read_to_string(&blocked).unwrap(), "keep this file");
    }

    #[test]
    fn retry_preserves_launch_flags_and_replaces_install_arguments() {
        let args = [
            "--scene",
            "START",
            "--install",
            "old folder",
            "--water=modern",
            "--install=another folder",
            "--",
        ];
        let actual = retry_args(
            args.into_iter().map(OsString::from),
            Path::new("chosen folder/Redguard"),
        );
        let expected: Vec<OsString> = [
            "--scene",
            "START",
            "--water=modern",
            "--install",
            "chosen folder/Redguard",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        assert_eq!(actual, expected);
    }
}
