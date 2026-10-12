//! Portable Smacker decoding and PCM conversion; game presentation policy is separate.
use anyhow::{Result, ensure};

/// Wraps original unsigned 8-bit or signed little-endian 16-bit PCM in WAV.
pub fn pcm_wav(pcm: &[u8], rate: u32, channels: u16, bits: u16) -> Result<Vec<u8>> {
    ensure!(
        rate > 0 && matches!(channels, 1 | 2) && matches!(bits, 8 | 16),
        "unsupported PCM format"
    );
    let alignment = channels * (bits / 8);
    ensure!(
        pcm.len().is_multiple_of(alignment as usize),
        "incomplete PCM frame"
    );
    let len = u32::try_from(pcm.len())?;
    let riff_len = len
        .checked_add(36 + len % 2)
        .ok_or_else(|| anyhow::anyhow!("PCM too large"))?;
    let byte_rate = rate
        .checked_mul(alignment.into())
        .ok_or_else(|| anyhow::anyhow!("PCM rate overflow"))?;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend(riff_len.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(channels.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend(byte_rate.to_le_bytes());
    wav.extend(alignment.to_le_bytes());
    wav.extend(bits.to_le_bytes());
    wav.extend(b"data");
    wav.extend(len.to_le_bytes());
    wav.extend(pcm);
    if len % 2 != 0 {
        wav.push(0);
    }
    Ok(wav)
}

/// Min/max PCM envelopes preserve transients while bounding the waveform size.
pub fn peaks(pcm: &[u8], bits: u16, count: usize) -> Vec<[f32; 2]> {
    let bytes_per_sample = if bits == 8 { 1 } else { 2 };
    let samples_per_bucket = (pcm.len() / bytes_per_sample).div_ceil(count.max(1)).max(1);
    pcm.chunks(samples_per_bucket * bytes_per_sample)
        .map(|bucket| {
            bucket
                .chunks_exact(bytes_per_sample)
                .fold([1f32, -1f32], |[min, max], sample| {
                    let value = if bits == 8 {
                        (sample[0] as f32 - 128.) / 128.
                    } else {
                        i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.
                    };
                    [min.min(value), max.max(value)]
                })
        })
        .collect()
}

/// One decoder and one current picture; no decoded-frame history is retained.
pub struct Movie {
    decoder: smk::Smk,
    pub width: u32,
    pub height: u32,
    pub display_height: u32,
    pub frames: u32,
    pub seconds_per_frame: f64,
    pub frame: u32,
}
impl Movie {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut decoder = smk::Smk::open_memory(bytes)?;
        let video = decoder.info_video();
        let info = decoder.info();
        ensure!(
            video.width > 0
                && video.height > 0
                && u64::from(video.width) * u64::from(video.height) <= 16_777_216,
            "invalid movie dimensions"
        );
        ensure!(
            info.frame_count > 0
                && info.microseconds_per_frame.is_finite()
                && info.microseconds_per_frame > 0.,
            "invalid movie timing"
        );
        decoder.enable_video(true);
        decoder.first_frame()?;
        let display_height = video.height
            * if video.y_scale == smk::YScaleMode::None {
                1
            } else {
                2
            };
        Ok(Self {
            decoder,
            width: video.width,
            height: video.height,
            display_height,
            frames: info.frame_count,
            seconds_per_frame: info.microseconds_per_frame / 1_000_000.,
            frame: 0,
        })
    }
    pub fn seek(&mut self, target: u32) -> Result<()> {
        ensure!(target < self.frames, "movie frame outside range");
        self.decoder.seek_keyframe(target)?;
        self.frame = self.decoder.info().current_frame;
        Ok(())
    }
    pub fn advance(&mut self) -> Result<()> {
        if self.frame + 1 < self.frames {
            self.decoder.next_frame()?;
            self.frame += 1;
        }
        Ok(())
    }
    pub fn rgba(&self) -> Vec<u8> {
        self.decoder
            .video_data()
            .iter()
            .flat_map(|index| {
                let [r, g, b] = self.decoder.palette()[*index as usize];
                [r, g, b, 255]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_pcm_is_padded_without_changing_data_length() {
        let wav = pcm_wav(&[0, 128, 255], 11025, 1, 8).unwrap();
        assert_eq!(&wav[44..], &[0, 128, 255, 0]);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 3);
        assert_eq!(peaks(&[0, 128, 255], 8, 1), vec![[-1., 127. / 128.]]);
        assert!(pcm_wav(&[1], 44100, 2, 16).is_err());
    }
}

pub mod packets;
pub mod stream;
pub use packets::Packets;
/// Existing Smacker codec for metadata and decoded packet access.
pub use smk as codec;
pub use stream::{MovieAudio, MovieFormat, MovieFrame, MovieStream};

/// Decode selected audio tracks in packet order. Progress uses zero-based frames.
/// Compressed input stays bounded; returned PCM contains the selected complete tracks.
pub fn audio_tracks<R: std::io::Read>(
    reader: R,
    mask: u8,
    mut progress: impl FnMut(u32, u32),
) -> Result<(codec::AudioInfo, [Vec<u8>; 7])> {
    let mut packets = Packets::new(reader, mask & 0x7f)?;
    let info = packets.codec.info_audio();
    let frames = packets.codec.info().frame_count;
    let mut pcm: [Vec<u8>; 7] = std::array::from_fn(|_| Vec::new());
    if mask & info.track_mask == 0 {
        return Ok((info, pcm));
    }
    for frame in 0..frames {
        progress(frame, frames);
        ensure!(packets.advance()?, "missing movie packet {}", frame + 1);
        for (track, output) in pcm.iter_mut().enumerate() {
            if mask & info.track_mask & (1 << track) != 0 {
                output.extend_from_slice(packets.codec.audio_data(track as u8).unwrap_or_default());
            }
        }
    }
    Ok((info, pcm))
}
