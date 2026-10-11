//! Sound effects from the install's `sound/MAIN.SFX`, addressed by their
//! 0-based index in the file, and voice clips from the `SYSTEM.INI`
//! `rtx_filename` container, addressed by their 4-character RTX label. Each
//! plays at its stored sample rate; effects loop when their loop flag is
//! non-zero (preservation `docs/formats/SFX.md`, `docs/formats/RTX.md`).
//!
//! Volume follows preservation `docs/engine/sound.md`: a non-positional start
//! plays at the clip's base volume; a positional one falls off linearly with
//! the distance from the camera within `sound_distance × 36` world units; both
//! are scaled by `[system] volume`.
//!
//! Positional effects pan by the emitter's heading from the camera, using
//! Miles' pan law; gains follow the original's Miles mixer (`output_gain`, `pan_gains`).
//!
//! Unsupported until evidence is published: script, animation, dialogue and
//! ambient triggers, volume and pan refresh while playing, and surface remapping.

use crate::install::GameInstall;
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use rgpre::import::rtx::{RtxEntry, parse_rtx_file};
use rgpre::import::sfx::{AudioType, parse_sfx_file};
use std::collections::HashMap;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            bevy::audio::AddAudioSource::add_audio_source::<Panned>(app);
        }
        app.add_message::<SoundRequest>()
            .add_systems(Startup, load_effects)
            .add_systems(
                Update,
                (
                    apply_menu.run_if(resource_exists_and_changed::<crate::startup::MenuState>),
                    finish_speech,
                    play_requests,
                )
                    .chain(),
            );
    }
}

/// `SYSTEM.INI` audio switches (preservation `docs/config/system-ini.md`).
#[derive(Resource, Debug, Clone)]
pub struct SoundSettings {
    /// `[system] sound`, default on.
    pub effects: bool,
    /// `[system] redbook`, default on.
    pub music: bool,
    /// `[dialog] dialog_use_speech`, default on.
    pub speech: bool,
    /// `[dialog] dialog_print_text`: draw subtitles, default on.
    pub subtitles: bool,
    /// `[system] rtx_filename`, relative to the install root.
    pub rtx_filename: String,
    /// `[system] redbook_volume`; `-1` when absent or not an integer.
    pub redbook_volume: i32,
    /// `[system] volume`, the master volume for effects and speech; 256 when absent.
    pub master_volume: i32,
    /// `[system] sound_distance`, in units of 36 world units; `None` when absent.
    pub sound_distance: Option<i32>,
}

impl SoundSettings {
    /// Overrides the SYSTEM.INI values with the book's saved Sound page preferences:
    /// actions 601 (sound) and 602 (music) in slider units, 603 (voice) as 0/1.
    pub fn apply_menu(&mut self, preferences: &crate::startup::MenuPreferences) {
        let value = |action| preferences.values.get(&action).copied();
        if let Some(sound) = value(601) {
            self.master_volume = slider_volume(sound);
        }
        if let Some(music) = value(602) {
            self.redbook_volume = slider_volume(music);
        }
        if let Some(voice) = value(603) {
            self.speech = voice != 0;
        }
        if let Some(subtitles) = value(504) {
            self.subtitles = subtitles != 0;
        }
    }

    pub fn parse(ini: &str) -> Self {
        let on = |section, key| {
            !ini_value(ini, section, key).is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
        };
        Self {
            effects: on("system", "sound"),
            music: on("system", "redbook"),
            speech: on("dialog", "dialog_use_speech"),
            subtitles: on("dialog", "dialog_print_text"),
            rtx_filename: ini_value(ini, "system", "rtx_filename")
                .unwrap_or("ENGLISH.RTX")
                .to_string(),
            redbook_volume: ini_value(ini, "system", "redbook_volume")
                .and_then(|v| v.parse().ok())
                .unwrap_or(-1),
            master_volume: ini_value(ini, "system", "volume")
                .and_then(|v| v.parse().ok())
                .unwrap_or(256),
            sound_distance: ini_value(ini, "system", "sound_distance").and_then(|v| v.parse().ok()),
        }
    }
}

/// Effects of `MAIN.SFX` in file order. `None` marks an effect whose PCM
/// cannot be played; it keeps later indices aligned with the file.
#[derive(Resource, Default)]
pub struct SoundEffects(pub Vec<Option<Effect>>);

pub struct Effect {
    pub audio: Handle<AudioSource>,
    pub looping: bool,
    /// Header volume of non-positional starts, 0..=127.
    pub base_volume: u8,
}

/// Voice clips by RTX label. Empty until the background load finishes.
#[derive(Resource, Default)]
pub struct Speech(pub HashMap<String, VoiceClip<Handle<AudioSource>>>);

/// One voiced RTX entry: its audio, header base volume and subtitle text.
#[derive(Clone)]
pub struct VoiceClip<A> {
    pub audio: A,
    pub base_volume: u8,
    pub seconds: f64,
    pub text: String,
}

// shortcut: holds every voice clip's PCM in memory; read clips on demand once rgpre exposes the RTX index.
#[derive(Resource)]
struct SpeechLoad(Task<Result<HashMap<String, VoiceClip<AudioSource>>, String>>);

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

/// A playing effect or voice clip started through [`SoundRequest`].
#[derive(Component)]
pub struct PlayingSound;

pub fn sfx_path(install: &GameInstall) -> std::path::PathBuf {
    install.root.join("sound").join("MAIN.SFX")
}

pub fn read_settings(install: &GameInstall) -> SoundSettings {
    let path = install.root.join("SYSTEM.INI");
    SoundSettings::parse(&std::fs::read_to_string(&path).unwrap_or_else(|e| {
        warn!("sound: {}: {e}; using defaults", path.display());
        String::new()
    }))
}

/// `Assets<AudioSource>` is absent without `AudioPlugin` (headless); then nothing loads.
fn load_effects(
    mut commands: Commands,
    install: Res<GameInstall>,
    audio: Option<ResMut<Assets<AudioSource>>>,
) {
    let mut settings = read_settings(&install);
    match crate::startup::MenuPreferences::load() {
        Ok(Some(preferences)) => settings.apply_menu(&preferences),
        Ok(None) => {}
        Err(e) => warn!("sound: {e:#}; using SYSTEM.INI volumes"),
    }
    commands.init_resource::<SoundEffects>();
    commands.init_resource::<Speech>();
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
    let path = sfx_path(&install);
    let file = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|data| parse_sfx_file(&data).map_err(|e| e.to_string()))
    {
        Ok(file) => file,
        Err(e) => {
            error!("sound: {}: {e}", path.display());
            return;
        }
    };
    let effects = file
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            match pcm_wav(effect.audio_type, effect.sample_rate, &effect.pcm_data) {
                Ok(bytes) => Some(Effect {
                    audio: audio.add(AudioSource {
                        bytes: bytes.into(),
                    }),
                    looping: effect.loop_flag != 0,
                    base_volume: effect.base_volume,
                }),
                Err(e) => {
                    warn!("sound: {} effect {index}: {e}", path.display());
                    None
                }
            }
        })
        .collect();
    commands.insert_resource(SoundEffects(effects));
}

/// Voice clips of an RTX file keyed by label; clips whose PCM cannot be played are skipped.
pub fn load_speech(
    path: &std::path::Path,
) -> Result<HashMap<String, VoiceClip<AudioSource>>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = parse_rtx_file(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut clips = HashMap::new();
    for entry in &file.entries {
        let RtxEntry::Audio {
            header,
            pcm_data,
            label: text,
            ..
        } = entry
        else {
            continue;
        };
        let label = entry.tag_str();
        match pcm_wav(header.audio_type, header.sample_rate, pcm_data) {
            Ok(bytes) => {
                let source = AudioSource {
                    bytes: bytes.into(),
                };
                clips.insert(
                    label,
                    VoiceClip {
                        audio: source,
                        base_volume: header.base_volume,
                        seconds: header.duration_secs(),
                        text: text.clone(),
                    },
                );
            }
            Err(e) => warn!("sound: {} clip {label}: {e}", path.display()),
        }
    }
    Ok(clips)
}

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
                    let audio = audio.add(clip.audio);
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

#[allow(clippy::too_many_arguments)]
fn play_requests(
    mut commands: Commands,
    mut requests: MessageReader<SoundRequest>,
    effects: Res<SoundEffects>,
    speech: Res<Speech>,
    settings: Option<Res<SoundSettings>>,
    camera: Query<&GlobalTransform, With<crate::player::ViewCamera>>,
    playing: Query<Entity, With<PlayingSound>>,
    sources: Option<Res<Assets<AudioSource>>>,
    mut panned: Option<ResMut<Assets<Panned>>>,
) {
    let master = settings.as_ref().map_or(256, |s| s.master_volume);
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
                let playback = if effect.looping {
                    PlaybackSettings::LOOP
                } else {
                    PlaybackSettings::DESPAWN
                };
                let Some(position) = position else {
                    let gain = output_gain(effect.base_volume.min(127), master);
                    commands.spawn((
                        PlayingSound,
                        AudioPlayer(effect.audio.clone()),
                        playback.with_volume(bevy::audio::Volume::Linear(gain)),
                    ));
                    continue;
                };
                let (Some(distance), Ok(camera)) = (
                    settings.as_ref().and_then(|s| s.sound_distance),
                    camera.single(),
                ) else {
                    warn!("sound: effect {index}: no camera or SYSTEM.INI sound_distance");
                    continue;
                };
                let (Some(panned), Some(source)) = (
                    panned.as_mut(),
                    sources.as_ref().and_then(|s| s.get(&effect.audio)),
                ) else {
                    continue;
                };
                // Bevy space is engine space ÷ 20 with X mirrored; the original works in 1/256 world units.
                let raw = |bevy: f32, sign: f32| {
                    (f64::from(bevy * sign * crate::scene::ENGINE_UNIT_SCALE) * 256.0).round()
                        as i64
                };
                let offset = *position - camera.translation();
                let world_units = offset.length() * crate::scene::ENGINE_UNIT_SCALE;
                let volume = match distance_volume(distance, world_units, 0) {
                    Ok(volume) => volume,
                    Err(e) => {
                        warn!("sound: effect {index}: {e}");
                        continue;
                    }
                };
                let forward = camera.forward();
                let yaw = heading(raw(forward.x, -65536.0), raw(forward.z, 65536.0));
                let pan = pan(heading(raw(offset.x, -1.0), raw(offset.z, 1.0)), yaw);
                let gain = output_gain(volume, master);
                let [left, right] = pan_gains(pan);
                commands.spawn((
                    PlayingSound,
                    AudioPlayer(panned.add(Panned {
                        source: source.clone(),
                        gains: [left * gain, right * gain],
                    })),
                    playback,
                ));
            }
            SoundRequest::Speech(label) => {
                if !settings.as_ref().is_some_and(|s| s.speech) {
                    continue;
                }
                let Some(clip) = speech.0.get(label) else {
                    warn!("sound: voice clip {label} is not loaded");
                    continue;
                };
                commands.spawn((
                    PlayingSound,
                    AudioPlayer(clip.audio.clone()),
                    PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(
                        output_gain(clip.base_volume.min(127), master),
                    )),
                ));
            }
            SoundRequest::StopAll => {
                for entity in &playing {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
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
fn apply_menu(menu: Res<crate::startup::MenuState>, mut settings: ResMut<SoundSettings>) {
    if let Some(preferences) = &menu.preferences {
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
pub fn slider_volume(slider: i32) -> i32 {
    slider * 64 / 5
}

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
pub(crate) fn ini_value<'a>(ini: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut current = "";
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current = name.trim();
        } else if current.eq_ignore_ascii_case(section)
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim());
        }
    }
    None
}

/// Wraps PCM in a RIFF/WAVE header. SFX, RTX and Red Book PCM is already
/// WAV's layout: unsigned 8-bit or signed little-endian 16-bit, interleaved.
pub fn pcm_wav(audio_type: AudioType, sample_rate: u32, pcm: &[u8]) -> Result<Vec<u8>, String> {
    cyrus_assets::media::pcm_wav(
        pcm,
        sample_rate,
        audio_type.channels(),
        audio_type.bits_per_sample(),
    )
    .map_err(|e| e.to_string())
}
