//! Hidden scene instances loaded without publishing a world entry request.
use crate::{RuntimeSessionSnapshot, staging::StagedSessionEntity};
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::LoadState,
    prelude::{
        AssetServer, Camera, Camera3d, Entity, GltfAssetLabel, Handle, Transform, Visibility,
        World, WorldAsset, WorldAssetRoot,
    },
    world_serialization::{WorldInstance, WorldInstanceSpawner},
};
use cyrus_world::lifecycle::PreparedWorld;

type ModelPaths = (
    Option<bevy::asset::AssetPath<'static>>,
    Vec<(u64, bevy::asset::AssetPath<'static>)>,
);

/// Owns hidden world/model roots and an inactive camera while assets instantiate.
/// Texture, terrain, sky and actor binding preparation follow instance readiness.
pub struct SessionSceneInstances {
    root: Entity,
    model: Option<Entity>,
    actor_models: std::collections::BTreeMap<u64, Entity>,
    camera: Entity,
    assets: Vec<Handle<WorldAsset>>,
    sky: Option<Entity>,
    sky_images: Option<cyrus_render::persistence::SkyImages>,
}
impl SessionSceneInstances {
    /// Assemble the rendered scene and playback owners from an already loaded
    /// hidden instance and private edited environment. No live resources change.
    /// On failure discard this stage; retry with a newly prepared stage.
    pub fn prepare_rendered(
        &mut self,
        snapshot: &RuntimeSessionSnapshot,
        prepared: &PreparedWorld,
        environment: &Handle<cyrus_assets::environment::Environment>,
        edits: &cyrus_render::persistence::EnvironmentEditsCandidate,
        world: &mut World,
    ) -> Result<(
        cyrus_render::persistence::SceneCandidate,
        crate::PreparedRenderPlayback,
    )> {
        use bevy::prelude::Assets;
        use cyrus_assets::environment::Environment;
        ensure!(
            self.ready(world)?,
            "session rendered preparation instances still loading"
        );
        ensure!(
            prepared.world == Some(snapshot.current_world()),
            "session rendered preparation current world identity differs"
        );
        ensure!(
            world.contains_resource::<Assets<Environment>>(),
            "session rendered preparation environment assets absent"
        );
        world.resource_scope(|world, assets: bevy::prelude::Mut<Assets<Environment>>| {
            let value = assets
                .get(environment)
                .context("session rendered private environment absent")?;
            self.prepare_sky(value, world)
        })?;
        let scene = cyrus_render::persistence::SceneReplacement {
            root: self.root,
            sky: self
                .sky
                .context("session rendered sky preparation absent")?,
            camera: self.camera,
            environment: environment.clone(),
        }
        .prepare_with_edits(prepared, edits, world)?;
        let playback = self.prepare_playback(snapshot, environment, world)?;
        Ok((scene, playback))
    }

    pub fn request(
        snapshot: &RuntimeSessionSnapshot,
        prepared: &PreparedWorld,
        world: &mut World,
    ) -> Result<Self> {
        Self::request_with_recipes(snapshot, prepared, None, world)
    }

    /// Load hidden model roots from the restored original logical poses and installed skins.
    /// Native snapshot frame sources are deliberately excluded from this request.
    pub fn request_original(
        snapshot: &RuntimeSessionSnapshot,
        prepared: &PreparedWorld,
        recipes: &crate::OriginalRenderRecipes,
        world: &mut World,
    ) -> Result<Self> {
        Self::request_with_recipes(snapshot, prepared, Some(recipes), world)
    }

    fn request_with_recipes(
        snapshot: &RuntimeSessionSnapshot,
        prepared: &PreparedWorld,
        recipes: Option<&crate::OriginalRenderRecipes>,
        world: &mut World,
    ) -> Result<Self> {
        ensure!(
            snapshot.sky.is_some() && snapshot.camera.is_some(),
            "session scene staging requires a rendered snapshot"
        );
        ensure!(
            prepared
                .world
                .is_none_or(|id| id == snapshot.current_world()),
            "session scene staging current world differs"
        );
        let (model_path, actor_paths) = Self::model_paths(snapshot, prepared, recipes)?;
        let server = world
            .get_resource::<AssetServer>()
            .context("session scene staging asset server absent")?
            .clone();
        let scene: Handle<WorldAsset> =
            server.load(GltfAssetLabel::Scene(0).from_asset(prepared.scene_path.clone()));
        let model: Option<Handle<WorldAsset>> =
            model_path.map(|path| server.load(GltfAssetLabel::Scene(0).from_asset(path)));
        let actor_assets: Vec<(u64, Handle<WorldAsset>)> = actor_paths
            .into_iter()
            .map(|(entity, path)| {
                (
                    entity,
                    server.load(GltfAssetLabel::Scene(0).from_asset(path)),
                )
            })
            .collect();
        let root = Self::spawn_model_root(world, &scene);
        let model_root = model
            .as_ref()
            .map(|handle| Self::spawn_model_root(world, handle));
        let actor_models = actor_assets
            .iter()
            .map(|(entity, handle)| (*entity, Self::spawn_model_root(world, handle)))
            .collect();
        let camera = world
            .spawn((
                StagedSessionEntity,
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..Default::default()
                },
            ))
            .id();
        Ok(Self {
            root,
            model: model_root,
            actor_models,
            camera,
            assets: std::iter::once(scene)
                .chain(model)
                .chain(actor_assets.into_iter().map(|(_, asset)| asset))
                .collect(),
            sky: None,
            sky_images: None,
        })
    }
    fn spawn_model_root(world: &mut World, handle: &Handle<WorldAsset>) -> Entity {
        world
            .spawn((
                StagedSessionEntity,
                cyrus_render::persistence::StagedRenderRoot,
                WorldAssetRoot(handle.clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id()
    }
    fn model_paths(
        snapshot: &RuntimeSessionSnapshot,
        prepared: &PreparedWorld,
        recipes: Option<&crate::OriginalRenderRecipes>,
    ) -> Result<ModelPaths> {
        let (model_path, actor_paths) = if let Some(recipes) = recipes {
            use cyrus_render::player_model::PlayerModelSnapshot;
            let model = recipes
                .player
                .as_ref()
                .map(|(pose, texture_set)| {
                    PlayerModelSnapshot::logical_source_path(prepared, &pose.model, *texture_set)
                })
                .transpose()?;
            let actors = recipes
                .actor_models
                .iter()
                .map(|(&entity, (pose, texture_set))| {
                    Ok((
                        entity,
                        PlayerModelSnapshot::logical_source_path(
                            prepared,
                            &pose.model,
                            *texture_set,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            (model, actors)
        } else {
            let model = snapshot
                .player_model()
                .map(|model| model.source_path(prepared))
                .transpose()?;
            let actors = snapshot
                .actor_models
                .iter()
                .map(|(&entity, model)| Ok((entity, model.source_path(prepared)?)))
                .collect::<Result<Vec<_>>>()?;
            (model, actors)
        };
        Ok((model_path, actor_paths))
    }
    /// Describe the exact loaded logical model sources without advancing actors or animations.
    /// Root-local identity and inherited visibility match ordinary frame-source replacement;
    /// actor world affine poses remain owned by the separately prepared scene state.
    pub fn original_models(
        &self,
        recipes: &crate::OriginalRenderRecipes,
        world: &World,
    ) -> Result<(
        Option<cyrus_render::player_model::PlayerModelSnapshot>,
        std::collections::BTreeMap<u64, cyrus_render::player_model::PlayerModelSnapshot>,
    )> {
        use cyrus_render::player_model::PlayerModelSnapshot;
        ensure!(
            self.ready(world)?,
            "session logical model instances still loading"
        );
        ensure!(
            self.model.is_some() == recipes.player.is_some()
                && self.actor_models.keys().eq(recipes.actor_models.keys()),
            "session logical model recipe/root census differs"
        );
        let player = self
            .model
            .zip(recipes.player.as_ref())
            .map(|(root, (pose, texture_set))| {
                PlayerModelSnapshot::from_logical(
                    world,
                    root,
                    pose.clone(),
                    *texture_set,
                    Transform::default(),
                    Visibility::Inherited,
                )
            })
            .transpose()?;
        let actors = recipes
            .actor_models
            .iter()
            .map(|(&entity, (pose, texture_set))| {
                Ok((
                    entity,
                    PlayerModelSnapshot::from_logical(
                        world,
                        *self.actor_models.get(&entity).with_context(|| {
                            format!("session logical actor {entity}: model root absent")
                        })?,
                        pose.clone(),
                        *texture_set,
                        Transform::default(),
                        Visibility::Inherited,
                    )?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok((player, actors))
    }

    /// Prepare private sky images and a hidden triangle using the normal renderer's
    /// asset builder. The environment must include the saved environment edits.
    pub fn prepare_sky(
        &mut self,
        environment: &cyrus_assets::environment::Environment,
        world: &mut World,
    ) -> Result<()> {
        use bevy::prelude::{Assets, Image, Mesh, Mesh3d};
        ensure!(self.sky.is_none(), "session sky already prepared");
        ensure!(
            world.contains_resource::<Assets<Image>>() && world.contains_resource::<Assets<Mesh>>(),
            "session sky image/mesh stores absent"
        );
        let prepared = cyrus_render::environment::prepare_sky_assets(environment)?;
        let images = {
            let mut images = world.resource_mut::<Assets<Image>>();
            cyrus_render::persistence::SkyImages {
                sky: images.add(prepared.sky),
                sun: images.add(prepared.sun),
            }
        };
        let mesh = world.resource_mut::<Assets<Mesh>>().add(prepared.mesh);
        self.sky = Some(
            world
                .spawn((
                    StagedSessionEntity,
                    Mesh3d(mesh),
                    Visibility::Hidden,
                    bevy::camera::visibility::NoFrustumCulling,
                ))
                .id(),
        );
        self.sky_images = Some(images);
        Ok(())
    }
    /// Prepare all rendered playback owners in dependency order under hidden roots.
    /// On failure discard this stage; only its private entities/assets may have changed.
    pub fn prepare_playback(
        &self,
        snapshot: &RuntimeSessionSnapshot,
        environment: &Handle<cyrus_assets::environment::Environment>,
        world: &mut World,
    ) -> Result<crate::PreparedRenderPlayback> {
        ensure!(
            self.ready(world)?,
            "session playback scene instance still loading"
        );
        ensure!(
            world
                .contains_resource::<bevy::prelude::Assets<cyrus_assets::environment::Environment>>(
                ),
            "session playback environment store absent"
        );
        let terrain_bindings = snapshot.terrain_bindings(world, self.root)?;
        let roots: Vec<_> = std::iter::once(self.root)
            .chain(self.model)
            .chain(self.actor_models.values().copied())
            .collect();
        cyrus_render::prepare_scene_flats(world, &roots)?;
        let textures = self.prepare_textures(environment, world)?;
        world.resource_scope(
            |world,
             environments: bevy::prelude::Mut<
                bevy::prelude::Assets<cyrus_assets::environment::Environment>,
            >| {
                let data = environments
                    .get(environment)
                    .context("session playback environment absent")?;
                self.prepare_materials(snapshot, data, environment.id(), world)
            },
        )?;
        let terrain = self.prepare_terrain(environment, &terrain_bindings, world)?;
        Ok(crate::PreparedRenderPlayback {
            textures,
            terrain,
            terrain_bindings,
        })
    }

    /// Bind replacement texture timelines under the hidden world and model roots.
    pub fn prepare_textures(
        &self,
        environment: &Handle<cyrus_assets::environment::Environment>,
        world: &mut World,
    ) -> Result<cyrus_render::fog::TextureAnimations> {
        ensure!(
            self.ready(world)?,
            "session texture scene instance still loading"
        );
        let roots: Vec<_> = std::iter::once(self.root)
            .chain(self.model)
            .chain(self.actor_models.values().copied())
            .collect();
        use bevy::prelude::{Assets, Image, Mut};
        use cyrus_assets::environment::Environment;
        ensure!(
            world.contains_resource::<Assets<Environment>>(),
            "session texture environment store absent"
        );
        let mut textures =
            cyrus_render::fog::TextureAnimations::prepare(world, environment.id(), &roots)?;
        world.resource_scope(
            |world, environments: Mut<Assets<Environment>>| -> Result<()> {
                let environment = environments
                    .get(environment)
                    .context("session effect texture environment absent")?;
                if let Some(source) = &environment.effects {
                    let mut images = world
                        .get_resource_mut::<Assets<Image>>()
                        .context("session effect texture image store absent")?;
                    textures.prepare_effects(source, &mut images)?;
                }
                Ok(())
            },
        )?;
        Ok(textures)
    }

    /// Prepare destination materials from saved camera, fidelity and clipping state.
    /// Call after flat and texture binding, before candidate assembly.
    pub fn prepare_materials(
        &self,
        snapshot: &RuntimeSessionSnapshot,
        environment: &cyrus_assets::environment::Environment,
        environment_id: bevy::prelude::AssetId<cyrus_assets::environment::Environment>,
        world: &mut World,
    ) -> Result<()> {
        ensure!(
            self.ready(world)?,
            "session material scene instance still loading"
        );
        let fidelity = snapshot
            .fidelity
            .as_ref()
            .context("session rendered fidelity absent")?;
        let camera = snapshot
            .camera
            .clone()
            .context("session rendered camera absent")?
            .restore()?;
        let clipping = snapshot
            .render_playback
            .as_ref()
            .context("session rendered playback absent")?
            .clipping(fidelity)?;
        let roots: Vec<_> = std::iter::once(self.root)
            .chain(self.model)
            .chain(self.actor_models.values().copied())
            .collect();
        cyrus_render::fog::prepare_scene_materials(
            world,
            &roots,
            environment,
            environment_id,
            fidelity,
            clipping,
            camera.position(),
            &snapshot.logical.object_lights()?,
            &snapshot.logical.actor_prelighting()?,
        )
    }

    /// Bind private terrain meshes in caller-supplied saved ordinal order.
    /// Targets must cover all terrain meshes under this hidden world instance.
    pub fn prepare_terrain(
        &self,
        environment: &Handle<cyrus_assets::environment::Environment>,
        targets: &[Entity],
        world: &mut World,
    ) -> Result<cyrus_render::WaterPlayback> {
        use bevy::prelude::{Assets, Children, Mesh, Mesh3d, Name};
        ensure!(
            self.ready(world)?,
            "session terrain scene instance still loading"
        );
        let data = world
            .get_resource::<Assets<cyrus_assets::environment::Environment>>()
            .and_then(|assets| assets.get(environment))
            .context("session terrain environment absent")?;
        let active = data.terrain_lighting.is_some()
            || data
                .water
                .as_ref()
                .is_some_and(|water| water.parameters[0] != 0.0);
        let mut pending = vec![(self.root, false)];
        let mut seen = std::collections::BTreeSet::new();
        let mut terrain = std::collections::BTreeSet::new();
        while let Some((entity, parent_terrain)) = pending.pop() {
            ensure!(
                seen.insert(entity),
                "session terrain hierarchy repeats an entity"
            );
            let is_terrain = parent_terrain
                || world
                    .get::<Name>(entity)
                    .is_some_and(|name| name.as_str() == "Terrain");
            if active && is_terrain && world.get::<Mesh3d>(entity).is_some() {
                terrain.insert(entity);
            }
            if let Some(children) = world.get::<Children>(entity) {
                pending.extend(children.iter().map(|child| (*child, is_terrain)));
            }
        }
        let supplied: std::collections::BTreeSet<_> = targets.iter().copied().collect();
        ensure!(
            supplied.len() == targets.len() && supplied == terrain,
            "session terrain target census differs"
        );
        ensure!(
            world.contains_resource::<Assets<Mesh>>(),
            "session terrain mesh store absent"
        );
        let (playback, bindings) =
            world.resource_scope(|world, mut meshes: bevy::prelude::Mut<Assets<Mesh>>| {
                let data = world
                    .get_resource::<Assets<cyrus_assets::environment::Environment>>()
                    .and_then(|assets| assets.get(environment))
                    .context("session terrain environment absent")?;
                let targets: Vec<_> = targets
                    .iter()
                    .map(|&entity| cyrus_render::TerrainTarget::of_entity(world, entity, data))
                    .collect::<Option<_>>()
                    .context("session terrain target has no mesh")?;
                cyrus_render::WaterPlayback::prepare_meshes(
                    data,
                    environment.id(),
                    &targets,
                    &mut meshes,
                )
            })?;
        for binding in bindings {
            let mut entity = world.entity_mut(binding.entity);
            entity.insert(Mesh3d(binding.mesh));
            if let Some(bounds) = binding.bounds {
                entity.insert(bounds);
            }
        }
        Ok(playback)
    }

    pub fn sky(&self) -> Option<Entity> {
        self.sky
    }
    pub fn sky_images(&self) -> Option<cyrus_render::persistence::SkyImages> {
        self.sky_images
            .as_ref()
            .map(|images| cyrus_render::persistence::SkyImages {
                sky: images.sky.clone(),
                sun: images.sun.clone(),
            })
    }

    pub fn root(&self) -> Entity {
        self.root
    }
    pub fn model(&self) -> Option<Entity> {
        self.model
    }
    pub fn actor_models(&self) -> &std::collections::BTreeMap<u64, Entity> {
        &self.actor_models
    }
    pub fn camera(&self) -> Entity {
        self.camera
    }

    /// Wait for every instance, reporting loader failures instead of waiting forever.
    pub fn ready(&self, world: &World) -> Result<bool> {
        let server = world
            .get_resource::<AssetServer>()
            .context("session scene staging asset server absent")?;
        let spawner = world
            .get_resource::<WorldInstanceSpawner>()
            .context("session scene instance spawner absent")?;
        for asset in &self.assets {
            if let Some(LoadState::Failed(error)) = server.get_load_state(asset.id()) {
                anyhow::bail!("session scene staging {:?}: {error}", asset.id());
            }
        }
        let mut ready = true;
        for entity in std::iter::once(self.root)
            .chain(self.model)
            .chain(self.actor_models.values().copied())
        {
            ensure!(
                world.get_entity(entity).is_ok(),
                "session staged scene {entity:?} disappeared"
            );
            ensure!(
                world.get::<Visibility>(entity) == Some(&Visibility::Hidden),
                "session staged scene {entity:?} became visible"
            );
            ready &= world
                .get::<WorldInstance>(entity)
                .is_some_and(|instance| spawner.instance_is_ready(**instance));
        }
        ensure!(
            world
                .get::<Camera>(self.camera)
                .is_some_and(|camera| !camera.is_active),
            "session staging camera absent or active"
        );
        Ok(ready)
    }
    /// Cancel only roots still owned by this stage. Aggregate commit removes the
    /// staging markers before these entities become live owners.
    pub fn discard(self, world: &mut World) {
        for entity in [Some(self.root), self.model, Some(self.camera), self.sky]
            .into_iter()
            .flatten()
            .chain(self.actor_models.into_values())
        {
            if world.get::<StagedSessionEntity>(entity).is_some() {
                world.despawn(entity);
            }
        }
    }
}
