//! CD census capture for the same completed session boundary as logical state.
use anyhow::{Context, Result, ensure};
use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackMode, PlaybackSettings},
    prelude::{Assets, Entity, With, World},
};
use cyrus_audio::{
    CinematicAudio,
    music::{Music, MusicPlayer, MusicSnapshot, MusicTransport},
};

pub(super) fn capture(world: &World) -> Result<MusicSnapshot> {
    ensure!(
        world
            .try_query_filtered::<Entity, With<CinematicAudio>>()
            .is_none_or(|mut query| query.iter(world).next().is_none()),
        "session capture: cinematic playback has no session replacement owner"
    );
    let mut entities = world
        .try_query::<Entity>()
        .context("session entity query unavailable")?;
    let music = super::resource::<Music>(world, "CD music owner")?;
    let assets = world.get_resource::<Assets<MusicTransport>>();
    let mut players = Vec::new();
    for entity in entities.iter(world) {
        let marked = world.get::<MusicPlayer>(entity).is_some();
        let player = world.get::<AudioPlayer<MusicTransport>>(entity);
        if !marked && player.is_none() {
            continue;
        }
        let settings = world.get::<PlaybackSettings>(entity);
        let sink = world.get::<AudioSink>(entity);
        ensure!(
            marked,
            "session CD player {entity:?}: music owner marker absent"
        );
        let player =
            player.with_context(|| format!("session CD player {entity:?}: transport absent"))?;
        let transport = assets
            .and_then(|assets| assets.get(&player.0))
            .with_context(|| format!("session CD player {entity:?}: transport asset absent"))?;
        let settings = settings
            .with_context(|| format!("session CD player {entity:?}: playback settings absent"))?;
        ensure!(
            matches!(settings.mode, PlaybackMode::Despawn)
                && !settings.spatial
                && settings.spatial_scale.is_none()
                && settings.start_position.is_none()
                && settings.duration.is_none(),
            "session CD player {entity:?}: unsupported backend playback configuration"
        );
        let (paused, muted, volume, speed) = if let Some(sink) = sink {
            (
                sink.is_paused(),
                sink.is_muted(),
                sink.volume(),
                sink.speed(),
            )
        } else {
            (
                settings.paused,
                settings.muted,
                settings.volume,
                settings.speed,
            )
        };
        ensure!(
            !muted && volume.to_linear() == music.gain && speed == 1.0,
            "session CD player {entity:?}: backend gain/speed/mute differs from music owner"
        );
        players.push((transport, paused));
    }
    music.snapshot(&players)
}
