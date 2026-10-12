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

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::install::GameInstall;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::music::{cue_path, read_track, world_track};
use cyrus_assets::sound::SoundSettings;
use cyrus_world::lifecycle::CurrentScene;

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Music>()
            .add_systems(OnEnter(cyrus_game::AppState::Menu), start_menu_track)
            .add_systems(
                Update,
                (
                    start_world_track.run_if(
                        resource_changed::<CurrentScene>
                            .or_else(resource_exists_and_changed::<SoundSettings>),
                    ),
                    finish_track,
                    apply_volume.run_if(resource_exists_and_changed::<SoundSettings>),
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
    #[cfg(not(target_arch = "wasm32"))]
    load: Option<Task<Result<AudioSource, String>>>,
    #[cfg(target_arch = "wasm32")]
    load: Option<Handle<AudioSource>>,
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
#[cfg(not(target_arch = "wasm32"))]
fn request(install: &GameInstall, music: &mut Music, track: u32) {
    let Some(cue) = cue_path(install) else {
        return warn!(
            "music: no CD image (game.ins) beside {}",
            install.root.display()
        );
    };
    music.track = Some(track);
    music.load = Some(IoTaskPool::get().spawn(async move {
        read_track(&cue, track).map(|bytes| AudioSource {
            bytes: bytes.into(),
        })
    }));
}

#[cfg(target_arch = "wasm32")]
fn request(assets: &AssetServer, music: &mut Music, track: u32) {
    music.track = Some(track);
    music.load = Some(assets.load(format!("cache://music/{track:02}.wav")));
}

fn start_menu_track(
    #[cfg(not(target_arch = "wasm32"))] install: Res<GameInstall>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
    settings: Option<Res<SoundSettings>>,
    audio: Option<Res<Assets<AudioSource>>>,
    mut music: ResMut<Music>,
) {
    if music_enabled(&settings, &audio) {
        #[cfg(not(target_arch = "wasm32"))]
        request(&install, &mut music, MENU_TRACK);
        #[cfg(target_arch = "wasm32")]
        request(&assets, &mut music, MENU_TRACK);
    }
}

fn start_world_track(
    mut commands: Commands,
    #[cfg(not(target_arch = "wasm32"))] install: Res<GameInstall>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
    current: Res<CurrentScene>,
    #[cfg(target_arch = "wasm32")] bank: Option<Res<cyrus_assets::sound::MusicSettings>>,
    settings: Option<Res<SoundSettings>>,
    audio: Option<Res<Assets<AudioSource>>>,
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    if !music_enabled(&settings, &audio) || current.name.is_empty() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let value = match world_track(&install, &current.name) {
        Ok(value) => value,
        Err(e) => return error!("music: scene {}: {e}", current.name),
    };
    #[cfg(target_arch = "wasm32")]
    let value = {
        let (Some(bank), Some(world)) = (bank, current.world) else {
            return;
        };
        bank.world_tracks.get(&world).copied().unwrap_or(-1)
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
        #[cfg(not(target_arch = "wasm32"))]
        request(&install, &mut music, track);
        #[cfg(target_arch = "wasm32")]
        request(&assets, &mut music, track);
    }
}

fn finish_track(
    mut commands: Commands,
    mut music: ResMut<Music>,
    mut audio: Option<ResMut<Assets<AudioSource>>>,
    settings: Option<Res<SoundSettings>>,
    playing: Query<Entity, With<MusicPlayer>>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
) {
    let (Some(load), Some(audio)) = (music.load.as_mut(), audio.as_mut()) else {
        return;
    };
    #[cfg(not(target_arch = "wasm32"))]
    let Some(result) = check_ready(load) else {
        return;
    };
    #[cfg(target_arch = "wasm32")]
    let result = match assets.get_load_state(load.id()) {
        Some(bevy::asset::LoadState::Failed(error)) => {
            Err(format!("CD track {:?}: {error}", music.track))
        }
        Some(bevy::asset::LoadState::Loaded) => {
            let Some(source) = audio.get(load).cloned() else {
                return;
            };
            Ok(source)
        }
        _ => return,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_waits_for_sound_settings_then_applies_volume() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<cyrus_game::AppState>()
            .init_resource::<CurrentScene>()
            .insert_resource(GameInstall {
                root: std::env::temp_dir().join("cyrus-absent-install"),
            })
            .add_plugins(MusicPlugin);
        app.update();
        assert_eq!(app.world().resource::<Music>().gain, 1.0);
        let mut settings = SoundSettings::parse("");
        settings.redbook_volume = 64;
        app.insert_resource(settings);
        app.update();
        assert_eq!(app.world().resource::<Music>().gain, 128.0 / 255.0);
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
