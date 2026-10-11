//! In-game overlay from installed HUD data: the health candle, its number, and the
//! active item with its count. Rules follow redguard-preservation's HUD documentation.
use crate::AppState;
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use serde::{Deserialize, Serialize};

/// Shows the exported `hud/overlay.hud` over gameplay, fitted like the original 640×480 viewport.
pub struct HudPlugin;

/// Installed HUD data, exported natively from SYSTEM.INI, ITEM.INI, KEYS.INI, GXA and FNT files.
#[derive(Asset, TypePath, Serialize, Deserialize)]
pub(crate) struct HudAsset {
    /// SYSTEM.INI `[screen] candle_mode`: 0 shows the candle only after a show trigger,
    /// 1 always, 2 always with the health number.
    pub candle_mode: u8,
    /// Top-left of the candle in 640×480 screen pixels, SYSTEM.INI `candle_xco/yco`.
    pub candle: [i32; 2],
    pub candle_size: [u16; 2],
    /// Top-left of the active item icon, SYSTEM.INI `pickup_xco/yco`.
    pub pickup: [i32; 2],
    /// Owned, unhidden starting items in item ID order.
    pub items: Vec<HudItem>,
    /// ITEM.INI `start_item_select`, if owned.
    pub selected: Option<u16>,
    /// Installed KEYS.INI scan codes for menu actions 710 through 724.
    pub bindings: [u8; 15],
    /// `fonts/REDSEL.FNT`; HUD text is drawn like an NPC subtitle line.
    pub font: cyrus_text::Font,
    /// SYSTEM.INI `[3dfx] font_sel` as main and shadow alpha.
    pub text_alpha: [u8; 2],
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct HudItem {
    pub id: u16,
    /// ITEM.INI `bitmap[id]`: the `hud/item-NN.png` frame.
    pub bitmap: u16,
    pub count: u32,
    pub size: [u16; 2],
}

/// The player's fixed maximum and spawn health; no INI key sets it.
const MAX_HEALTH: u8 = 100;

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

/// Live overlay state, reported by the control server.
#[derive(Resource, Serialize, Debug)]
pub struct HudState {
    /// Player health, 0..=100. The player spawns at the maximum; nothing changes it yet.
    pub health: u8,
    pub candle_visible: bool,
    candle: Candle,
    /// Selected item ID, or `None` when nothing is owned.
    pub selected: Option<u16>,
    pub count: Option<u32>,
    #[serde(skip)]
    elapsed: f32,
    /// Edited bindings when present, else the installed ones; read on entering a world.
    #[serde(skip)]
    bindings: [u8; 15],
    /// Every SNUFF frame, held so swapping frames never unloads and reloads an image.
    #[serde(skip)]
    candle_frames: Vec<Handle<Image>>,
    /// Text lines and pen positions currently drawn on the text layer.
    #[serde(skip)]
    text: Vec<(String, [i32; 2])>,
}

#[derive(Resource)]
struct HudHandle(Handle<HudAsset>);

#[derive(Component)]
struct HudContent;

#[derive(Component)]
struct HudViewport;

#[derive(Component)]
struct CandleImage;

#[derive(Component)]
enum HudLayer {
    Item,
    Text,
}

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
        app.init_asset::<HudAsset>()
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

/// The health number below the candle (`candle_mode` 2) and the count beside an item
/// owned more than once, as lines with their pen positions in 640×480 pixels.
fn text_lines(hud: &HudAsset, health: u8, item: Option<&HudItem>) -> Vec<(String, [i32; 2])> {
    let mut lines = Vec::new();
    if hud.candle_mode == 2 {
        let text = health.to_string();
        let x = hud.candle[0] + i32::from(hud.candle_size[0]) / 2 - 5;
        let at = [
            x - hud.font.width(&text) / 2,
            hud.candle[1] + i32::from(hud.candle_size[1]),
        ];
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
    mut commands: Commands,
    assets: Res<AssetServer>,
    handle: Option<Res<HudHandle>>,
    data: Res<Assets<HudAsset>>,
    state: Option<ResMut<HudState>>,
    viewport: Query<Entity, With<HudViewport>>,
    mut candle: Query<&mut ImageNode, With<CandleImage>>,
    layers: Query<(Entity, &HudLayer)>,
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
        let selected = hud.selected.or_else(|| hud.items.first().map(|i| i.id));
        commands.insert_resource(HudState {
            health: MAX_HEALTH,
            candle_visible: hud.candle_mode != 0,
            candle: Candle::default(),
            selected,
            count: None,
            elapsed: 0.0,
            bindings: crate::startup::MenuPreferences::load()
                .ok()
                .flatten()
                .map_or(hud.bindings, |p| p.bindings),
            // The exporter validates SNUFF.GXA's 81 frames.
            candle_frames: (0..81)
                .map(|f| assets.load(format!("cache://hud/candle-{f:02}.png")))
                .collect(),
            text: Vec::new(),
        });
        if hud.candle_mode != 0 {
            commands.entity(viewport).with_children(|viewport| {
                viewport.spawn((
                    CandleImage,
                    ImageNode::default(),
                    place(hud.candle, hud.candle_size),
                ));
            });
        }
        return;
    };

    // Preservation HUD facts: Glide swaps at 60 Hz; keep that animation rate independent of rendering.
    state.elapsed += time.delta_secs();
    let period = 1.0 / 60.0;
    while state.elapsed >= period {
        state.elapsed -= period;
        let health = state.health;
        state.candle.advance(health);
    }
    if let Ok(mut image) = candle.single_mut() {
        image.image = state.candle_frames[usize::from(state.candle.frame)].clone();
    }

    let previous_selection = state.selected;
    let bindings = state.bindings;
    // Menu actions 723 and 724: next and previous item.
    let pressed = |action: usize| {
        crate::player::input::scan_key(bindings[action]).is_some_and(|k| keys.just_pressed(k))
    };
    let position = hud.items.iter().position(|i| Some(i.id) == state.selected);
    let step = match (pressed(14), pressed(13)) {
        (true, false) => hud.items.len().checked_sub(1),
        (false, true) => Some(1),
        _ => None,
    };
    if let (Some(position), Some(step)) = (position, step) {
        state.selected = Some(hud.items[(position + step) % hud.items.len()].id);
    }
    let item = hud.items.iter().find(|i| Some(i.id) == state.selected);
    let count = item.map(|i| i.count);
    let text = text_lines(hud, state.health, item);
    if text != state.text {
        for (entity, layer) in &layers {
            if matches!(layer, HudLayer::Text) {
                commands.entity(entity).despawn();
            }
        }
        if !text.is_empty() {
            let image = images.add(text_image(hud, &text));
            // Above the item icon, which the count overlaps.
            commands.entity(viewport).with_child((
                HudLayer::Text,
                ImageNode::new(image),
                place([0, 0], [640, 480]),
                ZIndex(1),
            ));
        }
        state.text = text;
    }
    if state.selected == previous_selection
        && state.count == count
        && layers.iter().any(|(_, l)| matches!(l, HudLayer::Item))
    {
        return;
    }
    state.count = count;
    for (entity, layer) in &layers {
        if matches!(layer, HudLayer::Item) {
            commands.entity(entity).despawn();
        }
    }
    let Some(item) = item else {
        return;
    };
    commands.entity(viewport).with_children(|viewport| {
        viewport
            .spawn((HudLayer::Item, place([0, 0], [640, 480])))
            .with_children(|layer| {
                layer.spawn((
                    ImageNode::new(assets.load(format!("cache://hud/item-{:02}.png", item.bitmap))),
                    place(hud.pickup, item.size),
                ));
            });
    });
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
            text_lines(&hud, 100, Some(&item)),
            [("100".into(), [17, 106]), ("53".into(), [20, 394])]
        );
        hud.candle_mode = 1;
        assert!(text_lines(&hud, 100, Some(&HudItem { count: 1, ..item })).is_empty());
    }

    fn asset(candle_mode: u8) -> HudAsset {
        HudAsset {
            candle_mode,
            candle: [12, 8],
            candle_size: [50, 98],
            pickup: [12, 386],
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
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, update);
        let mut bindings = [0; 15];
        bindings[13] = 0x34;
        let hud = app
            .world_mut()
            .resource_mut::<Assets<HudAsset>>()
            .add(HudAsset {
                candle_mode: 0,
                candle: [0, 0],
                candle_size: [1, 1],
                pickup: [0, 0],
                items: vec![
                    HudItem {
                        id: 1,
                        bitmap: 1,
                        count: 1,
                        size: [1, 1],
                    },
                    HudItem {
                        id: 2,
                        bitmap: 2,
                        count: 1,
                        size: [1, 1],
                    },
                ],
                selected: Some(1),
                bindings,
                font: default(),
                text_alpha: [255, 199],
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
        let images: Vec<_> = app
            .world_mut()
            .query::<&ImageNode>()
            .iter(app.world())
            .map(|node| node.image.clone())
            .collect();
        assert_eq!(images, [expected]);
    }
}
