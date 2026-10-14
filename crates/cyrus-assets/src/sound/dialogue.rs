//! Indexed dialogue payload access for native and browser game hosts.
pub mod export;

use crate::source::{AssetFile, ReadAssets};
use anyhow::{Context, Result};
use rgpre::import::rtx::{RtxEntry, RtxIndex};

/// Read-only RTX directory. Payloads are loaded only when a label is requested;
/// runtime cache replacement and sample ownership belong to the game host.
pub struct DialogueArchive<'a> {
    file: AssetFile<'a>,
    path: String,
    index: RtxIndex,
}

impl<'a> DialogueArchive<'a> {
    /// `path` is the installed SYSTEM.INI rtx_filename, resolved by ReadAssets.
    pub fn open(source: &'a dyn ReadAssets, path: &str) -> Result<Self> {
        let mut file = AssetFile::open(source, path)?.without_read_ahead();
        let index =
            RtxIndex::read(&mut file).with_context(|| format!("{path}: dialogue directory"))?;
        Ok(Self {
            file,
            path: path.into(),
            index,
        })
    }

    /// ASCII-fold the four-byte authored label for installed file lookup.
    /// Return both payload kinds without display formatting.
    /// Missing labels and malformed payloads return contextual errors.
    pub fn read(&mut self, label: [u8; 4]) -> Result<RtxEntry> {
        let label = label.map(|byte| byte.to_ascii_lowercase());
        let context = || {
            format!(
                "{}: dialogue label {:?}",
                self.path,
                String::from_utf8_lossy(&label)
            )
        };
        let index = self
            .index
            .entries
            .iter()
            .position(|entry| entry.tag == label)
            .with_context(context)?;
        self.index
            .read_entry(&mut self.file, index)
            .with_context(context)
    }
}

/// Original dialogue lifetime before display formatting. A remembered voice
/// uses signed integer duration in 1/256-second units; absent samples use the
/// original text byte count times fifteen. See preservation engine/sound.md.
pub fn line_lifetime(entry: &RtxEntry, sample: Option<u8>) -> Result<i32> {
    let text = match entry {
        RtxEntry::Text { text, .. } => text,
        RtxEntry::Audio { label, .. } => label,
    };
    if sample.is_none() {
        anyhow::ensure!(
            text.is_ascii() && !text.contains('\0'),
            "dialogue lifetime requires supported original ASCII text without embedded NUL"
        );
        return Ok(i32::try_from(text.len())
            .context("dialogue text byte count exceeds signed lifetime range")?
            .wrapping_mul(15));
    }
    let RtxEntry::Audio {
        header, pcm_data, ..
    } = entry
    else {
        anyhow::bail!("dialogue text entry has a remembered voice sample");
    };
    let length = i32::try_from(pcm_data.len())
        .context("dialogue PCM byte count exceeds signed lifetime range")?;
    let divisor = i32::try_from(header.sample_rate)
        .ok()
        .and_then(|rate| rate.checked_mul(i32::from(header.audio_type.channels())))
        .and_then(|rate| rate.checked_mul(i32::from(header.audio_type.bits_per_sample() / 8)))
        .filter(|value| *value > 0)
        .context("dialogue voice has unsupported sample rate or PCM layout")?;
    Ok(length.wrapping_shl(8) / divisor)
}

impl crate::sound::DialoguePayload for RtxEntry {
    fn has_voice(&self) -> bool {
        matches!(self, Self::Audio { .. })
    }
    fn release_text(&mut self) {
        match self {
            Self::Text { text, .. } => *text = String::new(),
            Self::Audio { label, .. } => *label = String::new(),
        }
    }
}
