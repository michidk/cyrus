//! The installed Windows 3dfx splash, drawn live every frame as the original's Glide splash is.
//! Projection, per-vertex lighting and fog follow preservation's `docs/engine/startup.md`; the
//! meshes go through one perspective camera into a 1280 by 960 image that the startup sprite shows
//! at 640 by 480, so edges are antialiased by two-times supersampling.
use super::StartupContent;
use super::view3d::{projection_for, raw_texture_image, target_image_sized, view_camera};
use bevy::{
    asset::{AssetLoader, LoadContext, embedded_asset, io::Reader},
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::ImageSampler,
    math::{Mat4, Vec2, Vec3},
    mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use cyrus_assets::splash_asset::{SplashAsset, SplashDraw};

/// The supersampled render size; the sprite shows it at half this size.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 960;
/// Render layer of the splash scene.
const SPLASH_LAYER: usize = 9;
/// Frames before the fog starts to lift, and the frames it takes to lift.
const BLACK_FRAMES: usize = 24;
const FADE_IN_FRAMES: f32 = 8.0;

#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = SplashAsset;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> anyhow::Result<Self::Asset> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let asset: SplashAsset = serde_json::from_slice(&bytes)?;
        asset.validate()?;
        Ok(asset)
    }
    fn extensions(&self) -> &[&str] {
        &["splash"]
    }
}

/// Registers the splash asset and its loader, in every host.
pub(super) fn register_assets(app: &mut App) {
    app.init_asset::<SplashAsset>()
        .init_asset_loader::<Loader>();
}

/// A splash draw's material: the texture, and the fog visibility every draw of a frame shares.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SplashMaterial {
    #[texture(0)]
    #[sampler(1)]
    texture: Handle<Image>,
    #[uniform(2)]
    visibility: Vec4,
}

impl Material for SplashMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_startup/splash.wesl".into()
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

/// Registers [`SplashMaterial`] and its shader, in hosts that render.
pub struct SplashPlugin;
impl Plugin for SplashPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "splash.wesl");
        app.add_plugins(MaterialPlugin::<SplashMaterial>::default());
    }
}

/// The fog's visibility at `frame`: black for the first frames, then lifting; after the authored
/// frames the held final pose is darkened a little more on every second fade step, as the original's
/// fade overlay darkens its two alternating buffers.
pub(super) fn visibility(asset: &SplashAsset, frame: usize) -> f32 {
    if frame < asset.frames.len() {
        return if frame < BLACK_FRAMES {
            0.0
        } else {
            ((frame - BLACK_FRAMES) as f32 / FADE_IN_FRAMES).min(1.0)
        };
    }
    let step = frame - asset.frames.len();
    let swaps = 2 * asset.frames_per_second as usize;
    (step % 2..=step)
        .step_by(2)
        .map(|step| 1.0 - (102 * step.saturating_sub(1) / swaps) as f32 / 255.0)
        .product()
}

/// The draws that make up `frame`.
fn draws(asset: &SplashAsset, frame: usize) -> &[SplashDraw] {
    asset
        .frames
        .get(frame)
        .map_or(&asset.final_draws, Vec::as_slice)
}

/// Phong-style term of the splash's lighting: diffuse plus a specular lobe of power eight.
fn lighting(normal: Vec3, light: Vec3, view: Vec3) -> f32 {
    let dot = normal.dot(light);
    let reflection = (2.0 * normal * dot - light).normalize();
    dot.max(0.0) + reflection.dot(view).max(0.0).powi(8)
}

/// The corners of every face that uses one texture: position (in the reference camera's frame),
/// texture coordinates and 8-bit vertex colour.
#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
}

/// Per-vertex data of one draw, lit and projected as the original's splash does. Draws carry
/// their own field of view, so positions are scaled about the view axis to the `reference` field,
/// which leaves every screen position and depth order unchanged.
fn add_draw(asset: &SplashAsset, draw: &SplashDraw, reference: f32, batch: &mut Batch) {
    let camera = Mat4::from_cols_array(&draw.camera);
    let matrix = camera * Mat4::from_cols_array(&draw.transform);
    let flip = Vec3::new(1.0, 1.0, -1.0);
    let view = (camera.z_axis.truncate() * flip).normalize();
    let mesh = &asset.meshes[draw.mesh];
    let texture = &asset.textures[draw.texture];
    let uv_scale = texture.width.max(texture.height) as f32 / 256.0;
    let widen = (reference / 2.0).tan() / (draw.horizontal_fov / 2.0).tan();
    let corners: Vec<_> = mesh
        .vertices
        .iter()
        .map(|vertex| {
            let position = matrix.transform_point3(vertex.position.into()) * flip;
            let normal = (matrix.transform_vector3(vertex.normal.into()) * flip).normalize();
            let uv = if asset.sphere_mapped[draw.mesh] {
                let dot = normal.dot(position);
                let discriminant = dot * dot - (position.length_squared() - 160000.0);
                if discriminant > 0.0 {
                    let intersection = position + normal * (discriminant.sqrt() - dot);
                    (intersection.truncate() / 400.0 + Vec2::ONE) * 204.8
                } else {
                    Vec2::ZERO
                }
            } else {
                Vec2::from(vertex.uv) * 256.0
            };
            let to_light = Vec3::from(draw.light_position) - position;
            let attenuation = (4000000.0 / to_light.length_squared()).min(1.0).powi(4);
            let positional = lighting(normal, to_light.normalize(), view) * attenuation;
            let directional = lighting(normal, Vec3::new(1.0, 1.0, -1.0) / 3.0f32.sqrt(), view);
            let color = (Vec3::from(draw.light_color) * (255.0 * positional)
                + Vec3::splat(255.0 * directional + 16.0))
            .clamp(Vec3::ZERO, Vec3::splat(255.0));
            // Depth is forward in the original's view frame and toward the viewer in Bevy's.
            (
                [position.x * widen, position.y * widen, -position.z],
                [
                    uv.x * uv_scale / texture.width as f32,
                    uv.y * uv_scale / texture.height as f32,
                ],
                [color.x, color.y, color.z, 0.0],
            )
        })
        .collect();
    for face in &mesh.faces {
        for index in face {
            let (position, uv, color) = corners[*index];
            batch.positions.push(position);
            batch.uvs.push(uv);
            batch.colors.push(color);
        }
    }
}

/// The running splash: its render target, camera and one mesh per texture.
pub(super) struct SplashView {
    target: Handle<Image>,
    camera: Option<Entity>,
    meshes: Vec<(Entity, Handle<Mesh>, Handle<SplashMaterial>)>,
    shown: Option<usize>,
}

/// The asset stores the splash draws into. They are absent in hosts without a renderer, which
/// draw nothing.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct SplashAssets<'w> {
    meshes: Option<ResMut<'w, Assets<Mesh>>>,
    materials: Option<ResMut<'w, Assets<SplashMaterial>>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub splashes: Res<'w, Assets<SplashAsset>>,
}

impl SplashView {
    /// The view and the black image the startup sprite shows until the splash has loaded.
    pub fn new(images: &mut Assets<Image>) -> Self {
        let mut target = target_image_sized(WIDTH, HEIGHT, [0, 0, 0, 255]);
        // The sprite shows this at half size; bilinear sampling is the box filter of the 2x2 samples.
        target.sampler = ImageSampler::linear();
        Self {
            target: images.add(target),
            camera: None,
            meshes: Vec::new(),
            shown: None,
        }
    }

    pub fn target(&self) -> Handle<Image> {
        self.target.clone()
    }

    /// Draws `frame` of the splash.
    pub fn present(
        &mut self,
        commands: &mut Commands,
        assets: &mut SplashAssets,
        splash: &Handle<SplashAsset>,
        frame: usize,
    ) {
        let SplashAssets {
            meshes: Some(meshes),
            materials: Some(materials),
            images,
            splashes,
        } = assets
        else {
            return;
        };
        let Some(asset) = splashes.get(splash) else {
            return;
        };
        if self.camera.is_none() {
            self.camera = Some(
                commands
                    .spawn((
                        StartupContent,
                        view_camera(self.target.clone(), SPLASH_LAYER, Color::BLACK),
                    ))
                    .id(),
            );
            for texture in &asset.textures {
                let image = images.add(raw_texture_image(
                    texture.width,
                    texture.height,
                    texture.rgba.clone(),
                ));
                // A degenerate triangle until the first frame fills the mesh in.
                let mesh = meshes.add(
                    Mesh::new(PrimitiveTopology::TriangleList, default())
                        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3])
                        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; 3])
                        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0; 4]; 3]),
                );
                let material = materials.add(SplashMaterial {
                    texture: image,
                    visibility: Vec4::ZERO,
                });
                let entity = commands
                    .spawn((
                        StartupContent,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::IDENTITY,
                        NoFrustumCulling,
                        Visibility::Hidden,
                        RenderLayers::layer(SPLASH_LAYER),
                    ))
                    .id();
                self.meshes.push((entity, mesh, material));
            }
        }
        if self.shown == Some(frame) {
            return;
        }
        self.shown = Some(frame);
        let visibility = visibility(asset, frame);
        let frame_draws = draws(asset, frame);
        let reference = frame_draws.first().map_or(1.0, |draw| draw.horizontal_fov);
        if let Some(camera) = self.camera {
            // The splash's horizontal half-field tangent at its 1280 by 960 size.
            commands
                .entity(camera)
                .insert(projection_for((reference / 2.0).tan()));
        }
        let mut batches: Vec<Batch> = asset.textures.iter().map(|_| Batch::default()).collect();
        if visibility > 0.0 {
            for draw in frame_draws {
                add_draw(asset, draw, reference, &mut batches[draw.texture]);
            }
        }
        for ((entity, mesh, material), batch) in self.meshes.iter().zip(batches) {
            let shown = !batch.positions.is_empty();
            if shown && let Some(mut mesh) = meshes.get_mut(mesh) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, batch.uvs);
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, batch.colors);
            }
            if let Some(mut material) = materials.get_mut(material) {
                material.visibility = Vec4::new(visibility, 0.0, 0.0, 0.0);
            }
            commands.entity(*entity).insert(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }

    /// Removes the camera and meshes once the splash has finished.
    pub fn finish(&mut self, commands: &mut Commands) {
        for entity in self
            .camera
            .take()
            .into_iter()
            .chain(self.meshes.drain(..).map(|(entity, ..)| entity))
        {
            commands.entity(entity).despawn();
        }
        self.shown = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(frames: usize, fps: u32) -> SplashAsset {
        SplashAsset {
            frames_per_second: fps,
            meshes: Vec::new(),
            sphere_mapped: Vec::new(),
            textures: Vec::new(),
            frames: (0..frames).map(|_| Vec::new()).collect(),
            final_draws: Vec::new(),
        }
    }

    #[test]
    fn fog_is_black_then_lifts_over_eight_frames_then_holds() {
        let asset = asset(90, 30);
        assert_eq!(visibility(&asset, 0), 0.0);
        assert_eq!(visibility(&asset, 23), 0.0);
        assert_eq!(visibility(&asset, 24), 0.0);
        assert_eq!(visibility(&asset, 28), 0.5);
        assert_eq!(visibility(&asset, 32), 1.0);
        assert_eq!(visibility(&asset, 89), 1.0);
    }

    #[test]
    fn the_held_pose_fades_by_the_original_overlay_alpha_on_alternating_buffers() {
        let asset = asset(90, 30);
        // Steps 0 and 1 draw undimmed; step 2 uses alpha 102 * 1 / 60 = 1 on its buffer.
        assert_eq!(visibility(&asset, 90), 1.0);
        assert_eq!(visibility(&asset, 91), 1.0);
        assert_eq!(visibility(&asset, 92), 1.0 - 1.0 / 255.0);
        // The last frame has been dimmed on every second step, ending very dark.
        assert!(visibility(&asset, 149) < 0.1);
        assert!(visibility(&asset, 149) < visibility(&asset, 120));
    }
}
