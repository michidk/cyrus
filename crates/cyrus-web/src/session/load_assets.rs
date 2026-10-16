//! Browser cache preparation without publishing a world or starting gameplay.
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::LoadState,
    prelude::{Asset, AssetServer, Assets, AudioSource, Handle, World},
};
use cyrus_assets::hud::HudAsset;
use cyrus_audio::{SoundBank, sound::dialogue::DialogueClip};
use cyrus_session::{RuntimeSessionSnapshot, SessionLoadAssets, SessionWorldAssets};
use std::collections::BTreeMap;

pub struct LoadAssets {
    sources: SessionWorldAssets,
    hud: Handle<HudAsset>,
    images: Option<cyrus_render::hud::HudPresentationAssets>,
    bank: Option<Handle<SoundBank>>,
    dialogue: BTreeMap<[u8; 4], Handle<DialogueClip>>,
    music: Option<Handle<AudioSource>>,
}

impl LoadAssets {
    pub fn request(snapshot: &RuntimeSessionSnapshot, server: &AssetServer) -> Result<Self> {
        let prepared = snapshot
            .world_ids()
            .map(|id| {
                (
                    id,
                    cyrus_world::lifecycle::PreparedWorld::from_cache(id.to_string(), server),
                )
            })
            .collect();
        let dialogue = snapshot
            .dialogue()
            .required_labels()
            .into_iter()
            .map(|label| {
                let key = label
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                (
                    label,
                    server.load(format!("cache://dialogue/{key}.dialogue")),
                )
            })
            .collect();
        Ok(Self {
            sources: SessionWorldAssets::request(snapshot, prepared, server)?,
            hud: server.load("cache://hud/overlay.hud"),
            images: None,
            bank: (snapshot.sample_playback().sources().len() != 0)
                .then(|| server.load("cache://sounds/bank.sounds")),
            dialogue,
            music: snapshot
                .music()
                .source_identity()
                .map(|_| {
                    snapshot
                        .music()
                        .track()
                        .context("session CD source has no track")
                        .map(|track| server.load(format!("cache://music/{track:02}.wav")))
                })
                .transpose()?,
        })
    }

    pub fn poll(
        &mut self,
        snapshot: &RuntimeSessionSnapshot,
        world: &World,
    ) -> Result<Option<SessionLoadAssets>> {
        let Some(hud) = loaded(world, &self.hud, "HUD")? else {
            return Ok(None);
        };
        if snapshot.has_hud_presentation() && self.images.is_none() {
            let server = world.resource::<AssetServer>();
            self.images = Some(cyrus_render::hud::HudPresentationAssets::request(
                hud, server,
            ));
        }
        let mut audio = Vec::new();
        if let Some(bank) = &self.bank {
            let Some(bank) = loaded(world, bank, "effect bank")? else {
                return Ok(None);
            };
            audio.extend(bank.audio.iter().cloned());
        }
        for (label, handle) in &self.dialogue {
            let Some(clip) = loaded(world, handle, &format!("dialogue {label:?}"))? else {
                return Ok(None);
            };
            if let Some(handle) = &clip.audio {
                audio.push(handle.clone());
            }
        }
        let mut samples = BTreeMap::new();
        for handle in audio {
            let Some(source) = loaded(world, &handle, "sample audio")? else {
                return Ok(None);
            };
            let identity = blake3::hash(&source.bytes).to_hex().to_string();
            samples.insert(identity, handle);
        }
        for source in snapshot.sample_playback().sources() {
            ensure!(
                samples.contains_key(source.source_identity),
                "session sample {}: no matching installed effect or retained dialogue source",
                source.source_identity
            );
        }
        Ok(Some(SessionLoadAssets {
            sources: self.sources.clone(),
            hud: self.hud.clone(),
            hud_images: self.images.clone(),
            samples,
            dialogue: self.dialogue.clone(),
            music: self.music.clone(),
        }))
    }
}

fn loaded<'a, A: Asset>(
    world: &'a World,
    handle: &Handle<A>,
    owner: &str,
) -> Result<Option<&'a A>> {
    if let Some(value) = world
        .get_resource::<Assets<A>>()
        .and_then(|assets| assets.get(handle))
    {
        return Ok(Some(value));
    }
    let server = world
        .get_resource::<AssetServer>()
        .context("session asset server absent")?;
    match server.get_load_state(handle.id()) {
        Some(LoadState::Failed(error)) => {
            anyhow::bail!("session {owner} {:?}: {error}", handle.path())
        }
        _ => Ok(None),
    }
}
