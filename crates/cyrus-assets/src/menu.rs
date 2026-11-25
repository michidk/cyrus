//! Portable menu presentation cache. Installed parsers remain native in rgpre.
use crate::preferences::MenuPreferences;
use anyhow::ensure;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Installed page identity; numeric cache and control representations remain unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum MenuPageId {
    #[default]
    Main,
    Save,
    Load,
    Movies,
    Options,
    Display,
    Sound,
    Controls,
    /// Retained for contextual rejection of unsupported transitions.
    Unavailable(u8),
}
impl From<u8> for MenuPageId {
    fn from(code: u8) -> Self {
        match code {
            0 => Self::Main,
            1 => Self::Save,
            2 => Self::Load,
            3 => Self::Movies,
            4 => Self::Options,
            5 => Self::Display,
            6 => Self::Sound,
            7 => Self::Controls,
            _ => Self::Unavailable(code),
        }
    }
}
impl From<MenuPageId> for u8 {
    fn from(page: MenuPageId) -> Self {
        match page {
            MenuPageId::Main => 0,
            MenuPageId::Save => 1,
            MenuPageId::Load => 2,
            MenuPageId::Movies => 3,
            MenuPageId::Options => 4,
            MenuPageId::Display => 5,
            MenuPageId::Sound => 6,
            MenuPageId::Controls => 7,
            MenuPageId::Unavailable(code) => code,
        }
    }
}
impl std::fmt::Display for MenuPageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        u8::from(*self).fmt(f)
    }
}
impl MenuPageId {
    pub fn is_slot_list(self) -> bool {
        matches!(self, Self::Save | Self::Load)
    }
    pub fn is_settings(self) -> bool {
        matches!(self, Self::Display | Self::Sound | Self::Controls)
    }
    pub fn parent(self) -> Self {
        if self.is_settings() {
            Self::Options
        } else {
            Self::Main
        }
    }
}

/// Action decoded once at the installed/cache boundary, independently of entry labels.
/// Unavailable actions retain their code without acquiring a runtime handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i32", into = "i32")]
pub enum MenuAction {
    Quit,
    Continue,
    Save,
    Load,
    Movies,
    Options,
    NewGame,
    SaveBack,
    NewSave,
    SaveSlot,
    LoadBack,
    LoadSlot,
    MoviesBack,
    OptionsBack,
    Display,
    Sound,
    Controls,
    DisplayBack,
    Subtitles,
    MovieInterlace,
    SoundBack,
    SoundVolume,
    MusicVolume,
    Speech,
    ControlsBack,
    Movie(u8),
    Binding(u8),
    Unavailable(i32),
}
impl From<i32> for MenuAction {
    fn from(code: i32) -> Self {
        match code {
            -2 => Self::Quit,
            -1 => Self::Continue,
            1 => Self::Save,
            2 => Self::Load,
            3 => Self::Movies,
            4 => Self::Options,
            10 => Self::NewGame,
            100 => Self::SaveBack,
            101 => Self::NewSave,
            102 => Self::SaveSlot,
            200 => Self::LoadBack,
            202 => Self::LoadSlot,
            300 => Self::MoviesBack,
            400 => Self::OptionsBack,
            405 => Self::Display,
            406 => Self::Sound,
            407 => Self::Controls,
            500 => Self::DisplayBack,
            504 => Self::Subtitles,
            505 => Self::MovieInterlace,
            600 => Self::SoundBack,
            601 => Self::SoundVolume,
            602 => Self::MusicVolume,
            603 => Self::Speech,
            700 => Self::ControlsBack,
            301..=305 => Self::Movie((code - 301) as u8),
            710..=724 => Self::Binding((code - 710) as u8),
            _ => Self::Unavailable(code),
        }
    }
}
impl From<MenuAction> for i32 {
    fn from(action: MenuAction) -> Self {
        match action {
            MenuAction::Quit => -2,
            MenuAction::Continue => -1,
            MenuAction::Save => 1,
            MenuAction::Load => 2,
            MenuAction::Movies => 3,
            MenuAction::Options => 4,
            MenuAction::NewGame => 10,
            MenuAction::SaveBack => 100,
            MenuAction::NewSave => 101,
            MenuAction::SaveSlot => 102,
            MenuAction::LoadBack => 200,
            MenuAction::LoadSlot => 202,
            MenuAction::MoviesBack => 300,
            MenuAction::OptionsBack => 400,
            MenuAction::Display => 405,
            MenuAction::Sound => 406,
            MenuAction::Controls => 407,
            MenuAction::DisplayBack => 500,
            MenuAction::Subtitles => 504,
            MenuAction::MovieInterlace => 505,
            MenuAction::SoundBack => 600,
            MenuAction::SoundVolume => 601,
            MenuAction::MusicVolume => 602,
            MenuAction::Speech => 603,
            MenuAction::ControlsBack => 700,
            MenuAction::Movie(index) => 301 + i32::from(index),
            MenuAction::Binding(index) => 710 + i32::from(index),
            MenuAction::Unavailable(code) => code,
        }
    }
}
impl MenuAction {
    /// Navigation actions retain distinct codes while sharing their destination.
    pub fn destination(self) -> Option<MenuPageId> {
        Some(match self {
            Self::Save => MenuPageId::Save,
            Self::Load => MenuPageId::Load,
            Self::Movies => MenuPageId::Movies,
            Self::Options | Self::DisplayBack | Self::SoundBack | Self::ControlsBack => {
                MenuPageId::Options
            }
            Self::Display => MenuPageId::Display,
            Self::Sound => MenuPageId::Sound,
            Self::Controls => MenuPageId::Controls,
            Self::SaveBack | Self::LoadBack | Self::MoviesBack | Self::OptionsBack => {
                MenuPageId::Main
            }
            _ => return None,
        })
    }
}

/// Menu cache revision including the startup book palette upload precision.
pub const MENU_VERSION: u32 = 18;

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct MenuAsset {
    pub version: u32,
    /// Installed shared game-clock settings, available before entering a world.
    pub clock: crate::script::ClockSettings,
    /// Installed WORLD.INI destination and marker, read through preservation.
    pub new_game: NewGameEntry,
    pub pages: BTreeMap<MenuPageId, MenuPage>,
    /// Cover, page one, table, page two, page three, in that order.
    pub models: Vec<MenuModel>,
    pub textures: Vec<PageTexture>,
    pub palette: Vec<[u8; 3]>,
    /// Normal, selected, small normal and small selected; the small pair draws the Save/Load list rows.
    pub fonts: [cyrus_text::Font; 4],
    pub confirmation: ConfirmationArtwork,
    pub checks: Vec<MenuPicture>,
    pub sliders: Vec<MenuPicture>,
    pub movies: Vec<MenuPicture>,
    pub key_names: Vec<String>,
    pub waiting_message: String,
    pub defaults: MenuPreferences,
    pub repeat_seconds: f32,
}

/// Installed mini-menu image and REDSEL styles, per preservation's book-menu rules.
#[derive(Serialize, Deserialize)]
pub struct ConfirmationArtwork {
    pub picture: MenuPicture,
    pub font: cyrus_text::Font,
    /// Normal then selected, each `[colour factor, main alpha]` from SYSTEM.INI.
    pub styles: [[u8; 2]; 2],
}

/// Validated starting-world identity, marker, and 640×480 transition presentation.
#[derive(Serialize, Deserialize)]
pub struct NewGameEntry {
    /// WORLD.INI numeric world identity, not a map-stem alias.
    pub world: u32,
    /// Zero-based authored marker index within that world.
    pub marker: u32,
    /// Installed destination's transition artwork.
    pub loading: Vec<MenuPicture>,
    /// Installed REDSEL font for the loading percentage.
    pub font: cyrus_text::Font,
}

#[derive(Serialize, Deserialize)]
pub struct MenuPage {
    pub entries: Vec<MenuEntry>,
    pub selected: Option<usize>,
    pub surfaces: Vec<usize>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MenuEntry {
    pub item: MenuItem,
    pub position: [i32; 2],
    pub output_position: [i32; 2],
    pub justify: u8,
    pub output_justify: u8,
    pub slider: [i32; 2],
    pub texture: usize,
}

#[derive(Serialize, Deserialize)]
pub struct MenuModel {
    pub position: [f32; 3],
    /// Authored coordinates before the menu's half-revolution placement.
    pub frames: Vec<Vec<[f32; 3]>>,
    pub faces: Vec<MenuModelFace>,
}

#[derive(Serialize, Deserialize)]
pub struct MenuModelFace {
    pub corners: Vec<MenuCorner>,
    pub texture: Option<usize>,
    pub color: [u8; 4],
}

#[derive(Serialize, Deserialize)]
pub struct MenuCorner {
    pub vertex: usize,
    pub uv: [f32; 2],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PageTexture {
    pub width: u16,
    pub height: u16,
    pub indices: Vec<u8>,
    pub rgba: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct MenuPicture {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

impl MenuAsset {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.version == MENU_VERSION,
            "menu: cache version {} unsupported; re-export",
            self.version
        );
        self.clock.validate()?;
        ensure!(
            self.confirmation.font.line_height > 0,
            "menu: confirmation font has no line height"
        );
        ensure!(
            self.palette.len() == 256 && self.key_names.len() == 140,
            "menu: invalid palette/key-name count"
        );
        ensure!(
            (0..=7).all(|id| self.pages.contains_key(&MenuPageId::from(id))),
            "menu: missing supported page"
        );
        ensure!(
            self.repeat_seconds.is_finite() && self.repeat_seconds > 0.0,
            "menu: invalid repeat delay"
        );
        self.defaults.validate()?;
        ensure!(
            self.waiting_message.is_ascii() && self.key_names.iter().all(|name| name.is_ascii()),
            "menu: unsupported key-name/prompt text"
        );
        for texture in &self.textures {
            let count = usize::from(texture.width) * usize::from(texture.height);
            ensure!(
                count > 0 && texture.indices.len() == count && texture.rgba.len() == count * 4,
                "menu: invalid texture dimensions/pixel count"
            );
        }
        for picture in self
            .checks
            .iter()
            .chain(&self.sliders)
            .chain(&self.movies)
            .chain(&self.new_game.loading)
            .chain(std::iter::once(&self.confirmation.picture))
        {
            ensure!(
                picture.width > 0
                    && picture.height > 0
                    && picture.rgba.len()
                        == usize::from(picture.width) * usize::from(picture.height) * 4,
                "menu: invalid widget dimensions/pixel count"
            );
        }
        ensure!(
            self.checks.len() >= 2 && !self.sliders.is_empty(),
            "menu: missing widget frames"
        );
        ensure!(
            !self.new_game.loading.is_empty()
                && self
                    .new_game
                    .loading
                    .iter()
                    .all(|frame| (frame.width, frame.height) == (640, 480)),
            "menu: invalid new-game loading artwork"
        );
        for font in self
            .fonts
            .iter()
            .chain(std::iter::once(&self.new_game.font))
            .chain(std::iter::once(&self.confirmation.font))
        {
            for glyph in &font.glyphs {
                ensure!(
                    glyph.pixels.len() == usize::from(glyph.width) * usize::from(glyph.height),
                    "menu: invalid glyph pixel count"
                );
            }
        }
        for (id, page) in &self.pages {
            ensure!(
                !page.entries.is_empty() && page.surfaces.iter().all(|i| *i < self.textures.len()),
                "menu: page {id}: invalid surfaces/entries"
            );
            ensure!(
                page.selected
                    .is_none_or(|i| page.entries.get(i).is_some_and(|e| e.item.eligible())),
                "menu: page {id}: invalid selection"
            );
            for entry in &page.entries {
                ensure!(
                    entry.texture < self.textures.len()
                        && page.surfaces.contains(&entry.texture)
                        && entry.justify <= 2
                        && entry.output_justify <= 2
                        && entry.slider[0] <= entry.slider[1],
                    "menu: page {id}: invalid entry {:?}",
                    entry.item.text
                );
                ensure!(
                    entry.item.text.is_ascii(),
                    "menu: page {id}: unsupported text {:?}",
                    entry.item.text
                );
            }
        }
        ensure!(self.models.len() == 5, "menu: invalid model count");
        for (index, model) in self.models.iter().enumerate() {
            let expected_frames = if index == 2 { 1 } else { 9 };
            let vertices = model.frames.first().map_or(0, Vec::len);
            ensure!(
                model.position.iter().all(|v| v.is_finite())
                    && model.frames.len() == expected_frames
                    && vertices > 0
                    && model.frames.iter().all(|frame| {
                        frame.len() == vertices && frame.iter().flatten().all(|v| v.is_finite())
                    }),
                "menu: model {index}: invalid authored frames"
            );
            for face in &model.faces {
                ensure!(
                    face.texture.is_none_or(|i| i < self.textures.len())
                        && face.corners.len() >= 3
                        && face.corners.iter().all(|corner| {
                            corner.vertex < vertices && corner.uv.iter().all(|v| v.is_finite())
                        }),
                    "menu: model {index}: invalid face"
                );
            }
        }
        Ok(())
    }
}

/// One installed entry. Action IDs select handlers independently of labels.
#[derive(Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub text: String,
    pub action: MenuAction,
    pub selectable: bool,
    pub grayed: bool,
    pub implemented: bool,
    pub visible: bool,
}

impl MenuItem {
    pub fn eligible(&self) -> bool {
        self.visible && self.selectable && !self.grayed && !self.text.is_empty()
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn actions_keep_installed_codes_including_unavailable_definitions() {
        for code in -3..=800 {
            let action: MenuAction = serde_json::from_value(code.into()).unwrap();
            assert_eq!(i32::from(action), code);
            assert_eq!(serde_json::to_value(action).unwrap(), code);
        }
        assert_eq!(MenuAction::from(202), MenuAction::LoadSlot);
        assert_eq!(MenuAction::from(301), MenuAction::Movie(0));
        assert_eq!(MenuAction::from(724), MenuAction::Binding(14));
        for code in [201, 306, 501, i32::MIN, i32::MAX] {
            let action = MenuAction::from(code);
            assert_eq!(action, MenuAction::Unavailable(code));
            assert_eq!(action.destination(), None);
            assert_eq!(i32::from(action), code);
        }
    }

    #[test]
    fn typed_pages_preserve_numeric_cache_keys_and_navigation_destinations() {
        let keys = BTreeMap::from([(MenuPageId::Main, 1), (MenuPageId::Controls, 2)]);
        let json = serde_json::to_value(&keys).unwrap();
        assert_eq!(json, serde_json::json!({"0": 1, "7": 2}));
        assert_eq!(
            serde_json::from_value::<BTreeMap<MenuPageId, i32>>(json).unwrap(),
            keys
        );
        for (code, page) in [
            (1, MenuPageId::Save),
            (2, MenuPageId::Load),
            (405, MenuPageId::Display),
            (406, MenuPageId::Sound),
            (407, MenuPageId::Controls),
            (100, MenuPageId::Main),
            (200, MenuPageId::Main),
            (300, MenuPageId::Main),
            (400, MenuPageId::Main),
            (500, MenuPageId::Options),
            (600, MenuPageId::Options),
            (700, MenuPageId::Options),
        ] {
            assert_eq!(MenuAction::from(code).destination(), Some(page));
        }
    }
}
