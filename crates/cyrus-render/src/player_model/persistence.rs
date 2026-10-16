//! Adoption of a fully loaded player model without a second GLTF request.
use super::*;
use crate::persistence::SceneSourceIdentity;
use anyhow::{Result, ensure};
use bevy::world_serialization::{WorldInstance, WorldInstanceSpawner};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct PlayerModelSnapshot {
    version: u32,
    root: u64,
    model: String,
    frame: usize,
    source: SceneSourceIdentity,
    transform: Transform,
    visibility: u8,
}

pub struct PlayerModelCandidate {
    root: Entity,
    player: Entity,
    source: AssetPath<'static>,
    asset: Handle<WorldAsset>,
    saved: PlayerModelSnapshot,
    nodes: Vec<Entity>,
    base_meshes: Vec<(Entity, Visibility)>,
    frames: crate::animation::PreparedModelFrames,
}

fn ready(world: &World, root: Entity) -> Result<()> {
    let instance = world
        .get::<WorldInstance>(root)
        .ok_or_else(|| anyhow::anyhow!("player model {root:?}: instance absent"))?;
    ensure!(
        world
            .get_resource::<WorldInstanceSpawner>()
            .is_some_and(|spawner| spawner.instance_is_ready(**instance)),
        "player model {root:?}: instance is not ready"
    );
    Ok(())
}

fn model_nodes(world: &World, root: Entity, model: &str) -> Result<Vec<Entity>> {
    let mut pending = vec![root];
    let mut seen = std::collections::BTreeSet::new();
    let mut nodes = Vec::new();
    while let Some(entity) = pending.pop() {
        ensure!(
            seen.insert(entity),
            "player model hierarchy repeats an entity"
        );
        if world
            .get::<Name>(entity)
            .is_some_and(|name| name.as_str() == model)
        {
            nodes.push(entity);
        }
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
    }
    ensure!(
        !nodes.is_empty(),
        "player model {model}: frame source nodes absent"
    );
    nodes.sort();
    Ok(nodes)
}

impl PlayerModelSnapshot {
    pub fn saved_root(&self) -> u64 {
        self.root
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// Resolve the installed model sidecar for off-live asset preparation.
    pub fn source_path(&self, prepared: &PreparedWorld) -> Result<AssetPath<'static>> {
        ensure!(
            !self.model.is_empty()
                && self
                    .model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "player model source key invalid"
        );
        Ok(model_source(prepared, &self.model))
    }

    pub fn capture(world: &World, player: Entity) -> Result<Option<Self>> {
        let roots: Vec<_> = world
            .try_query::<(Entity, &ModelRoot)>()
            .map(|mut query| {
                query
                    .iter(world)
                    .filter(|(_, binding)| binding.player == player)
                    .map(|(entity, _)| entity)
                    .collect()
            })
            .unwrap_or_default();
        let pose = world.get::<AnimationPose>(player);
        if roots.is_empty()
            && pose.is_none_or(|pose| !replaces_base(world.get::<Name>(player), pose))
        {
            return Ok(None);
        }
        ensure!(
            roots.len() == 1,
            "player model capture requires one settled root"
        );
        let root = roots[0];
        ready(world, root)?;
        let pose =
            pose.ok_or_else(|| anyhow::anyhow!("player model root has no displayed player pose"))?;
        let owner = world.get::<ModelRoot>(root).expect("queried model root");
        let prepared = world
            .get_resource::<PreparedWorld>()
            .ok_or_else(|| anyhow::anyhow!("player model capture: prepared world absent"))?;
        let expected_source = model_source(prepared, &pose.model);
        ensure!(
            owner.source == expected_source,
            "player model capture: source differs from prepared world"
        );
        let expected_asset = world
            .get_resource::<AssetServer>()
            .and_then(|server| {
                server
                    .get_handle::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(expected_source))
            })
            .ok_or_else(|| anyhow::anyhow!("player model capture: source asset absent"))?;
        ensure!(
            world
                .get::<WorldAssetRoot>(root)
                .is_some_and(|binding| binding.0 == expected_asset),
            "player model capture: loaded asset differs from prepared world"
        );
        ensure!(
            owner.model == pose.model
                && world
                    .get::<ChildOf>(root)
                    .is_some_and(|parent| parent.parent() == player),
            "player model owner/pose binding differs"
        );
        let nodes = model_nodes(world, root, &pose.model)?;
        crate::animation::validate_model_pose(world, &nodes, &prepared.animations, pose)?;
        for node in nodes {
            ensure!(
                world.get::<AnimationPose>(node) == Some(pose),
                "player model frame binding has not settled"
            );
        }
        let transform = *world
            .get::<Transform>(root)
            .ok_or_else(|| anyhow::anyhow!("player model transform absent"))?;
        let visibility = match world.get::<Visibility>(root) {
            Some(Visibility::Inherited) => 0,
            Some(Visibility::Hidden) => 1,
            Some(Visibility::Visible) => 2,
            None => anyhow::bail!("player model visibility absent"),
        };
        Ok(Some(Self {
            version: 1,
            root: root.to_bits(),
            model: pose.model.clone(),
            frame: pose.frame,
            source: SceneSourceIdentity::capture(world, root)?,
            transform,
            visibility,
        }))
    }

    pub fn prepare(
        self,
        prepared: &PreparedWorld,
        root: Entity,
        player: Entity,
        world: &World,
    ) -> Result<PlayerModelCandidate> {
        ensure!(
            self.version == 1,
            "player model snapshot version unsupported"
        );
        ensure!(
            !self.model.is_empty()
                && self
                    .model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "player model source key invalid"
        );
        ensure!(
            self.visibility <= 2
                && self.transform.translation.is_finite()
                && self.transform.scale.is_finite()
                && self.transform.rotation.is_normalized(),
            "player model transform/visibility invalid"
        );
        ensure!(
            world
                .get_resource::<Assets<cyrus_assets::animation::Animations>>()
                .and_then(|assets| assets.get(&prepared.animations))
                .and_then(|data| data.models.get(&self.model))
                .is_some_and(|model| self.frame < model.positions.len()),
            "player model {} frame {} absent from prepared animation source",
            self.model,
            self.frame
        );
        let source = self.source_path(prepared)?;
        let asset = world
            .get_resource::<AssetServer>()
            .and_then(|server| {
                server.get_handle::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(source.clone()))
            })
            .ok_or_else(|| anyhow::anyhow!("player model {}: asset not prepared", self.model))?;
        let nodes = model_nodes(world, root, &self.model)?;
        let base_meshes = world
            .get::<Children>(player)
            .into_iter()
            .flatten()
            .filter(|entity| world.get::<Mesh3d>(**entity).is_some())
            .map(|entity| {
                Ok((
                    *entity,
                    *world
                        .get::<Visibility>(*entity)
                        .ok_or_else(|| anyhow::anyhow!("actor base mesh visibility absent"))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let frames = crate::animation::PreparedModelFrames::prepare(
            world,
            &nodes,
            &prepared.animations,
            &AnimationPose {
                model: self.model.clone(),
                frame: self.frame,
            },
        )?;
        let result = PlayerModelCandidate {
            root,
            player,
            source,
            asset,
            saved: self,
            nodes,
            base_meshes,
            frames,
        };
        result.validate(world)?;
        Ok(result)
    }
}

impl PlayerModelCandidate {
    pub fn root(&self) -> Entity {
        self.root
    }
    pub fn validate(&self, world: &World) -> Result<()> {
        ready(world, self.root)?;
        ensure!(
            world.get_entity(self.player).is_ok() && self.root != self.player,
            "player model replacement player absent or aliased"
        );
        ensure!(
            world.get::<ModelRoot>(self.root).is_none()
                && world.get::<Visibility>(self.root) == Some(&Visibility::Hidden),
            "player model replacement already live or visible"
        );
        ensure!(
            world
                .get::<WorldAssetRoot>(self.root)
                .is_some_and(|root| root.0 == self.asset),
            "player model replacement asset differs"
        );
        ensure!(
            model_nodes(world, self.root, &self.saved.model)? == self.nodes,
            "player model replacement node census changed"
        );
        for (entity, visibility) in &self.base_meshes {
            ensure!(
                world.get::<Mesh3d>(*entity).is_some()
                    && world
                        .get::<ChildOf>(*entity)
                        .is_some_and(|parent| parent.parent() == self.player)
                    && world.get::<Visibility>(*entity) == Some(visibility)
                    && world.get::<BaseModelVisibility>(*entity).is_none(),
                "actor model replacement base mesh changed"
            );
        }
        self.frames.validate(world)?;
        self.saved.source.validate(world, self.root)
    }
    pub fn queue(self, commands: &mut Commands) {
        self.frames.queue(commands);
        for (entity, visibility) in self.base_meshes {
            commands
                .entity(entity)
                .insert((BaseModelVisibility(visibility), Visibility::Hidden));
        }
        for node in self.nodes {
            commands.entity(node).insert(AnimationPose {
                model: self.saved.model.clone(),
                frame: self.saved.frame,
            });
        }
        let visibility = match self.saved.visibility {
            0 => Visibility::Inherited,
            1 => Visibility::Hidden,
            _ => Visibility::Visible,
        };
        commands.entity(self.root).insert((
            ModelRoot {
                player: self.player,
                source: self.source,
                model: self.saved.model,
            },
            self.saved.transform,
            visibility,
            ChildOf(self.player),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adopted_model_survives_binding_updates_without_reload() -> Result<()> {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), PlayerModelPlugin));
        let prepared = PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(17),
            animations: Handle::default(),
            scripts: Handle::default(),
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        };
        let source = model_source(&prepared, "TEST");
        app.insert_resource(prepared);
        let player = app
            .world_mut()
            .spawn((
                Player,
                AnimationPose {
                    model: "TEST".into(),
                    frame: 1,
                },
            ))
            .id();
        let root = app.world_mut().spawn(Visibility::Hidden).id();
        let node = app
            .world_mut()
            .spawn((Name::new("TEST"), ChildOf(root)))
            .id();
        let identity = serde_json::from_value(
            serde_json::json!({"digest":vec![0;32],"error":null,"scene":0}),
        )?;
        let candidate = PlayerModelCandidate {
            root,
            player,
            source,
            asset: Handle::default(),
            nodes: vec![node],
            base_meshes: vec![],
            frames: Default::default(),
            saved: PlayerModelSnapshot {
                version: 1,
                root: 0,
                model: "TEST".into(),
                frame: 1,
                source: identity,
                transform: Transform::from_xyz(1.0, 2.0, 3.0),
                visibility: 1,
            },
        };
        let mut queue = bevy::ecs::world::CommandQueue::default();
        candidate.queue(&mut Commands::new(&mut queue, app.world()));
        queue.apply(app.world_mut());
        app.update();
        app.update();
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), player);
        assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 1);
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<ModelRoot>>()
                .iter(app.world())
                .collect::<Vec<_>>(),
            [root]
        );
        Ok(())
    }

    #[test]
    fn loaded_model_round_trips_through_public_preparation() -> Result<()> {
        loaded_model_round_trip(false)?;
        loaded_model_round_trip(true)
    }

    fn loaded_model_round_trip(map_actor: bool) -> Result<()> {
        use bevy::{
            asset::io::{
                AssetSourceBuilder, AssetSourceId,
                memory::{Dir, MemoryAssetReader},
            },
            gltf::GltfPlugin,
            mesh::MeshPlugin,
            world_serialization::WorldSerializationPlugin,
        };
        let directory = Dir::default();
        directory.insert_asset_text(
            std::path::Path::new("test.player-models/TEST.glb"),
            &serde_json::json!({"asset":{"version":"2.0"},
                "nodes":[{"name":"TEST"}],"scenes":[{"nodes":[0]}],"scene":0})
            .to_string(),
        );
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
            MeshPlugin,
            GltfPlugin::default(),
            crate::persistence::SceneIdentityPlugin,
            PlayerModelPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<cyrus_assets::animation::Animations>();
        app.finish();
        app.cleanup();
        let animations: cyrus_assets::animation::Animations = serde_json::from_value(
            serde_json::json!({
                "version":2,"world":17,"metadata":{},"sections":{},
                "models":{"TEST":{"base_positions":[[0.0,0.0,0.0],[-20.0,0.0,0.0],[0.0,-20.0,0.0]],
                    "base_normals":[[0.0,0.0,1.0]], "faces":[[0,1,2]],"encoding":"animated_i16_absolute",
                    "positions":[[[0.0,0.0,0.0],[-20.0,0.0,0.0],[0.0,-20.0,0.0]],
                                 [[0.0,0.0,40.0],[-20.0,0.0,40.0],[0.0,-20.0,40.0]]],
                    "normals":[[[0.0,0.0,1.0]],[[0.0,0.0,1.0]]]}},
                "unsupported":{}
            }),
        )?;
        let animations = app
            .world_mut()
            .resource_mut::<Assets<cyrus_assets::animation::Animations>>()
            .add(animations);
        let prepared = PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(17),
            animations,
            scripts: Handle::default(),
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        };
        app.insert_resource(prepared);
        let player = app
            .world_mut()
            .spawn((
                Player,
                AnimationPose {
                    model: "TEST".into(),
                    frame: 1,
                },
            ))
            .id();
        if map_actor {
            app.world_mut()
                .entity_mut(player)
                .remove::<Player>()
                .insert((
                    Name::new("B_001_ACTOR"),
                    ScriptBinding {
                        actor: cyrus_game::script::ScriptActorId {
                            generation: 1,
                            index: 1,
                        },
                        source: Handle::default(),
                    },
                ));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let saved = loop {
            app.update();
            if let Ok(Some(saved)) = PlayerModelSnapshot::capture(app.world_mut(), player) {
                break saved;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "live model failed to settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        let old_root = Entity::from_bits(saved.saved_root());
        let handle = app
            .world()
            .get::<WorldAssetRoot>(old_root)
            .unwrap()
            .0
            .clone();
        let replacement_player = app.world_mut().spawn_empty().id();
        let replacement = app
            .world_mut()
            .spawn((
                WorldAssetRoot(handle),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while ready(app.world(), replacement).is_err() {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "replacement model failed to settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        let original_mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let node = model_nodes(app.world(), replacement, "TEST")?[0];
        let primitive = app
            .world_mut()
            .spawn((
                Mesh3d(original_mesh.clone()),
                ChildOf(node),
                bevy::gltf::GltfExtras {
                    value: serde_json::json!({"rgpre_geometry": {
                        "version": 1, "source_vertices": [[0,0],[1,0],[2,0]]
                    }})
                    .to_string(),
                },
            ))
            .id();
        let candidate = saved.clone().prepare(
            app.world().resource::<PreparedWorld>(),
            replacement,
            replacement_player,
            app.world(),
        )?;
        let mut malformed = serde_json::to_value(&saved)?;
        malformed["frame"] = serde_json::json!(2);
        assert!(
            serde_json::from_value::<PlayerModelSnapshot>(malformed)?
                .prepare(
                    app.world().resource::<PreparedWorld>(),
                    replacement,
                    replacement_player,
                    app.world()
                )
                .is_err()
        );
        assert!(app.world().get::<ModelRoot>(replacement).is_none());
        assert_eq!(
            app.world().get::<Visibility>(replacement),
            Some(&Visibility::Hidden)
        );
        app.world_mut().resource_mut::<PreparedWorld>().scene_path = "other.glb".into();
        assert!(
            PlayerModelSnapshot::capture(app.world_mut(), player)
                .err()
                .expect("wrong source must reject")
                .to_string()
                .contains("source differs")
        );
        app.world_mut().resource_mut::<PreparedWorld>().scene_path = "test.glb".into();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        candidate.queue(&mut Commands::new(&mut queue, app.world()));
        queue.apply(app.world_mut());
        let restored_mesh = &app.world().get::<Mesh3d>(primitive).unwrap().0;
        assert_ne!(restored_mesh, &original_mesh);
        let meshes = app.world().resource::<Assets<Mesh>>();
        assert_eq!(
            meshes
                .get(restored_mesh)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION),
            Some(&bevy::mesh::VertexAttributeValues::Float32x3(vec![
                [0.0, 0.0, 2.0],
                [1.0, 0.0, 2.0],
                [0.0, 1.0, 2.0]
            ]))
        );
        assert_eq!(
            meshes
                .get(&original_mesh)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION),
            Some(&bevy::mesh::VertexAttributeValues::Float32x3(vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0]
            ]))
        );
        app.world_mut().despawn(player);
        app.world_mut().entity_mut(replacement_player).insert((
            Player,
            AnimationPose {
                model: "TEST".into(),
                frame: 1,
            },
        ));
        if map_actor {
            app.world_mut()
                .entity_mut(replacement_player)
                .remove::<Player>()
                .insert((
                    Name::new("B_001_ACTOR"),
                    ScriptBinding {
                        actor: cyrus_game::script::ScriptActorId {
                            generation: 2,
                            index: 1,
                        },
                        source: Handle::default(),
                    },
                ));
        }
        app.update();
        app.update();
        let restored = PlayerModelSnapshot::capture(app.world_mut(), replacement_player)?
            .expect("restored model must remain bound");
        assert_eq!(restored.saved_root(), replacement.to_bits());
        assert_eq!(restored.model(), "TEST");
        assert_eq!(restored.frame(), 1);
        assert!(app.world().get_entity(old_root).is_err());
        Ok(())
    }
}
