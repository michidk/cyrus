//! Native installed audio readers and PCM conversion.
use crate::{install::GameInstall, sound::SoundSettings};
use log::warn;
use rgpre::import::sfx::AudioType;
#[cfg(not(target_arch = "wasm32"))]
pub fn sfx_path(install: &GameInstall) -> std::path::PathBuf {
    install.root.join("sound").join("MAIN.SFX")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_settings(install: &GameInstall) -> SoundSettings {
    let path = install.root.join("SYSTEM.INI");
    SoundSettings::parse(&std::fs::read_to_string(&path).unwrap_or_else(|e| {
        warn!("sound: {}: {e}; using defaults", path.display());
        String::new()
    }))
}

/// Original SFX, RTX and Red Book PCM in a WAV container; see `cyrus_media::pcm_wav`.
pub fn pcm_wav(audio_type: AudioType, sample_rate: u32, pcm: &[u8]) -> Result<Vec<u8>, String> {
    cyrus_media::pcm_wav(
        pcm,
        sample_rate,
        audio_type.channels(),
        audio_type.bits_per_sample(),
    )
    .map_err(|e| e.to_string())
}

use crate::sound::VoiceClip;
use rgpre::import::rtx::{RtxEntry, parse_rtx_file};
use std::collections::HashMap;
/// Voice clips of an RTX file keyed by label; clips whose PCM cannot be played are skipped.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_speech(path: &std::path::Path) -> Result<HashMap<String, VoiceClip<Vec<u8>>>, String> {
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
                let source = bytes;
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

/// Decoded PCM and installed playback metadata, without a renderer handle.
pub struct PreparedEffect {
    pub bytes: Vec<u8>,
    pub looping: bool,
    pub base_volume: u8,
    pub sample_rate: u32,
}
pub fn load_effect_bank(install: &GameInstall) -> Result<Vec<Option<PreparedEffect>>, String> {
    let path = sfx_path(install);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = rgpre::import::sfx::parse_sfx_file(&bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            match pcm_wav(effect.audio_type, effect.sample_rate, &effect.pcm_data) {
                Ok(bytes) => Some(PreparedEffect {
                    bytes,
                    looping: effect.loop_flag != 0,
                    base_volume: effect.base_volume,
                    sample_rate: effect.sample_rate,
                }),
                Err(error) => {
                    warn!("sound: {} effect {index}: {error}", path.display());
                    None
                }
            }
        })
        .collect())
}
