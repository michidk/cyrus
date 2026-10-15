//! Installed logbook paper, fonts and scroll geometry for native and browser readers.
use crate::menu::{MenuModel, MenuPicture, PageTexture};
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct LogbookArtwork {
    pub paper: MenuPicture,
    pub scroll: MenuModel,
    pub textures: Vec<PageTexture>,
    pub palette: Vec<[u8; 3]>,
    pub fonts: [cyrus_text::Font; 2],
    pub bindings: [u8; 15],
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
            self.textures.iter().all(|t| t.width > 0
                && t.height > 0
                && t.rgba.len() == usize::from(t.width) * usize::from(t.height) * 4),
            "logbook: invalid scroll texture dimensions"
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
    let paper = parse_gxa_file(&read("SYSTEM/LOGPAPER.GXA")?)?
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
                    rgba: image.decode_rgba(Some(&palette)),
                });
            }
        }
    }
    let fonts = ["FONTNORS", "FONTSELS"].map(|name| -> anyhow::Result<_> {
        Ok(cyrus_text::Font::from(&parse_fnt(&read(&format!(
            "fonts/{name}.FNT"
        ))?)?))
    });
    let [normal, selected] = fonts;
    let bindings =
        rgpre::import::keys_ini::KeysIni::parse(&crate::preparation::text(source, "KEYS.INI")?)
            .map_err(anyhow::Error::msg)?
            .control_codes();
    let artwork = LogbookArtwork {
        paper: MenuPicture {
            width: paper.width,
            height: paper.height,
            rgba: paper.rgba,
        },
        scroll: crate::startup::menu_export::export_model(model, [0.; 3], &references, &palette)?,
        textures,
        palette: palette.colors.to_vec(),
        fonts: [normal?, selected?],
        bindings,
    };
    artwork.validate()?;
    output.publish(
        std::path::Path::new("logbook/reader.logbook"),
        &serde_json::to_vec(&artwork)?,
    )
}
