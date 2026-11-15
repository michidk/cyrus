use bevy::audio::{ChannelCount, SampleRate};
use rodio::conversions::{ResamplingMethod, SampleRateConverter};

fn convert(input: Vec<f32>, channels: u16, from: u32, to: u32) -> Vec<f32> {
    SampleRateConverter::new_with_method(
        input.into_iter(),
        SampleRate::new(from).unwrap(),
        SampleRate::new(to).unwrap(),
        ChannelCount::new(channels).unwrap(),
        ResamplingMethod::Sinc24,
    )
    .collect()
}

#[test]
fn sinc_preserves_duration_and_channel_isolation_even_for_short_sources() {
    for frames in [0, 1, 2, 11, 25, 100] {
        let input: Vec<_> = (0..frames).flat_map(|_| [0.5, 0.0]).collect();
        let output = convert(input, 2, 11025, 48000);
        assert_eq!(output.len(), (frames * 48000_usize).div_ceil(11025) * 2);
        for frame in output.as_chunks::<2>().0 {
            assert!((frame[0] - 0.5).abs() < 1e-4, "DC sample: {}", frame[0]);
            assert_eq!(frame[1], 0.0);
        }
    }
}

#[test]
fn equal_rates_are_exactly_unchanged() {
    let input = vec![0.2, -0.5, 0.0, 1.0];
    assert_eq!(convert(input.clone(), 1, 22050, 22050), input);
}

#[test]
fn sinc_reconstructs_a_tone_more_accurately_than_linear_interpolation() {
    let from = 11025;
    let to = 44100;
    let frequency = 3000.0;
    let tone = |frame: f64, rate: u32| {
        (std::f64::consts::TAU * frequency * frame / f64::from(rate)).sin() as f32
    };
    let input: Vec<_> = (0..11025).map(|i| tone(f64::from(i), from)).collect();
    let sinc = convert(input.clone(), 1, from, to);
    let linear: Vec<_> = SampleRateConverter::new(
        input.into_iter(),
        SampleRate::new(from).unwrap(),
        SampleRate::new(to).unwrap(),
        ChannelCount::new(1).unwrap(),
    )
    .collect();
    let error = |samples: &[f32]| {
        (100..44000)
            .map(|i| f64::from(samples[i] - tone(i as f64, to)).powi(2))
            .sum::<f64>()
    };
    assert!(error(&sinc) < error(&linear) / 100.0);
}

#[test]
fn modern_mixer_uses_sinc_and_keeps_queued_cues_separate() {
    use rodio::conversions::set_default_resampling_method;
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            set_default_resampling_method(ResamplingMethod::Linear);
        }
    }
    let _reset = Reset;
    set_default_resampling_method(ResamplingMethod::Sinc24);
    let (mixer, output) = rodio::mixer::mixer(
        ChannelCount::new(2).unwrap(),
        SampleRate::new(44100).unwrap(),
    );
    let player = rodio::Player::connect_new(&mixer);
    let first = vec![0.125, 0.5, -0.25];
    player.append(rodio::buffer::SamplesBuffer::new(
        ChannelCount::new(1).unwrap(),
        SampleRate::new(11025).unwrap(),
        first.clone(),
    ));
    let second = vec![0.0, 0.0, 0.0];
    player.append(rodio::buffer::SamplesBuffer::new(
        ChannelCount::new(1).unwrap(),
        SampleRate::new(22050).unwrap(),
        second.clone(),
    ));
    let expected: Vec<_> = convert(first, 1, 11025, 44100)
        .into_iter()
        .chain(convert(second, 1, 22050, 44100))
        .flat_map(|sample| [sample, sample])
        .collect();
    assert_eq!(output.take(expected.len()).collect::<Vec<_>>(), expected);
}

#[test]
fn sinc_remaining_length_and_cloned_streams_stay_exact() {
    let mut converter = SampleRateConverter::new_with_method(
        (0..202)
            .map(|i| i as f32 / 256.0)
            .collect::<Vec<_>>()
            .into_iter(),
        SampleRate::new(22050).unwrap(),
        SampleRate::new(48000).unwrap(),
        ChannelCount::new(2).unwrap(),
        ResamplingMethod::Sinc24,
    );
    let count = (101 * 48000_usize).div_ceil(22050) * 2;
    let mut continuation = None;
    let mut actual_continuation = Vec::new();
    for remaining in (0..=count).rev() {
        assert_eq!(converter.size_hint(), (remaining, Some(remaining)));
        if remaining == count / 2 {
            continuation = Some(converter.clone().collect::<Vec<_>>());
        }
        if remaining != 0 {
            let sample = converter.next().unwrap();
            if remaining <= count / 2 {
                actual_continuation.push(sample);
            }
        }
    }
    assert_eq!(converter.next(), None);
    assert_eq!(continuation.unwrap(), actual_continuation);
}
