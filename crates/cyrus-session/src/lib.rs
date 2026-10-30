//! Shared off-live gameplay and audio assembly; native/browser storage stays host-owned.
mod boundary;
pub use boundary::{
    SessionBoundary, SessionBoundaryInspection, SessionBoundaryPlugin, SessionBoundaryResult,
    SessionBoundarySnapshot, SessionStagedAsset,
};
mod document;
mod entities;
mod lifecycle;
mod loading;
pub use entities::{EntityBindingCandidate, EntityBindingSnapshot};
pub use loading::SessionLoadAssets;
mod music;
mod publication;
mod render_playback;
mod sources;
mod staging;
pub use staging::SessionEntityStage;
mod scene_instances;
use anyhow::{Context, Result, ensure};
use bevy::audio::AudioSource;
use bevy::prelude::{Assets, Entity, Handle, State, Transform, With, World};
use cyrus_assets::hud::HudAsset;
use cyrus_audio::{
    music::{Music, MusicCandidate, MusicSnapshot},
    persistence::AudioRestoreBindings,
    sound::{
        RestoredDialogueAssets, RuntimeDialogue, RuntimeDialogueSnapshot,
        persistence::{SamplePlaybackCandidate, SamplePlaybackSnapshot},
    },
};
use cyrus_game::{
    AppState,
    clock::SimulationClock,
    player::{
        Player,
        data::PlayerSceneRequest,
        input::PlayerToggles,
        placement::{
            MarkerEntry, PlayerEntrySupport, PlayerPlacementCandidate, PlayerPlacementSnapshot,
            PlayerPose,
        },
        runtime::{PlayerRuntime, PlayerRuntimeBinding, PlayerRuntimeSnapshot},
    },
    random::EngineRandom,
    script::{
        LogicalSessionCandidate, LogicalSessionRestoreInputs, LogicalSessionRestorePlan,
        LogicalSessionSnapshot, ScriptActorId, ScriptActors, ScriptBinding, ScriptHud,
        WorldSnapshotSources,
    },
};
use cyrus_render::camera::{CameraCandidate, CameraSnapshot};
use cyrus_render::hud::{HudPresentationAssets, HudPresentationSnapshot, HudState};
use cyrus_render::persistence::{
    EnvironmentEditsCandidate, EnvironmentEditsSnapshot, PresentationClock,
    PresentationClockSnapshot, SkyCandidate, SkyImages, SkySnapshot,
};
use cyrus_world::animation::{AnimationRequest, Animations};
pub use lifecycle::LifecycleCandidate;
pub use render_playback::{PreparedRenderPlayback, RenderPlaybackCandidate};
pub use scene_instances::SessionSceneInstances;
use serde::{Deserialize, Serialize};
pub use sources::{SessionSourceCatalog, SessionWorldAssets};
use std::collections::BTreeMap;

/// Composed runtime owners with off-live preparation and exclusive publication.
/// Complete host lifecycle/revisit acceptance remains required before exposing
/// saves. This is not an original SAV codec.
#[derive(Clone, Serialize, Deserialize)]
pub struct RuntimeSessionSnapshot {
    version: u32,
    lifecycle: lifecycle::LifecycleSnapshot,
    logical: LogicalSessionSnapshot,
    player: PlayerSnapshot,
    entity_bindings: Vec<EntityBindingSnapshot>,
    samples: SamplePlaybackSnapshot,
    dialogue: RuntimeDialogueSnapshot,
    music: MusicSnapshot,
    cheats: Option<cyrus_game::cheats::CheatSnapshot>,
    #[serde(default)]
    movie_unlocks: Option<u16>,
    hud: Option<HudPresentationSnapshot>,
    fidelity: Option<cyrus_game::fidelity::Fidelity>,
    presentation_clock: Option<PresentationClockSnapshot>,
    environment_edits: Option<EnvironmentEditsSnapshot>,
    camera: Option<CameraSnapshot>,
    sky: Option<SkySnapshot>,
    render_playback: Option<render_playback::RenderPlaybackSnapshot>,
    player_model: Option<cyrus_render::player_model::PlayerModelSnapshot>,
    #[serde(default)]
    actor_models: BTreeMap<u64, cyrus_render::player_model::PlayerModelSnapshot>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PlayerSnapshot {
    entity: u64,
    actor: (u64, usize),
    placement: PlayerPlacementSnapshot,
    runtime: PlayerRuntimeSnapshot,
}

/// Validated logical identity reservation before host asset/entity preparation.
pub struct RuntimeSessionPlan<'a> {
    source_identities: BTreeMap<u32, String>,
    lifecycle: lifecycle::LifecycleSnapshot,
    logical: LogicalSessionRestorePlan<'a>,
    player: PlayerSnapshot,
    entity_bindings: Vec<EntityBindingSnapshot>,
    samples: SamplePlaybackSnapshot,
    dialogue: RuntimeDialogueSnapshot,
    music: MusicSnapshot,
    cheats: Option<cyrus_game::cheats::CheatSnapshot>,
    movie_unlocks: Option<u16>,
    hud: Option<HudPresentationSnapshot>,
    fidelity: Option<cyrus_game::fidelity::Fidelity>,
    presentation_clock: Option<PresentationClockSnapshot>,
    environment_edits: Option<EnvironmentEditsSnapshot>,
    camera: Option<CameraSnapshot>,
    sky: Option<SkySnapshot>,
    render_playback: Option<render_playback::RenderPlaybackSnapshot>,
    player_model: Option<cyrus_render::player_model::PlayerModelSnapshot>,
    actor_models: BTreeMap<u64, cyrus_render::player_model::PlayerModelSnapshot>,
    world: u32,
}

/// Prepared player assets and fresh entity chosen by the aggregate host.
pub struct PlayerReplacement {
    pub entity: Entity,
    pub scene: Handle<cyrus_game::player::data::PlayerScene>,
    pub animation: Handle<Animations>,
}

/// Already prepared service inputs; construction does not load assets or change owners.
pub struct SessionServices<'a> {
    pub sources: SessionWorldAssets,
    pub scene: Option<cyrus_render::persistence::SceneCandidate>,
    /// Explicit prepared camera target, including camera-only inspection sessions.
    pub camera_entity: Option<Entity>,
    pub entities: &'a [(u64, Entity)],
    pub source_audio: &'a BTreeMap<String, AudioSource>,
    pub dialogue: RestoredDialogueAssets<'a>,
    pub music_source: Option<AudioSource>,
    pub hud_asset: &'a HudAsset,
    /// Present exactly when the captured session had a rendered HUD.
    pub hud_images: Option<&'a HudPresentationAssets>,
    pub sky_images: Option<SkyImages>,
    pub render_playback: Option<PreparedRenderPlayback>,
    pub player_model_root: Option<Entity>,
    /// Saved actor entity to independently prepared model root.
    pub actor_model_roots: &'a BTreeMap<u64, Entity>,
    pub environments: &'a BTreeMap<u32, &'a cyrus_assets::environment::Environment>,
}

/// Constructed without live mutation. The complete session commit must consume all
/// fields together with rendered presentation and host lifecycle replacements.
pub struct RuntimeSessionCandidate {
    sources: SessionWorldAssets,
    source_identities: BTreeMap<u32, String>,
    lifecycle: LifecycleCandidate,
    scene: Option<cyrus_render::persistence::SceneCandidate>,
    /// Explicit prepared camera target, including camera-only inspection sessions.
    camera_entity: Option<Entity>,
    logical: LogicalSessionCandidate,
    player: PlayerReplacement,
    entity_bindings: Vec<EntityBindingCandidate>,
    placement: PlayerPlacementCandidate,
    runtime: PlayerRuntime,
    samples: SamplePlaybackCandidate,
    dialogue: RuntimeDialogue,
    music: MusicCandidate,
    cheats: Option<cyrus_game::cheats::CheatReplacement>,
    movie_unlocks: Option<cyrus_game::MovieUnlocks>,
    hud: Option<HudState>,
    fidelity: Option<cyrus_game::fidelity::Fidelity>,
    presentation_clock: Option<PresentationClock>,
    environment_edits: Option<EnvironmentEditsCandidate>,
    camera: Option<CameraCandidate>,
    sky: Option<SkyCandidate>,
    render_playback: Option<RenderPlaybackCandidate>,
    player_model: Option<cyrus_render::player_model::PlayerModelCandidate>,
    actor_models: Vec<cyrus_render::player_model::PlayerModelCandidate>,
    marker: MarkerEntry,
}

impl RuntimeSessionCandidate {
    /// Recheck identities and prepared targets at the exclusive publication boundary.
    /// This performs no mutation; the caller must not run systems between this check
    /// and the eventual complete commit.
    pub fn validate_publication(&self, world: &World) -> Result<()> {
        ensure!(
            !world.contains_resource::<cyrus_world::lifecycle::WorldPreparationPending>(),
            "session publication: world preparation is in flight"
        );
        sources::validate_retained(&self.sources, &self.source_identities, world)?;
        validate_reader_boundary(world)?;
        ensure!(
            self.cheats.is_some() == world.contains_resource::<cyrus_game::cheats::Cheats>(),
            "session publication cheat owner census differs"
        );
        if let Some(cheats) = &self.cheats {
            cheats.validate(resource::<cyrus_game::cheats::Cheats>(world, "cheats")?)?;
        }
        let state = resource::<State<AppState>>(world, "application state")?.get();
        ensure!(
            matches!(state, AppState::InGame | AppState::Menu),
            "session publication requires gameplay or a prepared menu load"
        );
        if state == &AppState::Menu {
            ensure!(
                matches!(
                    resource::<bevy::prelude::NextState<AppState>>(
                        world,
                        "application transition"
                    )?,
                    bevy::prelude::NextState::Unchanged
                ),
                "session publication conflicts with a pending application transition"
            );
        }
        // The menu has no HUD of its own; a saved HUD is published with the load.
        ensure!(
            self.hud.is_some() == world.contains_resource::<HudState>()
                || (state == &AppState::Menu && self.hud.is_some()),
            "session publication HUD host census differs"
        );
        ensure!(
            self.camera.is_some() == self.camera_entity.is_some(),
            "session publication camera target census differs"
        );
        ensure!(
            self.sky.is_some() == self.scene.is_some(),
            "session publication scene/sky census differs"
        );
        if let Some(entity) = self.camera_entity {
            entities::validate_retirement_ancestry(world, entity)?;
            ensure!(
                world.get::<bevy::prelude::Camera3d>(entity).is_some()
                    && world
                        .get::<bevy::prelude::Camera>(entity)
                        .is_some_and(|camera| !camera.is_active)
                    && world
                        .get::<cyrus_render::camera::ViewCamera>(entity)
                        .is_none(),
                "session publication camera target disappeared"
            );
        }
        ensure!(
            self.fidelity.is_some() == world.contains_resource::<cyrus_game::fidelity::Fidelity>(),
            "session publication fidelity host census differs"
        );
        let live = resource::<ScriptActors>(world, "live script actors")?;
        let high_water = live.generation_high_water();
        let current = self
            .logical
            .actors
            .current
            .as_ref()
            .context("session publication current world absent")?;
        for actor_world in
            std::iter::once(current).chain(self.logical.actors.visited_worlds().values())
        {
            ensure!(
                actor_world.generation() > high_water,
                "session publication actor allocation advanced after preparation"
            );
        }
        let mut targets = std::collections::BTreeSet::new();
        for binding in &self.entity_bindings {
            ensure!(
                targets.insert(binding.entity),
                "session publication entity targets alias"
            );
            binding.validate_publication(world)?;
        }
        ensure!(
            targets.contains(&self.player.entity),
            "session publication player target absent from entity census"
        );
        self.samples.validate_publication(world)?;
        let mut model_roots = std::collections::BTreeSet::new();
        for model in self.player_model.iter().chain(&self.actor_models) {
            ensure!(
                model_roots.insert(model.root()),
                "session model roots alias"
            );
            model.validate(world)?;
            entities::validate_retirement_ancestry(world, model.root())?;
        }

        ensure!(
            self.render_playback.is_some() == self.scene.is_some(),
            "session publication render playback/scene census differs"
        );
        if let Some(scene) = &self.scene {
            scene.validate(world)?;
            if let Some(playback) = &self.render_playback {
                playback.validate(world, scene)?;
            }
            entities::validate_retirement_ancestry(world, scene.root())?;
            entities::validate_retirement_ancestry(world, scene.sky())?;
            for binding in &self.entity_bindings {
                if binding.is_map() {
                    scene.validate_map_entity(world, binding.entity)?;
                }
            }
        }
        ensure!(
            self.music.playback.is_none()
                || world.contains_resource::<Assets<cyrus_audio::music::MusicTransport>>(),
            "session publication CD transport assets absent"
        );
        Ok(())
    }
}

fn validate_reader_boundary(world: &World) -> Result<()> {
    ensure!(
        !world
            .get_resource::<cyrus_game::logbook::LogbookScreen>()
            .is_some_and(|screen| screen.suspended()),
        "session boundary: logbook reader or closing-key release is active; reader continuation is not represented"
    );
    Ok(())
}

fn resource<'a, T: bevy::prelude::Resource>(world: &'a World, label: &str) -> Result<&'a T> {
    world
        .get_resource::<T>()
        .with_context(|| format!("session capture: {label} absent"))
}

impl RuntimeSessionSnapshot {
    /// Capture a completed ordinary boundary. The host drains cues/deferred commands
    /// and establishes its audio backend checkpoint policy before this call.
    pub fn capture(
        world: &World,
        sources: &BTreeMap<u32, WorldSnapshotSources<'_>>,
        hud_asset: &HudAsset,
    ) -> Result<Self> {
        validate_reader_boundary(world)?;
        let state = resource::<State<AppState>>(world, "application state")?.get();
        let paused = world.get_resource::<cyrus_game::PausedGameplay>();
        ensure!(
            state == &AppState::InGame || (state == &AppState::Menu && paused.is_some()),
            "session capture requires active gameplay or an eligible paused book"
        );
        ensure!(
            world
                .try_query_filtered::<Entity, With<cyrus_game::script::ScriptedPlayback>>()
                .is_none_or(|mut query| query.iter(world).next().is_none()),
            "session capture: explicit scripted playback has no replacement owner"
        );
        ensure!(
            world
                .try_query_filtered::<Entity, With<Player>>()
                .map_or(0, |mut query| query.iter(world).count())
                == 1,
            "session capture requires exactly one player entity"
        );
        let mut query = world
            .try_query_filtered::<(
                Entity,
                &PlayerPose,
                &PlayerEntrySupport,
                &Transform,
                &ScriptBinding,
            ), With<Player>>()
            .context("session player components absent")?;
        let (entity, pose, support, transform, binding) = query
            .single(world)
            .context("session capture requires one bound player with completed placement")?;
        let actors = resource::<ScriptActors>(world, "script actors")?;
        let current = actors
            .current
            .as_ref()
            .context("session capture: current logical world absent")?;
        ensure!(
            current.generation() == binding.actor.generation
                && current.selected_player() == Some(binding.actor),
            "session capture: presentation player is not the current logical player"
        );
        let scene = resource::<PlayerSceneRequest>(world, "player source")?;
        let animation = resource::<AnimationRequest>(world, "animation source")?;
        ensure!(
            binding.source == animation.0,
            "session capture: player animation binding differs"
        );
        for id in std::iter::once(current.world).chain(actors.visited_worlds().keys().copied()) {
            ensure!(
                sources
                    .get(&id)
                    .is_some_and(|source| source.player.is_some()),
                "session capture world {id}: complete decoded player source absent"
            );
        }
        let source = sources
            .get(&current.world)
            .context("session current source absent")?;
        validate_player_assets(
            world,
            &scene.0,
            &animation.0,
            source
                .player
                .context("session player source absent")?
                .source_identity
                .as_deref()
                .context("session player source has no decoded identity")?,
            source.animations,
        )?;
        let lifecycle =
            lifecycle::LifecycleSnapshot::capture(world, current.world, source.scripts)?;
        let logical = actors.snapshot_session(
            sources,
            resource::<ScriptHud>(world, "script HUD")?,
            match paused {
                Some(paused) => &paused.clock,
                None => resource::<SimulationClock>(world, "clock")?,
            },
            resource::<EngineRandom>(world, "random")?,
            *resource::<PlayerToggles>(world, "player toggles")?,
        )?;
        let player = PlayerSnapshot {
            entity: entity.to_bits(),
            actor: (binding.actor.generation, binding.actor.index),
            placement: PlayerPlacementSnapshot::capture(
                current.world,
                *resource::<MarkerEntry>(world, "marker selection")?,
                pose,
                support,
                transform,
            )?,
            runtime: resource::<PlayerRuntime>(world, "player runtime")?
                .snapshot(&scene.0, &animation.0)?,
        };
        let dialogue = resource::<RuntimeDialogue>(world, "dialogue services")?.snapshot(
            resource::<bevy::prelude::AssetServer>(world, "asset server")?,
            resource::<Assets<AudioSource>>(world, "audio source assets")?,
        )?;
        let samples = SamplePlaybackSnapshot::capture(world)?;
        let music = music::capture(world)?;
        let entity_bindings = entities::capture(world)?;
        let bound: std::collections::BTreeSet<_> = entity_bindings
            .iter()
            .map(EntityBindingSnapshot::saved_entity)
            .collect();
        ensure!(
            samples.entity_owners().is_subset(&bound),
            "session audio entity owner lacks a retained actor binding"
        );
        let hud = world
            .get_resource::<HudState>()
            .map(|state| state.snapshot(hud_asset))
            .transpose()?;
        let environment_edits = world
            .contains_resource::<cyrus_render::environment::EnvironmentRequest>()
            .then(|| EnvironmentEditsSnapshot::capture(world))
            .transpose()?;
        let sky = environment_edits
            .as_ref()
            .map(|_| SkySnapshot::capture(world))
            .transpose()?;
        let player_model = if environment_edits.is_some() {
            cyrus_render::player_model::PlayerModelSnapshot::capture(world, entity)?
        } else {
            None
        };
        let mut actor_models = BTreeMap::new();
        if environment_edits.is_some() {
            for binding in &entity_bindings {
                if binding.saved_entity() == entity.to_bits() {
                    continue;
                }
                if let Some(model) = cyrus_render::player_model::PlayerModelSnapshot::capture(
                    world,
                    Entity::from_bits(binding.saved_entity()),
                )? {
                    actor_models.insert(binding.saved_entity(), model);
                }
            }
        }
        let render_playback = environment_edits
            .as_ref()
            .map(|_| {
                render_playback::RenderPlaybackSnapshot::capture(
                    world,
                    player_model
                        .as_ref()
                        .map(|model| Entity::from_bits(model.saved_root())),
                    actor_models
                        .iter()
                        .filter(|(entity, _)| {
                            entity_bindings.iter().any(|binding| {
                                binding.saved_entity() == **entity && binding.role() != "map"
                            })
                        })
                        .map(|(_, model)| Entity::from_bits(model.saved_root()))
                        .collect(),
                )
            })
            .transpose()?;
        let camera = CameraSnapshot::capture(world)?;
        let presentation_clock = if hud.is_some() || environment_edits.is_some() || camera.is_some()
        {
            Some(PresentationClock::snapshot(
                world.get_resource::<PresentationClock>(),
                resource::<bevy::prelude::Time<bevy::prelude::Real>>(
                    world,
                    "presentation host clock",
                )?,
            ))
        } else {
            None
        };
        Ok(Self {
            version: 1,
            movie_unlocks: world
                .get_resource::<cyrus_game::MovieUnlocks>()
                .map(|unlocks| unlocks.bits()),
            cheats: world
                .get_resource::<cyrus_game::cheats::Cheats>()
                .map(cyrus_game::cheats::Cheats::snapshot)
                .transpose()?,
            fidelity: world
                .get_resource::<cyrus_game::fidelity::Fidelity>()
                .cloned(),
            lifecycle,
            logical,
            player,
            entity_bindings,
            samples,
            dialogue,
            music,
            hud,
            presentation_clock,
            environment_edits,
            camera,
            sky,
            render_playback,
            player_model,
            actor_models,
        })
    }

    /// Installed world preparation hints; validation happens in plan/finish.
    pub fn world_ids(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.logical.world_ids()
    }
    pub fn current_world(&self) -> u32 {
        self.logical.current_world()
    }
    /// Saved installed scene selection. Hosts must resolve it against WORLD.INI
    /// and verify `current_world` before using it to prepare cache files.
    pub fn current_scene_name(&self) -> &str {
        &self.lifecycle.current_name
    }
    pub fn entity_bindings(&self) -> &[EntityBindingSnapshot] {
        &self.entity_bindings
    }

    /// Saved terrain ordinal hints for explicit fresh scene bindings.
    pub fn terrain_entities(&self) -> &[u64] {
        self.render_playback
            .as_ref()
            .map_or(&[], |state| state.entities())
    }

    /// Resolve saved terrain ordinals beneath an exact-source replacement scene.
    pub fn terrain_bindings(&self, world: &World, root: Entity) -> Result<Vec<Entity>> {
        self.render_playback
            .as_ref()
            .context("session has no rendered playback")?
            .terrain_bindings(world, root)
    }

    /// Validate the complete installed environment census and apply saved edits to
    /// owned off-live copies before preparing sky, materials and terrain.
    pub fn prepare_environments(
        &self,
        sources: BTreeMap<u32, cyrus_assets::environment::Environment>,
    ) -> Result<BTreeMap<u32, cyrus_assets::environment::Environment>> {
        let Some(saved) = &self.environment_edits else {
            ensure!(
                sources.is_empty(),
                "headless session has no environment replacements"
            );
            return Ok(sources);
        };
        ensure!(
            self.world_ids().collect::<std::collections::BTreeSet<_>>()
                == sources.keys().copied().collect(),
            "session installed environment census differs"
        );
        let references = sources.iter().map(|(&id, source)| (id, source)).collect();
        let candidate = saved.clone().restore(&references)?;
        sources
            .into_iter()
            .map(|(id, source)| {
                candidate
                    .prepare_environment(id, source)
                    .map(|source| (id, source))
            })
            .collect()
    }

    /// Saved edit validation for private scene-environment preparation.
    pub fn environment_edits(&self) -> Option<&EnvironmentEditsSnapshot> {
        self.environment_edits.as_ref()
    }

    pub fn player_model(&self) -> Option<&cyrus_render::player_model::PlayerModelSnapshot> {
        self.player_model.as_ref()
    }

    pub fn sample_playback(&self) -> &SamplePlaybackSnapshot {
        &self.samples
    }
    /// Installed CD preparation hints; reconstruction validates the exact source bytes.
    pub fn music(&self) -> &MusicSnapshot {
        &self.music
    }

    /// Whether restoration requires prepared HUD image handles.
    pub fn has_hud_presentation(&self) -> bool {
        self.hud.is_some()
    }

    pub fn dialogue(&self) -> &RuntimeDialogueSnapshot {
        &self.dialogue
    }

    /// Reserve complete logical identities before preparing service replacements.
    pub fn plan<'a>(
        self,
        inputs: LogicalSessionRestoreInputs<'a>,
    ) -> Result<RuntimeSessionPlan<'a>> {
        ensure!(
            self.version == 1,
            "runtime session snapshot version unsupported"
        );
        let world = self.logical.current_world();
        for id in self.logical.world_ids() {
            ensure!(
                inputs
                    .sources
                    .get(&id)
                    .is_some_and(|source| source.player.is_some()),
                "session restoration world {id}: complete decoded player source absent"
            );
        }
        for (&id, source) in inputs.sources {
            ensure!(
                source.player.is_some(),
                "session source world {id}: player absent"
            );
        }
        let logical = self.logical.plan(inputs)?;
        let source_identities = logical.source_identities()?;
        ensure!(
            logical
                .actor_bindings()
                .iter()
                .any(|(saved, _)| (saved.generation, saved.index) == self.player.actor),
            "session saved player actor absent from world census"
        );
        Ok(RuntimeSessionPlan {
            source_identities,
            lifecycle: self.lifecycle,
            logical,
            player: self.player,
            entity_bindings: self.entity_bindings,
            samples: self.samples,
            dialogue: self.dialogue,
            music: self.music,
            cheats: self.cheats,
            movie_unlocks: self.movie_unlocks,
            hud: self.hud,
            fidelity: self.fidelity,
            presentation_clock: self.presentation_clock,
            environment_edits: self.environment_edits,
            camera: self.camera,
            sky: self.sky,
            render_playback: self.render_playback,
            player_model: self.player_model,
            actor_models: self.actor_models,
            world,
        })
    }
}

impl RuntimeSessionPlan<'_> {
    pub fn actor_bindings(&self) -> &[(ScriptActorId, ScriptActorId)] {
        self.logical.actor_bindings()
    }
    pub fn saved_player(&self) -> u64 {
        self.player.entity
    }

    /// Every owner must be prepared before this constructor runs. It allocates no
    /// entities/assets, plays no audio and never replaces a live resource.
    pub fn finish(
        self,
        player: PlayerReplacement,
        services: SessionServices<'_>,
        live_world: &World,
    ) -> Result<RuntimeSessionCandidate> {
        let SessionServices {
            sources,
            scene,
            camera_entity,
            entities,
            source_audio,
            dialogue: dialogue_assets,
            music_source,
            hud_asset,
            hud_images,
            sky_images,
            render_playback: prepared_render_playback,
            player_model_root,
            actor_model_roots,
            environments,
        } = services;
        sources::validate_retained(&sources, &self.source_identities, live_world)?;
        let (prepared, retained_player) = sources.current();
        ensure!(
            *retained_player == player.scene,
            "session retained player handle differs from replacement"
        );
        let prepared = prepared.clone();
        ensure!(
            prepared.animations == player.animation,
            "session lifecycle/player animation bindings differ"
        );
        ensure!(
            scene.is_some() == self.sky.is_some(),
            "session prepared scene/sky census differs"
        );
        if let Some(scene) = &scene {
            ensure!(
                self.camera.is_some(),
                "session rendered scene camera absent"
            );
            ensure!(
                scene.matches_prepared(&prepared),
                "session prepared scene lifecycle differs"
            );
            scene.validate(live_world)?;
            scene.validate_environment(
                environments
                    .get(&self.world)
                    .context("session current environment absent")?,
            )?;
            let playback = prepared_render_playback
                .as_ref()
                .context("session rendered playback absent")?;
            ensure!(
                playback.textures.is_bound_to(scene.environment().id())
                    && playback.terrain.is_bound_to(scene.environment().id()),
                "session playback environment handles differ from prepared scene"
            );
        }
        ensure!(
            self.camera.is_some() == camera_entity.is_some(),
            "session camera/prepared target census differs"
        );
        if let Some(entity) = camera_entity {
            ensure!(
                live_world.get::<bevy::prelude::Camera3d>(entity).is_some()
                    && live_world
                        .get::<bevy::prelude::Camera>(entity)
                        .is_some_and(|camera| !camera.is_active)
                    && live_world
                        .get::<cyrus_render::camera::ViewCamera>(entity)
                        .is_none(),
                "session prepared camera target absent"
            );
            ensure!(
                scene.as_ref().is_none_or(|scene| scene.camera() == entity),
                "session camera target differs from prepared scene"
            );
        }
        let player_model = match (self.player_model, player_model_root) {
            (Some(saved), Some(root)) => {
                let pose = self
                    .entity_bindings
                    .iter()
                    .find(|binding| binding.saved_entity() == self.player.entity)
                    .and_then(EntityBindingSnapshot::displayed);
                ensure!(
                    pose == Some((saved.model(), saved.frame())),
                    "session player model/displayed pose differs"
                );
                Some(saved.prepare(&prepared, root, player.entity, live_world)?)
            }
            (None, None) => None,
            _ => anyhow::bail!("session player model/prepared root census differs"),
        };
        let expected_models: std::collections::BTreeSet<_> = self
            .entity_bindings
            .iter()
            .filter(|binding| scene.is_some() && binding.saved_entity() != self.player.entity)
            .filter(|binding| {
                binding.displayed().is_some_and(|(model, _)| {
                    binding.role() != "map"
                        || model != entities::map_node_name(binding.actor().index, binding.name())
                })
            })
            .map(EntityBindingSnapshot::saved_entity)
            .collect();
        ensure!(
            self.actor_models
                .keys()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                == expected_models,
            "session displayed actor model census differs"
        );
        ensure!(
            self.actor_models.keys().eq(actor_model_roots.keys()),
            "session actor model/prepared root census differs"
        );
        let mut actor_models = Vec::new();
        for (saved_entity, saved) in self.actor_models {
            let binding = self
                .entity_bindings
                .iter()
                .find(|binding| binding.saved_entity() == saved_entity)
                .context("session actor model has no entity binding")?;
            ensure!(
                saved_entity != self.player.entity
                    && binding.displayed() == Some((saved.model(), saved.frame())),
                "session actor model/displayed pose differs"
            );
            let entity = entities
                .iter()
                .find(|(id, _)| *id == saved_entity)
                .map(|(_, entity)| *entity)
                .context("session actor model target absent")?;
            actor_models.push(saved.prepare(
                &prepared,
                actor_model_roots[&saved_entity],
                entity,
                live_world,
            )?);
        }
        let lifecycle = self.lifecycle.restore(prepared, live_world)?;
        let saved_entities: std::collections::BTreeSet<_> = self
            .entity_bindings
            .iter()
            .map(EntityBindingSnapshot::saved_entity)
            .chain(self.samples.sources().map(|source| source.saved_entity))
            .collect();
        let supplied_entities: std::collections::BTreeSet<_> =
            entities.iter().map(|(saved, _)| *saved).collect();
        ensure!(
            saved_entities == supplied_entities,
            "session prepared entity census differs from snapshot"
        );
        let bindings = AudioRestoreBindings::new(self.logical.actor_bindings(), entities)?;
        ensure!(
            bindings.entity(self.player.entity)? == player.entity,
            "session replacement player entity differs"
        );
        let player_actor = bindings.actor(ScriptActorId {
            generation: self.player.actor.0,
            index: self.player.actor.1,
        })?;
        let marker = self.player.placement.marker_entry();
        let runtime_binding =
            self.player
                .runtime
                .source_identity()
                .map(|(saved_player, saved_generation)| PlayerRuntimeBinding {
                    saved_player,
                    saved_generation,
                    player: player.entity,
                    generation: player_actor.generation,
                    scene: player.scene.clone(),
                    animation: player.animation.clone(),
                    marker,
                });
        if let Some(binding) = &runtime_binding {
            ensure!(
                binding.saved_player == self.player.entity
                    && binding.saved_generation == self.player.actor.0,
                "session saved runtime player identity differs"
            );
        }
        let runtime = PlayerRuntime::from_snapshot(self.player.runtime, runtime_binding)?;
        let placement = self.player.placement.restore(
            self.world,
            player.animation.clone(),
            player.scene.clone(),
        )?;
        let samples = self.samples.restore(source_audio, &bindings, live_world)?;
        let sample_slots = (0..16).map(|slot| (slot, slot)).collect();
        let dialogue = RuntimeDialogue::from_snapshot(
            self.dialogue,
            dialogue_assets,
            &sample_slots,
            &bindings,
        )?;
        let music = Music::from_snapshot(self.music, music_source)?;
        ensure!(
            self.presentation_clock.is_some()
                == (self.hud.is_some()
                    || self.environment_edits.is_some()
                    || self.camera.is_some()),
            "session HUD and presentation clock census differ"
        );
        let presentation_clock = self
            .presentation_clock
            .map(|saved| {
                PresentationClock::from_snapshot(
                    saved,
                    resource::<bevy::prelude::Time<bevy::prelude::Real>>(
                        live_world,
                        "presentation host clock",
                    )?,
                )
            })
            .transpose()?;
        ensure!(
            self.sky.is_some() == self.environment_edits.is_some(),
            "session sky/environment census differs"
        );
        let sky = match (self.sky, sky_images) {
            (Some(saved), Some(images)) => Some(
                saved.restore(
                    environments
                        .get(&self.world)
                        .context("session current sky environment absent")?,
                    images,
                )?,
            ),
            (None, None) => None,
            _ => anyhow::bail!("session sky/prepared image census differs"),
        };
        ensure!(
            self.render_playback.is_some() == self.environment_edits.is_some(),
            "session render playback/environment census differs"
        );
        let render_playback = match (self.render_playback, prepared_render_playback) {
            (Some(saved), Some(prepared)) => Some(
                saved.restore(
                    prepared,
                    environments
                        .get(&self.world)
                        .context("session terrain environment absent")?,
                    live_world,
                    std::iter::once(
                        scene
                            .as_ref()
                            .context("session rendered scene absent")?
                            .root(),
                    )
                    .chain(player_model.as_ref().map(|model| model.root()))
                    .chain(actor_models.iter().map(|model| model.root()))
                    .collect(),
                )?,
            ),
            (None, None) => None,
            _ => anyhow::bail!("session render playback/prepared census differs"),
        };
        let camera = self.camera.map(CameraSnapshot::restore).transpose()?;
        let environment_edits = self
            .environment_edits
            .map(|saved| saved.restore(environments))
            .transpose()?;
        let hud = match (self.hud, hud_images) {
            (Some(saved), Some(images)) => Some(HudState::from_snapshot(hud_asset, images, saved)?),
            (None, None) => None,
            _ => anyhow::bail!("session HUD presentation and prepared image census differ"),
        };
        ensure!(
            self.entity_bindings
                .iter()
                .any(|entry| entry.saved_entity() == self.player.entity
                    && (entry.actor().generation, entry.actor().index) == self.player.actor),
            "session selected player absent from entity binding census"
        );
        let logical = self.logical.finish(&sample_slots)?;
        let current = logical
            .actors
            .current
            .as_ref()
            .context("session replacement current world absent")?;
        ensure!(
            current.generation() == player_actor.generation
                && current.selected_player() == Some(player_actor),
            "session replacement player is not the current logical player"
        );
        let entity_bindings = entities::restore(
            self.entity_bindings,
            &bindings,
            current,
            &player.animation,
            live_world,
        )?;
        if let Some(scene) = &scene {
            if let Some(playback) = &render_playback {
                playback.validate(live_world, scene)?;
            }
            for binding in &entity_bindings {
                if binding.is_map() {
                    scene.validate_map_entity(live_world, binding.entity)?;
                }
            }
        }
        let cheats = self
            .cheats
            .map(|snapshot| {
                snapshot.prepare(resource::<cyrus_game::cheats::Cheats>(
                    live_world, "cheats",
                )?)
            })
            .transpose()?;
        Ok(RuntimeSessionCandidate {
            sources,
            source_identities: self.source_identities,
            cheats,
            movie_unlocks: self.movie_unlocks.map(cyrus_game::MovieUnlocks::from_bits),
            fidelity: self.fidelity,
            lifecycle,
            scene,
            camera_entity,
            logical,
            player,
            entity_bindings,
            placement,
            runtime,
            samples,
            dialogue,
            music,
            hud,
            presentation_clock,
            environment_edits,
            camera,
            sky,
            render_playback,
            player_model,
            actor_models,
            marker,
        })
    }
}

fn animation_identity(data: &Animations) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(data)?)
        .to_hex()
        .to_string())
}

fn validate_player_assets(
    world: &World,
    scene: &Handle<cyrus_game::player::data::PlayerScene>,
    animation: &Handle<Animations>,
    expected_player: &str,
    expected_animation: &Animations,
) -> Result<()> {
    let scene = resource::<Assets<cyrus_game::player::data::PlayerScene>>(world, "player assets")?
        .get(scene)
        .context("session prepared player asset absent")?;
    ensure!(
        scene.source_identity.as_deref() == Some(expected_player),
        "session prepared player asset differs from validated source"
    );
    let animation = resource::<Assets<Animations>>(world, "animation assets")?
        .get(animation)
        .context("session prepared animation asset absent")?;
    ensure!(
        std::ptr::eq(animation, expected_animation)
            || animation_identity(animation)? == animation_identity(expected_animation)?,
        "session prepared animation asset differs from validated source"
    );
    Ok(())
}
