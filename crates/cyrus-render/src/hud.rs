//! In-game overlay from installed HUD data: candle, compass, logbook icon, toggle notices,
//! power-up slots, active item, prompt text. Rules follow redguard-preservation's HUD documentation.
mod artwork;
mod composition;
mod inventory;
mod overlay;
mod persistence;
use artwork::StoredArtwork;
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::hud::{HudAsset, HudItem};
use cyrus_game::AppState;
use cyrus_script::{
    player::{
        Player,
        data::{PlayerScene, PlayerSceneRequest},
        placement::PlayerPose,
    },
    script::{ActivationInputs, ActivationOverlay, ScriptActors},
};
use cyrus_soup::Attribute;
use cyrus_world::heading::wrap;
use inventory::{inventory_draw, spawn_inventory};
use overlay::update;
pub use persistence::HudPresentationSnapshot;
use serde::{Deserialize, Serialize};

/// Shows the exported `hud/overlay.hud` over gameplay, fitted like the original 640×480 viewport.
pub struct HudPlugin;

/// The player's fixed maximum and spawn health; no INI key sets it.
const MAX_HEALTH: u8 = 100;
/// Show timer values: a trigger shows the candle for 25 ticks; 50 pins it and never decays.
/// Walk-mode and auto-defend notices last 24 ticks.
const NOTICE_TICKS: u32 = 24;

/// Candle level for player health: 0 is the full candle, 11 the snuffed one.
fn candle_level(health: u8) -> u8 {
    let max = u32::from(MAX_HEALTH);
    let lost = (max - u32::from(health).min(max)) * 256 / max;
    ((lost * 11) >> 8) as u8
}

/// The candle's shown level and flicker step; advanced once per original rendered frame.
#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
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
    (wrap(-(i32::from(heading) + world_compass)) >> 6) as u8
}

/// Player-facing toggles kept outside the install: native `hud.json` in the config directory,
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
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct HudState {
    /// Player health, 0..=100, from logical player `attr_health` when available.
    /// Use [`HudState::set_health`].
    pub health: u8,
    /// Candle visibility mode; F8 cycles 0 → 1 → 2 → 0.
    pub candle_mode: u8,
    pub candle_visible: bool,
    candle: Candle,
    /// Caller-supplied activation lockout, retaining all 16 bits.
    pub lockout: u16,
    /// Selected player weapon state, projected from the logical actor when known.
    pub sword_drawn: bool,
    /// Logical selected-player `attr_player_type` when actor state is available.
    pub player_type: u8,
    /// Placed player's heading in 2048-unit turns and the loaded WORLD.INI world ID, for the compass.
    pub heading: u16,
    pub world: Option<u32>,
    /// Shared session compass display flag, toggled by using owned item 0.
    pub compass_on: bool,
    pub compass_frame: Option<u8>,
    /// Script-owned power-up slots as `(id, frame)`.
    pub powerups: [Option<(u8, u8)>; 2],
    /// Logbook icon frame while it plays; see [`HudState::logbook_entry`].
    pub logbook_frame: Option<u8>,
    /// Display projections of the shared gameplay walk-mode and auto-defend toggles.
    pub walk_mode: bool,
    pub auto_defend: bool,
    /// Remaining ticks of the walk-mode and auto-defend notices.
    notices: [u32; 2],
    /// Activation prompt drawn by the last HUD update; pending text belongs to ScriptWorld.
    pub prompt: Option<String>,
    /// Selected item ID, or `None` when nothing is owned.
    pub selected: Option<u16>,
    pub count: Option<u32>,
    /// The held Next/Previous item press already stepped the selection; a release re-arms it.
    #[serde(skip)]
    item_press_consumed: bool,
    #[serde(skip)]
    elapsed: f32,
    /// Every frame of the fixed atlases, held so swapping frames never reloads an image.
    #[serde(skip)]
    frames: HudPresentationAssets,
    /// Text lines and pen positions currently drawn on the text layer.
    #[serde(skip)]
    text: Vec<(String, [i32; 2])>,
    #[serde(skip)]
    dialogue_text: Option<(usize, String)>,
    /// Scripted menu items currently drawn: text, pen position and alpha.
    #[serde(skip)]
    menu_text: Vec<MenuLine>,
    #[serde(skip)]
    redraw_text: bool,
}

#[derive(Default, Clone, Debug)]
/// Prepared installed HUD image handles for off-live presentation reconstruction.
pub struct HudPresentationAssets {
    pub candle: Vec<Handle<Image>>,
    pub compass: Vec<Handle<Image>>,
    pub logbook: Vec<Handle<Image>>,
    pub powerup: Vec<Handle<Image>>,
}

impl HudPresentationAssets {
    /// Request installed overlay frames without creating or changing HUD state.
    pub fn request(hud: &HudAsset, assets: &AssetServer) -> Self {
        let load = |name: &str, count: usize| -> Vec<Handle<Image>> {
            (0..count)
                .map(|f| {
                    assets
                        .load_builder()
                        .with_settings(|settings: &mut ImageLoaderSettings| {
                            settings.sampler = ImageSampler::nearest();
                        })
                        .load(format!("cache://hud/{name}-{f:02}.png"))
                })
                .collect()
        };
        // The exporter validates these frame counts.
        Self {
            candle: load("candle", 81),
            compass: load("compass", 32),
            logbook: load("logbook", 32),
            powerup: load("powerup", hud.powerup_sizes.len()),
        }
    }
}

impl HudState {
    fn new(
        hud: &HudAsset,
        preferences: Option<HudPreferences>,
        frames: HudPresentationAssets,
    ) -> Self {
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
            lockout: 0,
            sword_drawn: false,
            player_type: 0,
            heading: 0,
            world: None,
            compass_on: false,
            compass_frame: None,
            powerups: [None; 2],
            logbook_frame: None,
            walk_mode: preferences.walk_mode,
            auto_defend: preferences.auto_defend,
            notices: [0; 2],
            prompt: None,
            selected: hud.selected.or_else(|| hud.items.first().map(|i| i.id)),
            count: None,
            item_press_consumed: false,
            elapsed: 0.0,
            frames,
            text: Vec::new(),
            dialogue_text: None,
            menu_text: Vec::new(),
            redraw_text: false,
        }
    }

    /// Sets player health (clamped to 100); a change shows the candle for 25 ticks
    /// unless the timer is pinned.
    pub fn set_health(&mut self, health: u8, hud: &cyrus_script::script::ScriptHud) {
        let health = health.min(MAX_HEALTH);
        if health != self.health {
            hud.request_health_feedback();
        }
        self.health = health;
    }

    /// Drawing or sheathing the sword shows the candle for 25 ticks.
    pub fn set_sword_drawn(&mut self, drawn: bool, hud: &cyrus_script::script::ScriptHud) {
        if drawn != self.sword_drawn {
            hud.reset_health_feedback();
        }
        self.sword_drawn = drawn;
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

/// Installed HUD definition currently bound by the renderer. Session capture must
/// use this asset rather than choosing an arbitrary loaded HUD definition.
pub fn bound_asset(world: &World) -> Option<&HudAsset> {
    let handle = world.get_resource::<HudHandle>()?;
    world.get_resource::<Assets<HudAsset>>()?.get(&handle.0)
}

#[derive(Component)]
struct HudContent;

#[derive(Component)]
struct InventoryOverlay;

/// The examine view's description text, drawn above the examined model.
#[derive(Component)]
struct ExamineTextOverlay;

/// The island map screen: the map image and the cross frame over a black screen.
#[derive(Component)]
struct IslandMapOverlay;

#[derive(Component)]
struct IslandMapStage;

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
        context: &mut LoadContext<'_>,
    ) -> Result<HudAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let mut hud: HudAsset = serde_json::from_slice(&bytes).map_err(|error| {
            anyhow::anyhow!("HUD cache invalid; re-export installed HUD data: {error}")
        })?;
        for (name, count) in [
            ("candle", 81),
            ("compass", 32),
            ("logbook", 32),
            ("powerup", hud.powerup_sizes.len()),
        ] {
            hud.images.extend((0..count).map(|frame| {
                context
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| {
                        settings.sampler = ImageSampler::nearest();
                    })
                    .load::<Image>(format!("cache://hud/{name}-{frame:02}.png"))
                    .untyped()
            }));
        }
        for item in &hud.items {
            hud.images.push(
                context
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| {
                        settings.sampler = ImageSampler::nearest();
                    })
                    .load::<Image>(format!("cache://hud/item-{:02}.png", item.bitmap))
                    .untyped(),
            );
            hud.images.push(
                context
                    .load_builder()
                    .with_settings(|s: &mut ImageLoaderSettings| {
                        s.sampler = ImageSampler::nearest();
                    })
                    .load::<Image>(format!("cache://hud/selected-{:02}.png", item.bitmap))
                    .untyped(),
            );
        }
        for (name, count) in [("invbordr", 1usize), ("invarrow", 2)] {
            hud.images.extend((0..count).map(|frame| {
                context
                    .load_builder()
                    .with_settings(|s: &mut ImageLoaderSettings| {
                        s.sampler = ImageSampler::nearest();
                    })
                    .load::<Image>(format!("cache://hud/{name}-{frame:02}.png"))
                    .untyped()
            }));
        }
        Ok(hud)
    }
    fn extensions(&self) -> &[&str] {
        &["hud"]
    }
}

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(composition::CompositionPlugin)
            .init_resource::<artwork::ArtworkStorage>()
            .add_systems(PostUpdate, artwork::store_artwork)
            .init_resource::<cyrus_game::preferences::Preferences>()
            .init_resource::<cyrus_script::script::ScriptHud>()
            .init_asset::<HudAsset>()
            .init_asset_loader::<HudLoader>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(OnEnter(AppState::InGame), spawn_inventory.after(spawn))
            .add_systems(OnEnter(AppState::InGame), spawn_island_map)
            .add_systems(
                Update,
                configure_scripts
                    .before(cyrus_script::script::OrdinaryUpdates)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                update
                    .after(cyrus_script::script::OrdinaryUpdates)
                    .after(cyrus_script::player::input::InputSample)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                inventory_draw
                    .after(update)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                examine_text_draw
                    .after(update)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                island_map_draw
                    .after(update)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                PostUpdate,
                hide_for_book_capture
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(OnExit(AppState::InGame), (despawn, despawn_island_map));
    }
}

/// The book's gameplay picture holds no HUD: the view is captured in the frame that requests the
/// book, so the overlay is hidden for exactly that frame (verified against the original's fall and
/// death book pictures).
fn hide_for_book_capture(
    next: Res<NextState<AppState>>,
    mut content: Query<&mut Visibility, With<HudContent>>,
) {
    let book_pending = matches!(*next, NextState::Pending(AppState::Menu));
    for mut visibility in &mut content {
        visibility.set_if_neq(if book_pending {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

fn spawn(mut commands: Commands, assets: Res<AssetServer>, existing: Query<(), With<HudContent>>) {
    if !existing.is_empty() {
        return;
    }
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

fn spawn_island_map(mut commands: Commands) {
    commands
        .spawn((
            IslandMapOverlay,
            Visibility::Hidden,
            GlobalZIndex(110),
            BackgroundColor(Color::BLACK),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_child((
            IslandMapStage,
            // A 4:3 box as large as the window allows; children use its 640×480 units.
            Node {
                height: percent(100),
                max_width: percent(100),
                aspect_ratio: Some(640.0 / 480.0),
                ..default()
            },
        ));
}

fn despawn_island_map(mut commands: Commands, overlays: Query<Entity, With<IslandMapOverlay>>) {
    for overlay in &overlays {
        commands.entity(overlay).despawn();
    }
}

/// Draws the map image centred vertically and the current cross frame at the marker, rebuilding
/// the two images only when the frame changes.
#[allow(clippy::too_many_arguments)] // Bevy supplies each independent resource and query as a parameter.
fn island_map_draw(
    screen: Option<Res<cyrus_script::island_map::IslandMapScreen>>,
    hud_handle: Option<Res<HudHandle>>,
    huds: Res<Assets<HudAsset>>,
    environment: Res<crate::environment::EnvironmentRequest>,
    environments: Res<Assets<crate::environment::Environment>>,
    assets: Res<AssetServer>,
    mut overlays: Query<&mut Visibility, With<IslandMapOverlay>>,
    stages: Query<(Entity, Option<&Children>), With<IslandMapStage>>,
    mut commands: Commands,
    mut shown: Local<Option<usize>>,
) {
    use cyrus_script::island_map::Phase;
    let (Some(screen), Some(handle), Ok(mut visibility), Ok((overlay, children))) =
        (screen, hud_handle, overlays.single_mut(), stages.single())
    else {
        return;
    };
    if !screen.open() {
        visibility.set_if_neq(Visibility::Hidden);
        *shown = None;
        return;
    }
    visibility.set_if_neq(Visibility::Inherited);
    let Some(hud) = huds.get(&handle.0) else {
        return;
    };
    let Some(environment) = environments.get(&environment.0) else {
        return;
    };
    let art = &hud.island_map;
    // The cross animation ends on its last frame, which stays up while the screen waits.
    let frame = match screen.phase {
        Phase::Playing { frame } => frame,
        _ => art.cross.len().saturating_sub(1),
    };
    if *shown == Some(frame) {
        return;
    }
    *shown = Some(frame);
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    let Some(cross_size) = art.cross.get(frame).copied() else {
        return;
    };
    commands.entity(overlay).with_children(|parent| {
        let top = (480 - i32::from(art.map[1])) / 2;
        parent.spawn((
            StoredArtwork,
            artwork::ArtworkFog(environment.background),
            ImageNode::new(assets.load("cache://hud/mapmap-00.png")),
            place([0, top], art.map),
            ZIndex(1),
        ));
        parent.spawn((
            StoredArtwork,
            artwork::ArtworkFog(environment.background),
            ImageNode::new(assets.load(format!("cache://hud/cross-{frame:02}.png"))),
            place(screen.cross, cross_size),
            ZIndex(2),
        ));
    });
}

/// Inventory names and counts share the installed font's shadow placement.
/// The examine view's current description line (preservation `engine/hud.md` examine view): the
/// RTX text wrapped with a 30-byte step, each line centred on x = 320 with its top at
/// `10 + trunc(0.9 × n × line_height)`, shown only with `[dialog] dialog_print_text`.
fn examine_text_draw(
    examine: Option<Res<cyrus_script::item_viewer::ExamineScreen>>,
    settings: Option<Res<cyrus_assets::sound::SoundSettings>>,
    hud_handle: Option<Res<HudHandle>>,
    huds: Res<Assets<HudAsset>>,
    mut images: ResMut<Assets<Image>>,
    mut overlays: Query<(&mut Visibility, &mut ImageNode), With<ExamineTextOverlay>>,
    mut shown: Local<Option<[u8; 4]>>,
) {
    let Ok((mut visibility, mut node)) = overlays.single_mut() else {
        return;
    };
    let printing = settings.as_ref().is_none_or(|settings| settings.subtitles);
    let label = examine
        .filter(|examine| examine.open && printing)
        .and_then(|examine| examine.text);
    if label == *shown {
        return;
    }
    let Some(label) = label else {
        *shown = None;
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let Some(hud) = hud_handle.and_then(|handle| huds.get(&handle.0)) else {
        return;
    };
    *shown = Some(label);
    let key = String::from_utf8_lossy(&label);
    // Text starting with byte 0x9E counts as absent.
    let Some(text) = hud
        .labels
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&key))
        .map(|(_, text)| text.as_str())
        .filter(|text| !text.is_empty() && !text.starts_with('\u{9e}'))
    else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let mut rgba = vec![0; 640 * 480 * 4];
    for (line, text) in cyrus_script::inventory_screen::wrap_lines(text, 30)
        .iter()
        .enumerate()
    {
        let y = 10 + (line as i32 * i32::from(hud.font.line_height) * 9) / 10;
        hud.font.draw_shadowed(
            &mut rgba,
            640,
            text,
            [320 - hud.font.width(text) / 2, y],
            hud.text_style,
        );
    }
    let mut image = Image::new(
        Extent3d {
            width: 640,
            height: 480,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    node.image = images.add(image);
    visibility.set_if_neq(Visibility::Inherited);
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

/// One drawn scripted-menu item: its text, pen position and installed colour/alpha style.
type MenuLine = (String, [i32; 2], cyrus_text::FontStyle);

/// Scripted menu items stacked from the installed menu origin, one font line apart, in 640×480
/// pixels. The highlighted item uses `font_sel`, items already chosen `font_used`, the rest
/// `font_norm`, retaining both colour scale and alpha.
fn menu_lines(
    hud: &HudAsset,
    start: [i32; 2],
    menu: &cyrus_script::script::MenuState,
) -> Vec<MenuLine> {
    menu.items()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let alpha = if index == menu.highlighted() {
                hud.text_style
            } else if item.flag == 1 {
                hud.used_text_style
            } else {
                hud.player_text_style
            };
            let at = [
                start[0],
                start[1] + i32::from(hud.font.line_height) * index as i32,
            ];
            (item.text.clone(), at, alpha)
        })
        .collect()
}

/// Already formatted dialogue uses preservation's fixed virtual-screen band.
fn dialogue_lines(hud: &HudAsset, text: &str) -> Vec<(String, [i32; 2])> {
    let breaks = text.bytes().filter(|byte| *byte == b'\n').count();
    let top = 470i64 - 26 * (breaks.max(1) as i64 + 1);
    text.split('\n')
        .enumerate()
        .map(|(index, line)| {
            (
                line.to_owned(),
                centred(hud, line, 320, (top + index as i64 * 26) as i32),
            )
        })
        .collect()
}

/// Renders the HUD text lines into one 640×480 overlay image.
fn text_image(
    hud: &HudAsset,
    lines: &[(String, [i32; 2])],
    dialogue: Option<&(usize, String)>,
    menu: &[MenuLine],
) -> Image {
    let mut rgba = vec![0; 640 * 480 * 4];
    for (text, at) in lines {
        hud.font
            .draw_shadowed_wrapped(&mut rgba, 640, text, *at, hud.text_style);
    }
    if let Some((slot, text)) = dialogue {
        let alpha = if *slot == 0 {
            hud.player_text_style
        } else {
            hud.text_style
        };
        for (line, at) in dialogue_lines(hud, text) {
            hud.font.draw_shadowed(&mut rgba, 640, &line, at, alpha);
        }
    }
    for (text, at, alpha) in menu {
        hud.font.draw_shadowed(&mut rgba, 640, text, *at, *alpha);
    }
    let mut image = Image::new(
        Extent3d {
            width: 640,
            height: 480,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// The placed player and the loaded world's player scene, which supply the compass.
#[derive(bevy::ecs::system::SystemParam)]
struct CompassInputs<'w, 's> {
    player: Query<'w, 's, &'static PlayerPose, With<Player>>,
    request: Option<Res<'w, PlayerSceneRequest>>,
    scenes: Option<Res<'w, Assets<PlayerScene>>>,
    suppressed: Option<Res<'w, cyrus_script::player::input::InputSuppressed>>,
    console: Option<Res<'w, cyrus_console::Console>>,
    clock: Option<Res<'w, cyrus_script::clock::SimulationClock>>,
    logbook: Option<Res<'w, cyrus_script::logbook::LogbookScreen>>,
    inventory: Option<Res<'w, cyrus_script::inventory_screen::InventoryScreen>>,
}

fn configure_scripts(
    handle: Option<Res<HudHandle>>,
    data: Res<Assets<HudAsset>>,
    hud: Res<cyrus_script::script::ScriptHud>,
    preferences: Res<cyrus_game::preferences::Preferences>,
) {
    if let Some(asset) = handle.as_ref().and_then(|handle| data.get(&handle.0)) {
        hud.configure(asset, &preferences);
    }
}

fn despawn(
    mut commands: Commands,
    content: Query<Entity, With<HudContent>>,
    overlay: Option<Res<cyrus_game::GameplayOverlay>>,
) {
    if overlay.is_some() {
        return;
    }
    for entity in &content {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<HudState>();
    // The handle stays so a save made from the paused book can still capture the bound HUD.
}

#[cfg(test)]
mod tests;
