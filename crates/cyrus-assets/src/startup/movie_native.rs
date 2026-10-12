//! Read-only installed cinematic extraction and browser cache export.
use crate::install::GameInstall;
use crate::movie::{MovieInput, Prepared};
use anyhow::{Context, ensure};
use rgpre::import::cdrom::{CdFile, CdImage};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Export the browsable installed cinematics for on-demand browser loading.
/// Source files remain read-only; compressed video and decoded audio stay in the local cache.
pub fn export_movies(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let ini = std::fs::read_to_string(install.root.join("MENU.INI"))?;
    let page = rgpre::import::menu_ini::MenuPage::parse(&ini, 3).map_err(anyhow::Error::msg)?;
    let output = cache.join("movies");
    std::fs::create_dir_all(&output)?;
    for entry in page
        .entries
        .iter()
        .filter(|e| (301..=305).contains(&e.action))
    {
        let index = (entry.action - 301) as usize;
        let movie = prepare(&install.root, index)?;
        let mut data = Vec::new();
        movie.source.open()?.read_to_end(&mut data)?;
        let audio = if movie.metadata.has_audio {
            Some(decode_audio(&data).with_context(|| {
                format!("movie {}: audio export", movie.metadata.definition.name)
            })?)
        } else {
            None
        };
        crate::native::publish(&output.join(format!("{index}.smk")), &data)?;
        if let Some(audio) = audio {
            crate::native::publish(&output.join(format!("{index}.wav")), &audio)?;
        }
        crate::native::publish(
            &output.join(format!("{index}.movie")),
            &serde_json::to_vec(&movie.metadata)?,
        )?;
    }
    Ok(())
}

pub fn prepare(root: &Path, index: usize) -> anyhow::Result<Prepared> {
    let install = GameInstall::open(root)?;
    let files = crate::native::Files::scan(&install, false)?;
    let original = crate::movie::original::prepare(&files, index)?;
    let source = match original.location {
        crate::movie::original::Location::File { path, .. } => {
            MovieInput::File(install.root.join(path))
        }
        crate::movie::original::Location::Disc { path, .. } => MovieInput::Disc {
            path: install
                .root
                .parent()
                .context("movie: install has no parent")?
                .join(
                    path.strip_prefix("disc/")
                        .context("movie: invalid disc path")?,
                ),
            name: original.metadata.definition.name.clone(),
        },
    };
    Ok(Prepared {
        source,
        metadata: original.metadata,
    })
}

/// The first audio track as WAV.
fn decode_audio(data: &[u8]) -> anyhow::Result<std::sync::Arc<[u8]>> {
    let (info, pcm) = cyrus_media::audio_tracks(io::Cursor::new(data), 1, |_, _| {})?;
    ensure!(info.track_mask & 1 != 0, "no first audio track");
    Ok(cyrus_media::pcm_wav(
        &pcm[0],
        info.rate[0],
        info.channels[0].into(),
        info.bitdepth[0].into(),
    )?
    .into())
}

impl MovieInput {
    /// Open a bounded sequential reader. Dropping it closes the file; no background extraction remains.
    pub fn open(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        match self {
            Self::File(path) => Ok(Box::new(io::BufReader::new(
                File::open(path).with_context(|| format!("movie: {}", path.display()))?,
            ))),
            Self::Disc { path, name } => {
                let file = File::open(path)
                    .with_context(|| format!("movie {name}: {}", path.display()))?;
                let mut reader = io::BufReader::new(file);
                let member = CdImage::open(&mut reader)?.file(name)?;
                Ok(Box::new(MovieReader {
                    reader,
                    member,
                    offset: 0,
                }))
            }
        }
    }
}
struct MovieReader {
    reader: io::BufReader<File>,
    member: CdFile,
    offset: u64,
}
impl Read for MovieReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let length = bytes
            .len()
            .min((self.member.size - self.offset) as usize)
            .min(64 * 1024);
        let chunk = self
            .member
            .read_range(&mut self.reader, self.offset, length)?;
        bytes[..length].copy_from_slice(&chunk);
        self.offset += length as u64;
        Ok(length)
    }
}
