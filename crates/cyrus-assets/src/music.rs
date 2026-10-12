//! Read-only CD and world track preparation.
use crate::{audio::pcm_wav, install::GameInstall, sound::ini_value};
use rgpre::import::sfx::AudioType;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
/// Red Book raw sector size in bytes; audio is 44.1 kHz 16-bit stereo, 75 sectors per second.
const SECTOR_BYTES: u64 = 2352;
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
        let path = out.join(format!("{track:02}.wav"));
        let temporary = path.with_extension("wav.tmp");
        std::fs::write(
            &temporary,
            read_track(&cue, track).map_err(anyhow::Error::msg)?,
        )?;
        std::fs::rename(temporary, path)?;
    }
    Ok(())
}

/// Byte range of `track` in the single `BINARY` file a cue sheet names:
/// (file name, start, end). `audio` states whether the track must be audio or data.
/// `end` is `None` for the last track, which runs to the end of the file.
pub fn cue_track(cue: &str, track: u32, audio: bool) -> Result<(String, u64, Option<u64>), String> {
    let mut file = None;
    // (number, is audio, first index sector, INDEX 01 sector)
    let mut tracks: Vec<(u32, bool, Option<u64>, Option<u64>)> = Vec::new();
    for line in cue.lines().map(str::trim) {
        let mut words = line.split_whitespace();
        match words.next().map(str::to_ascii_uppercase).as_deref() {
            Some("FILE") => {
                let rest = line[4..].trim();
                let (name, kind) = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split_once('"'))
                    .map(|(n, k)| (n, k.trim()))
                    .ok_or_else(|| format!("cue FILE line without quoted name: {line}"))?;
                if !kind.eq_ignore_ascii_case("BINARY") || file.replace(name.to_string()).is_some()
                {
                    return Err("only cue sheets with one BINARY file are supported".into());
                }
            }
            Some("TRACK") => {
                let number = words.next().and_then(|n| n.parse().ok());
                let kind = words.next().unwrap_or_default();
                let number = number.ok_or_else(|| format!("bad cue TRACK line: {line}"))?;
                tracks.push((number, kind.eq_ignore_ascii_case("AUDIO"), None, None));
            }
            Some("INDEX") => {
                let (index, time) = (words.next(), words.next());
                let sector = time.and_then(msf_sector);
                let (Some(index), Some(sector), Some(current)) = (index, sector, tracks.last_mut())
                else {
                    return Err(format!("bad cue INDEX line: {line}"));
                };
                current.2.get_or_insert(sector);
                if index.parse::<u32>() == Ok(1) {
                    current.3 = Some(sector);
                }
            }
            _ => {}
        }
    }
    let file = file.ok_or("cue sheet names no file")?;
    let position = tracks
        .iter()
        .position(|t| t.0 == track)
        .ok_or_else(|| format!("CD has no track {track}"))?;
    let (_, is_audio, _, start) = tracks[position];
    if is_audio != audio {
        let kind = if audio { "audio" } else { "data" };
        return Err(format!("CD track {track} is not {kind}"));
    }
    let start = start.ok_or_else(|| format!("CD track {track} has no INDEX 01"))?;
    let end = tracks.get(position + 1).and_then(|next| next.2);
    Ok((file, start * SECTOR_BYTES, end.map(|e| e * SECTOR_BYTES)))
}

/// `mm:ss:ff` (75 frames per second) to a sector number.
fn msf_sector(msf: &str) -> Option<u64> {
    let mut parts = msf.split(':').map(|p| p.parse::<u64>().ok());
    let (m, s, f) = (parts.next()??, parts.next()??, parts.next()??);
    (s < 60 && f < 75 && parts.next().is_none()).then_some((m * 60 + s) * 75 + f)
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

    #[test]
    fn cue_track_ranges_end_at_the_next_track() {
        let cue = "FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 00:10:00\n    INDEX 01 00:12:00\n  TRACK 03 AUDIO\n    INDEX 01 01:00:74\n";
        assert_eq!(
            cue_track(cue, 2, true).unwrap(),
            (
                "disc.bin".into(),
                12 * 75 * SECTOR_BYTES,
                Some((60 * 75 + 74) * SECTOR_BYTES)
            )
        );
        assert_eq!(
            cue_track(cue, 3, true).unwrap(),
            ("disc.bin".into(), (60 * 75 + 74) * SECTOR_BYTES, None)
        );
        assert!(cue_track(cue, 1, true).is_err(), "data track");
        assert_eq!(cue_track(cue, 1, false).unwrap().1, 0);
        assert!(cue_track(cue, 4, true).is_err(), "missing track");
        assert!(cue_track(&format!("{cue}FILE \"b.bin\" BINARY\n"), 2, true).is_err());
        assert_eq!(msf_sector("00:60:00"), None);
    }
}
