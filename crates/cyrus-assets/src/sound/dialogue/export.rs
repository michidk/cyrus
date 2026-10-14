//! Per-label original RTX preparation for disk caches and browser workers.
use super::{DialogueArchive, line_lifetime};
use crate::{
    preparation::Output,
    sound::{DialogueEntry, DialogueVoice},
    source::ReadAssets,
};
use anyhow::{Context, Result, ensure};
use rgpre::import::rtx::RtxEntry;
use std::path::Path;

pub(crate) fn prepare(source: &dyn ReadAssets, path: &str, output: &mut dyn Output) -> Result<()> {
    let name = path
        .strip_prefix("dialogue/")
        .context("dialogue cache directory missing")?;
    let (key, extension) = name
        .split_once('.')
        .context("dialogue cache extension missing")?;
    ensure!(
        matches!(extension, "dialogue" | "wav")
            && key.len() == 8
            && key
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "invalid dialogue cache path {path}"
    );
    let mut label = [0; 4];
    for (index, byte) in label.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&key[index * 2..index * 2 + 2], 16)?;
    }
    let settings =
        crate::sound::SoundSettings::parse(&crate::preparation::text(source, "SYSTEM.INI")?);
    let mut archive = DialogueArchive::open(source, &settings.rtx_filename)?;
    let entry = archive.read(label)?;
    let text_lifetime = line_lifetime(&entry, None)?;
    let lifetime = match &entry {
        RtxEntry::Audio { .. } => Some(line_lifetime(&entry, Some(0))?),
        _ => None,
    };
    let (text, voice) = match entry {
        RtxEntry::Text { text, .. } => (text, None),
        RtxEntry::Audio {
            label: text,
            header,
            pcm_data,
            ..
        } => {
            let audio = format!("dialogue/{key}.wav");
            let channels = header.audio_type.channels();
            let bits_per_sample = header.audio_type.bits_per_sample();
            let wav =
                cyrus_media::pcm_wav(&pcm_data, header.sample_rate, channels, bits_per_sample)
                    .with_context(|| format!("{}: dialogue {key} PCM", settings.rtx_filename))?;
            output.publish(Path::new(&audio), &wav)?;
            (
                text,
                Some(DialogueVoice {
                    audio,
                    base_volume: header.base_volume,
                    sample_rate: header.sample_rate,
                    channels,
                    bits_per_sample,
                    loop_flag: header.loop_flag,
                    loop_offset: header.loop_offset,
                    loop_end: header.loop_end,
                    lifetime: lifetime.context("voiced dialogue lifetime unavailable")?,
                }),
            )
        }
    };
    let metadata = DialogueEntry {
        version: 1,
        label,
        text,
        text_lifetime,
        voice,
    };
    output.publish(
        Path::new(&format!("dialogue/{key}.dialogue")),
        &serde_json::to_vec(&metadata)?,
    )
}

/// Prepare one entry and its optional WAV in a native cache, without writing the install.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(install: &crate::install::GameInstall, cache: &Path, label: [u8; 4]) -> Result<()> {
    let source = crate::native::Files::scan(install, false)?;
    let key = label
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    prepare(
        &source,
        &format!("dialogue/{key}.dialogue"),
        &mut crate::preparation::DiskOutput(cache),
    )
}
