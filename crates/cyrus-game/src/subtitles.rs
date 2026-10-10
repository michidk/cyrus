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
use rgpre::import::fnt::{FntFile, parse_fnt};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;
/// Game ticks per second assumed for the text-only countdown.
// shortcut: the 12 Hz logic tick is inferred, not observed; confirm before relying on text-only timing.
const TICKS_PER_SECOND: f64 = 12.0;

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
    font: FntFile,
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
        Ok(font) => font,
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

/// `[3dfx] font_*` "A,B" → (main alpha B, shadow alpha A × 200 / 256).
pub fn font_alpha(value: &str) -> Result<[u8; 2], String> {
    let parts: Vec<u32> = value
        .split(',')
        .map(|v| v.trim().parse::<u32>().map_err(|e| format!("{value}: {e}")))
        .collect::<Result<_, _>>()?;
    let [a, b] = parts[..] else {
        return Err(format!("{value}: expected two values"));
    };
    let byte = |v: u32| u8::try_from(v).map_err(|_| format!("{value}: {v} exceeds 255"));
    Ok([byte(b)?, byte(a * 200 / 256)?])
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
                let text = wrap(&font.font, &clip.text, font.max_width);
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

/// Pixel advance of one byte: glyph width + 1, plus the width of `!` for a space.
fn advance(font: &FntFile, ch: u8) -> i32 {
    let start = usize::from(font.header.character_start);
    let glyph = |c: u8| {
        usize::from(c)
            .checked_sub(start)
            .and_then(|i| font.glyphs.get(i))
            .filter(|g| g.enabled != 0)
    };
    glyph(ch).map_or(0, |g| {
        i32::from(g.width)
            + 1
            + if ch == b' ' {
                glyph(b'!').map_or(0, |g| i32::from(g.width))
            } else {
                0
            }
    })
}

fn width(font: &FntFile, text: &str) -> i32 {
    text.bytes().map(|ch| advance(font, ch)).sum()
}

/// Breaks `text` into lines: a token (optional leading space + word) joins the
/// line while the running width stays strictly below `max_width`; after a break
/// the running width keeps the dropped space's width, as the original does.
pub fn wrap(font: &FntFile, text: &str, max_width: i32) -> String {
    let mut out = String::new();
    let mut current = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let lead = usize::from(rest.starts_with(' '));
        let end = rest[lead..].find(' ').map_or(rest.len(), |i| i + lead);
        let token = &rest[..end];
        let token_width = width(font, token);
        if current + token_width < max_width {
            out.push_str(token);
            current += token_width;
        } else {
            out.push('\n');
            out.push_str(&token[lead..]);
            current = token_width;
        }
        rest = &rest[end..];
    }
    out
}

/// Composites `colour` at `alpha` over the pixel (source-over, straight alpha).
fn blend(pixel: &mut [u8], colour: [u8; 3], alpha: u8) {
    let (sa, da) = (f32::from(alpha) / 255.0, f32::from(pixel[3]) / 255.0);
    let out = sa + da * (1.0 - sa);
    if out <= 0.0 {
        return;
    }
    for c in 0..3 {
        let mixed = (f32::from(colour[c]) * sa + f32::from(pixel[c]) * da * (1.0 - sa)) / out;
        pixel[c] = mixed.round() as u8;
    }
    pixel[3] = (out * 255.0).round() as u8;
}

/// Renders wrapped subtitle text into a 640×480 RGBA overlay: centred on x = 320,
/// first line top at `470 − 26 × (max(breaks, 1) + 1)`, a black shadow at (+1, +1)
/// under each glyph, then the glyph in its palette colours.
pub fn draw(font: &FntFile, text: &str, [main, shadow]: [u8; 2]) -> Vec<u8> {
    let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
    let line_height = i32::from(font.header.line_height);
    let lines: Vec<&str> = text.split('\n').collect();
    let breaks = lines.len() as i32 - 1;
    let mut top = 470 - line_height * (breaks.max(1) + 1);
    let start = usize::from(font.header.character_start);
    for line in lines {
        let mut pen = 320 + (-width(font, line)) / 2;
        for ch in line.bytes() {
            if let Some(glyph) = usize::from(ch)
                .checked_sub(start)
                .and_then(|i| font.glyphs.get(i))
                .filter(|g| g.enabled != 0)
            {
                for (offset, alpha, shadow_copy) in [(1, shadow, true), (0, main, false)] {
                    for y in 0..i32::from(glyph.height) {
                        for x in 0..i32::from(glyph.width) {
                            let index = glyph.pixels[(y * i32::from(glyph.width) + x) as usize];
                            if index == 0 {
                                continue;
                            }
                            let px = pen + i32::from(glyph.offset_left) + x + offset;
                            let py = top + i32::from(glyph.offset_top) + y + offset;
                            if !(0..WIDTH as i32).contains(&px) || !(0..HEIGHT as i32).contains(&py)
                            {
                                continue;
                            }
                            let colour = if shadow_copy {
                                [0, 0, 0]
                            } else {
                                // FNT palettes hold 6-bit components.
                                font.palette.colors[usize::from(index)]
                                    .map(|c| (u32::from(c) * 255 / 63) as u8)
                            };
                            let at = (py as usize * WIDTH + px as usize) * 4;
                            blend(&mut rgba[at..at + 4], colour, alpha);
                        }
                    }
                }
            }
            pen += advance(font, ch);
        }
        top += line_height;
    }
    rgba
}
