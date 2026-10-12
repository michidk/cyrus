//! Incremental movie packets retain the existing Smacker codec and validate exported metadata.
/// Neutral expected codec dimensions and timing; presentation supplies authored values.
#[derive(Clone, Copy)]
pub struct MovieFormat {
    pub frames: u32,
    pub frame_micros: u32,
    pub width: u32,
    pub height: u32,
    pub doubled_height: bool,
}
impl MovieFormat {
    /// Reject stale metadata and an unexpected codec viewport.
    pub fn validate(self, decoder: &smk::Smk) -> anyhow::Result<()> {
        let video = decoder.info_video();
        let info = decoder.info();
        anyhow::ensure!(
            video.width == self.width
                && video.height == self.height
                && (video.y_scale != smk::YScaleMode::None) == self.doubled_height,
            "unsupported movie viewport"
        );
        anyhow::ensure!(
            info.frame_count == self.frames
                && (info.microseconds_per_frame - f64::from(self.frame_micros)).abs() < 0.5,
            "movie format/header mismatch"
        );
        Ok(())
    }
}
use anyhow::{Context, ensure};

/// Borrowed first-track PCM; start_sample counts interleaved sample frames from zero.
#[derive(Debug)]
pub struct MovieAudio<'a> {
    pub pcm: &'a [u8],
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub start_sample: u64,
}

/// A decoded indexed frame; palette, indices and PCM remain valid until the next packet.
#[derive(Debug)]
pub struct MovieFrame<'a> {
    pub number: u32,
    pub audio: Option<MovieAudio<'a>>,
    pub indices: &'a [u8],
    pub palette: &'a [[u8; 3]; 256],
}

/// Sequential compressed movie input. Consumed packets are discarded, including during forward skips.
pub struct MovieStream {
    metadata: MovieFormat,
    input: Vec<u8>,
    consumed: usize,
    decoder: Option<smk::Smk>,
    frame: u32,
    mask: u8,
    audio_samples: u64,
}
impl MovieStream {
    /// Supply expected codec values and the audio/video enable mask (as in `Packets`).
    pub fn new(metadata: MovieFormat, mask: u8) -> Self {
        Self {
            metadata,
            input: Vec::new(),
            consumed: 0,
            decoder: None,
            frame: 0,
            mask,
            audio_samples: 0,
        }
    }
    /// Append a downloaded chunk after `next_frame` requests more input.
    pub fn push(&mut self, bytes: &[u8]) {
        self.input.drain(..self.consumed);
        self.consumed = 0;
        self.input.extend_from_slice(bytes);
    }
    /// Returns `None` until the next complete header or packet arrives, or at the authored end.
    pub fn next_frame(&mut self) -> anyhow::Result<Option<MovieFrame<'_>>> {
        if self.frame == self.metadata.frames {
            return Ok(None);
        }
        let name = "stream";
        if self.decoder.is_none() {
            let Some(size) = smk::Smk::header_size(&self.input)
                .with_context(|| format!("movie {name}: header"))?
            else {
                return Ok(None);
            };
            ensure!(
                size <= 8 * 1024 * 1024,
                "movie {name}: header exceeds streaming buffer capacity"
            );
            if self.input.len() < size {
                return Ok(None);
            }
            let mut decoder = smk::Smk::open_stream(&self.input[..size])
                .with_context(|| format!("movie {name}: decoding tables"))?;
            self.metadata.validate(&decoder)?;
            ensure!(
                self.mask & 1 == 0 || decoder.info_audio().track_mask & 1 != 0,
                "movie: missing first audio track"
            );
            decoder.enable_all(self.mask);
            self.decoder = Some(decoder);
            self.consumed = size;
        }
        let decoder = self.decoder.as_mut().unwrap();
        let size = decoder
            .packet_size()
            .with_context(|| format!("movie {name}: missing frame {}", self.frame + 1))?;
        ensure!(
            size <= 8 * 1024 * 1024,
            "movie {name}: frame {} exceeds streaming buffer capacity",
            self.frame + 1
        );
        if self.input.len() - self.consumed < size {
            return Ok(None);
        }
        decoder
            .decode_packet(&self.input[self.consumed..self.consumed + size])
            .with_context(|| format!("movie {name}: frame {}", self.frame + 1))?;
        self.consumed += size;
        self.frame += 1;
        let info = decoder.info_audio();
        let audio = if self.mask & info.track_mask & 1 != 0 {
            let channels = u16::from(info.channels[0]);
            let bits = u16::from(info.bitdepth[0]);
            ensure!(
                matches!(channels, 1 | 2) && matches!(bits, 8 | 16) && info.rate[0] > 0,
                "unsupported movie audio layout"
            );
            let pcm = decoder.audio_data(0).unwrap_or_default();
            let alignment = usize::from(channels * (bits / 8));
            ensure!(
                pcm.len().is_multiple_of(alignment),
                "incomplete movie PCM frame"
            );
            let start_sample = self.audio_samples;
            self.audio_samples += (pcm.len() / alignment) as u64;
            Some(MovieAudio {
                pcm,
                rate: info.rate[0],
                channels,
                bits,
                start_sample,
            })
        } else {
            None
        };
        Ok(Some(MovieFrame {
            number: self.frame,
            audio,
            indices: decoder.video_data(),
            palette: decoder.palette(),
        }))
    }
    /// Authored frames decoded, including frames discarded for a forward skip.
    pub fn decoded_frames(&self) -> u32 {
        self.frame
    }
}
