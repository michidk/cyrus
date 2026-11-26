//! Diagnostic CD ownership survives the ordinary world notification schedule.
use bevy::{asset::AssetApp, prelude::*};
use cyrus_assets::{install::GameInstall, sound::SoundSettings};
use cyrus_audio::music::{Music, MusicPlugin, MusicSnapshot, MusicTransport};
use cyrus_world::lifecycle::{CurrentScene, PreparedWorld, SceneRequest, WorldPlugin};

#[test]
fn restored_stopped_cd_survives_scene_notifications_but_not_a_later_world_entry() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (restored CD world notification schedule)");
        return;
    };
    let track = cyrus_assets::music::world_track(&install, "0").unwrap();
    assert!(track > 0);
    let saved: MusicSnapshot = serde_json::from_value(serde_json::json!({
        "version":1,"track":null,"gain":1.0,"playback":null
    }))
    .unwrap();
    let candidate = Music::from_snapshot(saved, None).unwrap();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::state::app::StatesPlugin,
    ))
    .insert_state(cyrus_game::AppState::InGame)
    .init_asset::<bevy::audio::AudioSource>()
    .init_asset::<MusicTransport>()
    .init_asset::<cyrus_assets::animation::Animations>()
    .insert_resource(install)
    .insert_resource(SoundSettings::parse(""))
    .insert_resource(CurrentScene {
        name: "0".into(),
        world: Some(0),
    })
    .insert_resource(SceneRequest("0".into()))
    .add_plugins((WorldPlugin, MusicPlugin))
    .insert_resource(candidate.music);
    let prepared = PreparedWorld {
        requested_name: "0".into(),
        world: Some(0),
        animations: default(),
        scripts: default(),
        player_path: "0.player".into(),
        scene_path: "0.glb".into(),
        environment_path: "0.environment".into(),
    };
    app.insert_resource(prepared.clone());
    app.update();
    assert_eq!(
        app.world().resource::<Music>().track,
        None,
        "restored stopped owner was replaced by world-entry music"
    );
    app.update();
    assert_eq!(app.world().resource::<Music>().track, None);
    app.world_mut().resource_mut::<SoundSettings>().speech = false;
    app.update();
    assert_eq!(
        app.world().resource::<Music>().track,
        None,
        "speech preference replaced established stopped CD ownership"
    );
    app.insert_resource(prepared);
    app.insert_resource(SceneRequest("0".into()));
    app.insert_resource(CurrentScene {
        name: "0".into(),
        world: Some(0),
    });
    app.update();
    assert_eq!(
        app.world().resource::<Music>().track,
        Some(track as u32),
        "restoration suppression must end before a later ordinary world entry"
    );
}

#[test]
fn world_music_waits_for_initial_settings_but_not_subsequent_preferences() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (world music settings admission)");
        return;
    };
    let track = cyrus_assets::music::world_track(&install, "0").unwrap();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::state::app::StatesPlugin,
    ))
    .insert_state(cyrus_game::AppState::InGame)
    .init_asset::<bevy::audio::AudioSource>()
    .init_asset::<MusicTransport>()
    .insert_resource(install)
    .insert_resource(CurrentScene {
        name: "0".into(),
        world: Some(0),
    })
    .add_plugins(MusicPlugin);
    app.update();
    assert_eq!(app.world().resource::<Music>().track, None);
    app.insert_resource(SoundSettings::parse(""));
    app.update();
    assert_eq!(
        app.world().resource::<Music>().track,
        Some(track as u32),
        "late installed settings must admit the pending world selection"
    );
    // Public diagnostic replacement establishes a stopped owner after world admission.
    let saved: MusicSnapshot = serde_json::from_value(serde_json::json!({
        "version":1,"track":null,"gain":1.0,"playback":null
    }))
    .unwrap();
    app.insert_resource(Music::from_snapshot(saved, None).unwrap().music);
    app.update();
    for setting in [0, 1, 2] {
        let mut settings = app.world_mut().resource_mut::<SoundSettings>();
        match setting {
            0 => settings.speech = false,
            1 => settings.master_volume = 0,
            _ => settings.redbook_volume = 0,
        }
        app.update();
        assert_eq!(
            app.world().resource::<Music>().track,
            None,
            "preference update selected world music after restoration"
        );
    }
}

#[test]
fn prepared_world_music_waits_for_gameplay_admission() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (world CD gameplay admission)");
        return;
    };
    let track = cyrus_assets::music::world_track(&install, "1").unwrap();
    let settings =
        SoundSettings::parse(&std::fs::read_to_string(install.root.join("SYSTEM.INI")).unwrap());
    let device = std::env::var_os("CYRUS_MUSIC_ADMISSION_DEVICE_ACCEPTANCE").is_some();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::state::app::StatesPlugin,
    ))
    .insert_state(cyrus_game::AppState::Loading)
    .init_asset::<bevy::audio::AudioSource>()
    .init_asset::<MusicTransport>()
    .insert_resource(install)
    .insert_resource(settings)
    .insert_resource(CurrentScene {
        name: "1".into(),
        world: Some(1),
    });
    if device {
        app.add_plugins(bevy::audio::AudioPlugin::default());
    }
    app.add_plugins(MusicPlugin);
    app.finish();
    app.cleanup();
    if device {
        eprintln!("music-admission: Loading");
    }
    let mut players = app
        .world_mut()
        .query_filtered::<&bevy::audio::PlaybackSettings, With<AudioPlayer<MusicTransport>>>();
    // Exercise asynchronous preparation without an audio device. Publication must
    // remain silent for the entire Loading state, even if the track is ready.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        app.update();
        assert!(
            players.iter(app.world()).all(|playback| playback.paused),
            "prepared world started CD playback before gameplay admission"
        );
        assert!(app.world().resource::<Music>().is_preparing());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // A repeated notification for the same destination must not release its
    // prepared audio while Loading still owns presentation.
    app.insert_resource(CurrentScene {
        name: "1".into(),
        world: Some(1),
    });
    app.update();
    assert!(players.iter(app.world()).all(|playback| playback.paused));
    if device {
        eprintln!("music-admission: InGame");
    }
    app.world_mut()
        .resource_mut::<NextState<cyrus_game::AppState>>()
        .set(cyrus_game::AppState::InGame);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        if players.iter(app.world()).any(|playback| !playback.paused) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "gameplay admission lost the pending world track"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(app.world().resource::<Music>().track, Some(track as u32));
    assert!(!app.world().resource::<Music>().is_preparing());
    if device {
        // The paced backend, rather than a render frame count, owns this span.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            app.update();
            let mut active = app.world_mut().query::<&AudioPlayer<MusicTransport>>();
            let player = active.single(app.world()).unwrap();
            let transports = app.world().resource::<Assets<MusicTransport>>();
            let transport = transports.get(&player.0).unwrap();
            let snapshot = app
                .world()
                .resource::<Music>()
                .snapshot(&[(transport, false)])
                .unwrap();
            let value = serde_json::to_value(snapshot).unwrap();
            let samples = value["playback"]["transport"]["input_samples"]
                .as_u64()
                .unwrap();
            if samples >= 10 * 44_100 * 2 {
                eprintln!("music-admission: complete {samples} source samples");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "CD device cursor stalled"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let mut owners = app
        .world_mut()
        .query_filtered::<Entity, With<AudioPlayer<MusicTransport>>>();
    let owner = owners.single(app.world()).unwrap();
    app.world_mut()
        .resource_mut::<NextState<cyrus_game::AppState>>()
        .set(cyrus_game::AppState::Loading);
    app.insert_resource(CurrentScene {
        name: "1".into(),
        world: Some(1),
    });
    app.update();
    assert_eq!(owners.single(app.world()).unwrap(), owner);
    assert!(players.iter(app.world()).any(|playback| !playback.paused));
}

#[test]
fn book_music_waits_for_installed_settings_and_cancels_on_exit() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (book CD settings admission)");
        return;
    };
    let settings =
        SoundSettings::parse(&std::fs::read_to_string(install.root.join("SYSTEM.INI")).unwrap());
    assert!(settings.music);
    for leave_before_settings in [false, true] {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
        ))
        .insert_state(cyrus_game::AppState::Menu)
        .init_resource::<CurrentScene>()
        .init_asset::<bevy::audio::AudioSource>()
        .init_asset::<MusicTransport>()
        .insert_resource(install.clone())
        .add_plugins(MusicPlugin);
        app.update();
        assert_eq!(app.world().resource::<Music>().track, None);
        if leave_before_settings {
            app.world_mut()
                .resource_mut::<NextState<cyrus_game::AppState>>()
                .set(cyrus_game::AppState::Loading);
            app.update();
        }
        app.insert_resource(settings.clone());
        app.update();
        assert_eq!(
            app.world().resource::<Music>().track,
            if leave_before_settings { None } else { Some(2) },
            "book music must wait for settings, but an exited book must not start music"
        );
        if !leave_before_settings {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                app.update();
                let playback = app.world_mut().query_filtered::<
                    &bevy::audio::PlaybackSettings,
                    With<bevy::audio::AudioPlayer<MusicTransport>>,
                >().iter(app.world()).copied().collect::<Vec<_>>();
                if let [playback] = playback.as_slice() {
                    assert!(!playback.paused);
                    assert_eq!(
                        playback.volume.to_linear(),
                        cyrus_audio::music::redbook_gain(settings.redbook_volume)
                    );
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "late book settings never published installed CD playback"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Public restoration installs a stopped owner; unrelated preference updates must
            // not replay the already-consumed book entry request.
            let saved: MusicSnapshot = serde_json::from_value(serde_json::json!({
                "version":1,"track":null,"gain":1.0,"playback":null
            }))
            .unwrap();
            app.insert_resource(Music::from_snapshot(saved, None).unwrap().music);
            app.world_mut().resource_mut::<SoundSettings>().speech = false;
            app.update();
            assert_eq!(app.world().resource::<Music>().track, None);
        }
    }
}
