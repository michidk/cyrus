//! Hardware ray-traced lighting through Bevy Solari, selected by `Area::RayTracing`.
//!
//! **Modern only.** The original shades models and terrain with its own fog, clipping and
//! palette arithmetic in custom forward shaders; Solari lights a deferred G-buffer and traces
//! standard PBR materials. While the area is Modern and the GPU supports ray queries, each
//! opaque world mesh gets a stand-in entity that Solari can use (a mesh with exactly the
//! vertex layout it accepts, and a plain `StandardMaterial` that opts into deferred rendering),
//! and the shader-driven originals stop drawing. Flats, water, sky, alpha-masked or
//! chroma-keyed meshes and the HUD keep their forward shaders and draw over the lit scene.
//! The sun, the ambient colour and the object lights come from the world's environment.
use bevy::prelude::*;

/// Whether this build and GPU can trace rays.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub enum RayTracingSupport {
    /// The build lacks ray tracing, or no renderer exists (headless, browser).
    #[default]
    Unavailable,
    /// The adapter lacks these wgpu features.
    MissingFeatures(String),
    /// Ray queries are available.
    Available,
}

impl RayTracingSupport {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// A world mesh's ray-traced stand-in. It has no parent, so scene preparation ignores it.
#[derive(Component)]
pub(crate) struct Proxy {
    source: Entity,
}

#[cfg(feature = "raytracing")]
pub use solari::RayTracingPlugin;

#[cfg(feature = "raytracing")]
mod solari {
    use super::super::{SceneMaterial, TerrainMaterial, clipping::Draw, owns_prepared_environment};
    use super::{Proxy, RayTracingSupport};
    use crate::flat::Flat;
    use crate::{camera::ViewCamera, environment::EnvironmentRequest};
    use bevy::{
        asset::{RenderAssetUsages, UntypedAssetId},
        camera::{CameraMainTextureUsages, Hdr, visibility::RenderLayers},
        core_pipeline::prepass::{DeferredPrepass, DepthPrepass, MotionVectorPrepass},
        light::{EnvironmentMapLight, light_consts},
        material::OpaqueRendererMethod,
        mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
        pbr::DefaultOpaqueRendererMethod,
        prelude::*,
        render::{
            RenderApp,
            render_resource::{
                Extent3d, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
                TextureViewDimension,
            },
            renderer::RenderDevice,
        },
        solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins},
    };
    use cyrus_assets::environment::{Environment, LocalLight};
    use cyrus_game::fidelity::{Area, Fidelity, Mode};
    use std::collections::{HashMap, HashSet};

    /// Peak sun illuminance in lux for a directional strength of 255; a Modern tuning choice.
    const SUN_ILLUMINANCE: f32 = light_consts::lux::AMBIENT_DAYLIGHT;
    /// Environment light in cd/m² for an ambient strength of 255; a Modern tuning choice.
    const AMBIENT_LUMINANCE: f32 = 3000.0;
    /// Emissive luminance per squared world-unit radius of an object light; a Modern tuning choice.
    const OBJECT_LIGHT_LUMINANCE: f32 = 180.0;
    /// Radius of the invisible emissive sphere that stands for an object light, in world units.
    const OBJECT_LIGHT_SIZE: f32 = 0.15;

    /// Adds Solari behind the `RayTracing` fidelity area. Add after `DefaultPlugins`.
    pub struct RayTracingPlugin;

    impl Plugin for RayTracingPlugin {
        fn build(&self, app: &mut App) {
            app.add_plugins(SolariPlugins)
                // Solari makes every opaque material deferred, which would bypass this
                // renderer's forward shaders. Only the stand-ins opt in.
                .insert_resource(DefaultOpaqueRendererMethod::forward())
                .init_resource::<RayTracingSupport>()
                .add_systems(
                    PostUpdate,
                    (
                        (
                            configure_camera,
                            sync_proxies,
                            hide_draws.after(sync_proxies),
                            sync_lights,
                        )
                            .run_if(active),
                        restore.run_if(not(active)),
                    )
                        .after(TransformSystems::Propagate)
                        .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
                );
        }

        fn finish(&self, app: &mut App) {
            let required = SolariPlugins::required_wgpu_features();
            let support = match app
                .get_sub_app(RenderApp)
                .and_then(|render| render.world().get_resource::<RenderDevice>())
            {
                None => RayTracingSupport::Unavailable,
                Some(device) if device.features().contains(required) => {
                    RayTracingSupport::Available
                }
                Some(device) => RayTracingSupport::MissingFeatures(format!(
                    "{:?}",
                    required.difference(device.features())
                )),
            };
            if support != RayTracingSupport::Available {
                warn!("ray tracing unavailable: {support:?}");
            }
            app.insert_resource(support);
        }
    }

    fn active(fidelity: Res<Fidelity>, support: Res<RayTracingSupport>) -> bool {
        fidelity.mode(Area::RayTracing) == Mode::Modern && support.is_available()
    }

    /// A source mesh that has a stand-in; its own draw is hidden.
    #[derive(Component)]
    struct Traced;

    /// The render layers an entity had before its draw was hidden for tracing.
    #[derive(Component)]
    struct Hidden(Option<RenderLayers>);

    /// A light, or the marker that lights were built, owned by this module.
    #[derive(Component)]
    struct Light;

    #[derive(Default)]
    struct StandIns {
        meshes: HashMap<AssetId<Mesh>, Handle<Mesh>>,
        materials: HashMap<UntypedAssetId, Handle<StandardMaterial>>,
    }

    fn configure_camera(
        mut commands: Commands,
        cameras: Query<Entity, (With<ViewCamera>, Without<SolariLighting>)>,
    ) {
        for camera in &cameras {
            // `Msaa::Off` is already set at spawn. Solari also requires storage binding,
            // and brings HDR and the deferred, depth and motion prepasses as required components.
            commands.entity(camera).insert((
                SolariLighting::default(),
                CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
            ));
        }
    }

    /// Undoes everything this module changed once ray tracing is off or unsupported.
    #[allow(clippy::type_complexity)] // Bevy query filters.
    fn restore(
        mut commands: Commands,
        cameras: Query<Entity, (With<ViewCamera>, With<SolariLighting>)>,
        owned: Query<Entity, Or<(With<Proxy>, With<Light>)>>,
        hidden: Query<(Entity, &Hidden)>,
    ) {
        for camera in &cameras {
            commands.entity(camera).remove::<(
                SolariLighting,
                CameraMainTextureUsages,
                EnvironmentMapLight,
                Hdr,
                DeferredPrepass,
                DepthPrepass,
                MotionVectorPrepass,
            )>();
        }
        for entity in &owned {
            commands.entity(entity).despawn();
        }
        for (entity, hidden) in &hidden {
            unhide(&mut commands, entity, hidden);
        }
    }

    fn unhide(commands: &mut Commands, entity: Entity, hidden: &Hidden) {
        let mut entity = commands.entity(entity);
        match &hidden.0 {
            Some(layers) => entity.insert(layers.clone()),
            None => entity.remove::<RenderLayers>(),
        };
        entity.remove::<(Hidden, Traced)>();
    }

    /// The traceable material of a source: asset identity, base material and solid tint.
    /// Meshes whose look depends on shader-side alpha or chroma keying are not traceable.
    fn traceable<'a>(
        scene: Option<&MeshMaterial3d<SceneMaterial>>,
        terrain: Option<&MeshMaterial3d<TerrainMaterial>>,
        scene_materials: &'a Assets<SceneMaterial>,
        terrain_materials: &'a Assets<TerrainMaterial>,
    ) -> Option<(UntypedAssetId, &'a StandardMaterial, Vec4)> {
        let (id, base, solid, keyed) = match (scene, terrain) {
            (Some(handle), _) => {
                let material = scene_materials.get(&handle.0)?;
                (
                    handle.0.id().untyped(),
                    &material.base,
                    material.extension.solid_color,
                    material.extension.chroma_key != Vec4::ZERO,
                )
            }
            (None, Some(handle)) => {
                let material = terrain_materials.get(&handle.0)?;
                (handle.0.id().untyped(), &material.base, Vec4::ZERO, false)
            }
            (None, None) => return None,
        };
        (!keyed && matches!(base.alpha_mode, AlphaMode::Opaque)).then_some((id, base, solid))
    }

    /// A copy of `source` with the exact vertex layout Solari accepts: position, normal, UV 0
    /// and tangent, as an indexed triangle list. Tangents are zero because no material here
    /// has a normal map.
    fn standin_mesh(source: &Mesh) -> Option<Mesh> {
        if source.primitive_topology() != PrimitiveTopology::TriangleList {
            return None;
        }
        let positions = source.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
        let normals = source.attribute(Mesh::ATTRIBUTE_NORMAL)?.as_float3()?;
        let count = positions.len();
        let uvs = match source.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uvs)) => uvs.clone(),
            _ => vec![[0.0; 2]; count],
        };
        let indices = match source.indices() {
            Some(indices) => indices.iter().map(|index| index as u32).collect(),
            None => (0..count as u32).collect(),
        };
        Some(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.to_vec())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count])
            .with_inserted_indices(Indices::U32(indices)),
        )
    }

    fn standin_material(base: &StandardMaterial, solid: Vec4) -> StandardMaterial {
        StandardMaterial {
            base_color: if solid.w != 0.0 {
                Color::srgb(solid.x / 256.0, solid.y / 256.0, solid.z / 256.0)
            } else {
                base.base_color
            },
            base_color_texture: base.base_color_texture.clone(),
            cull_mode: base.cull_mode,
            double_sided: base.double_sided,
            // The original has no specular term.
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            opaque_render_method: OpaqueRendererMethod::Deferred,
            ..default()
        }
    }

    // Sources are joined across both material kinds, the proxies and three asset stores.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn sync_proxies(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        mut standard: ResMut<Assets<StandardMaterial>>,
        scene_materials: Res<Assets<SceneMaterial>>,
        terrain_materials: Res<Assets<TerrainMaterial>>,
        mut mesh_events: MessageReader<AssetEvent<Mesh>>,
        sources: Query<
            (
                Entity,
                &Mesh3d,
                &GlobalTransform,
                &InheritedVisibility,
                Option<&MeshMaterial3d<SceneMaterial>>,
                Option<&MeshMaterial3d<TerrainMaterial>>,
                Has<Traced>,
                Option<&Hidden>,
                Option<&RenderLayers>,
            ),
            (
                Without<Proxy>,
                Without<Draw>,
                Without<Flat>,
                Or<(
                    With<MeshMaterial3d<SceneMaterial>>,
                    With<MeshMaterial3d<TerrainMaterial>>,
                )>,
            ),
        >,
        mut proxies: Query<(
            Entity,
            &Proxy,
            &mut Transform,
            &mut GlobalTransform,
            &mut Mesh3d,
            &mut RaytracingMesh3d,
            &mut MeshMaterial3d<StandardMaterial>,
        )>,
        mut stand_ins: Local<StandIns>,
    ) {
        // Animated and re-prepared source meshes are edited in place; rebuild their stand-ins.
        for event in mesh_events.read() {
            if let AssetEvent::Modified { id } = event
                && let Some(handle) = stand_ins.meshes.get(id)
                && let Some(mesh) = meshes.get(*id).and_then(standin_mesh)
                && let Err(error) = meshes.insert(handle.id(), mesh)
            {
                warn!("ray tracing stand-in mesh {id}: {error}");
            }
        }
        stand_ins.meshes.retain(|id, _| meshes.contains(*id));

        let mut live = HashSet::new();
        for (entity, proxy, mut transform, mut global, mut mesh, mut tracing, mut material) in
            &mut proxies
        {
            let traceable = sources.get(proxy.source).ok().and_then(
                |(_, source_mesh, source_global, inherited, scene, terrain, ..)| {
                    let (id, base, solid) =
                        traceable(scene, terrain, &scene_materials, &terrain_materials)?;
                    inherited
                        .get()
                        .then_some((source_mesh, source_global, id, base, solid))
                },
            );
            let Some((source_mesh, source_global, id, base, solid)) = traceable else {
                commands.entity(entity).despawn();
                continue;
            };
            live.insert(proxy.source);
            if *global != *source_global {
                *global = *source_global;
                *transform = source_global.compute_transform();
            }
            if let Some(handle) = stand_ins.meshes.get(&source_mesh.0.id())
                && mesh.0 != *handle
            {
                mesh.0 = handle.clone();
                tracing.0 = handle.clone();
            }
            let handle = stand_ins
                .materials
                .entry(id)
                .or_insert_with(|| standard.add(standin_material(base, solid)));
            if material.0 != *handle {
                material.0 = handle.clone();
            }
            // Texture animation edits the source material's base texture in place.
            if standard
                .get(&*handle)
                .is_some_and(|stand_in| stand_in.base_color_texture != base.base_color_texture)
                && let Some(mut stand_in) = standard.get_mut(&*handle)
            {
                stand_in.base_color_texture = base.base_color_texture.clone();
            }
        }

        for (entity, source_mesh, global, inherited, scene, terrain, traced, hidden, layers) in
            &sources
        {
            let traceable = inherited
                .get()
                .then(|| traceable(scene, terrain, &scene_materials, &terrain_materials))
                .flatten();
            let Some((id, base, solid)) = traceable else {
                if traced && let Some(hidden) = hidden {
                    unhide(&mut commands, entity, hidden);
                }
                continue;
            };
            if live.contains(&entity) {
                continue;
            }
            let mesh = match stand_ins.meshes.get(&source_mesh.0.id()) {
                Some(mesh) => mesh.clone(),
                None => {
                    let Some(stand_in) = meshes.get(&source_mesh.0).and_then(standin_mesh) else {
                        continue;
                    };
                    let handle = meshes.add(stand_in);
                    stand_ins.meshes.insert(source_mesh.0.id(), handle.clone());
                    handle
                }
            };
            let material = stand_ins
                .materials
                .entry(id)
                .or_insert_with(|| standard.add(standin_material(base, solid)))
                .clone();
            commands.spawn((
                Proxy { source: entity },
                Mesh3d(mesh.clone()),
                RaytracingMesh3d(mesh),
                MeshMaterial3d(material),
                global.compute_transform(),
                *global,
            ));
            let mut source = commands.entity(entity);
            source.insert(Traced);
            if hidden.is_none() {
                source.insert((Hidden(layers.cloned()), RenderLayers::none()));
            }
        }
    }

    /// Clipped model draws are children of their source; they stop drawing with it.
    #[allow(clippy::type_complexity)] // Bevy query filters.
    fn hide_draws(
        mut commands: Commands,
        draws: Query<(Entity, &ChildOf, Option<&RenderLayers>), (With<Draw>, Without<Hidden>)>,
        traced: Query<(), With<Traced>>,
    ) {
        for (draw, parent, layers) in &draws {
            if traced.contains(parent.parent()) {
                commands
                    .entity(draw)
                    .insert((Hidden(layers.cloned()), RenderLayers::none()));
            }
        }
    }

    /// Everything the lights were last built from.
    struct Built {
        environment: AssetId<Environment>,
        lights: Vec<LocalLight>,
    }

    // The lights join the active environment, the retained script world and the camera.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn sync_lights(
        mut commands: Commands,
        request: Res<EnvironmentRequest>,
        environments: Res<Assets<Environment>>,
        mut meshes: ResMut<Assets<Mesh>>,
        mut materials: ResMut<Assets<StandardMaterial>>,
        mut images: ResMut<Assets<Image>>,
        existing: Query<Entity, With<Light>>,
        cameras: Query<(Entity, Option<&EnvironmentMapLight>), With<ViewCamera>>,
        owners: (
            Option<Res<cyrus_script::script::ScriptActors>>,
            Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
            Option<Res<cyrus_world::animation::AnimationRequest>>,
            Option<Res<Assets<cyrus_assets::animation::Animations>>>,
        ),
        mut built: Local<Option<Built>>,
    ) {
        let Some(environment) = environments.get(&request.0) else {
            return;
        };
        let retained = owners
            .0
            .as_ref()
            .zip(owners.1.as_ref())
            .and_then(|(actors, prepared)| {
                let current = actors.current.as_ref()?;
                owns_prepared_environment(
                    current,
                    prepared,
                    &request,
                    owners.2.as_deref(),
                    owners.3.as_deref(),
                )
                .then_some(current)
            });
        let lights = retained.map_or_else(
            || environment.object_lights.clone(),
            |current| current.object_lights(),
        );
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
        // Marks that lights were built, even for an environment that has none.
        commands.spawn(Light);

        let lighting = &environment.model_lighting;
        let peak = |rgb: [f32; 3]| rgb.into_iter().fold(0.0, f32::max).min(255.0);
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
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::IDENTITY.looking_to(direction, Vec3::Y),
            ));
        }

        let ambient = lighting.ambient.map(|value| value.clamp(0.0, 255.0) as u8);
        let mut cube = Image::new(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 6,
            },
            TextureDimension::D2,
            (0..6)
                .flat_map(|_| [ambient[0], ambient[1], ambient[2], 255])
                .collect(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        cube.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        });
        let cube = images.add(cube);
        let environment_light = EnvironmentMapLight {
            diffuse_map: cube.clone(),
            specular_map: cube,
            intensity: AMBIENT_LUMINANCE * peak(lighting.ambient) / 255.0,
            ..default()
        };
        for (camera, _) in &cameras {
            commands.entity(camera).insert(environment_light.clone());
        }

        // Solari lights come from directional lights and emissive meshes only.
        let sphere = Sphere::new(OBJECT_LIGHT_SIZE)
            .mesh()
            .ico(2)
            .ok()
            .map(|mesh| {
                let count = mesh.count_vertices();
                meshes.add(
                    mesh.with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count]),
                )
            });
        if let Some(sphere) = sphere {
            for light in lights
                .iter()
                .filter(|light| light.radius > 0 && light.rgb.iter().all(|value| *value >= 0))
            {
                let [x, y, z] = cyrus_game::rope::world_position(light.position.map(f64::from));
                let radius = f32::from(light.radius) / cyrus_game::ENGINE_UNIT_SCALE;
                let [r, g, b] = light.rgb.map(|value| f32::from(value) / 255.0);
                commands.spawn((
                    Light,
                    RaytracingMesh3d(sphere.clone()),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::BLACK,
                        emissive: LinearRgba::rgb(r, g, b)
                            * (OBJECT_LIGHT_LUMINANCE * radius * radius),
                        ..default()
                    })),
                    Transform::from_xyz(x, y, z),
                ));
            }
        }
        *built = Some(Built {
            environment: request.0.id(),
            lights,
        });
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn stand_in_mesh_has_exactly_the_layout_solari_accepts() {
            let source = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0; 2]; 3])
            .with_inserted_indices(Indices::U16(vec![0, 1, 2]));
            let stand_in = standin_mesh(&source).expect("triangle list with normals");
            let attributes: Vec<_> = stand_in
                .attributes()
                .map(|(attribute, _)| attribute.id)
                .collect();
            assert_eq!(
                attributes,
                [
                    Mesh::ATTRIBUTE_POSITION.id,
                    Mesh::ATTRIBUTE_NORMAL.id,
                    Mesh::ATTRIBUTE_UV_0.id,
                    Mesh::ATTRIBUTE_TANGENT.id,
                ]
            );
            assert!(
                matches!(stand_in.indices(), Some(Indices::U32(indices)) if indices == &[0, 1, 2])
            );
        }

        #[test]
        fn meshes_without_normals_get_no_stand_in() {
            let source = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
            assert!(standin_mesh(&source).is_none());
        }
    }
}
