//! Redguard FNT text: glyph lookup, advance, wrapping and pixel output, following
//! redguard-preservation's FNT and subtitle documentation. Callers own where pixels
//! go (an RGBA overlay, a palette-indexed page, a movie frame) and their colours.

use serde::{Deserialize, Serialize};

/// An installed FNT font; glyphs are palette-indexed, index 0 is transparent.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Font {
    /// First encoded character code.
    pub start: u16,
    pub line_height: u16,
    pub glyphs: Vec<Glyph>,
    /// The font's own 6-bit palette (BPAL/FPAL); empty when the file has none.
    pub palette: Vec<[u8; 3]>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Glyph {
    pub enabled: bool,
    pub width: u16,
    pub height: u16,
    /// Pixel offset from the pen position to the glyph's top-left corner.
    pub offset: [i16; 2],
    /// Row-major palette indices, `width × height`.
    pub pixels: Vec<u8>,
}

#[cfg(feature = "native")]
impl From<&rgpre::import::fnt::FntFile> for Font {
    fn from(font: &rgpre::import::fnt::FntFile) -> Self {
        Self {
            start: font.header.character_start,
            line_height: font.header.line_height,
            glyphs: font
                .glyphs
                .iter()
                .map(|g| Glyph {
                    enabled: g.enabled != 0,
                    width: g.width,
                    height: g.height,
                    offset: [g.offset_left, g.offset_top],
                    pixels: g.pixels.clone(),
                })
                .collect(),
            palette: font.palette.colors.clone(),
        }
    }
}

impl Font {
    /// The enabled glyph for a character code; disabled and missing glyphs are `None`.
    pub fn glyph(&self, ch: u8) -> Option<&Glyph> {
        usize::from(ch)
            .checked_sub(usize::from(self.start))
            .and_then(|i| self.glyphs.get(i))
            .filter(|g| g.enabled)
    }

    /// Pen advance: glyph width + 1, plus the width of `!` for a space; 0 without a glyph.
    pub fn advance(&self, ch: u8) -> i32 {
        self.glyph(ch).map_or(0, |g| {
            let space = if ch == b' ' {
                self.glyph(b'!').map_or(0, |g| i32::from(g.width))
            } else {
                0
            };
            i32::from(g.width) + 1 + space
        })
    }

    pub fn width(&self, text: &str) -> i32 {
        text.bytes().map(|ch| self.advance(ch)).sum()
    }

    /// Breaks `text` into `\n`-separated lines: a token (optional leading space + word)
    /// joins the line while the running width stays strictly below `max_width`; after a
    /// break the running width keeps the dropped space's width, as the original does.
    pub fn wrap(&self, text: &str, max_width: i32) -> String {
        let mut out = String::new();
        let mut current = 0;
        let mut rest = text;
        while !rest.is_empty() {
            let lead = usize::from(rest.starts_with(' '));
            let end = rest[lead..].find(' ').map_or(rest.len(), |i| i + lead);
            let token = &rest[..end];
            let token_width = self.width(token);
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

    /// Calls `plot(x, y, index)` for every opaque pixel of one line with its pen at
    /// `at`; glyphs sit at pen + offset. Coordinates are unclipped.
    pub fn draw(&self, text: &str, at: [i32; 2], mut plot: impl FnMut(i32, i32, u8)) {
        let mut pen = at[0];
        for ch in text.bytes() {
            self.draw_glyph(ch, [pen, at[1]], &mut plot);
            pen += self.advance(ch);
        }
    }

    /// Draws one character code with its pen at `at`, like `draw` for a single glyph.
    pub fn draw_char(&self, ch: u8, at: [i32; 2], mut plot: impl FnMut(i32, i32, u8)) {
        self.draw_glyph(ch, at, &mut plot);
    }

    fn draw_glyph(&self, ch: u8, at: [i32; 2], plot: &mut impl FnMut(i32, i32, u8)) {
        let Some(glyph) = self.glyph(ch) else {
            return;
        };
        let row = usize::from(glyph.width).max(1);
        for (i, &index) in glyph.pixels.iter().enumerate() {
            if index != 0 {
                plot(
                    at[0] + i32::from(glyph.offset[0]) + (i % row) as i32,
                    at[1] + i32::from(glyph.offset[1]) + (i / row) as i32,
                    index,
                );
            }
        }
    }

    /// 8-bit colour of a font palette index: 6-bit components shifted left by 2, which
    /// matches the original's captured REDSEL subtitle colours.
    pub fn colour(&self, index: u8) -> [u8; 3] {
        self.palette
            .get(usize::from(index))
            .map_or([0; 3], |c| c.map(|v| v << 2))
    }

    /// Draws one line into straight-alpha RGBA like an NPC subtitle: per glyph a black
    /// copy at (+1, +1) with alpha `shadow`, then the glyph in its palette colours with
    /// alpha `main`. Pixels outside `width × (rgba.len() / 4 / width)` are clipped.
    pub fn draw_shadowed(
        &self,
        rgba: &mut [u8],
        width: usize,
        text: &str,
        at: [i32; 2],
        alpha: [u8; 2],
    ) {
        self.draw_shadowed_by(rgba, width, text, at, alpha, 1);
    }

    /// [`Self::draw_shadowed`] with the shadow copy `shadow_offset` pixels right and down.
    pub fn draw_shadowed_by(
        &self,
        rgba: &mut [u8],
        width: usize,
        text: &str,
        at: [i32; 2],
        [main, shadow]: [u8; 2],
        shadow_offset: i32,
    ) {
        let height = rgba.len() / 4 / width;
        let mut pen = at[0];
        for ch in text.bytes() {
            for (offset, alpha, shadow_copy) in [(shadow_offset, shadow, true), (0, main, false)] {
                self.draw_glyph(ch, [pen + offset, at[1] + offset], &mut |x, y, index| {
                    if (0..width as i32).contains(&x) && (0..height as i32).contains(&y) {
                        let colour = if shadow_copy {
                            [0; 3]
                        } else {
                            self.colour(index)
                        };
                        let at = (y as usize * width + x as usize) * 4;
                        blend(&mut rgba[at..at + 4], colour, alpha);
                    }
                });
            }
            pen += self.advance(ch);
        }
    }
}

/// SYSTEM.INI `[3dfx] font_*` "A,B" → `[main alpha B, shadow alpha B × 200 / 256]`; the glyph draw reads the shadow
/// value from the same field as the main value, and `A` does not enter the alpha.
pub fn font_alpha(value: &str) -> Result<[u8; 2], String> {
    let parts: Vec<u32> = value
        .split(',')
        .map(|v| v.trim().parse::<u32>().map_err(|e| format!("{value}: {e}")))
        .collect::<Result<_, _>>()?;
    let [a, b] = parts[..] else {
        return Err(format!("{value}: expected two values"));
    };
    let byte = |v: u32| u8::try_from(v).map_err(|_| format!("{value}: {v} exceeds 255"));
    byte(a)?;
    Ok([byte(b)?, byte(b * 200 / 256)?])
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

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Font {
        let glyph = |width, enabled| Glyph {
            enabled,
            width,
            height: 1,
            offset: [1, 2],
            pixels: vec![1; usize::from(width)],
        };
        Font {
            start: 32,
            line_height: 1,
            glyphs: vec![
                glyph(2, true),
                glyph(8, true),
                glyph(4, false),
                glyph(5, true),
            ],
            palette: vec![[0; 3], [63, 39, 1]],
        }
    }

    #[test]
    fn advance_adds_one_and_spaces_add_the_exclamation_width() {
        let font = font();
        assert_eq!(font.width(" !\"#"), 11 + 9 + 6);
        assert!(font.glyph(b'"').is_none(), "disabled glyphs are absent");
    }

    #[test]
    fn draw_places_glyphs_at_pen_plus_offset() {
        let mut plotted = Vec::new();
        font().draw(" #", [10, 20], |x, y, i| plotted.push((x, y, i)));
        assert_eq!(plotted[..3], [(11, 22, 1), (12, 22, 1), (22, 22, 1)]);
        assert_eq!(plotted.len(), 2 + 5);
    }

    #[test]
    fn wrap_breaks_below_the_width_and_drops_the_space() {
        // '#' is 6 wide; " #" is 17.
        assert_eq!(font().wrap("## ## ##", 30), "##\n##\n##");
        assert_eq!(font().wrap("## ##", 40), "## ##");
    }

    #[test]
    fn shadowed_text_draws_black_beneath_palette_colour() {
        let mut rgba = vec![0; 4 * 4 * 4];
        let mut font = font();
        font.glyphs[3].offset = [0, 0];
        font.glyphs[3].width = 1;
        font.glyphs[3].pixels = vec![1];
        font.draw_shadowed(&mut rgba, 4, "#", [1, 1], [255, 199]);
        assert_eq!(rgba[(4 + 1) * 4..][..4], [252, 156, 4, 255]);
        assert_eq!(rgba[(2 * 4 + 2) * 4..][..4], [0, 0, 0, 199]);
    }

    #[test]
    fn font_sel_pairs_become_main_and_shadow_alpha() {
        assert_eq!(font_alpha("255,255"), Ok([255, 199]));
        assert_eq!(font_alpha("125,255"), Ok([255, 199]), "A does not enter the alpha");
        assert_eq!(font_alpha("255,180"), Ok([180, 140]), "the movie copy");
        assert!(font_alpha("255").is_err());
    }
}
