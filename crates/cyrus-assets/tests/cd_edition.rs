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
