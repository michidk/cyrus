//! The modern lighting's own screen pass: screen-space reflections on water and the sun's lens
//! flare and glare.
//!
//! **Modern only**, a tuning choice throughout. Under modern lighting the terrain writes its
//! water weight into the colour's alpha (`terrain.wesl`); this pass, after the main pass,
//! marches each water pixel's reflected ray through the depth buffer, falls back to the horizon
//! colour where the ray leaves the screen, adds the flare where the sun is on screen and not
//! covered, and restores an opaque alpha. It runs on every modern-lit camera, also with both
//! effects off, so the water weight never reaches the screen.
use super::{day_night::Daylight, effects::ModernEffects, lights::Sun};
use crate::{camera::ViewCamera, environment::EnvironmentRequest, sky::SkyMaterial};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        FullscreenShader,
        core_3d::main_transparent_pass_3d,
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
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
            LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, SpecializedRenderPipeline, SpecializedRenderPipelines, StoreOp,
            TextureFormat, TextureSampleType, TextureUsages,
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{
            ExtractedView, Msaa, ViewDepthStencilTexture, ViewTarget, ViewUniform,
            ViewUniformOffset, ViewUniforms, prepare_view_targets,
        },
    },
    shader::Shader,
};
use cyrus_assets::environment::Environment;

/// The pass's settings for one camera; see `modern_post.wesl`.
#[derive(Component, Clone, Copy, Default, PartialEq, ShaderType, ExtractComponent)]
#[extract_app(RenderApp)]
#[extract_component_filter(With<Camera>)]
#[extract_component_sync_target((Self, PostPipelineId))]
pub(super) struct ModernPost {
    /// The sun's screen position as a texture coordinate, 1 while it is in front of the camera,
    /// and the flare's strength (0 turns it off).
    sun: Vec4,
    /// Linear sun colour.
    sun_color: Vec4,
    /// Linear horizon colour reflected where a ray leaves the screen, and the reflections'
    /// strength (0 turns them off).
    horizon: Vec4,
    /// Seconds as the scene shaders read them, for the water's ripples, and the modern waves'
    /// amplitude and grid cell size in world units (both 0 without them; see `fog::modern_waves`).
    time: Vec4,
}

/// Distance at which the sun is projected onto the screen, in world units.
const SUN_DISTANCE: f32 = 1000.0;

/// The sun's direction as the player sees it: the sky's sun disc where the sky has one, else
/// the sun light.
fn sun_direction(sky: Option<&SkyMaterial>, light: Option<&GlobalTransform>) -> Option<Vec3> {
    sky.filter(|sky| sky.sun_position.w > 0.0)
        .and_then(|sky| {
            let rotation = Quat::from_vec4(sky.day_night_sun).normalize();
            (rotation * sky.sun_position.truncate()).try_normalize()
        })
        .or_else(|| light.and_then(|light| (-light.forward().as_vec3()).try_normalize()))
}

/// Updates every modern-lit camera's pass settings.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_post(
    mut commands: Commands,
    effects: Res<ModernEffects>,
    (time, fidelity): (Res<Time>, Res<cyrus_game::fidelity::Fidelity>),
    daylight: Res<Daylight>,
    environment: (Option<Res<EnvironmentRequest>>, Res<Assets<Environment>>),
    skies: (
        Query<&MeshMaterial3d<SkyMaterial>>,
        Res<Assets<SkyMaterial>>,
    ),
    suns: Query<(&DirectionalLight, &GlobalTransform), With<Sun>>,
    mut cameras: Query<
        (Entity, &Camera, &GlobalTransform, Option<&mut ModernPost>),
        With<ViewCamera>,
    >,
) {
    let (request, environments) = environment;
    let (sky_handles, sky_materials) = skies;
    let sky = sky_handles
        .iter()
        .find_map(|handle| sky_materials.get(&handle.0));
    let sun = suns.iter().next();
    let direction = sun_direction(sky, sun.map(|(_, transform)| transform));
    // Hidden below the horizon by the day/night cycle, and absent without a sun.
    let visibility = match (direction, daylight.0) {
        (None, _) => 0.0,
        (Some(_), Some(cycle)) => cycle.sun_disc,
        (Some(_), None) => 1.0,
    };
    let sun_color = sun.map_or(Vec3::ZERO, |(light, _)| {
        Vec3::from_array(light.color.to_linear().to_f32_array_no_alpha())
    });
    let tint = daylight.0.map_or(Vec3::ONE, |cycle| cycle.tint);
    let environment = request.and_then(|request| environments.get(&request.0));
    let waves = environment.map_or(Vec4::ZERO, |environment| {
        crate::fog::modern_waves(environment, &fidelity)
    });
    let horizon = environment.map_or(Vec3::ZERO, |environment| {
        let encoded = Vec3::from_array(environment.background.map(f32::from)) / 255.0 * tint;
        Vec3::from_array(
            Color::srgb(encoded.x, encoded.y, encoded.z)
                .to_linear()
                .to_f32_array_no_alpha(),
        )
    });
    for (entity, camera, transform, post) in &mut cameras {
        let screen = direction.and_then(|direction| {
            let in_front = direction.dot(transform.forward().as_vec3()) > 0.0;
            let point = transform.translation() + direction * SUN_DISTANCE;
            let ndc = camera.world_to_ndc(transform, point)?;
            Some(Vec4::new(
                ndc.x * 0.5 + 0.5,
                0.5 - ndc.y * 0.5,
                f32::from(u8::from(in_front)),
                if effects.lens_flare { visibility } else { 0.0 },
            ))
        });
        let settings = ModernPost {
            sun: screen.unwrap_or(Vec4::ZERO),
            sun_color: sun_color.extend(0.0),
            horizon: horizon.extend(f32::from(u8::from(effects.water_reflections))),
            time: Vec4::new(time.elapsed_secs_wrapped(), waves.z, waves.w, 0.0),
        };
        match post {
            Some(mut post) => {
                post.set_if_neq(settings);
            }
            None => {
                commands.entity(entity).insert(settings);
            }
        }
    }
}

/// Renders the pass; add with the modern lighting plugin.
pub(super) struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "modern_post.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<ModernPost>::default(),
            UniformComponentPlugin::<ModernPost>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<SpecializedRenderPipelines<PostPipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(
                Render,
                (
                    prepare_pipelines.in_set(RenderSystems::Prepare),
                    sample_depth
                        .in_set(RenderSystems::PrepareViews)
                        .after(prepare_view_targets)
                        .ambiguous_with_all(),
                ),
            )
            // Before the volumetric fog, which follows the main pass, so it sees the opaque
            // scene and the water weight in the alpha.
            .add_systems(
                Core3d,
                modern_post
                    .after(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

#[derive(Resource)]
struct PostPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
    shader: Handle<Shader>,
}

fn init_pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    assets: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "modern_post_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<ModernPost>(true),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(PostPipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
        shader: load_embedded_asset!(assets.as_ref(), "modern_post.wesl"),
    });
}

impl SpecializedRenderPipeline for PostPipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("modern_post_pipeline".into()),
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
pub(super) struct PostPipelineId(CachedRenderPipelineId);

// Render-world views with this pass, and their pipelines and inputs.
#[allow(clippy::type_complexity)]
fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<PostPipeline>>,
    pipeline: Res<PostPipeline>,
    views: Query<(Entity, &ExtractedView), (With<ModernPost>, With<ExtractedCamera>)>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(&cache, &pipeline, view.target_format);
        commands.entity(entity).insert(PostPipelineId(id));
    }
}

/// The pass reads the main depth buffer.
fn sample_depth(mut cameras: Query<&mut Camera3d, With<ModernPost>>) {
    for mut camera in &mut cameras {
        camera.depth_texture_usages.0 |= TextureUsages::TEXTURE_BINDING.bits();
    }
}

#[allow(clippy::type_complexity)]
fn modern_post(
    view: ViewQuery<(
        &ViewTarget,
        &PostPipelineId,
        &ViewDepthStencilTexture,
        &ViewUniformOffset,
        &DynamicUniformIndex<ModernPost>,
        &Msaa,
    )>,
    pipeline: Res<PostPipeline>,
    cache: Res<PipelineCache>,
    settings: Res<ComponentUniforms<ModernPost>>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (target, pipeline_id, depth, view_offset, settings_index, msaa) = view.into_inner();
    // The view camera never multisamples.
    if *msaa != Msaa::Off {
        return;
    }
    let (Some(render_pipeline), Some(depth), Some(settings), Some(view_binding)) = (
        cache.get_render_pipeline(pipeline_id.0),
        depth.attachment.depth_stencil_views().depth_only_view(),
        settings.uniforms().binding(),
        view_uniforms.uniforms.binding(),
    ) else {
        return;
    };
    let post_process = target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        Some("modern_post_bind_group"),
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline.sampler,
            depth,
            view_binding,
            settings,
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("modern_post"),
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
    pass.set_bind_group(
        0,
        &bind_group,
        &[view_offset.offset, settings_index.index()],
    );
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sky_sun_follows_the_day_night_rotation() {
        let mut sky = SkyMaterial {
            image: default(),
            plane: Vec4::ZERO,
            fog_rgb: Vec4::ZERO,
            scroll: Vec4::ZERO,
            sun_image: default(),
            sun_position: Vec4::new(0.0, 100.0, -100.0, 5.0),
            sun_color: Vec4::ONE,
            day_night: SkyMaterial::NEUTRAL_DAY_NIGHT[0],
            day_night_sun: SkyMaterial::NEUTRAL_DAY_NIGHT[1],
            modern_sky: Vec4::ZERO,
        };
        let noon = sun_direction(Some(&sky), None).unwrap();
        assert!(noon.abs_diff_eq(Vec3::new(0.0, 1.0, -1.0).normalize(), 1e-5));
        let quarter = Quat::from_rotation_x(std::f32::consts::FRAC_PI_4);
        sky.day_night_sun = Vec4::from_array(quarter.to_array());
        let turned = sun_direction(Some(&sky), None).unwrap();
        assert!(turned.abs_diff_eq(Vec3::Y, 1e-5), "{turned}");
    }

    #[test]
    fn without_a_sky_sun_the_light_is_used() {
        let light = GlobalTransform::from(Transform::IDENTITY.looking_to(Vec3::NEG_Y, Vec3::Z));
        assert!(
            sun_direction(None, Some(&light))
                .unwrap()
                .abs_diff_eq(Vec3::Y, 1e-5)
        );
    }
}
