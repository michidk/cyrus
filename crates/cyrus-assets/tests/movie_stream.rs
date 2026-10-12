#![cfg(feature = "native")]

mod tests {
    use cyrus_media::{MovieStream, codec as smk};
    #[test]
    fn installed_movie_streams_before_full_download_and_matches_frames() {
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
        let mut stream = MovieStream::new(movie.metadata.codec_format(), 0x81);
        let mut original = smk::Smk::open_memory(&data).unwrap();
        original.enable_all(0x81);
        let mut supplied = 0;
        let mut samples = 0;
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
                    if let Some(audio) = frame.audio {
                        assert_eq!(
                            audio.pcm,
                            original.audio_data(0).unwrap_or_default(),
                            "PCM frame {number}"
                        );
                        assert_eq!(audio.start_sample, samples);
                        samples +=
                            (audio.pcm.len() / usize::from(audio.channels * audio.bits / 8)) as u64;
                    }
                    break;
                }
                let end = (supplied + 997).min(data.len());
                assert!(end > supplied, "unexpected need for data at frame {number}");
                stream.push(&data[supplied..end]);
                supplied = end;
            }
            if number == 1 {
                assert!(
                    supplied < data.len() / 100,
                    "first frame waited for the movie"
                );
            }
        }
        assert_eq!(
            stream.decoded_frames(),
            movie.metadata.definition.keys[0] + 1
        );
        let mut wrong_metadata = movie.metadata;
        wrong_metadata.frames += 1;
        let mut wrong = MovieStream::new(wrong_metadata.codec_format(), 0x81);
        let size = smk::Smk::header_size(&data).unwrap().unwrap();
        wrong.push(&data[..size]);
        let error = wrong.next_frame().unwrap_err().to_string();
        assert!(error.contains("format/header mismatch"), "{error}");
    }
}
