//! The first screen when no Redguard folder is known: choose it, then the launcher opens.
use crate::theme::{self, ERROR, GOLD};
use cyrus_launcher::install;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// What the player did this frame.
pub enum Outcome {
    Waiting,
    /// A folder that holds the game, already resolved.
    Chosen(PathBuf),
    Quit,
}

pub struct Picker {
    path: String,
    error: Option<String>,
    focus: bool,
    /// The open folder dialog, on its own thread so the window keeps answering the system
    /// (a blocked event loop shows as "Not Responding" on Windows).
    browsing: Option<Receiver<Option<PathBuf>>>,
}

impl Picker {
    /// `previous` is a folder that was chosen before but no longer holds the game.
    pub fn new(previous: Option<&Path>) -> Self {
        Self {
            path: previous
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            error: previous.map(|path| {
                format!(
                    "{}: the saved game folder no longer holds Redguard.",
                    path.display()
                )
            }),
            focus: true,
            browsing: None,
        }
    }

    fn browse(&mut self, context: &egui::Context) {
        let (sender, receiver) = channel();
        let start = self.path.trim().to_owned();
        let context = context.clone();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Select your Redguard folder");
            if !start.is_empty() {
                dialog = dialog.set_directory(&start);
            }
            let _ = sender.send(dialog.pick_folder());
            context.request_repaint();
        });
        self.browsing = Some(receiver);
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Outcome {
        let context = ui.ctx().clone();
        if let Some(receiver) = &self.browsing {
            match receiver.try_recv() {
                Ok(folder) => {
                    self.browsing = None;
                    if let Some(folder) = folder {
                        self.path = folder.to_string_lossy().into_owned();
                        self.error = None;
                        self.focus = true;
                    }
                }
                Err(TryRecvError::Disconnected) => self.browsing = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut outcome = Outcome::Waiting;
        let mut confirm = false;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() - 250.0).max(0.0) / 2.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width().min(520.0), 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(
                        egui::RichText::new("Game files not found")
                            .size(24.0)
                            .color(GOLD),
                    );
                    ui.add_space(8.0);
                    ui.label("Choose the folder for your own GOG copy of Redguard.");
                    ui.label(theme::muted(
                        "You can choose the game folder, its GOG folder, or a subfolder.",
                    ));
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.path)
                                .hint_text("Path to your Redguard installation")
                                .desired_width((ui.available_width() - 100.0).max(100.0)),
                        );
                        if field.changed() {
                            self.error = None;
                        }
                        if self.focus {
                            field.request_focus();
                            self.focus = false;
                        }
                        confirm = field.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if ui
                            .add_enabled(self.browsing.is_none(), egui::Button::new("Browse…"))
                            .clicked()
                        {
                            self.browse(&context);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(theme::muted(
                        "The launcher, the game and the asset viewer remember this folder.",
                    ));
                    if let Some(error) = &self.error {
                        ui.add_space(8.0);
                        ui.colored_label(ERROR, error);
                    }
                    theme::footer(ui, |ui| {
                        confirm |= ui
                            .add_enabled_ui(!self.path.trim().is_empty(), |ui| {
                                theme::primary(ui, "Continue")
                            })
                            .inner
                            .clicked();
                        if theme::secondary(ui, "Quit").clicked()
                            || ui.input(|input| input.key_pressed(egui::Key::Escape))
                        {
                            outcome = Outcome::Quit;
                        }
                    });
                },
            );
        });
        if confirm && !self.path.trim().is_empty() {
            match install::resolve(Path::new(self.path.trim())) {
                Ok(root) => outcome = Outcome::Chosen(root),
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }
        outcome
    }
}
