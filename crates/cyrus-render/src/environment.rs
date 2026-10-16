//! Cached environment inputs shared by native and browser builds.
//! Glide BSI sky projection and scrolling follow preservation.

use crate::sky::Sky;
use crate::sky::SkyMaterial;
pub use crate::sky::SkyScrollSync;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cyrus_assets::environment::EnvironmentLoader;
pub use cyrus_assets::environment::{Environment, SkyImage, SunImage, Water};

#[derive(Resource, Default)]
/// Environment asset associated with the currently requested world.
pub struct EnvironmentRequest(pub Handle<Environment>);

#[derive(Resource, Default)]
pub(crate) struct ActiveEnvironment {
    pub(crate) source: Option<AssetId<Environment>>,
    pub(crate) sky: Option<Entity>,
}

/// Renders the Glide sky and BIOS-timed sky scrolling and terrain waves.
pub struct EnvironmentPlugin;
impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        crate::console_environment::register(app);
        app.add_plugins((crate::sky::SkyPlugin, crate::fog::FogPlugin))
            .init_asset::<Environment>()
            .init_asset_loader::<EnvironmentLoader>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<ActiveEnvironment>()
            .init_resource::<crate::water::WaterPlayback>()
            .add_systems(
                Update,
                (crate::console_environment::apply, apply_sky)
                    .chain()
                    .after(crate::scene_presentation),
            )
            .add_systems(
                PostUpdate,
                crate::water::animate_water
                    .after(TransformSystems::Propagate)
                    .after(bevy::camera::CameraUpdateSystems)
                    .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
            );
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
        let prepared = match prepare_sky_assets(environment) {
            Ok(prepared) => prepared,
            Err(error) => {
                error!("sky preparation: {error:#}");
                return;
            }
        };
        let sun = sky.sun.as_ref();
        let material = materials.add(SkyMaterial {
            image: images.add(prepared.sky),
            plane: if environment.sky.is_some() {
                Vec4::new(sky.level, sky.scale, sky.width as f32, sky.height as f32)
            } else {
                Vec4::ZERO
            },
            fog_rgb: Vec3::from_array(environment.background.map(|v| v as f32 / 255.0)).extend(0.0),
            scroll: Vec4::ZERO,
            sun_image: images.add(prepared.sun),
            sun_position: sun.map_or(Vec4::ZERO, |sun| {
                let [x, y, z] = sun.center;
                Vec4::new(-x, -y, z, sun.half_size[0]) / 20.0
            }),
            sun_color: sun.map_or(Vec4::ZERO, |sun| {
                Vec3::from_array(sun.color.map(|v| v as f32 / 255.0))
                    .extend(sun.half_size[1] / 20.0)
            }),
        });
        active.sky = Some(
            commands
                .spawn((
                    Sky::new(sky.speed),
                    Mesh3d(meshes.add(prepared.mesh)),
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

/// CPU sky assets prepared without changing environment, clocks or live entities.
pub struct PreparedSkyAssets {
    pub sky: Image,
    pub sun: Image,
    pub mesh: Mesh,
}

/// Build the same images and fullscreen triangle used by ordinary world entry.
/// Invalid dimensions or payload sizes reject before Bevy image construction.
pub fn prepare_sky_assets(environment: &Environment) -> anyhow::Result<PreparedSkyAssets> {
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
    let pixels = (sky.width as usize)
        .checked_mul(sky.height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    anyhow::ensure!(
        sky.width > 0 && sky.height > 0 && pixels == Some(sky.rgba.len()),
        "sky image dimensions/payload differ"
    );
    if let Some(sun) = &sky.sun {
        anyhow::ensure!(
            sun.size[0] > 0
                && sun.size[1] > 0
                && (sun.size[0] as usize).checked_mul(sun.size[1] as usize)
                    == Some(sun.alpha.len()),
            "sun image dimensions/payload differ"
        );
    }
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
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-1.0, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]],
    );
    Ok(PreparedSkyAssets {
        sky: image,
        sun: sun_image,
        mesh,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_assets::environment::water_vertices;
    use cyrus_game::fidelity::Fidelity;
    use cyrus_game::fidelity::Mode;
    #[test]
    fn faithful_sky_uses_source_pixels_and_clears_on_world_change() {
        let mut app = App::new();
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let mut environments = Assets::<Environment>::default();
        let outdoor = environments.add(Environment {
            texture_scale: [1, 1],
            object_lights: Vec::new(),
            prelighting: Default::default(),
            model_lighting: Default::default(),
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
            texture_scale: [1, 1],
            object_lights: Vec::new(),
            prelighting: Default::default(),
            model_lighting: Default::default(),
            sky: None,
            background: [10, 20, 30],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
        });
        let prepared = prepare_sky_assets(environments.get(&outdoor).unwrap()).unwrap();
        assert_eq!(prepared.sky.data.as_deref(), Some(pixels.as_slice()));
        assert_eq!(prepared.sun.data.as_deref(), Some([0, 255].as_slice()));
        {
            let mut environment = environments.get_mut(&outdoor).unwrap();
            environment.sky.as_mut().unwrap().width = 0;
            assert!(prepare_sky_assets(&environment).is_err());
            environment.sky.as_mut().unwrap().width = 2;
            environment
                .sky
                .as_mut()
                .unwrap()
                .sun
                .as_mut()
                .unwrap()
                .alpha
                .pop();
            assert!(prepare_sky_assets(&environment).is_err());
            environment
                .sky
                .as_mut()
                .unwrap()
                .sun
                .as_mut()
                .unwrap()
                .alpha
                .push(255);
        }
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
