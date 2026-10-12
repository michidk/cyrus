//! Movie track preparation through the public runtime, using synthetic PCM only.
#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
use cyrus_assets::{Content, Preview, install::GameInstall, native::Library};

#[test]
fn sparse_movie_tracks_keep_their_samples_and_metadata() {
    let root = std::env::temp_dir().join(format!("cyrus-movie-tracks-{}", std::process::id()));
    std::fs::create_dir_all(root.join("install/maps")).unwrap();
    std::fs::create_dir_all(root.join("install/fxart")).unwrap();
    std::fs::write(root.join("install/RGFX.EXE"), []).unwrap();
    let mut bytes = vec![0u8; 104];
    bytes[..4].copy_from_slice(b"SMK4");
    for (offset, value) in [
        (4, 4u32),
        (8, 4),
        (12, 3),
        (16, 100),
        (28, 4),
        (40, 4),
        (52, 1),
        (56, 16),
        (60, 16),
        (64, 16),
        (68, 16),
        (76, 0x4000_0000 | 8000),
        (88, 0x4000_0000 | 16000),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for size in [16u32, 0, 8] {
        bytes.extend(size.to_le_bytes());
    }
    bytes.extend([0x24, 0, 0x20, 0]);
    bytes.extend(8u32.to_le_bytes());
    bytes.extend([128, 129, 130, 131]);
    bytes.extend(8u32.to_le_bytes());
    bytes.extend([132, 133, 134, 135]);
    bytes.extend(8u32.to_le_bytes());
    bytes.extend([136, 137, 138, 139]);
    std::fs::write(root.join("install/tracks.SMK"), bytes).unwrap();
    let library = Library::open(
        GameInstall::open(&root.join("install")).unwrap(),
        root.join("cache"),
        |_, _, _| {},
    )
    .unwrap();
    let entry = library
        .catalog
        .entries
        .iter()
        .find(|entry| entry.path == "tracks.SMK")
        .unwrap();
    let output = library.prepare(&entry.id, None).unwrap();
    let mut preview: Preview = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    preview.validate().unwrap();
    assert_eq!(preview.files().len(), 3);
    let Content::Movie {
        audio_tracks,
        seconds,
        ..
    } = &preview.content
    else {
        panic!("expected movie");
    };
    assert_eq!(*seconds, 0.3);
    assert_eq!(
        audio_tracks
            .iter()
            .map(|track| track.index)
            .collect::<Vec<_>>(),
        [1, 4]
    );
    assert_eq!(
        audio_tracks
            .iter()
            .map(|track| track.sample_frames)
            .collect::<Vec<_>>(),
        [4, 8]
    );
    for (track, expected) in audio_tracks
        .iter()
        .zip([vec![0i8, 1, 2, 3], vec![4, 5, 6, 7, 8, 9, 10, 11]])
    {
        let mut wav = hound::WavReader::open(library.cache.join(&track.wav)).unwrap();
        assert_eq!(wav.spec().sample_rate, track.sample_rate);
        assert_eq!(
            wav.samples::<i8>().collect::<Result<Vec<_>, _>>().unwrap(),
            expected
        );
    }
    assert_eq!(
        preview.metadata.as_ref().unwrap()["audio_tracks"][1]["sample_rate"],
        16000
    );
    let Content::Movie { audio_tracks, .. } = &mut preview.content else {
        unreachable!()
    };
    audio_tracks[1].index = 1;
    assert!(preview.validate().is_err());
    std::fs::remove_dir_all(root).unwrap();
}
