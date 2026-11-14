use bevy::audio::{AudioSource, Decodable, PlaybackMode, Volume};
use cyrus_assets::audio::pcm_wav;
use cyrus_audio::music::{Music, MusicSnapshot, MusicTransport};
use rgpre::import::sfx::AudioType;

fn source() -> AudioSource {
    let pcm: Vec<_> = [16384i16, -16384, 8192, -8192, 32767, -32768]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    AudioSource {
        bytes: pcm_wav(AudioType::Stereo16, 44100, &pcm).unwrap().into(),
    }
}

#[test]
fn cd_replacement_retains_track_gain_pause_and_exact_source_cursor() {
    let expected = [0.5, -0.5, 0.25, -0.25, 32767.0 / 32768.0, -1.0];
    for consumed in [0, 1, 5, 6, 9, 19] {
        for paused in [false, true] {
            let mut music = Music::default();
            music.track = Some(2);
            music.gain = 128.0 / 255.0;
            let track = MusicTransport::new(source()).unwrap();
            let mut live = track.decoder();
            assert_eq!(
                live.by_ref().take(consumed).collect::<Vec<_>>(),
                expected
                    .into_iter()
                    .cycle()
                    .take(consumed)
                    .collect::<Vec<_>>()
            );
            let saved = serde_json::to_vec(&music.snapshot(&[(&track, paused)]).unwrap()).unwrap();
            let snapshot: MusicSnapshot = serde_json::from_slice(&saved).unwrap();
            assert_eq!(snapshot.track(), Some(2));
            assert!(snapshot.source_identity().is_some());
            let candidate = Music::from_snapshot(snapshot, Some(source())).unwrap();
            assert_eq!(candidate.music.track, Some(2));
            assert_eq!(candidate.music.gain, 128.0 / 255.0);
            let (restored_track, settings) = candidate.playback.unwrap();
            assert_eq!(settings.paused, paused);
            assert!(matches!(settings.mode, PlaybackMode::Despawn));
            assert!(matches!(settings.volume, Volume::Linear(v) if v == 128.0 / 255.0));
            assert_eq!(
                serde_json::to_vec(
                    &candidate
                        .music
                        .snapshot(&[(&restored_track, paused)])
                        .unwrap()
                )
                .unwrap(),
                saved
            );
            let expected: Vec<_> = expected
                .into_iter()
                .cycle()
                .skip(consumed)
                .take(18)
                .collect();
            assert_eq!(
                restored_track.decoder().take(18).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(live.take(18).collect::<Vec<_>>(), expected);
        }
    }
}

#[test]
fn cd_transport_preserves_every_signed_pcm16_value_at_unity_gain() {
    let pcm: Vec<_> = (i16::MIN..=i16::MAX).flat_map(i16::to_le_bytes).collect();
    let source = AudioSource {
        bytes: pcm_wav(AudioType::Stereo16, 44100, &pcm).unwrap().into(),
    };
    let expected: Vec<_> = source.decoder().collect();
    let track = MusicTransport::new(source).unwrap();
    assert_eq!(
        track.decoder().take(expected.len() * 2).collect::<Vec<_>>(),
        expected.repeat(2)
    );
}

#[test]
fn invalid_music_candidates_preserve_live_track_and_stopped_music_needs_no_source() {
    let mut music = Music::default();
    music.track = Some(2);
    let track = MusicTransport::new(source()).unwrap();
    let _decoder = track.decoder();
    let saved = serde_json::to_value(music.snapshot(&[(&track, true)]).unwrap()).unwrap();
    for path in [
        "version",
        "track",
        "gain",
        "playback.transport.speed",
        "playback.transport.repetitions",
        "playback.transport.gains",
        "playback.transport.frame_gains",
    ] {
        let mut invalid = saved.clone();
        let replacement = match path {
            "version" => serde_json::json!(2),
            "track" => serde_json::json!(0),
            "gain" => serde_json::json!(0.5),
            "playback.transport.speed" => serde_json::json!(0.75),
            "playback.transport.repetitions" => serde_json::json!(1),
            _ => serde_json::json!({"Sixteen":[64,64]}),
        };
        let mut field = &mut invalid;
        for segment in path.split('.') {
            field = &mut field[segment];
        }
        *field = replacement;
        assert!(
            Music::from_snapshot(serde_json::from_value(invalid).unwrap(), Some(source())).is_err(),
            "accepted {path}"
        );
        assert_eq!(
            serde_json::to_value(music.snapshot(&[(&track, true)]).unwrap()).unwrap(),
            saved
        );
    }
    assert!(music.snapshot(&[]).is_err());
    assert!(music.snapshot(&[(&track, true), (&track, true)]).is_err());
    assert!(Music::default().snapshot(&[(&track, false)]).is_err());
    assert!(Music::from_snapshot(serde_json::from_value(saved.clone()).unwrap(), None).is_err());
    let changed = AudioSource {
        bytes: pcm_wav(AudioType::Stereo16, 44100, &[0; 12])
            .unwrap()
            .into(),
    };
    assert!(Music::from_snapshot(serde_json::from_value(saved).unwrap(), Some(changed)).is_err());
    let stopped = Music::default().snapshot(&[]).unwrap();
    assert!(stopped.track().is_none());
    assert!(stopped.source_identity().is_none());
    assert!(Music::from_snapshot(stopped.clone(), Some(source())).is_err());
    let candidate = Music::from_snapshot(stopped, None).unwrap();
    assert!(candidate.music.track.is_none());
    assert!(candidate.playback.is_none());
}

#[test]
fn cd_transport_rejects_non_stereo_and_incomplete_frames_before_playback() {
    for (kind, samples) in [
        (AudioType::Mono16, vec![0i16, 1]),
        (AudioType::Stereo16, vec![]),
    ] {
        let pcm: Vec<_> = samples.into_iter().flat_map(i16::to_le_bytes).collect();
        let source = AudioSource {
            bytes: pcm_wav(kind, 44100, &pcm).unwrap().into(),
        };
        assert!(
            MusicTransport::new(source).is_err(),
            "invalid CD layout admitted: {kind:?}"
        );
    }
    let mut partial = source().bytes.to_vec();
    partial.truncate(partial.len() - 2);
    let riff_size = (partial.len() - 8) as u32;
    let data_size = (partial.len() - 44) as u32;
    partial[4..8].copy_from_slice(&riff_size.to_le_bytes());
    partial[40..44].copy_from_slice(&data_size.to_le_bytes());
    assert!(
        MusicTransport::new(AudioSource {
            bytes: partial.into()
        })
        .is_err()
    );
}

#[test]
fn installed_cd_sectors_keep_exact_stereo_pcm_through_transport() -> anyhow::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (CD stereo transport)");
        return Ok(());
    };
    let directory = install.root.parent().unwrap();
    let cue = std::fs::read_to_string(directory.join("game.ins"))?;
    for track in 2..=7 {
        let (file, start, _) =
            cyrus_assets::disc::cue_track(&cue, track, true).map_err(anyhow::Error::msg)?;
        let mut image = std::fs::File::open(directory.join(file))?;
        image.seek(SeekFrom::Start(start))?;
        let mut sector = vec![0; cyrus_assets::disc::SECTOR_BYTES as usize];
        image.read_exact(&mut sector)?;
        let expected: Vec<_> = sector
            .as_chunks::<2>()
            .0
            .iter()
            .map(|sample| f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32768.0)
            .collect();
        let source = AudioSource {
            bytes: pcm_wav(AudioType::Stereo16, 44100, &sector).unwrap().into(),
        };
        let transport = MusicTransport::new(source)?;
        assert_eq!(
            transport
                .decoder()
                .take(expected.len() * 2)
                .collect::<Vec<_>>(),
            expected.repeat(2),
            "CD track {track}"
        );
    }
    Ok(())
}
