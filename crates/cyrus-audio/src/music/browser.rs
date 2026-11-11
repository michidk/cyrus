//! Retained browser CD buffers, independent of main-thread streaming callbacks.
use super::{
    MusicPlayer, MusicTransport,
    browser_owners::{self, Players},
};
use crate::sound::PannedSnapshot;
use anyhow::{Context, Result, ensure};
use bevy::{
    audio::AudioPlayer,
    prelude::{
        App, Assets, Entity, IntoScheduleConfigs, PlaybackSettings, Query, Res, ResMut, Update,
        With, World, error,
    },
};
use std::{io::Cursor, sync::atomic::Ordering};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/music/browser.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = createMusic)]
    fn create_music(
        left: &[f32],
        right: &[f32],
        rate: u32,
        gain: f32,
        paused: bool,
    ) -> Result<u32, JsValue>;
    #[wasm_bindgen(catch, js_name = prepareMusic)]
    fn prepare_music(left: &[f32], right: &[f32], rate: u32, gain: f32) -> Result<u32, JsValue>;
    #[wasm_bindgen(catch, js_name = publishMusic)]
    fn publish_music(id: u32, retiring: &[u32], paused: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = updateMusic)]
    fn update_music(id: u32, gain: f32, paused: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = musicFrames)]
    fn music_frames(id: u32) -> Result<f64, JsValue>;
    #[wasm_bindgen(js_name = stopMusic)]
    fn stop_music(id: u32);
}

pub(super) fn register(app: &mut App) {
    browser_owners::register(app, stop_music);
    app.add_systems(Update, sync.after(super::pause_track));
}

fn sync(
    mut players: ResMut<Players>,
    transports: Res<Assets<MusicTransport>>,
    query: Query<(Entity, &AudioPlayer<MusicTransport>, &PlaybackSettings), With<MusicPlayer>>,
) {
    for (entity, player, settings) in &query {
        let Some(transport) = transports.get(&player.0) else {
            continue;
        };
        let result = (|| -> Result<()> {
            if let Some(id) = players.ids.get(&entity) {
                update_music(*id, settings.volume.to_linear(), settings.paused)
                    .map_err(|e| anyhow::anyhow!("updating WebAudio CD player: {e:?}"))?;
            } else {
                ensure!(
                    transport.browser_clock().load(Ordering::Relaxed) == 0,
                    "CD transport already has a browser consumer"
                );
                let initial = transport.initial_snapshot()?;
                let buffer = super::checkpoint::cd_buffer(transport.source(), &initial)?;
                let id = create_music(
                    &buffer.left,
                    &buffer.right,
                    buffer.rate,
                    settings.volume.to_linear(),
                    settings.paused,
                )
                .map_err(|e| anyhow::anyhow!("starting WebAudio CD player: {e:?}"))?;
                transport.browser_clock().store(id, Ordering::Relaxed);
                players.ids.insert(entity, id);
            }
            Ok(())
        })();
        if let Err(error) = result {
            error!("CD player {entity:?}: {error:#}");
        }
    }
}

pub(super) fn snapshot(
    source: &bevy::audio::AudioSource,
    initial: PannedSnapshot,
    id: u32,
) -> Result<PannedSnapshot> {
    let reader = hound::WavReader::new(Cursor::new(&*source.bytes))
        .context("browser CD checkpoint WAV metadata")?;
    ensure!(
        reader.spec().channels == 2,
        "browser CD checkpoint requires stereo PCM"
    );
    let samples = u64::from(reader.len());
    let frames =
        music_frames(id).map_err(|e| anyhow::anyhow!("reading WebAudio CD progress: {e:?}"))?;
    ensure!(
        frames.is_finite()
            && frames >= 0.0
            && frames.fract() == 0.0
            && frames < (samples / 2) as f64,
        "browser CD render cursor is invalid"
    );
    Ok(initial
        .clone()
        .at_stereo_input_sample((initial.input_samples() + frames as u64 * 2) % samples))
}

pub(super) fn prepare(transport: &MusicTransport, settings: &PlaybackSettings) -> Result<u32> {
    let initial = transport.initial_snapshot()?;
    let buffer = super::checkpoint::cd_buffer(transport.source(), &initial)?;
    prepare_music(
        &buffer.left,
        &buffer.right,
        buffer.rate,
        settings.volume.to_linear(),
    )
    .map_err(|error| anyhow::anyhow!("preparing WebAudio CD output: {error:?}"))
}

pub(super) fn discard(id: u32) {
    stop_music(id);
}

pub(super) fn publish(
    transport: &MusicTransport,
    world: &mut World,
    entity: Entity,
    settings: &PlaybackSettings,
) -> Result<()> {
    let id = transport.prepared.load(Ordering::Relaxed);
    ensure!(id != 0, "session CD output was not prepared");
    let mut players = world
        .get_resource_mut::<Players>()
        .context("session CD browser owners absent")?;
    let retiring: Vec<_> = players.ids.values().copied().collect();
    publish_music(id, &retiring, settings.paused)
        .map_err(|error| anyhow::anyhow!("publishing WebAudio CD output: {error:?}"))?;
    players.ids.clear();
    players.ids.insert(entity, id);
    transport.prepared.store(0, Ordering::Relaxed);
    transport.browser_clock().store(id, Ordering::Relaxed);
    Ok(())
}
