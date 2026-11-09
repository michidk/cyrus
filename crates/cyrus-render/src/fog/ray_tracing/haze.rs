//! The scene fog over the traced image.
//!
//! The stand-ins Solari lights draw with plain PBR materials, which have no fog, while the meshes
//! that keep their own shaders, such as the chroma-keyed palm leaves, fog themselves. This pass fogs
//! the traced pixels as the scene materials fog theirs, after Solari lights them and before the
//! forward-drawn meshes draw over them, so the whole scene fades with distance together.
use super::super::SceneFog;
use crate::camera::ViewCamera;
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        FullscreenShader,
        core_3d::main_opaque_pass_3d,
        prepass::ViewPrepassTextures,
        schedule::{Core3d, Core3dSystems},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, LoadOp,
            Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
            SpecializedRenderPipelines, StoreOp, TextureFormat, TextureSampleType,
            binding_types::{texture_2d, texture_depth_2d, uniform_buffer},
        },
        renderer::{RenderContext, ViewQuery},
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
    shader::Shader,
    solari::realtime::{SolariLighting, solari_lighting},
};

/// The fog values of the scene materials, on a camera that Solari lights.
#[derive(Component, Clone, Copy, Default, PartialEq, ShaderType, ExtractComponent)]
#[extract_app(RenderApp)]
#[extract_component_filter(With<SolariLighting>)]
#[extract_component_sync_target((Self, HazePipelineId))]
pub(super) struct TracedFog {
    color: Vec4,
    clipping: Vec4,
    table: [Vec4; 16],
}

/// Gives the traced cameras the scene materials' current fog.
pub(super) fn update_traced_fog(
    mut commands: Commands,
    scene_fog: Res<SceneFog>,
    mut cameras: Query<(Entity, Option<&mut TracedFog>), With<ViewCamera>>,
) {
    let Some(extension) = &scene_fog.0 else {
        return;
    };
    let fog = TracedFog {
        color: extension.color,
        clipping: extension.clipping,
        table: extension.table,
    };
    for (camera, current) in &mut cameras {
        match current {
            Some(mut current) => {
                current.set_if_neq(fog);
            }
            None => {
                commands.entity(camera).insert(fog);
            }
        }
    }
}

pub(super) struct HazePlugin;

impl Plugin for HazePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "traced_fog.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<TracedFog>::default(),
            UniformComponentPlugin::<TracedFog>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<SpecializedRenderPipelines<HazePipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                traced_fog
                    .after(solari_lighting)
                    .before(main_opaque_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

#[derive(Resource)]
struct HazePipeline {
    layout: BindGroupLayoutDescriptor,
    fullscreen_shader: FullscreenShader,
    shader: Handle<Shader>,
}

fn init_pipeline(
    mut commands: Commands,
    fullscreen_shader: Res<FullscreenShader>,
    assets: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "traced_fog_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<TracedFog>(true),
            ),
        ),
    );
    commands.insert_resource(HazePipeline {
        layout,
        fullscreen_shader: fullscreen_shader.clone(),
        shader: load_embedded_asset!(assets.as_ref(), "traced_fog.wesl"),
    });
}

impl SpecializedRenderPipeline for HazePipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("traced_fog_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
pub(super) struct HazePipelineId(CachedRenderPipelineId);

#[allow(clippy::type_complexity)] // Bevy view query with two required component filters.
fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<HazePipeline>>,
    pipeline: Res<HazePipeline>,
    views: Query<(Entity, &ExtractedView), (With<TracedFog>, With<ExtractedCamera>)>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(&cache, &pipeline, view.target_format);
        commands.entity(entity).insert(HazePipelineId(id));
    }
}

/// Fogs the traced pixels: the main texture is read from one half and written fogged to the other.
#[allow(clippy::type_complexity)] // Render view and its dynamic uniform offsets travel together.
fn traced_fog(
    view: ViewQuery<(
        &ViewTarget,
        &HazePipelineId,
        &ViewPrepassTextures,
        &ViewUniformOffset,
        &DynamicUniformIndex<TracedFog>,
    )>,
    pipeline: Res<HazePipeline>,
    cache: Res<PipelineCache>,
    fogs: Res<ComponentUniforms<TracedFog>>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (target, pipeline_id, prepass, view_offset, fog_index) = view.into_inner();
    let (Some(render_pipeline), Some(depth), Some(fog), Some(view_binding)) = (
        cache.get_render_pipeline(pipeline_id.0),
        prepass.depth_only_view(),
        fogs.uniforms().binding(),
        view_uniforms.uniforms.binding(),
    ) else {
        return;
    };
    let post_process = target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        Some("traced_fog_bind_group"),
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post_process.source, depth, view_binding, fog)),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("traced_fog"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(default()),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[view_offset.offset, fog_index.index()]);
    pass.draw(0..3, 0..1);
}
