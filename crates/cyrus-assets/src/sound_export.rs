//! MAIN.SFX PCM is preserved verbatim inside standard WAV containers.
use crate::sound::{SoundBank, SoundEffect};
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
    cyrus_media::pcm_wav(&effect.pcm_data, effect.sample_rate, channels, bits)
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(install: &crate::install::GameInstall, cache: &Path) -> anyhow::Result<()> {
    let source = crate::native::Files::scan(install, false)?;
    prepare(&source, &mut crate::preparation::DiskOutput(cache))
}

pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let bank = sfx::parse_sfx_file(&crate::preparation::read(source, "sound/MAIN.SFX")?)?;
    let out = Path::new("sounds");
    let mut effects = Vec::new();
    let mut html = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Redguard sound effects</title><body><h1>Sound effects</h1><p>Original effect IDs. Loops follow MAIN.SFX. Press play to audition.</p>",
    );
    for (i, effect) in bank.effects.iter().enumerate() {
        let bytes = wav(effect)?;
        output.publish(&out.join(format!("{i:03}.wav")), &bytes)?;
        effects.push(SoundEffect {
            looping: effect.loop_flag != 0,
            sample_rate: effect.sample_rate,
            channels: effect.audio_type.channels(),
            bits_per_sample: effect.audio_type.bits_per_sample(),
            base_volume: effect.base_volume,
        });
        html.push_str(&format!("<p>Effect {i} <audio aria-label=\"Effect {i}\" controls preload=\"none\" {} src=\"{i:03}.wav\"></audio></p>",if effect.loop_flag != 0 {"loop"} else {""}));
    }
    html.push_str("</body></html>");
    output.publish(&out.join("index.html"), html.as_bytes())?;
    let settings =
        crate::sound::SoundSettings::parse(&crate::preparation::text(source, "SYSTEM.INI")?);
    output.publish(
        &out.join("bank.sounds"),
        &serde_json::to_vec(&SoundBank {
            effects,
            music: Some(crate::music::settings_source(source)?),
            enabled: settings.effects,
            master_volume: settings.master_volume,
            sound_distance: settings.sound_distance,
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
