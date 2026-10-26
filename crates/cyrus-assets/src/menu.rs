//! Portable menu presentation cache. Installed parsers remain native in rgpre.
use crate::preferences::MenuPreferences;
use anyhow::ensure;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Menu cache revision including installed mini-confirmation artwork and font styles.
pub const MENU_VERSION: u32 = 17;

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct MenuAsset {
    pub version: u32,
    /// Installed shared game-clock settings, available before entering a world.
    pub clock: crate::script::ClockSettings,
    /// Installed WORLD.INI destination and marker, read through preservation.
    pub new_game: NewGameEntry,
    pub pages: BTreeMap<u8, MenuPage>,
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

#[derive(Serialize, Deserialize)]
pub struct MenuFace {
    /// Screen X/Y, reciprocal depth, and texel U/V for each triangle corner.
    pub vertices: [[f32; 5]; 3],
    pub texture: Option<usize>,
    pub color: [u8; 4],
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
            (0..=7).all(|id| self.pages.contains_key(&id)),
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
    pub action: i32,
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
