//! The sun, ambient light, object lights and baked lights Solari traces, calibrated as the raster
//! modern lighting's are, so a fully lit surface shows its texture colour.
use super::super::modern_lighting::candle::{self, Candle};
use super::super::modern_lighting::lights::{
    Ambient, OBJECT_LIGHT_FULL_AT, RetainedLights, Sun, ambient_light, exposure, light_color,
    light_transform, peak, placed, point_light_power,
};
use super::plugin::Light;
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{ecs::system::SystemParam, light::EnvironmentMapLight, prelude::*};
use cyrus_assets::environment::{Environment, LocalLight, ModelLighting};
use std::f32::consts::PI;

/// Scale of the environment light while Solari traces it; a Modern tuning choice. Traced sky
/// light is occluded by the surrounding geometry, unlike the raster tier's, so shaded streets
/// would otherwise be far darker than the original's ambient-lit ones.
const AMBIENT_GAIN: f32 = 4.0;
/// Radius of the invisible emissive sphere that stands for an object light, in world units.
const OBJECT_LIGHT_SIZE: f32 = 0.15;

/// The lights as last built: the environment the sun and ambient light come from, and each object
/// light sphere with its material and what it was built from.
pub(super) struct Built {
    environment: AssetId<Environment>,
    sphere: Handle<Mesh>,
    objects: Vec<(Entity, Handle<StandardMaterial>, Emitter)>,
}

/// Where an object light sphere sits and what it gives off.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Emitter {
    position: Vec3,
    emissive: LinearRgba,
}

impl Emitter {
    fn of(light: &LocalLight) -> Option<Self> {
        let light = placed(std::slice::from_ref(light)).next()?;
        Some(Self {
            position: light.position,
            emissive: LinearRgba::from(light_color(light.rgb)) * emissive_luminance(light.radius),
        })
    }
}

/// The assets lights are made of, and the environments they are read from.
#[derive(SystemParam)]
pub(super) struct LightAssets<'w> {
    environments: Res<'w, Assets<Environment>>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

pub(super) fn sync_lights(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    mut assets: LightAssets,
    existing: Query<Entity, With<Light>>,
    cameras: Query<(Entity, Option<&EnvironmentMapLight>), With<ViewCamera>>,
    (retained, candle): (RetainedLights, Res<Candle>),
    mut built: Local<Option<Built>>,
) {
    let Some(environment) = assets.environments.get(&request.0) else {
        return;
    };
    // The object lights, then the lamps and fires the original bakes into vertex lighting when the
    // world loads, which the traced lighting replaces as the raster modern lighting does.
    let mut emitters: Vec<_> = retained
        .object_lights(&request, environment)
        .iter()
        .chain(&environment.prelighting.lights)
        .filter_map(Emitter::of)
        .collect();
    // The player's candle, which the raster tier lights with a point light.
    emitters.extend(candle.0.map(|flame| Emitter {
        position: flame.position,
        emissive: LinearRgba::from(light_color(candle::RGB))
            * emissive_luminance(candle::REACH)
            * flame.darkness,
    }));
    let current = !existing.is_empty()
        && cameras.iter().all(|(_, light)| light.is_some())
        && built
            .as_ref()
            .is_some_and(|built| built.environment == request.0.id());
    if let Some(built) = built.as_mut().filter(|_| current) {
        // Fire effects retime their light radius several times a second. Each light keeps its
        // sphere and material, so Solari never loses a light to a material it has not prepared
        // yet, and the sun and ambient light keep their history.
        update_object_lights(&mut commands, &mut assets, built, &emitters);
        return;
    }
    for light in &existing {
        commands.entity(light).despawn();
    }
    spawn_sun(&mut commands, &environment.model_lighting);
    let mut ambient = ambient_light(&mut assets.images, &environment.model_lighting);
    ambient.intensity *= AMBIENT_GAIN;
    // Marks that lights were built, even for an environment that has none. The day/night cycle
    // retints the cube and moves the sun as it does the raster tier's.
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
    let Ok(sphere) = Sphere::new(OBJECT_LIGHT_SIZE).mesh().ico(2) else {
        return;
    };
    let count = sphere.count_vertices();
    let mut fresh = Built {
        environment: request.0.id(),
        sphere: assets
            .meshes
            .add(sphere.with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count])),
        objects: Vec::new(),
    };
    update_object_lights(&mut commands, &mut assets, &mut fresh, &emitters);
    *built = Some(fresh);
}

/// Brings the object light spheres to `emitters`: moves and re-tints the ones that changed in place
/// and spawns or despawns the difference.
fn update_object_lights(
    commands: &mut Commands,
    assets: &mut LightAssets,
    built: &mut Built,
    emitters: &[Emitter],
) {
    for ((entity, material, held), emitter) in built.objects.iter_mut().zip(emitters) {
        if held.position != emitter.position {
            commands
                .entity(*entity)
                .insert(Transform::from_translation(emitter.position));
        }
        if held.emissive != emitter.emissive
            && let Some(mut material) = assets.materials.get_mut(&*material)
        {
            material.emissive = emitter.emissive;
        }
        *held = *emitter;
    }
    for (entity, _, _) in built
        .objects
        .drain(emitters.len().min(built.objects.len())..)
    {
        commands.entity(entity).despawn();
    }
    for emitter in &emitters[built.objects.len()..] {
        let material = assets.materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: emitter.emissive,
            ..default()
        });
        let entity = commands
            .spawn((
                Light,
                bevy::solari::prelude::RaytracingMesh3d(built.sphere.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(emitter.position),
            ))
            .id();
        built.objects.push((entity, material, *emitter));
    }
}

/// Solari traces the sun's shadows itself, so the sun has no shadow maps.
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
                shadow_maps_enabled: false,
                contact_shadows_enabled: false,
                ..default()
            },
            light_transform(direction),
        ));
    }
}

/// Luminance of an emissive sphere of `OBJECT_LIGHT_SIZE` that gives off the luminous power of
/// the raster tier's point light for an object light of `radius`: a diffuse sphere of radius `s`
/// and luminance `L` emits `π·L·4π·s²`.
fn emissive_luminance(radius: f32) -> f32 {
    point_light_power(radius * OBJECT_LIGHT_FULL_AT)
        / (4.0 * PI * PI * OBJECT_LIGHT_SIZE * OBJECT_LIGHT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_emissive_light_matches_the_raster_point_light_power() {
        let radius = 4.0;
        let area = 4.0 * PI * OBJECT_LIGHT_SIZE * OBJECT_LIGHT_SIZE;
        let emitted = PI * emissive_luminance(radius) * area;
        let point = point_light_power(radius * OBJECT_LIGHT_FULL_AT);
        assert!(
            (emitted - point).abs() / point < 1e-5,
            "{emitted} vs {point}"
        );
    }

    // A fire retimes its light radius several times a second. Respawning every light then made each
    // new material miss a frame of Solari preparation, and fires lit nothing.
    #[test]
    fn a_changing_light_keeps_its_sphere_and_material() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Environment>>();
        let light = |radius| LocalLight {
            position: [0; 3],
            radius,
            rgb: [255, 128, 0],
        };
        let run = |app: &mut App, built: &mut Built, lights: &[LocalLight]| {
            let emitters: Vec<_> = lights.iter().filter_map(Emitter::of).collect();
            let mut state =
                bevy::ecs::system::SystemState::<(Commands, LightAssets)>::new(app.world_mut());
            let (mut commands, mut assets) =
                state.get_mut(app.world_mut()).expect("light assets exist");
            update_object_lights(&mut commands, &mut assets, built, &emitters);
            state.apply(app.world_mut());
        };
        let mut built = Built {
            environment: AssetId::default(),
            sphere: Handle::default(),
            objects: Vec::new(),
        };
        run(&mut app, &mut built, &[light(100)]);
        let (entity, material, first) = built.objects[0].clone();
        run(&mut app, &mut built, &[light(140)]);
        assert_eq!(built.objects.len(), 1);
        assert_eq!(built.objects[0].0, entity, "the sphere stays");
        assert_eq!(built.objects[0].1, material, "the material stays");
        let emissive = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .emissive;
        assert!(
            emissive.red > first.emissive.red,
            "a wider light gives off more"
        );
        run(&mut app, &mut built, &[]);
        assert!(built.objects.is_empty());
        assert!(
            app.world().get_entity(entity).is_err(),
            "a light that goes out is despawned"
        );
    }
}
