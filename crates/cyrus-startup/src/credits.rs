//! The ending's credits scroll from `CREDITS.TXT`; rules live in preservation's engine/menu.md.
use super::StartupContent;
use super::menu::{MenuState, MovieStatus};
use super::movie::MoviePlayback;
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::logbook::{CreditAlign, CreditLine, LogbookArtwork, ReaderStyle};
use cyrus_game::{AppState, script::CreditsPlaying};
use cyrus_text::Font;

const WIDTH: i32 = 640;
const HEIGHT: i32 = 480;
/// The original moves one pixel per swap with swap interval one, i.e. per display refresh;
/// **Inferred** 60 Hz (the refresh rate it sets is not recovered).
const PIXELS_PER_SECOND: f64 = 60.0;
/// Reader style table entry that style 5 uses.
const STYLE_FIVE_ENTRY: usize = 9;

/// The ending asked for the credits once its movie has finished.
#[derive(Resource)]
pub(super) struct CreditsPending;

pub(super) fn register(app: &mut App) {
    app.add_systems(
        Update,
        drive
            .after(super::movie::drive)
            .run_if(in_state(AppState::Menu)),
    );
}

/// Where the scroll is: the height of the first visible record and its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    top: i32,
    first: i32,
}

fn continued(credits: &[CreditLine], index: i32) -> bool {
    usize::try_from(index)
        .ok()
        .and_then(|i| credits.get(i))
        .is_some_and(|line| line.continued)
}

impl Position {
    /// `line` is the line height of the style-0 font.
    fn start(line: i32) -> Self {
        Self {
            top: line,
            first: -(HEIGHT / line) + 1,
        }
    }

    /// Records on screen with their heights; a continued record shares its predecessor's height.
    fn visible(&self, credits: &[CreditLine], line: i32) -> Vec<(usize, i32)> {
        let (mut y, mut index) = (self.top, self.first);
        let mut shown = Vec::new();
        while y < HEIGHT {
            match usize::try_from(index).ok().filter(|&i| i < credits.len()) {
                Some(record) => {
                    shown.push((record, y));
                    if !continued(credits, index + 1) {
                        y += line;
                    }
                }
                None => y += line,
            }
            index += 1;
        }
        shown
    }

    /// One pixel up. Returns true once the first visible record passes the record count.
    fn advance(&mut self, credits: &[CreditLine], line: i32) -> bool {
        self.top -= 1;
        if self.top < 0 {
            self.top += line;
            let next = self.first + 1;
            if next < 0 || !continued(credits, next) {
                self.first = next;
                if i32::try_from(credits.len()).unwrap_or(i32::MAX) < next {
                    return true;
                }
            } else {
                self.first = if continued(credits, self.first + 2) {
                    self.first + 3
                } else {
                    self.first + 2
                };
            }
        }
        false
    }
}

fn style_of(styles: &[Option<ReaderStyle>], style: u8) -> Option<ReaderStyle> {
    let entry = if style == 5 {
        STYLE_FIVE_ENTRY
    } else {
        usize::from(style)
    };
    styles.get(entry).copied().flatten()
}

fn paint(
    fonts: &[Font],
    styles: &[Option<ReaderStyle>],
    palette: &[[u8; 3]],
    credits: &[CreditLine],
    position: &Position,
    line: i32,
) -> Vec<u8> {
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    for chunk in pixels.as_chunks_mut::<4>().0 {
        chunk[3] = 255;
    }
    for (record, y) in position.visible(credits, line) {
        let credit = &credits[record];
        let (Some(text), Some(style)) = (&credit.text, style_of(styles, credit.style)) else {
            continue;
        };
        let font = &fonts[style.font];
        let width: i32 = text.iter().map(|&ch| font.advance(ch)).sum();
        let mut pen_x = match credit.align {
            CreditAlign::Left => credit.x,
            CreditAlign::Centre => credit.x - width / 2,
            CreditAlign::Right => credit.x - width,
        };
        let main = u32::from(style.alpha);
        let shadow = main * 200 / 256;
        for &ch in text {
            if font.glyph(ch).is_some() {
                // The glyph shadow is a black copy two pixels right and down.
                for (offset, alpha, shadowed) in [(2, shadow, true), (0, main, false)] {
                    font.draw_char(ch, [pen_x + offset, y + offset], |x, py, index| {
                        if !(0..WIDTH).contains(&x) || !(0..HEIGHT).contains(&py) {
                            return;
                        }
                        let colour = if shadowed {
                            [0; 3]
                        } else if style.page_palette || font.palette.is_empty() {
                            palette[usize::from(index)]
                        } else {
                            super::logbook::scale_colour(font.colour(index), style.scale)
                        };
                        let at = ((py * WIDTH + x) * 4) as usize;
                        for c in 0..3 {
                            pixels[at + c] = ((u32::from(colour[c]) * alpha
                                + u32::from(pixels[at + c]) * (255 - alpha)
                                + 127)
                                / 255) as u8;
                        }
                    });
                }
            }
            pen_x += font.advance(ch);
        }
    }
    // The first and last line heights are covered with black.
    let cover = (line * WIDTH * 4) as usize;
    for chunk in pixels[..cover].as_chunks_mut::<4>().0 {
        chunk[..3].fill(0);
    }
    let bottom = ((HEIGHT - line) * WIDTH * 4) as usize;
    for chunk in pixels[bottom..].as_chunks_mut::<4>().0 {
        chunk[..3].fill(0);
    }
    pixels
}

struct Run {
    artwork: Handle<LogbookArtwork>,
    scroll: Option<(Position, f64)>,
    image: Handle<Image>,
    /// The key that skipped the movie must not also end the credits.
    first_update: bool,
}

#[allow(clippy::too_many_arguments)] // One owner reads the movie, menu, asset and input state.
fn drive(
    mut commands: Commands,
    pending: Option<Res<CreditsPending>>,
    playback: Res<MoviePlayback>,
    mut menu: ResMut<MenuState>,
    server: Res<AssetServer>,
    artworks: Res<Assets<LogbookArtwork>>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut images: ResMut<Assets<Image>>,
    mut sprite: Query<&mut Sprite, With<StartupContent>>,
    mut run: Local<Option<Run>>,
) {
    if run.is_none() {
        if pending.is_none() || !playback.idle() || menu.movie.is_some() {
            return;
        }
        *run = Some(Run {
            artwork: server.load("cache://logbook/reader.logbook"),
            scroll: None,
            image: Handle::default(),
            first_update: true,
        });
        menu.movie = Some(MovieStatus {
            name: "CREDITS.TXT".into(),
            frame: 0,
            frames: 0,
            loading: true,
        });
        menu.ready = false;
    }
    let active = run.as_mut().expect("credits run");
    let finish = |commands: &mut Commands, menu: &mut MenuState, run: &mut Option<Run>| {
        commands.remove_resource::<CreditsPending>();
        commands.remove_resource::<CreditsPlaying>();
        menu.movie = None;
        menu.dirty = true;
        *run = None;
    };
    let Some(data) = artworks.get(&active.artwork) else {
        if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(&active.artwork)
        {
            warn!("credits skipped: reader artwork unavailable: {error}");
            finish(&mut commands, &mut menu, &mut run);
        }
        return;
    };
    let Some(credits) = data.credits.as_deref() else {
        // The original skips the credits when CREDITS.TXT cannot be loaded.
        warn!("credits skipped: no CREDITS.TXT in the install");
        finish(&mut commands, &mut menu, &mut run);
        return;
    };
    let Some(line) =
        style_of(&data.styles, 0).map(|style| i32::from(data.fonts[style.font].line_height))
    else {
        warn!("credits skipped: reader style 0 is not initialised");
        finish(&mut commands, &mut menu, &mut run);
        return;
    };
    let escape = !active.first_update && keys.just_pressed(KeyCode::Escape);
    active.first_update = false;
    let (mut position, mut due) = active.scroll.unwrap_or_else(|| {
        commands.insert_resource(CreditsPlaying);
        if let Some(status) = menu.movie.as_mut() {
            status.loading = false;
        }
        (Position::start(line), 0.0)
    });
    due += time.delta_secs_f64() * PIXELS_PER_SECOND;
    let mut done = escape;
    while !done && due >= 1.0 {
        due -= 1.0;
        done = position.advance(credits, line);
    }
    if done {
        finish(&mut commands, &mut menu, &mut run);
        return;
    }
    active.scroll = Some((position, due));
    let pixels = paint(
        &data.fonts,
        &data.styles,
        &data.palette,
        credits,
        &position,
        line,
    );
    let image = Image::new(
        Extent3d {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // The default handle names the engine's fallback texture, which must not be overwritten.
    if active.image == Handle::default() {
        active.image = images.add(image);
    } else if let Some(mut previous) = images.get_mut(&active.image) {
        *previous = image;
    } else {
        active.image = images.add(image);
    }
    if let Ok(mut sprite) = sprite.single_mut() {
        sprite.image = active.image.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(continued: bool) -> CreditLine {
        CreditLine {
            continued,
            ..CreditLine::default()
        }
    }

    #[test]
    fn the_scroll_starts_below_the_screen_and_ends_past_the_last_record() {
        let credits = vec![line(false), line(false)];
        let mut position = Position::start(26);
        assert_eq!(
            position,
            Position {
                top: 26,
                first: -17
            }
        );
        // The first record starts under the black cover of the bottom line.
        assert_eq!(position.visible(&credits, 26), vec![(0, 468)]);
        let mut pixels = 0;
        while !position.advance(&credits, 26) {
            pixels += 1;
            assert!(pixels < 100_000);
        }
        // The first record index rises from -17 to 3 (20 steps of one line, the first one pixel longer).
        assert_eq!(pixels, 520);
    }

    #[test]
    fn a_record_scrolls_into_view_from_the_bottom_edge() {
        let credits = vec![line(false)];
        let mut position = Position::start(26);
        let mut first_height = None;
        for _ in 0..26 * 25 {
            if let Some(&(_, y)) = position.visible(&credits, 26).first() {
                first_height = Some(y);
                break;
            }
            position.advance(&credits, 26);
        }
        assert!(first_height.is_some_and(|y| (HEIGHT - 26..HEIGHT).contains(&y)));
    }

    #[test]
    fn continued_records_share_their_predecessors_height() {
        let credits = vec![line(false), line(true), line(false)];
        let position = Position { top: 100, first: 0 };
        let shown = position.visible(&credits, 26);
        assert_eq!(&shown[..3], &[(0, 100), (1, 100), (2, 126)]);
        let mut position = Position { top: 0, first: 0 };
        position.advance(&credits, 26);
        assert_eq!(
            position,
            Position { top: 25, first: 2 },
            "a continued group is passed as one"
        );
    }

    #[test]
    fn text_is_aligned_shadowed_and_clipped_by_the_black_covers() {
        let font = Font {
            start: 65,
            line_height: 26,
            glyphs: vec![cyrus_text::Glyph {
                enabled: true,
                width: 2,
                height: 2,
                offset: [0, 0],
                pixels: vec![1; 4],
            }],
            palette: vec![[0; 3], [63; 3]],
        };
        let styles = vec![Some(ReaderStyle {
            font: 0,
            scale: 255,
            alpha: 255,
            page_palette: false,
        })];
        let record = |align, x| CreditLine {
            x,
            align,
            text: Some(b"A".to_vec()),
            ..CreditLine::default()
        };
        let credits = vec![record(CreditAlign::Centre, 320)];
        let at = |pixels: &[u8], x: i32, y: i32| pixels[((y * WIDTH + x) * 4) as usize];
        let mut position = Position { top: 100, first: 0 };
        let pixels = paint(
            std::slice::from_ref(&font),
            &styles,
            &[],
            &credits,
            &position,
            26,
        );
        // Centred on x: advance 3, so the glyph starts at 320 - 1; its shadow is black.
        assert!(at(&pixels, 319, 100) > 200 && at(&pixels, 320, 101) > 200);
        assert_eq!(at(&pixels, 321, 102), 0, "the shadow copy is black");
        position.top = 10;
        assert!(
            paint(
                std::slice::from_ref(&font),
                &styles,
                &[],
                &credits,
                &position,
                26
            )
            .iter()
            .step_by(4)
            .all(|&r| r == 0),
            "text under the first line height is covered"
        );
    }
}
