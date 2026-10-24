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
//! Script `Sound` supports positional starts without animation surface remapping.
//! AmbientSound has a synchronous shared-channel adapter; maintenance reset ownership is
//! generation-scoped. AmbientRTX starts its clip through the dialogue source and then follows
//! it with positional volume and pan. The player torch effect uses a dedicated one-channel
//! lifecycle, and rope/combat starts use the same shared channel allocator. The decoder supports
//! live gain updates.
//! Animation sounds play only for actors
//! whose [`SoundSurface`] is known, remapped through the installed SURFACE.INI tables.

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
use cyrus_world::sound::ScriptSound;
pub use cyrus_world::sound::SoundCue;
pub use cyrus_world::sound::SoundSurface;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub mod dialogue;
pub mod persistence;
mod samples;
mod script;
use samples::{Reservation, SampleSlots};
#[cfg(target_arch = "wasm32")]
use script::PendingSounds;
pub use script::{RestoredDialogueAssets, RuntimeDialogue, RuntimeDialogueSnapshot, ScriptSounds};
use script::{execute_ordinary, play_audio};

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        if app.world().contains_resource::<AssetServer>() {
            app.init_asset::<dialogue::DialogueClip>()
                .init_asset_loader::<dialogue::DialogueClipLoader>();
        }
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
            .init_resource::<crate::AudioReadiness>()
            .init_resource::<SoundEffects>()
            .init_resource::<Speech>()
            .init_resource::<Channels>()
            .init_resource::<script::RuntimeDialogue>()
            .insert_resource(cyrus_game::script::OrdinarySoundExecutor(execute_ordinary))
            .add_systems(Update, play_audio.in_set(PlayCues));
        #[cfg(target_arch = "wasm32")]
        app.init_resource::<PendingSounds>();
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(
            Update,
            load_effects.run_if(start_audio).in_set(ApplyPreferences),
        )
        .add_systems(Update, finish_speech.in_set(ApplyPreferences));
        #[cfg(target_arch = "wasm32")]
        app.init_asset::<crate::sound_bank::SoundBank>()
            .init_asset_loader::<crate::sound_bank::SoundBankLoader>()
            .add_systems(
                Update,
                (|mut commands: Commands, assets: Res<AssetServer>| {
                    commands.insert_resource(crate::sound_bank::SoundBankHandle(
                        assets.load("cache://sounds/bank.sounds"),
                    ));
                })
                .run_if(start_audio)
                .in_set(ApplyPreferences),
            )
            .add_systems(Update, finish_bank.in_set(ApplyPreferences));
    }
}

fn start_audio(
    readiness: Res<crate::AudioReadiness>,
    #[cfg(not(target_arch = "wasm32"))] install: Option<Res<GameInstall>>,
    settings: Option<Res<SoundSettings>>,
    #[cfg(target_arch = "wasm32")] bank: Option<Res<crate::sound_bank::SoundBankHandle>>,
) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if install.is_none() {
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    if bank.is_some() {
        return false;
    }
    settings.is_none() && readiness.prepare
}

pub use cyrus_assets::sound::SoundSettings;
use cyrus_assets::sound::SurfaceSounds;
/// Effects of `MAIN.SFX` in file order. `None` marks an effect whose PCM
/// cannot be played; it keeps later indices aligned with the file.
#[derive(Resource, Default)]
pub struct SoundEffects(pub Vec<Option<Effect>>);

pub struct Effect {
    /// Original PCM sample width, 8 or 16 bits.
    pub bits_per_sample: u16,
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
    match preferences.menu().clone() {
        Ok(Some(preferences)) => settings.apply_menu(&preferences),
        Ok(None) => {}
        Err(e) => warn!("sound: {e:#}; using SYSTEM.INI volumes"),
    }
    commands.insert_resource(settings.clone());
    match cyrus_assets::audio::read_surface_sounds(&install) {
        Ok(surfaces) => {
            commands.insert_resource(surfaces);
        }
        Err(error) => error!("sound: {error:#}; surface-remapped animation cues unavailable"),
    }
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
                    bits_per_sample: effect.bits_per_sample,
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
                            bits_per_sample: clip.bits_per_sample,
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
    samples: SampleSlots,
    playing: HashMap<u64, Channel>,
    start: usize,
    /// The last sound each animated actor started, for its EndAnimation.
    animation: HashMap<Entity, u8>,
    /// One post-advancement animation slot snapshot per bound actor, including None.
    animation_checkpoint: HashMap<Entity, Option<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Entity(Entity),
    Script { generation: u64, index: usize },
}

struct Channel {
    sound: Entity,
    reservation: Reservation,
    gains: Option<PannedGains>,
}

/// One start: volume 0..=127 before the master volume, Miles pan (`None` = centre).
struct Start {
    bits_per_sample: u16,
    owner: Option<Owner>,
    effect: Option<i32>,
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

#[derive(Clone, Copy)]
struct PositionalCue {
    effect: i32,
    volume: i32,
    rate_offset_hz: i32,
    position: [i32; 3],
    start_index: Option<u8>,
}

#[cfg(target_arch = "wasm32")]
enum PendingCue {
    Flat(Option<Owner>, (usize, i32, i32)),
    Positional(Owner, PositionalCue),
}
#[cfg(target_arch = "wasm32")]
impl PendingCue {
    fn owner(&self) -> Option<Owner> {
        match self {
            Self::Flat(owner, _) => *owner,
            Self::Positional(owner, _) => Some(*owner),
        }
    }
    fn effect(&self) -> Option<i32> {
        match self {
            Self::Flat(_, (effect, _, _)) => i32::try_from(*effect).ok(),
            Self::Positional(_, cue) => Some(cue.effect),
        }
    }
}

impl Channels {
    fn start_positional(
        &mut self,
        commands: &mut Commands,
        mixer: &mut Mixer,
        effects: &SoundEffects,
        cue: PositionalCue,
        owner: Owner,
        listener: (&SoundSettings, &crate::AudioListener),
    ) {
        let Some(effect) = usize::try_from(cue.effect)
            .ok()
            .and_then(|index| effects.0.get(index)?.as_ref())
        else {
            return;
        };
        let (settings, listener) = listener;
        let (volume, pan) = match positional_q8(cue.position, settings, listener, cue.volume) {
            Ok(mix) => mix,
            Err(error) => {
                warn_once!("script Sound effect {}: {error}", cue.effect);
                return;
            }
        };
        self.start(
            commands,
            mixer,
            Start {
                owner: Some(owner),
                effect: Some(cue.effect),
                audio: effect.audio.clone(),
                bits_per_sample: effect.bits_per_sample,
                looping: effect.looping,
                volume,
                pan: Some(pan),
                speed: playback_speed(effect.sample_rate, cue.rate_offset_hz),
                start_index: cue.start_index.map_or(self.start, usize::from),
            },
        );
    }
    fn ambient(
        &mut self,
        commands: &mut Commands,
        mixer: &mut Mixer,
        effects: &SoundEffects,
        request: cyrus_world::sound::AmbientSoundRequest,
        volume: u8,
        pan: u8,
    ) -> Result<cyrus_world::sound::AmbientSoundUpdate, String> {
        if volume == 0 || request.object_update_excluded {
            if let Some(slot) = request.channel {
                self.stop_slot(commands, slot);
            }
            return Ok(cyrus_world::sound::AmbientSoundUpdate {
                channel: request.channel,
                active: false,
            });
        }
        let mut channel = request.channel;
        if self.samples.status(channel) == 2 {
            channel = usize::try_from(request.effect)
                .ok()
                .and_then(|index| effects.0.get(index)?.as_ref())
                .and_then(|effect| {
                    self.start(
                        commands,
                        mixer,
                        Start {
                            owner: Some(Owner::Script {
                                generation: request.generation,
                                index: request.actor_index,
                            }),
                            effect: Some(request.effect),
                            audio: effect.audio.clone(),
                            bits_per_sample: effect.bits_per_sample,
                            looping: effect.looping,
                            volume,
                            pan: Some(pan),
                            speed: playback_speed(effect.sample_rate, request.rate_offset_hz),
                            start_index: self.start,
                        },
                    )
                });
        }
        if let Some(slot) = channel {
            self.update_slot(slot, volume, mixer.master, pan)?;
        }
        Ok(cyrus_world::sound::AmbientSoundUpdate {
            channel,
            active: true,
        })
    }

    fn reclaim(&mut self, alive: &Query<(), With<PlayingSound>>) {
        self.reclaim_where(|sound| alive.contains(sound));
    }

    fn reclaim_where(&mut self, alive: impl Fn(Entity) -> bool) {
        let samples = &mut self.samples;
        self.playing.retain(|_, channel| {
            if alive(channel.sound) {
                true
            } else {
                samples.finish(channel.reservation);
                false
            }
        });
    }

    /// Starts `start` on the first idle channel at or above its start index; a silent
    /// start still occupies its channel. Returns the reusable sample slot.
    fn start(&mut self, commands: &mut Commands, mixer: &mut Mixer, start: Start) -> Option<u8> {
        let master = mixer.master;
        let width = match PcmWidth::try_from(start.bits_per_sample) {
            Ok(width) => width,
            Err(error) => {
                warn!(
                    "sound: effect {:?}, audio {:?}: {error}",
                    start.effect, start.audio
                );
                return None;
            }
        };
        let gain = output_gain(width, start.volume, master);
        let panned_source = match start.pan {
            Some(pan) => Some((
                mixer.panned.as_deref_mut()?,
                mixer.sources?.get(&start.audio)?,
                pan,
            )),
            None => mixer
                .panned
                .as_deref_mut()
                .zip(mixer.sources.and_then(|sources| sources.get(&start.audio)))
                .map(|(panned, source)| (panned, source, 64)),
        };
        let reservation = match self
            .samples
            .start(start.start_index, start.owner, start.effect)
        {
            Ok(reservation) => reservation?,
            Err(error) => {
                error_once!("sound: {error}");
                return None;
            }
        };
        let playback = if start.looping {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        }
        .with_speed(start.speed);
        let (sound, gains) = match panned_source {
            None => (
                commands
                    .spawn((
                        PlayingSound,
                        AudioPlayer(start.audio),
                        playback.with_volume(Volume::Linear(gain)),
                    ))
                    .id(),
                None,
            ),
            Some((panned, source, pan)) => {
                let gains = PannedGains::new(width, start.volume, master, pan);
                let transport = match Panned::with_playback(
                    source.clone(),
                    gains.clone(),
                    start.looping,
                    start.speed,
                ) {
                    Ok(transport) => transport,
                    Err(error) => {
                        self.samples.finish(reservation);
                        warn!(
                            "sound: effect {:?}, audio {:?}: {error:#}",
                            start.effect, start.audio
                        );
                        return None;
                    }
                };
                let playback = transport.playback_settings();
                let audio = panned.add(transport);
                let sound = commands
                    .spawn((PlayingSound, AudioPlayer(audio), playback, gains.clone()))
                    .id();
                (sound, Some(gains))
            }
        };
        debug!(
            "sound start: effect {:?} slot {} volume {} pan {:?} speed {}",
            start.effect, reservation.slot, start.volume, start.pan, start.speed
        );
        self.playing.insert(
            reservation.token,
            Channel {
                sound,
                reservation,
                gains,
            },
        );
        Some(reservation.slot)
    }

    /// The original's `FlatSound`: `(effect, volume offset, rate offset in Hz)` at pan 64
    /// from the current start index. An index beyond MAIN.SFX starts nothing.
    fn start_flat(
        &mut self,
        commands: &mut Commands,
        mixer: &mut Mixer,
        effects: &SoundEffects,
        (index, volume, rate_offset_hz): (usize, i32, i32),
        owner: Option<Owner>,
    ) {
        let Some(effect) = effects.0.get(index).and_then(Option::as_ref) else {
            return;
        };
        let start_index = self.start;
        self.start(
            commands,
            mixer,
            Start {
                owner,
                effect: i32::try_from(index).ok(),
                audio: effect.audio.clone(),
                bits_per_sample: effect.bits_per_sample,
                looping: effect.looping,
                volume: i32::from(effect.base_volume)
                    .wrapping_add(volume)
                    .clamp(0, 127) as u8,
                pan: None,
                speed: playback_speed(effect.sample_rate, rate_offset_hz),
                start_index,
            },
        );
    }

    fn retire(&mut self, commands: &mut Commands, reservations: Vec<Reservation>) {
        for reservation in reservations {
            if let Some(channel) = self.playing.remove(&reservation.token) {
                commands.entity(channel.sound).try_despawn();
            }
        }
    }

    fn stop_matching(
        &mut self,
        commands: &mut Commands,
        owner: Option<Owner>,
        effect: Option<i32>,
    ) {
        let stopped = self.samples.stop_matching(owner, effect);
        self.retire(commands, stopped);
    }

    fn stop_all(&mut self, commands: &mut Commands) {
        self.stop_matching(commands, None, None);
    }

    fn stop_script_generation(&mut self, commands: &mut Commands, generation: u64) {
        let stopped = self.samples.stop_script_generation(generation);
        self.retire(commands, stopped);
    }

    fn update_slot(&self, slot: u8, volume: u8, master: i32, pan: u8) -> Result<(), String> {
        let reservation = self
            .samples
            .current(slot)
            .ok_or_else(|| format!("ambient sample slot {slot}: no current sample"))?;
        let channel = self
            .playing
            .get(&reservation.token)
            .ok_or_else(|| format!("ambient sample slot {slot}: playback transport unavailable"))?;
        let gains = channel.gains.as_ref().ok_or_else(|| {
            format!("ambient sample slot {slot}: positional gain control unavailable")
        })?;
        gains.set(volume, master, pan);
        Ok(())
    }

    fn stop_slot(&mut self, commands: &mut Commands, slot: u8) {
        if let Some(reservation) = self.samples.stop(slot) {
            self.retire(commands, vec![reservation]);
        }
    }
}

/// Positional script and ambient calls share original signed Q8 differences.
fn positional_q8(
    position: [i32; 3],
    settings: &SoundSettings,
    listener: &crate::AudioListener,
    volume_offset: i32,
) -> Result<(u8, u8), String> {
    let distance = settings
        .sound_distance
        .ok_or("SYSTEM.INI sound_distance unavailable")?;
    let camera = listener.0.translation();
    let camera_q8 = [
        (f64::from(camera.x) * -5120.0).floor() as i32,
        (f64::from(camera.y) * -5120.0).floor() as i32,
        (f64::from(camera.z) * 5120.0 + 16_777_215.0).floor() as i32,
    ];
    let difference = std::array::from_fn(|axis| position[axis].wrapping_sub(camera_q8[axis]));
    let length =
        cyrus_game::script::rounded_distance(difference).map_err(|error| error.to_string())?;
    let volume = distance_volume_q8(distance, i64::from(length), volume_offset)?;
    let forward = listener.0.forward();
    let yaw = heading(
        (f64::from(forward.x) * -65536.0).round() as i64,
        (f64::from(forward.z) * 65536.0).round() as i64,
    );
    Ok((
        volume,
        pan(
            heading(i64::from(difference[0]), i64::from(difference[2])),
            yaw,
        ),
    ))
}

/// Playback speed uses a wrapping 32-bit sum before the signed 512..=44000 Hz clamp.
pub fn playback_speed(sample_rate: u32, rate_offset_hz: i32) -> f32 {
    let rate = sample_rate.wrapping_add_signed(rate_offset_hz) as i32;
    rate.clamp(512, 44000) as f32 / sample_rate.max(1) as f32
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
    listener: Option<Res<crate::AudioListener>>,
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
                    warn_once!("sound: effect {index} is not loaded");
                    continue;
                };
                let (volume, pan) = match position {
                    None => (effect.base_volume.min(127), None),
                    Some(position) => {
                        let (Some(distance), Some(listener)) = (
                            settings.as_ref().and_then(|s| s.sound_distance),
                            listener.as_deref(),
                        ) else {
                            warn_once!(
                                "sound: effect {index}: no listener or SYSTEM.INI sound_distance"
                            );
                            continue;
                        };
                        match positional(*position, &listener.0, distance, 0) {
                            Ok((volume, pan)) => (volume, Some(pan)),
                            Err(e) => {
                                warn_once!("sound: effect {index}: {e}");
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
                        owner: None,
                        effect: i32::try_from(*index).ok(),
                        audio: effect.audio.clone(),
                        bits_per_sample: effect.bits_per_sample,
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
                    warn_once!("sound: voice clip {label} is not loaded");
                    continue;
                };
                channels.start(
                    &mut commands,
                    &mut mixer,
                    Start {
                        owner: None,
                        effect: None,
                        audio: clip.audio.clone(),
                        bits_per_sample: clip.bits_per_sample,
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

/// Browser: fills the effects and settings from the exported bank once it has loaded.
#[cfg(target_arch = "wasm32")]
fn finish_bank(
    mut commands: Commands,
    handle: Option<Res<crate::sound_bank::SoundBankHandle>>,
    banks: Res<Assets<crate::sound_bank::SoundBank>>,
    assets: Res<AssetServer>,
    preferences: Res<cyrus_game::preferences::Preferences>,
    mut failure_reported: Local<bool>,
) {
    let Some(handle) = handle else {
        return;
    };
    match assets.recursive_dependency_load_state(&handle.0) {
        bevy::asset::RecursiveDependencyLoadState::Loaded => {}
        bevy::asset::RecursiveDependencyLoadState::Failed(e) => {
            if !*failure_reported {
                warn!("sound: exported bank: {e}");
                *failure_reported = true;
            }
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
    if let Some(music) = &bank.music {
        settings.music = music.enabled;
        settings.redbook_volume = music.volume;
        commands.insert_resource(music.clone());
    } else {
        warn!("sound: CD metadata absent; re-export startup assets for browser music");
    }
    if let Some(preferences) = preferences.menu().as_ref().ok().and_then(Option::as_ref) {
        settings.apply_menu(preferences);
    }
    commands.insert_resource(settings);
    commands.insert_resource(bank.surface_sounds.clone());
    if !bank.enabled {
        return;
    }
    commands.insert_resource(SoundEffects(
        bank.effects
            .iter()
            .zip(&bank.audio)
            .map(|(effect, audio)| {
                Some(Effect {
                    bits_per_sample: effect.bits_per_sample,
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
    distance_volume_q8(
        sound_distance,
        (f64::from(world_units) * 256.0).round() as i64,
        offset,
    )
}

fn distance_volume_q8(sound_distance: i32, distance: i64, offset: i32) -> Result<u8, String> {
    let radius = i64::from(sound_distance) * 36;
    let steps = radius / 128;
    if steps <= 0 {
        return Err(format!(
            "SYSTEM.INI sound_distance {sound_distance} must exceed 3"
        ));
    }
    let divisor = 65536 / steps;
    // Engine positions carry 8 fractional bits; the distance is rounded, then floored to whole units.
    let distance = distance >> 8;
    if distance > radius {
        return Ok(0);
    }
    let attenuation = ((distance * divisor) >> 16).clamp(0, 127);
    Ok((127 - attenuation as i32)
        .wrapping_add(offset)
        .clamp(0, 127) as u8)
}

/// Engine volume for a Sound page slider value: `trunc(slider × 12.8)`, exact as `slider × 64 / 5`.
pub use cyrus_assets::sound::slider_volume;

/// Original integer PCM sample width, retained before Bevy converts samples to floats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PcmWidth {
    Eight,
    Sixteen,
}

impl TryFrom<u16> for PcmWidth {
    type Error = String;
    fn try_from(bits: u16) -> Result<Self, Self::Error> {
        match bits {
            8 => Ok(Self::Eight),
            16 => Ok(Self::Sixteen),
            _ => Err(format!(
                "unsupported PCM sample width {bits}; expected 8 or 16 bits"
            )),
        }
    }
}

/// Nominal centre gain after the game master volume, excluding integer sample rounding.
/// PCM width selects the original table or scalar rule (preservation engine/sound.md).
pub fn output_gain(width: PcmWidth, volume: u8, master: i32) -> f32 {
    PcmScale::new(width, volume, master, 64).apply(1.0, 0)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum PcmScale {
    Eight { volume: i32, pan: [i32; 2] },
    Sixteen([i32; 2]),
}

impl PcmScale {
    fn new(width: PcmWidth, volume: u8, master: i32, pan: u8) -> Self {
        let volume = (i32::from(volume).wrapping_mul(master) >> 8).clamp(0, 127);
        let pan = pan.min(127);
        let table = |p: u8| if p < 63 { i32::from(p) * 2 } else { 128 };
        let pan = [table(127 - pan), table(pan)];
        match width {
            PcmWidth::Eight => Self::Eight {
                volume: if volume == 0 { 0 } else { volume + 1 },
                pan,
            },
            PcmWidth::Sixteen => Self::Sixteen(pan.map(|side| side * volume / 127)),
        }
    }

    fn valid(self, width: PcmWidth) -> bool {
        (0..=127).any(|volume| (0..=127).any(|pan| Self::new(width, volume, 256, pan) == self))
    }

    fn apply(self, sample: f32, channel: usize) -> f32 {
        let sample = (sample * 32768.0).round() as i32;
        let scaled = match self {
            Self::Eight { volume, pan } => (((sample * volume) >> 7) * pan[channel]) >> 7,
            Self::Sixteen(coefficients) => (sample * coefficients[channel]) >> 7,
        };
        scaled as f32 / 32768.0
    }
}

/// Shared PCM scaling. Updates affect subsequent stereo frames without restarting.
#[derive(Component, Clone)]
pub struct PannedGains {
    width: PcmWidth,
    scale: Arc<Mutex<PcmScale>>,
}

impl PannedGains {
    /// Retain source PCM width; volume, master and pan use original engine units.
    pub fn new(width: PcmWidth, volume: u8, master: i32, pan: u8) -> Self {
        Self {
            width,
            scale: Arc::new(Mutex::new(PcmScale::new(width, volume, master, pan))),
        }
    }

    /// Apply the engine's volume-set operation without changing the source PCM width.
    pub fn set(&self, volume: u8, master: i32, pan: u8) {
        *self
            .scale
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            PcmScale::new(self.width, volume, master, pan);
    }

    fn snapshot(&self) -> PcmScale {
        *self
            .scale
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

mod transport;
pub use transport::{Panned, PannedDecoder, PannedSnapshot};

/// First `key=value` of `[section]` in INI text, matching section and key
/// case-insensitively as the original does (preservation `docs/engine/menu.md`).

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApplyPreferences;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayCues;

#[cfg(test)]
mod ambient_tests {
    use super::*;
    use bevy::ecs::world::CommandQueue;

    #[test]
    fn ambient_updates_gains_restarts_finished_slots_and_stops_excluded_emitters() {
        let world = World::new();
        let mut queue = CommandQueue::default();
        let mut sources = Assets::<AudioSource>::default();
        let audio = sources.add(AudioSource {
            bytes: cyrus_assets::audio::pcm_wav(
                rgpre::import::sfx::AudioType::Mono8,
                11025,
                &[192; 8],
            )
            .unwrap()
            .into(),
        });
        let effects = SoundEffects(vec![Some(Effect {
            bits_per_sample: 8,
            audio,
            looping: false,
            base_volume: 64,
            sample_rate: 11025,
        })]);
        let mut panned = Assets::<Panned>::default();
        let mut channels = Channels::default();
        let mut request = cyrus_world::sound::AmbientSoundRequest {
            generation: 1,
            actor_index: 2,
            channel: None,
            effect: 0,
            volume: 0,
            rate_offset_hz: 0,
            position: [0; 3],
            object_update_excluded: false,
        };
        let mut invoke = |channels: &mut Channels, request, volume, pan| {
            let mut commands = Commands::new(&mut queue, &world);
            let mut mixer = Mixer {
                master: 256,
                sources: Some(&sources),
                panned: Some(&mut panned),
            };
            channels
                .ambient(&mut commands, &mut mixer, &effects, request, volume, pan)
                .unwrap()
        };
        let first = invoke(&mut channels, request, 127, 64);
        assert_eq!(first.channel, Some(0));
        assert!(first.active);
        request.channel = first.channel;
        let reservation = channels.samples.current(0).unwrap();
        let gains = channels.playing[&reservation.token]
            .gains
            .as_ref()
            .unwrap()
            .clone();
        let second = invoke(&mut channels, request, 32, 127);
        assert_eq!(second.channel, Some(0));
        assert_eq!(channels.playing.len(), 1);
        assert_eq!(
            [
                gains.snapshot().apply(0.5, 0),
                gains.snapshot().apply(0.5, 1)
            ],
            [0.0, 0.12890625]
        );
        channels.samples.finish(reservation);
        channels.playing.remove(&reservation.token);
        let restarted = invoke(&mut channels, request, 64, 64);
        assert_eq!(restarted.channel, Some(0));
        assert_ne!(channels.samples.current(0).unwrap(), reservation);
        request.object_update_excluded = true;
        let stopped = invoke(&mut channels, request, 127, 64);
        assert!(!stopped.active);
        assert_eq!(stopped.channel, Some(0));
        assert_eq!(channels.samples.status(Some(0)), 2);
        assert!(channels.playing.is_empty());
    }

    #[test]
    fn ambient_allocation_failure_stays_active_and_zero_volume_completes() {
        let world = World::new();
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut mixer = Mixer {
            master: 256,
            sources: None,
            panned: None,
        };
        let mut channels = Channels::default();
        let request = cyrus_world::sound::AmbientSoundRequest {
            generation: 1,
            actor_index: 2,
            channel: None,
            effect: -1,
            volume: 0,
            rate_offset_hz: 0,
            position: [0; 3],
            object_update_excluded: false,
        };
        let active = channels
            .ambient(
                &mut commands,
                &mut mixer,
                &SoundEffects::default(),
                request,
                64,
                64,
            )
            .unwrap();
        assert_eq!(active.channel, None);
        assert!(active.active);
        let stopped = channels
            .ambient(
                &mut commands,
                &mut mixer,
                &SoundEffects::default(),
                request,
                0,
                64,
            )
            .unwrap();
        assert!(!stopped.active);
        assert_eq!(stopped.channel, None);
    }
}
