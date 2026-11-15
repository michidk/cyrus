#![cfg_attr(windows, windows_subsystem = "windows")]

//! The launcher window: the original Redguard launcher, recreated, with its artwork read from the
//! player's own `REDGUARD.EXE`. Game Options holds the game files, the mods and the faithful or
//! modern presentation. Without the artwork it shows plain buttons instead.
mod theme;

use cyrus_launcher::artwork::{Artwork, BUTTON_POSITIONS, Button, Image};
use cyrus_launcher::cheats::{self, Cheat};
use cyrus_launcher::fidelity::{self, Area, Choices, Mode};
use cyrus_launcher::launch::{self, Options};
use cyrus_launcher::lettering;
use cyrus_launcher::mods::{self, Installed};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use std::collections::BTreeSet;
use std::path::PathBuf;
use theme::{ERROR, GOLD, GOLD_DIM, PARCHMENT, RED};

const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 480.0;
const PROJECT: &str = "https://github.com/michidk/cyrus";
const ISSUES: &str = "https://github.com/michidk/cyrus/issues";
/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Play,
    Options,
    Mods,
    Support,
    Quit,
}

/// Where a button's artwork comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Art {
    /// The original launcher's bitmaps for this button, by their index in the artwork.
    Original(usize),
    /// Assembled from the letters of the original bitmaps: "Mods".
    Mods,
}

/// A button, the slot of the original 640 by 480 layout it sits in (the original has six, the
/// fourth for a web link that this launcher does not show), and its fallback label.
struct Entry {
    action: Action,
    label: &'static str,
    art: Art,
    slot: usize,
}

const ENTRIES: [Entry; 5] = [
    Entry {
        action: Action::Play,
        label: "Play Redguard",
        art: Art::Original(0),
        slot: 0,
    },
    Entry {
        action: Action::Options,
        label: "Game Options",
        art: Art::Original(1),
        slot: 1,
    },
    Entry {
        action: Action::Mods,
        label: "Mods",
        art: Art::Mods,
        slot: 2,
    },
    Entry {
        action: Action::Support,
        label: "Technical Support",
        art: Art::Original(2),
        slot: 3,
    },
    Entry {
        action: Action::Quit,
        label: "Quit",
        art: Art::Original(5),
        slot: 5,
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Game,
    Graphics,
    Cheats,
}

#[derive(PartialEq)]
enum Dialog {
    None,
    Options(Page),
    Mods,
    Error(String),
}

/// A button's three bitmaps and where the original layout puts its top-left corner.
struct Placed {
    action: Action,
    button: Button,
    origin: (u32, u32),
}

struct Textures {
    background: TextureHandle,
    /// Up, focus, down for each button, and the rectangle it fills.
    buttons: Vec<([TextureHandle; 3], Rect, Action)>,
}

struct App {
    launcher: PathBuf,
    options: Options,
    remembered: Option<PathBuf>,
    installed: Installed,
    active: BTreeSet<String>,
    choices: Choices,
    cheats: cheats::State,
    art: Option<Artwork>,
    placed: Vec<Placed>,
    textures: Option<Textures>,
    dialog: Dialog,
    /// The button under the mouse.
    focus: Option<usize>,
    /// The last problem saving a choice, shown in the options dialog.
    problem: Option<String>,
}

fn texture(context: &egui::Context, name: &str, image: &Image) -> TextureHandle {
    let pixels = egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.rgba,
    );
    context.load_texture(name, pixels, TextureOptions::NEAREST)
}

/// `entry`'s artwork, right-aligned in its slot of the original layout and centred on it. `None` when
/// a label has to be cut from original bitmaps that do not separate into letters.
fn place(entry: &Entry, art: &Artwork) -> Option<Placed> {
    let slot = art.buttons.get(entry.slot)?;
    let (slot_x, slot_y) = BUTTON_POSITIONS[entry.slot];
    let button = match entry.art {
        Art::Original(index) => art.buttons.get(index)?.clone(),
        Art::Mods => lettering::label(art, &lettering::MODS)?,
    };
    let x = (slot_x + slot.up.width).saturating_sub(button.up.width);
    let y = (slot_y + slot.up.height / 2).saturating_sub(button.up.height / 2);
    Some(Placed {
        action: entry.action,
        button,
        origin: (x, y),
    })
}

/// Open a web page or folder with the system's default program.
fn open(target: &std::ffi::OsStr) -> std::io::Result<()> {
    let mut command = if cfg!(windows) {
        std::process::Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(target).spawn().map(drop)
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            launcher: std::env::current_exe().unwrap_or_default(),
            options: Options::default(),
            remembered: launch::remembered_install(),
            installed: Installed::default(),
            active: BTreeSet::new(),
            choices: Choices::default(),
            cheats: cheats::State::default(),
            art: None,
            placed: Vec::new(),
            textures: None,
            dialog: Dialog::None,
            focus: Some(0),
            problem: None,
        };
        let mut problems = Vec::new();
        match Options::load(&launch::options_file()) {
            Ok(options) => app.options = options,
            Err(error) => problems.push(format!("{error:#}")),
        }
        match mods::scan(&mods::mods_dir()) {
            Ok(installed) => app.installed = installed,
            Err(error) => problems.push(format!("{error:#}")),
        }
        match mods::load_active(&mods::active_file()) {
            Ok(active) => app.active = active,
            Err(error) => problems.push(format!("{error:#}")),
        }
        match fidelity::load(&fidelity::settings_file()) {
            Ok(choices) => app.choices = choices,
            Err(error) => problems.push(format!("{error:#}")),
        }
        match cheats::load(
            &cheats::local_file(),
            app.game_folder().map(PathBuf::as_path),
        ) {
            Ok(state) => app.cheats = state,
            Err(error) => problems.push(format!("{error:#}")),
        }
        if !problems.is_empty() {
            app.dialog = Dialog::Error(problems.join("\n"));
        }
        app.load_artwork();
        app
    }

    /// The game folder the artwork is read from: the one picked here, else the one the game remembered.
    fn game_folder(&self) -> Option<&PathBuf> {
        self.options.install.as_ref().or(self.remembered.as_ref())
    }

    fn load_artwork(&mut self) {
        self.textures = None;
        self.placed.clear();
        self.art = self.game_folder().and_then(|game| Artwork::load(game));
        let Some(art) = &self.art else {
            return;
        };
        let placed: Option<Vec<Placed>> = ENTRIES.iter().map(|entry| place(entry, art)).collect();
        match placed {
            Some(placed) => self.placed = placed,
            // When the new labels cannot be cut from the original bitmaps, plain buttons show instead
            // of a half-lettered screen.
            None => self.art = None,
        }
    }

    /// Writes every choice to the files the game reads. Called after each change, so closing the
    /// window never loses one.
    fn save(&mut self) {
        let result = self
            .options
            .save(&launch::options_file())
            .and_then(|()| mods::save_active(&mods::active_file(), &self.active))
            .and_then(|()| fidelity::save(&fidelity::settings_file(), &self.choices));
        self.problem = result.err().map(|error| format!("{error:#}"));
    }

    fn play(&mut self, context: &egui::Context) {
        self.save();
        if let Some(problem) = self.problem.clone() {
            self.dialog = Dialog::Error(problem);
            return;
        }
        let started = launch::find_game(&self.launcher).and_then(|game| {
            std::process::Command::new(&game)
                .args(self.options.arguments())
                .spawn()
                .map_err(|error| anyhow::anyhow!("{}: {error}", game.display()))
        });
        match started {
            Ok(_) => context.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(error) => {
                self.dialog = Dialog::Error(format!("Redguard could not be started:\n{error:#}"))
            }
        }
    }

    fn activate(&mut self, action: Action, context: &egui::Context) {
        match action {
            Action::Play => self.play(context),
            Action::Options => self.dialog = Dialog::Options(Page::Game),
            Action::Mods => self.dialog = Dialog::Mods,
            Action::Support => {
                let _ = open(ISSUES.as_ref());
            }
            Action::Quit => context.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn screen(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        if self.textures.is_none()
            && let Some(art) = &self.art
        {
            self.textures = Some(Textures {
                background: texture(&context, "background", &art.background),
                buttons: self
                    .placed
                    .iter()
                    .enumerate()
                    .map(|(i, placed)| {
                        let states = [
                            texture(&context, &format!("b{i}u"), &placed.button.up),
                            texture(&context, &format!("b{i}f"), &placed.button.focus),
                            texture(&context, &format!("b{i}d"), &placed.button.down),
                        ];
                        let (x, y) = placed.origin;
                        let rect = Rect::from_min_size(
                            Pos2::new(x as f32, y as f32),
                            states[0].size_vec2(),
                        );
                        (states, rect, placed.action)
                    })
                    .collect(),
            });
        }
        let modal = self.dialog != Dialog::None;
        let mut clicked = None;
        let mut hovered = None;
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        if let Some(textures) = &self.textures {
            let full = Rect::from_min_size(Pos2::ZERO, Vec2::new(WIDTH, HEIGHT));
            ui.painter()
                .image(textures.background.id(), full, uv, Color32::WHITE);
            for (i, (states, rect, _)) in textures.buttons.iter().enumerate() {
                let rect = *rect;
                let sense = if modal {
                    Sense::hover()
                } else {
                    Sense::click()
                };
                let response = ui.interact(rect, ui.id().with(i), sense);
                if !modal && response.hovered() {
                    hovered = Some(i);
                }
                if response.clicked() {
                    clicked = Some(i);
                }
                let state = if !modal && response.is_pointer_button_down_on() {
                    2
                } else if !modal && self.focus == Some(i) {
                    1
                } else {
                    0
                };
                ui.painter()
                    .image(states[state].id(), rect, uv, Color32::WHITE);
            }
        } else {
            ui.add_space(120.0);
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("Redguard").size(40.0).color(RED));
                ui.label("The original launcher artwork is shown when REDGUARD.EXE is in your game folder.");
                ui.add_space(20.0);
                for (i, entry) in ENTRIES.iter().enumerate() {
                    let button = ui.add_enabled(
                        !modal,
                        egui::Button::new(entry.label).min_size(Vec2::new(220.0, 30.0)),
                    );
                    if button.hovered() {
                        hovered = Some(i);
                    }
                    if button.clicked() {
                        clicked = Some(i);
                    }
                }
            });
        }
        self.corner_mark(ui, modal);
        if modal {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, Color32::from_black_alpha(150));
        }
        if hovered.is_some() {
            self.focus = hovered;
        }
        if let Some(i) = clicked {
            self.activate(ENTRIES[i].action, &context);
        }
    }

    /// cyrus's own mark in the corner the original art leaves to the map.
    fn corner_mark(&self, ui: &mut egui::Ui, modal: bool) {
        let text = format!(
            "cyrus {} · github.com/michidk/cyrus",
            env!("CARGO_PKG_VERSION")
        );
        let galley = ui
            .painter()
            .layout_no_wrap(text, egui::FontId::proportional(12.0), PARCHMENT);
        let rect = Rect::from_min_size(
            Pos2::new(10.0, HEIGHT - 28.0),
            galley.size() + Vec2::new(14.0, 8.0),
        );
        let sense = if modal {
            Sense::hover()
        } else {
            Sense::click()
        };
        let link = ui.interact(rect, ui.id().with("project"), sense);
        let hover = !modal && link.hovered();
        ui.painter().rect(
            rect,
            4.0,
            Color32::from_black_alpha(if hover { 200 } else { 150 }),
            egui::Stroke::new(1.0, if hover { GOLD } else { GOLD_DIM }),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.min + Vec2::new(7.0, 4.0), galley, PARCHMENT);
        if hover {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if link.clicked() {
            let _ = open(PROJECT.as_ref());
        }
    }

    fn dialogs(&mut self, context: &egui::Context) {
        match &self.dialog {
            Dialog::None => {}
            Dialog::Error(text) => {
                let text = text.clone();
                let mut close = false;
                theme::dialog(context, "Problem", |ui| {
                    ui.label(egui::RichText::new(&text).color(ERROR));
                    theme::footer(ui, |ui| close = theme::primary(ui, "OK").clicked());
                });
                if close {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::Mods => {
                let mut done = false;
                theme::dialog(context, "Mods", |ui| {
                    self.mods_page(ui);
                    if let Some(problem) = &self.problem {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(problem).color(ERROR));
                    }
                    theme::footer(ui, |ui| done = theme::primary(ui, "Done").clicked());
                });
                if done {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::Options(page) => {
                let mut page = *page;
                let mut done = false;
                theme::dialog(context, "Game Options", |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut page, Page::Game, "Game");
                        ui.selectable_value(&mut page, Page::Graphics, "Graphics and sound");
                        ui.selectable_value(&mut page, Page::Cheats, "Cheats");
                    });
                    ui.add_space(4.0);
                    match page {
                        Page::Game => self.game_page(ui),
                        Page::Graphics => self.graphics_page(ui),
                        Page::Cheats => self.cheats_page(ui),
                    }
                    if let Some(problem) = &self.problem {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(problem).color(ERROR));
                    }
                    theme::footer(ui, |ui| done = theme::primary(ui, "Done").clicked());
                });
                self.dialog = if done {
                    Dialog::None
                } else {
                    Dialog::Options(page)
                };
            }
        }
    }

    fn game_page(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Game files");
        match self.game_folder() {
            Some(game) => theme::path(ui, &game.display().to_string()),
            None => theme::path(
                ui,
                "Not chosen yet. The game searches for it when it starts.",
            ),
        }
        ui.horizontal(|ui| {
            if theme::secondary(ui, "Choose folder…").clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                self.options.install = Some(folder);
                self.save();
                self.load_artwork();
            }
            if self.options.install.is_some()
                && theme::secondary(ui, "Search automatically").clicked()
            {
                self.options.install = None;
                self.save();
                self.load_artwork();
            }
        });
        theme::section(ui, "Start");
        let mut changed = false;
        theme::row(
            ui,
            "Skip the introduction movie",
            "Enter a new game without it.",
            |ui| changed = theme::toggle(ui, &mut self.options.skip_movies).changed(),
        );
        if changed {
            self.save();
        }
    }

    fn mods_page(&mut self, ui: &mut egui::Ui) {
        if self.installed.mods.is_empty() && self.installed.problems.is_empty() {
            theme::section(ui, "No mods installed");
            ui.label(theme::muted(&format!(
                "Mods go in {}.",
                mods::mods_dir().display()
            )));
            return;
        }
        theme::section(ui, "Mods");
        ui.label(theme::muted("Active mods load when the game starts."));
        let mut changed = false;
        egui::ScrollArea::vertical()
            .max_height(330.0)
            .show(ui, |ui| {
                for entry in &self.installed.mods {
                    let byline = match (&entry.version, &entry.author) {
                        (Some(version), Some(author)) => format!("v{version} by {author}. "),
                        (Some(version), None) => format!("v{version}. "),
                        (None, Some(author)) => format!("By {author}. "),
                        (None, None) => String::new(),
                    };
                    let mut on = self.active.contains(&entry.id);
                    theme::row(
                        ui,
                        &entry.name,
                        &format!("{byline}{}", entry.description),
                        |ui| {
                            if theme::toggle(ui, &mut on).changed() {
                                changed = true;
                            }
                        },
                    );
                    if on {
                        self.active.insert(entry.id.clone());
                    } else {
                        self.active.remove(&entry.id);
                    }
                }
                for (folder, reason) in &self.installed.problems {
                    ui.label(egui::RichText::new(format!("{folder}: {reason}")).color(ERROR));
                }
            });
        if changed {
            self.save();
        }
    }

    fn cheats_page(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Cheats");
        ui.label(theme::muted(
            "They change how the game plays and are kept in your cyrus settings, not in the game folder.",
        ));
        let mut changed = false;
        for cheat in Cheat::ALL {
            let mut on = self.cheats.is_on(cheat);
            theme::row(ui, cheat.label(), cheat.detail(), |ui| {
                if theme::toggle(ui, &mut on).changed() {
                    self.cheats.set(cheat, on);
                    changed = true;
                }
            });
        }
        ui.add_space(6.0);
        if ui
            .add_enabled(self.cheats.any_on(), egui::Button::new("Turn all off"))
            .clicked()
        {
            self.cheats.clear();
            changed = true;
        }
        if changed {
            self.problem = cheats::save(&cheats::local_file(), &self.cheats)
                .err()
                .map(|error| format!("{error:#}"));
        }
    }

    fn graphics_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        theme::section(ui, "Presentation");
        ui.label(theme::muted(
            "Faithful matches the original. Modern changes only how it looks and sounds.",
        ));
        ui.horizontal(|ui| {
            let uniform = self.choices.uniform();
            if ui
                .selectable_label(uniform == Some(Mode::Faithful), "All faithful")
                .clicked()
            {
                self.choices = Choices::all(Mode::Faithful);
                changed = true;
            }
            if ui
                .selectable_label(uniform == Some(Mode::Modern), "All modern")
                .clicked()
            {
                self.choices = Choices::all(Mode::Modern);
                changed = true;
            }
            if uniform.is_none() {
                ui.label(theme::muted("Custom"));
            }
        });
        egui::ScrollArea::vertical()
            .max_height(230.0)
            .show(ui, |ui| {
                for area in Area::ALL {
                    let mut modern = self.choices.mode(area) == Mode::Modern;
                    theme::row(ui, area.label(), area.modern_effect(), |ui| {
                        if theme::toggle(ui, &mut modern).changed() {
                            self.choices
                                .set(area, if modern { Mode::Modern } else { Mode::Faithful });
                            changed = true;
                        }
                    });
                }
            });
        if changed {
            self.save();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(40, 28, 18)))
            .show(ui, |ui| self.screen(ui));
        self.dialogs(&context);
        // Enter starts the game and Escape backs out, as in the original.
        if self.dialog == Dialog::None {
            if context.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.play(&context);
            }
        } else if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.dialog = Dialog::None;
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("cyrus")
            .with_inner_size([WIDTH, HEIGHT])
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    eframe::run_native(
        "cyrus launcher",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}
