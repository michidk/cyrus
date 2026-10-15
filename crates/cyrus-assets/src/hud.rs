//! Installed HUD layout, item and font records.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Installed HUD data, exported natively from SYSTEM.INI, ITEM.INI, KEYS.INI, WORLD.INI,
/// the RTX file, GXA and FNT files. Positions are top-left corners in 640×480 pixels.
#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct HudAsset {
    /// Installed RTX text by label, including entries without speech audio.
    pub labels: BTreeMap<String, String>,
    /// Runtime image dependencies; cache serialization retains installed metadata only.
    #[dependency]
    #[serde(skip)]
    pub images: Vec<UntypedHandle>,
    /// SYSTEM.INI `[screen] candle_mode`: 0 shows the candle only while the show timer
    /// runs, 1 always, 2 always with the health number.
    pub candle_mode: u8,
    /// SYSTEM.INI `candle_xco/yco`.
    pub candle: [i32; 2],
    pub candle_size: [u16; 2],
    /// SYSTEM.INI `pickup_xco/yco`.
    pub pickup: [i32; 2],
    /// Pen of the prompt line: `pickup_xco`, `pickup_text_yco`.
    pub prompt: [i32; 2],
    /// SYSTEM.INI `rtx_pickup_override_time`, in ticks.
    pub override_ticks: u32,
    /// SYSTEM.INI `compass_xco/yco`; COMPASS2.GXA has 32 frames.
    pub compass: [i32; 2],
    pub compass_size: [u16; 2],
    /// WORLD.INI `world_compass[N]` by world ID; other worlds use 0.
    pub world_compass: BTreeMap<u32, i32>,
    /// SYSTEM.INI `game_xco1/yco1` and `game_xco2/yco2`.
    pub powerup_slots: [[i32; 2]; 2],
    /// Frame sizes of `[system] game_bitmap`.
    pub powerup_sizes: Vec<[u16; 2]>,
    /// SYSTEM.INI `logbook_xco/yco`; SCROLL.GXA has 32 frames.
    pub logbook: [i32; 2],
    pub logbook_size: [u16; 2],
    /// SYSTEM.INI `[cyrus] walk_mode` and `auto_defend`.
    pub walk_mode: bool,
    pub auto_defend: bool,
    /// RTX notice texts, off then on: walk mode `#WOF`/`#WON`, auto-defend `#DOF`/`#DON`.
    pub notices: [[String; 2]; 2],
    /// All installed item definitions, including hidden and unowned items.
    pub inventory: BTreeMap<u16, InventoryItem>,
    /// Unhidden artwork in ID order; counts are the initial session counts.
    pub items: Vec<HudItem>,
    /// ITEM.INI `start_item_select`; live selection belongs to the game session.
    pub selected: Option<u16>,
    /// Installed KEYS.INI scan codes for menu actions 710 through 724.
    pub bindings: [u8; 15],
    /// `fonts/REDSEL.FNT`; HUD text is drawn like an NPC subtitle line.
    pub font: cyrus_text::Font,
    /// SYSTEM.INI `[3dfx] font_sel` as main and shadow alpha.
    pub text_alpha: [u8; 2],
    /// SYSTEM.INI [3dfx] font_norm, for reserved player dialogue record zero.
    pub player_text_alpha: [u8; 2],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HudItem {
    pub id: u16,
    /// ITEM.INI `bitmap[id]`: the `hud/item-NN.png` frame.
    pub bitmap: u16,
    pub count: u32,
    pub size: [u16; 2],
}

/// Portable acquisition inputs decoded from ITEM.INI by preservation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InventoryItem {
    pub count: u32,
    pub total: u32,
    pub player_max: u32,
    pub hidden: bool,
    /// Acquisition lists require additional gameplay services before they can run.
    pub acquisition_effects: bool,
}
