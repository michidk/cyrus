//! MAIN.SFX PCM is preserved verbatim inside standard WAV containers.
use crate::{
    install::GameInstall,
    sound_bank::{SoundBank, SoundEffect},
};
use rgpre::import::sfx::{self, SfxEffect};
use std::path::Path;

fn wav(effect: &SfxEffect) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        effect.loop_offset == 0 && effect.loop_end == u32::MAX,
        "unsupported partial sound loop"
    );
    cyrus_assets::media::pcm_wav(
        &effect.pcm_data,
        effect.sample_rate,
        effect.audio_type.channels(),
        effect.audio_type.bits_per_sample(),
    )
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
