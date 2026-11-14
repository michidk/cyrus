//! Installed examine speech must wait for per-label assets instead of treating an empty bank as silence.
use anyhow::Result;
use bevy::{
    asset::io::{AssetSourceBuilder, file::FileAssetReader},
    audio::{AudioPlayer, AudioSource, Decodable},
    prelude::*,
};
use cyrus_assets::install::GameInstall;
use cyrus_audio::{
    AudioReadiness, SoundPlugin,
    sound::{Panned, Speech},
};
use cyrus_script::item_viewer::ExamineScreen;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[test]
fn installed_examine_waits_for_voice_without_the_legacy_speech_bank() -> Result<()> {
    check_examine(*b"CISW")
}

#[test]
fn installed_gold_description_waits_for_its_voice() -> Result<()> {
    check_examine(*b"CIGD")
}

fn check_examine(label: [u8; 4]) -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (examine speech acquisition)");
        return Ok(());
    };
    let cache = install.cache_path(cyrus_config::cache_dir())?;
    let device = std::env::var_os("CYRUS_EXAMINE_DEVICE_ACCEPTANCE").is_some();
    let mut app = App::new();
    app.register_asset_source(
        "cache",
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(cache.clone()))),
    )
    .add_plugins((MinimalPlugins, AssetPlugin::default()))
    .init_asset::<AudioSource>()
    .init_asset_loader::<bevy::audio::AudioLoader>()
    .init_asset::<Panned>();
    if device {
        app.add_plugins(bevy::audio::AudioPlugin::default());
    }
    app.add_plugins(SoundPlugin)
        .insert_resource(AudioReadiness { prepare: false })
        .insert_resource(cyrus_assets::audio::read_settings(&install))
        .insert_resource(install)
        .init_resource::<ExamineScreen>();
    app.finish();
    app.cleanup();
    assert!(app.world().resource::<Speech>().0.is_empty());
    app.world_mut()
        .resource_mut::<ExamineScreen>()
        .speech_request = Some(label);
    app.update();
    assert_eq!(
        app.world().resource::<ExamineScreen>().speech_request,
        Some(label),
        "pending installed voice was consumed as an absent speech clip"
    );
    assert_eq!(app.world().resource::<ExamineScreen>().speech_reply, None);
    // A close while preparation is pending must not admit a delayed source.
    {
        let mut screen = app.world_mut().resource_mut::<ExamineScreen>();
        screen.speech_request = None;
        screen.speech_stop = true;
    }
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(
        app.world_mut()
            .query::<&AudioPlayer<Panned>>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(app.world().resource::<ExamineScreen>().speech_reply, None);
    app.world_mut()
        .resource_mut::<ExamineScreen>()
        .speech_request = Some(label);
    let deadline = Instant::now() + Duration::from_secs(60);
    while app
        .world()
        .resource::<ExamineScreen>()
        .speech_reply
        .is_none()
    {
        anyhow::ensure!(
            Instant::now() < deadline,
            "installed examine voice never became ready"
        );
        std::thread::sleep(Duration::from_millis(2));
        app.update();
    }
    let seconds = app
        .world()
        .resource::<ExamineScreen>()
        .speech_reply
        .flatten()
        .expect("installed description must have speech");
    let handle = app
        .world_mut()
        .query::<&AudioPlayer<Panned>>()
        .single(app.world())?
        .0
        .clone();
    let transport = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&handle)
        .unwrap();

    let source = hound::WavReader::new(std::io::Cursor::new(transport.source().bytes.as_ref()))?;
    assert_eq!(
        seconds,
        source.duration() as f32 / source.spec().sample_rate as f32
    );
    let frames = source.duration();
    eprintln!(
        "examine voice: {frames} frames at {} Hz, {seconds} seconds",
        source.spec().sample_rate
    );
    if !device {
        let pcm: Vec<_> = transport.decoder().collect();
        assert!(pcm.iter().any(|sample| *sample != 0.0));
        assert_eq!(pcm.len(), frames as usize * 2);
    }
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&AudioPlayer<Panned>>()
            .iter(app.world())
            .count(),
        1,
        "completed examine request replayed its voice"
    );
    if device {
        let end = Instant::now() + Duration::from_secs(15);
        while app
            .world_mut()
            .query::<&AudioPlayer<Panned>>()
            .iter(app.world())
            .count()
            != 0
        {
            anyhow::ensure!(
                Instant::now() < end,
                "real audio backend did not retire the description within its acquisition deadline"
            );
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            app.world_mut()
                .query::<&AudioPlayer<Panned>>()
                .iter(app.world())
                .count(),
            0,
            "real audio backend did not finish the installed description"
        );
    }
    Ok(())
}
