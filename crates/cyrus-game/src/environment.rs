//! Cached environment inputs shared by native and browser builds.
//! Static Glide BSI sky projection follows preservation; sky motion and water playback remain unsupported.

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
    /// Palette-derived RGB background; `None` means sky-derived color is unresolved.
    pub background: Option<[u8; 3]>,
    /// Terrain water mask and original wave parameters, if declared.
    pub water: Option<Water>,
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
}

#[derive(Serialize, Deserialize)]
/// Wave inputs from WORLD.INI and the WLD texture-selection grid.
pub struct Water {
    /// Raw amplitude, speed, and spatial frequency, in INI order.
    pub parameters: [f32; 3],
    /// Number of vertices along each side of the square terrain grid.
    pub side: usize,
    /// Row-major mask identifying vertices belonging to four-corner water cells.
    pub vertices: Vec<bool>,
}

impl Environment {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
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
        if let Some(water) = &self.water {
            anyhow::ensure!(
                (2..=256).contains(&water.side) && water.vertices.len() == water.side * water.side,
                "invalid water grid"
            );
            anyhow::ensure!(
                water.parameters.iter().all(|v| v.is_finite() && *v >= 0.0),
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

/// Renders the verified static Glide sky; water playback remains unsupported.
pub struct EnvironmentPlugin;
impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::sky::SkyPlugin)
            .init_asset::<Environment>()
            .init_asset_loader::<EnvironmentLoader>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<ActiveEnvironment>()
            .add_systems(Update, apply_sky);
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
) {
    if active.source == Some(request.0.id()) || cameras.is_empty() {
        return;
    }
    // Remove the old world's sky even while the next sidecar is still loading.
    if let Some(entity) = active.sky.take() {
        commands.entity(entity).despawn();
    }
    active.source = None;
    let Some(environment) = environments.get(&request.0) else {
        return;
    };
    if let Some(sky) = &environment.sky {
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
        let material = materials.add(SkyMaterial {
            image: images.add(image),
            plane: Vec4::new(sky.level, sky.scale, sky.width as f32, sky.height as f32),
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
                    Sky,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    NoFrustumCulling,
                ))
                .id(),
        );
    }
    let [r, g, b] = environment.background.unwrap_or([0; 3]);
    clear.0 = Color::srgb_u8(r, g, b);
    active.source = Some(request.0.id());
}

/// Four-corner water classification from preservation's water renderer documentation.
pub fn water_vertices(textures: &[u8], side: usize) -> Vec<bool> {
    let mut vertices = vec![false; textures.len()];
    for z in 0..side - 1 {
        for x in 0..side - 1 {
            let corners = [
                x + z * side,
                x + 1 + z * side,
                x + (z + 1) * side,
                x + 1 + (z + 1) * side,
            ];
            if corners
                .iter()
                .all(|&i| matches!(textures[i] & 63, 0 | 5 | 30 | 31))
            {
                for i in corners {
                    vertices[i] = true;
                }
            }
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
            }),
            background: Some([0; 3]),
            water: None,
        });
        let indoor = environments.add(Environment {
            sky: None,
            background: Some([10, 20, 30]),
            water: None,
        });
        app.insert_resource(environments)
            .insert_resource(EnvironmentRequest(outdoor.clone()))
            .init_resource::<ActiveEnvironment>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SkyMaterial>>()
            .init_resource::<Fidelity>()
            .init_resource::<ClearColor>()
            .add_systems(Update, apply_sky);
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
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&material.image)
            .unwrap();
        assert_eq!(image.data.as_ref().unwrap(), &pixels);
        assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 1);
        app.world_mut().resource_mut::<Fidelity>().graphics = Mode::Modern;
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
        assert!(app.world().resource::<ActiveEnvironment>().sky.is_none());
        assert_eq!(
            app.world().resource::<ClearColor>().0,
            Color::srgb_u8(10, 20, 30)
        );
    }

    #[test]
    fn water_requires_all_four_corners_and_ignores_rotation_bits() {
        assert_eq!(water_vertices(&[0, 69, 158, 223], 2), vec![true; 4]);
        assert_eq!(water_vertices(&[0, 5, 30, 2], 2), vec![false; 4]);
        assert_eq!(
            water_vertices(&[0, 0, 2, 0, 0, 2, 2, 2, 2], 3),
            vec![true, true, false, true, true, false, false, false, false]
        );
    }
}
