//! Frame-spanning hidden preparation before one exclusive session publication.
use crate::{
    PlayerReplacement, RuntimeSessionCandidate, RuntimeSessionSnapshot, SessionEntityStage,
    SessionSceneInstances, SessionServices, SessionWorldAssets,
};
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::LoadState,
    prelude::{
        Asset, AssetServer, Assets, AudioSource, Camera, Camera3d, Entity, Handle, Real, Time,
        World, default,
    },
};
use cyrus_assets::{environment::Environment, hud::HudAsset};
use cyrus_audio::sound::{RestoredDialogueAssets, dialogue::DialogueClip};
use cyrus_game::{
    preferences::Preferences,
    script::{LogicalSessionRestoreInputs, ScriptActors},
};
use cyrus_render::hud::HudPresentationAssets;
use std::collections::BTreeMap;

/// Host-resolved installed assets. Handles may still be loading. Paths and export
/// work belong to native/browser hosts; source compatibility is validated here.
#[derive(Clone)]
pub struct SessionLoadAssets {
    pub sources: SessionWorldAssets,
    pub hud: Handle<HudAsset>,
    pub hud_images: Option<HudPresentationAssets>,
    pub samples: BTreeMap<String, Handle<AudioSource>>,
    pub dialogue: BTreeMap<[u8; 4], Handle<DialogueClip>>,
    pub music: Option<Handle<AudioSource>>,
}

pub(crate) struct SessionLoad {
    snapshot: RuntimeSessionSnapshot,
    assets: SessionLoadAssets,
    environments: BTreeMap<u32, Handle<Environment>>,
    scene: Option<SessionSceneInstances>,
    entities: Option<SessionEntityStage>,
    camera: Option<Entity>,
    private_environment: Option<Handle<Environment>>,
}

impl SessionLoad {
    pub(crate) fn new(snapshot: RuntimeSessionSnapshot, assets: SessionLoadAssets) -> Self {
        Self {
            snapshot,
            assets,
            environments: BTreeMap::new(),
            scene: None,
            entities: None,
            camera: None,
            private_environment: None,
        }
    }

    /// Pending loads leave the current session running. All replacements are built
    /// and committed at the same exclusive boundary, using fresh allocation epochs.
    pub(crate) fn poll(&mut self, world: &mut World) -> Result<Option<RuntimeSessionCandidate>> {
        if self.assets.sources.sources(world)?.is_none() {
            return Ok(None);
        }
        ensure!(
            self.snapshot.has_hud_presentation() == self.assets.hud_images.is_some(),
            "session load HUD image census differs"
        );
        if loaded(world, &self.assets.hud, "HUD")?.is_none() {
            return Ok(None);
        }
        if let Some(images) = &self.assets.hud_images {
            for handle in images
                .candle
                .iter()
                .chain(&images.compass)
                .chain(&images.logbook)
                .chain(&images.powerup)
            {
                if loaded(world, handle, "HUD image")?.is_none() {
                    return Ok(None);
                }
            }
        }
        for source in self.snapshot.sample_playback().sources() {
            let handle = self
                .assets
                .samples
                .get(source.source_identity)
                .with_context(|| {
                    format!(
                        "session load sample {}: installed source absent",
                        source.source_identity
                    )
                })?;
            if loaded(world, handle, "sample audio")?.is_none() {
                return Ok(None);
            }
        }
        let mut clips = BTreeMap::new();
        for label in self.snapshot.dialogue().required_labels() {
            let handle = self.assets.dialogue.get(&label).with_context(|| {
                format!("session load dialogue {label:?}: installed source absent")
            })?;
            let Some(clip) = loaded(world, handle, "dialogue")? else {
                return Ok(None);
            };
            if let Some(audio) = &clip.audio {
                if loaded(world, audio, "dialogue audio")?.is_none() {
                    return Ok(None);
                }
            }
            clips.insert(label, clip.clone());
        }
        let music = match &self.assets.music {
            Some(handle) => {
                let Some(source) = loaded(world, handle, "CD audio")? else {
                    return Ok(None);
                };
                Some(source.clone())
            }
            None => None,
        };
        let prepared = self.assets.sources.current().0.clone();
        if self.snapshot.sky.is_some() {
            if self.environments.is_empty() {
                let server = world.resource::<AssetServer>();
                self.environments = self
                    .assets
                    .sources
                    .prepared_worlds()
                    .map(|(id, prepared)| (id, server.load(prepared.environment_path.clone())))
                    .collect();
            }
            for (id, handle) in &self.environments {
                if loaded(world, handle, &format!("world {id} environment"))?.is_none() {
                    return Ok(None);
                }
            }
            if self.scene.is_none() {
                self.scene = Some(SessionSceneInstances::request(
                    &self.snapshot,
                    &prepared,
                    world,
                )?);
                return Ok(None);
            }
            if !self.scene.as_ref().unwrap().ready(world)? {
                return Ok(None);
            }
        }
        let installed = self
            .environments
            .iter()
            .map(|(&id, handle)| {
                Ok((
                    id,
                    copy_environment(
                        loaded(world, handle, "environment")?
                            .context("session environment disappeared")?,
                    )?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let environments = self.snapshot.prepare_environments(installed)?;
        let references = environments.iter().map(|(&id, env)| (id, env)).collect();
        let (scene, playback) = if let Some(stage) = &mut self.scene {
            let edits = self
                .snapshot
                .environment_edits()
                .context("session environment edits absent")?
                .clone()
                .restore(&references)?;
            let environment = world
                .resource_mut::<Assets<Environment>>()
                .add(copy_environment(
                    environments
                        .get(&self.snapshot.current_world())
                        .context("session current environment absent")?,
                )?);
            self.private_environment = Some(environment.clone());
            let (scene, playback) =
                stage.prepare_rendered(&self.snapshot, &prepared, &environment, &edits, world)?;
            (Some(scene), Some(playback))
        } else {
            (None, None)
        };
        if self.snapshot.camera.is_some() && self.scene.is_none() {
            self.camera = Some(
                world
                    .spawn((
                        crate::staging::StagedSessionEntity,
                        Camera3d::default(),
                        Camera {
                            is_active: false,
                            ..default()
                        },
                    ))
                    .id(),
            );
        }
        self.entities = Some(SessionEntityStage::prepare(
            &self.snapshot,
            scene.as_ref(),
            world,
        )?);
        let stage = self.entities.as_ref().unwrap();
        let sources = self
            .assets
            .sources
            .sources(world)?
            .context("session sources disappeared")?;
        let hud = loaded(world, &self.assets.hud, "HUD")?.context("session HUD disappeared")?;
        let plan = self.snapshot.clone().plan(LogicalSessionRestoreInputs {
            sources: &sources,
            last_generation: world.resource::<ScriptActors>().generation_high_water(),
            hud,
            preferences: world.resource::<Preferences>(),
            bios_tick: cyrus_game::clock::bios_tick(world.resource::<Time<Real>>().elapsed()),
        })?;
        let audio = self
            .assets
            .samples
            .iter()
            .map(|(identity, handle)| {
                Ok((
                    identity.clone(),
                    loaded(world, handle, "sample audio")?
                        .context("session sample disappeared")?
                        .clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let candidate = plan.finish(
            PlayerReplacement {
                entity: stage.player(),
                scene: self.assets.sources.current().1.clone(),
                animation: prepared.animations.clone(),
            },
            SessionServices {
                sources: self.assets.sources.clone(),
                scene,
                camera_entity: self
                    .scene
                    .as_ref()
                    .map(SessionSceneInstances::camera)
                    .or(self.camera),
                entities: stage.bindings(),
                source_audio: &audio,
                dialogue: RestoredDialogueAssets {
                    server: world.resource::<AssetServer>(),
                    audio_sources: world.resource::<Assets<AudioSource>>(),
                    loaded: world.resource::<Assets<DialogueClip>>(),
                    clips: &clips,
                    requests: &self.assets.dialogue,
                    hud: Some(self.assets.hud.clone()),
                },
                music_source: music,
                hud_asset: hud,
                hud_images: self.assets.hud_images.as_ref(),
                sky_images: self
                    .scene
                    .as_ref()
                    .and_then(SessionSceneInstances::sky_images),
                render_playback: playback,
                player_model_root: self.scene.as_ref().and_then(SessionSceneInstances::model),
                environments: &references,
            },
            world,
        )?;
        Ok(Some(candidate))
    }

    pub(crate) fn discard(self, world: &mut World) {
        if let Some(entities) = self.entities {
            entities.discard(world);
        }
        if let Some(scene) = self.scene {
            scene.discard(world);
        }
        if let Some(camera) = self.camera {
            if world
                .get::<crate::staging::StagedSessionEntity>(camera)
                .is_some()
            {
                world.despawn(camera);
            }
        }
    }
}

fn loaded<'a, T: Asset>(
    world: &'a World,
    handle: &Handle<T>,
    owner: &str,
) -> Result<Option<&'a T>> {
    let server = world
        .get_resource::<AssetServer>()
        .context("session load asset server absent")?;
    if let Some(LoadState::Failed(error)) = server.get_load_state(handle.id()) {
        anyhow::bail!("session load {owner}: {error}");
    }
    Ok(world
        .get_resource::<Assets<T>>()
        .with_context(|| format!("session load {owner}: asset storage absent"))?
        .get(handle))
}

fn copy_environment(source: &Environment) -> Result<Environment> {
    Ok(serde_json::from_slice(&serde_json::to_vec(source)?)?)
}
