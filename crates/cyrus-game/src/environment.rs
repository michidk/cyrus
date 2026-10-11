//! Cached environment inputs shared by native and browser builds.
//! Glide BSI sky projection and scrolling follow preservation.

pub use crate::sky::SkyScrollSync;
use crate::sky::{Sky, SkyMaterial};
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Serialize, Deserialize)]
/// Per-world rendering inputs extracted from WORLD.INI and the installed assets.
pub struct Environment {
    /// Decoded Glide BSI sky and its plane parameters, if the world declares a sky.
    pub sky: Option<SkyImage>,
    /// Glide fog and clear color as encoded RGB bytes.
    pub background: [u8; 3],
    /// Scene near/far clipping distances in engine units, from the install.
    pub clipping: [f32; 2],
    /// The 64-entry Glide scene fog density table, packed in groups of four.
    pub fog_table: [[u8; 4]; 16],
    /// Terrain water mask and original wave parameters, if declared.
    pub water: Option<Water>,
    /// Original per-world terrain shading inputs, absent for worlds without terrain.
    pub terrain_lighting: Option<crate::lighting::TerrainLighting>,
}

#[derive(Serialize, Deserialize)]
/// Opaque BSI sky pixels and WORLD.INI projection inputs, per preservation's sky docs.
pub struct SkyImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel, exactly `width * height * 4` bytes.
    pub rgba: Vec<u8>,
    /// Absolute engine Y coordinate, positive down.
    pub level: f32,
    /// Half-span of plane texture coordinates in texels.
    pub scale: f32,
    /// Raw WORLD.INI scrolling speed; one unit is 1/16 texel per BIOS tick.
    pub speed: u8,
    /// Optional camera-facing sun, using the BSI bytes as opacity.
    pub sun: Option<SunImage>,
}

#[derive(Serialize, Deserialize)]
/// Glide sun billboard inputs, per preservation's sky documentation.
pub struct SunImage {
    /// Texture dimensions in pixels.
    pub size: [u32; 2],
    /// One opacity byte per pixel; no palette conversion.
    pub alpha: Vec<u8>,
    /// Camera-relative center in engine coordinates, positive Y down.
    pub center: [f32; 3],
    /// Billboard half-width and half-height in engine units.
    pub half_size: [f32; 2],
    /// Encoded RGB tint.
    pub color: [u8; 3],
}

#[derive(Serialize, Deserialize)]
/// Wave inputs from WORLD.INI and the WLD texture-selection grid.
pub struct Water {
    /// Raw amplitude, speed, and spatial frequency, in INI order.
    pub parameters: [f32; 3],
    /// Number of vertices along each side of the square terrain grid.
    pub side: usize,
    /// Row-major mask of selected wave vertices, per preservation water docs.
    pub animated_vertices: Vec<bool>,
}

impl Environment {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.clipping.iter().all(|value| value.is_finite())
                && self.clipping[0] > 0.0
                && self.clipping[1] > self.clipping[0],
            "invalid scene clipping distances"
        );
        if let Some(sky) = &self.sky {
            anyhow::ensure!(
                sky.width > 0 && sky.height > 0 && sky.width <= 8192 && sky.height <= 8192,
                "invalid sky dimensions"
            );
            anyhow::ensure!(
                sky.rgba.len() == sky.width as usize * sky.height as usize * 4,
                "invalid sky pixels"
            );
            anyhow::ensure!(
                sky.level.is_finite() && sky.scale.is_finite() && sky.scale != 0.0,
                "invalid sky plane parameters"
            );
        }
        if let Some(sun) = self.sky.as_ref().and_then(|sky| sky.sun.as_ref()) {
            anyhow::ensure!(
                sun.size.iter().all(|&v| (1..=256).contains(&v))
                    && sun.alpha.len() == sun.size[0] as usize * sun.size[1] as usize,
                "invalid sun pixels"
            );
            anyhow::ensure!(
                sun.center.iter().all(|v| v.is_finite())
                    && sun.half_size.iter().all(|v| v.is_finite() && *v > 0.0),
                "invalid sun geometry"
            );
        }
        if let Some(light) = &self.terrain_lighting {
            anyhow::ensure!(
                (2..=256).contains(&light.side)
                    && light
                        .ambient
                        .iter()
                        .chain(&light.directional)
                        .chain(&light.direction)
                        .all(|v| v.is_finite())
                    && (Vec3::from(light.direction) == Vec3::ZERO
                        || (Vec3::from(light.direction).length_squared() - 1.0).abs() < 0.00001),
                "invalid terrain lighting grid, strengths or direction"
            );
            anyhow::ensure!(
                self.water
                    .as_ref()
                    .is_none_or(|water| water.side == light.side),
                "terrain lighting/water grid mismatch"
            );
        }
        if let Some(water) = &self.water {
            anyhow::ensure!(
                (2..=256).contains(&water.side)
                    && water.animated_vertices.len() == water.side * water.side,
                "invalid water grid"
            );
            anyhow::ensure!(
                water.parameters.iter().all(|v| v.is_finite())
                    && water.parameters[1..].iter().all(|v| v.fract() == 0.0
                        && *v >= i32::MIN as f32
                        && *v <= i32::MAX as f32),
                "invalid wave parameters"
            );
        }
        Ok(())
    }
}

#[derive(Default, TypePath)]
struct EnvironmentLoader;
impl AssetLoader for EnvironmentLoader {
    type Asset = Environment;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Environment, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let environment: Environment = serde_json::from_slice(&bytes)?;
        environment.validate()?;
        Ok(environment)
    }
    fn extensions(&self) -> &[&str] {
        &["environment"]
    }
}

#[derive(Resource, Default)]
/// Environment asset associated with the currently requested world.
pub struct EnvironmentRequest(pub Handle<Environment>);

#[derive(Resource, Default)]
struct ActiveEnvironment {
    source: Option<AssetId<Environment>>,
    sky: Option<Entity>,
}

/// Renders the Glide sky and BIOS-timed sky scrolling and terrain waves.
pub struct EnvironmentPlugin;
impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((crate::sky::SkyPlugin, crate::fog::FogPlugin))
            .init_asset::<Environment>()
            .init_asset_loader::<EnvironmentLoader>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<ActiveEnvironment>()
            .init_resource::<crate::water::WaterPlayback>()
            .add_systems(Update, apply_sky)
            .add_systems(PostUpdate, crate::water::animate_water);
    }
}

// Bevy supplies the independent asset and rendering resources to this system.
#[allow(clippy::too_many_arguments)]
fn apply_sky(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    mut active: ResMut<ActiveEnvironment>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    cameras: Query<Entity, With<Camera3d>>,
    mut clear: ResMut<ClearColor>,
    mut distance: ResMut<crate::fog::RenderDistance>,
) {
    if active.source == Some(request.0.id()) || cameras.is_empty() {
        return;
    }
    // Remove the old world's sky even while the next sidecar is still loading.
    if let Some(entity) = active.sky.take() {
        commands.entity(entity).despawn();
    }
    active.source = None;
    distance.original = None;
    let Some(environment) = environments.get(&request.0) else {
        return;
    };
    {
        let background = SkyImage {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
            level: 0.0,
            scale: 0.0,
            speed: 0,
            sun: None,
        };
        let sky = environment.sky.as_ref().unwrap_or(&background);
        let mut image = Image::new(
            Extent3d {
                width: sky.width,
                height: sky.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            sky.rgba.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        let sun = sky.sun.as_ref();
        let size = sun.map_or([1, 1], |sun| sun.size);
        let mut sun_image = Image::new(
            Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            sun.map_or_else(|| vec![0], |sun| sun.alpha.clone()),
            TextureFormat::R8Unorm,
            RenderAssetUsages::default(),
        );
        sun_image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
        let material = materials.add(SkyMaterial {
            image: images.add(image),
            plane: if environment.sky.is_some() {
                Vec4::new(sky.level, sky.scale, sky.width as f32, sky.height as f32)
            } else {
                Vec4::ZERO
            },
            fog_rgb: Vec3::from_array(environment.background.map(|v| v as f32 / 255.0)).extend(0.0),
            scroll: Vec4::ZERO,
            sun_image: images.add(sun_image),
            sun_position: sun.map_or(Vec4::ZERO, |sun| {
                let [x, y, z] = sun.center;
                Vec4::new(-x, -y, z, sun.half_size[0]) / 20.0
            }),
            sun_color: sun.map_or(Vec4::ZERO, |sun| {
                Vec3::from_array(sun.color.map(|v| v as f32 / 255.0))
                    .extend(sun.half_size[1] / 20.0)
            }),
        });
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-1.0, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]],
        );
        active.sky = Some(
            commands
                .spawn((
                    Sky::new(sky.speed),
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    NoFrustumCulling,
                ))
                .id(),
        );
    }
    distance.original = Some(environment.clipping);
    let [r, g, b] = environment.background;
    clear.0 = Color::srgb_u8(r, g, b);
    active.source = Some(request.0.id());
}

/// Selected-corner water classification from preservation's water renderer documentation.
pub fn water_vertices(textures: &[u8], side: usize) -> Vec<bool> {
    let mut vertices = vec![false; textures.len()];
    assert!(side >= 2 && textures.len() == side * side);
    for row in 1..side {
        for x in 1..side {
            let selected = x + row * side;
            let neighbours = [selected - 1, selected - side, selected - side - 1];
            vertices[selected] = matches!(textures[selected] & 63, 0 | 30 | 31)
                && neighbours
                    .iter()
                    .all(|&i| matches!(textures[i] & 63, 0 | 5 | 30 | 31));
        }
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fidelity::{Fidelity, Mode};
    #[test]
    fn faithful_sky_uses_source_pixels_and_clears_on_world_change() {
        let mut app = App::new();
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let mut environments = Assets::<Environment>::default();
        let outdoor = environments.add(Environment {
            sky: Some(SkyImage {
                width: 2,
                height: 1,
                rgba: pixels.clone(),
                level: -4500.0,
                scale: 2000.0,
                speed: 6,
                sun: Some(SunImage {
                    size: [2, 1],
                    alpha: vec![0, 255],
                    center: [-100.0, -200.0, -300.0],
                    half_size: [40.0, 20.0],
                    color: [255, 0, 128],
                }),
            }),
            background: [0; 3],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
        });
        let indoor = environments.add(Environment {
            sky: None,
            background: [10, 20, 30],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
        });
        app.insert_resource(environments)
            .insert_resource(EnvironmentRequest(outdoor.clone()))
            .init_resource::<ActiveEnvironment>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkyMaterial>>()
            .init_resource::<Fidelity>()
            .init_resource::<ClearColor>()
            .init_resource::<crate::fog::RenderDistance>()
            .init_resource::<Time<Real>>()
            .init_resource::<crate::water::WaterPlayback>()
            .add_systems(Update, apply_sky)
            .add_systems(PostUpdate, crate::water::animate_water);
        app.world_mut().spawn(Camera3d::default());
        app.update();
        let entity = app.world().resource::<ActiveEnvironment>().sky.unwrap();
        let material_handle = &app
            .world()
            .get::<MeshMaterial3d<SkyMaterial>>(entity)
            .unwrap()
            .0;
        let material = app
            .world()
            .resource::<Assets<SkyMaterial>>()
            .get(material_handle)
            .unwrap();
        assert_eq!(material.plane, Vec4::new(-4500.0, 2000.0, 2.0, 1.0));
        assert_eq!(material.sun_position, Vec4::new(5.0, 10.0, -15.0, 2.0));
        assert_eq!(material.sun_color, Vec4::new(1.0, 0.0, 128.0 / 255.0, 1.0));
        let sun = app
            .world()
            .resource::<Assets<Image>>()
            .get(&material.sun_image)
            .unwrap();
        assert_eq!(sun.texture_descriptor.format, TextureFormat::R8Unorm);
        assert_eq!(sun.data.as_deref(), Some([0, 255].as_slice()));
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&material.image)
            .unwrap();
        assert_eq!(image.data.as_ref().unwrap(), &pixels);
        assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 1);
        app.world_mut().resource_mut::<Fidelity>().sky = Mode::Modern;
        app.update();
        assert_eq!(
            app.world().resource::<ActiveEnvironment>().sky,
            Some(entity)
        );

        app.world_mut().resource_mut::<EnvironmentRequest>().0 = Handle::default();
        app.update();
        assert!(
            app.world().get::<Sky>(entity).is_none(),
            "a pending world must not keep the old sky"
        );
        app.world_mut().resource_mut::<EnvironmentRequest>().0 = outdoor;
        app.update();
        assert!(app.world().resource::<ActiveEnvironment>().sky.is_some());
        app.world_mut().resource_mut::<EnvironmentRequest>().0 = indoor;
        app.update();
        let background = app.world().resource::<ActiveEnvironment>().sky.unwrap();
        let handle = &app
            .world()
            .get::<MeshMaterial3d<SkyMaterial>>(background)
            .unwrap()
            .0;
        let material = app
            .world()
            .resource::<Assets<SkyMaterial>>()
            .get(handle)
            .unwrap();
        assert_eq!(material.plane, Vec4::ZERO);
        assert_eq!(material.sun_position, Vec4::ZERO);
        assert_eq!(
            app.world().resource::<ClearColor>().0,
            Color::srgb_u8(10, 20, 30)
        );
    }

    #[test]
    fn water_selects_only_the_corner_and_ignores_rotation_bits() {
        assert_eq!(
            water_vertices(&[0, 69, 158, 223], 2),
            vec![false, false, false, true]
        );
        assert_eq!(water_vertices(&[0, 30, 31, 5], 2), vec![false; 4]);
        assert_eq!(water_vertices(&[0, 5, 30, 2], 2), vec![false; 4]);
        assert_eq!(
            water_vertices(&[0, 0, 2, 0, 0, 2, 2, 2, 2], 3),
            vec![false, false, false, false, true, false, false, false, false]
        );
    }
    #[test]
    fn old_water_sidecars_require_reexport() {
        let old = serde_json::json!({"sky":null,"background":[0,0,0],"clipping":[7,3800],"fog_table": ([[0_u8;4];16]),"water":{"parameters":[24,28,5],"side":2,"vertices":[true,true,true,true]}}).to_string();
        let error = serde_json::from_str::<Environment>(&old).err().unwrap();
        assert!(error.to_string().contains("animated_vertices"));
    }
}
