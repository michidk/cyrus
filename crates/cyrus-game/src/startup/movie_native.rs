//! Read-only installed cinematic extraction and browser cache export.
use super::movie_data::{MovieDefinition, MovieMetadata, MovieSubtitle, Prepared};
use crate::install::GameInstall;
use anyhow::{Context, ensure};
use bevy::audio::AudioSource;
use rgpre::import::{
    cdrom::CdImage, fnt::parse_fnt, menu_movies::parse_menu_movies, sfx::AudioType,
    smacker::SmackerHeader,
};
use std::{fs::File, path::Path};

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
        std::fs::write(output.join(format!("{index}.smk")), &movie.data)?;
        if let Some(audio) = movie.audio {
            std::fs::write(output.join(format!("{index}.wav")), &audio.bytes)?;
        }
        std::fs::write(
            output.join(format!("{index}.movie")),
            serde_json::to_vec(&movie.metadata)?,
        )?;
    }
    Ok(())
}

pub(super) fn prepare(root: &Path, index: usize) -> anyhow::Result<Prepared> {
    let ini = std::fs::read_to_string(root.join("MENU.INI")).context("movie: reading MENU.INI")?;
    let definition = parse_menu_movies(&ini)
        .map_err(anyhow::Error::msg)?
        .into_iter()
        .nth(index)
        .with_context(|| format!("movie: MENU.INI definition {index} missing"))?;
    let system =
        std::fs::read_to_string(root.join("SYSTEM.INI")).context("movie: reading SYSTEM.INI")?;
    let name = &definition.name;
    let drive = crate::sound::ini_value(&system, "system", "animation_drive").unwrap_or("");
    let data = if drive.contains(':') {
        let prefix = drive.as_bytes();
        ensure!(
            prefix.len() >= 3
                && prefix[0].is_ascii_alphabetic()
                && prefix[1] == b':'
                && prefix[2..].iter().all(|b| matches!(*b, b'/' | b'\\')),
            "movie {name}: animation_drive {drive:?}: only the CD root is supported"
        );
        let disc = root
            .parent()
            .context("movie: install has no parent")?
            .join("game.gog");
        let mut image = CdImage::open(std::io::BufReader::new(
            File::open(&disc).with_context(|| format!("movie {name}: {}", disc.display()))?,
        ))?;
        let mut data = Vec::new();
        image
            .copy_file(name, &mut data)
            .with_context(|| format!("movie {name}: CD extraction"))?;
        data
    } else {
        let mut directory = root.to_path_buf();
        for part in drive.split(['/', '\\']).filter(|p| !p.is_empty()) {
            ensure!(
                part != "." && part != "..",
                "movie {name}: invalid animation_drive"
            );
            directory = crate::scene::find_file(&directory, part)
                .with_context(|| format!("movie {name}: animation_drive {drive:?} missing"))?;
        }
        let path = crate::scene::find_file(&directory, name)
            .with_context(|| format!("movie {name}: not found in {}", directory.display()))?;
        std::fs::read(&path).with_context(|| format!("movie {name}: reading {}", path.display()))?
    };
    let header = SmackerHeader::parse(&data)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("movie {name}: Smacker header"))?;
    ensure!(
        header.frames < u32::MAX,
        "movie {name}: frame count exceeds playback counter capacity"
    );
    ensure!(
        header.width == 640 && header.height == 240 && header.double_height,
        "movie {name}: unsupported viewport {}x{}",
        header.width,
        header.height
    );
    ensure!(
        definition.keys.iter().all(|key| *key <= header.frames),
        "movie {name}: skip point exceeds frame count"
    );
    let font_name = crate::sound::ini_value(&system, "system", "gui_low_font")
        .context("movie: SYSTEM.INI gui_low_font missing")?;
    let mut font_path = root.to_path_buf();
    for part in font_name.split(['/', '\\']) {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "movie: invalid gui_low_font path"
        );
        font_path = crate::scene::find_file(&font_path, part)
            .with_context(|| format!("movie: gui_low_font {font_name} missing"))?;
    }
    let font = cyrus_text::Font::from(
        &parse_fnt(&std::fs::read(&font_path)?).context("movie: gui_low_font")?,
    );
    let line_height = font
        .glyph(b'W')
        .filter(|g| g.width > 0)
        .context("movie: gui_low_font has no W glyph for line spacing")?
        .width;
    let palette =
        rgpre::import::palette::Palette::parse(&std::fs::read(root.join("fxart/REDGUARD.COL"))?)
            .context("movie: REDGUARD.COL")?;
    let font_color = palette.colors[161];
    let audio = if header.has_audio {
        Some(decode_audio(&data).with_context(|| format!("movie {name}: audio decode"))?)
    } else {
        None
    };
    let prepared = Prepared {
        data: data.into(),
        metadata: MovieMetadata {
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
            line_height,
            font_color,
        },
        audio,
    };
    prepared.metadata.validate()?;
    Ok(prepared)
}

/// The first audio track as WAV.
fn decode_audio(data: &[u8]) -> anyhow::Result<AudioSource> {
    let mut smk = smk::Smk::open_memory(data)?;
    let info = smk.info_audio();
    ensure!(info.track_mask & 1 != 0, "no first audio track");
    let audio_type = match (info.channels[0], info.bitdepth[0]) {
        (1, 8) => AudioType::Mono8,
        (1, 16) => AudioType::Mono16,
        (2, 8) => AudioType::Stereo8,
        (2, 16) => AudioType::Stereo16,
        other => anyhow::bail!("unsupported audio layout {other:?}"),
    };
    smk.enable_all(1);
    let mut pcm = Vec::new();
    let mut status = smk.first_frame()?;
    loop {
        pcm.extend_from_slice(smk.audio_data(0).unwrap_or_default());
        if status != smk::FrameStatus::More {
            break;
        }
        status = smk.next_frame()?;
    }
    let bytes =
        crate::sound::pcm_wav(audio_type, info.rate[0], &pcm).map_err(anyhow::Error::msg)?;
    Ok(AudioSource {
        bytes: bytes.into(),
    })
}
