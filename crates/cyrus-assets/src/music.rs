//! Read-only CD and world track preparation, from GOG's disc image or a CD edition's ripped soundtrack.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
use crate::{audio::pcm_wav, install::GameInstall};
use crate::{disc::cue_track, sound::ini_value};
use anyhow::Context;
use rgpre::import::sfx::AudioType;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
use std::path::PathBuf;
const RED_BOOK_RATE: u32 = 44_100;

pub(crate) fn settings_source(
    source: &dyn crate::source::ReadAssets,
) -> anyhow::Result<crate::sound::MusicSettings> {
    let settings =
        crate::sound::SoundSettings::parse(&crate::preparation::text(source, "SYSTEM.INI")?);
    let ini = crate::preparation::text(source, "WORLD.INI")?;
    let worlds = rgpre::import::world_ini::WorldIni::parse(&ini);
    let world_tracks = worlds
        .entries
        .iter()
        .map(|entry| {
            let key = format!("world_redbook[{}]", entry.index);
            let track = ini_value(&ini, "world", &key)
                .unwrap_or("-1")
                .parse::<i32>()
                .with_context(|| format!("WORLD.INI {key}"))?;
            Ok((entry.index, track))
        })
        .collect::<anyhow::Result<_>>()?;
    let mut world_names = std::collections::BTreeMap::new();
    for entry in &worlds.entries {
        let filename = entry.map.rsplit(['/', '\\']).next().unwrap_or_default();
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .with_context(|| {
                format!(
                    "WORLD.INI world_map[{}]: invalid map {}",
                    entry.index, entry.map
                )
            })?;
        world_names
            .entry(stem.to_ascii_uppercase())
            .or_insert(entry.index);
    }
    Ok(crate::sound::MusicSettings {
        enabled: settings.music,
        volume: settings.redbook_volume,
        world_tracks,
        world_names,
    })
}

pub(crate) fn prepare_track(
    source: &dyn crate::source::ReadAssets,
    path: &str,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let track = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .context("invalid music track")?
        .parse::<u32>()?;
    let bytes = cyrus_media::pcm_wav(
        &track_pcm(source, track)?,
        RED_BOOK_RATE,
        AudioType::Stereo16.channels(),
        AudioType::Stereo16.bits_per_sample(),
    )?;
    output.publish(Path::new(path), &bytes)
}

/// Red Book PCM of one CD audio track: from the cue sheet's image (`disc/game.ins`), or,
/// for a CD install without one, from its ripped soundtrack (`disc/trackNN.mp3`).
pub(crate) fn track_pcm(
    source: &dyn crate::source::ReadAssets,
    track: u32,
) -> anyhow::Result<Vec<u8>> {
    let Some(cue) = source.manifest().find("disc/game.ins") else {
        let mp3 = crate::preparation::path(source, &format!("disc/track{track:02}.mp3"))
            .with_context(|| format!("CD track {track}: no game.ins and no soundtrack MP3"))?;
        return mp3_pcm(source.read(&mp3)?).with_context(|| mp3);
    };
    let cue = source.text(&cue.path)?;
    let (name, start, end) = cue_track(&cue, track, true).map_err(anyhow::Error::msg)?;
    crate::validate_path(&name)?;
    let image = crate::preparation::path(source, &format!("disc/{name}"))?;
    let info = source.manifest().find(&image).context("missing CD image")?;
    let end = end.unwrap_or(info.bytes);
    anyhow::ensure!(
        start <= end && end <= info.bytes,
        "CD track {track}: invalid byte range"
    );
    source.read_range(&image, start, usize::try_from(end - start)?)
}

/// The CD audio track a ripped soundtrack file holds: `NN Track N.mp3` is track NN+1,
/// because CD track 1 is data. shortcut: only this naming is recognised; add others when an edition needs them.
pub(crate) fn soundtrack_track(file_name: &str) -> Option<u32> {
    let name = file_name.to_ascii_lowercase();
    let number: u32 = name.strip_suffix(".mp3")?.get(..2)?.parse().ok()?;
    (number > 0).then_some(number + 1)
}

/// Decodes a 44.1 kHz stereo MP3 to the interleaved 16-bit little-endian PCM a CD track holds.
fn mp3_pcm(bytes: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
        io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
    };
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let options = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let mut format = symphonia::default::get_probe()
        .format(
            Hint::new().with_extension("mp3"),
            stream,
            &options,
            &MetadataOptions::default(),
        )?
        .format;
    let params = format
        .default_track()
        .context("no audio stream")?
        .codec_params
        .clone();
    anyhow::ensure!(
        params.sample_rate == Some(RED_BOOK_RATE) && params.channels.map(|c| c.count()) == Some(2),
        "not 44.1 kHz stereo"
    );
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;
    let mut pcm = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        let decoded = decoder.decode(&packet)?;
        let mut samples = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        pcm.extend(samples.samples().iter().flat_map(|s| s.to_le_bytes()));
    }
    Ok(pcm)
}

/// `world_redbook` of the world a scene name resolves to; `-1` when absent.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn world_track(install: &GameInstall, scene: &str) -> anyhow::Result<i32> {
    let index = crate::scene::world_index(install, scene)?;
    let path = rgpre::import::world_ini::find_world_ini(&install.root)
        .ok_or_else(|| anyhow::anyhow!("missing WORLD.INI"))?;
    let ini = std::fs::read_to_string(path)?;
    let Some(value) = ini_value(&ini, "world", &format!("world_redbook[{index}]")) else {
        return Ok(-1);
    };
    value
        .parse()
        .map_err(|e| anyhow::anyhow!("world {index}: world_redbook {value}: {e}"))
}

/// Installed music settings and WORLD.INI track assignments for a portable sound bank.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn settings(install: &GameInstall) -> anyhow::Result<crate::sound::MusicSettings> {
    settings_source(&crate::native::Files::scan(install, false)?)
}

/// Export menu and world CD tracks as WAV files without modifying the install.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let source = crate::native::Files::scan(install, false)?;
    let settings = settings_source(&source)?;
    if !settings.enabled {
        return Ok(());
    }
    let tracks: std::collections::BTreeSet<_> = settings
        .world_tracks
        .values()
        .filter_map(|&value| u32::try_from(value).ok().filter(|&track| track > 0))
        .chain([2])
        .collect();
    let mut output = crate::preparation::DiskOutput(cache);
    for track in tracks {
        prepare_track(&source, &format!("music/{track:02}.wav"), &mut output)?;
    }
    Ok(())
}

/// Reads one audio track from `music_source` as a playable WAV source.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn read_track(source: &Path, track: u32) -> Result<Vec<u8>, String> {
    let context = |e: &dyn std::fmt::Display| format!("{} track {track}: {e}", source.display());
    let pcm = if source.is_dir() {
        let file = std::fs::read_dir(source)
            .map_err(|e| context(&e))?
            .filter_map(Result::ok)
            .find(|entry| soundtrack_track(&entry.file_name().to_string_lossy()) == Some(track))
            .ok_or_else(|| context(&"no soundtrack MP3"))?;
        let bytes = std::fs::read(file.path()).map_err(|e| context(&e))?;
        mp3_pcm(bytes).map_err(|e| context(&e))?
    } else {
        cue_pcm(source, track).map_err(|e| context(&e))?
    };
    pcm_wav(AudioType::Stereo16, RED_BOOK_RATE, &pcm).map_err(|e| context(&e))
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
fn cue_pcm(cue_path: &Path, track: u32) -> anyhow::Result<Vec<u8>> {
    let cue = std::fs::read_to_string(cue_path)?;
    let (name, start, end) = cue_track(&cue, track, true).map_err(anyhow::Error::msg)?;
    let mut file = std::fs::File::open(cue_path.with_file_name(name))?;
    let end = match end {
        Some(end) => end,
        None => file.metadata()?.len(),
    };
    let len = end
        .checked_sub(start)
        .and_then(|l| usize::try_from(l).ok())
        .context("track ends before it starts")?;
    let mut pcm = vec![0; len];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut pcm)?;
    Ok(pcm)
}

/// Where the CD audio comes from: GOG's cue sheet `game.ins` beside the install, else the
/// `Soundtrack/` directory of ripped MP3s that the CD-edition repack ships, placed beside it.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn music_source(install: &GameInstall) -> Option<PathBuf> {
    let parent = install.root.parent()?;
    let cue = parent.join("game.ins");
    if cue.is_file() {
        return Some(cue);
    }
    let soundtrack = parent.join("Soundtrack");
    soundtrack.is_dir().then_some(soundtrack)
}

#[cfg(all(test, feature = "native", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::disc::SECTOR_BYTES;
    #[test]
    fn soundtrack_files_skip_the_data_track() {
        assert_eq!(soundtrack_track("01 Track 1.mp3"), Some(2));
        assert_eq!(soundtrack_track("06 TRACK 6.MP3"), Some(7));
        for other in ["00 Track 0.mp3", "Soundtrack.jpg", "1.mp3", "xx.mp3"] {
            assert_eq!(soundtrack_track(other), None, "{other}");
        }
    }
    #[test]
    fn browser_export_retains_installed_settings_and_menu_and_world_audio() {
        let root = std::env::temp_dir().join(format!("cyrus-music-export-{}", std::process::id()));
        let install = GameInstall {
            root: root.join("Redguard"),
        };
        std::fs::create_dir_all(&install.root).unwrap();
        std::fs::write(
            install.root.join("SYSTEM.INI"),
            "[system]\nredbook=on\nredbook_volume=64\n",
        )
        .unwrap();
        std::fs::write(install.root.join("WORLD.INI"), "[world]\nworld_map[0]=MAPS/START.RGM\nworld_palette[0]=ART/COL.COL\nworld_redbook[0]=3\n").unwrap();
        std::fs::write(root.join("game.ins"), "FILE \"game.gog\" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:00:01\nTRACK 03 AUDIO\nINDEX 01 00:00:02\n").unwrap();
        std::fs::write(root.join("game.gog"), vec![7; 3 * SECTOR_BYTES as usize]).unwrap();
        let music = settings(&install).unwrap();
        assert!(music.enabled);
        assert_eq!(music.volume, 64);
        assert_eq!(music.world_tracks, [(0, 3)].into());
        assert_eq!(music.track_for_scene("start"), Some(3));
        assert_eq!(music.track_for_scene("0"), Some(3));
        for invalid in ["", "missing", "../START", "START.RGM"] {
            assert_eq!(music.track_for_scene(invalid), None);
        }
        std::fs::write(install.root.join("WORLD.INI"), "[world]\nworld_map[0]=MAPS\\ROOM.RGM\nworld_palette[0]=ART/COL.COL\nworld_redbook[0]=3\nworld_map[9]=MAPS/room.RGM\nworld_palette[9]=ART/COL.COL\nworld_redbook[9]=2\n").unwrap();
        let variants = settings(&install).unwrap();
        assert_eq!(variants.track_for_scene("ROOM"), Some(3));
        assert_eq!(variants.track_for_scene("9"), Some(2));
        let cache = root.join("cache");
        export(&install, &cache).unwrap();
        for track in ["02.wav", "03.wav"] {
            let bytes = std::fs::read(cache.join("music").join(track)).unwrap();
            assert_eq!(&bytes[..4], b"RIFF");
            assert_eq!(&bytes[44..], vec![7; SECTOR_BYTES as usize]);
        }
        std::fs::write(install.root.join("SYSTEM.INI"), "[system]\nredbook=off\n").unwrap();
        assert!(!settings(&install).unwrap().enabled);
        export(&install, &root.join("disabled-cache")).unwrap();
        assert!(!root.join("disabled-cache").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
