//! Read-only CD and world track preparation.
use crate::{audio::pcm_wav, disc::cue_track, install::GameInstall, sound::ini_value};
use rgpre::import::sfx::AudioType;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
const RED_BOOK_RATE: u32 = 44_100;

/// `world_redbook` of the world a scene name resolves to; `-1` when absent.
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
pub fn settings(install: &GameInstall) -> anyhow::Result<crate::sound::MusicSettings> {
    let settings = crate::audio::read_settings(install);
    let path = rgpre::import::world_ini::find_world_ini(&install.root)
        .ok_or_else(|| anyhow::anyhow!("music: missing WORLD.INI"))?;
    let worlds = rgpre::import::world_ini::WorldIni::parse(&std::fs::read_to_string(path)?);
    let world_tracks = worlds
        .entries
        .iter()
        .map(|entry| Ok((entry.index, world_track(install, &entry.index.to_string())?)))
        .collect::<anyhow::Result<_>>()?;
    Ok(crate::sound::MusicSettings {
        enabled: settings.music,
        volume: settings.redbook_volume,
        world_tracks,
    })
}

/// Export menu and world CD tracks as WAV files without modifying the install.
pub fn export(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let settings = settings(install)?;
    if !settings.enabled {
        return Ok(());
    }
    let cue = cue_path(install)
        .ok_or_else(|| anyhow::anyhow!("music: no game.ins beside {}", install.root.display()))?;
    let tracks: std::collections::BTreeSet<_> = settings
        .world_tracks
        .values()
        .filter_map(|&value| u32::try_from(value).ok().filter(|&track| track > 0))
        .chain([2])
        .collect();
    let out = cache.join("music");
    std::fs::create_dir_all(&out)?;
    for track in tracks {
        crate::native::publish(
            &out.join(format!("{track:02}.wav")),
            &read_track(&cue, track).map_err(anyhow::Error::msg)?,
        )?;
    }
    Ok(())
}

/// Reads one audio track from the image a cue sheet names, as a playable WAV source.
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

pub fn cue_path(install: &GameInstall) -> Option<PathBuf> {
    let path = install.root.parent()?.join("game.ins");
    path.is_file().then_some(path)
}

#[cfg(test)]
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
