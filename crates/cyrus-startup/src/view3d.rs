//! Shared GPU drawing for the screens that show textured models in the original's fixed 640 by 480
//! frame: the book menu, the logbook scrolls and the examine view. Each renders its meshes through a
//! perspective camera into a 640 by 480 image that the screen shows like any other picture.
//!
//! Original engine coordinates have X right, Y down and Z forward. Bevy's have Y up and Z toward the
//! viewer, so every point maps `(x, y, z)` to `(x, -y, -z)`, a half turn about X that keeps
//! handedness and winding.
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::{
        ClearColorConfig, RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
        TextureDimension, TextureFormat, TextureUsages,
    },
    shader::ShaderRef,
};
use std::collections::BTreeMap;

/// Width and height of every view's render target, the original's frame size.
pub(super) const WIDTH: u32 = 640;
pub(super) const HEIGHT: u32 = 480;
/// Horizontal half-field tangent of the original's viewport; the vertical one follows from 4:3.
const HALF_FIELD_TANGENT: f32 = 0.788;

/// Marks a camera that renders a screen's own 3D scene into an image, so the systems that pause or
/// restore the gameplay cameras leave it alone.
#[derive(Component)]
pub struct OffscreenView;

/// The cameras of the running game, not a screen's own offscreen view.
pub(super) type GameplayCameras = (With<Camera3d>, Without<OffscreenView>);

/// The asset stores a lit view draws into. The meshes and materials are absent in hosts without a
/// renderer, which then draw nothing.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct LitAssets<'w> {
    meshes: Option<ResMut<'w, Assets<Mesh>>>,
    materials: Option<ResMut<'w, Assets<LitMaterial>>>,
    pub images: ResMut<'w, Assets<Image>>,
}

/// A point in Bevy's frame from original engine coordinates.
pub(super) fn to_bevy(point: [f32; 3]) -> [f32; 3] {
    [point[0], -point[1], -point[2]]
}

/// A texture image: bilinear with wrap-around addressing and no mip levels.
pub(super) fn texture_image(width: u16, height: u16, rgba: Vec<u8>) -> Image {
    texture_image_in(
        width.into(),
        height.into(),
        rgba,
        TextureFormat::Rgba8UnormSrgb,
    )
}

/// [`texture_image`] with the texel bytes read as they are, not as sRGB, so a shader that mixes them
/// does so in the 8-bit space the original's hardware used.
pub(super) fn raw_texture_image(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    texture_image_in(width, height, rgba, TextureFormat::Rgba8Unorm)
}

fn texture_image_in(width: u32, height: u32, rgba: Vec<u8>, format: TextureFormat) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        format,
        RenderAssetUsages::default(),
    );
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.address_mode_u = ImageAddressMode::Repeat;
    sampler.address_mode_v = ImageAddressMode::Repeat;
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

/// An empty 640 by 480 image a view camera renders into.
pub(super) fn target_image(fill: [u8; 4]) -> Image {
    target_image_sized(WIDTH, HEIGHT, fill)
}

/// An empty image of any size a view camera renders into.
pub(super) fn target_image_sized(width: u32, height: u32, fill: [u8; 4]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &fill,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image
}

/// The original's perspective projection at 640 by 480.
pub(super) fn projection() -> Projection {
    projection_for(HALF_FIELD_TANGENT)
}

/// A 4:3 perspective projection with the given horizontal half-field tangent.
pub(super) fn projection_for(horizontal_tangent: f32) -> Projection {
    let tangent = horizontal_tangent * 3.0 / 4.0;
    Projection::Perspective(PerspectiveProjection {
        fov: 2.0 * tangent.atan(),
        aspect_ratio: 4.0 / 3.0,
        near: 0.5,
        far: 100_000.0,
        ..default()
    })
}

/// A camera that draws `layer` into `target` before the main cameras. `clear` is the target's
/// background; `Color::NONE` leaves everything the scene does not cover transparent.
pub(super) fn view_camera(target: Handle<Image>, layer: usize, clear: Color) -> impl Bundle {
    (
        OffscreenView,
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(clear),
            ..default()
        },
        RenderTarget::Image(target.into()),
        projection(),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(layer),
        Transform::default(),
    )
}

/// One lit triangle in original view space: every corner's position, texel UV and per-channel
/// light level (0 to 255), and the constant added to each level before the 256ths divide.
pub(super) struct LitFace {
    pub vertices: [[f32; 3]; 3],
    pub uv: [[f32; 2]; 3],
    pub levels: [[f32; 3]; 3],
    /// 1 for a textured face (`floor(T * (L + 1) / 256)`), 0 for a solid colour
    /// (`floor(C * L / 256)`).
    pub bias: f32,
    pub texture: usize,
}

/// The triangles of one view and the RGBA textures they index.
#[derive(Default)]
pub(super) struct LitScene {
    pub textures: Vec<(u16, u16, Vec<u8>)>,
    pub faces: Vec<LitFace>,
    solid: BTreeMap<[u8; 4], usize>,
}

impl LitScene {
    /// The index of a one-texel texture of `colour`, shared by every face of that colour.
    pub fn solid_texture(&mut self, colour: [u8; 4]) -> usize {
        *self.solid.entry(colour).or_insert_with(|| {
            self.textures.push((1, 1, colour.to_vec()));
            self.textures.len() - 1
        })
    }
}

/// A texture's lit material: the level multiplies the texel's 8-bit channels as the original's
/// Glide colour combine does.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct LitMaterial {
    #[texture(0)]
    #[sampler(1)]
    texture: Handle<Image>,
}

impl Material for LitMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_startup/lit.wesl".into()
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
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?];
        Ok(())
    }
}

/// Registers [`LitMaterial`] and its shader.
pub struct LitViewPlugin;
impl Plugin for LitViewPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "lit.wesl");
        app.add_plugins(MaterialPlugin::<LitMaterial>::default());
    }
}

/// The mesh of every face that uses `texture`, in the order `scene.faces` lists them.
fn texture_mesh(scene: &LitScene, texture: usize) -> Mesh {
    let (width, height, _) = &scene.textures[texture];
    let (mut positions, mut uvs, mut levels) = (Vec::new(), Vec::new(), Vec::new());
    for face in scene.faces.iter().filter(|face| face.texture == texture) {
        for corner in 0..3 {
            positions.push(to_bevy(face.vertices[corner]));
            uvs.push([
                face.uv[corner][0] / f32::from(*width),
                face.uv[corner][1] / f32::from(*height),
            ]);
            let [r, g, b] = face.levels[corner];
            levels.push([r, g, b, face.bias]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, levels)
}

/// The meshes a view currently shows, replaced whenever its scene changes.
#[derive(Default)]
pub(super) struct LitModel {
    entities: Vec<Entity>,
    /// The texture images, reused while `key` is unchanged.
    textures: Vec<Handle<Image>>,
    key: Option<u64>,
}

impl LitModel {
    /// Shows `scene` on `layer`. The textures are uploaded again only when `key` differs from the
    /// one the previous call passed, so a scene whose textures never change passes a constant.
    pub fn show(
        &mut self,
        commands: &mut Commands,
        assets: &mut LitAssets,
        scene: &LitScene,
        layer: usize,
        key: u64,
    ) {
        self.clear(commands);
        let LitAssets {
            meshes: Some(meshes),
            materials: Some(materials),
            images,
        } = assets
        else {
            return;
        };
        if self.key != Some(key) || self.textures.len() != scene.textures.len() {
            self.textures = scene
                .textures
                .iter()
                .map(|(width, height, rgba)| {
                    images.add(texture_image(*width, *height, rgba.clone()))
                })
                .collect();
            self.key = Some(key);
        }
        for (texture, handle) in self.textures.iter().enumerate() {
            if !scene.faces.iter().any(|face| face.texture == texture) {
                continue;
            }
            self.entities.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(texture_mesh(scene, texture))),
                        MeshMaterial3d(materials.add(LitMaterial {
                            texture: handle.clone(),
                        })),
                        Transform::IDENTITY,
                        NoFrustumCulling,
                        RenderLayers::layer(layer),
                    ))
                    .id(),
            );
        }
    }

    /// Removes the meshes and forgets the textures.
    pub fn clear(&mut self, commands: &mut Commands) {
        for entity in self.entities.drain(..) {
            commands.entity(entity).despawn();
        }
    }

    /// Whether any mesh is shown.
    pub fn is_shown(&self) -> bool {
        !self.entities.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn engine_points_map_to_bevy_with_y_and_z_flipped() {
        assert_eq!(to_bevy([1.0, 2.0, 3.0]), [1.0, -2.0, -3.0]);
    }

    #[test]
    fn solid_colours_share_one_texel_texture() {
        let mut scene = LitScene::default();
        let red = scene.solid_texture([255, 0, 0, 255]);
        assert_eq!(scene.solid_texture([255, 0, 0, 255]), red);
        assert_ne!(scene.solid_texture([0, 255, 0, 255]), red);
        assert_eq!(scene.textures.len(), 2);
    }

    #[test]
    fn a_texture_mesh_carries_levels_bias_and_normalised_uvs() {
        let mut scene = LitScene::default();
        scene.textures.push((4, 2, vec![0; 32]));
        scene.faces.push(LitFace {
            vertices: [[0.0, 0.0, 10.0], [0.0, 5.0, 10.0], [5.0, 0.0, 10.0]],
            uv: [[0.0, 0.0], [0.0, 2.0], [4.0, 0.0]],
            levels: [[10.0, 20.0, 30.0]; 3],
            bias: 1.0,
            texture: 0,
        });
        let mesh = texture_mesh(&scene, 0);
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("uv attribute");
        };
        assert_eq!(uvs, &[[0.0, 0.0], [0.0, 1.0], [1.0, 0.0]]);
        let Some(VertexAttributeValues::Float32x4(colours)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colour attribute");
        };
        assert_eq!(colours[0], [10.0, 20.0, 30.0, 1.0]);
    }
}
