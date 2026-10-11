//! MENU.INI-derived presentation data, shared by native and browser builds.
use super::{StartupContent, fit_image, menu_data::MenuAsset, menu_preferences::MenuPreferences};
use crate::AppState;
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One installed entry. Action IDs select handlers independently of labels.
#[derive(Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub text: String,
    pub action: i32,
    pub selectable: bool,
    pub grayed: bool,
    pub implemented: bool,
    pub visible: bool,
}

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
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct MovieRequest(pub usize);

impl MenuItem {
    pub(super) fn eligible(&self) -> bool {
        self.visible && self.selectable && !self.grayed && !self.text.is_empty()
    }
}

/// Current installed menu selection and explicitly unsupported activation.
#[derive(Resource, Default, Serialize)]
pub struct MenuState {
    /// True once the installed book is rendered and input can be accepted.
    pub ready: bool,
    pub page: u8,
    pub capturing: Option<i32>,
    /// Native cinematic loading/playback; menu input is suspended while present.
    pub movie: Option<MovieStatus>,
    pub preferences: Option<MenuPreferences>,
    pub entries: Vec<MenuItem>,
    pub selected: Option<usize>,
    pub unavailable: Option<String>,
    #[serde(skip)]
    image: Handle<Image>,
    #[serde(skip)]
    pub(super) selections: BTreeMap<u8, Option<usize>>,
    #[serde(skip)]
    pub(super) blink_on: bool,
    #[serde(skip)]
    blink_seconds: f32,
    #[serde(skip)]
    pub(super) dirty: bool,
    #[serde(skip)]
    persistence_error: Option<String>,
    #[serde(skip)]
    asset: Handle<MenuAsset>,
    #[serde(skip)]
    repeat_seconds: f32,
    #[serde(skip)]
    held_seconds: f32,
}

impl MenuState {
    fn navigate(&mut self, direction: isize) {
        let Some(selected) = self.selected else {
            return;
        };
        let len = self.entries.len();
        for step in 1..=len {
            let index =
                (selected as isize + direction * step as isize).rem_euclid(len as isize) as usize;
            if self.entries[index].eligible() {
                self.selected = Some(index);
                self.unavailable = None;
                return;
            }
        }
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
    app.init_asset::<MenuAsset>()
        .init_asset_loader::<MenuLoader>()
        .add_systems(OnEnter(AppState::Menu), setup)
        .add_systems(Update, (fit_image, drive).run_if(in_state(AppState::Menu)))
        .add_systems(OnExit(AppState::Menu), cleanup);
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((Camera2d, StartupContent));
    commands.spawn((Sprite::default(), StartupContent));
    commands.insert_resource(MenuState {
        asset: assets.load("cache://menu/book.menu"),
        ..default()
    });
}

pub(super) fn drive(
    assets: Res<AssetServer>,
    definitions: Res<Assets<MenuAsset>>,
    input: (Res<Time>, Res<ButtonInput<KeyCode>>),
    mut menu: ResMut<MenuState>,
    rendering: (
        ResMut<Assets<Image>>,
        Query<&mut Sprite, With<StartupContent>>,
    ),
    mut output: (MessageWriter<AppExit>, Query<&mut Text, With<MenuNotice>>),
    runtime: (ResMut<crate::player::input::PlayerBindings>, Commands),
) {
    let (mut images, mut sprite) = rendering;
    let (mut bindings, mut commands) = runtime;
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
    if menu.preferences.is_none() {
        menu.preferences = Some(match MenuPreferences::load() {
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
        menu.open(book, 0);
        menu.blink_on = true;
        apply_bindings(&menu, &mut bindings);
    }
    if menu.movie.is_some() {
        return;
    }
    let keys = &input.1;
    let previous = (menu.page, menu.selected, menu.capturing, menu.blink_on);
    if menu.capturing.is_some() {
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
                .filter_map(|key| crate::player::input::scan_code(*key))
                .min();
            if let Some(code) = captured {
                if matches!(code, 12 | 13 | 15 | 28 | 59..=68 | 87 | 88) {
                    menu.unavailable = Some("That key is reserved.".into());
                } else {
                    let mut preferences = menu.preferences.clone().unwrap();
                    preferences.rebind((menu.capturing.unwrap() - 710) as usize, code);
                    if persist(&mut menu, preferences) {
                        menu.capturing = None;
                        apply_bindings(&menu, &mut bindings);
                    }
                }
            }
        }
    } else if keys.just_pressed(KeyCode::Escape) && menu.page != 0 {
        let target = if matches!(menu.page, 5..=7) { 4 } else { 0 };
        menu.open(book, target);
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
                if adjust {
                    adjust_value(book, &mut menu, direction as i32, false);
                } else {
                    menu.navigate(direction);
                }
            }
        }
        let activation = menu
            .preferences
            .as_ref()
            .and_then(|p| crate::player::input::scan_key(p.bindings[5]));
        if (keys.just_pressed(KeyCode::Enter)
            || activation.is_some_and(|key| keys.just_pressed(key)))
            && let Some(index) = menu.selected
        {
            let entry = menu.entries[index].clone();
            match entry.action {
                -2 => {
                    output.0.write(AppExit::Success);
                }
                3 | 4 => menu.open(book, entry.action as u8),
                300 | 400 => menu.open(book, 0),
                405..=407 => menu.open(book, (entry.action - 400) as u8),
                500 | 600 | 700 => menu.open(book, 4),
                504 | 505 | 603 => adjust_value(book, &mut menu, 0, true),
                601 | 602 => {}
                #[cfg(not(target_arch = "wasm32"))]
                301..=305 => {
                    menu.unavailable = None;
                    commands.write_message(MovieRequest((entry.action - 301) as usize));
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
                menu.ready = true;
                menu.dirty = false;
            }
            Err(error) => {
                menu.ready = false;
                menu.unavailable = Some(format!("Menu rendering unavailable: {error}"));
            }
        }
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
            self.selections.insert(self.page, self.selected);
        }
        self.page = page;
        self.entries = book.pages[&page]
            .entries
            .iter()
            .map(|e| {
                let item = e.item.clone();
                #[cfg(target_arch = "wasm32")]
                let item = MenuItem {
                    implemented: item.implemented && !(301..=305).contains(&item.action),
                    ..item
                };
                item
            })
            .collect();
        self.selected = self.selections.get(&page).copied().flatten();
        self.unavailable = self.persistence_error.clone();
        self.capturing = None;
        self.held_seconds = 0.0;
        self.dirty = true;
    }
}

fn persist(menu: &mut MenuState, preferences: MenuPreferences) -> bool {
    if let Some(error) = &menu.persistence_error {
        menu.unavailable = Some(error.clone());
        return false;
    }
    match preferences.save() {
        Ok(()) => {
            menu.preferences = Some(preferences);
            menu.unavailable = None;
            menu.dirty = true;
            true
        }
        Err(error) => {
            menu.unavailable = Some(format!("Settings were not saved: {error}"));
            false
        }
    }
}

fn apply_bindings(menu: &MenuState, bindings: &mut crate::player::input::PlayerBindings) {
    if let Some(preferences) = &menu.preferences {
        match crate::player::input::PlayerBindings::from_codes(std::array::from_fn(|i| {
            preferences.bindings[i]
        })) {
            Ok(updated) => *bindings = updated,
            Err(error) => warn!("{error}; unsupported device bindings remain unavailable"),
        }
    }
}

fn adjust_value(book: &MenuAsset, menu: &mut MenuState, direction: i32, toggle: bool) {
    let Some(index) = menu.selected else {
        return;
    };
    let entry = &book.pages[&menu.page].entries[index];
    let mut preferences = menu.preferences.clone().unwrap();
    let Some(value) = preferences.values.get_mut(&entry.item.action) else {
        return;
    };
    let next = if toggle {
        1 - *value
    } else {
        (*value + direction).clamp(entry.slider[0], entry.slider[1])
    };
    if *value == next {
        return;
    }
    *value = next;
    persist(menu, preferences);
}

#[derive(Component)]
pub(super) struct MenuNotice;
fn notice(commands: &mut Commands, assets: &AssetServer, text: String) {
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
fn cleanup(mut commands: Commands) {
    commands.remove_resource::<MenuState>();
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
