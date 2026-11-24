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
use cyrus_game::preferences::Preferences;
use cyrus_render::hud::HudPresentationAssets;
use cyrus_script::script::{LogicalSessionRestoreInputs, ScriptActors};
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
    #[cfg(feature = "original-save-import")]
    complete_original: Option<crate::OriginalRuntimeAdmission>,
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
            #[cfg(feature = "original-save-import")]
            complete_original: None,
            assets,
            environments: BTreeMap::new(),
            scene: None,
            entities: None,
            camera: None,
            private_environment: None,
        }
    }

    #[cfg(feature = "original-save-import")]
    pub(crate) fn from_original(
        admission: crate::OriginalRuntimeAdmission,
        assets: SessionLoadAssets,
    ) -> Self {
        let mut load = Self::new(admission.snapshot.clone(), assets);
        load.complete_original = Some(admission);
        load
    }

    pub(crate) fn is_complete_original(&self) -> bool {
        #[cfg(feature = "original-save-import")]
        {
            self.complete_original.is_some()
        }
        #[cfg(not(feature = "original-save-import"))]
        {
            false
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
        for source in self
            .snapshot
            .sample_playback()
            .sources()
            .filter(|_| !self.is_complete_original())
        {
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
        let labels = if self.is_complete_original() {
            self.snapshot.dialogue().required_labels()
        } else {
            self.snapshot.required_dialogue_labels()
        };
        for label in labels {
            let handle = self.assets.dialogue.get(&label).with_context(|| {
                format!("session load dialogue {label:?}: installed source absent")
            })?;
            let Some(clip) = loaded(world, handle, "dialogue")? else {
                return Ok(None);
            };
            if let Some(audio) = &clip.audio
                && loaded(world, audio, "dialogue audio")?.is_none()
            {
                return Ok(None);
            }
            clips.insert(label, clip.clone());
        }
        let music = match self
            .assets
            .music
            .as_ref()
            .filter(|_| !self.is_complete_original())
        {
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
                let server = crate::resource::<AssetServer>(world, "session load asset server")?;
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
                #[cfg(feature = "original-save-import")]
                let scene = if let Some(original) = &self.complete_original {
                    SessionSceneInstances::request_original(
                        &self.snapshot,
                        &prepared,
                        &original.recipes,
                        world,
                    )?
                } else {
                    SessionSceneInstances::request(&self.snapshot, &prepared, world)?
                };
                #[cfg(not(feature = "original-save-import"))]
                let scene = SessionSceneInstances::request(&self.snapshot, &prepared, world)?;
                self.scene = Some(scene);
                return Ok(None);
            }
            if !self.scene.as_ref().unwrap().ready(world)? {
                return Ok(None);
            }
        }
        #[cfg(feature = "original-save-import")]
        if let (Some(original), Some(scene)) = (&mut self.complete_original, &self.scene) {
            let (player, actors) = scene.original_models(&original.recipes, world)?;
            self.snapshot.player_model = player;
            self.snapshot.actor_models = actors;
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
        self.entities = Some(if self.is_complete_original() {
            SessionEntityStage::prepare_stopped(&self.snapshot, scene.as_ref(), world)?
        } else {
            SessionEntityStage::prepare(&self.snapshot, scene.as_ref(), world)?
        });
        let stage = self.entities.as_ref().unwrap();
        let sources = self
            .assets
            .sources
            .sources(world)?
            .context("session sources disappeared")?;
        let hud = loaded(world, &self.assets.hud, "HUD")?.context("session HUD disappeared")?;
        let inputs = LogicalSessionRestoreInputs {
            sources: &sources,
            last_generation: crate::resource::<ScriptActors>(world, "session load registry")?
                .generation_high_water(),
            hud,
            preferences: crate::resource::<Preferences>(world, "session load preferences")?,
            bios_tick: cyrus_script::clock::bios_tick(
                crate::resource::<Time<Real>>(world, "session load real clock")?.elapsed(),
            ),
        };
        #[cfg(feature = "original-save-import")]
        let plan = if let Some(mut original) = self.complete_original.take() {
            original.snapshot = self.snapshot.clone();
            original.plan(inputs)?
        } else {
            self.snapshot.clone().plan(inputs)?
        };
        #[cfg(not(feature = "original-save-import"))]
        let plan = self.snapshot.clone().plan(inputs)?;
        let audio = self
            .assets
            .samples
            .iter()
            .filter(|_| !plan.is_original())
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
                    server: crate::resource::<AssetServer>(world, "session load asset server")?,
                    audio_sources: crate::resource::<Assets<AudioSource>>(
                        world,
                        "session load audio assets",
                    )?,
                    loaded: crate::resource::<Assets<DialogueClip>>(
                        world,
                        "session load dialogue assets",
                    )?,
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
                actor_model_roots: &self
                    .scene
                    .as_ref()
                    .map(SessionSceneInstances::actor_models)
                    .cloned()
                    .unwrap_or_default(),
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
        if let Some(camera) = self.camera
            && world
                .get::<crate::staging::StagedSessionEntity>(camera)
                .is_some()
        {
            world.despawn(camera);
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
