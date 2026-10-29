//! MENU.INI-derived presentation data, shared by native and browser builds.
use super::StartupContent;
use super::book_motion::BookMotion;
use super::fit_image;
use super::settings::{adjust_value, apply_bindings, persist};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    input::mouse::MouseButton,
    prelude::*,
};
use cyrus_assets::menu::MenuAsset;
pub use cyrus_assets::menu::MenuItem;
use cyrus_assets::preferences::MenuPreferences;
use cyrus_game::AppState;
use cyrus_game::clock::{ClockCheckpoint, SimulationClock, bios_tick};
use cyrus_world::sound::SoundCue;
use serde::Serialize;
use std::{collections::BTreeMap, hash::BuildHasher};

/// Cinematic progress exposed through the existing menu control state.
#[derive(Clone, Serialize)]
pub struct MovieStatus {
    /// Installed filename; empty until preparation completes.
    pub name: String,
    /// Last presented one-based frame, or zero before the first frame.
    pub frame: u32,
    /// Source frame count, or zero before preparation completes.
    pub frames: u32,
    /// Waiting for preparation or the first decoded frame.
    pub loading: bool,
}

#[derive(Message)]
pub(super) enum MovieRequest {
    Menu(usize),
    NewGame,
}

/// What the startup book does by itself once it exists, instead of waiting for a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchAction {
    /// The New Game entry.
    NewGame,
    /// The Movies-page cinematic with this zero-based MENU.INI index; hidden entries play too.
    Movie(usize),
}

/// Launch-time shortcuts through the book. The action runs once; `skip_movies` stays for the session.
#[derive(Resource, Default, Debug)]
pub struct LaunchRequest {
    pub action: Option<LaunchAction>,
    /// New Game enters its world without the introduction movie.
    pub skip_movies: bool,
}

impl LaunchRequest {
    /// The shared launch options in the book's terms.
    pub fn new(new_game: bool, movie: Option<usize>, skip_movies: bool) -> Self {
        Self {
            action: if new_game {
                Some(LaunchAction::NewGame)
            } else {
                movie.map(LaunchAction::Movie)
            },
            skip_movies,
        }
    }
}

struct MenuTiming {
    checkpoint: ClockCheckpoint,
    last_tick: u32,
}

pub(super) struct DeleteConfirmation {
    slot: super::SaveSlot,
    pub dialog: super::confirmation::LastGameConfirmation,
    pub background: image::RgbaImage,
}

impl DeleteConfirmation {
    pub fn paint(&self, book: &MenuAsset) -> anyhow::Result<image::RgbaImage> {
        self.dialog.paint(
            &book.confirmation,
            &self.background,
            &format!("DELETE GAME?\n{}", self.slot.description),
        )
    }
}

/// Original caller context selects motion and the main-page restrictions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuContext {
    #[default]
    Startup,
    Death,
    /// Escape from live gameplay: the ordinary in-game book.
    InGame,
    LastGame,
}

impl MenuContext {
    /// Contexts entered from a running world, whose Continue and Quit use the paused world.
    fn is_from_world(self) -> bool {
        matches!(self, Self::Death | Self::InGame)
    }
}

/// Current installed menu selection and explicitly unsupported activation.
#[derive(Resource, Default, Serialize)]
pub struct MenuState {
    pub context: MenuContext,
    #[serde(skip)]
    pub(super) gameplay_thumbnail: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_game: Option<super::last_game::Presentation>,
    /// True once the installed book is rendered and input can be accepted.
    pub ready: bool,
    pub page: u8,
    pub capturing: Option<i32>,
    /// Cinematic loading/playback; menu input is suspended while present.
    pub movie: Option<MovieStatus>,
    #[serde(skip)]
    pub(super) enter_world: bool,
    pub preferences: Option<MenuPreferences>,
    pub entries: Vec<MenuItem>,
    pub selected: Option<usize>,
    pub slots: Vec<super::SaveSlot>,
    #[serde(skip)]
    pub(super) list_page: usize,
    #[serde(skip)]
    pub(super) slot_rows: BTreeMap<usize, usize>,
    #[serde(skip)]
    pub(super) delete_confirmation: Option<DeleteConfirmation>,
    #[serde(skip)]
    pub(super) description_editor: Option<super::save_load::DescriptionEditor>,
    pub unavailable: Option<String>,
    #[serde(skip)]
    pub(super) image: Handle<Image>,
    #[serde(skip)]
    pub(super) selections: BTreeMap<u8, Option<usize>>,
    #[serde(skip)]
    pub(super) blink_on: bool,
    #[serde(skip)]
    blink_seconds: f32,
    #[serde(skip)]
    pub(super) dirty: bool,
    #[serde(skip)]
    pub(super) persistence_error: Option<String>,
    #[serde(skip)]
    pub(super) motion: Option<BookMotion>,
    #[serde(skip)]
    timing: Option<MenuTiming>,
    #[serde(skip)]
    closing: bool,
    #[serde(skip)]
    pub(super) asset: Handle<MenuAsset>,
    #[serde(skip)]
    repeat_seconds: f32,
    #[serde(skip)]
    held_seconds: f32,
    #[serde(skip)]
    pub(super) main_disabled: [bool; 2],
    /// Page a gameplay shortcut opens directly; zero for the main page.
    #[serde(skip)]
    entry_page: u8,
    /// Quit waits for its automatic-save preparation to finish.
    #[serde(skip)]
    quit_pending: bool,
}

impl MenuState {
    pub(super) fn main_entry_disabled(&self, page: u8, index: usize) -> bool {
        page == 0
            && match index {
                0 => self.main_disabled[0],
                5 => self.main_disabled[1],
                _ => false,
            }
    }

    // shortcut: only the first list page is exposed; #197 owns later-page input and selection.
    fn fill_load_rows(&mut self) {
        self.slot_rows.clear();
        for (row, (index, entry)) in self
            .entries
            .iter_mut()
            .enumerate()
            .filter(|(_, entry)| matches!(entry.action, 102 | 202))
            .enumerate()
        {
            let base = if self.page == 1 {
                if self.list_page == 0 {
                    0
                } else {
                    8 + (self.list_page - 1) * 9
                }
            } else {
                self.list_page * 9
            };
            let slot_index = self
                .slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| self.page != 1 || (slot.slot != 0 && slot.slot != 1))
                .skip(base)
                .nth(row)
                .map(|(index, _)| index);
            let slot = slot_index.and_then(|index| self.slots.get(index));
            entry.text = slot
                .map(|slot| slot.description.chars().take(40).collect())
                .unwrap_or_default();
            entry.grayed = slot.is_none();
            if let Some(slot_index) = slot_index {
                self.slot_rows.insert(index, slot_index);
            }
        }
        if self
            .selected
            .is_none_or(|index| !self.entries[index].eligible())
        {
            self.selected = self.entries.iter().position(|entry| entry.eligible());
        }
        self.dirty = true;
    }

    fn change_list_page(&mut self, direction: isize) {
        self.list_page = (self.list_page as isize + direction).max(0) as usize;
        self.fill_load_rows();
        self.selected = self.entries.iter().position(MenuItem::eligible);
    }

    fn navigate(&mut self, direction: isize) -> bool {
        let Some(selected) = self.selected else {
            return false;
        };
        let len = self.entries.len();
        for step in 1..=len {
            let index =
                (selected as isize + direction * step as isize).rem_euclid(len as isize) as usize;
            if self.entries[index].eligible() {
                self.selected = Some(index);
                self.unavailable = None;
                return true;
            }
        }
        false
    }
}

#[derive(Default, TypePath)]
struct MenuLoader;
impl AssetLoader for MenuLoader {
    type Asset = MenuAsset;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<MenuAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let menu: MenuAsset = serde_json::from_slice(&bytes)?;
        menu.validate()?;
        Ok(menu)
    }
    fn extensions(&self) -> &[&str] {
        &["menu"]
    }
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<cyrus_game::preferences::Preferences>()
        .init_resource::<super::SaveLoadHost>()
        .init_resource::<LaunchRequest>()
        .add_message::<MovieRequest>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .init_asset::<MenuAsset>()
        .init_asset_loader::<MenuLoader>()
        .add_systems(OnEnter(AppState::Menu), (pause_cameras, setup))
        .add_systems(OnEnter(AppState::InGame), restore_cameras)
        .add_systems(
            Update,
            (
                sync_thumbnail.before(drive),
                fit_image,
                super::last_game::drive.before(drive),
                description_text.before(drive),
                drive.before(cyrus_audio::ApplyPreferences),
                launch.after(drive),
            )
                .run_if(in_state(AppState::Menu)),
        )
        .add_systems(OnExit(AppState::Menu), cleanup);
}

fn sync_thumbnail(capture: Option<Res<super::GameplayThumbnail>>, mut menu: ResMut<MenuState>) {
    let current = capture.and_then(|capture| capture.0.clone());
    if menu.gameplay_thumbnail != current {
        menu.gameplay_thumbnail = current;
        menu.dirty = true;
    }
}

// Only the browser host needs `commands`; the native book exits through its closing motion.
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
fn begin_quit(menu: &mut MenuState, commands: &mut Commands) {
    menu.quit_pending = false;
    // The browser host owns closing; the native book plays its closing motion first.
    #[cfg(target_arch = "wasm32")]
    commands.write_message(super::QuitRequest);
    #[cfg(not(target_arch = "wasm32"))]
    {
        menu.motion.as_mut().unwrap().close();
        menu.closing = true;
    }
}

/// Session reset the New Game entry performs before its movie or world entry.
fn reset_for_new_game(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        world
            .resource_mut::<cyrus_game::script::ScriptActors>()
            .begin_new_game();
        // The original stops every sound channel before its New Game initializer
        // (preservation engine/menu.md); world entry does not.
        world.write_message(cyrus_audio::sound::SoundRequest::StopAll);
        if let Some(hud) = world.get_resource::<cyrus_game::script::ScriptHud>() {
            hud.begin_new_game();
        }
    });
}

/// Runs the launch request once the book has its opening motion, as a key press on the entry would.
fn launch(
    mut request: ResMut<LaunchRequest>,
    menu: Res<MenuState>,
    mut commands: Commands,
    mut movies: MessageWriter<MovieRequest>,
) {
    if request.action.is_none()
        || menu.motion.is_none()
        || menu.movie.is_some()
        || menu.last_game.is_some()
        || menu.context != MenuContext::Startup
    {
        return;
    }
    match request.action.take() {
        Some(LaunchAction::NewGame) => {
            reset_for_new_game(&mut commands);
            movies.write(MovieRequest::NewGame);
        }
        Some(LaunchAction::Movie(index)) => {
            movies.write(MovieRequest::Menu(index));
        }
        None => {}
    }
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    startup: Res<super::StartupPresentation>,
    (death, pause): (
        Option<Res<cyrus_game::script::DeathBookEntry>>,
        Option<Res<cyrus_game::PauseBookEntry>>,
    ),
    last_game: Option<Res<cyrus_game::script::LastGameEntry>>,
    mut dispatch: Option<ResMut<cyrus_game::script::DeathDispatch>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    if let Some(entry) = last_game.as_deref() {
        super::last_game::request_capture(&mut commands, &windows);
        commands.insert_resource(MenuState {
            context: MenuContext::LastGame,
            last_game: Some(super::last_game::Presentation::new(entry)),
            asset: if startup.menu == Handle::default() {
                assets.load("cache://menu/book.menu")
            } else {
                startup.menu.clone()
            },
            ..default()
        });
        return;
    }
    if let Some(dispatch) = dispatch.as_deref_mut() {
        dispatch.book_context = Some(if death.is_some() { 3 } else { 1 });
    }
    commands.spawn((Camera2d, StartupContent));
    commands.spawn((Sprite::default(), StartupContent));
    commands.insert_resource(MenuState {
        context: if death.is_some() {
            MenuContext::Death
        } else if pause.is_some() {
            MenuContext::InGame
        } else {
            MenuContext::Startup
        },
        entry_page: pause.as_ref().map_or(0, |entry| entry.page),
        main_disabled: match (&death, &pause) {
            (Some(entry), _) => [entry.override_group != 0; 2],
            (None, Some(_)) => [false; 2],
            (None, None) => [true, true],
        },
        asset: if startup.menu == Handle::default() {
            assets.load("cache://menu/book.menu")
        } else {
            startup.menu.clone()
        },
        ..default()
    });
}

/// Frame time, keyboard, real time and mouse buttons (absent in headless hosts;
/// Controls capture reads their releases).
type Inputs<'w> = (
    Res<'w, Time>,
    Res<'w, ButtonInput<KeyCode>>,
    Res<'w, Time<Real>>,
    Option<Res<'w, ButtonInput<MouseButton>>>,
);

type MenuRuntime<'w, 's> = (
    ResMut<'w, cyrus_game::preferences::Preferences>,
    ResMut<'w, cyrus_game::player::input::PlayerBindings>,
    Commands<'w, 's>,
    ResMut<'w, NextState<AppState>>,
    ResMut<'w, super::SaveLoadHost>,
);

fn description_text(
    mut events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut menu: ResMut<MenuState>,
) {
    for event in events.read() {
        if event.state != bevy::input::ButtonState::Pressed
            || matches!(
                event.key_code,
                KeyCode::Enter | KeyCode::Backspace | KeyCode::Escape
            )
        {
            continue;
        }
        if let Some(editor) = &mut menu.description_editor
            && let Some(text) = &event.text
        {
            editor.append(text);
            menu.dirty = true;
        }
    }
}

pub(super) fn drive(
    assets: (Res<AssetServer>, Res<Assets<MenuAsset>>),
    clock: Option<ResMut<SimulationClock>>,
    input: Inputs,
    mut menu: ResMut<MenuState>,
    rendering: (
        ResMut<Assets<Image>>,
        Query<&mut Sprite, With<StartupContent>>,
    ),
    mut output: (
        MessageWriter<AppExit>,
        Query<&mut Text, With<MenuNotice>>,
        MessageWriter<SoundCue>,
    ),
    runtime: MenuRuntime,
) {
    if menu.last_game.is_some() {
        return;
    }
    let (assets, definitions) = assets;
    let (mut images, mut sprite) = rendering;
    let (mut preferences_state, mut bindings, mut commands, mut next, mut save_load) = runtime;
    if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(menu.asset.id()) {
        if menu.unavailable.is_none() {
            let text = format!("Menu assets unavailable: {error}");
            error!("{text}");
            menu.unavailable = Some(text.clone());
            notice(&mut commands, &assets, text);
        }
        return;
    }
    let Some(book) = definitions.get(&menu.asset) else {
        return;
    };
    let Some(mut clock) = clock else {
        match SimulationClock::new(book.clock) {
            Ok(clock) => commands.insert_resource(clock),
            Err(error) => {
                let text = format!("Menu clock unavailable: {error:#}");
                if menu.unavailable.as_ref() != Some(&text) {
                    notice(&mut commands, &assets, text.clone());
                    menu.unavailable = Some(text);
                }
                menu.ready = false;
            }
        }
        return;
    };
    if clock.settings() != book.clock {
        menu.ready = false;
        let text = "Menu clock settings differ from the active game's installed settings.";
        if menu.unavailable.as_deref() != Some(text) {
            notice(&mut commands, &assets, text.into());
            menu.unavailable = Some(text.into());
        }
        return;
    }
    let sample = bios_tick(input.2.elapsed());
    let timing = menu.timing.get_or_insert_with(|| MenuTiming {
        checkpoint: clock.checkpoint(),
        last_tick: sample,
    });
    let menu_tick = timing.last_tick != sample;
    if menu.preferences.is_none() {
        menu.preferences = Some(match preferences_state.menu().clone() {
            Ok(Some(preferences)) => preferences,
            Ok(None) => book.defaults.clone(),
            Err(error) => {
                let message =
                    format!("Menu preferences unavailable: {error}. Changes are disabled.");
                menu.persistence_error = Some(message.clone());
                menu.unavailable = Some(message.clone());
                notice(&mut commands, &assets, message);
                book.defaults.clone()
            }
        });
        menu.selections = book
            .pages
            .iter()
            .map(|(id, page)| (*id, page.selected))
            .collect();
        menu.repeat_seconds = book.repeat_seconds;
        let frames = std::array::from_fn(|i| book.models[i].frames.len());
        menu.motion = Some(match menu.context {
            MenuContext::Startup => BookMotion::opening(frames),
            MenuContext::InGame if menu.entry_page >= 5 => {
                BookMotion::settled_on(menu.entry_page, frames)
            }
            MenuContext::InGame if menu.entry_page != 0 => BookMotion::settled(frames),
            MenuContext::Death | MenuContext::InGame => BookMotion::death_entry(frames),
            MenuContext::LastGame => return,
        });
        // The first page opened needs no transition; a settings shortcut never shows the main page.
        if menu.entry_page < 5 {
            menu.open(book, 0);
        }
        if menu.entry_page != 0 {
            let entry_page = menu.entry_page;
            menu.open(book, entry_page);
            if matches!(entry_page, 1 | 2)
                && let Err(error) = save_load.start(super::SaveLoadRequest::List)
            {
                menu.unavailable = Some(error);
            }
        }
        menu.blink_on = true;
        apply_bindings(&menu, &mut bindings);
    }
    if let Some(result) = save_load.take_result() {
        match result {
            Ok(_) if menu.quit_pending => begin_quit(&mut menu, &mut commands),
            Ok(value) if value.get("slots").is_some() => {
                match serde_json::from_value::<Vec<super::SaveSlot>>(value["slots"].clone()) {
                    Ok(slots) => {
                        menu.slots = slots;
                        menu.unavailable = menu.persistence_error.clone();
                        if matches!(menu.page, 1 | 2) {
                            menu.fill_load_rows();
                        }
                    }
                    Err(error) => menu.unavailable = Some(format!("Save list: {error}")),
                }
            }
            Ok(value) if value.get("deleted").is_some() || value.get("slot").is_some() => {
                if let Err(error) = save_load.start(super::SaveLoadRequest::List) {
                    menu.unavailable = Some(error);
                }
            }
            Ok(_) => {}
            Err(error) => {
                menu.quit_pending = false;
                menu.unavailable = Some(error);
            }
        }
        menu.dirty = true;
    }
    if menu.enter_world {
        menu.enter_world = false;
        begin_world_entry(
            &mut commands,
            &mut images,
            &mut next,
            book,
            input.0.elapsed_secs_f64(),
        );
        menu.ready = false;
        return;
    }
    if menu.movie.is_some() {
        return;
    }
    let mut moving = menu.motion.as_ref().is_some_and(BookMotion::busy);
    if moving && menu_tick {
        menu.motion.as_mut().unwrap().tick(|effect| {
            output.2.write(flat_sound(effect));
        });
        menu.dirty = true;
    }
    if menu_tick {
        clock.advance(sample);
        menu.timing.as_mut().unwrap().last_tick = sample;
    }
    if menu.closing && !moving && menu_tick {
        output.0.write(AppExit::Success);
        return;
    }
    let keys = &input.1;
    let previous = (menu.page, menu.selected, menu.capturing, menu.blink_on);
    if !moving && !menu.closing && !save_load.busy() {
        if let Some(mut editor) = menu.description_editor.take() {
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            if keys.just_pressed(KeyCode::Escape) {
                menu.dirty = true;
            } else if keys.just_pressed(KeyCode::Enter) && !shift && editor.request().is_some() {
                if let Err(error) = save_load.start(editor.request().unwrap()) {
                    menu.unavailable = Some(error);
                }
                menu.dirty = true;
            } else {
                if keys.just_pressed(KeyCode::Backspace) {
                    if shift {
                        editor.append("\r");
                    } else {
                        editor.text.pop();
                    }
                }
                if keys.just_pressed(KeyCode::Enter) && shift {
                    editor.append("\u{b}");
                }
                menu.description_editor = Some(editor);
                menu.dirty = true;
            }
        } else if let Some(mut confirmation) = menu.delete_confirmation.take() {
            let logical = bindings.sample_with_mouse(keys, input.3.as_deref());
            let result = confirmation.dialog.update(
                sample,
                super::confirmation::ConfirmationInput {
                    up: logical.forward,
                    down: logical.backward,
                    enter: keys.pressed(KeyCode::Enter),
                    action_d: logical.action,
                },
            );
            match result {
                Some(super::confirmation::ConfirmationChoice::Yes) => {
                    if let Err(error) = save_load.start(super::SaveLoadRequest::Delete {
                        slot: confirmation.slot.slot,
                        revision: confirmation.slot.revision,
                    }) {
                        menu.unavailable = Some(error);
                    }
                }
                Some(super::confirmation::ConfirmationChoice::No) => {}
                None => menu.delete_confirmation = Some(confirmation),
            }
            menu.dirty = true;
        } else if matches!(menu.page, 1 | 2) && keys.just_pressed(KeyCode::Delete) {
            let slot = menu
                .selected
                .and_then(|index| menu.slot_rows.get(&index))
                .and_then(|row| menu.slots.get(*row))
                .cloned();
            if let Some(slot) = slot {
                menu.unavailable = menu.persistence_error.clone();
                match super::menu_raster::paint(book, &menu) {
                    Ok(background) => {
                        menu.delete_confirmation = Some(DeleteConfirmation {
                            slot,
                            dialog: super::confirmation::LastGameConfirmation::displayed(0, sample),
                            background,
                        })
                    }
                    Err(error) => {
                        menu.unavailable = Some(format!("Delete confirmation: {error:#}"))
                    }
                }
                menu.dirty = true;
            }
        } else if matches!(menu.page, 1 | 2) && keys.just_pressed(KeyCode::PageDown) {
            menu.change_list_page(1);
        } else if matches!(menu.page, 1 | 2) && keys.just_pressed(KeyCode::PageUp) {
            menu.change_list_page(-1);
        } else if menu.capturing.is_some() {
            menu.blink_seconds += input.0.delta_secs();
            // Blink cadence is presentation policy; the original toggles on menu updates.
            if menu.blink_seconds >= 0.25 {
                menu.blink_on = !menu.blink_on;
                menu.blink_seconds %= 0.25;
            }
            if keys.just_pressed(KeyCode::Escape) {
                menu.capturing = None;
            } else {
                let captured = keys
                    .get_just_pressed()
                    .filter_map(|key| cyrus_game::player::input::scan_code(*key))
                    .min()
                    .or_else(|| {
                        input
                            .3
                            .as_deref()
                            .and_then(cyrus_game::player::input::mouse_release_code)
                    });
                if let Some(code) = captured {
                    if matches!(code, 12 | 13 | 15 | 28 | 59..=68 | 87 | 88) {
                        menu.unavailable = Some("That key is reserved.".into());
                        output.2.write(flat_sound(18));
                    } else {
                        let mut preferences = menu.preferences.clone().unwrap();
                        preferences.rebind((menu.capturing.unwrap() - 710) as usize, code);
                        if persist(&mut menu, preferences, &mut preferences_state) {
                            menu.capturing = None;
                            apply_bindings(&menu, &mut bindings);
                            output.2.write(flat_sound(13));
                        }
                    }
                }
            }
        } else if keys.just_released(KeyCode::Escape) {
            if menu.page != 0 {
                let target = if matches!(menu.page, 5..=7) { 4 } else { 0 };
                menu.open(book, target);
            } else if menu.context.is_from_world() && !menu.main_disabled[1] {
                next.set(AppState::InGame);
            }
        } else {
            let directions = [
                (KeyCode::ArrowUp, -1, false),
                (KeyCode::ArrowDown, 1, false),
                (KeyCode::ArrowLeft, -1, true),
                (KeyCode::ArrowRight, 1, true),
            ];
            let held: Vec<_> = directions
                .into_iter()
                .filter(|(key, _, _)| keys.pressed(*key))
                .collect();
            if held.len() != 1 {
                menu.held_seconds = 0.0;
            } else {
                let (key, direction, adjust) = held[0];
                menu.held_seconds += input.0.delta_secs();
                if keys.just_pressed(key) || menu.held_seconds >= menu.repeat_seconds {
                    menu.held_seconds = 0.0;
                    let changed = if adjust {
                        adjust_value(
                            book,
                            &mut menu,
                            direction as i32,
                            false,
                            &mut preferences_state,
                        )
                    } else {
                        menu.navigate(direction)
                    };
                    if changed {
                        output.2.write(flat_sound(13));
                    }
                }
            }
            let activation = menu
                .preferences
                .as_ref()
                .and_then(|p| cyrus_game::player::input::scan_key(p.bindings[5]));
            if (keys.just_pressed(KeyCode::Enter)
                || activation.is_some_and(|key| keys.just_pressed(key)))
                && let Some(index) = menu.selected
            {
                let entry = menu.entries[index].clone();
                match entry.action {
                    -1 if menu.context.is_from_world() && !menu.main_disabled[1] => {
                        next.set(AppState::InGame);
                    }
                    -2 if menu.context.is_from_world() => {
                        // Preservation engine/menu.md: Quit saves the live world, or removes
                        // the automatic save once the death override is active.
                        let request = if menu.main_disabled[0] {
                            Some(super::SaveLoadRequest::DiscardAutoSave)
                        } else {
                            menu.gameplay_thumbnail
                                .is_some()
                                .then_some(super::SaveLoadRequest::AutoSave)
                        };
                        match request.map(|request| save_load.start(request)) {
                            None => begin_quit(&mut menu, &mut commands),
                            Some(Ok(())) => menu.quit_pending = true,
                            Some(Err(error)) => menu.unavailable = Some(error),
                        }
                    }
                    -2 => begin_quit(&mut menu, &mut commands),
                    1 | 2 => {
                        menu.open(book, entry.action as u8);
                        if let Err(error) = save_load.start(super::SaveLoadRequest::List) {
                            menu.unavailable = Some(error);
                        }
                    }
                    10 => {
                        reset_for_new_game(&mut commands);
                        commands.write_message(MovieRequest::NewGame);
                        menu.ready = false;
                        return;
                    }
                    202 => {
                        if let Some(slot) = menu
                            .slot_rows
                            .get(&index)
                            .and_then(|row| menu.slots.get(*row))
                        {
                            let request = super::SaveLoadRequest::Load {
                                slot: slot.slot,
                                revision: slot.revision.clone(),
                            };
                            if let Err(error) = save_load.start(request) {
                                menu.unavailable = Some(error);
                            }
                        }
                    }
                    101 | 102 => {
                        let slot = menu
                            .slot_rows
                            .get(&index)
                            .and_then(|row| menu.slots.get(*row))
                            .cloned();
                        if entry.action == 101 || slot.is_some() {
                            match super::menu_raster::paint(book, &menu) {
                                Ok(background) => {
                                    menu.description_editor =
                                        Some(super::save_load::DescriptionEditor {
                                            text: slot
                                                .as_ref()
                                                .map(|slot| slot.description.clone())
                                                .unwrap_or_default(),
                                            slot,
                                            background,
                                        })
                                }
                                Err(error) => {
                                    menu.unavailable = Some(format!("Game Description: {error:#}"))
                                }
                            }
                            menu.dirty = true;
                        }
                    }
                    100 | 200 => menu.open(book, 0),
                    3 | 4 => menu.open(book, entry.action as u8),
                    300 | 400 => menu.open(book, 0),
                    405..=407 => menu.open(book, (entry.action - 400) as u8),
                    500 | 600 | 700 => menu.open(book, 4),
                    504 | 505 | 603 => {
                        adjust_value(book, &mut menu, 0, true, &mut preferences_state);
                    }
                    601 | 602 => {}
                    301..=305 => {
                        menu.unavailable = None;
                        commands.write_message(MovieRequest::Menu((entry.action - 301) as usize));
                    }
                    710..=724 => {
                        menu.capturing = Some(entry.action);
                        menu.blink_on = true;
                        menu.blink_seconds = 0.0;
                    }
                    _ => {
                        menu.unavailable = Some(format!("{} is not implemented yet.", entry.text));
                    }
                }
            }
        }
    }
    moving |= menu.motion.as_ref().is_some_and(BookMotion::busy);
    let page = menu.page;
    let selected = menu.selected;
    menu.selections.insert(page, selected);
    menu.dirty |= previous != (menu.page, menu.selected, menu.capturing, menu.blink_on);
    if menu.dirty {
        match super::menu_raster::paint(book, &menu) {
            Ok(pixels) => {
                let image = Image::new(
                    bevy::render::render_resource::Extent3d {
                        width: 640,
                        height: 480,
                        depth_or_array_layers: 1,
                    },
                    bevy::render::render_resource::TextureDimension::D2,
                    pixels.into_raw(),
                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::default(),
                );
                if menu.image == Handle::default() {
                    menu.image = images.add(image);
                } else if let Some(mut existing) = images.get_mut(&menu.image) {
                    *existing = image;
                } else {
                    menu.image = images.add(image);
                }
                if let Ok(mut sprite) = sprite.single_mut() {
                    sprite.image = menu.image.clone();
                }
                menu.ready = !moving && !menu.closing;
                menu.dirty = false;
            }
            Err(error) => {
                menu.ready = false;
                menu.unavailable = Some(format!("Menu rendering unavailable: {error}"));
            }
        }
    }
    if !menu.dirty {
        menu.ready = !moving && !menu.closing;
    }
    let message = menu.unavailable.clone().unwrap_or_default();
    if let Ok(mut notice) = output.1.single_mut() {
        **notice = message;
    } else if !message.is_empty() {
        notice(&mut commands, &assets, message);
    }
}

impl MenuState {
    fn open(&mut self, book: &MenuAsset, page: u8) {
        if !self.entries.is_empty() {
            if let Err(error) = self.motion.as_mut().unwrap().navigate(self.page, page) {
                self.unavailable = Some(error);
                return;
            }
            self.selections.insert(self.page, self.selected);
        }
        self.page = page;
        self.list_page = 0;
        self.entries = book.pages[&page]
            .entries
            .iter()
            .enumerate()
            .map(|(index, e)| {
                let mut item = e.item.clone();
                item.grayed |= self.main_entry_disabled(page, index);
                item
            })
            .collect();
        if matches!(page, 1 | 2) {
            self.fill_load_rows();
        }
        let start = self.selections.get(&page).copied().flatten().unwrap_or(0);
        self.selected = (0..self.entries.len())
            .map(|step| (start + step) % self.entries.len())
            .find(|index| self.entries[*index].eligible());
        self.selections.insert(page, self.selected);
        self.unavailable = self.persistence_error.clone();
        self.capturing = None;
        self.held_seconds = 0.0;
        self.dirty = true;
    }
}

fn flat_sound(effect: usize) -> SoundCue {
    SoundCue::Flat {
        effect,
        volume: 0,
        rate_offset_hz: 0,
    }
}

#[derive(Component)]
pub(super) struct MenuNotice;
pub(super) fn notice(commands: &mut Commands, assets: &AssetServer, text: String) {
    commands.spawn((
        StartupContent,
        MenuNotice,
        Text::new(text),
        TextFont {
            font: assets.load("cache://menu/font.ttf").into(),
            font_size: FontSize::Px(20.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: px(8),
            left: px(8),
            ..default()
        },
    ));
}
fn cleanup(
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    clock: Option<ResMut<SimulationClock>>,
    time: Res<Time<Real>>,
    restored: Option<Res<cyrus_game::RestoredSessionEntry>>,
    death: Option<Res<cyrus_game::script::DeathBookEntry>>,
    actors: Option<ResMut<cyrus_game::script::ScriptActors>>,
) {
    if death.is_some() {
        if restored.is_none()
            && let Some(mut actors) = actors
            && let Err(error) = actors.finish_death_book()
        {
            actors.report_runtime_error(format!("{error:#}"));
        }
        commands.remove_resource::<cyrus_game::script::DeathBookEntry>();
    }
    if restored.is_none()
        && let Some(mut clock) = clock
        && let Some(timing) = menu.timing.take()
    {
        clock.restore(timing.checkpoint, bios_tick(time.elapsed()));
    }
    commands.remove_resource::<cyrus_game::script::LastGameEntry>();
    commands.remove_resource::<super::last_game::CapturedFrame>();
    commands.remove_resource::<MenuState>();
    commands.remove_resource::<cyrus_game::PausedGameplay>();
    commands.remove_resource::<cyrus_game::PauseBookEntry>();
}

#[derive(Component)]
pub(super) struct BookPausedCamera;

fn pause_cameras(
    mut commands: Commands,
    mut cameras: Query<(Entity, &mut Camera), With<Camera3d>>,
    confirmation: Option<Res<cyrus_game::script::LastGameEntry>>,
    death: Option<Res<cyrus_game::script::DeathBookEntry>>,
    pause: Option<Res<cyrus_game::PauseBookEntry>>,
    clock: Option<Res<SimulationClock>>,
) {
    if confirmation.is_some() {
        return;
    }
    // Every book entry stops all sound channels first (preservation engine/menu.md).
    if pause.is_some() || death.is_some() {
        commands.queue(|world: &mut World| {
            world.write_message(cyrus_audio::sound::SoundRequest::StopAll);
        });
    }
    let mut active_cameras = Vec::new();
    for (entity, mut camera) in &mut cameras {
        if camera.is_active {
            active_cameras.push(entity);
            camera.is_active = false;
            commands.entity(entity).insert(BookPausedCamera);
        }
    }
    if (pause.is_some() || death.is_some_and(|entry| entry.override_group == 0))
        && let Some(clock) = clock
    {
        commands.insert_resource(cyrus_game::PausedGameplay {
            clock: clock.clone(),
            active_cameras,
        });
    }
}

fn restore_cameras(
    mut commands: Commands,
    mut cameras: Query<(Entity, &mut Camera), With<BookPausedCamera>>,
) {
    for (entity, mut camera) in &mut cameras {
        camera.is_active = true;
        commands.entity(entity).remove::<BookPausedCamera>();
    }
}

/// Enter the installed starting world after a completed New Game introduction.
fn begin_world_entry(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    next: &mut NextState<AppState>,
    book: &MenuAsset,
    elapsed: f64,
) {
    commands.remove_resource::<cyrus_world::lifecycle::PreparedWorld>();
    commands.remove_resource::<cyrus_world::lifecycle::SceneLoadError>();
    commands.insert_resource(cyrus_world::lifecycle::SceneRequest(
        book.new_game.world.to_string(),
    ));
    commands.insert_resource(cyrus_game::player::placement::MarkerEntry {
        marker: book.new_game.marker as usize,
        heading: 0,
    });
    let variation = std::collections::hash_map::RandomState::new().hash_one(elapsed.to_bits())
        as usize
        % book.new_game.loading.len();
    let picture = &book.new_game.loading[variation];
    let image = images.add(Image::new(
        bevy::render::render_resource::Extent3d {
            width: picture.width.into(),
            height: picture.height.into(),
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        picture.rgba.clone(),
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    ));
    commands.insert_resource(super::StartGame {
        image,
        background: picture.rgba.clone(),
        font: book.new_game.font.clone(),
        progress: (0, 1),
        ..default()
    });
    next.set(AppState::Loading);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_request_runs_once_and_only_when_the_startup_book_is_open() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Messages<MovieRequest>>();
        world.insert_resource(LaunchRequest::new(false, Some(3), false));
        world.init_resource::<MenuState>();
        world.run_system_once(launch).unwrap();
        assert_eq!(
            world.resource::<LaunchRequest>().action,
            Some(LaunchAction::Movie(3))
        );
        world.resource_mut::<MenuState>().motion = Some(BookMotion::opening([1; 5]));
        world.run_system_once(launch).unwrap();
        world.run_system_once(launch).unwrap();
        assert_eq!(world.resource::<LaunchRequest>().action, None);
        let movies: Vec<_> = world
            .resource_mut::<Messages<MovieRequest>>()
            .drain()
            .collect();
        assert!(matches!(movies.as_slice(), [MovieRequest::Menu(3)]));
        assert_eq!(
            LaunchRequest::new(true, None, true).action,
            Some(LaunchAction::NewGame)
        );
    }

    #[test]
    fn restored_entry_does_not_resume_the_replaced_menu_clock() {
        let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 24,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        let checkpoint = clock.checkpoint();
        clock.advance(18);
        let expected = serde_json::to_value(clock.snapshot()).unwrap();
        let mut app = App::new();
        app.insert_resource(clock)
            .insert_resource(Time::<Real>::default())
            .insert_resource(cyrus_game::RestoredSessionEntry)
            .insert_resource(MenuState {
                timing: Some(MenuTiming {
                    checkpoint,
                    last_tick: 18,
                }),
                ..default()
            })
            .add_systems(Update, cleanup);
        app.update();
        assert!(!app.world().contains_resource::<MenuState>());
        assert_eq!(
            serde_json::to_value(app.world().resource::<SimulationClock>().snapshot()).unwrap(),
            expected
        );
    }

    #[test]
    fn menu_exit_restores_saved_clock_without_resetting_menu_warmup_or_last_step() {
        let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 24,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        let checkpoint = clock.checkpoint();
        let menu_step = clock.advance(18);
        let mut time = Time::<Real>::default();
        time.advance_by(std::time::Duration::from_secs(10));
        let sample = bios_tick(time.elapsed());
        let mut app = App::new();
        app.insert_resource(clock)
            .insert_resource(time)
            .insert_resource(MenuState {
                timing: Some(MenuTiming {
                    checkpoint,
                    last_tick: 18,
                }),
                ..default()
            })
            .add_systems(Update, cleanup);
        app.update();
        assert!(!app.world().contains_resource::<MenuState>());
        let mut clock = app.world_mut().resource_mut::<SimulationClock>();
        assert_eq!(clock.step(), menu_step);
        for _ in 0..11 {
            assert_eq!(clock.advance(sample).nominal_ticks, 1);
        }
        assert_eq!(clock.advance(sample).frame_scale, 512);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn installed_death_book_restricts_resume_and_never_loads_a_fresh_world() {
        use cyrus_assets::install::GameInstall;
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = GameInstall::open(&root) else {
            eprintln!("skipped: no install (death book)");
            return;
        };
        let cache = std::env::temp_dir().join(format!("cyrus-death-book-{}", std::process::id()));
        cyrus_assets::startup::export_menu(&install, &cache).unwrap();
        let book: MenuAsset =
            serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
        let clock = SimulationClock::new(book.clock).unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .init_state::<AppState>()
        .init_asset::<MenuAsset>()
        .init_asset::<Image>()
        .init_asset::<Font>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<cyrus_game::preferences::Preferences>()
        .init_resource::<cyrus_game::player::input::PlayerBindings>()
        .add_message::<AppExit>()
        .add_message::<SoundCue>()
        .init_resource::<crate::SaveLoadHost>()
        .insert_resource(clock)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(60),
        ))
        .add_systems(Update, drive);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<MenuAsset>>()
            .add(book);
        app.insert_resource(super::super::StartupPresentation {
            menu: handle,
            ..default()
        });
        app.insert_resource(cyrus_game::script::DeathBookEntry { override_group: 74 });
        bevy::ecs::system::RunSystemOnce::run_system_once(app.world_mut(), setup).unwrap();
        for _ in 0..12 {
            app.update();
        }
        let menu = app.world().resource::<MenuState>();
        assert!(menu.ready, "{:?}", menu.unavailable);
        assert!(menu.entries[0].grayed);
        assert!(menu.entries[5].grayed);
        assert_ne!(menu.selected, Some(0));
        assert_ne!(menu.selected, Some(5));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Escape);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Unchanged
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let load = app
            .world()
            .resource::<MenuState>()
            .entries
            .iter()
            .position(|entry| entry.action == 2)
            .unwrap();
        app.world_mut().resource_mut::<MenuState>().selected = Some(load);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world().resource::<MenuState>().page, 2);
        assert!(matches!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request(),
            Some(crate::SaveLoadRequest::List)
        ));
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Ok(serde_json::json!({"slots": [{
                "slot": 17, "description": "Harbour save", "revision": "harbour-r1"
            }]})));
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        for _ in 0..100 {
            app.update();
        }
        let row = app
            .world()
            .resource::<MenuState>()
            .entries
            .iter()
            .position(|entry| entry.action == 202 && entry.eligible())
            .unwrap();
        app.world_mut().resource_mut::<MenuState>().selected = Some(row);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(matches!(
            app.world_mut().resource_mut::<crate::SaveLoadHost>().take_request(),
            Some(crate::SaveLoadRequest::Load { slot: 17, revision }) if revision == "harbour-r1"
        ));
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Err("save slot 17 changed; refresh the save list".into()));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert_eq!(app.world().resource::<MenuState>().page, 2);
        assert_eq!(
            app.world().resource::<MenuState>().unavailable.as_deref(),
            Some("save slot 17 changed; refresh the save list")
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        assert!(
            app.world()
                .resource::<MenuState>()
                .delete_confirmation
                .is_some()
        );
        assert!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request()
                .is_none()
        );
        let down = app
            .world()
            .resource::<MenuState>()
            .preferences
            .as_ref()
            .unwrap()
            .bindings[1];
        let down = cyrus_game::player::input::scan_key(down).unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(down);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(
            app.world()
                .resource::<MenuState>()
                .delete_confirmation
                .is_none()
        );
        assert!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request()
                .is_none()
        );
        assert_eq!(app.world().resource::<MenuState>().slots.len(), 1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        // The dialog keeps ownership until the confirmation key is released.
        assert!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request()
                .is_none()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(matches!(
            app.world_mut().resource_mut::<crate::SaveLoadHost>().take_request(),
            Some(crate::SaveLoadRequest::Delete { slot: 17, revision }) if revision == "harbour-r1"
        ));
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Ok(serde_json::json!({"deleted":17})));
        app.update();
        assert!(matches!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request(),
            Some(crate::SaveLoadRequest::List)
        ));
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Ok(serde_json::json!({"slots":[]})));
        app.update();
        assert!(
            app.world()
                .resource::<MenuState>()
                .entries
                .iter()
                .filter(|entry| entry.action == 202)
                .all(|entry| !entry.eligible() && entry.text.is_empty())
        );
        app.world_mut()
            .resource_scope(|world, mut menu: Mut<MenuState>| {
                let books = world.resource::<Assets<MenuAsset>>();
                let handle = menu.asset.clone();
                menu.open(books.get(&handle).unwrap(), 0);
                while menu.motion.as_ref().is_some_and(BookMotion::busy) {
                    menu.motion.as_mut().unwrap().tick(|_| {});
                }
            });
        assert!(
            !app.world()
                .contains_resource::<cyrus_world::lifecycle::SceneRequest>()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let quit = app
            .world()
            .resource::<MenuState>()
            .entries
            .iter()
            .position(|entry| entry.action == -2)
            .unwrap();
        app.world_mut().resource_mut::<MenuState>().selected = Some(quit);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        // The death override removes the automatic save before Quit proceeds.
        assert!(matches!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request(),
            Some(crate::SaveLoadRequest::DiscardAutoSave)
        ));
        assert!(app.world().resource::<MenuState>().quit_pending);
        assert!(!app.world().resource::<MenuState>().closing);
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Err("disk full".into()));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        // The session adapter publishes the terminal result after the menu drive system.
        app.update();
        let menu = app.world().resource::<MenuState>();
        assert_eq!(menu.unavailable.as_deref(), Some("disk full"));
        assert!(!menu.quit_pending && !menu.closing);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_scope(|world, mut menu: Mut<MenuState>| {
                menu.main_disabled = [false; 2];
                menu.entries.clear();
                let books = world.resource::<Assets<MenuAsset>>();
                let handle = menu.asset.clone();
                menu.open(books.get(&handle).unwrap(), 0);
            });
        assert!(!app.world().resource::<MenuState>().entries[5].grayed);
        let save = app
            .world()
            .resource::<MenuState>()
            .entries
            .iter()
            .position(|entry| entry.action == 1)
            .unwrap();
        app.world_mut().resource_mut::<MenuState>().selected = Some(save);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world().resource::<MenuState>().page, 1);
        assert!(matches!(
            app.world_mut()
                .resource_mut::<crate::SaveLoadHost>()
                .take_request(),
            Some(crate::SaveLoadRequest::List)
        ));
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Ok(serde_json::json!({"slots":[
                {"slot":0,"description":"Quick","revision":"q0"},
                {"slot":17,"description":"Mixed case","revision":"r17"}
            ]})));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        for _ in 0..100 {
            app.update();
        }
        let existing = app
            .world()
            .resource::<MenuState>()
            .entries
            .iter()
            .position(|entry| entry.action == 102 && entry.eligible())
            .unwrap();
        assert_eq!(
            app.world().resource::<MenuState>().entries[existing].text,
            "Mixed case"
        );
        app.world_mut().resource_mut::<MenuState>().selected = Some(existing);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<MenuState>()
                .description_editor
                .as_ref()
                .unwrap()
                .text,
            "Mixed case"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(
            matches!(app.world_mut().resource_mut::<crate::SaveLoadHost>().take_request(), Some(crate::SaveLoadRequest::Overwrite { slot:17, revision, description }) if revision == "r17" && description == "Mixed case")
        );
        app.world_mut()
            .resource_mut::<crate::SaveLoadHost>()
            .finish(Err("save slot changed".into()));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert_eq!(
            app.world().resource::<MenuState>().unavailable.as_deref(),
            Some("save slot changed")
        );
        app.world_mut()
            .resource_scope(|world, mut menu: Mut<MenuState>| {
                let books = world.resource::<Assets<MenuAsset>>();
                let handle = menu.asset.clone();
                menu.open(books.get(&handle).unwrap(), 0);
                while menu.motion.as_ref().is_some_and(BookMotion::busy) {
                    menu.motion.as_mut().unwrap().tick(|_| {});
                }
            });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Unchanged
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Escape);
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Pending(AppState::InGame)
        ));
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn book_restores_only_cameras_it_paused() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let active = world.spawn((Camera3d::default(), Camera::default())).id();
        let inactive = world
            .spawn((
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..default()
                },
            ))
            .id();
        world.run_system_once(pause_cameras).unwrap();
        assert!(!world.get::<Camera>(active).unwrap().is_active);
        assert!(world.get::<BookPausedCamera>(active).is_some());
        assert!(world.get::<BookPausedCamera>(inactive).is_none());
        world.run_system_once(restore_cameras).unwrap();
        assert!(world.get::<Camera>(active).unwrap().is_active);
        assert!(!world.get::<Camera>(inactive).unwrap().is_active);
        assert!(world.get::<BookPausedCamera>(active).is_none());
    }

    #[test]
    fn gameplay_book_entry_stops_every_sound_but_the_startup_book_does_not() {
        use bevy::ecs::system::RunSystemOnce;
        use cyrus_audio::sound::SoundRequest;
        let stops = |world: &mut World| {
            world
                .resource_mut::<Messages<SoundRequest>>()
                .drain()
                .filter(|request| matches!(request, SoundRequest::StopAll))
                .count()
        };
        let mut world = World::new();
        world.init_resource::<Messages<SoundRequest>>();
        world.run_system_once(pause_cameras).unwrap();
        assert_eq!(stops(&mut world), 0);
        world.insert_resource(cyrus_game::PauseBookEntry { page: 0 });
        world.run_system_once(pause_cameras).unwrap();
        assert_eq!(stops(&mut world), 1);
    }

    #[test]
    fn navigation_wraps_and_skips_data_defined_disabled_entries() {
        let mut menu = MenuState {
            entries: vec![
                MenuItem {
                    text: "First".into(),
                    action: 72,
                    selectable: true,
                    grayed: false,
                    implemented: false,
                    visible: true,
                },
                MenuItem {
                    text: "Disabled".into(),
                    action: 80,
                    selectable: true,
                    grayed: true,
                    implemented: false,
                    visible: true,
                },
                MenuItem {
                    text: "Last".into(),
                    action: -2,
                    selectable: true,
                    grayed: false,
                    implemented: true,
                    visible: true,
                },
            ],
            selected: Some(0),
            ..default()
        };
        menu.navigate(1);
        assert_eq!(menu.selected, Some(2));
        menu.navigate(1);
        assert_eq!(menu.selected, Some(0));
        menu.navigate(-1);
        assert_eq!(menu.selected, Some(2));
        menu.entries[2].selectable = false;
        menu.navigate(1);
        assert_eq!(menu.selected, Some(0));
    }
    #[test]
    fn load_rows_keep_slot_identity_and_limit_authored_labels() {
        let row = || MenuItem {
            text: "placeholder".into(),
            action: 202,
            selectable: true,
            grayed: false,
            implemented: true,
            visible: true,
        };
        let mut menu = MenuState {
            entries: vec![row(), row(), row()],
            slots: vec![super::super::SaveSlot {
                slot: 19,
                revision: "rev-19".into(),
                thumbnail: None,
                description: "123456789012345678901234567890123456789012345".into(),
            }],
            ..default()
        };
        menu.fill_load_rows();
        assert_eq!(
            menu.entries[0].text,
            "1234567890123456789012345678901234567890"
        );
        assert_eq!(menu.slots[menu.slot_rows[&0]].slot, 19);
        assert_eq!(menu.slots[menu.slot_rows[&0]].revision, "rev-19");
        assert!(menu.entries[1].text.is_empty());
        assert!(!menu.entries[1].eligible());
        assert_eq!(menu.selected, Some(0));
    }

    #[test]
    fn save_and_load_pages_advance_by_their_authored_row_counts() {
        let row = |action| MenuItem {
            text: "row".into(),
            action,
            selectable: true,
            grayed: false,
            implemented: true,
            visible: true,
        };
        let mut menu = MenuState {
            page: 2,
            entries: (0..9).map(|_| row(202)).collect(),
            slots: (0..20)
                .map(|slot| super::super::SaveSlot {
                    slot,
                    description: format!("S{slot}"),
                    revision: format!("r{slot}"),
                    thumbnail: None,
                })
                .collect(),
            ..default()
        };
        menu.fill_load_rows();
        assert_eq!(menu.slots[menu.slot_rows[&0]].slot, 0);
        menu.change_list_page(1);
        assert_eq!(menu.slots[menu.slot_rows[&0]].slot, 9);
        menu.page = 1;
        menu.list_page = 0;
        menu.entries = std::iter::once(row(101))
            .chain((0..8).map(|_| row(102)))
            .collect();
        menu.fill_load_rows();
        assert_eq!(menu.slots[menu.slot_rows[&1]].slot, 2);
        menu.change_list_page(1);
        assert_eq!(menu.slots[menu.slot_rows[&1]].slot, 10);
    }
}
