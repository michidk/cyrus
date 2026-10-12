//! Native packet decoding and bounded audio playback without whole-movie PCM preparation.
use anyhow::{Context, ensure};
use bevy::{
    asset::Asset,
    audio::{ChannelCount, Decodable, SampleRate, Source},
    reflect::TypePath,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// Reads only the header and current packet from the installed movie.
pub(super) struct Packets {
    reader: Box<dyn std::io::Read + Send>,
    packet: Vec<u8>,
    pub codec: smk::Smk,
}
impl Packets {
    pub fn new(input: &cyrus_assets::movie::MovieInput, mask: u8) -> anyhow::Result<Self> {
        use std::io::Read;
        let mut reader = input.open()?;
        let mut header = vec![0; 104];
        reader.read_exact(&mut header)?;
        let size = smk::Smk::header_size(&header)?.context("incomplete movie header")?;
        ensure!(
            size <= 8 * 1024 * 1024,
            "movie header exceeds streaming capacity"
        );
        header.resize(size, 0);
        reader.read_exact(&mut header[104..])?;
        let mut codec = smk::Smk::open_stream(&header)?;
        let video = codec.info_video();
        ensure!(
            video.width == 640
                && video.height == 240
                && matches!(
                    video.y_scale,
                    smk::YScaleMode::Double | smk::YScaleMode::Interlace
                ),
            "unsupported movie viewport"
        );
        codec.enable_all(mask);
        Ok(Self {
            reader,
            packet: Vec::new(),
            codec,
        })
    }
    pub fn advance(&mut self) -> anyhow::Result<bool> {
        let Some(size) = self.codec.packet_size() else {
            return Ok(false);
        };
        ensure!(
            size <= 8 * 1024 * 1024,
            "movie packet exceeds streaming capacity"
        );
        self.packet.resize(size, 0);
        self.reader
            .read_exact(&mut self.packet)
            .context("truncated movie packet")?;
        self.codec.decode_packet(&self.packet)?;
        Ok(true)
    }
}

#[derive(Default)]
struct Progress {
    samples: AtomicU64,
    ended: AtomicBool,
    error: Mutex<Option<String>>,
}

/// One playback owns one queue. Dropping the playback and its asset cancels the producer.
#[derive(Asset, TypePath)]
pub(super) struct StreamingAudio {
    receiver: Arc<Mutex<mpsc::Receiver<Vec<f32>>>>,
    progress: Arc<Progress>,
    channels: ChannelCount,
    rate: SampleRate,
    start: Duration,
}
impl StreamingAudio {
    /// Decode only through the requested sample before returning; later blocks are bounded.
    pub fn prepare(
        input: &cyrus_assets::movie::MovieInput,
        start: Duration,
        name: String,
    ) -> anyhow::Result<Self> {
        let mut packets = Packets::new(input, 1)?;
        let info = packets.codec.info_audio();
        ensure!(
            info.track_mask & 1 != 0,
            "movie {name}: missing first audio track"
        );
        ensure!(
            matches!(info.channels[0], 1 | 2) && matches!(info.bitdepth[0], 8 | 16),
            "movie {name}: unsupported audio layout"
        );
        let channels =
            ChannelCount::new(u16::from(info.channels[0])).context("invalid movie channels")?;
        let rate = SampleRate::new(info.rate[0]).context("invalid movie audio rate")?;
        let mut skip = (start.as_secs_f64() * f64::from(rate.get())).floor() as usize
            * usize::from(channels.get());
        let bits = info.bitdepth[0];
        let mut next = move || -> anyhow::Result<Option<Vec<f32>>> {
            while packets.advance()? {
                let bytes = packets.codec.audio_data(0).unwrap_or_default();
                let width = usize::from(bits / 8);
                ensure!(
                    bytes
                        .len()
                        .is_multiple_of(width * usize::from(channels.get())),
                    "incomplete movie PCM frame"
                );
                let count = bytes.len() / width;
                let discarded = skip.min(count);
                skip -= discarded;
                let bytes = &bytes[discarded * width..];
                if !bytes.is_empty() {
                    return Ok(Some(if bits == 8 {
                        bytes
                            .iter()
                            .map(|v| (f32::from(*v) - 128.0) / 128.0)
                            .collect()
                    } else {
                        bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|v| f32::from(i16::from_le_bytes([v[0], v[1]])) / 32768.0)
                            .collect()
                    }));
                }
            }
            Ok(None)
        };
        let (sender, receiver) = mpsc::sync_channel(4);
        if let Some(first) = next().with_context(|| format!("movie {name}: audio"))? {
            sender.send(first)?;
        }
        let progress = Arc::new(Progress::default());
        let worker = progress.clone();
        std::thread::spawn(move || {
            loop {
                match next() {
                    Ok(Some(block)) => {
                        if sender.send(block).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        *worker.error.lock().unwrap() =
                            Some(format!("movie {name}: audio: {error:#}"));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            receiver: Arc::new(Mutex::new(receiver)),
            progress,
            channels,
            rate,
            start,
        })
    }
    pub fn elapsed(&self) -> Option<f64> {
        (!self.progress.ended.load(Ordering::Relaxed)).then(|| {
            self.start.as_secs_f64()
                + self.progress.samples.load(Ordering::Relaxed) as f64
                    / f64::from(self.channels.get())
                    / f64::from(self.rate.get())
        })
    }
    pub fn error(&self) -> Option<String> {
        self.progress.error.lock().unwrap().clone()
    }
}
impl Decodable for StreamingAudio {
    type Decoder = AudioDecoder;
    fn decoder(&self) -> AudioDecoder {
        AudioDecoder {
            receiver: self.receiver.clone(),
            progress: self.progress.clone(),
            channels: self.channels,
            rate: self.rate,
            block: Vec::new().into_iter(),
            silence: 0,
        }
    }
}
pub(super) struct AudioDecoder {
    receiver: Arc<Mutex<mpsc::Receiver<Vec<f32>>>>,
    progress: Arc<Progress>,
    channels: ChannelCount,
    rate: SampleRate,
    block: std::vec::IntoIter<f32>,
    silence: u16,
}
impl Iterator for AudioDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.silence > 0 {
            self.silence -= 1;
            return Some(0.0);
        }
        if self.block.len() == 0 {
            match self.receiver.lock().unwrap().try_recv() {
                Ok(block) => self.block = block.into_iter(),
                Err(mpsc::TryRecvError::Empty) => {
                    self.silence = self.channels.get() - 1;
                    return Some(0.0);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.progress.ended.store(true, Ordering::Relaxed);
                    return None;
                }
            }
        }
        let sample = self.block.next()?;
        self.progress.samples.fetch_add(1, Ordering::Relaxed);
        Some(sample)
    }
}
impl Source for AudioDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> SampleRate {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underrun_preserves_stereo_alignment_and_freezes_the_audio_clock() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let source = StreamingAudio {
            receiver: Arc::new(Mutex::new(receiver)),
            progress: Arc::new(Progress::default()),
            channels: ChannelCount::new(2).unwrap(),
            rate: SampleRate::new(10).unwrap(),
            start: Duration::from_secs(3),
        };
        let mut decoder = source.decoder();
        assert_eq!(decoder.next(), Some(0.0));
        sender.send(vec![0.25, -0.5]).unwrap();
        assert_eq!(decoder.next(), Some(0.0));
        assert_eq!(source.elapsed(), Some(3.0));
        assert_eq!(decoder.next(), Some(0.25));
        assert_eq!(decoder.next(), Some(-0.5));
        assert_eq!(source.elapsed(), Some(3.1));
        drop(sender);
        assert_eq!(decoder.next(), None);
        assert_eq!(source.elapsed(), None);
    }

    #[test]
    fn installed_streamed_audio_matches_full_decode_and_sample_exact_skip() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let install = match cyrus_assets::install::GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => return eprintln!("skipped: no install ({error})"),
        };
        let movie = cyrus_assets::startup::movie_native::prepare(&install.root, 0).unwrap();
        let mut data = Vec::new();
        movie.source.open().unwrap().read_to_end(&mut data).unwrap();
        let mut original = smk::Smk::open_memory(&data).unwrap();
        original.enable_all(1);
        let info = original.info_audio();
        let mut pcm = Vec::new();
        let mut status = original.first_frame().unwrap();
        loop {
            pcm.extend_from_slice(original.audio_data(0).unwrap_or_default());
            if status != smk::FrameStatus::More {
                break;
            }
            status = original.next_frame().unwrap();
        }
        let wav = cyrus_assets::media::pcm_wav(
            &pcm,
            info.rate[0],
            u16::from(info.channels[0]),
            u16::from(info.bitdepth[0]),
        )
        .unwrap();
        let expected: Vec<_> = bevy::audio::AudioSource { bytes: wav.into() }
            .decoder()
            .collect();
        assert!(!expected.is_empty());
        for start in [Duration::ZERO, Duration::from_millis(1379)] {
            let source = StreamingAudio::prepare(
                &movie.source,
                start,
                movie.metadata.definition.name.clone(),
            )
            .unwrap();
            let first = source.receiver.lock().unwrap().recv().unwrap();
            assert!(
                first.len() < expected.len() / 10,
                "startup decoded the whole movie"
            );
            let mut actual = first;
            while let Ok(block) = source.receiver.lock().unwrap().recv() {
                actual.extend(block);
            }
            assert_eq!(source.error(), None);
            let offset = (start.as_secs_f64() * f64::from(info.rate[0])).floor() as usize
                * usize::from(info.channels[0]);
            assert_eq!(actual, expected[offset..]);
        }
        // A packet truncated after a valid header must report a contextual error.
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cyrus-config/movie-tests");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("truncated-{}.smk", std::process::id()));
        std::fs::write(&path, &data[..data.len() / 2]).unwrap();
        let source = StreamingAudio::prepare(
            &cyrus_assets::movie::MovieInput::File(path.clone()),
            Duration::ZERO,
            "truncated test".into(),
        )
        .unwrap();
        while source.receiver.lock().unwrap().recv().is_ok() {}
        assert!(source.error().unwrap().contains("truncated movie packet"));
        std::fs::remove_file(path).unwrap();
    }
}
