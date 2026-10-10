//! Background music: Red Book audio tracks of the game CD, which the GOG
//! release ships as the cue sheet `game.ins` and image `game.gog` next to the
//! install directory.
//!
//! Entering a world whose `WORLD.INI` `world_redbook` is a positive track
//! number starts that track unless it is already playing, so worlds sharing a
//! track keep it running; other worlds leave the current track playing. The
//! track repeats until another one starts. `SYSTEM.INI` `redbook` disables
//! music; the shipped `redbook_volume=200` lies outside the CD volume range
//! 0..=127 the original accepts, so music plays at full level.
//!
//! Unsupported until evidence is published: menu and cutscene music and the
//! in-game music volume option.

use crate::install::GameInstall;
use crate::scene::CurrentScene;
use crate::sound::{SoundSettings, ini_value, pcm_wav};
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use rgpre::import::sfx::AudioType;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Red Book raw sector size in bytes; audio is 44.1 kHz 16-bit stereo, 75 sectors per second.
const SECTOR_BYTES: u64 = 2352;
const RED_BOOK_RATE: u32 = 44_100;

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Music>().add_systems(
            Update,
            (
                start_world_track.run_if(resource_changed::<CurrentScene>),
                finish_track,
            )
                .chain(),
        );
    }
}

/// The track playing or loading, as numbered on the CD (1 is the data track).
#[derive(Resource, Default)]
pub struct Music {
    pub track: Option<u32>,
    load: Option<Task<Result<AudioSource, String>>>,
}

#[derive(Component)]
struct MusicPlayer;

/// The cue sheet of the GOG CD image, beside the install directory.
pub fn cue_path(install: &GameInstall) -> Option<PathBuf> {
    let path = install.root.parent()?.join("game.ins");
    path.is_file().then_some(path)
}

fn start_world_track(
    install: Res<GameInstall>,
    current: Res<CurrentScene>,
    settings: Option<Res<SoundSettings>>,
    audio: Option<Res<Assets<AudioSource>>>,
    mut music: ResMut<Music>,
) {
    if audio.is_none() || !settings.is_some_and(|s| s.music) || current.name.is_empty() {
        return;
    }
    let track = match world_track(&install, &current.name) {
        Ok(Some(track)) => track,
        Ok(None) => return,
        Err(e) => return error!("music: scene {}: {e}", current.name),
    };
    if music.track == Some(track) {
        return;
    }
    let Some(cue) = cue_path(&install) else {
        return warn!(
            "music: no CD image (game.ins) beside {}",
            install.root.display()
        );
    };
    music.track = Some(track);
    music.load = Some(IoTaskPool::get().spawn(async move { read_track(&cue, track) }));
}

fn finish_track(
    mut commands: Commands,
    mut music: ResMut<Music>,
    mut audio: Option<ResMut<Assets<AudioSource>>>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    let (Some(load), Some(audio)) = (music.load.as_mut(), audio.as_mut()) else {
        return;
    };
    let Some(result) = check_ready(load) else {
        return;
    };
    music.load = None;
    let source = match result {
        Ok(source) => source,
        Err(e) => {
            music.track = None;
            return error!("music: {e}");
        }
    };
    for entity in &playing {
        commands.entity(entity).despawn();
    }
    commands.spawn((
        MusicPlayer,
        AudioPlayer(audio.add(source)),
        PlaybackSettings::LOOP,
    ));
}

/// `world_redbook` of the world a scene name resolves to; `None` when absent or not positive.
pub fn world_track(install: &GameInstall, scene: &str) -> anyhow::Result<Option<u32>> {
    let index = crate::scene::world_index(install, scene)?;
    let path = rgpre::import::world_ini::find_world_ini(&install.root)
        .ok_or_else(|| anyhow::anyhow!("missing WORLD.INI"))?;
    let ini = std::fs::read_to_string(path)?;
    let Some(value) = ini_value(&ini, "world", &format!("world_redbook[{index}]")) else {
        return Ok(None);
    };
    let track: i32 = value
        .parse()
        .map_err(|e| anyhow::anyhow!("world {index}: world_redbook {value}: {e}"))?;
    Ok(u32::try_from(track).ok().filter(|&t| t > 0))
}

/// Byte range of audio `track` in the single `BINARY` file a cue sheet names:
/// (file name, start, end). `end` is `None` for the last track, which runs to the end of the file.
pub fn cue_track(cue: &str, track: u32) -> Result<(String, u64, Option<u64>), String> {
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
    let (_, audio, _, start) = tracks[position];
    if !audio {
        return Err(format!("CD track {track} is not audio"));
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
pub fn read_track(cue_path: &Path, track: u32) -> Result<AudioSource, String> {
    let context = |e: &dyn std::fmt::Display| format!("{} track {track}: {e}", cue_path.display());
    let cue = std::fs::read_to_string(cue_path).map_err(|e| context(&e))?;
    let (name, start, end) = cue_track(&cue, track).map_err(|e| context(&e))?;
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
    Ok(AudioSource {
        bytes: bytes.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_track_ranges_end_at_the_next_track() {
        let cue = "FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 00:10:00\n    INDEX 01 00:12:00\n  TRACK 03 AUDIO\n    INDEX 01 01:00:74\n";
        assert_eq!(
            cue_track(cue, 2).unwrap(),
            (
                "disc.bin".into(),
                12 * 75 * SECTOR_BYTES,
                Some((60 * 75 + 74) * SECTOR_BYTES)
            )
        );
        assert_eq!(
            cue_track(cue, 3).unwrap(),
            ("disc.bin".into(), (60 * 75 + 74) * SECTOR_BYTES, None)
        );
        assert!(cue_track(cue, 1).is_err(), "data track");
        assert!(cue_track(cue, 4).is_err(), "missing track");
        assert!(cue_track(&format!("{cue}FILE \"b.bin\" BINARY\n"), 2).is_err());
        assert_eq!(msf_sector("00:60:00"), None);
    }
}
