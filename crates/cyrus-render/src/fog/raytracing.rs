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
pub(crate) struct Proxy;

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
        ecs::system::SystemParam,
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
    use cyrus_assets::environment::{Environment, LocalLight, ModelLighting};
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

    /// The source mesh a [`Proxy`] stands in for.
    #[derive(Component)]
    struct StandInOf(Entity);

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

    /// The asset stores a stand-in is built from; all four are needed together.
    #[derive(SystemParam)]
    struct StandInAssets<'w> {
        meshes: ResMut<'w, Assets<Mesh>>,
        standard: ResMut<'w, Assets<StandardMaterial>>,
        scene: Res<'w, Assets<SceneMaterial>>,
        terrain: Res<'w, Assets<TerrainMaterial>>,
    }

    impl StandIns {
        /// Animated and re-prepared source meshes are edited in place; rebuild their stand-ins.
        fn rebuild_edited(
            &mut self,
            meshes: &mut Assets<Mesh>,
            events: &mut MessageReader<AssetEvent<Mesh>>,
        ) {
            for event in events.read() {
                if let AssetEvent::Modified { id } = event
                    && let Some(handle) = self.meshes.get(id)
                    && let Some(mesh) = meshes.get(*id).and_then(standin_mesh)
                    && let Err(error) = meshes.insert(handle.id(), mesh)
                {
                    warn!("ray tracing stand-in mesh {id}: {error}");
                }
            }
            self.meshes.retain(|id, _| meshes.contains(*id));
        }

        /// The stand-in mesh of a source mesh, built on first use.
        fn mesh(
            &mut self,
            meshes: &mut Assets<Mesh>,
            source: &Handle<Mesh>,
        ) -> Option<Handle<Mesh>> {
            if let Some(mesh) = self.meshes.get(&source.id()) {
                return Some(mesh.clone());
            }
            let handle = meshes.add(meshes.get(source).and_then(standin_mesh)?);
            self.meshes.insert(source.id(), handle.clone());
            Some(handle)
        }

        /// The stand-in material shared by every mesh using one source material.
        fn material(
            &mut self,
            standard: &mut Assets<StandardMaterial>,
            id: UntypedAssetId,
            base: &StandardMaterial,
            solid: Vec4,
        ) -> Handle<StandardMaterial> {
            let handle = self
                .materials
                .entry(id)
                .or_insert_with(|| standard.add(standin_material(base, solid)))
                .clone();
            // Texture animation edits the source material's base texture in place.
            if standard
                .get(&handle)
                .is_some_and(|stand_in| stand_in.base_color_texture != base.base_color_texture)
                && let Some(mut stand_in) = standard.get_mut(&handle)
            {
                stand_in.base_color_texture = base.base_color_texture.clone();
            }
            handle
        }
    }

    type SourceData = (
        Entity,
        &'static Mesh3d,
        &'static GlobalTransform,
        &'static InheritedVisibility,
        Option<&'static MeshMaterial3d<SceneMaterial>>,
        Option<&'static MeshMaterial3d<TerrainMaterial>>,
        Has<Traced>,
        Option<&'static Hidden>,
        Option<&'static RenderLayers>,
    );
    type SourceFilter = (
        Without<Proxy>,
        Without<Draw>,
        Without<Flat>,
        Or<(
            With<MeshMaterial3d<SceneMaterial>>,
            With<MeshMaterial3d<TerrainMaterial>>,
        )>,
    );
    type ProxyData = (
        Entity,
        &'static StandInOf,
        &'static mut Transform,
        &'static mut GlobalTransform,
        &'static mut Mesh3d,
        &'static mut RaytracingMesh3d,
        &'static mut MeshMaterial3d<StandardMaterial>,
    );

    fn sync_proxies(
        mut commands: Commands,
        mut assets: StandInAssets,
        mut mesh_events: MessageReader<AssetEvent<Mesh>>,
        sources: Query<SourceData, SourceFilter>,
        mut proxies: Query<ProxyData>,
        mut stand_ins: Local<StandIns>,
    ) {
        stand_ins.rebuild_edited(&mut assets.meshes, &mut mesh_events);
        let live = update_proxies(
            &mut commands,
            &mut assets,
            &mut stand_ins,
            &sources,
            &mut proxies,
        );
        spawn_proxies(&mut commands, &mut assets, &mut stand_ins, &sources, &live);
    }

    /// Follows each stand-in to its source, and drops those whose source stopped being traceable.
    /// Returns the sources that still have one.
    fn update_proxies(
        commands: &mut Commands,
        assets: &mut StandInAssets,
        stand_ins: &mut StandIns,
        sources: &Query<SourceData, SourceFilter>,
        proxies: &mut Query<ProxyData>,
    ) -> HashSet<Entity> {
        let mut live = HashSet::new();
        for (entity, stand_in, mut transform, mut global, mut mesh, mut tracing, mut material) in
            proxies
        {
            let traceable = sources.get(stand_in.0).ok().and_then(
                |(_, source_mesh, source_global, inherited, scene, terrain, ..)| {
                    let (id, base, solid) =
                        traceable(scene, terrain, &assets.scene, &assets.terrain)?;
                    inherited
                        .get()
                        .then_some((source_mesh, source_global, id, base, solid))
                },
            );
            let Some((source_mesh, source_global, id, base, solid)) = traceable else {
                commands.entity(entity).despawn();
                continue;
            };
            live.insert(stand_in.0);
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
            let handle = stand_ins.material(&mut assets.standard, id, base, solid);
            if material.0 != handle {
                material.0 = handle;
            }
        }
        live
    }

    /// Gives every traceable source without a stand-in one, and hides the source's own draw.
    fn spawn_proxies(
        commands: &mut Commands,
        assets: &mut StandInAssets,
        stand_ins: &mut StandIns,
        sources: &Query<SourceData, SourceFilter>,
        live: &HashSet<Entity>,
    ) {
        for (entity, source_mesh, global, inherited, scene, terrain, traced, hidden, layers) in
            sources
        {
            let traceable = inherited
                .get()
                .then(|| traceable(scene, terrain, &assets.scene, &assets.terrain))
                .flatten();
            let Some((id, base, solid)) = traceable else {
                if traced && let Some(hidden) = hidden {
                    unhide(commands, entity, hidden);
                }
                continue;
            };
            if live.contains(&entity) {
                continue;
            }
            let Some(mesh) = stand_ins.mesh(&mut assets.meshes, &source_mesh.0) else {
                continue;
            };
            let material = stand_ins.material(&mut assets.standard, id, base, solid);
            commands.spawn((
                Proxy,
                StandInOf(entity),
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

    /// The retained script world, whose object lights replace the environment's while it owns it.
    #[derive(SystemParam)]
    struct RetainedLights<'w> {
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
    struct LightAssets<'w> {
        environments: Res<'w, Assets<Environment>>,
        meshes: ResMut<'w, Assets<Mesh>>,
        materials: ResMut<'w, Assets<StandardMaterial>>,
        images: ResMut<'w, Assets<Image>>,
    }

    fn peak(rgb: [f32; 3]) -> f32 {
        rgb.into_iter().fold(0.0, f32::max).min(255.0)
    }

    fn sync_lights(
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
        // Marks that lights were built, even for an environment that has none.
        commands.spawn(Light);
        spawn_sun(&mut commands, &environment.model_lighting);
        let ambient = ambient_light(&mut assets.images, &environment.model_lighting);
        for (camera, _) in &cameras {
            commands.entity(camera).insert(ambient.clone());
        }
        spawn_object_lights(&mut commands, &mut assets, &lights);
        *built = Some(Built {
            environment: request.0.id(),
            lights,
        });
    }

    fn spawn_sun(commands: &mut Commands, lighting: &ModelLighting) {
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

    /// Solari lights come from directional lights and emissive meshes only.
    fn spawn_object_lights(
        commands: &mut Commands,
        assets: &mut LightAssets,
        lights: &[LocalLight],
    ) {
        let Ok(sphere) = Sphere::new(OBJECT_LIGHT_SIZE).mesh().ico(2) else {
            return;
        };
        let count = sphere.count_vertices();
        let sphere = assets
            .meshes
            .add(sphere.with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count]));
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
                MeshMaterial3d(assets.materials.add(StandardMaterial {
                    base_color: Color::BLACK,
                    emissive: LinearRgba::rgb(r, g, b) * (OBJECT_LIGHT_LUMINANCE * radius * radius),
                    ..default()
                })),
                Transform::from_xyz(x, y, z),
            ));
        }
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
