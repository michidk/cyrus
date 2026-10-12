//! Incremental movie packets retain the existing Smacker codec and validate exported metadata.
use super::MovieMetadata;
use anyhow::{Context, ensure};

/// A decoded indexed frame; palette and indices remain valid until the next packet.
#[derive(Debug)]
pub struct MovieFrame<'a> {
    pub number: u32,
    pub indices: &'a [u8],
    pub palette: &'a [[u8; 3]; 256],
}

/// Sequential compressed movie input. Consumed packets are discarded, including during forward skips.
pub struct MovieStream {
    metadata: MovieMetadata,
    input: Vec<u8>,
    consumed: usize,
    decoder: Option<smk::Smk>,
    frame: u32,
}
impl MovieStream {
    /// Metadata must be the validated export for this movie.
    pub fn new(metadata: MovieMetadata) -> Self {
        Self {
            metadata,
            input: Vec::new(),
            consumed: 0,
            decoder: None,
            frame: 0,
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
        let name = &self.metadata.definition.name;
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
            let video = decoder.info_video();
            let info = decoder.info();
            ensure!(
                video.width == 640
                    && video.height == 240
                    && matches!(
                        video.y_scale,
                        smk::YScaleMode::Double | smk::YScaleMode::Interlace
                    ),
                "movie {name}: unsupported viewport"
            );
            ensure!(
                info.frame_count == self.metadata.frames
                    && (info.microseconds_per_frame - f64::from(self.metadata.frame_micros)).abs()
                        < 0.5,
                "movie {name}: cache/header mismatch; re-export startup"
            );
            decoder.enable_all(0x80);
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
        Ok(Some(MovieFrame {
            number: self.frame,
            indices: decoder.video_data(),
            palette: decoder.palette(),
        }))
    }
    /// Authored frames decoded, including frames discarded for a forward skip.
    pub fn decoded_frames(&self) -> u32 {
        self.frame
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    #[test]
    fn installed_movie_streams_before_full_download_and_matches_frames() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let install = match crate::install::GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => return eprintln!("skipped: no install ({error})"),
        };
        let movie = crate::startup::movie_native::prepare(&install.root, 0).unwrap();
        let mut data = Vec::new();
        movie.source.open().unwrap().read_to_end(&mut data).unwrap();
        let mut stream = MovieStream::new(movie.metadata.clone());
        let mut original = smk::Smk::open_memory(&data).unwrap();
        original.enable_all(0x80);
        let mut supplied = 0;
        let mut max_pending = 0;
        for number in 1..=movie.metadata.definition.keys[0] + 1 {
            loop {
                if let Some(frame) = stream.next_frame().unwrap() {
                    if number == 1 {
                        original.first_frame()
                    } else {
                        original.next_frame()
                    }
                    .unwrap();
                    assert_eq!(frame.number, number);
                    assert_eq!(frame.indices, original.video_data(), "frame {number}");
                    assert_eq!(frame.palette, original.palette(), "palette {number}");
                    break;
                }
                let end = (supplied + 997).min(data.len());
                assert!(end > supplied, "unexpected need for data at frame {number}");
                stream.push(&data[supplied..end]);
                supplied = end;
                max_pending = max_pending.max(stream.input.len());
            }
            if number == 1 {
                assert!(
                    supplied < data.len() / 100,
                    "first frame waited for the movie"
                );
            }
        }
        assert!(max_pending < 1024 * 1024, "consumed packets accumulated");
        assert_eq!(
            stream.decoded_frames(),
            movie.metadata.definition.keys[0] + 1
        );
        let mut wrong_metadata = movie.metadata;
        wrong_metadata.frames += 1;
        let mut wrong = MovieStream::new(wrong_metadata);
        let size = smk::Smk::header_size(&data).unwrap().unwrap();
        wrong.push(&data[..size]);
        assert!(
            wrong
                .next_frame()
                .unwrap_err()
                .to_string()
                .contains("cache/header mismatch")
        );
    }
}
