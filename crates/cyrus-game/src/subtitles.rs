//! Dialogue subtitles: the RTX text of a voice clip, drawn in `fonts/REDSEL.FNT`
//! in a fixed band at the bottom of the 640×480 screen while the line runs
//! (preservation `docs/engine/subtitles.md`,
//! [PR 25](https://github.com/michidk/redguard-preservation/pull/25)).
//!
//! Glyphs keep the font's own palette colours at full opacity; a black copy
//! offset by one pixel sits beneath at alpha 255×200/256 (NPC `[3dfx]
//! font_sel`), as captured from the original. Lines wrap at `[dialog]
//! dialog_max_text_width` pixels. Only the lowest active slot is drawn.
//!
//! Unsupported until evidence is published: Cyrus' own lines (no speaker
//! exists yet; they would use `font_norm`), movie overlays and combat taunts.

use crate::install::GameInstall;
use crate::sound::{SoundRequest, SoundSettings, Speech, ini_value};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cyrus_text::Font;
pub use cyrus_text::font_alpha;
use rgpre::import::fnt::parse_fnt;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;
/// Game ticks per second: `normal_frame_rate` (12) per 18 BIOS timer ticks of 18.2065 Hz
/// (preservation `docs/engine/player.md#fps-derived-frame-scale`).
const TICKS_PER_SECOND: f64 = 12.0 * 18.2065 / 18.0;

pub struct SubtitlePlugin;

impl Plugin for SubtitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_font).add_systems(
            Update,
            (start_lines, show_lines)
                .chain()
                .run_if(resource_exists::<SubtitleFont>),
        );
    }
}

/// REDSEL.FNT and the `[dialog]`/`[3dfx]` settings subtitles need.
#[derive(Resource)]
pub struct SubtitleFont {
    font: Font,
    max_width: i32,
    /// Main and shadow alpha of the NPC copy: `B` and `A × 200 / 256` of `[3dfx] font_sel`.
    alpha: [u8; 2],
    slots: Vec<Option<Line>>,
    shown: Option<usize>,
    overlay: Option<(Entity, Handle<Image>)>,
}

struct Line {
    text: String,
    /// Seconds left; the line is freed when it reaches 0.
    remaining: f64,
}

fn load_font(mut commands: Commands, install: Res<GameInstall>) {
    let font_path = install.root.join("fonts").join("REDSEL.FNT");
    let system_path = install.root.join("SYSTEM.INI");
    let font = match std::fs::read(&font_path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| parse_fnt(&bytes).map_err(|e| e.to_string()))
    {
        Ok(font) => Font::from(&font),
        Err(e) => return error!("subtitles: {}: {e}", font_path.display()),
    };
    let ini = std::fs::read_to_string(&system_path).unwrap_or_else(|e| {
        warn!("subtitles: {}: {e}; using defaults", system_path.display());
        String::new()
    });
    let number = |key: &str, default: i32| {
        ini_value(&ini, "dialog", key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let slots = number("dialog_max_dialog_lines", 16).max(16) as usize;
    let alpha = match font_alpha(ini_value(&ini, "3dfx", "font_sel").unwrap_or("255,255")) {
        Ok(alpha) => alpha,
        Err(e) => return error!("subtitles: SYSTEM.INI [3dfx] font_sel: {e}"),
    };
    commands.insert_resource(SubtitleFont {
        font,
        max_width: number("dialog_max_text_width", 150).max(150),
        alpha,
        slots: (0..slots).map(|_| None).collect(),
        shown: None,
        overlay: None,
    });
}

fn start_lines(
    mut requests: MessageReader<SoundRequest>,
    speech: Res<Speech>,
    settings: Option<Res<SoundSettings>>,
    mut font: ResMut<SubtitleFont>,
    time: Res<Time<Real>>,
) {
    let speaking = settings.as_ref().is_none_or(|s| s.speech);
    for request in requests.read() {
        match request {
            SoundRequest::Speech(label) => {
                let Some(clip) = speech.0.get(label) else {
                    continue;
                };
                // Speech on: the line lasts as long as its clip. Off: clip seconds × 256, minus 21 per tick.
                let remaining = if speaking {
                    clip.seconds
                } else {
                    (clip.seconds * 256.0).trunc() / (21.0 * TICKS_PER_SECOND)
                };
                let text = font.font.wrap(&clip.text, font.max_width);
                // Slot 0 belongs to Cyrus; other speakers take the lowest free slot or get none.
                if let Some(slot) = font.slots.iter_mut().skip(1).find(|s| s.is_none()) {
                    *slot = Some(Line { text, remaining });
                }
            }
            SoundRequest::StopAll => font.slots.iter_mut().for_each(|s| *s = None),
            SoundRequest::Play { .. } => {}
        }
    }
    let delta = time.delta_secs_f64();
    for slot in font.slots.iter_mut() {
        if let Some(line) = slot {
            line.remaining -= delta;
            if line.remaining <= 0.0 {
                *slot = None;
            }
        }
    }
}

fn show_lines(
    mut commands: Commands,
    mut font: ResMut<SubtitleFont>,
    settings: Option<Res<SoundSettings>>,
    mut images: Option<ResMut<Assets<Image>>>,
) {
    let Some(images) = images.as_mut() else {
        return;
    };
    let visible = settings.as_ref().is_none_or(|s| s.subtitles);
    let shown = font
        .slots
        .iter()
        .position(Option::is_some)
        .filter(|_| visible);
    if shown == font.shown {
        return;
    }
    font.shown = shown;
    if let Some((entity, _)) = font.overlay.take() {
        commands.entity(entity).despawn();
    }
    let Some(index) = shown else {
        return;
    };
    let text = font.slots[index]
        .as_ref()
        .map(|l| l.text.clone())
        .unwrap_or_default();
    let rgba = draw(&font.font, &text, font.alpha);
    let image = images.add(Image::new(
        Extent3d {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let entity = commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(i32::MAX - 1),
            Pickable::IGNORE,
            children![(
                Node {
                    height: percent(100),
                    max_width: percent(100),
                    aspect_ratio: Some(4.0 / 3.0),
                    ..default()
                },
                ImageNode::new(image.clone()),
                Pickable::IGNORE,
            )],
        ))
        .id();
    font.overlay = Some((entity, image));
}

/// Renders wrapped subtitle text into a 640×480 RGBA overlay: centred on x = 320,
/// first line top at `470 − 26 × (max(breaks, 1) + 1)`, drawn by [`Font::draw_shadowed`].
pub fn draw(font: &Font, text: &str, alpha: [u8; 2]) -> Vec<u8> {
    let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
    let line_height = i32::from(font.line_height);
    let lines: Vec<&str> = text.split('\n').collect();
    let breaks = lines.len() as i32 - 1;
    let mut top = 470 - line_height * (breaks.max(1) + 1);
    for line in lines {
        let pen = 320 + (-font.width(line)) / 2;
        font.draw_shadowed(&mut rgba, WIDTH, line, [pen, top], alpha);
        top += line_height;
    }
    rgba
}
