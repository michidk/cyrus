use bevy::audio::{AudioSource, Decodable};
use cyrus_assets::audio::pcm_wav;
use cyrus_audio::sound::{Panned, PannedGains, PannedSnapshot, PcmWidth};
use rgpre::import::sfx::AudioType;

fn source(kind: AudioType) -> AudioSource {
    let samples = if kind.bits_per_sample() == 8 {
        vec![192, 224, 64, 160, 200, 32, 240, 128]
    } else {
        [16384i16, 24576, -8192, 4096, 257, -257, 8192, -16384]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect()
    };
    AudioSource {
        bytes: pcm_wav(kind, 22050, &samples).unwrap().into(),
    }
}

#[test]
fn source_transport_replacement_keeps_exact_future_samples_and_gain_boundaries() {
    for kind in [
        AudioType::Mono8,
        AudioType::Stereo8,
        AudioType::Mono16,
        AudioType::Stereo16,
    ] {
        for looping in [false, true] {
            for consumed in [0, 1, 2, 7, 8, 16, 19] {
                let width: PcmWidth = kind.bits_per_sample().try_into().unwrap();
                let gains = PannedGains::new(width, 112, 256, 32);
                let bytes = source(kind);
                let live =
                    Panned::with_playback(bytes.clone(), gains.clone(), looping, 0.75).unwrap();
                let mut decoder = live.decoder();
                for _ in 0..consumed {
                    decoder.next();
                }
                gains.set(64, 255, 127);
                let before = serde_json::to_value(live.snapshot().unwrap()).unwrap();
                let saved: PannedSnapshot = serde_json::from_value(before.clone()).unwrap();
                assert_eq!(saved.looping(), looping);
                assert_eq!(saved.speed(), 0.75);
                assert!(!saved.source_identity().is_empty());
                let replacement = Panned::from_snapshot(bytes, width, saved).unwrap();
                assert_eq!(
                    serde_json::to_value(replacement.snapshot().unwrap()).unwrap(),
                    before
                );
                let mut restored = replacement.decoder();
                assert_eq!(restored.size_hint(), decoder.size_hint());
                for _ in 0..3 {
                    assert_eq!(
                        restored.by_ref().take(3).collect::<Vec<_>>(),
                        decoder.by_ref().take(3).collect::<Vec<_>>(),
                        "{kind:?} looping={looping} consumed={consumed}"
                    );
                    gains.set(127, 256, 0);
                    replacement.gains().set(127, 256, 0);
                }
                assert_eq!(
                    serde_json::to_value(replacement.snapshot().unwrap()).unwrap(),
                    serde_json::to_value(live.snapshot().unwrap()).unwrap()
                );
            }
        }
    }
}

#[test]
fn transport_rejects_invalid_source_cursor_gains_and_ambiguous_capture() {
    let width = PcmWidth::Sixteen;
    let bytes = source(AudioType::Mono16);
    let live = Panned::new(bytes.clone(), PannedGains::new(width, 127, 256, 64)).unwrap();
    let mut decoder = live.decoder();
    assert_eq!(decoder.next(), Some(0.5));
    let saved = serde_json::to_value(live.snapshot().unwrap()).unwrap();
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("source_identity", serde_json::json!("different")),
        ("speed", serde_json::json!(0.0)),
        ("input_samples", serde_json::json!(999)),
        ("channel", serde_json::json!(1)),
        ("right_bits", serde_json::json!(f32::NAN.to_bits())),
        ("gains", serde_json::json!({"Sixteen":[999, 128]})),
        ("frame_gains", serde_json::json!({"Sixteen":[128, -1]})),
    ] {
        let mut invalid = saved.clone();
        invalid[field] = value;
        assert!(
            Panned::from_snapshot(
                bytes.clone(),
                width,
                serde_json::from_value(invalid).unwrap()
            )
            .is_err(),
            "accepted {field}"
        );
        assert_eq!(
            serde_json::to_value(live.snapshot().unwrap()).unwrap(),
            saved
        );
    }
    let saved: PannedSnapshot = serde_json::from_value(saved).unwrap();
    assert!(Panned::from_snapshot(source(AudioType::Mono8), width, saved.clone()).is_err());
    assert!(Panned::from_snapshot(bytes, PcmWidth::Eight, saved.clone()).is_err());
    assert!(
        Panned::from_snapshot(
            AudioSource {
                bytes: vec![0; 44].into()
            },
            width,
            saved
        )
        .is_err()
    );
    let _second = live.decoder();
    assert!(
        live.snapshot()
            .unwrap_err()
            .to_string()
            .contains("multiple decoder")
    );
}

#[test]
fn unstarted_snapshot_and_replacement_never_consume_the_source() {
    let bytes = source(AudioType::Stereo16);
    let live = Panned::new(
        bytes.clone(),
        PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
    )
    .unwrap();
    let saved = live.snapshot().unwrap();
    let replacement = Panned::from_snapshot(bytes, PcmWidth::Sixteen, saved).unwrap();
    assert_eq!(live.decoder().next(), Some(0.5));
    assert_eq!(replacement.decoder().next(), Some(0.5));
}

#[test]
fn malformed_empty_loop_and_invalid_rates_reject_before_playback() {
    let gains = PannedGains::new(PcmWidth::Sixteen, 127, 256, 64);
    assert!(
        Panned::new(
            AudioSource {
                bytes: vec![0; 44].into()
            },
            gains.clone()
        )
        .is_err()
    );
    for speed in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            Panned::with_playback(source(AudioType::Mono16), gains.clone(), false, speed).is_err()
        );
    }
    let empty = AudioSource {
        bytes: pcm_wav(AudioType::Mono16, 22050, &[]).unwrap().into(),
    };
    assert!(Panned::with_playback(empty.clone(), gains.clone(), true, 1.0).is_err());
    let finite = Panned::new(empty, gains).unwrap();
    let mut decoder = finite.decoder();
    assert_eq!(decoder.next(), None);
    assert_eq!(decoder.next(), None);
    assert_eq!(decoder.size_hint(), (0, Some(0)));
    assert!(finite.snapshot().is_ok());
}

#[test]
fn malformed_pcm_metadata_and_truncated_data_reject_before_decoder_construction() {
    let valid = source(AudioType::Stereo16).bytes.to_vec();
    let mut zero_rate = valid.clone();
    zero_rate[24..28].copy_from_slice(&0u32.to_le_bytes());
    zero_rate[28..32].copy_from_slice(&0u32.to_le_bytes());
    let mut truncated = valid;
    truncated.pop();
    for (name, bytes) in [
        ("zero sample rate", zero_rate),
        ("truncated PCM data", truncated),
        (
            "mismatched original PCM width",
            source(AudioType::Mono8).bytes.to_vec(),
        ),
    ] {
        for looping in [false, true] {
            let outcome = std::panic::catch_unwind(|| {
                Panned::with_playback(
                    AudioSource {
                        bytes: bytes.clone().into(),
                    },
                    PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
                    looping,
                    1.0,
                )
            });
            assert!(
                matches!(outcome, Ok(Err(_))),
                "{name}, looping={looping}: expected a contextual error without a panic"
            );
            let valid = Panned::with_playback(
                source(AudioType::Stereo16),
                PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
                looping,
                1.0,
            )
            .unwrap();
            let mut saved = serde_json::to_value(valid.snapshot().unwrap()).unwrap();
            saved["source_identity"] = serde_json::json!(blake3::hash(&bytes).to_hex().to_string());
            let outcome = std::panic::catch_unwind(|| {
                Panned::from_snapshot(
                    AudioSource {
                        bytes: bytes.clone().into(),
                    },
                    PcmWidth::Sixteen,
                    serde_json::from_value(saved).unwrap(),
                )
            });
            assert!(
                matches!(outcome, Ok(Err(_))),
                "restored {name}, looping={looping}: expected a contextual error without a panic"
            );
        }
    }
}
