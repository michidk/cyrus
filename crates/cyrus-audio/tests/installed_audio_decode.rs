//! Installed source PCM survives the public playback transports without resampling.
use bevy::audio::{AudioSource, Decodable, Source};
use cyrus_assets::{
    audio::load_speech,
    install::GameInstall,
    music::{music_source, read_track},
};
use cyrus_audio::{
    music::MusicTransport,
    sound::{Panned, PannedGains, PcmWidth},
};
use std::{io::Cursor, path::PathBuf};

fn install() -> Option<GameInstall> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    match GameInstall::locate(Some(&root)) {
        Ok(install) => Some(install),
        Err(error) => {
            eprintln!("skipped: no install (source PCM playback: {error})");
            None
        }
    }
}

#[test]
fn installed_cd_transport_preserves_every_sample_and_the_loop_boundary() {
    let Some(install) = install() else {
        return;
    };
    let source = music_source(&install).expect("installed CD source");
    let bytes = read_track(&source, 2).unwrap();
    let mut reference = hound::WavReader::new(Cursor::new(&bytes)).unwrap();
    let spec = reference.spec();
    let count = reference.len() as usize;
    let transport = MusicTransport::new(AudioSource {
        bytes: bytes.clone().into(),
    })
    .unwrap();
    let mut playback = transport.decoder();
    assert_eq!(playback.sample_rate().get(), spec.sample_rate);
    assert_eq!(playback.channels().get(), spec.channels);
    let mut head = Vec::new();
    let mut peak = 0.0_f32;
    let mut energy = 0.0_f64;
    for (index, sample) in reference.samples::<i16>().enumerate() {
        let expected = f32::from(sample.unwrap()) / 32768.0;
        assert_eq!(playback.next(), Some(expected), "CD sample {index}");
        if index < 512 {
            head.push(expected);
        }
        peak = peak.max(expected.abs());
        energy += f64::from(expected).powi(2);
    }
    assert_eq!(
        playback.by_ref().take(head.len()).collect::<Vec<_>>(),
        head,
        "the next pass must retain the first stereo frame, without padding or duplication"
    );
    eprintln!(
        "installed CD track 2: frames={} rate={} peak={peak} rms={} exact_samples={count}",
        count / usize::from(spec.channels),
        spec.sample_rate,
        (energy / count as f64).sqrt()
    );
}

#[test]
fn installed_sabre_voice_preserves_onset_pcm_rate_and_complete_duration() {
    let Some(install) = install() else {
        return;
    };
    let settings = cyrus_assets::audio::read_settings(&install);
    let clips = load_speech(&install.root.join(settings.rtx_filename)).unwrap();
    let (_, clip) = clips
        .iter()
        .find(|(label, _)| label.eq_ignore_ascii_case("CISW"))
        .expect("installed sabre description");
    let mut reference = hound::WavReader::new(Cursor::new(&clip.audio)).unwrap();
    let spec = reference.spec();
    assert_eq!((spec.channels, spec.bits_per_sample), (1, 8));
    let frames = reference.len();
    let transport = Panned::new(
        AudioSource {
            bytes: clip.audio.clone().into(),
        },
        PannedGains::new(PcmWidth::Eight, 127, 256, 64),
    )
    .unwrap();
    let mut playback = transport.decoder();
    assert_eq!(playback.sample_rate().get(), spec.sample_rate);
    assert_eq!(playback.channels().get(), 2);
    // Published sound.md gives the 8-bit table increment. The shipped header
    // volume 100 and master 255 yield table factor 100/128 at centre pan.
    assert_eq!((clip.base_volume, settings.master_volume), (100, 255));
    let configured = Panned::new(
        AudioSource {
            bytes: clip.audio.clone().into(),
        },
        PannedGains::new(
            PcmWidth::Eight,
            clip.base_volume,
            settings.master_volume,
            64,
        ),
    )
    .unwrap();
    let mut configured = configured.decoder();
    let mut peak = 0.0_f32;
    let mut energy = 0.0_f64;
    for (index, sample) in reference.samples::<i8>().enumerate() {
        let expected = f32::from(sample.unwrap()) / 128.0;
        for channel in 0..2 {
            assert_eq!(
                playback.next(),
                Some(expected),
                "speech frame {index}, channel {channel}"
            );
            assert_eq!(
                configured.next(),
                Some(expected * (100.0 / 128.0)),
                "configured speech frame {index}, channel {channel}"
            );
        }
        let expected = expected * (100.0 / 128.0);
        peak = peak.max(expected.abs());
        energy += f64::from(expected).powi(2);
    }
    assert_eq!(configured.next(), None);
    assert_eq!(
        playback.next(),
        None,
        "speech must retire after its complete source PCM"
    );
    eprintln!(
        "installed CISW: frames={frames} rate={} seconds={} peak={peak} rms={} exact_samples={}",
        spec.sample_rate,
        f64::from(frames) / f64::from(spec.sample_rate),
        (energy / f64::from(frames)).sqrt(),
        frames * 2
    );
}
