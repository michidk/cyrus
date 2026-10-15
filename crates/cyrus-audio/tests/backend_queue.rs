use bevy::audio::{AudioSource, ChannelCount, Decodable, SampleRate};
use cyrus_assets::audio::pcm_wav;
use cyrus_audio::sound::{Panned, PannedGains, PcmWidth};
use rgpre::import::sfx::AudioType;

fn source(rate: u32) -> Panned {
    let pcm: Vec<_> = [4096i16, 16384, -8192]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    Panned {
        source: AudioSource {
            bytes: pcm_wav(AudioType::Mono16, rate, &pcm).unwrap().into(),
        },
        gains: PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
    }
}

#[test]
fn mono_decoder_retains_remaining_stereo_sample_count() {
    let mut decoder = source(22050).decoder();
    for remaining in (0..=6).rev() {
        assert_eq!(decoder.size_hint(), (remaining, Some(remaining)));
        if remaining != 0 {
            assert!(decoder.next().is_some());
        }
    }
    assert_eq!(decoder.next(), None);
}

#[test]
fn first_queued_cue_preserves_every_interpolated_frame_in_both_startup_orders() {
    for connect_first in [true, false] {
        let (mixer, output) = rodio::mixer::mixer(
            ChannelCount::new(2).unwrap(),
            SampleRate::new(44100).unwrap(),
        );
        let player;
        if connect_first {
            player = rodio::Player::connect_new(&mixer);
            player.append(source(22050).decoder());
        } else {
            let (handle, queue) = rodio::Player::new();
            player = handle;
            player.append(source(22050).decoder());
            mixer.add(queue);
        }
        assert_eq!(
            output.take(10).collect::<Vec<_>>(),
            [
                0.125, 0.125, 0.3125, 0.3125, 0.5, 0.5, 0.125, 0.125, -0.25, -0.25,
            ],
            "connect_first={connect_first}"
        );
    }
}

#[test]
fn cue_after_idle_uses_its_own_rate_and_channels() {
    let (mixer, mut output) = rodio::mixer::mixer(
        ChannelCount::new(2).unwrap(),
        SampleRate::new(44100).unwrap(),
    );
    let player = rodio::Player::connect_new(&mixer);
    assert!(output.by_ref().take(1024).all(|sample| sample == 0.0));
    player.append(source(22050).decoder());
    let samples: Vec<_> = output.take(2048).collect();
    let start = samples.iter().position(|sample| *sample != 0.0).unwrap();
    assert_eq!(start % 2, 0);
    assert_eq!(
        &samples[start..start + 10],
        &[
            0.125, 0.125, 0.3125, 0.3125, 0.5, 0.5, 0.125, 0.125, -0.25, -0.25
        ]
    );
}

#[test]
fn next_queued_cue_changes_rate_at_its_source_boundary() {
    let (mixer, output) = rodio::mixer::mixer(
        ChannelCount::new(2).unwrap(),
        SampleRate::new(44100).unwrap(),
    );
    let player = rodio::Player::connect_new(&mixer);
    player.append(source(44100).decoder());
    player.append(source(22050).decoder());
    assert_eq!(
        output.take(16).collect::<Vec<_>>(),
        [
            0.125, 0.125, 0.5, 0.5, -0.25, -0.25, 0.125, 0.125, 0.3125, 0.3125, 0.5, 0.5, 0.125,
            0.125, -0.25, -0.25,
        ]
    );
}

#[test]
fn queued_cue_applies_speed_before_conversion_reads_its_rate() {
    for set_speed_first in [false, true] {
        let (mixer, output) = rodio::mixer::mixer(
            ChannelCount::new(2).unwrap(),
            SampleRate::new(44100).unwrap(),
        );
        let player = rodio::Player::connect_new(&mixer);
        if set_speed_first {
            player.set_speed(2.0);
        }
        player.append(source(22050).decoder());
        if !set_speed_first {
            player.set_speed(2.0);
        }
        assert_eq!(
            output.take(6).collect::<Vec<_>>(),
            [0.125, 0.125, 0.5, 0.5, -0.25, -0.25],
            "set_speed_first={set_speed_first}"
        );
    }
}

#[test]
fn long_finite_cue_keeps_interpolation_continuous() {
    let (mixer, output) = rodio::mixer::mixer(
        ChannelCount::new(2).unwrap(),
        SampleRate::new(44100).unwrap(),
    );
    let player = rodio::Player::connect_new(&mixer);
    let mut pcm = vec![0.25; 20000];
    *pcm.last_mut().unwrap() = -0.25;
    player.append(rodio::buffer::SamplesBuffer::new(
        ChannelCount::new(1).unwrap(),
        SampleRate::new(22050).unwrap(),
        pcm,
    ));
    let actual: Vec<_> = output.take(79998).collect();
    assert!(actual[..79992].iter().all(|sample| *sample == 0.25));
    assert_eq!(&actual[79992..], &[0.25, 0.25, 0.0, 0.0, -0.25, -0.25]);
}
