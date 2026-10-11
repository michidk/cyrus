//! Sound effects from the install's `sound/MAIN.SFX`, addressed by their
//! 0-based index in the file, and voice clips from the `SYSTEM.INI`
//! `rtx_filename` container, addressed by their 4-character RTX label. Each
//! plays at its stored sample rate; effects loop when their loop flag is
//! non-zero (preservation `docs/formats/SFX.md`, `docs/formats/RTX.md`).
//!
//! Every start follows preservation `docs/engine/sound.md`: 16 channels taken
//! from a start index and never stolen, a rate clamped to 512–44000 Hz, and the master volume. A
//! non-positional start plays at the clip's base volume plus its offset; a
//! positional one falls off linearly with the distance from the camera within
//! `sound_distance × 36` world units and pans by the emitter's heading, with the
//! Miles mixer's gains (`output_gain`, `pan_gains`).
//!
//! Triggers arrive as [`SoundCue`] (game code) or [`SoundRequest`] (control).
//! Native builds read the install; the browser plays the exported bank.
//!
//! Unsupported until their callers exist: script (`Sound`, `AmbientSound`,
//! `AmbientRTX`, `EndSound`), rope, torch and combat starts, volume refresh while
//! playing, and SURFACE.INI remapping: animation sounds play only for actors
//! whose [`SoundSurface`] is known to be 0 (no SURFACE.INI parser yet).

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::audio::load_effect_bank;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::audio::load_speech;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::install::GameInstall;
use cyrus_world::animation::AnimationSound;
pub use cyrus_world::sound::SoundCue;
pub use cyrus_world::sound::SoundSurface;
use std::collections::HashMap;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            bevy::audio::AddAudioSource::add_audio_source::<Panned>(app);
        }
        app.init_resource::<cyrus_game::preferences::Preferences>()
            .add_message::<SoundRequest>()
            .add_message::<SoundCue>()
            .configure_sets(
                Update,
                (
                    ApplyPreferences,
                    cyrus_world::animation::AnimationSystems,
                    PlayCues,
                )
                    .chain(),
            )
            .init_resource::<SoundEffects>()
            .init_resource::<Speech>()
            .init_resource::<Channels>()
            .add_systems(
                Update,
                (
                    apply_menu
                        .in_set(ApplyPreferences)
                        .run_if(resource_exists_and_changed::<crate::startup::MenuState>),
                    play_requests,
                    play_cues.in_set(PlayCues),
                )
                    .chain(),
            );
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Startup, load_effects)
            .add_systems(Update, finish_speech.before(play_requests));
        #[cfg(target_arch = "wasm32")]
        app.init_asset::<crate::sound_bank::SoundBank>()
            .init_asset_loader::<crate::sound_bank::SoundBankLoader>()
            .add_systems(
                Startup,
                |mut commands: Commands, assets: Res<AssetServer>| {
                    commands.insert_resource(crate::sound_bank::SoundBankHandle(
                        assets.load("cache://sounds/bank.sounds"),
                    ));
                },
            )
            .add_systems(Update, finish_bank.before(play_requests));
    }
}

pub use cyrus_assets::sound::SoundSettings;
/// Effects of `MAIN.SFX` in file order. `None` marks an effect whose PCM
/// cannot be played; it keeps later indices aligned with the file.
#[derive(Resource, Default)]
pub struct SoundEffects(pub Vec<Option<Effect>>);

pub struct Effect {
    pub audio: Handle<AudioSource>,
    pub looping: bool,
    /// Header volume of non-positional starts, 0..=127.
    pub base_volume: u8,
    pub sample_rate: u32,
}

/// Voice clips by RTX label. Empty until the background load finishes.
#[derive(Resource, Default)]
pub struct Speech(pub HashMap<String, VoiceClip<Handle<AudioSource>>>);

use cyrus_assets::sound::VoiceClip;

// shortcut: holds every voice clip's PCM in memory; read clips on demand once rgpre exposes the RTX index.
#[cfg(not(target_arch = "wasm32"))]
type PreparedSpeech = HashMap<String, VoiceClip<Vec<u8>>>;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct SpeechLoad(Task<Result<PreparedSpeech, String>>);

#[derive(Message, Debug, Clone, PartialEq)]
pub enum SoundRequest {
    /// Effect index; with a position (Bevy world space) it attenuates with the camera's distance.
    Play {
        effect: usize,
        position: Option<Vec3>,
    },
    Speech(String),
    StopAll,
}

/// A playing effect or voice clip.
#[derive(Component)]
pub struct PlayingSound;

#[cfg(not(target_arch = "wasm32"))]
use cyrus_assets::audio::read_settings;
/// `Assets<AudioSource>` is absent without `AudioPlugin` (headless); then nothing loads.
#[cfg(not(target_arch = "wasm32"))]
fn load_effects(
    preferences: Res<cyrus_game::preferences::Preferences>,
    mut commands: Commands,
    install: Res<GameInstall>,
    audio: Option<ResMut<Assets<AudioSource>>>,
) {
    let mut settings = read_settings(&install);
    match preferences.menu.clone() {
        Ok(Some(preferences)) => settings.apply_menu(&preferences),
        Ok(None) => {}
        Err(e) => warn!("sound: {e:#}; using SYSTEM.INI volumes"),
    }
    commands.insert_resource(settings.clone());
    let Some(mut audio) = audio else {
        info!("sound: no audio plugin, nothing loaded");
        return;
    };
    // Voice can be switched on later from the Sound page, so clips load either way.
    {
        let path = install.root.join(&settings.rtx_filename);
        commands.insert_resource(SpeechLoad(
            IoTaskPool::get().spawn(async move { load_speech(&path) }),
        ));
    }
    if !settings.effects {
        return;
    }
    let effects = match load_effect_bank(&install) {
        Ok(effects) => effects
            .into_iter()
            .map(|effect| {
                effect.map(|effect| Effect {
                    audio: audio.add(AudioSource {
                        bytes: effect.bytes.into(),
                    }),
                    looping: effect.looping,
                    base_volume: effect.base_volume,
                    sample_rate: effect.sample_rate,
                })
            })
            .collect(),
        Err(error) => {
            error!("sound: {error}");
            return;
        }
    };
    commands.insert_resource(SoundEffects(effects));
}

#[cfg(not(target_arch = "wasm32"))]
fn finish_speech(
    mut commands: Commands,
    load: Option<ResMut<SpeechLoad>>,
    mut speech: ResMut<Speech>,
    audio: Option<ResMut<Assets<AudioSource>>>,
) {
    let (Some(mut load), Some(mut audio)) = (load, audio) else {
        return;
    };
    let Some(result) = check_ready(&mut load.0) else {
        return;
    };
    commands.remove_resource::<SpeechLoad>();
    match result {
        Ok(clips) => {
            speech.0 = clips
                .into_iter()
                .map(|(label, clip)| {
                    let audio = audio.add(AudioSource {
                        bytes: clip.audio.into(),
                    });
                    (
                        label,
                        VoiceClip {
                            audio,
                            base_volume: clip.base_volume,
                            seconds: clip.seconds,
                            text: clip.text,
                        },
                    )
                })
                .collect();
        }
        Err(e) => error!("sound: {e}"),
    }
}

/// The original's 16 sample channels. A start takes the first idle channel at or
/// above a start index and is dropped when none is idle; playing sounds are never
/// stolen. The start index is 0 at boot; animation and speech starts set it to 6.
#[derive(Resource, Default)]
pub(crate) struct Channels {
    slots: [Option<Channel>; 16],
    start: usize,
    /// The last sound each animated actor started, for its EndAnimation.
    animation: HashMap<Entity, Entity>,
}

// shortcut: channels record only their sound; add the owner and requested effect with EndSound, EndMySound(s) and KillMe.
struct Channel {
    sound: Entity,
}

/// One start: volume 0..=127 before the master volume, Miles pan (`None` = centre).
struct Start {
    audio: Handle<AudioSource>,
    looping: bool,
    volume: u8,
    pan: Option<u8>,
    speed: f32,
    start_index: usize,
}

/// Mixer state a start needs: the master volume and the audio assets.
struct Mixer<'a> {
    master: i32,
    sources: Option<&'a Assets<AudioSource>>,
    panned: Option<&'a mut Assets<Panned>>,
}

impl Channels {
    fn reclaim(&mut self, alive: &Query<(), With<PlayingSound>>) {
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|channel| !alive.contains(channel.sound))
            {
                *slot = None;
            }
        }
    }

    /// Starts `start` on the first idle channel at or above its start index; a silent
    /// start still occupies its channel. Returns the playing entity.
    fn start(
        &mut self,
        commands: &mut Commands,
        mixer: &mut Mixer,
        start: Start,
    ) -> Option<Entity> {
        let slot = (start.start_index..16).find(|&i| self.slots[i].is_none())?;
        let gain = output_gain(start.volume, mixer.master);
        let playback = if start.looping {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        }
        .with_speed(start.speed);
        let sound = match start.pan {
            None => commands
                .spawn((
                    PlayingSound,
                    AudioPlayer(start.audio),
                    playback.with_volume(Volume::Linear(gain)),
                ))
                .id(),
            Some(pan) => {
                let (Some(panned), Some(source)) = (
                    mixer.panned.as_mut(),
                    mixer.sources.and_then(|s| s.get(&start.audio)),
                ) else {
                    return None;
                };
                let [left, right] = pan_gains(pan);
                let audio = panned.add(Panned {
                    source: source.clone(),
                    gains: [left * gain, right * gain],
                });
                commands
                    .spawn((PlayingSound, AudioPlayer(audio), playback))
                    .id()
            }
        };
        self.slots[slot] = Some(Channel { sound });
        Some(sound)
    }

    /// The original's `FlatSound`: `(effect, volume offset, rate offset in Hz)` at pan 64
    /// from the current start index. An index beyond MAIN.SFX starts nothing.
    fn start_flat(
        &mut self,
        commands: &mut Commands,
        mixer: &mut Mixer,
        effects: &SoundEffects,
        (index, volume, rate_offset_hz): (usize, i32, i32),
    ) {
        let Some(effect) = effects.0.get(index).and_then(Option::as_ref) else {
            return;
        };
        let start_index = self.start;
        self.start(
            commands,
            mixer,
            Start {
                audio: effect.audio.clone(),
                looping: effect.looping,
                volume: (i32::from(effect.base_volume) + volume).clamp(0, 127) as u8,
                pan: None,
                speed: playback_speed(effect.sample_rate, rate_offset_hz),
                start_index,
            },
        );
    }

    fn stop_all(&mut self, commands: &mut Commands) {
        for channel in self.slots.iter_mut().filter_map(Option::take) {
            commands.entity(channel.sound).try_despawn();
        }
        self.animation.clear();
    }
}

/// Playback speed for a rate offset: `clamp(rate + offset, 512, 44000) / rate`.
pub fn playback_speed(sample_rate: u32, rate_offset_hz: i32) -> f32 {
    let rate = i64::from(sample_rate);
    (rate + i64::from(rate_offset_hz)).clamp(512, 44000) as f32 / rate.max(1) as f32
}

/// Volume and pan of an emitter at Bevy `position` heard from `camera`, with a
/// per-call volume offset.
fn positional(
    position: Vec3,
    camera: &GlobalTransform,
    sound_distance: i32,
    offset: i32,
) -> Result<(u8, u8), String> {
    // Bevy space is engine space ÷ 20 with X mirrored; the original works in 1/256 world units.
    let raw = |bevy: f32, sign: f32| {
        (f64::from(bevy * sign * cyrus_game::ENGINE_UNIT_SCALE) * 256.0).round() as i64
    };
    let offset_vector = position - camera.translation();
    let world_units = offset_vector.length() * cyrus_game::ENGINE_UNIT_SCALE;
    let volume = distance_volume(sound_distance, world_units, offset)?;
    let forward = camera.forward();
    let yaw = heading(raw(forward.x, -65536.0), raw(forward.z, 65536.0));
    let pan = pan(
        heading(raw(offset_vector.x, -1.0), raw(offset_vector.z, 1.0)),
        yaw,
    );
    Ok((volume, pan))
}

#[allow(clippy::too_many_arguments)]
fn play_requests(
    mut commands: Commands,
    mut requests: MessageReader<SoundRequest>,
    effects: Res<SoundEffects>,
    speech: Res<Speech>,
    settings: Option<Res<SoundSettings>>,
    camera: Query<&GlobalTransform, With<crate::camera::ViewCamera>>,
    alive: Query<(), With<PlayingSound>>,
    sources: Option<Res<Assets<AudioSource>>>,
    mut panned: Option<ResMut<Assets<Panned>>>,
    mut channels: ResMut<Channels>,
) {
    channels.reclaim(&alive);
    let mut mixer = Mixer {
        master: settings.as_ref().map_or(256, |s| s.master_volume),
        sources: sources.as_deref(),
        panned: panned.as_deref_mut(),
    };
    for request in requests.read() {
        match request {
            SoundRequest::Play {
                effect: index,
                position,
            } => {
                let Some(effect) = effects.0.get(*index).and_then(Option::as_ref) else {
                    warn!("sound: effect {index} is not loaded");
                    continue;
                };
                let (volume, pan) = match position {
                    None => (effect.base_volume.min(127), None),
                    Some(position) => {
                        let (Some(distance), Ok(camera)) = (
                            settings.as_ref().and_then(|s| s.sound_distance),
                            camera.single(),
                        ) else {
                            warn!("sound: effect {index}: no camera or SYSTEM.INI sound_distance");
                            continue;
                        };
                        match positional(*position, camera, distance, 0) {
                            Ok((volume, pan)) => (volume, Some(pan)),
                            Err(e) => {
                                warn!("sound: effect {index}: {e}");
                                continue;
                            }
                        }
                    }
                };
                let start_index = channels.start;
                channels.start(
                    &mut commands,
                    &mut mixer,
                    Start {
                        audio: effect.audio.clone(),
                        looping: effect.looping,
                        volume,
                        pan,
                        speed: 1.0,
                        start_index,
                    },
                );
            }
            SoundRequest::Speech(label) => {
                if !settings.as_ref().is_some_and(|s| s.speech) {
                    continue;
                }
                let Some(clip) = speech.0.get(label) else {
                    warn!("sound: voice clip {label} is not loaded");
                    continue;
                };
                channels.start(
                    &mut commands,
                    &mut mixer,
                    Start {
                        audio: clip.audio.clone(),
                        looping: false,
                        volume: clip.base_volume.min(127),
                        pan: None,
                        speed: 1.0,
                        start_index: 0,
                    },
                );
                channels.start = 6;
            }
            SoundRequest::StopAll => channels.stop_all(&mut commands),
        }
    }
}

/// Plays [`SoundCue`]s from game code through the same channels as control requests.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play_cues(
    mut commands: Commands,
    mut cues: MessageReader<SoundCue>,
    effects: Res<SoundEffects>,
    settings: Option<Res<SoundSettings>>,
    camera: Query<&GlobalTransform, With<crate::camera::ViewCamera>>,
    actors: Query<(
        &GlobalTransform,
        Option<&SoundSurface>,
        Has<cyrus_game::player::Player>,
    )>,
    alive: Query<(), With<PlayingSound>>,
    sources: Option<Res<Assets<AudioSource>>>,
    mut panned: Option<ResMut<Assets<Panned>>>,
    mut channels: ResMut<Channels>,
    #[cfg(target_arch = "wasm32")] mut pending: Local<Vec<(usize, i32, i32)>>,
    #[cfg(target_arch = "wasm32")] bank_loading: Option<Res<crate::sound_bank::SoundBankHandle>>,
) {
    channels.reclaim(&alive);
    let mut mixer = Mixer {
        master: settings.as_ref().map_or(256, |s| s.master_volume),
        sources: sources.as_deref(),
        panned: panned.as_deref_mut(),
    };
    // Cues held while the bank loaded play once it has; a failed or disabled bank drops them.
    #[cfg(target_arch = "wasm32")]
    if bank_loading.is_none() {
        for flat in pending.drain(..) {
            channels.start_flat(&mut commands, &mut mixer, &effects, flat);
        }
    }
    // A despawned actor's remembered sound is forgotten, not stopped.
    channels
        .animation
        .retain(|actor, _| actors.contains(*actor));
    for cue in cues.read() {
        match cue {
            SoundCue::Flat {
                effect,
                volume,
                rate_offset_hz,
            } => {
                // Browser cues wait for the exported bank instead of being lost.
                #[cfg(target_arch = "wasm32")]
                if bank_loading.is_some() {
                    pending.push((*effect, *volume, *rate_offset_hz));
                    continue;
                }
                channels.start_flat(
                    &mut commands,
                    &mut mixer,
                    &effects,
                    (*effect, *volume, *rate_offset_hz),
                );
            }
            SoundCue::Animation { actor, sound } => match sound {
                AnimationSound::Play {
                    effect: requested,
                    rate_offset_hz,
                } => {
                    channels.animation.remove(actor);
                    let Some((index, effect)) = usize::try_from(*requested)
                        .ok()
                        .and_then(|i| Some((i, effects.0.get(i)?.as_ref()?)))
                    else {
                        continue;
                    };
                    let Ok((transform, surface, is_player)) = actors.get(*actor) else {
                        warn!("sound: animation cue for {actor} without a transform");
                        continue;
                    };
                    // The original remaps by the actor's surface; only a known surface 0 needs no table.
                    match surface {
                        Some(SoundSurface(0)) => {}
                        Some(SoundSurface(surface)) => {
                            warn_once!(
                                "sound: SURFACE.INI remap for surface {surface} is unsupported; animation sounds on it are not played"
                            );
                            continue;
                        }
                        None => {
                            warn_once!(
                                "sound: animation sound for an actor without a known surface is unsupported"
                            );
                            continue;
                        }
                    }
                    let (Some(distance), Ok(camera)) = (
                        settings.as_ref().and_then(|s| s.sound_distance),
                        camera.single(),
                    ) else {
                        continue;
                    };
                    let (volume, pan) =
                        match positional(transform.translation(), camera, distance, 0) {
                            Ok(mix) => mix,
                            Err(e) => {
                                warn!("sound: effect {index}: {e}");
                                continue;
                            }
                        };
                    let started = channels.start(
                        &mut commands,
                        &mut mixer,
                        Start {
                            audio: effect.audio.clone(),
                            looping: effect.looping,
                            volume,
                            pan: Some(pan),
                            speed: playback_speed(effect.sample_rate, *rate_offset_hz),
                            start_index: if is_player { 0 } else { 6 },
                        },
                    );
                    channels.start = 6;
                    if let Some(sound) = started {
                        channels.animation.insert(*actor, sound);
                    }
                }
                AnimationSound::Stop => {
                    if let Some(sound) = channels.animation.remove(actor) {
                        for slot in &mut channels.slots {
                            if slot.as_ref().is_some_and(|channel| channel.sound == sound) {
                                *slot = None;
                            }
                        }
                        commands.entity(sound).try_despawn();
                    }
                }
                AnimationSound::Forget => {
                    channels.animation.remove(actor);
                }
            },
        }
    }
}

/// Browser: fills the effects and settings from the exported bank once it has loaded.
#[cfg(target_arch = "wasm32")]
fn finish_bank(
    mut commands: Commands,
    handle: Option<Res<crate::sound_bank::SoundBankHandle>>,
    banks: Res<Assets<crate::sound_bank::SoundBank>>,
    assets: Res<AssetServer>,
    menu: Option<Res<crate::startup::MenuState>>,
) {
    let Some(handle) = handle else {
        return;
    };
    match assets.recursive_dependency_load_state(&handle.0) {
        bevy::asset::RecursiveDependencyLoadState::Loaded => {}
        bevy::asset::RecursiveDependencyLoadState::Failed(e) => {
            warn!("sound: exported bank: {e}");
            commands.remove_resource::<crate::sound_bank::SoundBankHandle>();
            return;
        }
        _ => return,
    }
    commands.remove_resource::<crate::sound_bank::SoundBankHandle>();
    let Some(bank) = banks.get(&handle.0) else {
        return;
    };
    let mut settings = SoundSettings::parse("");
    settings.master_volume = bank.master_volume;
    settings.sound_distance = bank.sound_distance;
    settings.effects = bank.enabled;
    if let Some(preferences) = menu.as_ref().and_then(|m| m.preferences.as_ref()) {
        settings.apply_menu(preferences);
    }
    commands.insert_resource(settings);
    if !bank.enabled {
        return;
    }
    commands.insert_resource(SoundEffects(
        bank.effects
            .iter()
            .zip(&bank.audio)
            .map(|(effect, audio)| {
                Some(Effect {
                    audio: audio.clone(),
                    looping: effect.looping,
                    base_volume: effect.base_volume,
                    sample_rate: effect.sample_rate,
                })
            })
            .collect(),
    ));
}

/// Positional volume 0..=127 for an emitter `world_units` from the listener,
/// with a per-call `offset` (preservation `docs/engine/sound.md`). Values of
/// `sound_distance` up to 3 make the original divide by zero and are rejected.
pub fn distance_volume(sound_distance: i32, world_units: f32, offset: i32) -> Result<u8, String> {
    let radius = i64::from(sound_distance) * 36;
    let steps = radius / 128;
    if steps <= 0 {
        return Err(format!(
            "SYSTEM.INI sound_distance {sound_distance} must exceed 3"
        ));
    }
    let divisor = 65536 / steps;
    // Engine positions carry 8 fractional bits; the distance is rounded, then floored to whole units.
    let distance = (f64::from(world_units) * 256.0).round() as i64 >> 8;
    if distance > radius {
        return Ok(0);
    }
    let attenuation = ((distance * divisor) >> 16).clamp(0, 127);
    Ok((127 - attenuation + i64::from(offset)).clamp(0, 127) as u8)
}

/// Keeps the sound settings in step with the book's Sound page.
pub(crate) fn apply_menu(
    menu: Res<crate::startup::MenuState>,
    settings: Option<ResMut<SoundSettings>>,
) {
    if let (Some(preferences), Some(mut settings)) = (&menu.preferences, settings) {
        let mut next = settings.clone();
        next.apply_menu(preferences);
        if (next.master_volume, next.redbook_volume, next.speech)
            != (
                settings.master_volume,
                settings.redbook_volume,
                settings.speech,
            )
        {
            *settings = next;
        }
    }
}

/// Engine volume for a Sound page slider value: `trunc(slider × 12.8)`, exact as `slider × 64 / 5`.
pub use cyrus_assets::sound::slider_volume;

/// Linear gain of a 0..=127 sample volume after the master volume, as the
/// original's Miles mixer applies it: `v = volume × master >> 8`, then
/// `(v + 1) / 128` for any non-zero `v` (the driver's own master stays at 127).
pub fn output_gain(volume: u8, master: i32) -> f32 {
    match (i32::from(volume) * master) >> 8 {
        v if v <= 0 => 0.0,
        v => (v.min(127) + 1) as f32 / 128.0,
    }
}

/// Left and right gains of a Miles pan 0..=127 (0 left, 64 centre, 127 right):
/// each side scales by `T[p] / 128` with `T[p] = 2p` below 63 and 128 from 63,
/// using `p = pan` on the right and `127 − pan` on the left.
pub fn pan_gains(pan: u8) -> [f32; 2] {
    let side = |p: u8| {
        if p < 63 {
            f32::from(p) * 2.0 / 128.0
        } else {
            1.0
        }
    };
    let pan = pan.min(127);
    [side(127 - pan), side(pan)]
}

/// The engine's integer heading of `(dx, dz)` in 1/256 world units, 2048 per
/// turn: 0 along +Z, 512 along +X (preservation `docs/engine/sound.md`).
pub fn heading(dx: i64, dz: i64) -> i32 {
    let (mut x, mut z) = (dx.abs(), dz.abs());
    while x > 511 || z > 511 {
        x >>= 1;
        z >>= 1;
    }
    if x == 0 {
        return if dz >= 0 { 0 } else { 1024 };
    }
    if z == 0 {
        return if dx >= 0 { 512 } else { 1536 };
    }
    let table = |i: i64| {
        ((((i as f64) / 512.0).atan() * 65536.0 / std::f64::consts::PI) as i64 >> 6) as i32
    };
    let t = table(x.min(z) * 512 / x.max(z));
    let base = match x.cmp(&z) {
        std::cmp::Ordering::Less => t,
        std::cmp::Ordering::Greater => 512 - t,
        std::cmp::Ordering::Equal => 256,
    };
    let angle = match (dx < 0, dz < 0) {
        (true, false) => -base,
        (false, true) => 1024 - base,
        (true, true) => base + 1024,
        (false, false) => base,
    };
    angle & 2047
}

/// Pan 1..=127 of an emitter at heading `angle` for a listener facing `yaw`:
/// a triangle wave, centred ahead and behind, fully to one side at a quarter
/// turn, and centred again at exactly a quarter turn (the original's quirk).
pub fn pan(angle: i32, yaw: i32) -> u8 {
    let u = (angle - yaw) & 2047;
    let p = match u {
        0..512 => u >> 3,
        512..1024 => ((1024 - u) & 511) >> 3,
        1024..1536 => -((u - 1024) >> 3),
        _ => -(((2048 - u) & 511) >> 3),
    };
    (64 + p) as u8
}

/// An effect with fixed left/right gains, decoded to stereo.
#[derive(Asset, TypePath, Clone)]
pub struct Panned {
    pub source: AudioSource,
    pub gains: [f32; 2],
}

impl bevy::audio::Decodable for Panned {
    type Decoder = PannedDecoder;
    fn decoder(&self) -> PannedDecoder {
        PannedDecoder {
            inner: bevy::audio::Decodable::decoder(&self.source),
            gains: self.gains,
            right: None,
            channel: 0,
        }
    }
}

pub struct PannedDecoder {
    inner: <AudioSource as bevy::audio::Decodable>::Decoder,
    gains: [f32; 2],
    right: Option<f32>,
    channel: u16,
}

impl Iterator for PannedDecoder {
    type Item = bevy::audio::Sample;
    fn next(&mut self) -> Option<Self::Item> {
        use bevy::audio::Source;
        if self.inner.channels().get() == 1 {
            if let Some(right) = self.right.take() {
                return Some(right);
            }
            let sample = self.inner.next()?;
            self.right = Some(sample * self.gains[1]);
            return Some(sample * self.gains[0]);
        }
        let sample = self.inner.next()?;
        let gain = self.gains[usize::from(self.channel % 2)];
        self.channel = (self.channel + 1) % self.inner.channels().get();
        Some(sample * gain)
    }
}

impl bevy::audio::Source for PannedDecoder {
    fn current_span_len(&self) -> Option<usize> {
        let channels = usize::from(self.inner.channels().get());
        self.inner.current_span_len().map(|n| n * 2 / channels)
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        bevy::audio::ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}

/// First `key=value` of `[section]` in INI text, matching section and key
/// case-insensitively as the original does (preservation `docs/engine/menu.md`).

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApplyPreferences;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayCues;
