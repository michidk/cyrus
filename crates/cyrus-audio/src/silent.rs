//! Device-free playback consumes the same decoded sources and sink controls on the host clock.
use bevy::{
    audio::{AudioSink, AudioSinkPlayback, Decodable, GlobalVolume, PlaybackMode, Source},
    prelude::*,
};
use std::{marker::PhantomData, sync::Mutex};

/// Native unattended audio transport. Sources advance without an output device while ordinary
/// `AudioSink` pause, stop, speed and completion contracts remain available to gameplay.
/// Add after the audio source plugins; this host intentionally produces no audible output.
pub struct SilentAudioPlugin;

impl Plugin for SilentAudioPlugin {
    fn build(&self, app: &mut App) {
        register::<AudioSource>(app);
        register::<crate::sound::Panned>(app);
        register::<crate::music::MusicTransport>(app);
    }
}

fn register<T: Asset + Decodable>(app: &mut App) {
    // Match device playback admission: gameplay queues sources during Update,
    // then PostUpdate creates their sinks before the next host interval begins.
    app.add_systems(PostUpdate, start::<T>)
        .add_systems(Update, advance::<T>.before(crate::ApplyPreferences));
}

#[derive(Component)]
struct SilentSink<T: Asset + Decodable> {
    output: Mutex<rodio::queue::SourcesQueueOutput>,
    remaining: f64,
    last: f64,
    source: PhantomData<fn() -> T>,
}

fn start<T: Asset + Decodable>(
    mut commands: Commands,
    time: Res<Time<Real>>,
    assets: Res<Assets<T>>,
    volume: Res<GlobalVolume>,
    players: Query<(Entity, &AudioPlayer<T>, &PlaybackSettings), Without<AudioSink>>,
) {
    for (entity, player, settings) in &players {
        let Some(asset) = assets.get(&player.0) else {
            continue;
        };
        let (player, output) = rodio::Player::new();
        let mut source: Box<dyn Source + Send> = Box::new(asset.decoder());
        if let Some(start) = settings.start_position {
            source = Box::new(source.skip_duration(start));
        }
        if let Some(duration) = settings.duration {
            source = Box::new(source.take_duration(duration));
        }
        if matches!(settings.mode, PlaybackMode::Loop) {
            source = Box::new(source.repeat_infinite());
        }
        player.append(source);
        let mut sink = AudioSink::new(player);
        sink.set_speed(settings.speed);
        sink.set_volume(settings.volume * volume.volume);
        if settings.paused {
            sink.pause();
        }
        if settings.muted {
            sink.mute();
        }
        commands.entity(entity).insert((
            sink,
            SilentSink::<T> {
                output: Mutex::new(output),
                remaining: 0.0,
                last: time.elapsed_secs_f64(),
                source: PhantomData,
            },
        ));
    }
}

fn advance<T: Asset + Decodable>(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut sinks: Query<(Entity, &AudioSink, &PlaybackSettings, &mut SilentSink<T>)>,
) {
    for (entity, sink, settings, mut transport) in &mut sinks {
        let now = time.elapsed_secs_f64();
        transport.remaining += now - transport.last;
        transport.last = now;
        let mut remaining = transport.remaining;
        {
            let output = transport
                .output
                .get_mut()
                .expect("silent audio queue poisoned");
            while !sink.empty() {
                let interval = 1.0
                    / (f64::from(output.sample_rate().get()) * f64::from(output.channels().get()));
                if remaining < interval {
                    break;
                }
                let _ = output.next();
                remaining -= interval;
            }
        }
        transport.remaining = remaining;
        if sink.empty() {
            match settings.mode {
                PlaybackMode::Despawn => {
                    commands.entity(entity).despawn();
                }
                PlaybackMode::Remove => {
                    commands.entity(entity).remove::<(
                        AudioPlayer<T>,
                        AudioSink,
                        PlaybackSettings,
                        SilentSink<T>,
                    )>();
                }
                PlaybackMode::Once | PlaybackMode::Loop => {}
            }
        }
    }
}
