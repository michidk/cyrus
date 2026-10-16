//! Background music: Red Book audio tracks of the game CD, which the GOG
//! release ships as the cue sheet `game.ins` and image `game.gog` next to the
//! install directory.
//!
//! Entering a world whose `WORLD.INI` `world_redbook` is a positive track
//! number starts that track unless it is already playing, so worlds sharing a
//! track keep it running. A missing entry (or `-1`) keeps the current track;
//! `0` or another negative value stops the music. A different track stops the
//! playing one when the world request begins, so the loading screen is silent,
//! and the new track starts once the world is prepared. The startup book menu
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
//! pauses the CD from its request until it ends, and the CD then resumes where
//! it was (preservation `docs/engine/music.md`).
//!
//! Unsupported: credits and won-game music (those screens do not exist yet).

use bevy::audio::{AudioPlayer, AudioSource};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::install::GameInstall;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::music::{cue_path, read_track, world_track};
use cyrus_assets::sound::SoundSettings;
use cyrus_world::lifecycle::{CurrentScene, SceneRequest};

mod checkpoint;
pub use checkpoint::{MusicCandidate, MusicSnapshot, MusicTransport};

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            bevy::audio::AddAudioSource::add_audio_source::<MusicTransport>(app);
        }
        app.init_resource::<Music>()
            .add_systems(OnEnter(cyrus_game::AppState::Menu), start_menu_track)
            .add_systems(
                Update,
                (
                    stop_for_world_entry.run_if(resource_exists_and_changed::<SceneRequest>),
                    start_world_track.run_if(
                        resource_changed::<CurrentScene>
                            .or_else(resource_exists_and_changed::<SoundSettings>),
                    ),
                    apply_volume.run_if(resource_exists_and_changed::<SoundSettings>),
                    finish_track,
                    pause_track,
                )
                    .chain()
                    .after(crate::ApplyPreferences),
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

fn stop_track(
    commands: &mut Commands,
    music: &mut Music,
    playing: &Query<Entity, With<MusicPlayer>>,
) {
    for entity in playing {
        commands.entity(entity).despawn();
    }
    music.track = None;
    music.load = None;
}

/// A world load stops the CD when it begins, ahead of the loading screen, if the
/// world names a different track; the new track starts once the world is prepared.
fn stop_for_world_entry(
    mut commands: Commands,
    #[cfg(not(target_arch = "wasm32"))] install: Option<Res<GameInstall>>,
    #[cfg(target_arch = "wasm32")] bank: Option<Res<cyrus_assets::sound::MusicSettings>>,
    request: Res<SceneRequest>,
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    let value = install.and_then(|install| world_track(&install, &request.0).ok());
    // shortcut: the browser knows a world's track only by numeric WORLD.INI ID; other names stop at commit.
    #[cfg(target_arch = "wasm32")]
    let value = bank.and_then(|bank| {
        let world = request.0.parse::<u32>().ok()?;
        bank.world_tracks.get(&world).copied()
    });
    if let Some(value) = value
        && value != -1
        && u32::try_from(value).ok() != music.track
    {
        stop_track(&mut commands, &mut music, &playing);
    }
}

fn start_world_track(
    mut commands: Commands,
    #[cfg(not(target_arch = "wasm32"))] install: Option<Res<GameInstall>>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
    current: Res<CurrentScene>,
    #[cfg(target_arch = "wasm32")] bank: Option<Res<cyrus_assets::sound::MusicSettings>>,
    (settings, audio): (Option<Res<SoundSettings>>, Option<Res<Assets<AudioSource>>>),
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    if !music_enabled(&settings, &audio) || current.name.is_empty() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let Some(install) = install else {
        return;
    };
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
    stop_track(&mut commands, &mut music, &playing);
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
    #[cfg(target_arch = "wasm32")] audio: Option<Res<Assets<AudioSource>>>,
    mut transports: Option<ResMut<Assets<MusicTransport>>>,
    settings: Option<Res<SoundSettings>>,
    (playing, movie): (
        Query<Entity, With<MusicPlayer>>,
        Query<(), With<crate::CinematicAudio>>,
    ),
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
) {
    let (Some(load), Some(transports)) = (music.load.as_mut(), transports.as_mut()) else {
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
            let Some(source) = audio.as_ref().and_then(|audio| audio.get(load)).cloned() else {
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
    let transport = match MusicTransport::new(source) {
        Ok(transport) => transport,
        Err(error) => {
            music.track = None;
            return error!("music: preparing playback transport: {error:#}");
        }
    };
    for entity in &playing {
        commands.entity(entity).despawn();
    }
    let mut playback = transport
        .playback_settings()
        .with_volume(bevy::audio::Volume::Linear(music.gain));
    playback.paused = !movie.is_empty() || settings.is_some_and(|s| s.redbook_volume == 0);
    commands.spawn((
        MusicPlayer,
        AudioPlayer(transports.add(transport)),
        playback,
    ));
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
    movie: Query<(), With<crate::CinematicAudio>>,
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

    fn prepared_music(source: AudioSource, volume: i32, cinematic: bool) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .init_state::<cyrus_game::AppState>()
        .init_resource::<CurrentScene>()
        .init_asset::<AudioSource>()
        .init_asset::<MusicTransport>()
        .insert_resource(GameInstall {
            root: std::env::temp_dir().join("cyrus-absent-install"),
        })
        .add_plugins(MusicPlugin);
        let mut settings = SoundSettings::parse("");
        settings.redbook_volume = volume;
        app.insert_resource(settings);
        if cinematic {
            app.world_mut().spawn(crate::CinematicAudio);
        }
        {
            let mut music = app.world_mut().resource_mut::<Music>();
            music.track = Some(2);
            music.load = Some(IoTaskPool::get().spawn(async move { Ok(source) }));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app
            .world()
            .resource::<Music>()
            .load
            .as_ref()
            .unwrap()
            .is_finished()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "music preparation did not finish"
            );
            std::thread::yield_now();
        }
        app.update();
        assert!(app.world().resource::<Music>().load.is_none());
        app
    }

    #[test]
    fn pending_track_capture_does_not_poll_or_replace_the_load() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let music = Music {
            track: Some(2),
            load: Some(IoTaskPool::get().spawn(std::future::pending())),
            ..Music::default()
        };
        assert!(
            music
                .snapshot(&[])
                .err()
                .unwrap()
                .to_string()
                .contains("preparation is in flight")
        );
        assert!(music.load.is_some());
        assert_eq!(music.track, Some(2));
    }

    #[test]
    fn production_music_preserves_linear_drive_gain_and_queued_pause() {
        use bevy::audio::{Decodable, PlaybackMode, PlaybackSettings, Volume};
        for (volume, cinematic) in [(0, false), (64, false), (200, false), (64, true)] {
            let source = AudioSource {
                bytes: cyrus_assets::audio::pcm_wav(
                    rgpre::import::sfx::AudioType::Stereo16,
                    44100,
                    &[0, 64, 0, 192],
                )
                .unwrap()
                .into(),
            };
            let mut app = prepared_music(source, volume, cinematic);
            let (player, settings) = app.world_mut().query_filtered::<(&AudioPlayer<MusicTransport>, &PlaybackSettings), With<MusicPlayer>>().single(app.world()).unwrap();
            assert!(matches!(settings.mode, PlaybackMode::Despawn));
            assert_eq!(settings.paused, cinematic || volume == 0);
            let expected_gain = redbook_gain(volume).unwrap_or(1.0);
            assert!(matches!(settings.volume, Volume::Linear(gain) if gain == expected_gain));
            let transport = app
                .world()
                .resource::<Assets<MusicTransport>>()
                .get(&player.0)
                .unwrap();
            assert_eq!(
                transport.decoder().take(6).collect::<Vec<_>>(),
                [0.5, -0.5, 0.5, -0.5, 0.5, -0.5]
            );
            let music = app.world().resource::<Music>();
            let saved = music.snapshot(&[(transport, settings.paused)]).unwrap();
            let candidate = Music::from_snapshot(saved, Some(transport.source().clone())).unwrap();
            let (_, restored_settings) = candidate.playback.unwrap();
            assert_eq!(restored_settings.paused, cinematic || volume == 0);
            assert_eq!(candidate.music.gain, expected_gain);
        }
    }

    #[test]
    fn installed_cd_track_reconstructs_through_the_production_headless_host() {
        use bevy::audio::{Decodable, PlaybackSettings};
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = GameInstall::locate(Some(&root)) else {
            eprintln!("skipped: no install (CD decoder replacement headless host)");
            return;
        };
        let source = AudioSource {
            bytes: read_track(&cue_path(&install).unwrap(), 2).unwrap().into(),
        };
        let expected: Vec<_> = source.decoder().skip(1023).take(1024).collect();
        assert_eq!(expected.len(), 1024);
        let mut app = prepared_music(source, 64, false);
        let (player, settings) = app
            .world_mut()
            .query_filtered::<(&AudioPlayer<MusicTransport>, &PlaybackSettings), With<MusicPlayer>>(
            )
            .single(app.world())
            .unwrap();
        let transport = app
            .world()
            .resource::<Assets<MusicTransport>>()
            .get(&player.0)
            .unwrap();
        let mut live = transport.decoder();
        assert_eq!(live.by_ref().take(1023).count(), 1023);
        let saved = app
            .world()
            .resource::<Music>()
            .snapshot(&[(transport, settings.paused)])
            .unwrap();
        let saved: MusicSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let candidate = Music::from_snapshot(saved, Some(transport.source().clone())).unwrap();
        let (restored, settings) = candidate.playback.unwrap();
        assert!(!settings.paused);
        assert_eq!(restored.decoder().take(1024).collect::<Vec<_>>(), expected);
        assert_eq!(live.take(1024).collect::<Vec<_>>(), expected);
    }

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
    fn stopping_ends_the_playing_track_and_forgets_it() {
        let mut app = App::new();
        app.init_resource::<Music>().add_systems(
            Update,
            |mut commands: Commands,
             mut music: ResMut<Music>,
             playing: Query<Entity, With<MusicPlayer>>| {
                stop_track(&mut commands, &mut music, &playing)
            },
        );
        app.world_mut().resource_mut::<Music>().track = Some(2);
        app.world_mut().spawn(MusicPlayer);
        app.update();
        assert_eq!(app.world().resource::<Music>().track, None);
        assert!(
            app.world_mut()
                .query::<&MusicPlayer>()
                .iter(app.world())
                .next()
                .is_none()
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
