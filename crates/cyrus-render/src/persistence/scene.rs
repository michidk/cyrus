//! Adoption of a fully prepared scene without replaying world presentation startup.
use anyhow::{Context, Result, ensure};
use bevy::prelude::{
    AssetId, AssetServer, Assets, ChildOf, Commands, Entity, GltfAssetLabel, Handle, Mesh3d,
    Resource, Visibility, With, World, WorldAsset, WorldAssetRoot,
};
use bevy::world_serialization::{WorldInstance, WorldInstanceSpawner};
use cyrus_assets::{animation::Animations, environment::Environment, script::Scripts};
use cyrus_world::lifecycle::PreparedWorld;

#[derive(Resource)]
pub(crate) struct RestoredScene {
    animations: AssetId<Animations>,
    scripts: AssetId<Scripts>,
}
impl RestoredScene {
    #[cfg(test)]
    pub(crate) fn new(prepared: &PreparedWorld) -> Self {
        Self {
            animations: prepared.animations.id(),
            scripts: prepared.scripts.id(),
        }
    }
    pub(crate) fn matches(&self, prepared: &PreparedWorld) -> bool {
        self.animations == prepared.animations.id() && self.scripts == prepared.scripts.id()
    }
}

/// Prepared replacement scene roots. These must not yet carry live script/sky bindings.
/// The host keeps staged geometry out of live presentation until publication.
pub struct SceneReplacement {
    pub root: Entity,
    pub sky: Entity,
    pub camera: Entity,
    pub environment: Handle<Environment>,
}

pub struct SceneCandidate {
    replacement: SceneReplacement,
    world_asset: Handle<WorldAsset>,
    animations: Handle<Animations>,
    scripts: Handle<Scripts>,
    environment_identity: [u8; 32],
}
impl SceneReplacement {
    /// Staged roots must survive retirement of the old scene and stay hidden until commit.
    pub fn validate_staging(&self, world: &World) -> Result<()> {
        ensure!(
            world.get::<Visibility>(self.root) == Some(&Visibility::Hidden),
            "scene replacement root must remain hidden during preparation"
        );
        for target in [self.root, self.sky, self.camera] {
            let mut cursor = Some(target);
            let mut seen = std::collections::BTreeSet::new();
            while let Some(entity) = cursor {
                ensure!(
                    seen.insert(entity),
                    "scene replacement hierarchy contains a cycle"
                );
                ensure!(
                    world.get_entity(entity).is_ok(),
                    "scene replacement hierarchy entity {entity:?} absent"
                );
                ensure!(
                    world.get::<crate::SceneEntity>(entity).is_none()
                        && world.get::<crate::sky::Sky>(entity).is_none(),
                    "scene replacement target {target:?} belongs to the retiring scene"
                );
                cursor = world.get::<ChildOf>(entity).map(ChildOf::parent);
            }
        }
        Ok(())
    }

    /// Require completed world-instance preparation and exact host-selected asset handles.
    pub fn prepare(self, prepared: &PreparedWorld, world: &World) -> Result<SceneCandidate> {
        self.prepare_inner(prepared, None, world)
    }

    /// Validate a private edited environment against the installed source plus saved edits.
    pub fn prepare_with_edits(
        self,
        prepared: &PreparedWorld,
        edits: &super::EnvironmentEditsCandidate,
        world: &World,
    ) -> Result<SceneCandidate> {
        self.prepare_inner(prepared, Some(edits), world)
    }

    fn prepare_inner(
        self,
        prepared: &PreparedWorld,
        edits: Option<&super::EnvironmentEditsCandidate>,
        world: &World,
    ) -> Result<SceneCandidate> {
        let server = world
            .get_resource::<AssetServer>()
            .context("scene replacement asset server absent")?;
        let path = GltfAssetLabel::Scene(0).from_asset(prepared.scene_path.clone());
        let expected = server
            .get_handle::<WorldAsset>(path)
            .context("scene replacement world asset not prepared")?;
        let environment = server
            .get_handle::<Environment>(prepared.environment_path.clone())
            .context("scene replacement environment not prepared")?;
        let assets = world
            .get_resource::<Assets<Environment>>()
            .context("scene environment store absent")?;
        let original = assets
            .get(&environment)
            .context("scene installed environment absent")?;
        let replacement = assets
            .get(&self.environment)
            .context("scene replacement environment absent")?;
        if let Some(edits) = edits {
            ensure!(
                environment != self.environment,
                "edited scene environment must be a private asset"
            );
            let original: Environment = serde_json::from_slice(&serde_json::to_vec(original)?)?;
            let expected = edits.prepare_environment(
                prepared
                    .world
                    .context("edited scene world identity absent")?,
                original,
            )?;
            ensure!(
                environment_identity(&expected)? == environment_identity(replacement)?,
                "scene replacement environment differs from installed source plus saved edits"
            );
        } else {
            ensure!(
                environment == self.environment,
                "scene replacement environment binding differs"
            );
        }
        let environment_identity = environment_identity(replacement)?;
        let candidate = SceneCandidate {
            environment_identity,
            replacement: self,
            world_asset: expected,
            animations: prepared.animations.clone(),
            scripts: prepared.scripts.clone(),
        };
        candidate.validate(world)?;
        Ok(candidate)
    }
}
impl SceneCandidate {
    /// Exact private environment that publication will adopt.
    pub fn environment(&self) -> &Handle<Environment> {
        &self.replacement.environment
    }

    /// Require every reconstructed owner to use the environment published by this scene.
    pub fn validate_environment(&self, environment: &Environment) -> Result<()> {
        ensure!(
            environment_identity(environment)? == self.environment_identity,
            "session owner environment differs from prepared scene"
        );
        Ok(())
    }

    /// Recheck prepared entities immediately before the aggregate's irreversible publication.
    pub fn validate(&self, world: &World) -> Result<()> {
        self.replacement.validate_staging(world)?;
        let target = &self.replacement;
        let environment = world
            .get_resource::<Assets<Environment>>()
            .and_then(|assets| assets.get(&target.environment))
            .context("scene replacement environment disappeared")?;
        ensure!(
            environment_identity(environment)? == self.environment_identity,
            "scene replacement environment changed after validation"
        );
        ensure!(
            target.root != target.sky
                && target.root != target.camera
                && target.sky != target.camera,
            "scene replacement entities alias"
        );
        ensure!(
            world
                .get::<WorldAssetRoot>(target.root)
                .is_some_and(|root| root.0 == self.world_asset),
            "scene replacement root asset differs"
        );
        let instance = world
            .get::<WorldInstance>(target.root)
            .context("scene replacement instance absent")?;
        ensure!(
            world
                .get_resource::<WorldInstanceSpawner>()
                .is_some_and(|spawner| spawner.instance_is_ready(**instance)),
            "scene replacement instance is not ready"
        );
        ensure!(
            world
                .get::<cyrus_game::script::ScriptScene>(target.root)
                .is_none()
                && world.get::<crate::SceneEntity>(target.root).is_none(),
            "scene replacement root already has a live owner"
        );
        ensure!(
            world.get::<Mesh3d>(target.sky).is_some()
                && world.get::<crate::sky::Sky>(target.sky).is_none(),
            "scene replacement sky is absent or already live"
        );
        ensure!(
            world
                .get::<bevy::prelude::Camera3d>(target.camera)
                .is_some()
                && world
                    .get::<crate::camera::ViewCamera>(target.camera)
                    .is_none(),
            "scene replacement camera absent"
        );
        ensure!(
            world
                .get_resource::<Assets<Environment>>()
                .and_then(|assets| assets.get(&target.environment))
                .is_some(),
            "scene replacement environment asset absent"
        );
        ensure!(
            world.contains_resource::<crate::fog::RenderDistance>(),
            "scene replacement clipping owner absent"
        );
        ensure!(
            world.contains_resource::<Assets<crate::sky::SkyMaterial>>(),
            "scene replacement sky material store absent"
        );
        Ok(())
    }
    pub fn matches_prepared(&self, prepared: &PreparedWorld) -> bool {
        self.animations == prepared.animations && self.scripts == prepared.scripts
    }
    /// Require actor presentation nodes to belong to the prepared world instance.
    pub fn validate_map_entity(&self, world: &World, entity: Entity) -> Result<()> {
        let mut cursor = Some(entity);
        let mut seen = std::collections::BTreeSet::new();
        while let Some(node) = cursor {
            ensure!(seen.insert(node), "scene actor hierarchy contains a cycle");
            if node == self.replacement.root {
                return Ok(());
            }
            cursor = world.get::<ChildOf>(node).map(ChildOf::parent);
        }
        anyhow::bail!("session map entity {entity:?} is outside the prepared scene")
    }

    pub fn root(&self) -> Entity {
        self.replacement.root
    }
    pub fn camera(&self) -> Entity {
        self.replacement.camera
    }
    pub fn sky(&self) -> Entity {
        self.replacement.sky
    }

    /// Queue only after every session owner and entity binding has validated.
    pub fn queue(self, commands: &mut Commands) {
        commands.queue(move |world: &mut World| {
            let old: Vec<_> = world
                .query_filtered::<Entity, With<crate::SceneEntity>>()
                .iter(world)
                .collect();
            for entity in old {
                world.despawn(entity);
            }
            let old: Vec<_> = world
                .query_filtered::<Entity, With<crate::sky::Sky>>()
                .iter(world)
                .collect();
            for entity in old {
                world.despawn(entity);
            }
            let target = self.replacement;
            let environment = world
                .resource::<Assets<Environment>>()
                .get(&target.environment)
                .expect("validated environment");
            let clipping = environment.clipping;
            let background = environment.background;
            world.insert_resource(crate::environment::EnvironmentRequest(
                target.environment.clone(),
            ));
            world.insert_resource(crate::environment::ActiveEnvironment {
                source: Some(target.environment.id()),
                sky: Some(target.sky),
            });
            world.resource_mut::<crate::fog::RenderDistance>().original = Some(clipping);
            world.insert_resource(bevy::prelude::ClearColor(bevy::prelude::Color::srgb_u8(
                background[0],
                background[1],
                background[2],
            )));
            world.entity_mut(target.root).insert((
                crate::SceneEntity,
                cyrus_game::script::ScriptScene::new(self.animations.clone()),
                Visibility::Inherited,
            ));
            world.insert_resource(crate::ReadyScene(self.animations.clone()));
            world.insert_resource(RestoredScene {
                animations: self.animations.id(),
                scripts: self.scripts.id(),
            });
        });
    }
}

fn environment_identity(environment: &Environment) -> Result<[u8; 32]> {
    Ok(*blake3::hash(&serde_json::to_vec(environment)?).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_environment_commit_survives_first_scene_update() -> Result<()> {
        use bevy::asset::io::{
            AssetSourceBuilder, AssetSourceId,
            memory::{Dir, MemoryAssetReader},
        };
        use bevy::prelude::*;
        use bevy::world_serialization::WorldSerializationPlugin;
        let directory = Dir::default();
        directory.insert_asset_text(std::path::Path::new("test.glb"),
            r#"{"asset":{"version":"2.0"},"nodes":[{"name":"map"}],"scenes":[{"nodes":[0]}],"scene":0}"#);
        let data = serde_json::json!({"texture_scale":[1,1],"sky":null,"background":[1,2,3],
            "clipping":[7.0,3800.0],"fog_table":vec![[0,0,0,0];16],"water":null,"terrain_lighting":null,
            "model_lighting":{"ambient":[64.0,64.0,64.0],"directional":[255.0,255.0,255.0],"direction":[0.0,-1.0,0.0]},
            "prelighting":{"instances":{},"lights":[]},"object_lights":[]});
        directory.insert_asset_text(std::path::Path::new("test.environment"), &data.to_string());
        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: directory.clone(),
                })
            }),
        )
        .add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            WorldSerializationPlugin,
            bevy::mesh::MeshPlugin,
            bevy::gltf::GltfPlugin::default(),
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Environment>()
        .init_asset::<crate::sky::SkyMaterial>()
        .init_asset_loader::<cyrus_assets::environment::EnvironmentLoader>()
        .init_resource::<crate::fog::RenderDistance>()
        .insert_resource(crate::camera::FrameScene(false))
        .add_systems(Update, crate::scene_presentation);
        app.finish();
        app.cleanup();
        let server = app.world().resource::<AssetServer>().clone();
        let original: Handle<Environment> = server.load("test.environment");
        let scene: Handle<WorldAsset> =
            server.load(GltfAssetLabel::Scene(0).from_asset("test.glb"));
        let root = app
            .world_mut()
            .spawn((WorldAssetRoot(scene), Visibility::Hidden))
            .id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.update();
            let ready = app
                .world()
                .get::<WorldInstance>(root)
                .is_some_and(|instance| {
                    app.world()
                        .resource::<WorldInstanceSpawner>()
                        .instance_is_ready(**instance)
                });
            if ready
                && app
                    .world()
                    .resource::<Assets<Environment>>()
                    .contains(&original)
            {
                break;
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "scene/environment fixture load timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let private = app
            .world_mut()
            .resource_mut::<Assets<Environment>>()
            .add(serde_json::from_value::<Environment>(data)?);
        let sky = app.world_mut().spawn(Mesh3d(Handle::default())).id();
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        let prepared = PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(17),
            animations: Handle::default(),
            scripts: Handle::default(),
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        };
        let edits: super::super::EnvironmentEditsSnapshot =
            serde_json::from_value(serde_json::json!({"version":1,"edits":{}}))?;
        let edits = edits.restore(&std::collections::BTreeMap::from([(
            17,
            app.world()
                .resource::<Assets<Environment>>()
                .get(&original)
                .unwrap(),
        )]))?;
        let candidate = SceneReplacement {
            root,
            sky,
            camera,
            environment: private.clone(),
        }
        .prepare_with_edits(&prepared, &edits, app.world())?;
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .get_mut(&private)
            .unwrap()
            .background = [9, 9, 9];
        assert!(candidate.validate(app.world()).is_err());
        assert!(
            !app.world()
                .contains_resource::<crate::environment::EnvironmentRequest>()
        );
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .get_mut(&private)
            .unwrap()
            .background = [1, 2, 3];
        candidate.validate(app.world())?;
        let mut queue = bevy::ecs::world::CommandQueue::default();
        candidate.queue(&mut Commands::new(&mut queue, app.world()));
        queue.apply(app.world_mut());
        app.insert_resource(prepared);
        app.update();
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::environment::EnvironmentRequest>()
                .0,
            private
        );
        assert!(app.world().get::<crate::SceneEntity>(root).is_some());
        assert!(!app.world().resource::<crate::camera::FrameScene>().0);
        assert_eq!(
            app.world()
                .resource::<Assets<Environment>>()
                .get(&original)
                .unwrap()
                .background,
            [1, 2, 3]
        );
        Ok(())
    }

    #[test]
    fn staging_rejects_retiring_ancestors_and_visible_preparation() -> Result<()> {
        let mut world = World::new();
        let old = world.spawn(crate::SceneEntity).id();
        let root = world.spawn(Visibility::Hidden).id();
        let sky = world.spawn(ChildOf(root)).id();
        let camera = world.spawn_empty().id();
        let replacement = SceneReplacement {
            root,
            sky,
            camera,
            environment: Handle::default(),
        };
        replacement.validate_staging(&world)?;
        world.entity_mut(root).insert(ChildOf(old));
        assert!(replacement.validate_staging(&world).is_err());
        world.entity_mut(root).remove::<ChildOf>();
        world.entity_mut(camera).insert(ChildOf(old));
        assert!(replacement.validate_staging(&world).is_err());
        world.entity_mut(camera).remove::<ChildOf>();
        world.entity_mut(root).insert(Visibility::Inherited);
        assert!(replacement.validate_staging(&world).is_err());
        world.entity_mut(root).insert(Visibility::Hidden);
        replacement.validate_staging(&world)?;
        let candidate = SceneCandidate {
            replacement,
            environment_identity: [0; 32],
            world_asset: Handle::default(),
            animations: Handle::default(),
            scripts: Handle::default(),
        };
        candidate.validate_map_entity(&world, sky)?;
        assert!(candidate.validate_map_entity(&world, old).is_err());
        Ok(())
    }
}
