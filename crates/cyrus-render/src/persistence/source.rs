//! Scene identity recorded by the same GLTF decode that creates render assets.
use anyhow::{Result, ensure};
use bevy::{
    asset::LoadContext,
    gltf::{
        GltfLoaderSettings,
        extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers},
    },
    prelude::*,
};
use serde::{Deserialize, Serialize};

/// Complete embedded scene inputs and loader settings, independent of runtime handles.
#[derive(Component, Clone, Reflect, PartialEq, Eq, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SceneSourceIdentity {
    digest: Option<[u8; 32]>,
    error: Option<String>,
    scene: usize,
}

impl SceneSourceIdentity {
    /// Find the loaded scene identity beneath a host-created instance root.
    pub fn capture(world: &World, root: Entity) -> Result<Self> {
        let mut pending = vec![root];
        let mut seen = std::collections::BTreeSet::new();
        let mut found = None;
        while let Some(entity) = pending.pop() {
            ensure!(
                seen.insert(entity),
                "scene source hierarchy repeats an entity"
            );
            ensure!(
                world.get_entity(entity).is_ok(),
                "scene source entity {entity:?} absent"
            );
            if let Some(identity) = world.get::<Self>(entity) {
                ensure!(
                    identity.digest.is_some() && identity.error.is_none(),
                    "scene source unavailable: {}",
                    identity.error.as_deref().unwrap_or("missing digest")
                );
                ensure!(
                    found.is_none(),
                    "multiple loaded scene sources under replacement root"
                );
                found = Some(identity.clone());
                continue;
            }
            if let Some(children) = world.get::<Children>(entity) {
                pending.extend(children.iter());
            }
        }
        anyhow::Context::context(
            found,
            "scene source identity absent; prepare with SceneIdentityPlugin",
        )
    }

    pub fn validate(&self, world: &World, root: Entity) -> Result<()> {
        ensure!(
            *self == Self::capture(world, root)?,
            "prepared static scene source differs"
        );
        Ok(())
    }
}

fn fingerprint(gltf: &gltf::Gltf, settings: &GltfLoaderSettings) -> Result<[u8; 32]> {
    for buffer in gltf.buffers() {
        if let gltf::buffer::Source::Uri(uri) = buffer.source() {
            ensure!(
                uri.starts_with("data:"),
                "scene buffer {} references external data",
                buffer.index()
            );
        }
    }
    for image in gltf.images() {
        if let gltf::image::Source::Uri { uri, .. } = image.source() {
            ensure!(
                uri.starts_with("data:"),
                "scene image {} references external data",
                image.index()
            );
        }
    }
    let mut hash = blake3::Hasher::new();
    hash.update(&serde_json::to_vec(&(gltf.document.as_json(), settings))?);
    if let Some(blob) = &gltf.blob {
        hash.update(blob);
    }
    Ok(*hash.finalize().as_bytes())
}

#[derive(Default)]
struct RetainIdentity(Option<Result<[u8; 32], String>>);
impl GltfExtensionHandler for RetainIdentity {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(Self::default())
    }
    fn on_root(
        &mut self,
        _: &mut LoadContext<'_>,
        gltf: &gltf::Gltf,
        settings: &GltfLoaderSettings,
    ) {
        self.0 = Some(fingerprint(gltf, settings).map_err(|error| format!("{error:#}")));
    }
    fn on_scene_completed(
        &mut self,
        _: &mut LoadContext<'_>,
        scene: &gltf::Scene,
        root: Entity,
        world: &mut World,
    ) {
        let result = self
            .0
            .clone()
            .unwrap_or_else(|| Err("scene source hook did not run".into()));
        let (digest, error) = match result {
            Ok(hash) => (Some(hash), None),
            Err(error) => (None, Some(error)),
        };
        world.entity_mut(root).insert(SceneSourceIdentity {
            digest,
            error,
            scene: scene.index(),
        });
    }
}

/// Register before loading scenes in live or off-live preparation worlds.
pub struct SceneIdentityPlugin;
impl Plugin for SceneIdentityPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<SceneSourceIdentity>()
            .init_resource::<GltfExtensionHandlers>();
        app.world()
            .resource::<GltfExtensionHandlers>()
            .0
            .try_write()
            .expect("scene identity registers before asset loading")
            .push(Box::new(RetainIdentity::default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_identity_covers_binary_settings_and_external_references() -> Result<()> {
        let json = serde_json::json!({"asset":{"version":"2.0"},"scenes":[{"nodes":[]}],"scene":0});
        let mut gltf = gltf::Gltf::from_slice(&serde_json::to_vec(&json)?)?;
        gltf.blob = Some(vec![1, 2, 3, 4]);
        let mut settings = GltfLoaderSettings::default();
        let original = fingerprint(&gltf, &settings)?;
        gltf.blob.as_mut().unwrap()[0] ^= 1;
        assert_ne!(original, fingerprint(&gltf, &settings)?);
        gltf.blob.as_mut().unwrap()[0] ^= 1;
        assert_eq!(original, fingerprint(&gltf, &settings)?);
        settings.load_cameras = !settings.load_cameras;
        assert_ne!(original, fingerprint(&gltf, &settings)?);
        let mut external = json;
        external["buffers"] = serde_json::json!([{"uri":"unverified.bin","byteLength":4}]);
        let gltf = gltf::Gltf::from_slice(&serde_json::to_vec(&external)?)?;
        assert!(fingerprint(&gltf, &settings).is_err());
        Ok(())
    }

    #[test]
    fn real_loader_attaches_scene_source_identity() -> Result<()> {
        use bevy::{
            asset::io::{
                AssetSourceBuilder, AssetSourceId,
                memory::{Dir, MemoryAssetReader},
            },
            gltf::{GltfAssetLabel, GltfPlugin},
            mesh::MeshPlugin,
            world_serialization::WorldSerializationPlugin,
        };
        let data = serde_json::json!({"asset":{"version":"2.0"},
            "nodes":[{"name":"Original","translation":[1,2,3]}],"scenes":[{"nodes":[0]}],"scene":0});
        let directory = Dir::default();
        directory.insert_asset_text(std::path::Path::new("source.gltf"), &data.to_string());
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
            SceneIdentityPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>();
        app.finish();
        app.cleanup();
        let server = app.world().resource::<AssetServer>().clone();
        let handle: Handle<WorldAsset> =
            server.load(GltfAssetLabel::Scene(0).from_asset("source.gltf"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app
            .world()
            .resource::<Assets<WorldAsset>>()
            .contains(handle.id())
        {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "scene failed to load: {:?}",
                server.load_state(handle.id())
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let mut assets = app.world_mut().resource_mut::<Assets<WorldAsset>>();
        let mut scene = assets.get_mut(&handle).unwrap();
        let root = scene
            .world
            .query_filtered::<Entity, With<SceneSourceIdentity>>()
            .single(&scene.world)?;
        let identity = SceneSourceIdentity::capture(&scene.world, root)?;
        identity.validate(&scene.world, root)?;
        let parsed = gltf::Gltf::from_slice(&serde_json::to_vec(&data)?)?;
        assert_eq!(
            identity.digest,
            Some(fingerprint(&parsed, &GltfLoaderSettings::default())?)
        );
        let mut wrong = identity.clone();
        wrong.scene = 1;
        assert!(wrong.validate(&scene.world, root).is_err());
        Ok(())
    }
}
