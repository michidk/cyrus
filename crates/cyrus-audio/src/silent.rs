//! Device-free playback consumes the same decoded sources and sink controls on the host clock.
use bevy::{
    audio::{AudioSink, AudioSinkPlayback, Decodable, GlobalVolume, PlaybackMode, Source},
    prelude::*,
};
use std::{any::TypeId, marker::PhantomData, sync::Mutex};

/// Native unattended audio transport. Sources advance without an output device while ordinary
/// `AudioSink` pause, stop, speed and completion contracts remain available to gameplay.
/// Add after the audio source plugins; this host intentionally produces no audible output.
pub struct SilentAudioPlugin;

impl Plugin for SilentAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SilentAudioHost>();
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
    service_fraction: u128,
    source_rate: u32,
    source_channels: u16,
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
        let source_rate = source.sample_rate().get();
        let source_channels = source.channels().get();
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
                service_fraction: 0,
                source_rate,
                source_channels,
                last: time.elapsed_secs_f64(),
                source: PhantomData,
            },
        ));
    }
}

fn advance<T: Asset + Decodable>(
    mut commands: Commands,
    time: Res<Time<Real>>,
    output: Option<Res<DigitalAudioOutput>>,
    mut sinks: Query<(Entity, &AudioSink, &PlaybackSettings, &mut SilentSink<T>)>,
) {
    if output.is_some() && TypeId::of::<T>() == TypeId::of::<crate::sound::Panned>() {
        return;
    }
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

#[derive(Resource, Default)]
struct SilentAudioHost;

/// One active half of a double-buffered digital output. A repeated half is not
/// another service request. Sources share these transitions, including sources
/// admitted while the output is already running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigitalOutputHalf {
    First,
    Second,
}

/// Device-free digital output driven by actual output-half observations.
///
/// Geometry is supplied by the host, in stereo output frames and Hz. It is not
/// inferred from a source duration or a game/BIOS clock. Source consumption and
/// completion happen during service of a changed half, before device drainage,
/// following preservation's digital sample completion contract.
#[derive(Resource)]
pub struct DigitalAudioOutput {
    frames_per_half: std::num::NonZeroU32,
    sample_rate: std::num::NonZeroU32,
    serviced: Option<DigitalOutputHalf>,
}

/// Select observed digital-output servicing before admitting any sample players.
/// Requires the silent host; never adds a second consumer to a real device.
/// Invalid geometry or existing sample players leave the prior output intact.
pub fn configure_digital_output(
    world: &mut World,
    frames_per_half: u32,
    sample_rate: u32,
) -> anyhow::Result<()> {
    use anyhow::ensure;
    ensure!(
        world.contains_resource::<SilentAudioHost>(),
        "digital output requires the device-free audio host"
    );
    ensure!(
        world.contains_resource::<Assets<crate::sound::Panned>>(),
        "digital output requires sample assets"
    );
    let frames_per_half = std::num::NonZeroU32::new(frames_per_half)
        .ok_or_else(|| anyhow::anyhow!("digital output frames per half must be nonzero"))?;
    let sample_rate = std::num::NonZeroU32::new(sample_rate)
        .ok_or_else(|| anyhow::anyhow!("digital output sample rate must be nonzero"))?;
    ensure!(
        world
            .query_filtered::<Entity, With<AudioPlayer<crate::sound::Panned>>>()
            .iter(world)
            .next()
            .is_none(),
        "digital output cannot replace geometry while sample players are admitted"
    );
    world.insert_resource(DigitalAudioOutput {
        frames_per_half,
        sample_rate,
        serviced: None,
    });
    Ok(())
}

/// Service a measured output-half transition for every admitted sample source.
///
/// A new source joins the shared output phase. Repeated observations do nothing;
/// no host elapsed-time advance consumes samples in this mode. Fractional source
/// frames carry between services. CD music retains its separate host transport.
/// The caller delivers callbacks in their observed order relative to source
/// admission and ordinary updates; this function does not reconstruct that order
/// from frame endpoints or supply a source-admission timestamp.
pub fn service_digital_output(world: &mut World, half: DigitalOutputHalf) -> anyhow::Result<()> {
    let output = world
        .get_resource::<DigitalAudioOutput>()
        .ok_or_else(|| anyhow::anyhow!("digital output geometry has not been configured"))?;
    if output.serviced == Some(half) {
        return Ok(());
    }
    let frames = u128::from(output.frames_per_half.get());
    let rate = u128::from(output.sample_rate.get());
    let finished = world.resource_scope(|world, assets: Mut<Assets<crate::sound::Panned>>| {
        let mut finished = Vec::new();
        let mut query = world.query::<(
            Entity,
            &AudioSink,
            &PlaybackSettings,
            &AudioPlayer<crate::sound::Panned>,
            &mut SilentSink<crate::sound::Panned>,
        )>();
        for (entity, sink, settings, player, mut transport) in query.iter_mut(world) {
            if !sink.is_paused() {
                let source_rate = (transport.source_rate as f32 * sink.speed()).max(1.0) as u32;
                let count = transport.service_fraction
                    + frames * u128::from(source_rate) * u128::from(transport.source_channels);
                let output = transport
                    .output
                    .get_mut()
                    .expect("silent audio queue poisoned");
                let mut samples = count / rate;
                while samples > 0 && !sink.empty() {
                    let _ = output.next();
                    samples -= 1;
                }
                if !sink.empty()
                    && assets
                        .get(&player.0)
                        .is_some_and(crate::sound::Panned::source_exhausted)
                {
                    // Finish queue bookkeeping at an exact source boundary. The
                    // admitted decoder has no source sample left for this read.
                    let _ = output.next();
                }
                transport.service_fraction = count % rate;
            }
            if sink.empty() {
                finished.push((entity, settings.mode));
            }
        }
        finished
    });
    world.resource_mut::<DigitalAudioOutput>().serviced = Some(half);
    for (entity, mode) in finished {
        match mode {
            PlaybackMode::Despawn => {
                world.despawn(entity);
            }
            PlaybackMode::Remove => {
                world.entity_mut(entity).remove::<(
                    AudioPlayer<crate::sound::Panned>,
                    AudioSink,
                    PlaybackSettings,
                    SilentSink<crate::sound::Panned>,
                )>();
            }
            PlaybackMode::Once | PlaybackMode::Loop => {}
        }
    }
    Ok(())
}
