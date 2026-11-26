//! Background music: Red Book audio tracks of the game CD, which the GOG
//! release ships as the cue sheet `game.ins` and image `game.gog` next to the
//! install directory. A CD-edition install without them reads the ripped
//! `Soundtrack/` MP3s placed beside it instead (`cyrus_assets::music::music_source`).
//!
//! Entering a world whose `WORLD.INI` `world_redbook` is a positive track
//! number starts that track unless it is already playing, so worlds sharing a
//! track keep it running. A missing entry (or `-1`) keeps the current track;
//! `0` or another negative value stops the music. A different track stops the
//! playing one when the world request begins, so the loading screen is silent,
//! and the new track starts once gameplay is admitted. The startup book menu
//! restarts track 2 from its beginning. A completed track repeats only while
//! the world runs. The book leaves current playback unchanged and does not
//! replay a completed track. `SYSTEM.INI` `redbook` disables music.
//!
//! Volume: the engine clamps `redbook_volume` to 0..=255 and halves it for
//! the 0..=127 Miles drive call. DOSBox's CD channel level is twice that
//! integer out of 255. The shipped 200 therefore plays at 200/255 linear gain.
//! Facts: released preservation `docs/engine/music.md#volume` and
//! `docs/config/system-ini.md`. The Music slider writes trunc(slider * 12.8)
//! through this same conversion. Zero pauses the CD until the slider leaves
//! zero. Movies pause and resume CD playback without restarting the track.
//!
//! Both music modes mute leading CD-ROM data-sector bytes in the GOG image
//! without changing track length or timing (preservation `docs/engine/music.md`).
//!
//! During the ending, `CreditsPlaying` requests track 2 (the book track) and
//! stops it when the credits finish or are escaped. `EndingEntry` suppresses
//! the normal menu-track restart while the ending movie and credits are being
//! presented. This follows the installed MENU.INI/music contract; the host
//! owns the movie and credits state transitions.

use bevy::audio::{AudioPlayer, AudioSource};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::install::GameInstall;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::music::{music_source, read_track, world_track};
use cyrus_assets::sound::SoundSettings;
use cyrus_world::lifecycle::{CurrentScene, SceneRequest};

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(any(target_arch = "wasm32", test))]
mod browser_owners;
mod checkpoint;
pub use checkpoint::{MusicCandidate, MusicSnapshot, MusicTransport};

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            #[cfg(not(target_arch = "wasm32"))]
            bevy::audio::AddAudioSource::add_audio_source::<MusicTransport>(app);
            #[cfg(target_arch = "wasm32")]
            {
                app.init_asset::<MusicTransport>();
                browser::register(app);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let credits_track = credits_track.run_if(resource_exists::<GameInstall>);
        #[cfg(target_arch = "wasm32")]
        let credits_track = credits_track;
        app.init_resource::<Music>()
            .add_systems(OnEnter(cyrus_game::AppState::Menu), start_menu_track)
            .add_systems(OnExit(cyrus_game::AppState::Menu), cancel_menu_track)
            .add_systems(
                Update,
                (
                    stop_for_world_entry.run_if(resource_exists_and_changed::<SceneRequest>),
                    start_world_track,
                    start_menu_track
                        .run_if(resource_exists::<PendingMenuMusic>)
                        .run_if(in_state(cyrus_game::AppState::Menu)),
                    apply_volume.run_if(resource_exists_and_changed::<SoundSettings>),
                    credits_track,
                    finish_track,
                    replay_track,
                    pause_track,
                    finish_restoration,
                )
                    .chain()
                    .after(crate::ApplyPreferences)
                    .after(cyrus_world::lifecycle::WorldChange),
            );
    }
}

/// The last requested CD track, retained after playback finishes (1 is the data track).
#[derive(Resource)]
pub struct Music {
    pub track: Option<u32>,
    /// The drive's linear channel gain; full level until the installed setting is applied.
    pub gain: f32,
    /// World selection waits for its inputs; restored publication preserves the saved owner.
    world_selection: WorldSelection,
    /// Installed bytes retained for replay without repeating disk I/O.
    source: Option<AudioSource>,
    #[cfg(not(target_arch = "wasm32"))]
    load: Option<Task<Result<AudioSource, String>>>,
    #[cfg(target_arch = "wasm32")]
    load: Option<Handle<AudioSource>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorldSelection {
    Settled,
    Pending,
    AwaitingGameplay,
    Restored,
}

impl Music {
    /// Whether track selection or source preparation still prevents a complete checkpoint.
    /// Observing this state neither polls nor consumes the pending task.
    pub fn is_preparing(&self) -> bool {
        self.load.is_some() || self.world_selection == WorldSelection::Pending
    }
}

impl Default for Music {
    fn default() -> Self {
        Self {
            track: None,
            gain: 1.0,
            world_selection: WorldSelection::Settled,
            source: None,
            load: None,
        }
    }
}

/// The entity playing the current CD track.
#[derive(Component)]
#[cfg_attr(any(target_arch = "wasm32", test), component(on_remove = browser_owners::retire))]
pub struct MusicPlayer;

/// The CD track the startup book menu plays.
const MENU_TRACK: u32 = 2;

/// Drive gain after the engine's 0..=255 clamp and integer halving for Miles.
/// DOSBox maps the resulting 0..=127 drive value to an even channel level of 255.
/// This follows preservation's released music volume and SYSTEM.INI rules.
pub fn redbook_gain(volume: i32) -> f32 {
    ((volume.clamp(0, 255) / 2) * 2) as f32 / 255.0
}

fn music_enabled(
    settings: &Option<Res<SoundSettings>>,
    audio: &Option<Res<Assets<AudioSource>>>,
) -> bool {
    audio.is_some() && settings.as_ref().is_some_and(|s| s.music)
}

/// A copy of a prepared CD WAV with its leading CD-ROM data-sector bytes muted,
/// or `None` when the track starts with audio.
pub(crate) fn muted(source: &AudioSource) -> Option<AudioSource> {
    let data = source
        .bytes
        .get(..64)?
        .windows(4)
        .position(|tag| tag == b"data")?
        + 8;
    let mut bytes = source.bytes.to_vec();
    (cyrus_assets::disc::mute_leading_data_sectors(bytes.get_mut(data..)?) > 0).then(|| {
        AudioSource {
            bytes: bytes.into(),
        }
    })
}

/// Starts loading `track`; the playing track is replaced once it is read.
#[cfg(not(target_arch = "wasm32"))]
fn request(install: &GameInstall, music: &mut Music, track: u32) {
    let Some(cue) = music_source(install) else {
        return warn!(
            "music: no CD audio (game.ins or Soundtrack/) beside {}",
            install.root.display()
        );
    };
    music.world_selection = WorldSelection::Settled;
    music.track = Some(track);
    music.source = None;
    music.load = Some(IoTaskPool::get().spawn(async move {
        read_track(&cue, track).map(|bytes| AudioSource {
            bytes: bytes.into(),
        })
    }));
}

#[cfg(target_arch = "wasm32")]
fn request(assets: &AssetServer, music: &mut Music, track: u32) {
    music.world_selection = WorldSelection::Settled;
    music.track = Some(track);
    music.source = None;
    music.load = Some(assets.load(format!("cache://music/{track:02}.wav")));
}

/// Book entry retains its request until the installed audio settings are available.
#[derive(Resource)]
struct PendingMenuMusic;

fn cancel_menu_track(mut commands: Commands) {
    commands.remove_resource::<PendingMenuMusic>();
}

fn start_menu_track(
    mut commands: Commands,
    death: Option<Res<cyrus_script::script::DeathBookEntry>>,
    confirmation: Option<Res<cyrus_script::script::LastGameEntry>>,
    ending: Option<Res<cyrus_script::script::EndingEntry>>,
    #[cfg(not(target_arch = "wasm32"))] install: Res<GameInstall>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
    enabled: (Option<Res<SoundSettings>>, Option<Res<Assets<AudioSource>>>),
    mut music: ResMut<Music>,
) {
    let (settings, audio) = enabled;
    commands.remove_resource::<PendingMenuMusic>();
    // The ending pauses the CD instead of starting the book's track.
    if death.is_some() || confirmation.is_some() || ending.is_some() {
        return;
    }
    if settings.is_none() || audio.is_none() {
        commands.insert_resource(PendingMenuMusic);
        return;
    }
    if music_enabled(&settings, &audio) {
        #[cfg(not(target_arch = "wasm32"))]
        request(&install, &mut music, MENU_TRACK);
        #[cfg(target_arch = "wasm32")]
        request(&assets, &mut music, MENU_TRACK);
    }
}

/// The ending's credits play the book's CD track and stop the CD when they end.
fn credits_track(
    mut commands: Commands,
    credits: Option<Res<cyrus_script::script::CreditsPlaying>>,
    #[cfg(not(target_arch = "wasm32"))] install: Res<GameInstall>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
    enabled: (Option<Res<SoundSettings>>, Option<Res<Assets<AudioSource>>>),
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
    mut was_playing: Local<bool>,
) {
    let (settings, audio) = enabled;
    let now = credits.is_some();
    if now == std::mem::replace(&mut *was_playing, now) {
        return;
    }
    if !now {
        for entity in &playing {
            commands.entity(entity).despawn();
        }
        music.load = None;
    } else if music_enabled(&settings, &audio) {
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
    music.source = None;
}

/// A world load stops the CD when it begins, ahead of the loading screen, if the
/// world names a different track; the new track starts once gameplay is admitted.
#[cfg(not(target_arch = "wasm32"))]
fn stop_for_world_entry(
    mut commands: Commands,
    #[cfg(not(target_arch = "wasm32"))] install: Option<Res<GameInstall>>,
    request: Res<SceneRequest>,
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    if music.world_selection == WorldSelection::Restored {
        return;
    }
    let value = install.and_then(|install| world_track(&install, &request.0).ok());
    if let Some(value) = value
        && value != -1
        && u32::try_from(value).ok() != music.track
    {
        stop_track(&mut commands, &mut music, &playing);
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn stop_for_browser_world_entry(
    mut commands: Commands,
    bank: Option<Res<cyrus_assets::sound::MusicSettings>>,
    request: Res<SceneRequest>,
    mut music: ResMut<Music>,
    playing: Query<Entity, With<MusicPlayer>>,
) {
    if music.world_selection == WorldSelection::Restored {
        return;
    }
    let value = bank.and_then(|bank| bank.track_for_scene(&request.0));
    if let Some(value) = value
        && value != -1
        && u32::try_from(value).ok() != music.track
    {
        stop_track(&mut commands, &mut music, &playing);
    }
}

#[cfg(target_arch = "wasm32")]
use stop_for_browser_world_entry as stop_for_world_entry;

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
    if music.world_selection == WorldSelection::Restored {
        return;
    }
    let awaiting_gameplay = music.world_selection == WorldSelection::AwaitingGameplay;
    if current.is_changed() {
        music.world_selection = if current.name.is_empty() {
            WorldSelection::Settled
        } else {
            WorldSelection::Pending
        };
    }
    if music.world_selection != WorldSelection::Pending || settings.is_none() || audio.is_none() {
        return;
    }
    if !music_enabled(&settings, &audio) {
        music.world_selection = WorldSelection::Settled;
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let Some(install) = install else {
        return;
    };
    #[cfg(not(target_arch = "wasm32"))]
    let value = match world_track(&install, &current.name) {
        Ok(value) => value,
        Err(e) => {
            music.world_selection = WorldSelection::Settled;
            return error!("music: scene {}: {e}", current.name);
        }
    };
    #[cfg(target_arch = "wasm32")]
    let value = {
        let (Some(bank), Some(world)) = (bank, current.world) else {
            return;
        };
        bank.world_tracks.get(&world).copied().unwrap_or(-1)
    };
    music.world_selection = WorldSelection::Settled;
    if value == -1
        || (u32::try_from(value).ok() == music.track
            && (!playing.is_empty() || music.load.is_some()))
    {
        if awaiting_gameplay && music.load.is_some() {
            music.world_selection = WorldSelection::AwaitingGameplay;
        }
        return;
    }
    stop_track(&mut commands, &mut music, &playing);
    if let Ok(track @ 1..) = u32::try_from(value) {
        #[cfg(not(target_arch = "wasm32"))]
        request(&install, &mut music, track);
        #[cfg(target_arch = "wasm32")]
        request(&assets, &mut music, track);
        music.world_selection = WorldSelection::AwaitingGameplay;
    }
}

// WorldChange runs first, so the restored scene's notification is consumed in this update.
fn finish_restoration(mut music: ResMut<Music>) {
    if music.world_selection == WorldSelection::Restored {
        music.world_selection = WorldSelection::Settled;
    }
}

fn finish_track(
    mut commands: Commands,
    mut music: ResMut<Music>,
    state: Option<Res<State<cyrus_game::AppState>>>,
    paused: Option<Res<cyrus_script::PausedGameplay>>,
    #[cfg(target_arch = "wasm32")] audio: Option<Res<Assets<AudioSource>>>,
    mut transports: Option<ResMut<Assets<MusicTransport>>>,
    settings: Option<Res<SoundSettings>>,
    (playing, movie): (
        Query<Entity, With<MusicPlayer>>,
        Query<(), With<crate::CinematicAudio>>,
    ),
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
) {
    // A paused gameplay book retains admission; loading and startup menus do not.
    let admitted = state.is_some_and(|state| {
        *state.get() == cyrus_game::AppState::InGame
            || (*state.get() == cyrus_game::AppState::Menu && paused.is_some())
    });
    if music.world_selection == WorldSelection::AwaitingGameplay && !admitted {
        return;
    }
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
    if music.world_selection == WorldSelection::AwaitingGameplay {
        music.world_selection = WorldSelection::Settled;
    }
    let source = match result {
        Ok(source) => source,
        Err(e) => {
            music.track = None;
            return error!("music: {e}");
        }
    };
    music.source = Some(source.clone());
    let source = muted(&source).unwrap_or(source);
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

/// Requested tracks restart only during world execution, never inside the book or a movie.
fn replay_track(
    mut commands: Commands,
    music: Res<Music>,
    (state, credits): (
        Option<Res<State<cyrus_game::AppState>>>,
        Option<Res<cyrus_script::script::CreditsPlaying>>,
    ),
    settings: Option<Res<SoundSettings>>,
    mut transports: Option<ResMut<Assets<MusicTransport>>>,
    (playing, movie): (
        Query<(), With<MusicPlayer>>,
        Query<(), With<crate::CinematicAudio>>,
    ),
) {
    if !state.is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
        || !playing.is_empty()
        || !movie.is_empty()
        || credits.is_some()
        || music.load.is_some()
        || music.track.is_none()
        || !settings.as_ref().is_some_and(|settings| settings.music)
    {
        return;
    }
    let (Some(source), Some(transports)) = (music.source.as_ref(), transports.as_mut()) else {
        return;
    };
    let source = muted(source).unwrap_or_else(|| source.clone());
    match MusicTransport::new(source) {
        Ok(transport) => {
            let mut playback = transport
                .playback_settings()
                .with_volume(bevy::audio::Volume::Linear(music.gain));
            playback.paused = settings.is_some_and(|settings| settings.redbook_volume == 0);
            commands.spawn((
                MusicPlayer,
                AudioPlayer(transports.add(transport)),
                playback,
            ));
        }
        Err(error) => error!("music: replaying CD track {:?}: {error:#}", music.track),
    }
}

/// Applies a changed `redbook_volume` through the engine clamp and drive conversion.
fn apply_volume(
    settings: Res<SoundSettings>,
    global: Option<Res<bevy::audio::GlobalVolume>>,
    mut music: ResMut<Music>,
    mut sinks: Query<&mut bevy::audio::AudioSink, With<MusicPlayer>>,
    mut playback: Query<&mut PlaybackSettings, With<MusicPlayer>>,
) {
    music.gain = redbook_gain(settings.redbook_volume);
    for mut settings in &mut playback {
        settings.volume = bevy::audio::Volume::Linear(music.gain);
    }
    // Bevy applies the global volume only when a sink starts; a direct change must include it.
    let global = global.map_or(1.0, |global| global.volume.to_linear());
    for mut sink in &mut sinks {
        use bevy::audio::AudioSinkPlayback;
        sink.set_volume(bevy::audio::Volume::Linear(music.gain * global));
    }
}

/// Pauses the CD while `redbook_volume` is 0 or a movie plays, and resumes it otherwise.
fn pause_track(
    settings: Option<Res<SoundSettings>>,
    movie: Query<(), With<crate::CinematicAudio>>,
    sinks: Query<&bevy::audio::AudioSink, With<MusicPlayer>>,
    mut playback: Query<&mut PlaybackSettings, With<MusicPlayer>>,
) {
    use bevy::audio::AudioSinkPlayback;
    let paused = !movie.is_empty() || settings.is_some_and(|s| s.redbook_volume == 0);
    for mut settings in &mut playback {
        settings.paused = paused;
    }
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
    fn death_book_keeps_the_current_cd_track() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let mut settings = SoundSettings::parse("");
        settings.music = true;
        world.insert_resource(settings);
        world.insert_resource(Assets::<AudioSource>::default());
        world.insert_resource(GameInstall {
            root: std::env::temp_dir().join("cyrus-absent-install"),
        });
        world.insert_resource(cyrus_script::script::DeathBookEntry { override_group: 74 });
        world.insert_resource(Music {
            track: Some(7),
            ..default()
        });
        world.run_system_once(start_menu_track).unwrap();
        let music = world.resource::<Music>();
        assert_eq!(music.track, Some(7));
        assert!(music.load.is_none());
        world.remove_resource::<cyrus_script::script::DeathBookEntry>();
        world.insert_resource(cyrus_script::script::LastGameEntry {
            save: cyrus_script::script::LastSaveSelection {
                slot: 17,
                description: "Example".into(),
            },
            override_group: 74,
            retained_book_context: 1,
        });
        world.run_system_once(start_menu_track).unwrap();
        assert_eq!(world.resource::<Music>().track, Some(7));
        assert!(world.resource::<Music>().load.is_none());
    }

    fn prepared_music(source: AudioSource, volume: i32, cinematic: bool) -> App {
        prepared_music_mode(
            source,
            volume,
            cinematic,
            cyrus_game::fidelity::Mode::Faithful,
        )
    }

    fn prepared_music_mode(
        source: AudioSource,
        volume: i32,
        cinematic: bool,
        mode: cyrus_game::fidelity::Mode,
    ) -> App {
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
        app.insert_resource(cyrus_game::fidelity::Fidelity {
            music: mode,
            ..default()
        });
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
    fn cd_data_noise_is_muted_in_both_modes_on_start_and_replay() {
        for mode in [
            cyrus_game::fidelity::Mode::Faithful,
            cyrus_game::fidelity::Mode::Modern,
        ] {
            let mut pcm = vec![7; 2352 * 3];
            for offset in [24, 2376] {
                pcm[offset..offset + 12]
                    .copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0]);
            }
            let source = AudioSource {
                bytes: cyrus_assets::audio::pcm_wav(
                    rgpre::import::sfx::AudioType::Stereo16,
                    44100,
                    &pcm,
                )
                .unwrap()
                .into(),
            };
            let mut app = prepared_music_mode(source.clone(), 64, false, mode);
            for replay in [false, true] {
                if replay {
                    let entity = app
                        .world_mut()
                        .query_filtered::<Entity, With<MusicPlayer>>()
                        .single(app.world())
                        .unwrap();
                    app.world_mut().despawn(entity);
                    app.world_mut()
                        .resource_mut::<NextState<cyrus_game::AppState>>()
                        .set(cyrus_game::AppState::InGame);
                    app.update();
                }
                let player = app
                    .world_mut()
                    .query_filtered::<&AudioPlayer<MusicTransport>, With<MusicPlayer>>()
                    .single(app.world())
                    .unwrap();
                let actual = &app
                    .world()
                    .resource::<Assets<MusicTransport>>()
                    .get(&player.0)
                    .unwrap()
                    .source()
                    .bytes;
                assert_eq!(actual.len(), source.bytes.len());
                assert_eq!(&actual[..44], &source.bytes[..44]);
                assert!(
                    actual[44..44 + 4728].iter().all(|byte| *byte == 0),
                    "{mode:?}, replay={replay}"
                );
                assert_eq!(&actual[44 + 4728..], &source.bytes[44 + 4728..]);
                assert_eq!(
                    app.world()
                        .resource::<Music>()
                        .source
                        .as_ref()
                        .unwrap()
                        .bytes,
                    source.bytes
                );
            }
        }
    }

    #[test]
    fn browser_world_request_stops_named_destination_before_publication() {
        for destination in ["1", "ROOM", "room"] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(SceneRequest(destination.into()))
                .insert_resource(Music {
                    track: Some(2),
                    ..default()
                })
                .insert_resource(
                    serde_json::from_value::<cyrus_assets::sound::MusicSettings>(
                        serde_json::json!({"enabled":true,"volume":64,
                        "world_tracks":{"1":3},"world_names":{"ROOM":1}}),
                    )
                    .unwrap(),
                )
                .add_systems(Update, stop_for_browser_world_entry);
            let old = app.world_mut().spawn(MusicPlayer).id();
            app.update();
            assert_eq!(
                app.world().resource::<Music>().track,
                None,
                "request {destination} kept the retiring track before world publication"
            );
            assert!(app.world().get_entity(old).is_err());
        }
    }

    #[test]
    fn browser_world_request_preserves_same_and_unassigned_music() {
        for destination in ["ROOM", "OTHER", "unknown", "../ROOM"] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(SceneRequest(destination.into()))
                .insert_resource(Music {
                    track: Some(2),
                    ..default()
                })
                .insert_resource(
                    serde_json::from_value::<cyrus_assets::sound::MusicSettings>(
                        serde_json::json!({"enabled":true,"volume":64,
                        "world_tracks":{"1":2,"2":-1},"world_names":{"ROOM":1,"OTHER":2}}),
                    )
                    .unwrap(),
                )
                .add_systems(Update, stop_for_browser_world_entry);
            let old = app.world_mut().spawn(MusicPlayer).id();
            app.update();
            assert_eq!(app.world().resource::<Music>().track, Some(2));
            assert!(app.world().get_entity(old).is_ok());
        }
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
        assert!(music.is_preparing());
        assert!(
            music
                .snapshot(&[])
                .err()
                .unwrap()
                .to_string()
                .contains("preparation is in flight")
        );
        assert!(music.load.is_some());
        assert!(music.is_preparing());
        assert_eq!(music.track, Some(2));
    }

    #[test]
    fn pending_world_track_settles_in_paused_gameplay_book() {
        let source = AudioSource {
            bytes: cyrus_assets::audio::pcm_wav(
                rgpre::import::sfx::AudioType::Stereo16,
                44100,
                &[0, 64, 0, 192],
            )
            .unwrap()
            .into(),
        };
        let mut app = prepared_music(source.clone(), 64, false);
        // Death-book entry retains the world's pending track instead of requesting menu music.
        app.insert_resource(cyrus_script::script::DeathBookEntry { override_group: 0 });
        {
            let mut music = app.world_mut().resource_mut::<Music>();
            music.load = Some(IoTaskPool::get().spawn(async move { Ok(source) }));
            music.world_selection = WorldSelection::AwaitingGameplay;
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
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        app.world_mut()
            .resource_mut::<NextState<cyrus_game::AppState>>()
            .set(cyrus_game::AppState::Menu);
        app.update();
        assert!(app.world().resource::<Music>().is_preparing());
        let clock =
            cyrus_script::clock::SimulationClock::new(cyrus_assets::script::ClockSettings {
                normal_frame_rate: 12,
                min_frame_rate: 6,
                max_frame_rate: 300,
                use_smooth_fps: true,
                use_smooth_divisor: true,
            })
            .unwrap();
        let before = serde_json::to_value(clock.snapshot()).unwrap();
        app.insert_resource(cyrus_script::PausedGameplay {
            clock,
            active_cameras: Vec::new(),
        });
        app.update();
        assert!(
            !app.world().resource::<Music>().is_preparing(),
            "a paused gameplay book must allow pending CD preparation to finish"
        );
        assert_eq!(
            serde_json::to_value(
                app.world()
                    .resource::<cyrus_script::PausedGameplay>()
                    .clock
                    .snapshot()
            )
            .unwrap(),
            before,
            "CD preparation advanced the paused gameplay clock"
        );
    }

    #[test]
    fn production_music_preserves_linear_drive_gain_and_queued_pause() {
        use bevy::audio::{Decodable, PlaybackMode, PlaybackSettings};
        for (volume, cinematic, expected_gain) in [
            (0, false, 0.0),
            (64, false, 64.0 / 255.0),
            (200, false, 200.0 / 255.0),
            (64, true, 64.0 / 255.0),
        ] {
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
            assert_eq!(
                settings.volume.to_linear(),
                expected_gain,
                "engine volume {volume}"
            );
            eprintln!(
                "CD engine volume {volume}: published linear gain {expected_gain}, cinematic {cinematic}"
            );
            let transport = app
                .world()
                .resource::<Assets<MusicTransport>>()
                .get(&player.0)
                .unwrap();
            assert_eq!(transport.decoder().take(6).collect::<Vec<_>>(), [0.5, -0.5]);
            let music = app.world().resource::<Music>();
            let saved = music.snapshot(&[(transport, settings.paused)]).unwrap();
            let candidate = Music::from_snapshot(saved, Some(transport.source().clone())).unwrap();
            let (_, restored_settings) = candidate.playback.unwrap();
            assert_eq!(restored_settings.paused, cinematic || volume == 0);
            assert_eq!(candidate.music.gain, expected_gain);
        }
    }

    #[test]
    fn completed_cd_stays_stopped_in_the_book_and_replays_when_the_world_resumes() {
        use bevy::audio::Decodable;
        let source = AudioSource {
            bytes: cyrus_assets::audio::pcm_wav(
                rgpre::import::sfx::AudioType::Stereo16,
                44100,
                &[0, 64, 0, 192],
            )
            .unwrap()
            .into(),
        };
        let mut app = prepared_music(source.clone(), 64, false);
        app.insert_resource(cyrus_script::script::DeathBookEntry { override_group: 74 });
        app.world_mut()
            .resource_mut::<NextState<cyrus_game::AppState>>()
            .set(cyrus_game::AppState::Menu);
        app.update();
        let (entity, player) = app
            .world_mut()
            .query_filtered::<(Entity, &AudioPlayer<MusicTransport>), With<MusicPlayer>>()
            .single(app.world())
            .unwrap();
        let transport = app
            .world()
            .resource::<Assets<MusicTransport>>()
            .get(&player.0)
            .unwrap();
        assert_eq!(transport.decoder().collect::<Vec<_>>(), [0.5, -0.5]);
        app.world_mut().despawn(entity);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world_mut()
                .query::<&MusicPlayer>()
                .iter(app.world())
                .count(),
            0
        );
        let saved = app.world().resource::<Music>().snapshot(&[]).unwrap();
        assert_eq!(saved.track(), Some(2));
        assert!(saved.source_identity().is_some());
        let wrong_source = AudioSource {
            bytes: cyrus_assets::audio::pcm_wav(
                rgpre::import::sfx::AudioType::Stereo16,
                44100,
                &[0; 4],
            )
            .unwrap()
            .into(),
        };
        assert!(Music::from_snapshot(saved.clone(), Some(wrong_source)).is_err());
        let candidate = Music::from_snapshot(saved, Some(source)).unwrap();
        assert!(candidate.playback.is_none());
        app.insert_resource(candidate.music);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&MusicPlayer>()
                .iter(app.world())
                .count(),
            0
        );
        let movie = app.world_mut().spawn(crate::CinematicAudio).id();
        app.world_mut()
            .resource_mut::<NextState<cyrus_game::AppState>>()
            .set(cyrus_game::AppState::InGame);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&MusicPlayer>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut().despawn(movie);
        app.update();
        let player = app
            .world_mut()
            .query_filtered::<&AudioPlayer<MusicTransport>, With<MusicPlayer>>()
            .single(app.world())
            .unwrap();
        let transport = app
            .world()
            .resource::<Assets<MusicTransport>>()
            .get(&player.0)
            .unwrap();
        assert_eq!(transport.decoder().collect::<Vec<_>>(), [0.5, -0.5]);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&MusicPlayer>()
                .iter(app.world())
                .count(),
            1
        );
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
            bytes: read_track(&music_source(&install).unwrap(), 2)
                .unwrap()
                .into(),
        };
        let expected_source = muted(&source).unwrap_or_else(|| source.clone());
        let expected: Vec<_> = expected_source.decoder().skip(1023).take(1024).collect();
        assert_eq!(expected.len(), 1024);
        let mut app = prepared_music(source.clone(), 64, false);
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
        let candidate = Music::from_snapshot(saved, Some(source)).unwrap();
        let (restored, settings) = candidate.playback.unwrap();
        assert!(!settings.paused);
        assert_eq!(restored.decoder().take(1024).collect::<Vec<_>>(), expected);
        assert_eq!(live.take(1024).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn music_updates_while_the_install_picker_is_open() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_state(cyrus_game::AppState::SelectingInstall)
            .init_resource::<CurrentScene>()
            .add_plugins(MusicPlugin);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<Music>().track, None);
        assert!(app.world().resource::<Music>().load.is_none());
        app.insert_resource(GameInstall {
            root: std::env::temp_dir().join("cyrus-absent-install"),
        });
        app.insert_resource(cyrus_script::script::CreditsPlaying);
        app.update();
        app.world_mut().resource_mut::<Music>().track = Some(2);
        app.world_mut().spawn(MusicPlayer);
        app.world_mut()
            .remove_resource::<cyrus_script::script::CreditsPlaying>();
        app.update();
        assert_eq!(app.world().resource::<Music>().track, Some(2));
        assert!(
            app.world_mut()
                .query::<&MusicPlayer>()
                .iter(app.world())
                .next()
                .is_none()
        );
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
        assert_eq!(app.world().resource::<Music>().gain, 64.0 / 255.0);
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
    fn redbook_volume_clamps_then_halves_before_the_drive_call() {
        for (volume, channel) in [
            (-1, 0),
            (0, 0),
            (1, 0),
            (64, 64),
            (127, 126),
            (128, 128),
            (200, 200),
            (255, 254),
            (256, 254),
        ] {
            assert_eq!(redbook_gain(volume), channel as f32 / 255.0);
        }
    }
}
