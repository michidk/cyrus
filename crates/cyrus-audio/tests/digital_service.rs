//! Shared output servicing through the public silent-host interface.
use bevy::{asset::AssetApp, audio::GlobalVolume, prelude::*, time::TimeUpdateStrategy};
use cyrus_audio::sound::{Panned, PannedGains, PcmWidth};
use cyrus_audio::{DigitalOutputHalf, configure_digital_output, service_digital_output};
use std::{io::Cursor, time::Duration};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<AudioSource>()
        .init_asset::<Panned>()
        .init_asset::<cyrus_audio::music::MusicTransport>()
        .init_resource::<GlobalVolume>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)))
        .add_plugins(cyrus_audio::SilentAudioPlugin);
    configure_digital_output(app.world_mut(), 512, 22050).unwrap();
    app
}
fn voice(app: &mut App, samples: usize) -> (Entity, Handle<Panned>) {
    let mut bytes = Cursor::new(Vec::new());
    let mut wav = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 1,
            sample_rate: 11025,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..samples {
        wav.write_sample(1_i16).unwrap();
    }
    wav.finalize().unwrap();
    let asset = Panned::new(
        AudioSource {
            bytes: bytes.into_inner().into(),
        },
        PannedGains::new(PcmWidth::Sixteen, 127, 256, 64),
    )
    .unwrap();
    let settings = asset.playback_settings();
    let handle = app.world_mut().resource_mut::<Assets<Panned>>().add(asset);
    let entity = app
        .world_mut()
        .spawn((AudioPlayer(handle.clone()), settings))
        .id();
    app.update();
    (entity, handle)
}
fn cursor(app: &App, handle: &Handle<Panned>) -> u64 {
    let snapshot = app
        .world()
        .resource::<Assets<Panned>>()
        .get(handle)
        .unwrap()
        .snapshot()
        .unwrap();
    serde_json::to_value(snapshot).unwrap()["input_samples"]
        .as_u64()
        .unwrap()
}
#[test]
fn sources_share_half_transitions_and_finish_during_service() {
    let mut app = app();
    let (first, source) = voice(&mut app, 1003);
    assert_eq!(cursor(&app, &source), 0);
    service_digital_output(app.world_mut(), DigitalOutputHalf::Second).unwrap();
    assert_eq!(cursor(&app, &source), 256);
    // Time, polling and repeated observations cannot produce another service.
    app.update();
    service_digital_output(app.world_mut(), DigitalOutputHalf::Second).unwrap();
    assert_eq!(cursor(&app, &source), 256);
    let (second, other) = voice(&mut app, 1003);
    service_digital_output(app.world_mut(), DigitalOutputHalf::Second).unwrap();
    assert_eq!(
        cursor(&app, &other),
        0,
        "admission restarted the shared phase"
    );
    service_digital_output(app.world_mut(), DigitalOutputHalf::First).unwrap();
    assert_eq!(cursor(&app, &source), 512);
    assert_eq!(cursor(&app, &other), 256);
    for index in 2..4 {
        let half = if index % 2 == 0 {
            DigitalOutputHalf::Second
        } else {
            DigitalOutputHalf::First
        };
        service_digital_output(app.world_mut(), half).unwrap();
    }
    assert!(
        app.world().get_entity(first).is_err(),
        "finite source waited for device drainage"
    );
    assert!(
        app.world().get_entity(second).is_ok(),
        "later admission consumed earlier services"
    );
    service_digital_output(app.world_mut(), DigitalOutputHalf::Second).unwrap();
    assert!(app.world().get_entity(second).is_err());
}
#[test]
fn invalid_geometry_and_reconfiguration_preserve_playback() {
    let mut app = app();
    let (_, source) = voice(&mut app, 1003);
    assert!(configure_digital_output(app.world_mut(), 0, 22050).is_err());
    assert!(configure_digital_output(app.world_mut(), 512, 0).is_err());
    assert!(configure_digital_output(app.world_mut(), 1024, 44100).is_err());
    assert_eq!(cursor(&app, &source), 0);
    service_digital_output(app.world_mut(), DigitalOutputHalf::First).unwrap();
    assert_eq!(cursor(&app, &source), 256);
}

#[test]
fn exact_half_boundary_finishes_in_that_service() {
    let mut app = app();
    let (entity, _) = voice(&mut app, 512);
    service_digital_output(app.world_mut(), DigitalOutputHalf::First).unwrap();
    assert!(app.world().get_entity(entity).is_ok());
    service_digital_output(app.world_mut(), DigitalOutputHalf::Second).unwrap();
    assert!(app.world().get_entity(entity).is_err());
}

#[test]
fn fractional_source_samples_carry_between_shared_services() {
    use bevy::audio::{AudioSink, AudioSinkPlayback};
    let mut app = app();
    configure_digital_output(app.world_mut(), 1, 48000).unwrap();
    let (entity, source) = voice(&mut app, 1003);
    for i in 0..160 {
        service_digital_output(
            app.world_mut(),
            if i % 2 == 0 {
                DigitalOutputHalf::First
            } else {
                DigitalOutputHalf::Second
            },
        )
        .unwrap();
    }
    let snapshot = app
        .world()
        .resource::<Assets<Panned>>()
        .get(&source)
        .unwrap()
        .snapshot()
        .unwrap();
    let json = serde_json::to_value(snapshot).unwrap();
    assert_eq!(cursor(&app, &source), 37);
    assert!(
        !json["right_bits"].is_null(),
        "73 output samples retain the pending right sample"
    );
    app.world().get::<AudioSink>(entity).unwrap().pause();
    service_digital_output(app.world_mut(), DigitalOutputHalf::First).unwrap();
    assert_eq!(cursor(&app, &source), 37);
}
