//! Public device-free transport contract: decoded time, pause, stop and entity completion.
use bevy::{
    asset::AssetApp,
    audio::{AudioSink, AudioSinkPlayback, GlobalVolume},
    prelude::*,
    time::TimeUpdateStrategy,
};
use std::{io::Cursor, time::Duration};

#[test]
fn device_free_voice_finishes_and_obeys_sink_controls() {
    let mut bytes = Cursor::new(Vec::new());
    {
        let mut wav = hound::WavWriter::new(
            &mut bytes,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..800 {
            wav.write_sample(1_i16).unwrap();
        }
        wav.finalize().unwrap();
    }
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<AudioSource>()
        .init_asset::<cyrus_audio::sound::Panned>()
        .init_asset::<cyrus_audio::music::MusicTransport>()
        .init_resource::<GlobalVolume>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            20,
        )))
        .add_plugins(cyrus_audio::SilentAudioPlugin);
    let source = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: bytes.into_inner().into(),
        });
    let voice = app
        .world_mut()
        .spawn((AudioPlayer(source.clone()), PlaybackSettings::DESPAWN))
        .id();
    app.update();
    app.world().get::<AudioSink>(voice).unwrap().pause();
    for _ in 0..10 {
        app.update();
    }
    assert!(
        app.world().get_entity(voice).is_ok(),
        "paused voice finished"
    );
    app.world().get::<AudioSink>(voice).unwrap().play();
    for _ in 0..10 {
        app.update();
    }
    assert!(
        app.world().get_entity(voice).is_err(),
        "finished voice retained its sample owner"
    );
    let voice = app
        .world_mut()
        .spawn((AudioPlayer(source), PlaybackSettings::LOOP))
        .id();
    app.update();
    for _ in 0..10 {
        app.update();
    }
    let sink = app.world().get::<AudioSink>(voice).unwrap();
    assert!(!sink.empty(), "looped source finished");
    sink.stop();
    app.update();
    assert!(app.world().get::<AudioSink>(voice).unwrap().empty());
}
