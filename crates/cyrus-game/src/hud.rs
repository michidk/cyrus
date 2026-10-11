//! In-game overlay from installed HUD data: the health candle, its number, and the
//! active item with its count. Rules follow redguard-preservation's HUD documentation.
use crate::AppState;
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
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
    pub font: HudFont,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct HudItem {
    pub id: u16,
    /// ITEM.INI `bitmap[id]`: the `hud/item-NN.png` frame.
    pub bitmap: u16,
    pub count: u32,
    pub size: [u16; 2],
}

/// REDSEL.FNT metrics; glyph images are `hud/glyph-CCC.png` by character code.
#[derive(Serialize, Deserialize)]
pub(crate) struct HudFont {
    pub start: u16,
    /// `None` for disabled glyphs, which advance 0 and are not drawn.
    pub glyphs: Vec<Option<HudGlyph>>,
    /// SYSTEM.INI `[3dfx] font_sel`: shadow value A and main alpha B.
    pub font_sel: [u8; 2],
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub(crate) struct HudGlyph {
    pub width: u16,
    pub height: u16,
    pub offset: [i16; 2],
}

impl HudFont {
    fn glyph(&self, ch: u8) -> Option<HudGlyph> {
        let index = usize::from(ch).checked_sub(usize::from(self.start))?;
        *self.glyphs.get(index)?
    }

    fn advance(&self, ch: u8) -> i32 {
        let Some(glyph) = self.glyph(ch) else {
            return 0;
        };
        let extra = if ch == b' ' {
            self.glyph(b'!').map_or(0, |g| i32::from(g.width))
        } else {
            0
        };
        i32::from(glyph.width) + 1 + extra
    }

    fn width(&self, text: &str) -> i32 {
        text.bytes().map(|ch| self.advance(ch)).sum()
    }

    /// Glyph codes and top-left screen positions for `text` with its pen at `(x, y)`.
    // shortcut: no wrap at x = 640; HUD numbers are short. Upgrade with activation prompts.
    fn layout(&self, text: &str, mut x: i32, y: i32) -> Vec<(u8, [i32; 2], HudGlyph)> {
        let mut placed = Vec::new();
        for ch in text.bytes() {
            if let Some(glyph) = self.glyph(ch) {
                placed.push((
                    ch,
                    [
                        x + i32::from(glyph.offset[0]),
                        y + i32::from(glyph.offset[1]),
                    ],
                    glyph,
                ));
            }
            x += self.advance(ch);
        }
        placed
    }
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
    Health,
    Item,
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

/// A full-viewport layer holding `text` drawn like an NPC subtitle: shadow at (1, 1), then main.
fn spawn_text(
    commands: &mut Commands,
    parent: Entity,
    layer: HudLayer,
    assets: &AssetServer,
    font: &HudFont,
    text: &str,
    at: [i32; 2],
) {
    let [shadow, main] = [
        u32::from(font.font_sel[0]) * 200 / 256,
        u32::from(font.font_sel[1]),
    ];
    let glyphs = font.layout(text, at[0], at[1]);
    commands.entity(parent).with_children(|viewport| {
        viewport
            .spawn((layer, place([0, 0], [640, 480])))
            .with_children(|layer| {
                for (offset, tint) in [
                    (1, Color::srgba(0.0, 0.0, 0.0, shadow as f32 / 255.0)),
                    (0, Color::srgba(1.0, 1.0, 1.0, main as f32 / 255.0)),
                ] {
                    for (ch, [x, y], glyph) in &glyphs {
                        layer.spawn((
                            ImageNode::new(assets.load(format!("cache://hud/glyph-{ch:03}.png")))
                                .with_color(tint),
                            place([x + offset, y + offset], [glyph.width, glyph.height]),
                        ));
                    }
                }
            });
    });
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
        if hud.candle_mode == 2 {
            let text = &MAX_HEALTH.to_string();
            let x = hud.candle[0] + i32::from(hud.candle_size[0]) / 2 - 5;
            let at = [
                x - hud.font.width(text) / 2,
                hud.candle[1] + i32::from(hud.candle_size[1]),
            ];
            spawn_text(
                &mut commands,
                viewport,
                HudLayer::Health,
                &assets,
                &hud.font,
                text,
                at,
            );
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
    if item.count > 1 {
        let at = [
            hud.pickup[0] + i32::from(item.size[0]) - 32,
            hud.pickup[1] + 8,
        ];
        spawn_text(
            &mut commands,
            viewport,
            HudLayer::Item,
            &assets,
            &hud.font,
            &item.count.to_string(),
            at,
        );
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
    fn text_advances_by_width_plus_one_and_spaces_add_the_exclamation_width() {
        let glyph = |width| {
            Some(HudGlyph {
                width,
                height: 1,
                offset: [1, 2],
            })
        };
        let font = HudFont {
            start: 32,
            glyphs: vec![glyph(2), glyph(8), None, glyph(5)],
            font_sel: [255, 255],
        };
        assert_eq!(font.width(" !\"#"), 11 + 9 + 6);
        let placed: Vec<_> = font
            .layout(" #", 10, 20)
            .into_iter()
            .map(|(ch, at, _)| (ch, at))
            .collect();
        assert_eq!(placed, [(b' ', [11, 22]), (b'#', [22, 22])]);
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
                font: HudFont {
                    start: 0,
                    glyphs: vec![],
                    font_sel: [255, 255],
                },
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
