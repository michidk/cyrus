//! The sun, ambient light, object lights and baked lights the modern lighting shades the world with.
//!
//! The lights are calibrated against the camera's exposure so a surface that faces the sun shows
//! its texture colour scaled by the original's light values, read as the original's 0..=255 light
//! factors: Bevy's diffuse term is `albedo / π · illuminance · exposure` for the sun and
//! `albedo · intensity · exposure` for a uniform environment light.
use super::super::owns_prepared_environment;
use super::plugin::Light;
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{
    asset::RenderAssetUsages,
    camera::Exposure,
    ecs::system::SystemParam,
    light::{CascadeShadowConfigBuilder, EnvironmentMapLight},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};
use cyrus_assets::environment::{Environment, LocalLight, ModelLighting};
use std::f32::consts::PI;

/// Distance from a map light, as a fraction of its radius, where it lights a facing surface with its
/// full colour. The original's vertex lighting attenuates by `(r² + 3r) / 256 / (d² + 3d)`
/// (`lighting::runtime`, `model_lights.wesl`), which is 1 at about a sixteenth of the radius r.
pub(in crate::fog) const OBJECT_LIGHT_FULL_AT: f32 = 1.0 / 16.0;
/// Distance of the first sun shadow cascade, in world units; a Modern tuning choice.
const SHADOW_FIRST_CASCADE: f32 = 20.0;
/// Distance the sun shadow cascades reach, in world units; a Modern tuning choice.
const SHADOW_DISTANCE: f32 = 400.0;

/// The camera exposure the lights are calibrated for; the view camera keeps Bevy's default.
pub(in crate::fog) fn exposure() -> f32 {
    Exposure::default().exposure()
}
/// Everything the lights were last built from.
pub(super) struct Built {
    environment: AssetId<Environment>,
    lights: Vec<LocalLight>,
}

/// The retained script world, whose object lights replace the environment's while it owns it.
#[derive(SystemParam)]
pub(in crate::fog) struct RetainedLights<'w> {
    actors: Option<Res<'w, cyrus_script::script::ScriptActors>>,
    active: Option<Res<'w, crate::environment::ActiveEnvironment>>,
    prepared: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    animation: Option<Res<'w, cyrus_world::animation::AnimationRequest>>,
    animations: Option<Res<'w, Assets<cyrus_assets::animation::Animations>>>,
}

impl RetainedLights<'_> {
    pub(in crate::fog) fn object_lights(
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
                    self.active.as_deref(),
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
    images: ResMut<'w, Assets<Image>>,
    lamps: ResMut<'w, super::glow::LampLights>,
}

pub(in crate::fog) fn peak(rgb: [f32; 3]) -> f32 {
    rgb.into_iter().fold(0.0, f32::max).min(255.0)
}

pub(super) fn sync_lights(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    mut assets: LightAssets,
    existing: Query<Entity, With<Light>>,
    cameras: Query<(Entity, Option<&EnvironmentMapLight>), With<ViewCamera>>,
    retained: RetainedLights,
    mut built: Local<Option<Built>>,
) {
    let Some(environment) = assets.environments.get(&request.0) else {
        return;
    };
    let lights = retained.object_lights(&request, environment);
    if !existing.is_empty()
        && cameras.iter().all(|(_, light)| light.is_some())
        && built
            .as_ref()
            .is_some_and(|built| built.environment == request.0.id() && built.lights == lights)
    {
        return;
    }
    for light in &existing {
        commands.entity(light).despawn();
    }
    spawn_sun(&mut commands, &environment.model_lighting);
    let ambient = ambient_light(&mut assets.images, &environment.model_lighting);
    // Marks that lights were built, even for an environment that has none.
    commands.spawn((
        Light,
        Ambient {
            image: ambient.diffuse_map.clone(),
            rgb: environment.model_lighting.ambient,
        },
    ));
    for (camera, _) in &cameras {
        commands.entity(camera).insert(ambient.clone());
    }
    spawn_object_lights(&mut commands, &lights);
    // The lamps and fires the original bakes into vertex lighting when the world loads; modern
    // lighting replaces that lighting, so they shine as point lights too.
    spawn_object_lights(&mut commands, &environment.prelighting.lights);
    assets.lamps.set_if_neq(super::glow::LampLights(
        placed(&lights)
            .chain(placed(&environment.prelighting.lights))
            .map(|light| light.position)
            .collect(),
    ));
    *built = Some(Built {
        environment: request.0.id(),
        lights,
    });
}

/// The world's sun as installed: its 0..=255 colour and the direction its light travels.
#[derive(Component)]
pub(in crate::fog) struct Sun {
    pub(in crate::fog) rgb: [f32; 3],
    pub(in crate::fog) direction: Dir3,
}

/// The ambient environment cube and the world's installed 0..=255 ambient colour it was made of.
#[derive(Component)]
pub(in crate::fog) struct Ambient {
    pub(in crate::fog) image: Handle<Image>,
    pub(in crate::fog) rgb: [f32; 3],
}

/// An original 0..=255 light triple as a colour whose linear value is the light factor.
pub(in crate::fog) fn light_color(rgb: [f32; 3]) -> Color {
    let [r, g, b] = rgb.map(|value| (value / 255.0).clamp(0.0, 1.0));
    Color::srgb(r, g, b)
}

/// A light shining along `direction`; straight up or down keeps a horizontal up axis.
pub(in crate::fog) fn light_transform(direction: Dir3) -> Transform {
    let up = if direction.y.abs() > 0.999 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Transform::IDENTITY.looking_to(direction, up)
}

/// The texels of a uniform ambient cube of a 0..=255 colour.
pub(super) fn ambient_texels(rgb: [f32; 3]) -> Vec<u8> {
    let [r, g, b] = rgb.map(|value| value.clamp(0.0, 255.0) as u8);
    (0..6).flat_map(|_| [r, g, b, 255]).collect()
}

fn spawn_sun(commands: &mut Commands, lighting: &ModelLighting) {
    if let Ok(direction) = Dir3::new(Vec3::from(lighting.direction))
        && peak(lighting.directional) >= 1.0
    {
        commands.spawn((
            Light,
            Sun {
                rgb: lighting.directional,
                direction,
            },
            DirectionalLight {
                color: light_color(lighting.directional),
                // Cancels Bevy's 1/π and the exposure, so the light factor reaches the surface.
                illuminance: PI / exposure(),
                shadow_maps_enabled: true,
                contact_shadows_enabled: true,
                ..default()
            },
            CascadeShadowConfigBuilder {
                first_cascade_far_bound: SHADOW_FIRST_CASCADE,
                maximum_distance: SHADOW_DISTANCE,
                ..default()
            }
            .build(),
            light_transform(direction),
            super::shadows::sun_layers(),
        ));
    }
}

/// The world's ambient colour as a uniform environment cubemap.
pub(in crate::fog) fn ambient_light(
    images: &mut Assets<Image>,
    lighting: &ModelLighting,
) -> EnvironmentMapLight {
    // Kept in the main world too, so the day/night cycle can retint it.
    let mut cube = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        ambient_texels(lighting.ambient),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    cube.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    let cube = images.add(cube);
    EnvironmentMapLight {
        diffuse_map: cube.clone(),
        specular_map: cube,
        // The cube's sRGB texels are the light factor; this cancels the exposure.
        intensity: 1.0 / exposure(),
        ..default()
    }
}

/// An object light as the world places it: position, reach in world units and 0..=255 colour.
pub(in crate::fog) struct Placed {
    pub(in crate::fog) position: Vec3,
    pub(in crate::fog) radius: f32,
    pub(in crate::fog) rgb: [f32; 3],
}

/// The lights that add light. Map light colours are signed: the original's vertex lighting adds
/// the positive channels and takes the negative ones away, so a fire is orange with its blue
/// removed and some lights only darken. Bevy's point lights only add, so each light shines with
/// its positive channels and darkening-only lights are left out; a Modern tuning choice.
pub(in crate::fog) fn placed(lights: &[LocalLight]) -> impl Iterator<Item = Placed> + '_ {
    lights
        .iter()
        .filter(|light| light.radius > 0 && light.rgb.iter().any(|value| *value > 0))
        .map(|light| Placed {
            position: Vec3::from(cyrus_game::rope::world_position(
                light.position.map(f64::from),
            )),
            radius: f32::from(light.radius) / cyrus_game::ENGINE_UNIT_SCALE,
            rgb: light.rgb.map(|value| f32::from(value.max(0))),
        })
}

/// Luminous power, in lumens, of a point light that gives a facing surface the sun's full light
/// factor at `distance` world units: `power / (4π·d²)` is the illuminance and `π / exposure` the
/// sun's.
pub(in crate::fog) fn point_light_power(distance: f32) -> f32 {
    4.0 * PI * PI * distance * distance / exposure()
}

/// Object and baked lights are point lights.
fn spawn_object_lights(commands: &mut Commands, lights: &[LocalLight]) {
    for light in placed(lights) {
        commands.spawn((
            Light,
            PointLight {
                color: light_color(light.rgb),
                intensity: point_light_power(light.radius * OBJECT_LIGHT_FULL_AT),
                range: light.radius,
                // Dozens of lights per world; shadows would multiply the shadow passes.
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(light.position),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_lights_shine_with_their_positive_channels_only() {
        let light = |radius, rgb| LocalLight {
            position: [0, 0, 0],
            radius,
            rgb,
        };
        let lights = [
            light(2709, [255, 121, -255]),
            light(1000, [-255, -255, -255]),
            light(0, [255, 255, 255]),
            light(1500, [-75, 21, 40]),
        ];
        let placed: Vec<_> = placed(&lights).map(|light| light.rgb).collect();
        assert_eq!(placed, [[255.0, 121.0, 0.0], [0.0, 21.0, 40.0]]);
    }
}
