//! In-game overlay from installed HUD data: candle, compass, logbook icon, toggle notices,
//! power-up slots, active item, prompt text. Rules follow redguard-preservation's HUD documentation.
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::hud::{HudAsset, HudItem};
use cyrus_game::AppState;
use serde::{Deserialize, Serialize};

/// Shows the exported `hud/overlay.hud` over gameplay, fitted like the original 640×480 viewport.
pub struct HudPlugin;

/// The player's fixed maximum and spawn health; no INI key sets it.
const MAX_HEALTH: u8 = 100;
/// Game ticks per second: 12 per 18 BIOS timer ticks (preservation player doc).
const TICKS_PER_SECOND: f64 = 12.0 * 18.2065 / 18.0;
/// Show timer values: a trigger shows the candle for 25 ticks; 50 pins it and never decays.
const SHOW_TICKS: u32 = 25;
const SHOW_PINNED: u32 = 50;
/// Activation hold-off after an activation fires, in ticks.
const ACTIVATION_HOLD_OFF: u32 = 22;
/// Walk-mode and auto-defend notices last 24 ticks.
const NOTICE_TICKS: u32 = 24;

/// Candle level for player health: 0 is the full candle, 11 the snuffed one.
fn candle_level(health: u8) -> u8 {
    let max = u32::from(MAX_HEALTH);
    let lost = (max - u32::from(health).min(max)) * 256 / max;
    ((lost * 11) >> 8) as u8
}

/// The candle's shown level and flicker step; advanced once per original rendered frame.
#[derive(Default, Serialize, Clone, Copy, Debug, PartialEq)]
struct Candle {
    shown: Option<u8>,
    step: u8,
    frame: u16,
}

impl Candle {
    /// SNUFF.GXA: six frames per level (five flicker, one transition), then fifteen snuff frames.
    fn advance(&mut self, health: u8) -> u16 {
        let target = candle_level(health);
        let shown = *self.shown.get_or_insert(target);
        self.frame = if target < 11 {
            self.step = if self.step >= 4 { 0 } else { self.step + 1 };
            if shown < target {
                self.shown = Some(shown + 1);
                self.step = 4;
                u16::from(shown) * 6 + 5
            } else if shown > target {
                self.shown = Some(shown - 1);
                self.step = 4;
                u16::from(shown - 1) * 6 + 5
            } else {
                u16::from(shown) * 6 + u16::from(self.step)
            }
        } else {
            if shown == 11 {
                self.step = (self.step + 1).min(14);
            } else {
                self.shown = Some(11);
                self.step = 0;
            }
            66 + u16::from(self.step)
        };
        self.frame
    }
}

/// COMPASS2.GXA frame for a heading in 2048-unit turns and the world's `world_compass`.
fn compass_frame(heading: u16, world_compass: i32) -> u8 {
    ((-(i32::from(heading) + world_compass)).rem_euclid(2048) >> 6) as u8
}

/// Player-facing toggles kept outside the install: native `cyrus-config/hud.json`,
/// browser localStorage `cyrus-hud`. The original writes them to SYSTEM.INI.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct HudPreferences {
    candle_mode: u8,
    walk_mode: bool,
    auto_defend: bool,
}

impl HudPreferences {
    fn load(store: &cyrus_game::preferences::Preferences) -> anyhow::Result<Option<Self>> {
        store
            .load_hud()?
            .map(|text| {
                let value: Self = serde_json::from_str(&text)?;
                anyhow::ensure!(
                    value.candle_mode <= 2,
                    "local HUD preferences: candle_mode must be 0, 1 or 2"
                );
                Ok(value)
            })
            .transpose()
    }
    fn save(&self, store: &cyrus_game::preferences::Preferences) -> anyhow::Result<()> {
        store.save_hud(&serde_json::to_string_pretty(self)?)
    }
}

/// Live overlay state, reported by the control server. The inputs marked *unwired* have
/// no gameplay system writing them yet; they keep the defaults of a fresh session.
#[derive(Resource, Serialize, Debug)]
pub struct HudState {
    /// Player health, 0..=100. Use [`HudState::set_health`]; *unwired*, stays at 100.
    pub health: u8,
    /// Candle visibility mode; F8 cycles 0 → 1 → 2 → 0.
    pub candle_mode: u8,
    pub candle_visible: bool,
    candle: Candle,
    /// Candle show timer in ticks (25 after a trigger, 50 pinned).
    show_timer: u32,
    /// Value last passed to the script function `LockoutPlayer`; *unwired*.
    pub lockout: u8,
    /// Sword drawn; use [`HudState::set_sword_drawn`]. *Unwired*.
    pub sword_drawn: bool,
    /// SOUP386.DEF `attr_player_type`: 1 (`ply_gremlin`) hides the icon and compass. *Unwired*.
    pub player_type: u8,
    /// Player heading in 2048-unit turns and the WORLD.INI world ID, for the compass. *Unwired*.
    pub heading: u16,
    pub world: Option<u32>,
    /// Compass display flag, toggled by using item 0. *Unwired*.
    pub compass_on: bool,
    pub compass_frame: Option<u8>,
    /// Power-up slots as `(id, frame)`; see [`HudState::show_bitmap`]. *Unwired*.
    pub powerups: [Option<(u8, u8)>; 2],
    /// The slot `show_bitmap` chooses when both are full.
    next_slot: usize,
    /// Logbook icon frame while it plays; see [`HudState::logbook_entry`].
    pub logbook_frame: Option<u8>,
    /// Walk mode (Tab) and auto-defend (F7); the player does not consume them yet.
    pub walk_mode: bool,
    pub auto_defend: bool,
    /// Remaining ticks of the walk-mode and auto-defend notices.
    notices: [u32; 2],
    /// Activation prompt offered this frame; see [`HudState::activate`].
    pub prompt: Option<String>,
    /// Activation hold-off and prompt override timers, in ticks.
    hold_off: u32,
    prompt_override: u32,
    /// Selected item ID, or `None` when nothing is owned.
    pub selected: Option<u16>,
    pub count: Option<u32>,
    #[serde(skip)]
    elapsed: f32,
    #[serde(skip)]
    ticks: f64,
    /// Edited bindings when present, else the installed ones; read on entering a world.
    #[serde(skip)]
    bindings: [u8; 15],
    /// Every frame of the fixed atlases, held so swapping frames never reloads an image.
    #[serde(skip)]
    frames: Frames,
    /// Text lines and pen positions currently drawn on the text layer.
    #[serde(skip)]
    text: Vec<(String, [i32; 2])>,
}

#[derive(Default, Debug)]
struct Frames {
    candle: Vec<Handle<Image>>,
    compass: Vec<Handle<Image>>,
    logbook: Vec<Handle<Image>>,
    powerup: Vec<Handle<Image>>,
}

impl HudState {
    fn new(hud: &HudAsset, preferences: Option<HudPreferences>, frames: Frames) -> Self {
        let preferences = preferences.unwrap_or(HudPreferences {
            candle_mode: hud.candle_mode,
            walk_mode: hud.walk_mode,
            auto_defend: hud.auto_defend,
        });
        Self {
            health: MAX_HEALTH,
            candle_mode: preferences.candle_mode,
            candle_visible: preferences.candle_mode != 0,
            candle: Candle::default(),
            show_timer: 0,
            lockout: 0,
            sword_drawn: false,
            player_type: 0,
            heading: 0,
            world: None,
            compass_on: false,
            compass_frame: None,
            powerups: [None; 2],
            next_slot: 0,
            logbook_frame: None,
            walk_mode: preferences.walk_mode,
            auto_defend: preferences.auto_defend,
            notices: [0; 2],
            prompt: None,
            hold_off: 0,
            prompt_override: 0,
            selected: hud.selected.or_else(|| hud.items.first().map(|i| i.id)),
            count: None,
            elapsed: 0.0,
            ticks: 0.0,
            bindings: hud.bindings,
            frames,
            text: Vec::new(),
        }
    }

    /// Sets player health (clamped to 100); a change shows the candle for 25 ticks
    /// unless the timer is pinned.
    pub fn set_health(&mut self, health: u8) {
        let health = health.min(MAX_HEALTH);
        if health != self.health && self.show_timer < SHOW_PINNED {
            self.show_timer = SHOW_TICKS;
        }
        self.health = health;
    }

    /// Drawing or sheathing the sword shows the candle for 25 ticks.
    pub fn set_sword_drawn(&mut self, drawn: bool) {
        if drawn != self.sword_drawn {
            self.show_timer = SHOW_TICKS;
        }
        self.sword_drawn = drawn;
    }

    /// Script `Activate`: offers `text` as this frame's prompt. With the use key held the
    /// activation fires instead, blanking the item area for 22 ticks, and returns true.
    /// Unknown further player-state conditions are not modelled.
    pub fn activate(&mut self, text: &str, use_held: bool) -> bool {
        if self.lockout == 1 || self.sword_drawn || self.hold_off > 0 || self.prompt.is_some() {
            return false;
        }
        if use_held {
            self.hold_off = ACTIVATION_HOLD_OFF;
            return true;
        }
        self.prompt = Some(text.to_owned());
        false
    }

    /// Script `Deactivate`: clears the activation hold-off.
    pub fn deactivate(&mut self) {
        self.hold_off = 0;
    }

    /// Script `ShowBitmap(id, frame)`: `id` 0 or 1, `frame` a `game_bitmap` frame.
    pub fn show_bitmap(&mut self, id: u8, frame: u8) -> Result<(), String> {
        if id > 1 || usize::from(frame) >= self.frames.powerup.len() {
            return Err(format!("ShowBitmap({id}, {frame}): invalid id or frame"));
        }
        if self.powerups.iter().flatten().any(|(_, f)| *f == frame) {
            return Ok(());
        }
        let slot = self
            .powerups
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.next_slot);
        self.powerups[slot] = Some((id, frame));
        self.next_slot = 1 - slot;
        Ok(())
    }

    /// Script `UnShowBitmap(id)`: clears slot 1 if its id matches, else slot 2.
    pub fn unshow_bitmap(&mut self, id: u8) {
        if let Some(slot) = self
            .powerups
            .iter_mut()
            .find(|s| s.is_some_and(|(i, _)| i == id))
        {
            *slot = None;
        }
    }

    /// A new logbook entry plays the scroll icon once unless it is already playing.
    pub fn logbook_entry(&mut self) {
        self.logbook_frame.get_or_insert(0);
    }

    /// Advances the per-rendered-frame animations by one original frame.
    fn advance_frame(&mut self) {
        let health = self.health;
        self.candle.advance(health);
        self.logbook_frame = self.logbook_frame.and_then(|f| (f < 31).then_some(f + 1));
    }
}

#[derive(Resource)]
struct HudHandle(Handle<HudAsset>);

#[derive(Component)]
struct HudContent;

#[derive(Component)]
struct HudViewport;

/// One bitmap element; spawned hidden and updated every frame, in draw order.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum HudSprite {
    Compass,
    Candle,
    Logbook,
    Powerup(usize),
    Item,
}

const SPRITES: [HudSprite; 6] = [
    HudSprite::Compass,
    HudSprite::Candle,
    HudSprite::Logbook,
    HudSprite::Powerup(0),
    HudSprite::Powerup(1),
    HudSprite::Item,
];

#[derive(Component)]
struct HudText;

#[derive(Default, TypePath)]
struct HudLoader;

impl AssetLoader for HudLoader {
    type Asset = HudAsset;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<HudAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    fn extensions(&self) -> &[&str] {
        &["hud"]
    }
}

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<cyrus_game::preferences::Preferences>()
            .init_asset::<HudAsset>()
            .init_asset_loader::<HudLoader>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(Update, update.run_if(in_state(AppState::InGame)))
            .add_systems(OnExit(AppState::InGame), despawn);
    }
}

fn spawn(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(HudHandle(assets.load("cache://hud/overlay.hud")));
    commands
        .spawn((
            HudContent,
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            // A 4:3 box as large as the window allows; children use its 640×480 units.
            root.spawn((
                HudViewport,
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(640.0 / 480.0),
                    ..default()
                },
            ));
        });
}

/// Absolute node covering a 640×480 screen rectangle within the viewport.
fn place(at: [i32; 2], size: [u16; 2]) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: percent(at[0] as f32 / 6.4),
        top: percent(at[1] as f32 / 4.8),
        width: percent(f32::from(size[0]) / 6.4),
        height: percent(f32::from(size[1]) / 4.8),
        ..default()
    }
}

/// Pen position that centres `text` on `x` by the subtitle rule.
fn centred(hud: &HudAsset, text: &str, x: i32, y: i32) -> [i32; 2] {
    [x - hud.font.width(text) / 2, y]
}

/// The health number below a drawn candle in mode 2, then the count beside an item
/// owned more than once, as lines with their pen positions in 640×480 pixels.
fn text_lines(
    hud: &HudAsset,
    health: Option<u8>,
    item: Option<&HudItem>,
) -> Vec<(String, [i32; 2])> {
    let mut lines = Vec::new();
    if let Some(health) = health {
        let text = health.to_string();
        let x = hud.candle[0] + i32::from(hud.candle_size[0]) / 2 - 5;
        let at = centred(hud, &text, x, hud.candle[1] + i32::from(hud.candle_size[1]));
        lines.push((text, at));
    }
    if let Some(item) = item.filter(|i| i.count > 1) {
        let at = [
            hud.pickup[0] + i32::from(item.size[0]) - 32,
            hud.pickup[1] + 8,
        ];
        lines.push((item.count.to_string(), at));
    }
    lines
}

/// Renders the HUD text lines into one 640×480 overlay image.
fn text_image(hud: &HudAsset, lines: &[(String, [i32; 2])]) -> Image {
    let mut rgba = vec![0; 640 * 480 * 4];
    for (text, at) in lines {
        hud.font
            .draw_shadowed(&mut rgba, 640, text, *at, hud.text_alpha);
    }
    Image::new(
        Extent3d {
            width: 640,
            height: 480,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

#[allow(clippy::too_many_arguments)] // One Bevy system owning the overlay's input, state and nodes.
fn update(
    preferences_store: Res<cyrus_game::preferences::Preferences>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    handle: Option<Res<HudHandle>>,
    data: Res<Assets<HudAsset>>,
    state: Option<ResMut<HudState>>,
    viewport: Query<Entity, With<HudViewport>>,
    mut sprites: Query<(&HudSprite, &mut ImageNode, &mut Node, &mut Visibility)>,
    text_layer: Query<Entity, With<HudText>>,
    (keys, time): (Res<ButtonInput<KeyCode>>, Res<Time>),
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(handle), Ok(viewport)) = (handle, viewport.single()) else {
        return;
    };
    let Some(hud) = data.get(&handle.0) else {
        return;
    };
    let Some(mut state) = state else {
        let load = |name: &str, count: usize| -> Vec<Handle<Image>> {
            (0..count)
                .map(|f| assets.load(format!("cache://hud/{name}-{f:02}.png")))
                .collect()
        };
        // The exporter validates these frame counts.
        let frames = Frames {
            candle: load("candle", 81),
            compass: load("compass", 32),
            logbook: load("logbook", 32),
            powerup: load("powerup", hud.powerup_sizes.len()),
        };
        let preferences = HudPreferences::load(&preferences_store).unwrap_or_else(|e| {
            warn!("hud: {e:#}");
            None
        });
        let mut state = HudState::new(hud, preferences, frames);
        if let Ok(Some(menu)) = &preferences_store.menu {
            state.bindings = menu.bindings;
        }
        commands.insert_resource(state);
        commands.entity(viewport).with_children(|viewport| {
            for (z, sprite) in SPRITES.into_iter().enumerate() {
                viewport.spawn((
                    sprite,
                    ImageNode::default(),
                    Node::default(),
                    Visibility::Hidden,
                    ZIndex(z as i32),
                ));
            }
        });
        return;
    };
    let state = &mut *state;

    // Fixed, non-configurable keys: F8 candle mode, Tab walk mode, F7 auto-defend.
    let mut toggled = false;
    if keys.just_pressed(KeyCode::F8) {
        state.candle_mode = (state.candle_mode + 1) % 3;
        toggled = true;
    }
    for (notice, key) in [KeyCode::Tab, KeyCode::F7].into_iter().enumerate() {
        if keys.just_pressed(key) {
            let flag = if notice == 0 {
                &mut state.walk_mode
            } else {
                &mut state.auto_defend
            };
            *flag = !*flag;
            state.notices[notice] = NOTICE_TICKS;
            toggled = true;
        }
    }
    if toggled {
        let preferences = HudPreferences {
            candle_mode: state.candle_mode,
            walk_mode: state.walk_mode,
            auto_defend: state.auto_defend,
        };
        if let Err(e) = preferences.save(&preferences_store) {
            warn!("hud: {e:#}");
        }
    }

    // Preservation HUD facts: Glide swaps at 60 Hz; keep that animation rate independent of rendering.
    state.elapsed += time.delta_secs();
    let period = 1.0 / 60.0;
    while state.elapsed >= period {
        state.elapsed -= period;
        state.advance_frame();
    }
    state.ticks += time.delta_secs_f64() * TICKS_PER_SECOND;
    let ticks = state.ticks.floor() as u32;
    state.ticks -= f64::from(ticks);

    // Candle.
    if state.sword_drawn && state.lockout == 0 {
        state.show_timer = SHOW_PINNED;
    }
    state.candle_visible = (state.show_timer != 0 || state.candle_mode != 0) && state.lockout == 0;
    if state.candle_visible && state.show_timer < SHOW_PINNED {
        state.show_timer = state.show_timer.saturating_sub(ticks);
    }

    // Item selection. A pending prompt blocks cycling unless the override runs; holding
    // a cycle key starts the override (its first press does not cycle, inferred order).
    let bindings = state.bindings;
    let scan = |action: usize| cyrus_game::player::input::scan_key(bindings[action]);
    let pressed = |action: usize| scan(action).is_some_and(|k| keys.just_pressed(k));
    let held = |action: usize| scan(action).is_some_and(|k| keys.pressed(k));
    let blocked = state.prompt.is_some() && state.prompt_override == 0;
    if !state.sword_drawn && !blocked {
        // Menu actions 723 and 724: next and previous item.
        let position = hud.items.iter().position(|i| Some(i.id) == state.selected);
        let step = match (pressed(14), pressed(13)) {
            (true, false) => hud.items.len().checked_sub(1),
            (false, true) => Some(1),
            _ => None,
        };
        if let (Some(position), Some(step)) = (position, step) {
            state.selected = Some(hud.items[(position + step) % hud.items.len()].id);
        }
    }

    // Item area: lockout 1 freezes it; the hold-off blanks it; a prompt replaces the icon.
    let item = hud.items.iter().find(|i| Some(i.id) == state.selected);
    let mut shown_item = None;
    let mut prompt = None;
    if state.lockout != 1 {
        if state.prompt.is_none() {
            state.prompt_override = 0;
        } else if held(13) || held(14) {
            state.prompt_override = hud.override_ticks;
        } else {
            state.prompt_override = state.prompt_override.saturating_sub(ticks);
        }
        if state.hold_off > 0 {
            state.hold_off = state.hold_off.saturating_sub(ticks);
        } else if state.prompt.is_some() && state.prompt_override == 0 {
            prompt = state.prompt.clone();
        } else if state.player_type != 1 {
            shown_item = item;
        }
    }
    // Scripts offer the prompt anew every frame.
    state.prompt = None;
    state.count = shown_item.map(|i| i.count);

    // Compass: owning item 0 and its display flag.
    let owns_compass = hud.items.iter().any(|i| i.id == 0 && i.count > 0);
    state.compass_frame =
        (state.lockout != 1 && state.player_type != 1 && owns_compass && state.compass_on).then(
            || {
                let world = state.world.and_then(|w| hud.world_compass.get(&w));
                compass_frame(state.heading, world.copied().unwrap_or(0))
            },
        );

    for notice in &mut state.notices {
        *notice = notice.saturating_sub(ticks);
    }

    for (sprite, mut image, mut node, mut visibility) in &mut sprites {
        let frames = &state.frames;
        let target =
            match *sprite {
                HudSprite::Compass => state.compass_frame.map(|f| {
                    (
                        frames.compass[usize::from(f)].clone(),
                        hud.compass,
                        hud.compass_size,
                    )
                }),
                HudSprite::Candle => state.candle_visible.then(|| {
                    let frame = usize::from(state.candle.frame);
                    (frames.candle[frame].clone(), hud.candle, hud.candle_size)
                }),
                HudSprite::Logbook => state.logbook_frame.map(|f| {
                    (
                        frames.logbook[usize::from(f)].clone(),
                        hud.logbook,
                        hud.logbook_size,
                    )
                }),
                HudSprite::Powerup(slot) => state.powerups[slot]
                    .filter(|_| state.lockout == 0)
                    .map(|(_, f)| {
                        let f = usize::from(f);
                        (
                            frames.powerup[f].clone(),
                            hud.powerup_slots[slot],
                            hud.powerup_sizes[f],
                        )
                    }),
                HudSprite::Item => shown_item.map(|item| {
                    let path = format!("cache://hud/item-{:02}.png", item.bitmap);
                    (assets.load(path), hud.pickup, item.size)
                }),
            };
        let Some((frame, at, size)) = target else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        if image.image != frame {
            image.image = frame;
        }
        node.set_if_neq(place(at, size));
        visibility.set_if_neq(Visibility::Inherited);
    }

    // Text drawn last: none of its lines overlaps a later bitmap in the original order.
    let health = (state.candle_visible && state.candle_mode == 2).then_some(state.health);
    let mut text = text_lines(hud, health, shown_item);
    for (notice, y) in [(0, 10), (1, 30)] {
        if state.notices[notice] > 0 {
            let on = [state.walk_mode, state.auto_defend][notice];
            let line = hud.notices[notice][usize::from(on)].clone();
            text.push((line.clone(), centred(hud, &line, 320, y)));
        }
    }
    if let Some(prompt) = prompt {
        text.push((prompt, hud.prompt));
    }
    if text != state.text {
        for entity in &text_layer {
            commands.entity(entity).despawn();
        }
        if !text.is_empty() {
            let image = images.add(text_image(hud, &text));
            commands.entity(viewport).with_child((
                HudText,
                ImageNode::new(image),
                place([0, 0], [640, 480]),
                ZIndex(SPRITES.len() as i32),
            ));
        }
        state.text = text;
    }
}

fn despawn(mut commands: Commands, content: Query<Entity, With<HudContent>>) {
    for entity in &content {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<HudState>();
    commands.remove_resource::<HudHandle>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn frames(candle: &mut Candle, health: u8, n: usize) -> Vec<u16> {
        (0..n).map(|_| candle.advance(health)).collect()
    }

    #[test]
    fn candle_levels_follow_the_published_health_table() {
        let levels: Vec<_> = [100, 91, 90, 82, 81, 54, 46, 36, 27, 9, 8, 1, 0]
            .map(candle_level)
            .into();
        assert_eq!(levels, [0, 0, 1, 1, 2, 5, 5, 7, 7, 9, 10, 10, 11]);
    }

    #[test]
    fn candle_flickers_steps_between_levels_and_snuffs() {
        let mut candle = Candle::default();
        assert_eq!(frames(&mut candle, 100, 6), [1, 2, 3, 4, 0, 1]);
        assert_eq!(
            frames(&mut candle, 50, 10),
            [5, 11, 17, 23, 29, 30, 31, 32, 33, 34]
        );
        assert_eq!(frames(&mut candle, 100, 7), [29, 23, 17, 11, 5, 0, 1]);
        let mut candle = Candle::default();
        frames(&mut candle, 50, 1);
        let snuff = frames(&mut candle, 0, 17);
        assert_eq!(snuff[..15], (66..=80).collect::<Vec<_>>()[..]);
        assert_eq!(snuff[15..], [80, 80]);
        assert_eq!(frames(&mut candle, 100, 3), [65, 59, 53]);
    }

    #[test]
    fn compass_frames_follow_the_published_table() {
        let frames: Vec<_> = [(0, 0), (1985, 0), (2047, 0), (1984, 0), (1024, 0), (65, 0)]
            .into_iter()
            .chain([(1, 0), (64, 0), (0, 2000), (1000, 2000)])
            .map(|(heading, world)| compass_frame(heading, world))
            .collect();
        assert_eq!(frames, [0, 0, 0, 1, 16, 30, 31, 31, 0, 17]);
    }

    #[test]
    fn powerup_slots_fill_then_alternate_and_clear_by_id() {
        let mut state = HudState::new(
            &asset(2),
            None,
            Frames {
                powerup: vec![default(); 2],
                ..default()
            },
        );
        state.show_bitmap(0, 0).unwrap();
        state.show_bitmap(1, 0).unwrap();
        assert_eq!(
            state.powerups,
            [Some((0, 0)), None],
            "a shown frame is kept"
        );
        state.show_bitmap(1, 1).unwrap();
        assert_eq!(state.powerups, [Some((0, 0)), Some((1, 1))]);
        state.unshow_bitmap(0);
        state.show_bitmap(0, 0).unwrap();
        assert_eq!(state.powerups, [Some((0, 0)), Some((1, 1))]);
        assert!(state.show_bitmap(2, 0).is_err() && state.show_bitmap(0, 2).is_err());
    }

    #[test]
    fn show_timer_and_logbook_follow_their_triggers() {
        let mut state = HudState::new(&asset(0), None, Frames::default());
        state.set_health(80);
        assert_eq!(state.show_timer, SHOW_TICKS);
        state.show_timer = SHOW_PINNED;
        state.set_health(70);
        assert_eq!(state.show_timer, SHOW_PINNED, "a pinned timer is kept");
        state.logbook_entry();
        for _ in 0..31 {
            state.advance_frame();
        }
        assert_eq!(state.logbook_frame, Some(31));
        state.logbook_entry();
        state.advance_frame();
        assert_eq!(state.logbook_frame, None, "plays once, 32 frames");
    }

    #[test]
    fn activation_fires_with_use_and_holds_off() {
        let mut state = HudState::new(&asset(2), None, Frames::default());
        assert!(!state.activate("Open", false));
        assert_eq!(state.prompt.as_deref(), Some("Open"));
        state.prompt = None;
        assert!(state.activate("Open", true));
        assert_eq!(state.hold_off, ACTIVATION_HOLD_OFF);
        assert!(!state.activate("Open", false), "hold-off blocks prompts");
        state.deactivate();
        state.set_sword_drawn(true);
        assert!(
            !state.activate("Open", false),
            "a drawn sword blocks prompts"
        );
    }

    #[test]
    fn text_lines_centre_health_under_the_candle_and_place_counts() {
        let digit = cyrus_text::Glyph {
            enabled: true,
            width: 9,
            ..default()
        };
        let mut hud = asset(2);
        hud.font = cyrus_text::Font {
            start: 48,
            glyphs: vec![digit; 10],
            ..default()
        };
        let item = HudItem {
            id: 2,
            bitmap: 26,
            count: 53,
            size: [40, 30],
        };
        // "100" is 30 wide; the candle's centre minus 5 is 12 + 25 - 5 = 32.
        assert_eq!(
            text_lines(&hud, Some(100), Some(&item)),
            [("100".into(), [17, 106]), ("53".into(), [20, 394])]
        );
        assert!(text_lines(&hud, None, Some(&HudItem { count: 1, ..item })).is_empty());
    }

    fn asset(candle_mode: u8) -> HudAsset {
        HudAsset {
            candle_mode,
            candle: [12, 8],
            candle_size: [50, 98],
            pickup: [12, 386],
            prompt: [12, 436],
            override_ticks: 24,
            compass: [546, 396],
            compass_size: [73, 65],
            world_compass: BTreeMap::new(),
            powerup_slots: [[576, 6], [576, 96]],
            powerup_sizes: vec![[39, 61], [54, 61]],
            logbook: [540, 20],
            logbook_size: [73, 58],
            walk_mode: false,
            auto_defend: true,
            notices: default(),
            items: vec![],
            selected: None,
            bindings: [0; 15],
            font: default(),
            text_alpha: [255, 199],
        }
    }

    #[test]
    fn changing_between_equal_count_items_updates_the_icon() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<HudAsset>()
            .init_resource::<cyrus_game::preferences::Preferences>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, update);
        let mut bindings = [0; 15];
        bindings[13] = 0x34;
        let item = |id| HudItem {
            id,
            bitmap: id,
            count: 1,
            size: [1, 1],
        };
        let hud = app
            .world_mut()
            .resource_mut::<Assets<HudAsset>>()
            .add(HudAsset {
                items: vec![item(1), item(2)],
                selected: Some(1),
                bindings,
                ..asset(0)
            });
        app.world_mut().insert_resource(HudHandle(hud));
        app.world_mut().spawn(HudViewport);
        app.update();
        app.world_mut().resource_mut::<HudState>().bindings = bindings;
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Period);
        app.update();
        assert_eq!(app.world().resource::<HudState>().selected, Some(2));
        let expected: Handle<Image> = app
            .world()
            .resource::<AssetServer>()
            .load("cache://hud/item-02.png");
        let (image, visibility) = app
            .world_mut()
            .query::<(&HudSprite, &ImageNode, &Visibility)>()
            .iter(app.world())
            .find(|(sprite, ..)| **sprite == HudSprite::Item)
            .map(|(_, node, visibility)| (node.image.clone(), *visibility))
            .unwrap();
        assert_eq!((image, visibility), (expected, Visibility::Inherited));
    }
}
