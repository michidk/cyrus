//! Read-only CD and world track preparation.
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
    Ok(crate::sound::MusicSettings {
        enabled: settings.music,
        volume: settings.redbook_volume,
        world_tracks,
    })
}

pub(crate) fn prepare_track(
    source: &dyn crate::source::ReadAssets,
    path: &str,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let track = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .context("invalid music track")?
        .parse::<u32>()?;
    let cue = crate::preparation::text(source, "disc/game.ins")?;
    let (name, start, end) = cue_track(&cue, track, true).map_err(anyhow::Error::msg)?;
    let image = crate::preparation::path(source, &format!("disc/{name}"))?;
    let info = source.manifest().find(&image).context("missing CD image")?;
    let end = end.unwrap_or(info.bytes);
    anyhow::ensure!(
        start <= end && end <= info.bytes,
        "CD track {track}: invalid byte range"
    );
    let pcm = source.read_range(&image, start, usize::try_from(end - start)?)?;
    let bytes = cyrus_media::pcm_wav(
        &pcm,
        RED_BOOK_RATE,
        AudioType::Stereo16.channels(),
        AudioType::Stereo16.bits_per_sample(),
    )?;
    output.publish(Path::new(path), &bytes)
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

/// Reads one audio track from the image a cue sheet names, as a playable WAV source.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn read_track(cue_path: &Path, track: u32) -> Result<Vec<u8>, String> {
    let context = |e: &dyn std::fmt::Display| format!("{} track {track}: {e}", cue_path.display());
    let cue = std::fs::read_to_string(cue_path).map_err(|e| context(&e))?;
    let (name, start, end) = cue_track(&cue, track, true).map_err(|e| context(&e))?;
    let image = cue_path.with_file_name(name);
    let mut file = std::fs::File::open(&image).map_err(|e| context(&e))?;
    let end = match end {
        Some(end) => end,
        None => file.metadata().map_err(|e| context(&e))?.len(),
    };
    let len = end
        .checked_sub(start)
        .and_then(|l| usize::try_from(l).ok())
        .ok_or_else(|| context(&"track ends before it starts"))?;
    let mut pcm = vec![0; len];
    file.seek(SeekFrom::Start(start))
        .and_then(|_| file.read_exact(&mut pcm))
        .map_err(|e| context(&e))?;
    let bytes = pcm_wav(AudioType::Stereo16, RED_BOOK_RATE, &pcm).map_err(|e| context(&e))?;
    Ok(bytes)
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn cue_path(install: &GameInstall) -> Option<PathBuf> {
    let path = install.root.parent()?.join("game.ins");
    path.is_file().then_some(path)
}

#[cfg(all(test, feature = "native", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::disc::SECTOR_BYTES;
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
