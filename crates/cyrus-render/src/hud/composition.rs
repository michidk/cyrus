//! Encoded framebuffer composition of inventory artwork and text against the live scene.
use super::{InventoryOverlay, artwork};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset, load_embedded_asset},
    core_pipeline::{
        FullscreenShader,
        schedule::{Core3d, Core3dSystems},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState, LoadOp,
            Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, StoreOp, TextureDimension, TextureFormat,
            TextureSampleType, binding_types::texture_2d,
        },
        renderer::{RenderContext, ViewQuery},
        texture::GpuImage,
        view::{ExtractedView, ViewTarget},
    },
    shader::Shader,
};

#[derive(Component, Clone, ExtractComponent)]
#[extract_app(RenderApp)]
#[extract_component_sync_target((Self, CompositionPipelineId))]
struct InventoryLayers {
    artwork: Handle<Image>,
    text: Handle<Image>,
}

pub(super) struct CompositionPlugin;
impl Plugin for CompositionPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "composition.wesl");
        app.add_plugins(ExtractComponentPlugin::<InventoryLayers>::default())
            .add_systems(PostUpdate, prepare_layers.after(artwork::store_artwork));
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render
            .init_resource::<SpecializedRenderPipelines<CompositionPipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                composite
                    .after(Core3dSystems::PostProcess)
                    .before(bevy::ui_render::render_pass::ui_pass),
            );
    }
}

fn new_image() -> Image {
    Image::new(
        Extent3d {
            width: 640,
            height: 480,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; 640 * 480 * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Flatten only layers with an opaque destination on the CPU. Translucent pixels over the
/// scene remain separate so the render pass can read their actual destination.
fn paint(destination: &mut [u8], source: &[u8], width: usize, at: [i32; 2]) {
    for (index, source) in source.as_chunks::<4>().0.iter().enumerate() {
        let x = at[0] + (index % width) as i32;
        let y = at[1] + (index / width) as i32;
        if !(0..640).contains(&x) || !(0..480).contains(&y) || source[3] == 0 {
            continue;
        }
        let target = &mut destination[(y as usize * 640 + x as usize) * 4..][..4];
        if source[3] == 255 {
            target.copy_from_slice(source);
        } else if target[3] == 255 {
            let colour = cyrus_assets::framebuffer::blend(
                target[..3].try_into().unwrap(),
                source[..3].try_into().unwrap(),
                u16::from(source[3]),
                x as u32,
                y as u32,
            );
            target[..3].copy_from_slice(&colour);
        }
    }
}

#[allow(clippy::too_many_arguments)] // The inventory owns UI layers; cameras own their rendered composition.
fn prepare_layers(
    mut commands: Commands,
    screen: Option<Res<cyrus_script::inventory_screen::InventoryScreen>>,
    state: Option<Res<State<cyrus_game::AppState>>>,
    next: Option<Res<NextState<cyrus_game::AppState>>>,
    mut roots: Query<(&Children, &mut Visibility), With<InventoryOverlay>>,
    nodes: Query<(&Node, Option<&ImageNode>, Option<&BackgroundColor>)>,
    cameras: Query<Entity, With<crate::camera::ViewCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut layers: Local<Option<InventoryLayers>>,
) {
    let active = screen.is_some_and(|screen| screen.open)
        && state.is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
        && !next
            .is_some_and(|next| matches!(*next, NextState::Pending(cyrus_game::AppState::Menu)));
    if !active {
        for camera in &cameras {
            commands.entity(camera).remove::<InventoryLayers>();
        }
        return;
    }
    let Ok((children, mut visibility)) = roots.single_mut() else {
        return;
    };
    let mut base = vec![0; 640 * 480 * 4];
    let mut text = vec![0; 640 * 480 * 4];
    for child in children.iter() {
        let Ok((node, image, background)) = nodes.get(child) else {
            continue;
        };
        let (Val::Percent(x), Val::Percent(y)) = (node.left, node.top) else {
            continue;
        };
        let at = [(x * 6.4).round() as i32, (y * 4.8).round() as i32];
        if let Some(image) = image {
            let Some(image) = images.get(&image.image) else {
                return;
            };
            let Some(pixels) = image.data.as_deref() else {
                return;
            };
            if image.width() == 640 && image.height() == 480 {
                text.copy_from_slice(pixels);
            } else {
                paint(&mut base, pixels, image.width() as usize, at);
            }
        } else if let Some(background) = background {
            let (Val::Percent(w), Val::Percent(h)) = (node.width, node.height) else {
                continue;
            };
            let colour = background.0.to_srgba().to_u8_array();
            for y in at[1].max(0)..(at[1] + (h * 4.8).round() as i32).min(480) {
                for x in at[0].max(0)..(at[0] + (w * 6.4).round() as i32).min(640) {
                    base[(y as usize * 640 + x as usize) * 4..][..4].copy_from_slice(&colour);
                }
            }
        }
    }
    // Share loading-fade arithmetic for text pixels whose artwork destination is known.
    for (index, source) in text.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let target = &mut base[index * 4..][..4];
        if source[3] != 0 && target[3] == 255 {
            if source[3] == 255 {
                target.copy_from_slice(source);
            } else {
                let colour = cyrus_assets::framebuffer::blend(
                    target[..3].try_into().unwrap(),
                    source[..3].try_into().unwrap(),
                    u16::from(source[3]),
                    index as u32 % 640,
                    index as u32 / 640,
                );
                target[..3].copy_from_slice(&colour);
            }
            source.fill(0);
        }
    }
    let handles = layers.get_or_insert_with(|| InventoryLayers {
        artwork: images.add(new_image()),
        text: images.add(new_image()),
    });
    images.get_mut(&handles.artwork).unwrap().data = Some(base);
    images.get_mut(&handles.text).unwrap().data = Some(text);
    *visibility = Visibility::Hidden;
    for camera in &cameras {
        commands.entity(camera).insert(handles.clone());
    }
}

#[derive(Resource)]
struct CompositionPipeline {
    layout: BindGroupLayoutDescriptor,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
}
fn init_pipeline(
    mut commands: Commands,
    fullscreen: Res<FullscreenShader>,
    assets: Res<AssetServer>,
) {
    let texture = || texture_2d(TextureSampleType::Float { filterable: false });
    commands.insert_resource(CompositionPipeline {
        layout: BindGroupLayoutDescriptor::new(
            "inventory_composition",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (texture(), texture(), texture()),
            ),
        ),
        fullscreen: fullscreen.clone(),
        shader: load_embedded_asset!(assets.as_ref(), "composition.wesl"),
    });
}
impl SpecializedRenderPipeline for CompositionPipeline {
    type Key = TextureFormat;
    fn specialize(&self, format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("inventory_composition".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
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
struct CompositionPipelineId(CachedRenderPipelineId);
fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<CompositionPipeline>>,
    pipeline: Res<CompositionPipeline>,
    views: Query<(Entity, &ExtractedView), With<InventoryLayers>>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(&cache, &pipeline, view.target_format);
        commands.entity(entity).insert(CompositionPipelineId(id));
    }
}
fn composite(
    view: ViewQuery<(&ViewTarget, &CompositionPipelineId, &InventoryLayers)>,
    pipeline: Res<CompositionPipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let (target, id, layers) = view.into_inner();
    let (Some(render_pipeline), Some(artwork), Some(text)) = (
        cache.get_render_pipeline(id.0),
        images.get(&layers.artwork),
        images.get(&layers.text),
    ) else {
        return;
    };
    let post = target.post_process_write();
    let bind = ctx.render_device().create_bind_group(
        Some("inventory_composition"),
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, &artwork.texture_view, &text.texture_view)),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("inventory_composition"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post.destination,
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
    pass.set_bind_group(0, &bind, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_uploads_encoded_shadow_then_removes_composition_when_closed() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .insert_resource(State::new(cyrus_game::AppState::InGame))
            .insert_resource(NextState::<cyrus_game::AppState>::Unchanged)
            .add_systems(Update, prepare_layers);
        let mut screen = cyrus_script::inventory_screen::InventoryScreen::default();
        screen.open = true;
        app.insert_resource(screen);
        let mut source = new_image();
        let index = (117 * 640 + 298) * 4;
        source.data.as_mut().unwrap()[index..index + 4].copy_from_slice(&[0, 0, 0, 199]);
        let mut artwork = Image::new(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![165, 117, 82, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        artwork.sampler = bevy::image::ImageSampler::nearest();
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(artwork);
        let text = app.world_mut().resource_mut::<Assets<Image>>().add(source);
        let root = app
            .world_mut()
            .spawn((InventoryOverlay, Visibility::Inherited))
            .id();
        app.world_mut().entity_mut(root).with_children(|parent| {
            parent.spawn((
                super::super::place([298, 117], [1, 1]),
                ImageNode::new(image),
            ));
            parent.spawn((
                super::super::place([0, 0], [640, 480]),
                ImageNode::new(text),
            ));
        });
        let camera = app.world_mut().spawn(crate::camera::ViewCamera).id();
        app.update();
        let layers = app.world().get::<InventoryLayers>(camera).unwrap();
        let images = app.world().resource::<Assets<Image>>();
        assert_eq!(
            &images.get(&layers.artwork).unwrap().data.as_ref().unwrap()[index..index + 4],
            &[41, 28, 16, 255]
        );
        assert_eq!(
            &images.get(&layers.text).unwrap().data.as_ref().unwrap()[index..index + 4],
            &[0; 4]
        );
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
        let count = images.len();
        app.update();
        assert_eq!(app.world().resource::<Assets<Image>>().len(), count);
        app.world_mut()
            .resource_mut::<cyrus_script::inventory_screen::InventoryScreen>()
            .open = false;
        app.update();
        assert!(app.world().get::<InventoryLayers>(camera).is_none());
    }
    #[test]
    fn shadow_uses_encoded_artwork_destination_and_preserves_transparent_world() {
        let mut rgba = vec![0; 640 * 480 * 4];
        let at = (117 * 640 + 298) * 4;
        rgba[at..at + 4].copy_from_slice(&[165, 117, 82, 255]);
        super::paint(&mut rgba, &[0, 0, 0, 199], 1, [298, 117]);
        assert_eq!(&rgba[at..at + 4], &[41, 28, 16, 255]);
        assert_eq!(&rgba[..4], &[0; 4]);
    }
}
