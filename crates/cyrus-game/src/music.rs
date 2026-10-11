//! Background music: Red Book audio tracks of the game CD, which the GOG
//! release ships as the cue sheet `game.ins` and image `game.gog` next to the
//! install directory.
//!
//! Entering a world whose `WORLD.INI` `world_redbook` is a positive track
//! number starts that track unless it is already playing, so worlds sharing a
//! track keep it running. A missing entry (or `-1`) keeps the current track;
//! `0` or another negative value stops the music. The startup book menu
//! restarts track 2 from its beginning. A track repeats until another one
//! starts. `SYSTEM.INI` `redbook` disables music.
//!
//! Volume: `redbook_volume` 0..=127 sets the drive's channel volume to twice
//! the value out of 255, which GOG's DOSBox applies linearly; other values are
//! ignored and the drive stays at its initial 255, full level. The shipped 200
//! is therefore full level. Facts: preservation `docs/engine/music.md`
//! ([PR 21](https://github.com/michidk/redguard-preservation/pull/21)).
//!
//! The Music slider writes `trunc(slider × 12.8)` through the same call, so
//! slider 1–9 sets a level, 10–20 are ignored by the drive, and 0 pauses the
//! CD until the slider leaves 0 (preservation `docs/engine/menu.md`). A movie
//! pauses the CD and it resumes where it was (preservation `docs/engine/music.md`).
//!
//! Unsupported: credits and won-game music (those screens do not exist yet).

use crate::install::GameInstall;
use crate::scene::CurrentScene;
use crate::sound::{SoundSettings, ini_value, pcm_wav};
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use rgpre::import::sfx::AudioType;
use std::path::{Path, PathBuf};

/// Red Book raw sector size in bytes; audio is 44.1 kHz 16-bit stereo, 75 sectors per second.
#[cfg(test)]
const SECTOR_BYTES: u64 = 2352;
const RED_BOOK_RATE: u32 = 44_100;

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Music>()
            .add_systems(OnEnter(crate::AppState::Menu), start_menu_track)
            .add_systems(
                Update,
                (
                    start_world_track.run_if(resource_changed::<CurrentScene>),
                    finish_track,
                    apply_volume.run_if(resource_changed::<SoundSettings>),
                    pause_track,
                )
                    .chain(),
            );
    }
}

/// The track playing or loading, as numbered on the CD (1 is the data track).
#[derive(Resource)]
pub struct Music {
    pub track: Option<u32>,
    /// The drive's linear channel gain; it starts at full level and only accepted volumes change it.
    pub gain: f32,
    load: Option<Task<Result<AudioSource, String>>>,
}

impl Default for Music {
    fn default() -> Self {
        Self {
            track: None,
            gain: 1.0,
            load: None,
        }
    }
}

/// The entity playing the current CD track.
#[derive(Component)]
pub struct MusicPlayer;

/// The cue sheet of the GOG CD image, beside the install directory.
pub fn cue_path(install: &GameInstall) -> Option<PathBuf> {
    let path = install.root.parent()?.join("game.ins");
    path.is_file().then_some(path)
}

/// The CD track the startup book menu plays.
const MENU_TRACK: u32 = 2;

/// Drive gain for a `redbook_volume` value: `Some` for the accepted 0..=127
/// (channel volume `2v` of 255, applied linearly), `None` when it is ignored.
pub fn redbook_gain(volume: i32) -> Option<f32> {
    (0..=127)
        .contains(&volume)
        .then(|| (volume * 2) as f32 / 255.0)
}

fn music_enabled(
    settings: &Option<Res<SoundSettings>>,
    audio: &Option<Res<Assets<AudioSource>>>,
) -> bool {
    audio.is_some() && settings.as_ref().is_some_and(|s| s.music)
}

/// Starts loading `track`; the playing track is replaced once it is read.
fn request(install: &GameInstall, music: &mut Music, track: u32) {
    let Some(cue) = cue_path(install) else {
        return warn!(
            "music: no CD image (game.ins) beside {}",
            install.root.display()
        );
    };
    music.track = Some(track);
    music.load = Some(IoTaskPool::get().spawn(async move { read_track(&cue, track) }));
}

fn start_menu_track(
    install: Res<GameInstall>,
    settings: Option<Res<SoundSettings>>,
    audio: Option<Res<Assets<AudioSource>>>,
    mut music: ResMut<Music>,
) {
    if music_enabled(&settings, &audio) {
        request(&install, &mut music, MENU_TRACK);
    }
}

fn start_world_track(
    mut commands: Commands,
    install: Res<GameInstall>,
    current: Res<CurrentScene>,
    settings: Option<Res<SoundSettings>>,
    audio: Option<Res<Assets<AudioSource>>>,
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    if !music_enabled(&settings, &audio) || current.name.is_empty() {
        return;
    }
    let value = match world_track(&install, &current.name) {
        Ok(value) => value,
        Err(e) => return error!("music: scene {}: {e}", current.name),
    };
    if value == -1 || u32::try_from(value).ok() == music.track {
        return;
    }
    for entity in &playing {
        commands.entity(entity).despawn();
    }
    music.track = None;
    music.load = None;
    if let Ok(track @ 1..) = u32::try_from(value) {
        request(&install, &mut music, track);
    }
}

fn finish_track(
    mut commands: Commands,
    mut music: ResMut<Music>,
    mut audio: Option<ResMut<Assets<AudioSource>>>,
    settings: Option<Res<SoundSettings>>,
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
    let mut playback = PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(music.gain));
    playback.paused = settings.is_some_and(|s| s.redbook_volume == 0);
    commands.spawn((MusicPlayer, AudioPlayer(audio.add(source)), playback));
}

/// Applies a changed `redbook_volume`: accepted values set the drive level.
fn apply_volume(
    settings: Res<SoundSettings>,
    mut music: ResMut<Music>,
    mut sinks: Query<&mut bevy::audio::AudioSink, With<MusicPlayer>>,
) {
    if let Some(gain) = redbook_gain(settings.redbook_volume) {
        music.gain = gain;
    }
    for mut sink in &mut sinks {
        use bevy::audio::AudioSinkPlayback;
        sink.set_volume(bevy::audio::Volume::Linear(music.gain));
    }
}

/// Pauses the CD while `redbook_volume` is 0 or a movie plays, and resumes it otherwise.
fn pause_track(
    settings: Option<Res<SoundSettings>>,
    movie: Query<(), With<crate::startup::MovieAudio>>,
    sinks: Query<&bevy::audio::AudioSink, With<MusicPlayer>>,
) {
    use bevy::audio::AudioSinkPlayback;
    let paused = !movie.is_empty() || settings.is_some_and(|s| s.redbook_volume == 0);
    for sink in &sinks {
        if paused && !sink.is_paused() {
            sink.pause();
        } else if !paused && sink.is_paused() {
            sink.play();
        }
    }
}

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

pub use cyrus_assets::native::disc::cue_track;

/// Reads one audio track from the image a cue sheet names, as a playable WAV source.
pub fn read_track(cue_path: &Path, track: u32) -> Result<AudioSource, String> {
    let context = |e: &dyn std::fmt::Display| format!("{} track {track}: {e}", cue_path.display());
    let pcm = cyrus_assets::native::disc::read_pcm(cue_path, track).map_err(|e| context(&e))?;
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
        assert!(
            cue_track(
                "FILE \"disc.bin\" BINARY\nTRACK 2 AUDIO\nINDEX 01 00:60:00",
                2,
                true
            )
            .is_err()
        );
    }

    #[test]
    fn redbook_volume_accepts_only_0_to_127() {
        assert_eq!(redbook_gain(0), Some(0.0));
        assert_eq!(redbook_gain(127), Some(254.0 / 255.0));
        assert_eq!(redbook_gain(64), Some(128.0 / 255.0));
        assert_eq!(redbook_gain(200), None, "shipped value is ignored");
        assert_eq!(redbook_gain(-1), None);
    }
}
