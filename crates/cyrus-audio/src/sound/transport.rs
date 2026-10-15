//! Exact source-decoder continuation, separate from backend/device output buffers.
use super::{PannedGains, PcmScale, PcmWidth};
use anyhow::{Context, Result, ensure};
use bevy::{
    audio::{AudioSource, Decodable, PlaybackSettings, Source},
    prelude::{Asset, TypePath},
};
use serde::{Deserialize, Serialize};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

type RawDecoder = <AudioSource as Decodable>::Decoder;

/// Opaque source-decoder checkpoint. Contains no installed audio bytes.
/// Cursor units are interleaved source samples in the current loop, with pending
/// stereo output retained separately. This is not an output-device checkpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PannedSnapshot {
    version: u32,
    source_identity: String,
    width: PcmWidth,
    gains: PcmScale,
    frame_gains: PcmScale,
    input_samples: u64,
    right_bits: Option<u32>,
    channel: u16,
    looping: bool,
    speed: f32,
}
impl PannedSnapshot {
    /// BLAKE3 identity of the complete source bytes supplied to reconstruction.
    pub fn source_identity(&self) -> &str {
        &self.source_identity
    }
    /// Whether EOF returns to the beginning before applying current gains.
    pub fn looping(&self) -> bool {
        self.looping
    }
    /// Playback speed multiplier, applied by the host after decoding.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub(crate) fn is_unscaled_loop(&self) -> bool {
        let unity = PcmScale::new(PcmWidth::Sixteen, 127, 256, 64);
        self.width == PcmWidth::Sixteen
            && self.gains == unity
            && self.frame_gains == unity
            && self.looping
            && self.speed == 1.0
    }
}

/// Validated-source stereo gain transport. Looping runs before scaling, so later
/// loops use current gains. Backend resampling and device buffers remain external.
#[derive(Asset, TypePath)]
pub struct Panned {
    source: AudioSource,
    gains: PannedGains,
    looping: bool,
    speed: f32,
    initial: Option<PannedSnapshot>,
    capture: Mutex<Capture>,
}
#[derive(Default)]
struct Capture {
    state: Option<Arc<Mutex<State>>>,
    consumers: usize,
}
impl Clone for Panned {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            gains: self.gains.clone(),
            looping: self.looping,
            speed: self.speed,
            initial: self.initial.clone(),
            capture: Mutex::default(),
        }
    }
}
impl Panned {
    /// Validate and prepare an unstarted finite mono/stereo transport at normal speed.
    pub fn new(source: AudioSource, gains: PannedGains) -> Result<Self> {
        Self::with_playback(source, gains, false, 1.0)
    }
    /// Prepare a validated mono/stereo transport. Use playback_settings for the
    /// backend; another loop wrapper would cache the scaled output.
    pub fn with_playback(
        source: AudioSource,
        gains: PannedGains,
        looping: bool,
        speed: f32,
    ) -> Result<Self> {
        let result = Self {
            source,
            gains,
            looping,
            speed,
            initial: None,
            capture: Mutex::default(),
        };
        let state = result.build_state()?;
        *result
            .capture
            .lock()
            .expect("new audio transport capture lock") = Capture {
            state: Some(Arc::new(Mutex::new(state))),
            consumers: 0,
        };
        Ok(result)
    }
    /// Original immutable bytes; source selection belongs to the channel owner.
    pub fn source(&self) -> &AudioSource {
        &self.source
    }
    /// Live gain control shared with this transport's decoder.
    pub fn gains(&self) -> &PannedGains {
        &self.gains
    }
    /// The decoder owns looping; the backend removes the entity at finite EOF.
    pub fn playback_settings(&self) -> PlaybackSettings {
        PlaybackSettings::DESPAWN.with_speed(self.speed)
    }
    /// Capture at one source-decoder boundary without consuming samples.
    /// Multiple decoder consumers are ambiguous and reject capture. The caller
    /// must separately account for samples already queued downstream; this API
    /// cannot establish an exact backend/device checkpoint from a sink duration.
    pub fn snapshot(&self) -> Result<PannedSnapshot> {
        let mut capture = self
            .capture
            .lock()
            .map_err(|_| anyhow::anyhow!("audio transport capture lock poisoned"))?;
        ensure!(
            capture.consumers <= 1,
            "audio transport has multiple decoder consumers"
        );
        if capture.state.is_none() {
            capture.state = Some(Arc::new(Mutex::new(self.build_state()?)));
        }
        let state = capture
            .state
            .as_ref()
            .unwrap()
            .lock()
            .map_err(|_| anyhow::anyhow!("audio transport decoder lock poisoned"))?;
        Ok(state.snapshot())
    }
    /// Validate exact source identity and decoder continuation before returning
    /// an off-live transport. Does not start a sink, allocate channels or emit cues.
    pub fn from_snapshot(
        source: AudioSource,
        width: PcmWidth,
        snapshot: PannedSnapshot,
    ) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "audio transport snapshot version {} is unsupported",
            snapshot.version
        );
        ensure!(
            snapshot.width == width,
            "audio transport source PCM width differs"
        );
        ensure!(
            snapshot.speed.is_finite() && snapshot.speed > 0.0,
            "audio transport playback speed is invalid"
        );
        ensure!(
            snapshot.source_identity == blake3::hash(&source.bytes).to_hex().as_str(),
            "audio transport source bytes differ"
        );
        ensure!(
            snapshot.gains.valid(width) && snapshot.frame_gains.valid(width),
            "audio transport gain state is invalid"
        );
        let gains = PannedGains {
            width,
            scale: Arc::new(Mutex::new(snapshot.gains)),
        };
        let mut result = Self {
            source,
            gains,
            looping: snapshot.looping,
            speed: snapshot.speed,
            initial: None,
            capture: Mutex::default(),
        };
        result.initial = Some(snapshot);
        let state = result.build_state()?;
        result.capture = Mutex::new(Capture {
            state: Some(Arc::new(Mutex::new(state))),
            consumers: 0,
        });
        Ok(result)
    }
    fn build_state(&self) -> Result<State> {
        ensure!(
            self.speed.is_finite() && self.speed > 0.0,
            "audio transport playback speed is invalid"
        );
        let (channels, remaining) = validate_pcm_source(&self.source, self.gains.width)?;
        let inner = raw_decoder(&self.source)?;
        ensure!(
            !self.looping || remaining != 0,
            "audio transport cannot loop an empty source"
        );
        let mut state = State {
            inner,
            source: self.source.clone(),
            source_identity: blake3::hash(&self.source.bytes).to_hex().to_string(),
            gains: self.gains.clone(),
            frame_gains: self.gains.snapshot(),
            right: None,
            channel: 0,
            input_samples: 0,
            sample_count: remaining,
            looping: self.looping,
            speed: self.speed,
        };
        if let Some(saved) = &self.initial {
            ensure!(
                saved.input_samples <= remaining,
                "audio transport input sample cursor exceeds source length"
            );
            ensure!(
                saved.channel
                    == if channels == 1 {
                        0
                    } else {
                        (saved.input_samples % 2) as u16
                    },
                "audio transport channel cursor is invalid"
            );
            ensure!(
                channels == 1 || saved.right_bits.is_none(),
                "audio transport stereo source has pending mono output"
            );
            let mut last = None;
            for _ in 0..saved.input_samples {
                last = Some(
                    state
                        .inner
                        .next()
                        .context("audio transport source ended before saved cursor")?,
                );
            }
            if let Some(bits) = saved.right_bits {
                let sample =
                    last.context("audio transport pending output precedes source samples")?;
                ensure!(
                    saved.frame_gains.apply(sample, 1).to_bits() == bits,
                    "audio transport pending right sample differs"
                );
            }
            state.frame_gains = saved.frame_gains;
            state.right = saved.right_bits.map(f32::from_bits);
            state.channel = saved.channel;
            state.input_samples = saved.input_samples;
        }
        Ok(state)
    }
}
fn validate_pcm_source(source: &AudioSource, width: PcmWidth) -> Result<(u16, u64)> {
    let mut cursor = Cursor::new(&*source.bytes);
    let riff_bytes = hound::read_wave_header(&mut cursor).context("audio transport WAV header")?;
    cursor.set_position(0);
    let mut reader = hound::WavReader::new(cursor).context("audio transport PCM metadata")?;
    let spec = reader.spec();
    ensure!(
        spec.sample_rate > 0,
        "audio transport source sample rate is zero"
    );
    ensure!(
        matches!(spec.channels, 1 | 2),
        "audio transport source has unsupported channel count {}",
        spec.channels
    );
    let bits = match width {
        PcmWidth::Eight => 8,
        PcmWidth::Sixteen => 16,
    };
    ensure!(
        spec.sample_format == hound::SampleFormat::Int && spec.bits_per_sample == bits,
        "audio transport source PCM format differs from {bits}-bit integer samples"
    );
    let samples = u64::from(reader.len());
    // Validate the stored sample width without scanning the entire clip.
    let first = reader
        .samples::<i32>()
        .next()
        .transpose()
        .context("audio transport first PCM sample")?;
    let remaining = samples - u64::from(first.is_some());
    let data_end = reader.into_inner().position() + remaining * u64::from(bits / 8);
    ensure!(
        riff_bytes <= source.bytes.len() as u64 && data_end <= riff_bytes,
        "audio transport source PCM data is truncated"
    );
    Ok((spec.channels, samples))
}
fn raw_decoder(source: &AudioSource) -> Result<RawDecoder> {
    rodio::Decoder::builder()
        .with_byte_len(source.bytes.len() as u64)
        .with_data(Cursor::new(source.clone()))
        .build()
        .context("audio transport source decoding")
}
impl Decodable for Panned {
    type Decoder = PannedDecoder;
    fn decoder(&self) -> PannedDecoder {
        let mut capture = self
            .capture
            .lock()
            .expect("audio transport capture lock poisoned");
        let state = if capture.consumers == 0 {
            capture.state.take().unwrap_or_else(|| {
                Arc::new(Mutex::new(
                    self.build_state()
                        .expect("validated audio transport source"),
                ))
            })
        } else {
            Arc::new(Mutex::new(
                self.build_state()
                    .expect("validated audio transport source"),
            ))
        };
        capture.consumers = capture.consumers.saturating_add(1);
        capture.state = Some(Arc::clone(&state));
        PannedDecoder(state)
    }
}
struct State {
    inner: RawDecoder,
    source: AudioSource,
    source_identity: String,
    gains: PannedGains,
    frame_gains: PcmScale,
    right: Option<f32>,
    channel: u16,
    input_samples: u64,
    sample_count: u64,
    looping: bool,
    speed: f32,
}
impl State {
    fn snapshot(&self) -> PannedSnapshot {
        PannedSnapshot {
            version: 1,
            source_identity: self.source_identity.clone(),
            width: self.gains.width,
            gains: self.gains.snapshot(),
            frame_gains: self.frame_gains,
            input_samples: self.input_samples,
            right_bits: self.right.map(f32::to_bits),
            channel: self.channel,
            looping: self.looping,
            speed: self.speed,
        }
    }
    fn input(&mut self) -> Option<f32> {
        if self.input_samples == self.sample_count {
            if !self.looping {
                return None;
            }
            self.inner = raw_decoder(&self.source).expect("validated looping audio source");
            self.input_samples = 0;
            self.channel = 0;
        }
        let sample = self.inner.next()?;
        self.input_samples += 1;
        Some(sample)
    }
}
/// Source decoder whose progress is shared with its owning Panned asset.
/// Its sample cursor precedes backend conversion and physical device buffering.
pub struct PannedDecoder(Arc<Mutex<State>>);
impl Iterator for PannedDecoder {
    type Item = bevy::audio::Sample;
    fn next(&mut self) -> Option<Self::Item> {
        let mut state = self
            .0
            .lock()
            .expect("audio transport decoder lock poisoned");
        if state.inner.channels().get() == 1 {
            if let Some(right) = state.right.take() {
                return Some(right);
            }
            let sample = state.input()?;
            let gains = state.gains.snapshot();
            state.frame_gains = gains;
            state.right = Some(gains.apply(sample, 1));
            return Some(gains.apply(sample, 0));
        }
        let sample = state.input()?;
        if state.channel == 0 {
            state.frame_gains = state.gains.snapshot();
        }
        let channel = usize::from(state.channel);
        state.channel = 1 - state.channel;
        Some(state.frame_gains.apply(sample, channel))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let state = self
            .0
            .lock()
            .expect("audio transport decoder lock poisoned");
        if state.looping {
            return (0, None);
        }
        let lower = (state.sample_count - state.input_samples) as usize;
        let upper = Some(lower);
        if state.inner.channels().get() == 1 {
            let pending = usize::from(state.right.is_some());
            (
                lower.saturating_mul(2).saturating_add(pending),
                upper.and_then(|n| n.checked_mul(2)?.checked_add(pending)),
            )
        } else {
            (lower, upper)
        }
    }
}
impl Source for PannedDecoder {
    fn current_span_len(&self) -> Option<usize> {
        let state = self
            .0
            .lock()
            .expect("audio transport decoder lock poisoned");
        let channels = usize::from(state.inner.channels().get());
        state
            .inner
            .current_span_len()
            .map(|n| n * 2 / channels + usize::from(state.right.is_some()))
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        bevy::audio::ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        self.0
            .lock()
            .expect("audio transport decoder lock poisoned")
            .inner
            .sample_rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        let state = self
            .0
            .lock()
            .expect("audio transport decoder lock poisoned");
        if state.looping {
            None
        } else {
            state.inner.total_duration()
        }
    }
}
