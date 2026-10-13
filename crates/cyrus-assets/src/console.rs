//! Original console paper and configured monochrome font, decoded through preservation.
use bevy::prelude::{Asset, TypePath};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
pub struct ConsoleArtwork {
    pub width: u16,
    pub height: u16,
    pub paper: Vec<u8>,
    pub font: cyrus_text::Font,
    pub palettes: std::collections::BTreeMap<u32, ConsolePalette>,
}
/// World palette and the sixteen GUI matches defined by preservation's console rules.
#[derive(Serialize, Deserialize)]
pub struct ConsolePalette {
    pub colors: Vec<[u8; 3]>,
    pub gui: [u8; 16],
}
impl ConsoleArtwork {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.width > 0
                && self.height > 0
                && self.paper.len() == usize::from(self.width) * usize::from(self.height) * 4,
            "console paper dimensions do not match its pixels"
        );
        anyhow::ensure!(
            self.font.line_height > 0,
            "console font has zero line height"
        );
        anyhow::ensure!(
            self.palettes
                .values()
                .all(|palette| palette.colors.len() == 256),
            "console world palette must contain 256 colors"
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
    let system = rgpre::import::system_ini::SystemIni::parse(&crate::preparation::text(
        source,
        "SYSTEM.INI",
    )?)?;
    let path = system
        .system
        .gui_low_font
        .context("SYSTEM.INI [system] gui_low_font missing")?;
    let font = cyrus_text::Font::from(
        &rgpre::import::fnt::parse_fnt(&crate::preparation::read(source, &path)?)
            .with_context(|| format!("console font {path}"))?,
    );
    let paper =
        rgpre::import::gxa::parse_gxa_file(&crate::preparation::read(source, "SYSTEM/PAPER.GXA")?)?
            .frames
            .into_iter()
            .next()
            .context("SYSTEM/PAPER.GXA has no frames")?;
    let worlds =
        rgpre::import::world_ini::WorldIni::parse(&crate::preparation::text(source, "WORLD.INI")?);
    let mut palettes = std::collections::BTreeMap::new();
    for world in worlds.entries {
        let name = world
            .palette
            .rsplit(['/', '\\'])
            .next()
            .context("empty world palette")?;
        let path = ["fxart", "3dart"]
            .iter()
            .find_map(|directory| source.manifest().find(&format!("{directory}/{name}")))
            .with_context(|| format!("world {}: missing palette {}", world.index, world.palette))?;
        let palette = rgpre::import::palette::Palette::parse(&source.read(&path.path)?)?;
        palettes.insert(
            world.index,
            ConsolePalette {
                colors: palette.colors.to_vec(),
                gui: std::array::from_fn(|index| {
                    if index == 0 {
                        0
                    } else {
                        gui_ink(&palette.colors, GUI_COLORS[index])
                    }
                }),
            },
        );
    }
    let artwork = ConsoleArtwork {
        width: paper.width,
        height: paper.height,
        paper: paper.rgba,
        font,
        palettes,
    };
    artwork.validate()?;
    output.publish(
        std::path::Path::new("console/paper.console"),
        &serde_json::to_vec(&artwork)?,
    )
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(
    install: &crate::install::GameInstall,
    cache: &std::path::Path,
) -> anyhow::Result<()> {
    install.cache_path(cache)?;
    prepare(
        &crate::native::Files::scan(install, false)?,
        &mut crate::preparation::DiskOutput(cache),
    )
}

/// GUI palette matching follows preservation's console presentation contract.
#[cfg(feature = "parsers")]
fn gui_ink(palette: &[[u8; 3]; 256], target: [i32; 3]) -> u8 {
    let weights = [19661, 38666, 7209];
    let candidates: Vec<_> = palette
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, rgb)| (index, rgb.map(|value| i32::from(value >> 2))))
        .collect();
    if let Some((index, _)) = candidates.iter().find(|(_, rgb)| *rgb == target) {
        return *index as u8;
    }
    let differences = |rgb: [i32; 3]| {
        std::array::from_fn::<_, 3, _>(|i| {
            if target[i] >= rgb[i] {
                target[i] - rgb[i]
            } else {
                rgb[i] - target[i] - 1
            }
        })
    };
    let eligible: Vec<_> = candidates
        .into_iter()
        .filter(|(_, rgb)| rgb.iter().sum::<i32>() != 189)
        .collect();
    let close = eligible
        .iter()
        .filter(|(_, rgb)| differences(*rgb).iter().all(|value| *value < 9))
        .map(|(index, rgb)| {
            (
                *index,
                (0..3)
                    .map(|i| (target[i] - rgb[i]) * weights[i])
                    .sum::<i32>()
                    .abs(),
            )
        })
        .min_by_key(|(index, score)| (*score, std::cmp::Reverse(*index)));
    let index = close
        .filter(|(_, score)| *score <= 24)
        .map(|(index, _)| index)
        .or_else(|| {
            eligible
                .iter()
                .min_by_key(|(index, rgb)| {
                    (
                        differences(*rgb).iter().sum::<i32>(),
                        std::cmp::Reverse(*index),
                    )
                })
                .map(|(index, _)| *index)
        })
        .unwrap_or(0);
    index as u8
}

/// Six-bit RGB targets; GUI index zero bypasses matching and uses palette entry zero.
#[cfg(feature = "parsers")]
const GUI_COLORS: [[i32; 3]; 16] = [
    [0, 0, 0],
    [62, 62, 62],
    [42, 42, 42],
    [32, 32, 32],
    [22, 22, 22],
    [12, 12, 12],
    [32, 51, 63],
    [19, 33, 44],
    [10, 17, 26],
    [53, 3, 23],
    [49, 0, 0],
    [60, 0, 0],
    [0, 30, 0],
    [0, 42, 0],
    [0, 0, 0],
    [24, 0, 28],
];
