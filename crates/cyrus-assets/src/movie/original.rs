//! Installed cinematic metadata and bounded original-file reads on either target.
use super::{MovieDefinition, MovieMetadata, MovieSubtitle};
use crate::source::{AssetFile, ReadAssets};
use anyhow::{Context, Result, ensure};
use rgpre::import::{
    cdrom::{CdFile, CdImage},
    fnt::parse_fnt,
    menu_movies::parse_menu_movies,
    smacker::SmackerHeader,
};

pub enum Location {
    File { path: String, size: u64 },
    Disc { path: String, file: CdFile },
}
pub struct OriginalMovie {
    pub metadata: MovieMetadata,
    pub location: Location,
}
impl Location {
    pub fn size(&self) -> u64 {
        match self {
            Location::File { size, .. } => *size,
            Location::Disc { file, .. } => file.size,
        }
    }
    /// Read a compressed payload range without retaining earlier movie packets.
    pub fn read_range(
        &self,
        source: &dyn ReadAssets,
        start: u64,
        length: usize,
    ) -> Result<Vec<u8>> {
        match self {
            Location::File { path, .. } => source.read_range(path, start, length),
            Location::Disc { path, file } => Ok(file.read_range(
                &mut AssetFile::open(source, path)?.without_read_ahead(),
                start,
                length,
            )?),
        }
    }
}
impl OriginalMovie {
    pub fn size(&self) -> u64 {
        self.location.size()
    }
    pub fn read_range(
        &self,
        source: &dyn ReadAssets,
        start: u64,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.location.read_range(source, start, length)
    }
}

/// Validate the authored drive, viewport, skip points and subtitle presentation.
pub fn prepare(source: &dyn ReadAssets, index: usize) -> Result<OriginalMovie> {
    let ini = crate::preparation::text(source, "MENU.INI")?;
    let definition = parse_menu_movies(&ini)
        .map_err(anyhow::Error::msg)?
        .into_iter()
        .nth(index)
        .with_context(|| format!("movie: MENU.INI definition {index} missing"))?;
    let system = crate::preparation::text(source, "SYSTEM.INI")?;
    let name = &definition.name;
    let drive = crate::sound::ini_value(&system, "system", "animation_drive").unwrap_or("");
    let location = if drive.contains(':') {
        let prefix = drive.as_bytes();
        ensure!(
            prefix.len() >= 3
                && prefix[0].is_ascii_alphabetic()
                && prefix[1] == b':'
                && prefix[2..].iter().all(|b| matches!(*b, b'/' | b'\\')),
            "movie {name}: animation_drive {drive:?}: only the CD root is supported"
        );
        let path = crate::preparation::path(source, "disc/game.gog")?;
        let file =
            CdImage::open(AssetFile::open(source, &path)?.without_read_ahead())?.file(name)?;
        Location::Disc { path, file }
    } else {
        let path = if drive.is_empty() {
            name.clone()
        } else {
            format!("{}/{name}", drive.replace('\\', "/").trim_end_matches('/'))
        };
        let path = crate::preparation::path(source, &path)?;
        let size = source
            .manifest()
            .find(&path)
            .context("movie: original absent")?
            .bytes;
        Location::File { path, size }
    };
    let header =
        SmackerHeader::parse(&location.read_range(source, 0, 104)?).map_err(anyhow::Error::msg)?;
    ensure!(
        header.frames < u32::MAX
            && header.width == 640
            && header.height == 240
            && header.double_height,
        "movie {}: unsupported viewport/frame count",
        definition.name
    );
    // The player uses its own copy of the startup REDSEL font, not a SYSTEM.INI font.
    let font = cyrus_text::Font::from(&parse_fnt(&crate::preparation::read(
        source,
        "fonts/REDSEL.FNT",
    )?)?);
    let metadata = MovieMetadata {
        definition: MovieDefinition {
            name: definition.name,
            keys: definition.keys,
            subtitles: definition
                .subtitles
                .into_iter()
                .map(|s| MovieSubtitle {
                    always: s.always,
                    start: s.start,
                    stop: s.stop,
                    text: s.text,
                })
                .collect(),
        },
        frames: header.frames,
        frame_micros: header.frame_micros,
        has_audio: header.has_audio,
        font,
    };
    metadata.validate()?;
    Ok(OriginalMovie { metadata, location })
}
