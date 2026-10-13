//! Retain authored texture frames during the original GLTF decode.
use bevy::{
    asset::LoadContext,
    gltf::extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers},
    prelude::*,
};
use std::collections::HashMap;

#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
pub(super) struct Frames(pub Vec<Handle<Image>>);

#[derive(Default)]
struct RetainFrames(HashMap<usize, Handle<Image>>);

impl GltfExtensionHandler for RetainFrames {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(Self::default())
    }

    fn on_texture(&mut self, texture: &gltf::Texture, image: Handle<Image>) {
        self.0.insert(texture.index(), image);
    }

    fn on_spawn_mesh_and_material(
        &mut self,
        context: &mut LoadContext<'_>,
        _: &gltf::Primitive,
        _: &gltf::Mesh,
        material: &gltf::Material,
        entity: &mut EntityWorldMut,
        _: &str,
    ) {
        let Some(extras) = material.extras() else {
            return;
        };
        let result = (|| -> anyhow::Result<()> {
            let Some(description) = serde_json::from_str::<super::MaterialExtras>(extras.get())?
                .rgpre_texture_animation
            else {
                return Ok(());
            };
            let frames = description
                .textures
                .iter()
                .map(|index| {
                    self.0
                        .get(index)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("missing texture frame {index}"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            entity.insert(Frames(frames));
            Ok(())
        })();
        if let Err(error) = result {
            error!(
                "GLTF {}, material {:?}: texture frames unavailable: {error:#}",
                context.path(),
                material.index()
            );
        }
    }
}

pub(super) fn register(app: &mut App) {
    app.register_type::<Frames>()
        .init_resource::<GltfExtensionHandlers>();
    app.world()
        .resource::<GltfExtensionHandlers>()
        .0
        .try_write()
        .expect("texture frame retention registers before asset loading")
        .push(Box::new(RetainFrames::default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::{
            AssetPlugin,
            io::{
                AssetSourceBuilder, AssetSourceId,
                memory::{Dir, MemoryAssetReader},
            },
        },
        gltf::{GltfAssetLabel, GltfPlugin},
        mesh::MeshPlugin,
        world_serialization::WorldSerializationPlugin,
    };

    #[test]
    fn scene_retains_unused_animation_frames_in_authored_order() {
        let data = serde_json::json!({
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}],
            "bufferViews": [{"buffer":0,"byteLength":36}],
            "accessors": [{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[0,0,0]}],
            "images": [{"uri":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=="}],
            "textures": [{"source":0},{"source":0},{"source":0},{"source":0}],
            "materials": [{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}},"extras":{"rgpre_texture_animation":{"version":1,"image_name":"A00000","delay_ms":100,"textures":[3,1,2,0]}}}],
            "meshes": [{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
            "nodes": [{"mesh":0}],
            "scenes": [{"nodes":[0]}],
            "scene": 0
        });
        let directory = Dir::default();
        directory.insert_asset_text(std::path::Path::new("frames.gltf"), &data.to_string());
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
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>();
        register(&mut app);
        app.finish();
        app.cleanup();
        let server = app.world().resource::<AssetServer>().clone();
        let scene: Handle<WorldAsset> =
            server.load(GltfAssetLabel::Scene(0).from_asset("frames.gltf"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app
            .world()
            .resource::<Assets<WorldAsset>>()
            .contains(scene.id())
        {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "GLTF scene did not load: {:?}",
                server.load_state(scene.id())
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for _ in 0..10 {
            app.update();
        }
        let frames = {
            let mut scenes = app.world_mut().resource_mut::<Assets<WorldAsset>>();
            let mut scene = scenes.get_mut(&scene).unwrap();
            scene
                .world
                .query::<&Frames>()
                .single(&scene.world)
                .unwrap()
                .0
                .clone()
        };
        let images = app.world().resource::<Assets<Image>>();
        for (frame, index) in frames.iter().zip([3, 1, 2, 0]) {
            assert!(images.contains(frame.id()));
            assert_eq!(
                frame.path().unwrap().label(),
                Some(format!("Texture{index}").as_str())
            );
        }
        assert_eq!(frames.len(), 4);
    }
}
