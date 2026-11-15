//! CD-edition media: the repack's `CDROM/` (movies) and ripped `Soundtrack/` (music) beside
//! the install, with no GOG `game.gog`/`game.ins`. `REDGUARD_CD_DIR` names that install folder.
#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
use cyrus_assets::{install::GameInstall, movie::MovieInput, music, startup::movie_native};

#[test]
fn cd_edition_movies_and_music_load_without_the_gog_image() {
    let Some(root) = std::env::var_os("REDGUARD_CD_DIR") else {
        eprintln!("skipped: no CD-edition install (REDGUARD_CD_DIR)");
        return;
    };
    let install = GameInstall::open(root.as_ref()).unwrap();
    let parent = install.root.parent().unwrap();
    assert!(
        !parent.join("game.gog").exists(),
        "layout must not borrow GOG's image"
    );

    let intro = movie_native::prepare(&install.root, 0).unwrap();
    assert!(
        matches!(intro.source, MovieInput::File(_)),
        "INTRO.SMK read from CDROM/"
    );
    assert!(intro.metadata.frames > 0);

    let source = music::music_source(&install).expect("Soundtrack/ beside the install");
    let wav = music::read_track(&source, 2).unwrap();
    let seconds = (wav.len() - 44) as f64 / 176_400.0;
    // GOG's track 2 lasts 5:45.04 (game.ins INDEX 51:59:71 to 57:45:00).
    assert!(
        (344.0..346.0).contains(&seconds),
        "track 2 lasts {seconds} s"
    );
    assert!(
        music::read_track(&source, 1).is_err(),
        "track 1 is the data track"
    );

    // The shared (viewer and browser export) path reads the same tracks from the manifest.
    let cache = std::env::temp_dir().join(format!("cyrus-cd-music-{}", std::process::id()));
    music::export(&install, &cache).unwrap();
    assert!(cache.join("music/02.wav").is_file());
    std::fs::remove_dir_all(cache).unwrap();
}

#[test]
fn ripped_cd_pcm_keeps_frame_count_and_silence() {
    let folder = std::env::temp_dir().join(format!("cyrus-cd-decoder-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    // Synthetic MPEG-1 Layer III, 128 kb/s, 44.1 kHz stereo, zero side/main data.
    // Eight frames contain 8 * 1152 stereo sample frames, with no gapless tag.
    let mut frame = vec![0; 417];
    frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
    std::fs::write(folder.join("01 Track 1.mp3"), frame.repeat(8)).unwrap();
    let wav = music::read_track(&folder, 2).unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(wav.len(), 44 + 8 * 1152 * 2 * 2);
    assert!(wav[44..].iter().all(|sample| *sample == 0));
    std::fs::write(folder.join("01 Another rip.mp3"), frame.repeat(8)).unwrap();
    let error =
        music::read_track(&folder, 2).expect_err("ambiguous ripped tracks must be rejected");
    assert!(
        error.contains("track 2") && error.contains("multiple soundtrack MP3"),
        "{error}"
    );
    assert!(
        error.contains("01 Track 1.mp3") && error.contains("01 Another rip.mp3"),
        "{error}"
    );
    std::fs::remove_file(folder.join("01 Another rip.mp3")).unwrap();
    std::fs::write(folder.join("02 Track 2.mp3"), b"not an MP3").unwrap();
    assert!(music::read_track(&folder, 3).is_err());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn cd_catalog_rejects_duplicate_track_aliases() {
    let root = std::env::temp_dir().join(format!("cyrus-cd-catalog-{}", std::process::id()));
    let install_root = root.join("Redguard");
    let soundtrack = root.join("Soundtrack");
    std::fs::create_dir_all(&install_root).unwrap();
    std::fs::create_dir_all(&soundtrack).unwrap();
    for name in ["01 Track 1.mp3", "01 Another rip.mp3"] {
        std::fs::write(soundtrack.join(name), b"synthetic media").unwrap();
    }
    let install = GameInstall { root: install_root };
    let error = cyrus_assets::native::Files::open(&install)
        .err()
        .expect("ambiguous CD media must not enter the catalog")
        .to_string();
    assert!(error.contains("disc/track02.mp3"), "{error}");
    assert!(
        error.contains("01 Track 1.mp3") && error.contains("01 Another rip.mp3"),
        "{error}"
    );
    std::fs::remove_dir_all(root).unwrap();
}
