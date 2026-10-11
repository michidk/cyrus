//! MAIN.SFX PCM is preserved verbatim inside standard WAV containers.
use crate::{
    install::GameInstall,
    sound_bank::{SoundBank, SoundEffect},
};
use rgpre::import::sfx::{self, SfxEffect};
use std::path::Path;

fn wav(effect: &SfxEffect) -> anyhow::Result<Vec<u8>> {
    let channels = effect.audio_type.channels();
    let bits = effect.audio_type.bits_per_sample();
    let alignment = channels * (bits / 8);
    anyhow::ensure!(
        effect.sample_rate > 0 && effect.pcm_data.len().is_multiple_of(alignment as usize),
        "invalid PCM layout"
    );
    anyhow::ensure!(
        effect.loop_offset == 0 && effect.loop_end == u32::MAX,
        "unsupported partial sound loop"
    );
    let length = u32::try_from(effect.pcm_data.len())?;
    let padding = length % 2;
    let riff_length = length
        .checked_add(36 + padding)
        .ok_or_else(|| anyhow::anyhow!("sound too large"))?;
    let byte_rate = effect
        .sample_rate
        .checked_mul(alignment.into())
        .ok_or_else(|| anyhow::anyhow!("sound rate overflow"))?;
    let mut bytes = Vec::with_capacity(riff_length as usize + 8);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&riff_length.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&effect.sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&alignment.to_le_bytes());
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&effect.pcm_data);
    if padding != 0 {
        bytes.push(0);
    }
    Ok(bytes)
}

pub fn export(install: &GameInstall, cache: &Path) -> anyhow::Result<()> {
    let directory = std::fs::read_dir(&install.root)?
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("sound")
        })
        .ok_or_else(|| anyhow::anyhow!("missing sound directory"))?
        .path();
    let path = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("MAIN.SFX")
        })
        .ok_or_else(|| anyhow::anyhow!("missing MAIN.SFX"))?
        .path();
    let source = sfx::parse_sfx_file(&std::fs::read(path)?)?;
    let out = cache.join("sounds");
    std::fs::create_dir_all(&out)?;
    let mut effects = Vec::new();
    let mut html = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Redguard sound effects</title><body><h1>Sound effects</h1><p>Original effect IDs. Loops follow MAIN.SFX. Press play to audition.</p>",
    );
    for (i, effect) in source.effects.iter().enumerate() {
        let bytes = wav(effect)?;
        let path = out.join(format!("{i:03}.wav"));
        let temporary = path.with_extension("wav.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, path)?;
        effects.push(SoundEffect {
            looping: effect.loop_flag != 0,
            sample_rate: effect.sample_rate,
            channels: effect.audio_type.channels(),
            bits_per_sample: effect.audio_type.bits_per_sample(),
        });
        html.push_str(&format!("<p>Effect {i} <audio aria-label=\"Effect {i}\" controls preload=\"none\" {} src=\"{i:03}.wav\"></audio></p>",if effect.loop_flag != 0 {"loop"} else {""}));
    }
    html.push_str("</body></html>");
    std::fs::write(out.join("index.html"), html)?;
    std::fs::write(
        out.join("bank.sounds"),
        serde_json::to_vec(&SoundBank {
            effects,
            audio: Vec::new(),
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgpre::import::sfx::AudioType;
    #[test]
    fn wav_preserves_unsigned_pcm_and_pads_odd_data() {
        let effect = SfxEffect {
            audio_type: AudioType::Mono8,
            base_volume: 127,
            sample_rate: 11025,
            loop_flag: 0,
            loop_offset: 0,
            loop_end: u32::MAX,
            pcm_data: vec![0, 128, 255],
        };
        let bytes = wav(&effect).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 40);
        assert_eq!(&bytes[44..], &[0, 128, 255, 0]);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 3);
    }
}
