//! In-memory presentation of preservation parser results, shared by both targets.
use crate::{Content, Picture};
use anyhow::{Result, ensure};

/// Builds glyph images directly from rgpre's decoded pixels and local font palette.
/// Color expansion and transparency match the preservation bitmap exporter.
pub fn font(font: &rgpre::import::fnt::FntFile) -> Result<Content> {
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
                let rgb = font.palette.colors.get(usize::from(color)).ok_or_else(|| {
                    anyhow::anyhow!("font glyph {index}: palette index {color} is missing")
                })?;
                *pixel = [
                    rgb[0].saturating_mul(4),
                    rgb[1].saturating_mul(4),
                    rgb[2].saturating_mul(4),
                    255,
                ];
            }
        }
        let picture = Picture {
            label: char::from_u32(u32::from(font.header.character_start) + index as u32)
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
        first: font.header.character_start,
        line_height: font.header.line_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgpre::import::fnt::{FntFile, FntGlyph, FntHeader, FntPalette};

    #[test]
    fn decoded_glyphs_need_no_filesystem_or_export() {
        let mut source = FntFile {
            chunk_order: Vec::new(),
            header: FntHeader {
                description_raw: [0; 32],
                description_text: String::new(),
                unknown_24: 0,
                has_rdat: 0,
                unknown_28: 0,
                unknown_2a: 0,
                unknown_2c: 0,
                max_width: 2,
                line_height: 1,
                character_start: 65,
                character_count: 1,
                unknown_36: 0,
                unknown_38: 0,
                has_palette: 1,
            },
            palette: FntPalette {
                tag: *b"FPAL",
                colors: vec![[63, 63, 63], [63, 16, 0]],
            },
            glyphs: vec![FntGlyph {
                enabled: 1,
                offset_left: 0,
                offset_top: 0,
                width: 2,
                height: 1,
                pixels: vec![0, 1],
            }],
            rdat: None,
            trailing_padding: Vec::new(),
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
