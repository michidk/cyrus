//! In-game overlay from installed HUD data: candle, compass, logbook icon, toggle notices,
//! power-up slots, active item, prompt text. Rules follow redguard-preservation's HUD documentation.
mod persistence;
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::hud::{HudAsset, HudItem};
use cyrus_game::{
    AppState,
    player::{
        Player,
        data::{PlayerScene, PlayerSceneRequest},
        placement::PlayerPose,
    },
    script::{ActivationInputs, ActivationOverlay, ScriptActors},
};
pub use persistence::HudPresentationSnapshot;
use serde::{Deserialize, Serialize};

/// Shows the exported `hud/overlay.hud` over gameplay, fitted like the original 640×480 viewport.
pub struct HudPlugin;

/// The player's fixed maximum and spawn health; no INI key sets it.
const MAX_HEALTH: u8 = 100;
/// Show timer values: a trigger shows the candle for 25 ticks; 50 pins it and never decays.
const SHOW_TICKS: u32 = 25;
const SHOW_PINNED: u32 = 50;
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
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct HudState {
    /// Player health, 0..=100, from logical player attribute 3 when available.
    /// Use [`HudState::set_health`].
    pub health: u8,
    /// Candle visibility mode; F8 cycles 0 → 1 → 2 → 0.
    pub candle_mode: u8,
    pub candle_visible: bool,
    candle: Candle,
    /// Candle show timer in ticks (25 after a trigger, 50 pinned).
    show_timer: u32,
    /// Caller-supplied activation lockout, retaining all 16 bits.
    pub lockout: u16,
    /// Selected player weapon state, projected from the logical actor when known.
    pub sword_drawn: bool,
    /// Logical selected-player attribute 105 when actor state is available.
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
    #[serde(skip)]
    elapsed: f32,
    /// Edited bindings when present, else the installed ones; read on entering a world.
    #[serde(skip)]
    bindings: [u8; 15],
    /// Every frame of the fixed atlases, held so swapping frames never reloads an image.
    #[serde(skip)]
    frames: HudPresentationAssets,
    /// Text lines and pen positions currently drawn on the text layer.
    #[serde(skip)]
    text: Vec<(String, [i32; 2])>,
    #[serde(skip)]
    dialogue_text: Option<(usize, String)>,
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
            show_timer: 0,
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
            elapsed: 0.0,
            bindings: hud.bindings,
            frames,
            text: Vec::new(),
            dialogue_text: None,
            redraw_text: false,
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

    /// An explicit weapon cleanup resets the timer even without a flag transition.
    pub fn show_candle(&mut self) {
        self.show_timer = SHOW_TICKS;
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
        }
        Ok(hud)
    }
    fn extensions(&self) -> &[&str] {
        &["hud"]
    }
}

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<cyrus_game::preferences::Preferences>()
            .init_resource::<cyrus_game::script::ScriptHud>()
            .init_asset::<HudAsset>()
            .init_asset_loader::<HudLoader>()
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(
                Update,
                configure_scripts
                    .before(cyrus_game::script::OrdinaryUpdates)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Update,
                update
                    .after(cyrus_game::script::OrdinaryUpdates)
                    .after(cyrus_game::player::input::InputSample)
                    .run_if(in_state(AppState::InGame)),
            )
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
) -> Image {
    let mut rgba = vec![0; 640 * 480 * 4];
    for (text, at) in lines {
        hud.font
            .draw_shadowed(&mut rgba, 640, text, *at, hud.text_alpha);
    }
    if let Some((slot, text)) = dialogue {
        let alpha = if *slot == 0 {
            hud.player_text_alpha
        } else {
            hud.text_alpha
        };
        for (line, at) in dialogue_lines(hud, text) {
            hud.font.draw_shadowed(&mut rgba, 640, &line, at, alpha);
        }
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
    suppressed: Option<Res<'w, cyrus_game::player::input::InputSuppressed>>,
    console: Option<Res<'w, cyrus_console::Console>>,
    clock: Option<Res<'w, cyrus_game::clock::SimulationClock>>,
    logbook: Option<Res<'w, cyrus_game::logbook::LogbookScreen>>,
}

fn configure_scripts(
    handle: Option<Res<HudHandle>>,
    data: Res<Assets<HudAsset>>,
    hud: Res<cyrus_game::script::ScriptHud>,
    preferences: Res<cyrus_game::preferences::Preferences>,
) {
    if let Some(asset) = handle.as_ref().and_then(|handle| data.get(&handle.0)) {
        hud.configure(asset, &preferences);
    }
}

#[allow(clippy::too_many_arguments)] // One Bevy system owning the overlay's input, state and nodes.
fn update(
    script_hud: Res<cyrus_game::script::ScriptHud>,
    preferences_store: Res<cyrus_game::preferences::Preferences>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    handle: Option<Res<HudHandle>>,
    data: Res<Assets<HudAsset>>,
    (state, toggles): (
        Option<ResMut<HudState>>,
        Option<ResMut<cyrus_game::player::input::PlayerToggles>>,
    ),
    viewport: Query<Entity, With<HudViewport>>,
    mut sprites: Query<(&HudSprite, &mut ImageNode, &mut Node, &mut Visibility)>,
    text_layer: Query<Entity, With<HudText>>,
    (keys, time, sound_settings): (
        Res<ButtonInput<KeyCode>>,
        Res<Time>,
        Option<Res<cyrus_assets::sound::SoundSettings>>,
    ),
    mut images: ResMut<Assets<Image>>,
    (mut actors, activation): (Option<ResMut<ScriptActors>>, Option<Res<ActivationInputs>>),
    compass: CompassInputs,
) {
    let (Some(handle), Ok(viewport)) = (handle, viewport.single()) else {
        return;
    };
    let Some(hud) = data.get(&handle.0) else {
        return;
    };
    let Some(mut state) = state else {
        let frames = HudPresentationAssets::request(hud, &assets);
        let preferences = HudPreferences::load(&preferences_store).unwrap_or_else(|e| {
            warn!("hud: {e:#}");
            None
        });
        let mut state = HudState::new(hud, preferences, frames);
        if let Some(toggles) = toggles.as_ref() {
            state.walk_mode = toggles.walk_mode;
            state.auto_defend = toggles.auto_defend;
        } else {
            commands.insert_resource(cyrus_game::player::input::PlayerToggles {
                walk_mode: state.walk_mode,
                auto_defend: state.auto_defend,
            });
        }
        if let Ok(Some(menu)) = preferences_store.menu() {
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
    if compass.console.as_ref().is_some_and(|console| console.open) {
        return;
    }
    if compass
        .logbook
        .as_ref()
        .is_some_and(|screen| screen.suspended())
    {
        state.logbook_frame = None;
        for (_, _, _, mut visibility) in &mut sprites {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    let (powerups, notification) = script_hud.presentation();
    state.powerups = powerups;
    let start_notification = notification && state.logbook_frame.is_none();

    let Some(mut toggles) = toggles else {
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
                &mut toggles.walk_mode
            } else {
                &mut toggles.auto_defend
            };
            *flag = !*flag;
            state.notices[notice] = NOTICE_TICKS;
            toggled = true;
        }
    }
    state.walk_mode = toggles.walk_mode;
    state.auto_defend = toggles.auto_defend;
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

    let dialogue_text = actors
        .as_ref()
        .and_then(|actors| actors.current.as_ref())
        .and_then(|world| world.globals().dialogue())
        .and_then(|dialogue| dialogue.presented_line())
        .filter(|_| {
            sound_settings
                .as_ref()
                .is_some_and(|settings| settings.subtitles)
        })
        .map(|(slot, text)| (slot, text.to_owned()));
    let mut world = actors.as_mut().and_then(|actors| actors.current.as_mut());
    if world
        .as_mut()
        .is_some_and(|world| world.globals_mut().take_candle_show_request())
    {
        state.show_candle();
    }
    let activation = activation.as_deref().filter(|inputs| {
        world
            .as_ref()
            .is_some_and(|world| world.generation() == inputs.player.generation)
    });
    state.lockout = world
        .as_ref()
        .and_then(|world| world.globals().lockout())
        .unwrap_or_else(|| activation.map_or(0, |inputs| inputs.lockout));
    let player_state = activation
        .and_then(|inputs| {
            inputs
                .actors
                .get(&inputs.player.index)
                .copied()
                .map(|actor| (inputs.player, actor))
        })
        .or_else(|| {
            if activation.is_some() {
                return None;
            }
            world.as_ref()?.player_activation_state().ok()
        });
    let on_edge = player_state.is_some_and(|(_, actor)| (10..=19).contains(&actor.state));
    if let Some((id, actor)) = player_state
        && let Some(player) = world
            .as_ref()
            .and_then(|world| world.actors().get(id.index))
    {
        let attributes = player.attributes();
        state.player_type = attributes[105];
        state.set_health(attributes[3]);
        state.set_sword_drawn(
            player
                .control_state()
                .map_or(actor.weapon_drawn, |control| control.weapon_drawn),
        );
    }
    let compass_eligible = match (world.as_ref(), activation, player_state) {
        (Some(world), Some(inputs), _) => world
            .actors()
            .get(inputs.player.index)
            .is_some_and(|player| inputs.compass_eligible(player).unwrap_or(false)),
        (Some(world), None, Some((id, actor))) => actor.compass_eligible(
            world.actors()[id.index].attributes()[110],
            script_hud.ready(),
            false,
        ),
        _ => false,
    };

    // Preservation HUD facts: Glide swaps at 60 Hz; keep that animation rate independent of rendering.
    state.elapsed += time.delta_secs();
    let period = 1.0 / 60.0;
    while state.elapsed >= period {
        state.elapsed -= period;
        state.advance_frame();
    }
    if start_notification {
        state.logbook_entry();
    }
    let ticks = compass
        .clock
        .as_ref()
        .filter(|clock| clock.is_changed())
        .map_or(0, |clock| clock.step().nominal_ticks);

    // Candle.
    if state.sword_drawn && state.lockout == 0 {
        state.show_timer = SHOW_PINNED;
    }
    state.candle_visible = (state.show_timer != 0 || state.candle_mode != 0) && state.lockout == 0;
    if state.candle_visible && state.show_timer < SHOW_PINNED {
        state.show_timer = state.show_timer.saturating_sub(ticks);
    }

    // Select the overlay path before cycling; timer zero crossings keep their entry path.
    let bindings = state.bindings;
    let scan = |action: usize| cyrus_game::player::input::scan_key(bindings[action]);
    let pressed = |action: usize| scan(action).is_some_and(|k| keys.just_pressed(k));
    let held = |action: usize| scan(action).is_some_and(|k| keys.pressed(k));
    let fallback_overlay = if state.player_type == 1 {
        ActivationOverlay::Hidden
    } else {
        ActivationOverlay::Item {
            visible: true,
            cycle_allowed: !state.sword_drawn,
        }
    };
    let overlay = match (world, activation) {
        (Some(world), Some(inputs)) => {
            match world.activation_overlay(inputs, ticks, held(13) || held(14), hud.override_ticks)
            {
                Ok(overlay) => {
                    let attributes = world.actors()[inputs.player.index].attributes();
                    state.player_type = attributes[105];
                    // attr_health is attribute 3, clamped to 0..=100 by `set_health`.
                    state.set_health(attributes[3]);
                    overlay
                }
                Err(error) => {
                    warn_once!("HUD activation: {error:#}");
                    ActivationOverlay::Hidden
                }
            }
        }
        (Some(world), None) => {
            let pending = world.activation().prompt().is_some();
            world
                .ordinary_activation_overlay(
                    &script_hud,
                    ticks,
                    held(13) || held(14),
                    hud.override_ticks,
                )
                .unwrap_or_else(|error| {
                    if pending {
                        warn_once!("HUD ordinary activation: {error:#}");
                        ActivationOverlay::Hidden
                    } else {
                        fallback_overlay
                    }
                })
        }
        _ => fallback_overlay,
    };
    let cycle = if matches!(
        overlay,
        ActivationOverlay::Item {
            cycle_allowed: true,
            ..
        }
    ) {
        match (pressed(14), pressed(13)) {
            (true, false) => Some(false),
            (false, true) => Some(true),
            _ => None,
        }
    } else {
        None
    };
    let inventory = script_hud.inventory(cycle);
    state.selected = inventory.as_ref().and_then(|items| items.selected);
    let live_items: Vec<_> = hud
        .items
        .iter()
        .filter_map(|item| {
            let count = inventory.as_ref()?.counts.get(&item.id).copied()?;
            (count > 0).then(|| HudItem {
                count,
                ..item.clone()
            })
        })
        .collect();

    let item_path = matches!(&overlay, ActivationOverlay::Item { .. });
    let (shown_item, prompt) = match overlay {
        ActivationOverlay::Hidden | ActivationOverlay::Item { visible: false, .. } => (None, None),
        ActivationOverlay::Prompt(text) => (None, Some(text)),
        ActivationOverlay::Item { visible: true, .. } => (
            live_items.iter().find(|i| Some(i.id) == state.selected),
            None,
        ),
    };
    state.prompt = prompt.clone();
    state.count = shown_item.map(|i| i.count);

    // The original consumes use once per hold, independently of drawing its compass.
    let owns_compass = inventory
        .as_ref()
        .is_some_and(|items| items.counts.get(&0).is_some_and(|count| *count > 0));
    let attempt = if state.lockout == 1
        || on_edge
        || compass
            .suppressed
            .as_ref()
            .is_some_and(|suppressed| suppressed.0)
        || compass.console.as_ref().is_some_and(|console| console.open)
        // Readiness rejects after consuming use, even when it hides the item overlay.
        || (!item_path && activation.is_none_or(|inputs| inputs.item_system_ready))
    {
        cyrus_game::script::CompassUse::Blocked
    } else if state.selected == Some(0)
        && owns_compass
        && state.player_type != 1
        && compass_eligible
    {
        cyrus_game::script::CompassUse::Accepted
    } else {
        cyrus_game::script::CompassUse::Rejected
    };
    state.compass_on = script_hud.compass_input(held(7), attempt);

    // Compass inputs: the placed player's heading and the loaded world's WORLD.INI ID.
    state.heading = compass.player.single().map_or(0, |pose| pose.heading);
    state.world = compass
        .request
        .as_deref()
        .zip(compass.scenes.as_deref())
        .and_then(|(request, scenes)| scenes.get(&request.0))
        .map(|scene| scene.world);

    // Compass: owning item 0 and its display flag.
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
                    (
                        assets
                            .load_builder()
                            .with_settings(|settings: &mut ImageLoaderSettings| {
                                settings.sampler = ImageSampler::nearest();
                            })
                            .load(path),
                        hud.pickup,
                        item.size,
                    )
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
    if state.redraw_text || text != state.text || dialogue_text != state.dialogue_text {
        for entity in &text_layer {
            commands.entity(entity).despawn();
        }
        if !text.is_empty() || dialogue_text.is_some() {
            let image = images.add(text_image(hud, &text, dialogue_text.as_ref()));
            commands.entity(viewport).with_child((
                HudText,
                ImageNode::new(image),
                place([0, 0], [640, 480]),
                ZIndex(SPRITES.len() as i32),
            ));
        }
        state.redraw_text = false;
        state.text = text;
        state.dialogue_text = dialogue_text;
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
    fn show_timer_and_logbook_follow_their_triggers() {
        let mut state = HudState::new(&asset(0), None, HudPresentationAssets::default());
        state.set_health(80);
        assert_eq!(state.show_timer, SHOW_TICKS);
        state.show_timer = SHOW_PINNED;
        state.set_health(70);
        assert_eq!(state.show_timer, SHOW_PINNED, "a pinned timer is kept");
        state.show_candle();
        assert_eq!(
            state.show_timer, SHOW_TICKS,
            "explicit weapon reset replaces the pin"
        );
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
    fn dialogue_band_uses_formatted_breaks_without_advancing_or_wrapping() {
        let hud = asset(0);
        assert_eq!(dialogue_lines(&hud, "one")[0].1[1], 418);
        let two = dialogue_lines(&hud, "one\ntwo");
        assert_eq!(
            two.iter().map(|(_, at)| at[1]).collect::<Vec<_>>(),
            [418, 444]
        );
        let three = dialogue_lines(&hud, "one\ntwo\nthree");
        assert_eq!(
            three.iter().map(|(_, at)| at[1]).collect::<Vec<_>>(),
            [392, 418, 444]
        );
        assert_eq!(three[2].0, "three");
    }

    #[test]
    fn dialogue_record_selects_installed_player_or_npc_shadow_alpha() {
        let mut hud = asset(2);
        hud.font = cyrus_text::Font {
            start: 65,
            line_height: 26,
            palette: vec![[0, 0, 0], [63, 39, 1]],
            glyphs: vec![cyrus_text::Glyph {
                enabled: true,
                width: 1,
                height: 1,
                pixels: vec![1],
                ..default()
            }],
        };
        let (_, at) = dialogue_lines(&hud, "A").remove(0);
        let shadow = ((at[1] + 1) as usize * 640 + (at[0] + 1) as usize) * 4;
        let player = text_image(&hud, &[], Some(&(0, "A".into())));
        let npc = text_image(&hud, &[], Some(&(1, "A".into())));
        assert_eq!(player.data.as_ref().unwrap()[shadow + 3], 97);
        assert_eq!(npc.data.as_ref().unwrap()[shadow + 3], 199);
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
            labels: BTreeMap::new(),
            images: Vec::new(),
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
            inventory: BTreeMap::new(),
            start_items: vec![],
            items: vec![],
            selected: None,
            bindings: [0; 15],
            font: default(),
            text_alpha: [255, 199],
            player_text_alpha: [255, 97],
        }
    }

    fn prepare_script(app: &mut App, requests: &[(&str, &[i32])]) {
        use cyrus_assets::script::*;
        use cyrus_game::script::{ActivationActor, ScriptInputs, ScriptStage, ScriptWorld, Thread};
        use cyrus_script::program::*;
        let player = Actor {
            header_index: 0,
            code_origin: 100,
            instances: 1,
            namespace: Some("CYRUS".into()),
            instance_variables: vec![vec![]],
            location_offsets: vec![],
            attributes: BTreeMap::new(),
            combat_taunts: vec![],
            native_hook: None,
            program: Program {
                version: 2,
                entry: 0,
                variables: vec![],
                strings: vec![],
                steps: BTreeMap::from([(
                    0,
                    Step {
                        next: 1,
                        operation: Operation::End { target: 0 },
                    },
                )]),
            },
        };
        let mut lever = player.clone();
        lever.namespace = Some("LEVER".into());
        lever.code_origin = 0;
        lever.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: 1,
                            kind: CallKind::Function,
                            target: Object::Me,
                            function: "Activate".into(),
                            arguments: vec![Value::Integer {
                                value: i32::from_le_bytes(*b"#OPN"),
                            }],
                        },
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        let mut scripts = Scripts {
            version: 14,
            combat_attacks: vec![],
            in_combat_threshold: None,
            dialogue: cyrus_assets::script::DialogueSettings {
                menu_start: [35, 25],
                max_menu_items: 300,
                max_dialog_lines: 20,
                max_text_width: 500,
                menu_traverse_delay: 4,
                print_text: true,
                use_speech: true,
                max_distance: 1600,
                rtx_filename: "ENGLISH.RTX".into(),
            },
            player_distance_override: false,
            updates: cyrus_assets::script::UpdateSettings {
                task_system: true,
                animation_system: true,
                disable_slaves: false,
                disable_master_slaves: false,
            },
            clock: cyrus_assets::script::ClockSettings {
                normal_frame_rate: 12,
                min_frame_rate: 6,
                max_frame_rate: 300,
                use_smooth_fps: true,
                use_smooth_divisor: true,
            },
            actor_pool_capacity: 10,
            content_id: String::new(),
            world: 1,
            definition: Definition {
                functions: vec![
                    Function {
                        name: "NullFunction".into(),
                        param_count: 0,
                        kind: CallKind::Function,
                    },
                    Function {
                        name: "Activate".into(),
                        param_count: 1,
                        kind: CallKind::Function,
                    },
                ],
                flags: vec![],
                references: vec![],
                attributes: vec![],
            },
            actors: BTreeMap::from([("CYRUS".into(), player), ("LEVER".into(), lever)]),
            instances: vec![Instance {
                object_id: 1,
                actor: "LEVER".into(),
                object_type: 0,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                map_position: [0; 3],
                angles: [0; 3],
            }],
            unsupported: BTreeMap::new(),
        };
        if !requests.is_empty() {
            let mut steps = BTreeMap::new();
            for (index, (function, arguments)) in requests.iter().enumerate() {
                let function_id = scripts.definition.functions.len() as u16;
                scripts.definition.functions.push(Function {
                    name: (*function).into(),
                    param_count: arguments.len().try_into().unwrap(),
                    kind: CallKind::Function,
                });
                steps.insert(
                    index as u32,
                    Step {
                        next: index as u32 + 1,
                        operation: Operation::Call {
                            call: Call {
                                function_id,
                                kind: CallKind::Function,
                                target: Object::Me,
                                function: (*function).into(),
                                arguments: arguments
                                    .iter()
                                    .map(|value| Value::Integer { value: *value })
                                    .collect(),
                            },
                        },
                    },
                );
            }
            let end = requests.len() as u32;
            steps.insert(
                end,
                Step {
                    next: end + 1,
                    operation: Operation::End { target: 0 },
                },
            );
            scripts.actors.get_mut("LEVER").unwrap().program.steps = steps;
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let state = ActivationActor {
            state: 0,
            on_rope: false,
            water_state: 0,
            torch_burning: 0,
            weapon_assigned: false,
            weapon_drawn: false,
            animation: Some(cyrus_game::script::ActivationAnimation { group: Some(0) }),
        };
        let inputs = ActivationInputs {
            player: world.id(1).unwrap(),
            actors: BTreeMap::from([(0, state), (1, state)]),
            labels: BTreeMap::from([("#opn".into(), "Open".into())]),
            lockout: 0,
            dialogue_open: false,
            lineup_result: 0,
            use_held: false,
            item_system_ready: true,
            shell_execution: false,
            render_disabled: false,
        };
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    activation: Some(&inputs),
                    hud: app.world().get_resource::<cyrus_game::script::ScriptHud>(),
                    ..Default::default()
                },
                None,
                None,
                10,
            )
            .unwrap();
        let mut actors = ScriptActors::default();
        actors.current = Some(world);
        app.insert_resource(actors).insert_resource(inputs);
    }

    #[test]
    fn script_hud_renders_powerups_and_new_logbook_frame_zero_with_lockout_rules() {
        struct Store;
        impl cyrus_game::preferences::PreferenceStore for Store {
            fn load(
                &self,
                _: cyrus_game::preferences::PreferenceKey,
            ) -> anyhow::Result<Option<String>> {
                Ok(None)
            }
            fn save(
                &self,
                _: cyrus_game::preferences::PreferenceKey,
                _: &str,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<HudAsset>()
            .init_resource::<cyrus_game::script::ScriptHud>()
            .insert_resource(cyrus_game::preferences::Preferences::new(Store))
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
            ))
            .add_systems(Update, (configure_scripts, update).chain());
        let mut data = asset(0);
        data.labels.insert("TEST".into(), "New entry".into());
        data.labels.insert("MORE".into(), "Another entry".into());
        let handle = app.world_mut().resource_mut::<Assets<HudAsset>>().add(data);
        app.insert_resource(HudHandle(handle));
        app.world_mut().spawn(HudViewport);
        app.update();
        prepare_script(
            &mut app,
            &[
                ("ShowBitmap", &[0, 0]),
                ("AddLog", &[i32::from_le_bytes(*b"TEST")]),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<HudState>().powerups,
            [Some((0, 0)), None]
        );
        assert_eq!(app.world().resource::<HudState>().logbook_frame, Some(0));
        let visible = |app: &mut App, expected| {
            app.world_mut()
                .query::<(&HudSprite, &Visibility)>()
                .iter(app.world())
                .find(|(sprite, _)| **sprite == expected)
                .map(|(_, visibility)| *visibility)
                .unwrap()
        };
        assert_eq!(
            visible(&mut app, HudSprite::Powerup(0)),
            Visibility::Inherited
        );
        assert_eq!(visible(&mut app, HudSprite::Logbook), Visibility::Inherited);
        prepare_script(&mut app, &[("AddLog", &[i32::from_le_bytes(*b"MORE")])]);
        app.world_mut().resource_mut::<ActivationInputs>().lockout = 1;
        app.update();
        assert_eq!(
            app.world().resource::<HudState>().logbook_frame,
            Some(1),
            "new entry does not restart active icon"
        );
        assert_eq!(visible(&mut app, HudSprite::Powerup(0)), Visibility::Hidden);
        assert_eq!(visible(&mut app, HudSprite::Logbook), Visibility::Inherited);
        for _ in 0..31 {
            app.update();
        }
        assert_eq!(app.world().resource::<HudState>().logbook_frame, None);
        assert_eq!(visible(&mut app, HudSprite::Logbook), Visibility::Hidden);
    }

    fn inventory_items(counts: &[(u16, u32)]) -> BTreeMap<u16, cyrus_assets::hud::InventoryItem> {
        counts
            .iter()
            .map(|&(id, count)| {
                (
                    id,
                    cyrus_assets::hud::InventoryItem {
                        count,
                        total: 10,
                        player_max: 10,
                        hidden: false,
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    fn compass_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<cyrus_game::script::ScriptHud>()
            .init_asset::<HudAsset>()
            .init_resource::<cyrus_game::preferences::Preferences>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (configure_scripts, update).chain());
        let hud = app
            .world_mut()
            .resource_mut::<Assets<HudAsset>>()
            .add(HudAsset {
                inventory: inventory_items(&[(0, 1), (1, 1)]),
                items: vec![HudItem {
                    id: 0,
                    bitmap: 1,
                    count: 1,
                    size: [1, 1],
                }],
                selected: Some(0),
                ..asset(0)
            });
        app.insert_resource(HudHandle(hud));
        app.world_mut().spawn(HudViewport);
        app.world_mut().spawn((
            Player,
            PlayerPose {
                position: [0; 3],
                heading: 1024,
            },
        ));
        app.update();
        prepare_script(&mut app, &[]);
        app.world_mut().resource_mut::<HudState>().bindings[7] = 18;
        app
    }

    #[test]
    fn candle_uses_current_actor_health_on_the_first_transition_update() {
        let mut app = compass_app();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / 60.0),
        ));
        {
            let mut state = app.world_mut().resource_mut::<HudState>();
            state.health = 100;
            state.candle.shown = Some(0);
            state.elapsed = 0.0;
        }
        // The prepared actor has health zero; the number and snuff must change together.
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.health, 0);
        assert_eq!(state.candle.frame, 66);
        app.update();
        assert_eq!(app.world().resource::<HudState>().candle.frame, 67);
    }

    #[test]
    fn hud_timers_follow_simulation_ticks_and_hold_in_the_console() {
        let mut app = compass_app();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ));
        let mut clock =
            cyrus_game::clock::SimulationClock::new(cyrus_assets::script::ClockSettings {
                normal_frame_rate: 12,
                min_frame_rate: 6,
                max_frame_rate: 300,
                use_smooth_fps: true,
                use_smooth_divisor: true,
            })
            .unwrap();
        assert_eq!(clock.advance(0).nominal_ticks, 1);
        app.insert_resource(clock);
        {
            let mut state = app.world_mut().resource_mut::<HudState>();
            state.health = 100;
            state.notices = [24, 24];
        }
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.show_timer, 24);
        assert_eq!(state.notices, [23, 23]);
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.show_timer, 24, "an old clock step is not replayed");
        assert_eq!(state.notices, [23, 23]);
        let candle = state.candle;
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs(1),
        ));
        app.init_resource::<cyrus_console::Console>();
        app.world_mut()
            .resource_mut::<cyrus_console::Console>()
            .open = true;
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.candle, candle);
        assert_eq!(state.show_timer, 24);
        assert_eq!(state.notices, [23, 23]);
    }

    #[test]
    fn gameplay_toggles_survive_hud_recreation_and_drive_its_projection() {
        use cyrus_game::player::input::PlayerToggles;
        let mut app = compass_app();
        assert!(!app.world().resource::<PlayerToggles>().walk_mode);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Tab);
        app.update();
        assert!(app.world().resource::<PlayerToggles>().walk_mode);
        assert!(app.world().resource::<HudState>().walk_mode);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().remove_resource::<HudState>();
        app.update();
        assert!(app.world().resource::<PlayerToggles>().walk_mode);
        assert!(app.world().resource::<HudState>().walk_mode);
        app.world_mut().resource_mut::<PlayerToggles>().walk_mode = false;
        app.world_mut().resource_mut::<PlayerToggles>().auto_defend = true;
        app.update();
        let state = app.world().resource::<HudState>();
        assert!(!state.walk_mode);
        assert!(state.auto_defend);
    }

    fn use_key(app: &mut App, held: bool) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        if held {
            keys.press(KeyCode::KeyE);
        } else {
            keys.release(KeyCode::KeyE);
        }
        app.update();
    }

    #[test]
    fn compass_uses_owned_item_once_per_hold_and_retains_session_state() {
        let mut app = compass_app();
        app.update();
        assert!(!app.world().resource::<HudState>().compass_on);
        use_key(&mut app, true);
        let state = app.world().resource::<HudState>();
        assert_eq!((state.heading, state.compass_frame), (1024, Some(16)));
        assert_eq!(
            app.world()
                .resource::<cyrus_game::script::ScriptHud>()
                .inspect()["compass_on"],
            true
        );
        app.update();
        assert!(app.world().resource::<HudState>().compass_on);
        let (node, visibility) = app
            .world_mut()
            .query::<(&HudSprite, &Node, &Visibility)>()
            .iter(app.world())
            .find(|(sprite, _, _)| **sprite == HudSprite::Compass)
            .map(|(_, node, visibility)| (node.clone(), *visibility))
            .unwrap();
        assert_eq!(visibility, Visibility::Inherited);
        assert_eq!(
            (node.left, node.top, node.width, node.height),
            (
                Val::Percent(85.3125),
                Val::Percent(82.5),
                Val::Percent(11.40625),
                Val::Percent(13.541666)
            )
        );
        prepare_script(&mut app, &[("LockoutPlayer", &[1])]);
        app.update();
        assert_eq!(app.world().resource::<HudState>().compass_frame, None);
        assert!(app.world().resource::<HudState>().compass_on);
        prepare_script(&mut app, &[("LockoutPlayer", &[0])]);
        app.update();
        assert_eq!(app.world().resource::<HudState>().compass_frame, Some(16));
        use_key(&mut app, false);
        app.world_mut().remove_resource::<HudState>();
        prepare_script(&mut app, &[]);
        app.update();
        app.update();
        assert!(
            app.world().resource::<HudState>().compass_on,
            "world/HUD recreation retains the flag"
        );
        app.world_mut().resource_mut::<HudState>().bindings[7] = 18;
        use_key(&mut app, true);
        assert!(!app.world().resource::<HudState>().compass_on);
        assert_eq!(app.world().resource::<HudState>().compass_frame, None);
    }

    #[test]
    fn script_acquired_compass_uses_authoritative_player_without_inspection_inputs() {
        let mut app = compass_app();
        // New session starts without compass ownership, with the installed sabre selected.
        let handle = app.world().resource::<HudHandle>().0.clone();
        {
            let mut assets = app.world_mut().resource_mut::<Assets<HudAsset>>();
            let mut hud = assets.get_mut(&handle).unwrap();
            hud.inventory.get_mut(&0).unwrap().count = 0;
            hud.inventory.get_mut(&0).unwrap().player_max = 1;
            hud.selected = Some(1);
        }
        app.insert_resource(cyrus_game::script::ScriptHud::default());
        app.update();
        app.world_mut().remove_resource::<ActivationInputs>();
        use_key(&mut app, true);
        assert!(!app.world().resource::<HudState>().compass_on);
        use_key(&mut app, false);
        prepare_script(&mut app, &[("AddItem", &[0, 8]), ("SelectItem", &[0])]);
        app.world_mut().remove_resource::<ActivationInputs>();
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!((state.selected, state.count), (Some(0), Some(1)));
        assert!(
            !state.compass_on,
            "acquisition does not activate the compass"
        );
        use_key(&mut app, true);
        assert!(app.world().resource::<HudState>().compass_on);
        assert_eq!(app.world().resource::<HudState>().compass_frame, Some(16));
        app.update();
        assert!(
            app.world().resource::<HudState>().compass_on,
            "held use does not repeat"
        );
        use_key(&mut app, false);
        // Surface changes from the actor host, without a separate activation resource.
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            let (id, _) = world.player_activation_state().unwrap();
            world.set_actor_surface(id, 2).unwrap();
        }
        use_key(&mut app, true);
        assert!(
            app.world().resource::<HudState>().compass_on,
            "deep water rejects use"
        );
        use_key(&mut app, false);
        prepare_script(&mut app, &[("LockoutPlayer", &[0])]);
        app.world_mut().remove_resource::<ActivationInputs>();
        app.world_mut().remove_resource::<HudState>();
        app.update();
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!((state.selected, state.count), (Some(0), Some(1)));
        assert!(
            state.compass_on,
            "world and HUD replacement retain live inventory"
        );
        app.world_mut().resource_mut::<HudState>().bindings[7] = 18;
        use_key(&mut app, true);
        assert!(!app.world().resource::<HudState>().compass_on);
    }

    #[test]
    fn compass_requires_known_actor_state_ownership_selection_and_current_binding() {
        let mut app = compass_app();
        let inputs = app
            .world_mut()
            .remove_resource::<ActivationInputs>()
            .unwrap();
        let world = app
            .world_mut()
            .resource_mut::<ScriptActors>()
            .current
            .take();
        use_key(&mut app, true);
        assert!(
            !app.world().resource::<HudState>().compass_on,
            "unknown gates reject use"
        );
        app.world_mut().resource_mut::<ScriptActors>().current = world;
        app.insert_resource(inputs);
        app.update();
        assert!(
            !app.world().resource::<HudState>().compass_on,
            "rejected hold stays consumed"
        );
        use_key(&mut app, false);
        app.world()
            .resource::<cyrus_game::script::ScriptHud>()
            .select_item(1)
            .unwrap();
        use_key(&mut app, true);
        assert!(!app.world().resource::<HudState>().compass_on);
        use_key(&mut app, false);
        app.world()
            .resource::<cyrus_game::script::ScriptHud>()
            .select_item(0)
            .unwrap();
        // Re-exported initial metadata cannot replace live ownership.
        let handle = app.world().resource::<HudHandle>().0.clone();
        app.world_mut()
            .resource_mut::<Assets<HudAsset>>()
            .get_mut(&handle)
            .unwrap()
            .items[0]
            .count = 0;
        app.world_mut().resource_mut::<HudState>().bindings[7] = 0x21; // F
        use_key(&mut app, true);
        assert!(
            !app.world().resource::<HudState>().compass_on,
            "old binding cannot activate"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);
        app.update();
        assert!(app.world().resource::<HudState>().compass_on);
        app.world_mut().resource_mut::<HudState>().player_type = 1;
        app.world_mut().remove_resource::<ActivationInputs>();
        app.world_mut().resource_mut::<ScriptActors>().current = None;
        app.update();
        assert_eq!(app.world().resource::<HudState>().compass_frame, None);
        assert!(
            app.world().resource::<HudState>().compass_on,
            "hiding does not reset the flag"
        );
    }

    #[test]
    fn compass_early_blocks_defer_use_but_late_actor_gates_consume_it() {
        let mut app = compass_app();
        app.init_resource::<cyrus_console::Console>();
        app.world_mut()
            .resource_mut::<cyrus_console::Console>()
            .open = true;
        use_key(&mut app, true);
        assert!(
            !app.world().resource::<HudState>().compass_on,
            "console owns input"
        );
        app.world_mut()
            .resource_mut::<cyrus_console::Console>()
            .open = false;
        app.insert_resource(cyrus_game::player::input::InputSuppressed(true));
        app.update();
        assert!(!app.world().resource::<HudState>().compass_on);
        app.world_mut()
            .resource_mut::<cyrus_game::player::input::InputSuppressed>()
            .0 = false;
        app.world_mut().resource_mut::<ActivationInputs>().lockout = 1;
        app.update();
        assert!(!app.world().resource::<HudState>().compass_on);
        app.world_mut().resource_mut::<ActivationInputs>().lockout = 0;
        app.world_mut()
            .resource_mut::<ActivationInputs>()
            .actors
            .get_mut(&1)
            .unwrap()
            .state = 10;
        app.update();
        assert!(!app.world().resource::<HudState>().compass_on);
        app.world_mut()
            .resource_mut::<ActivationInputs>()
            .actors
            .get_mut(&1)
            .unwrap()
            .state = 0;
        app.update();
        assert!(
            app.world().resource::<HudState>().compass_on,
            "early blocks leave use available"
        );
        use_key(&mut app, false);
        for gate in 0..10 {
            let baseline = {
                let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
                let baseline = inputs.actors[&1];
                match gate {
                    0 => inputs.item_system_ready = false,
                    1 => inputs.shell_execution = true,
                    2 => inputs.actors.get_mut(&1).unwrap().water_state = 2,
                    3 => inputs.actors.get_mut(&1).unwrap().water_state = 4,
                    4 => {
                        let actor = inputs.actors.get_mut(&1).unwrap();
                        actor.weapon_assigned = true;
                        actor.weapon_drawn = true;
                    }
                    5 => inputs.actors.get_mut(&1).unwrap().state = 33,
                    6 => inputs.actors.get_mut(&1).unwrap().state = 34,
                    7 | 8 => {
                        inputs.actors.get_mut(&1).unwrap().animation =
                            Some(cyrus_game::script::ActivationAnimation {
                                group: Some(if gate == 7 { 149 } else { 150 }),
                            })
                    }
                    _ => inputs.actors.get_mut(&1).unwrap().torch_burning = 1,
                }
                baseline
            };
            use_key(&mut app, true);
            assert!(
                app.world().resource::<HudState>().compass_on,
                "gate {gate} rejected"
            );
            {
                let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
                inputs.item_system_ready = true;
                inputs.shell_execution = false;
                inputs.actors.insert(1, baseline);
            }
            app.update();
            assert!(
                app.world().resource::<HudState>().compass_on,
                "gate {gate} consumed hold"
            );
            use_key(&mut app, false);
        }
        use_key(&mut app, true);
        assert!(!app.world().resource::<HudState>().compass_on);
    }

    #[test]
    fn ordinary_prompt_renders_without_an_inspection_resource() {
        let mut app = compass_app();
        app.world_mut().remove_resource::<ActivationInputs>();
        let handle = app.world().resource::<HudHandle>().0.clone();
        app.world_mut()
            .resource_mut::<Assets<HudAsset>>()
            .get_mut(&handle)
            .unwrap()
            .labels
            .insert("#opn".into(), "Installed prompt".into());
        app.insert_resource(cyrus_game::script::ScriptHud::default());
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            actors.begin_new_game();
            let world = actors.current.as_mut().unwrap();
            let animation = cyrus_assets::animation::Animations {
                version: 2,
                world: 1,
                sections: BTreeMap::new(),
                models: BTreeMap::new(),
                unsupported: BTreeMap::new(),
                metadata: serde_json::json!({
                    "script_content_id":world.inspect(0, 0)["content_id"],
                    "actors":[{"script_name":"CYRUS","animation_groups":[{
                        "group_index":0,"anim_type":0,"anim_id":1,"commands":[
                            {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]
                }),
            };
            world
                .initialize_player_animation(world.id(1).unwrap(), &animation, 0, &mut |_| Ok(()))
                .unwrap();
        }
        app.update();
        assert_eq!(
            app.world().resource::<HudState>().prompt.as_deref(),
            Some("Open")
        );
        app.world_mut()
            .resource_scope(|world, mut actors: Mut<ScriptActors>| {
                let logical = actors.current.as_mut().unwrap();
                logical
                    .advance(
                        logical.id(0).unwrap(),
                        cyrus_game::script::ScriptStage::Body(cyrus_game::script::Thread::Main),
                        cyrus_game::script::ScriptInputs {
                            hud: world.get_resource::<cyrus_game::script::ScriptHud>(),
                            activation_use_held: Some(false),
                            shell_execution: Some(false),
                            nominal_ticks: Some(0),
                            ..default()
                        },
                        None,
                        None,
                        10,
                    )
                    .unwrap();
            });
        app.update();
        assert_eq!(
            app.world().resource::<HudState>().prompt.as_deref(),
            Some("Installed prompt")
        );
        assert!(app.world().get_resource::<ActivationInputs>().is_none());
    }

    #[test]
    fn item_cycle_waits_for_prompt_draw_then_updates_equal_count_icon() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<cyrus_game::script::ScriptHud>()
            .init_asset::<HudAsset>()
            .init_resource::<cyrus_game::preferences::Preferences>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (configure_scripts, update).chain());
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
                inventory: inventory_items(&[(1, 1), (2, 1)]),
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
        prepare_script(&mut app, &[]);
        let inputs = app
            .world_mut()
            .remove_resource::<ActivationInputs>()
            .unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .activation()
                .prompt(),
            Some("Open")
        );
        assert_eq!(app.world().resource::<HudState>().prompt, None);
        app.insert_resource(inputs);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Period);
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.selected, Some(1));
        assert_eq!(state.prompt.as_deref(), Some("Open"));
        assert_eq!(state.text, vec![("Open".into(), [12, 436])]);
        assert!(
            app.world_mut()
                .query::<(&HudSprite, &Visibility)>()
                .iter(app.world())
                .any(|(sprite, visibility)| *sprite == HudSprite::Item
                    && *visibility == Visibility::Hidden)
        );
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

        {
            let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
            let player_index = inputs.player.index;
            let player = inputs.actors.get_mut(&player_index).unwrap();
            player.weapon_assigned = true;
            player.weapon_drawn = true;
            player.animation = Some(cyrus_game::script::ActivationAnimation { group: None });
        }
        app.update();
        assert_eq!(app.world().resource::<HudState>().selected, Some(2));
        {
            let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
            let player_index = inputs.player.index;
            inputs.actors.get_mut(&player_index).unwrap().animation = None;
        }
        app.update();
        assert_eq!(app.world().resource::<HudState>().selected, Some(1));
    }
    #[test]
    fn presentation_snapshot_retains_next_candle_icon_and_fractional_frame() -> anyhow::Result<()> {
        let asset = asset(2);
        let frames = HudPresentationAssets {
            candle: vec![Handle::default(); 81],
            compass: vec![Handle::default(); 32],
            logbook: vec![Handle::default(); 32],
            powerup: vec![Handle::default(); 2],
        };
        let prepared = HudState::new(&asset, None, frames);
        let mut live = prepared.clone();
        live.set_health(47);
        live.logbook_entry();
        for _ in 0..9 {
            live.advance_frame();
        }
        live.elapsed = 0.01;
        live.notices = [7, 13];
        live.bindings[0] = 72;
        live.text = vec![("retained prompt".into(), [17, 20])];
        live.dialogue_text = Some((3, "retained dialogue".into()));
        let saved = serde_json::to_value(live.snapshot(&asset)?)?;
        let mut restored = HudState::from_snapshot(
            &asset,
            &prepared.frames,
            serde_json::from_value(saved.clone())?,
        )?;
        assert_eq!(serde_json::to_value(restored.snapshot(&asset)?)?, saved);
        for _ in 0..26 {
            live.advance_frame();
            restored.advance_frame();
            assert_eq!(restored.candle, live.candle);
            assert_eq!(restored.logbook_frame, live.logbook_frame);
        }
        for (pointer, value) in [
            ("/version", serde_json::json!(2)),
            ("/elapsed_bits", serde_json::json!(f32::NAN.to_bits())),
            ("/state/logbook_frame", serde_json::json!(32)),
            ("/state/candle/frame", serde_json::json!(81)),
        ] {
            let mut invalid = saved.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(
                HudState::from_snapshot(&asset, &prepared.frames, serde_json::from_value(invalid)?)
                    .is_err()
            );
        }
        let mut changed = asset;
        changed.logbook[0] += 1;
        assert!(
            HudState::from_snapshot(&changed, &prepared.frames, serde_json::from_value(saved)?)
                .is_err()
        );
        Ok(())
    }
}
