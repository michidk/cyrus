//! The sun, ambient light and object lights the stand-ins are lit with.
use super::super::owns_prepared_environment;
use super::{
    Tier,
    plugin::{ActiveTier, Light},
};
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{
    asset::RenderAssetUsages,
    ecs::system::SystemParam,
    light::{CascadeShadowConfigBuilder, EnvironmentMapLight, light_consts},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};
use cyrus_assets::environment::{Environment, LocalLight, ModelLighting};

/// Peak sun illuminance in lux for a directional strength of 255; a Modern tuning choice.
const SUN_ILLUMINANCE: f32 = light_consts::lux::AMBIENT_DAYLIGHT;
/// Environment light in cd/m² for an ambient strength of 255; a Modern tuning choice.
const AMBIENT_LUMINANCE: f32 = 3000.0;
/// Emissive luminance per squared world-unit radius of an object light; a Modern tuning choice.
#[cfg(feature = "raytracing")]
const OBJECT_LIGHT_LUMINANCE: f32 = 180.0;
/// Radius of the invisible emissive sphere that stands for an object light, in world units.
#[cfg(feature = "raytracing")]
const OBJECT_LIGHT_SIZE: f32 = 0.15;
/// Luminous power of a raster object light per squared world-unit radius, in lumens. It equals the
/// power of the emissive sphere the ray-traced tier uses; a Modern tuning choice.
const OBJECT_LIGHT_LUMENS: f32 = 160.0;
/// Distance of the first sun shadow cascade, in world units; a Modern tuning choice.
const SHADOW_FIRST_CASCADE: f32 = 60.0;
/// Distance the sun shadow cascades reach, in world units; a Modern tuning choice.
const SHADOW_DISTANCE: f32 = 2000.0;
/// Everything the lights were last built from.
pub(super) struct Built {
    tier: Tier,
    environment: AssetId<Environment>,
    lights: Vec<LocalLight>,
}

/// The retained script world, whose object lights replace the environment's while it owns it.
#[derive(SystemParam)]
pub(super) struct RetainedLights<'w> {
    actors: Option<Res<'w, cyrus_script::script::ScriptActors>>,
    prepared: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    animation: Option<Res<'w, cyrus_world::animation::AnimationRequest>>,
    animations: Option<Res<'w, Assets<cyrus_assets::animation::Animations>>>,
}

impl RetainedLights<'_> {
    fn object_lights(
        &self,
        request: &EnvironmentRequest,
        environment: &Environment,
    ) -> Vec<LocalLight> {
        self.actors
            .as_ref()
            .zip(self.prepared.as_ref())
            .and_then(|(actors, prepared)| {
                let current = actors.current.as_ref()?;
                owns_prepared_environment(
                    current,
                    prepared,
                    request,
                    self.animation.as_deref(),
                    self.animations.as_deref(),
                )
                .then(|| current.object_lights())
            })
            .unwrap_or_else(|| environment.object_lights.clone())
    }
}

/// The assets lights are made of, and the environments they are read from.
#[derive(SystemParam)]
pub(super) struct LightAssets<'w> {
    environments: Res<'w, Assets<Environment>>,
    #[cfg(feature = "raytracing")]
    meshes: ResMut<'w, Assets<Mesh>>,
    #[cfg(feature = "raytracing")]
    materials: ResMut<'w, Assets<StandardMaterial>>,
    images: ResMut<'w, Assets<Image>>,
}

fn peak(rgb: [f32; 3]) -> f32 {
    rgb.into_iter().fold(0.0, f32::max).min(255.0)
}

// The lights join the environment, the retained script world, the camera and the lighting tier.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_lights(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    mut assets: LightAssets,
    existing: Query<Entity, With<Light>>,
    cameras: Query<(Entity, Option<&EnvironmentMapLight>), With<ViewCamera>>,
    retained: RetainedLights,
    tier: ActiveTier,
    mut built: Local<Option<Built>>,
) {
    let Some(tier) = tier.get() else {
        return;
    };
    let Some(environment) = assets.environments.get(&request.0) else {
        return;
    };
    let lights = retained.object_lights(&request, environment);
    if !existing.is_empty()
        && cameras.iter().all(|(_, light)| light.is_some())
        && built.as_ref().is_some_and(|built| {
            built.tier == tier && built.environment == request.0.id() && built.lights == lights
        })
    {
        return;
    }
    for light in &existing {
        commands.entity(light).despawn();
    }
    // Marks that lights were built, even for an environment that has none.
    commands.spawn(Light);
    spawn_sun(&mut commands, &environment.model_lighting, tier);
    let ambient = ambient_light(&mut assets.images, &environment.model_lighting);
    for (camera, _) in &cameras {
        commands.entity(camera).insert(ambient.clone());
    }
    spawn_object_lights(&mut commands, &mut assets, &lights, tier);
    *built = Some(Built {
        tier,
        environment: request.0.id(),
        lights,
    });
}

fn spawn_sun(commands: &mut Commands, lighting: &ModelLighting, tier: Tier) {
    let sun = peak(lighting.directional);
    if let Ok(direction) = Dir3::new(Vec3::from(lighting.direction))
        && sun >= 1.0
    {
        let [r, g, b] = lighting
            .directional
            .map(|value| (value / sun).clamp(0.0, 1.0));
        commands.spawn((
            Light,
            DirectionalLight {
                color: Color::srgb(r, g, b),
                illuminance: SUN_ILLUMINANCE * sun / 255.0,
                // Solari replaces shadow mapping.
                shadow_maps_enabled: tier == Tier::Raster,
                contact_shadows_enabled: tier == Tier::Raster,
                ..default()
            },
            CascadeShadowConfigBuilder {
                first_cascade_far_bound: SHADOW_FIRST_CASCADE,
                maximum_distance: SHADOW_DISTANCE,
                ..default()
            }
            .build(),
            Transform::IDENTITY.looking_to(direction, Vec3::Y),
        ));
    }
}

/// The world's ambient colour as a uniform environment cubemap.
fn ambient_light(images: &mut Assets<Image>, lighting: &ModelLighting) -> EnvironmentMapLight {
    let [r, g, b] = lighting.ambient.map(|value| value.clamp(0.0, 255.0) as u8);
    let mut cube = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        (0..6).flat_map(|_| [r, g, b, 255]).collect(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    cube.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    let cube = images.add(cube);
    EnvironmentMapLight {
        diffuse_map: cube.clone(),
        specular_map: cube,
        intensity: AMBIENT_LUMINANCE * peak(lighting.ambient) / 255.0,
        ..default()
    }
}

/// An object light as the world places it: position, reach in world units and colour.
struct Placed {
    position: Vec3,
    radius: f32,
    rgb: [f32; 3],
}

/// The object lights that give off light; the original skips disabled and negative ones.
fn placed(lights: &[LocalLight]) -> impl Iterator<Item = Placed> + '_ {
    lights
        .iter()
        .filter(|light| light.radius > 0 && light.rgb.iter().all(|value| *value >= 0))
        .map(|light| Placed {
            position: Vec3::from(cyrus_game::rope::world_position(
                light.position.map(f64::from),
            )),
            radius: f32::from(light.radius) / cyrus_game::ENGINE_UNIT_SCALE,
            rgb: light.rgb.map(|value| f32::from(value) / 255.0),
        })
}

/// The raster tier lights with point lights; Solari lights with emissive meshes only.
fn spawn_object_lights(
    commands: &mut Commands,
    assets: &mut LightAssets,
    lights: &[LocalLight],
    tier: Tier,
) {
    match tier {
        Tier::Raster => {
            for light in placed(lights) {
                let [r, g, b] = light.rgb;
                commands.spawn((
                    Light,
                    PointLight {
                        color: Color::srgb(r, g, b),
                        intensity: OBJECT_LIGHT_LUMENS * light.radius * light.radius,
                        range: light.radius,
                        // Dozens of lights per world; shadows would multiply the shadow passes.
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(light.position),
                ));
            }
        }
        Tier::Traced => spawn_emissive_lights(commands, assets, lights),
    }
}

#[cfg(feature = "raytracing")]
fn spawn_emissive_lights(commands: &mut Commands, assets: &mut LightAssets, lights: &[LocalLight]) {
    let Ok(sphere) = Sphere::new(OBJECT_LIGHT_SIZE).mesh().ico(2) else {
        return;
    };
    let count = sphere.count_vertices();
    let sphere = assets
        .meshes
        .add(sphere.with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count]));
    for light in placed(lights) {
        let [r, g, b] = light.rgb;
        let radius = light.radius;
        commands.spawn((
            Light,
            bevy::solari::prelude::RaytracingMesh3d(sphere.clone()),
            MeshMaterial3d(assets.materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(r, g, b) * (OBJECT_LIGHT_LUMINANCE * radius * radius),
                ..default()
            })),
            Transform::from_translation(light.position),
        ));
    }
}

/// Without Solari no tier traces, so there are no emissive lights to spawn.
#[cfg(not(feature = "raytracing"))]
fn spawn_emissive_lights(_: &mut Commands, _: &mut LightAssets, _: &[LocalLight]) {}
