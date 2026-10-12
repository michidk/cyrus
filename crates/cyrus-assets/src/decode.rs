//! In-memory presentation of preservation parser results, shared by both targets.
use crate::{Content, Picture};
use anyhow::{Result, ensure};

/// Glyph images from the shared text font: its 6-bit palette colours, index 0 transparent.
pub fn font(font: &cyrus_text::Font) -> Result<Content> {
    let mut glyphs = Vec::with_capacity(font.glyphs.len());
    for (index, glyph) in font.glyphs.iter().enumerate() {
        let width = u32::from(glyph.width).max(1);
        let height = u32::from(glyph.height).max(1);
        ensure!(
            glyph.pixels.len() == usize::from(glyph.width) * usize::from(glyph.height),
            "font glyph {index}: pixel count does not match dimensions"
        );
        ensure!(
            u64::from(width) * u64::from(height) <= 16_777_216,
            "font glyph {index}: invalid image dimensions"
        );
        let mut rgba = vec![0; (width * height * 4) as usize];
        for (pixel, &color) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&glyph.pixels) {
            if color != 0 {
                ensure!(
                    usize::from(color) < font.palette.len(),
                    "font glyph {index}: palette index {color} is missing"
                );
                let [r, g, b] = font.colour(color);
                *pixel = [r, g, b, 255];
            }
        }
        let picture = Picture {
            label: char::from_u32(u32::from(font.start) + index as u32)
                .unwrap_or('?')
                .to_string(),
            width,
            height,
            rgba,
            indices: None,
            embedded_palette: true,
        };
        picture.validate()?;
        glyphs.push(picture);
    }
    Ok(Content::Font {
        glyphs,
        first: font.start,
        line_height: font.line_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_glyphs_need_no_filesystem_or_export() {
        let mut source = cyrus_text::Font {
            start: 65,
            line_height: 1,
            glyphs: vec![cyrus_text::Glyph {
                enabled: true,
                width: 2,
                height: 1,
                offset: [0, 0],
                pixels: vec![0, 1],
            }],
            palette: vec![[63, 63, 63], [63, 16, 0]],
        };
        let Content::Font {
            glyphs,
            first,
            line_height,
        } = font(&source).unwrap()
        else {
            panic!("expected a font preview");
        };
        assert_eq!((first, line_height), (65, 1));
        assert_eq!(glyphs[0].label, "A");
        assert_eq!((glyphs[0].width, glyphs[0].height), (2, 1));
        assert_eq!(glyphs[0].rgba, [0, 0, 0, 0, 252, 64, 0, 255]);
        source.glyphs[0].pixels.pop();
        assert!(
            font(&source)
                .unwrap_err()
                .to_string()
                .contains("pixel count")
        );
    }
}
