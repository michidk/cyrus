//! Glide sky material: a BSI plane projected behind scene geometry, per preservation's sky docs.

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

#[derive(Component)]
pub(crate) struct Sky;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct SkyMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
    /// Engine level, UV half-span in texels, image width, image height.
    #[uniform(2)]
    pub plane: Vec4,
    /// Encoded Glide fog color shared with the scene background.
    #[uniform(3)]
    pub fog_rgb: Vec4,
}

impl Material for SkyMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_game/sky.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        Self::vertex_shader()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

pub(crate) struct SkyPlugin;
impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "sky.wesl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default());
    }
}
