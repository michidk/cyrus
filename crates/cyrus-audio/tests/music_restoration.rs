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
