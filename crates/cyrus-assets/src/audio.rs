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
