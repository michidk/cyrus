//! Installed logbook paper, fonts and scroll geometry for native and browser readers.
use crate::menu::{MenuModel, MenuPicture, PageTexture};
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct LogbookArtwork {
    pub paper: MenuPicture,
    pub scroll: MenuModel,
    /// Per scroll face, the normal of each corner that the engine lights: the vertex normal the
    /// indirection table selects, or the face normal when that one is unusable. Absent in caches
    /// written before the scroll was lit, which `validate` rejects.
    #[serde(default)]
    pub scroll_normals: Vec<Vec<[f32; 3]>>,
    /// Per scroll face, the lighting brightness byte `(tex_hi * 4) & 255`.
    #[serde(default)]
    pub scroll_brightness: Vec<u8>,
    pub textures: Vec<PageTexture>,
    pub palette: Vec<[u8; 3]>,
    /// Reader fonts: small normal, small selected, REDSEL, FONTNORM and `gui_low_font`.
    pub fonts: Vec<cyrus_text::Font>,
    /// Style table by prefix digit 0-9; `None` where the original's entry is not decoded.
    /// Absent in caches written before the table existed, which `validate` rejects.
    #[serde(default)]
    pub styles: Vec<Option<ReaderStyle>>,
    /// `CREDITS.TXT` records; `None` when the install has no such file, which skips the credits.
    pub credits: Option<Vec<CreditLine>>,
    /// Set by caches written after the credits were added; `validate` rejects older ones.
    #[serde(default)]
    pub credits_exported: bool,
}

/// How a credits record places its text at its x position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreditAlign {
    #[default]
    Left,
    Centre,
    Right,
}

/// One `CREDITS.TXT` record as the original's loader builds it (preservation engine/menu.md).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditLine {
    pub x: i32,
    /// Drawn at the previous record's height and adding no line advance.
    pub continued: bool,
    pub align: CreditAlign,
    /// Index into the reader style table; 5 draws with the table's entry 9.
    pub style: u8,
    /// `None` for a line without fields: it draws nothing but still occupies a line.
    pub text: Option<Vec<u8>>,
}

/// Records of `CREDITS.TXT`: at most 2000, split at line feeds after the text-mode read drops
/// carriage returns and stops at a Ctrl-Z. A line is `[-]x,align,style,text`; an empty line, or
/// one whose three separating commas do not all lie within its first ten bytes (the original reads
/// garbage there; none ships), keeps the previous style and draws nothing.
pub fn parse_credits(bytes: &[u8]) -> Vec<CreditLine> {
    let end = bytes.iter().position(|&b| b == 0x1a).unwrap_or(bytes.len());
    let data: Vec<u8> = bytes[..end]
        .iter()
        .copied()
        .filter(|&b| b != b'\r')
        .collect();
    let mut lines = Vec::new();
    let mut style = 0;
    for line in data.split(|&b| b == b'\n').take(2000) {
        let mut record = CreditLine {
            style,
            ..CreditLine::default()
        };
        let body = match line.first() {
            Some(b'-') => {
                record.continued = true;
                &line[1..]
            }
            _ => line,
        };
        let commas: Vec<usize> = body
            .iter()
            .take(10)
            .enumerate()
            .filter_map(|(i, &b)| (b == b',').then_some(i))
            .collect();
        if !line.is_empty() && commas.len() >= 3 {
            let number = |field: &[u8]| {
                let text = String::from_utf8_lossy(field);
                let text = text.trim_start();
                let digits = text
                    .char_indices()
                    .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')))
                    .count();
                text[..digits].parse::<i32>().unwrap_or(0)
            };
            record.x = number(&body[..commas[0]]);
            record.align = match body.get(commas[0] + 1) {
                Some(b'C' | b'c') => CreditAlign::Centre,
                Some(b'R' | b'r') => CreditAlign::Right,
                _ => CreditAlign::Left,
            };
            record.style = number(&body[commas[1] + 1..commas[2]]) as u8;
            record.text = Some(body[commas[2] + 1..].to_vec());
        }
        style = record.style;
        lines.push(record);
    }
    lines
}

/// One entry of the original reader's font table: the font drawn, its `A` value (a constant
/// colour factor, 255 is full strength) and `B` (main alpha), per preservation's reader style table.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct ReaderStyle {
    pub font: usize,
    pub scale: u8,
    pub alpha: u8,
    /// Glyph indices select page-palette colours (the `gui_low_font` entries) instead of the
    /// font's own palette, and the colour factor does not apply.
    #[serde(default)]
    pub page_palette: bool,
}
impl LogbookArtwork {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (self.paper.width, self.paper.height) == (640, 480)
                && self.paper.rgba.len() == 640 * 480 * 4,
            "logbook: LOGPAPER.GXA must supply 640x480 RGBA paper"
        );
        anyhow::ensure!(self.palette.len() == 256, "logbook: invalid palette");
        anyhow::ensure!(
            !self.scroll.frames.is_empty()
                && self.scroll.faces.iter().all(|face| face.corners.len() >= 3
                    && face.texture.is_none_or(|id| id < self.textures.len())
                    && self
                        .scroll
                        .frames
                        .iter()
                        .all(|frame| face.corners.iter().all(|c| c.vertex < frame.len()))),
            "logbook: SCROLL.3D has invalid frame, face or texture references"
        );
        anyhow::ensure!(
            self.scroll_normals.len() == self.scroll.faces.len()
                && self.scroll_brightness.len() == self.scroll.faces.len()
                && self
                    .scroll_normals
                    .iter()
                    .zip(&self.scroll.faces)
                    .all(|(normals, face)| normals.len() == face.corners.len()),
            "logbook: scroll lighting data absent from this cache; re-export the logbook"
        );
        anyhow::ensure!(
            self.textures.iter().all(|t| t.width > 0
                && t.height > 0
                && t.rgba.len() == usize::from(t.width) * usize::from(t.height) * 4),
            "logbook: invalid scroll texture dimensions"
        );
        anyhow::ensure!(
            self.fonts.len() == 5
                && self.styles.len() == 10
                && self
                    .styles
                    .iter()
                    .flatten()
                    .all(|style| style.font < self.fonts.len()),
            "logbook: reader styles absent or invalid; re-export the logbook"
        );
        anyhow::ensure!(
            self.credits_exported,
            "logbook: credits absent from this cache; re-export the logbook"
        );
        anyhow::ensure!(
            self.fonts.iter().all(|font| font.line_height > 0
                && font
                    .glyphs
                    .iter()
                    .all(|g| g.pixels.len() == usize::from(g.width) * usize::from(g.height))),
            "logbook: invalid font glyph dimensions"
        );
        Ok(())
    }
}

/// The 256-entry palette of a GXA file (its `BPAL` section holds 6-bit values, shifted left two
/// bits for display), found by walking the file's sections: a four-byte tag, then a big-endian
/// payload length, except for `END `.
#[cfg(feature = "parsers")]
fn gxa_palette(bytes: &[u8]) -> anyhow::Result<[[u8; 3]; 256]> {
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
        let payload = at + 8;
        if &bytes[at..at + 4] == b"BPAL" {
            let table = bytes
                .get(payload..payload + 768)
                .filter(|_| length >= 768)
                .ok_or_else(|| anyhow::anyhow!("logbook: LOGPAPER.GXA palette is truncated"))?;
            return Ok(std::array::from_fn(|index| {
                std::array::from_fn(|channel| table[index * 3 + channel] << 2)
            }));
        }
        if &bytes[at..at + 4] == b"END " {
            break;
        }
        at = payload + length;
    }
    anyhow::bail!("logbook: LOGPAPER.GXA has no palette")
}

#[cfg(feature = "parsers")]
/// Corner normals and brightness bytes the engine's model lighting reads for each face, following
/// preservation's formats/models/3d.md: with an indirection table each face vertex, counted across
/// the faces in order, selects its vertex normal; otherwise the vertex's own normal applies; an
/// unusable (non-normal-length) normal falls back to the face normal. The brightness byte is the
/// stored face byte shifted left two bits, low eight bits kept.
fn scroll_shading(model: &rgpre::import::model3d::Model3DFile) -> (Vec<Vec<[f32; 3]>>, Vec<u8>) {
    let mut counter = 0;
    let mut normals = Vec::new();
    for (face_index, face) in model.face_data.iter().enumerate() {
        let face_normal = model
            .face_normals
            .get(face_index)
            .map_or([0.0; 3], |n| [n.x, n.y, n.z]);
        normals.push(
            face.face_vertices
                .iter()
                .map(|corner| {
                    let selected = if model.normal_indices.is_empty() {
                        Some(corner.vertex_index as usize)
                    } else {
                        model.normal_indices.get(counter).map(|&i| i as usize)
                    };
                    counter += 1;
                    selected
                        .and_then(|index| model.vertex_normals.get(index))
                        .filter(|n| (n.x * n.x + n.y * n.y + n.z * n.z).is_normal())
                        .map_or(face_normal, |n| [n.x, n.y, n.z])
                })
                .collect(),
        );
    }
    let brightness = model
        .face_data
        .iter()
        .map(|face| face.tex_hi.wrapping_mul(4))
        .collect();
    (normals, brightness)
}

#[cfg(feature = "parsers")]
pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use rgpre::import::{
        bsi::parse_bsi_file,
        fnt::parse_fnt,
        gxa::parse_gxa_file,
        model3d::{TextureData, parse_3d_file},
        palette::Palette,
        rob::parse_rob_file,
    };
    let read = |path: &str| {
        crate::preparation::read(source, path).with_context(|| format!("logbook: {path}"))
    };
    let paper_bytes = read("SYSTEM/LOGPAPER.GXA")?;
    // The reader draws the paper and the scrolls with the paper file's own palette, so the scroll
    // textures take their colours from it, not from REDGUARD.COL.
    let reader_palette = Palette {
        colors: gxa_palette(&paper_bytes)?,
    };
    let paper = parse_gxa_file(&paper_bytes)?
        .frames
        .into_iter()
        .next()
        .context("logbook: LOGPAPER.GXA has no frames")?;
    let archive = parse_rob_file(&read("fxart/MENU.ROB")?)?;
    let segment = archive
        .segments
        .iter()
        .find(|s| s.name().eq_ignore_ascii_case("SCROLL"))
        .context("logbook: MENU.ROB lacks SCROLL.3D")?;
    let model = parse_3d_file(&segment.data)?;
    let palette = Palette::parse(&read("fxart/REDGUARD.COL")?)?;
    let mut textures = Vec::new();
    let mut references = std::collections::BTreeMap::new();
    for face in &model.face_data {
        if let TextureData::Texture { texture_id, .. } = face.texture_data {
            if references.keys().any(|(bank, _)| *bank == texture_id) {
                continue;
            }
            let bank = parse_bsi_file(&read(&format!("fxart/TEXBSI.{texture_id:03}"))?)?;
            for (index, image) in bank.images.iter().enumerate() {
                references.insert((texture_id, index as u16), textures.len());
                textures.push(PageTexture {
                    width: image.width,
                    height: image.height,
                    indices: image.pixel_data.clone(),
                    rgba: image.decode_rgba(Some(&reader_palette)),
                });
            }
        }
    }
    let system = rgpre::import::system_ini::SystemIni::parse(&crate::preparation::text(
        source,
        "SYSTEM.INI",
    )?)
    .map_err(anyhow::Error::msg)?;
    let glide = system.glide;
    let low_font = system
        .system
        .gui_low_font
        .context("logbook: SYSTEM.INI [system] gui_low_font missing")?
        .replace('\\', "/");
    let fonts = [
        "fonts/FONTNORS.FNT",
        "fonts/FONTSELS.FNT",
        "fonts/REDSEL.FNT",
        "fonts/FONTNORM.FNT",
        &low_font,
    ]
    .into_iter()
    .map(|path| -> anyhow::Result<_> { Ok(cyrus_text::Font::from(&parse_fnt(&read(path)?)?)) })
    .collect::<anyhow::Result<Vec<_>>>()?;
    let pair = |value: Option<[i32; 2]>, key: &str| -> anyhow::Result<[u8; 2]> {
        let [a, b] = value.with_context(|| format!("logbook: SYSTEM.INI [3dfx] {key} missing"))?;
        Ok([u8::try_from(a)?, u8::try_from(b)?])
    };
    let (sel, norm, used) = (
        pair(glide.font_sel, "font_sel")?,
        pair(glide.font_norm, "font_norm")?,
        pair(glide.font_used, "font_used")?,
    );
    // Indices follow preservation's reader style table; 5 has no initialised entry in the original.
    // The reader sets the alpha of the small fonts behind 7 and 9 to 200 and 255.
    let style = |font, [scale, alpha]: [u8; 2]| {
        Some(ReaderStyle {
            font,
            scale,
            alpha,
            page_palette: font == 4,
        })
    };
    let styles = vec![
        style(2, sel),
        style(4, [255, 255]),
        style(4, [255, 255]),
        style(2, norm),
        style(2, used),
        None,
        style(2, [255, 180]),
        style(0, [norm[0], 200]),
        style(3, [used[0], 50]),
        style(1, [sel[0], 255]),
    ];
    let (scroll_normals, scroll_brightness) = scroll_shading(&model);
    let artwork = LogbookArtwork {
        paper: MenuPicture {
            width: paper.width,
            height: paper.height,
            rgba: paper.rgba,
        },
        scroll_normals,
        scroll_brightness,
        scroll: crate::startup::menu_export::export_model(model, [0.; 3], &references, &palette)?,
        textures,
        palette: palette.colors.to_vec(),
        fonts,
        styles,
        credits: source
            .manifest()
            .find("CREDITS.TXT")
            .map(|file| source.read(&file.path))
            .transpose()
            .context("logbook: CREDITS.TXT")?
            .map(|bytes| parse_credits(&bytes)),
        credits_exported: true,
    };
    artwork.validate()?;
    output.publish(
        std::path::Path::new("logbook/reader.logbook"),
        &serde_json::to_vec(&artwork)?,
    )
}

#[cfg(test)]
mod credits_tests {
    use super::*;

    #[test]
    fn records_follow_the_original_loader() {
        let lines = parse_credits(
            b"320,C,5,TEAM\r\n\r\n-100,r,3,Name\r\n50,x,0,Plain\r\n\x1aignored,C,0,x",
        );
        // The line feed after the last line leaves one more, empty record, as in the original.
        assert_eq!(lines.len(), 5);
        assert_eq!(
            lines[0],
            CreditLine {
                x: 320,
                continued: false,
                align: CreditAlign::Centre,
                style: 5,
                text: Some(b"TEAM".to_vec()),
            }
        );
        // A blank line keeps the previous style and draws nothing.
        assert_eq!((lines[1].style, lines[1].text.clone()), (5, None));
        assert!(lines[2].continued && lines[2].align == CreditAlign::Right);
        assert_eq!((lines[2].x, lines[2].style), (100, 3));
        assert_eq!(
            lines[3].align,
            CreditAlign::Left,
            "unknown letters stay left"
        );
    }

    #[cfg(feature = "parsers")]
    #[test]
    fn gxa_palette_is_found_by_walking_sections_and_widened_to_eight_bits() {
        let mut file = Vec::new();
        file.extend(b"BMHD");
        file.extend(34u32.to_be_bytes());
        file.extend([0u8; 34]);
        file.extend(b"BPAL");
        file.extend(768u32.to_be_bytes());
        file.extend((0..768).map(|i| (i % 64) as u8));
        file.extend(b"END ");
        let palette = gxa_palette(&file).unwrap();
        assert_eq!(palette[0], [0, 4, 8]);
        assert_eq!(palette[1], [12, 16, 20]);
        assert_eq!(palette[255][2], (767 % 64) as u8 * 4);
        // No palette section, or a truncated one, is an explicit error.
        assert!(gxa_palette(&file[..42]).is_err());
        assert!(gxa_palette(b"END ").is_err());
        assert!(gxa_palette(&file[..60]).is_err());
    }

    #[test]
    fn at_most_two_thousand_records() {
        assert_eq!(parse_credits(&b"\n".repeat(3000)).len(), 2000);
    }
}
