//! Sound effects from the install's `sound/MAIN.SFX`, addressed by their
//! 0-based index in the file, and voice clips from the `SYSTEM.INI`
//! `rtx_filename` container, addressed by their 4-character RTX label. Each
//! plays at its stored sample rate; effects loop when their loop flag is
//! non-zero (preservation `docs/formats/SFX.md`, `docs/formats/RTX.md`).
//!
//! Unsupported until evidence is published: script, animation, dialogue and
//! ambient triggers, subtitles, positional attenuation (`SYSTEM.INI`
//! `sound_distance`), per-call volume and surface remapping. Everything plays
//! at unity gain.

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
        app.add_message::<SoundRequest>()
            .add_systems(Startup, load_effects)
            .add_systems(Update, (finish_speech, play_requests).chain());
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
    /// `[system] rtx_filename`, relative to the install root.
    pub rtx_filename: String,
}

impl SoundSettings {
    pub fn parse(ini: &str) -> Self {
        let on = |section, key| {
            !ini_value(ini, section, key).is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
        };
        Self {
            effects: on("system", "sound"),
            music: on("system", "redbook"),
            speech: on("dialog", "dialog_use_speech"),
            rtx_filename: ini_value(ini, "system", "rtx_filename")
                .unwrap_or("ENGLISH.RTX")
                .to_string(),
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
}

/// Voice clips by RTX label. Empty until the background load finishes.
#[derive(Resource, Default)]
pub struct Speech(pub HashMap<String, Handle<AudioSource>>);

// shortcut: holds every voice clip's PCM in memory; read clips on demand once rgpre exposes the RTX index.
#[derive(Resource)]
struct SpeechLoad(Task<Result<HashMap<String, AudioSource>, String>>);

#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub enum SoundRequest {
    Play(usize),
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
    let settings = read_settings(&install);
    commands.init_resource::<SoundEffects>();
    commands.init_resource::<Speech>();
    commands.insert_resource(settings.clone());
    let Some(mut audio) = audio else {
        info!("sound: no audio plugin, nothing loaded");
        return;
    };
    if settings.speech {
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
pub fn load_speech(path: &std::path::Path) -> Result<HashMap<String, AudioSource>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = parse_rtx_file(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut clips = HashMap::new();
    for entry in &file.entries {
        let RtxEntry::Audio {
            header, pcm_data, ..
        } = entry
        else {
            continue;
        };
        let label = entry.tag_str();
        match pcm_wav(header.audio_type, header.sample_rate, pcm_data) {
            Ok(bytes) => {
                clips.insert(
                    label,
                    AudioSource {
                        bytes: bytes.into(),
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
                .map(|(label, clip)| (label, audio.add(clip)))
                .collect();
        }
        Err(e) => error!("sound: {e}"),
    }
}

fn play_requests(
    mut commands: Commands,
    mut requests: MessageReader<SoundRequest>,
    effects: Res<SoundEffects>,
    speech: Res<Speech>,
    playing: Query<Entity, With<PlayingSound>>,
) {
    for request in requests.read() {
        match request {
            SoundRequest::Play(index) => {
                let Some(effect) = effects.0.get(*index).and_then(Option::as_ref) else {
                    warn!("sound: effect {index} is not loaded");
                    continue;
                };
                let settings = if effect.looping {
                    PlaybackSettings::LOOP
                } else {
                    PlaybackSettings::DESPAWN
                };
                commands.spawn((PlayingSound, AudioPlayer(effect.audio.clone()), settings));
            }
            SoundRequest::Speech(label) => {
                let Some(clip) = speech.0.get(label) else {
                    warn!("sound: voice clip {label} is not loaded");
                    continue;
                };
                commands.spawn((
                    PlayingSound,
                    AudioPlayer(clip.clone()),
                    PlaybackSettings::DESPAWN,
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

/// Last `key=value` of `[section]` in INI text; section and key match case-insensitively.
pub(crate) fn ini_value<'a>(ini: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut current = "";
    let mut found = None;
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current = name.trim();
        } else if current.eq_ignore_ascii_case(section)
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            found = Some(v.trim());
        }
    }
    found
}

/// Wraps PCM in a RIFF/WAVE header. SFX, RTX and Red Book PCM is already
/// WAV's layout: unsigned 8-bit or signed little-endian 16-bit, interleaved.
pub fn pcm_wav(audio_type: AudioType, sample_rate: u32, pcm: &[u8]) -> Result<Vec<u8>, String> {
    let channels = audio_type.channels();
    let bits = audio_type.bits_per_sample();
    let block_align = channels * bits / 8;
    let len = pcm.len();
    if sample_rate == 0 {
        return Err("sample rate 0".into());
    }
    if !len.is_multiple_of(usize::from(block_align)) {
        return Err(format!(
            "{len} PCM bytes are not whole {block_align}-byte frames"
        ));
    }
    let data_len = u32::try_from(len).map_err(|_| format!("{len} PCM bytes exceed WAV size"))?;
    let mut wav = Vec::with_capacity(44 + len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // integer PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    Ok(wav)
}
