//! Documented Glide fog and clipping for exported scene materials.
mod compositing;
mod texture;

use crate::camera::ViewCamera;
use crate::environment::EnvironmentRequest;
use bevy::{
    asset::embedded_asset,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{ShaderRef, load_shader_library},
};
use cyrus_assets::environment::Environment;
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use std::collections::{HashMap, HashSet};

/// Original scene clipping and a debug-only Modern fog override, in engine units.
#[derive(Resource, Default)]
pub struct RenderDistance {
    /// Near/far distances from the active world's environment. None while loading.
    pub original: Option<[f32; 2]>,
    /// Optional far distance selected in the debug menu. Faithful fog ignores it.
    pub override_far: Option<f32>,
}

impl RenderDistance {
    /// Effective near/far range in engine units; sky clipping remains independent.
    pub fn effective(&self, fidelity: &Fidelity) -> Option<[f32; 2]> {
        self.original.map(|[near, far]| {
            [
                near,
                if fidelity.mode(Area::Fog) == Mode::Modern {
                    self.override_far.unwrap_or(far * 4.0)
                } else {
                    far
                },
            ]
        })
    }
}

type SceneMaterial = ExtendedMaterial<StandardMaterial, ModelExtension>;
type MaterialCache = HashMap<
    (
        AssetId<StandardMaterial>,
        i32,
        Option<Entity>,
        Option<u8>,
        Option<u16>,
    ),
    Handle<SceneMaterial>,
>;
type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct TerrainExtension {
    #[uniform(100)]
    fog: FogExtension,
}

impl MaterialExtension for TerrainExtension {
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_render/terrain.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://cyrus_render/terrain.wesl".into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if layout.0.contains(crate::terrain::PROJECTED) {
            descriptor
                .vertex
                .shader_defs
                .push("PREPARED_TERRAIN".into());
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader_defs.push("PREPARED_TERRAIN".into());
            }
            let extra = layout.0.get_layout(&[
                crate::terrain::PROJECTED.at_shader_location(8),
                crate::terrain::RED.at_shader_location(9),
                crate::terrain::GREEN.at_shader_location(10),
                crate::terrain::BLUE.at_shader_location(11),
                // Prepared terrain removes tangents, leaving location 4 available.
                crate::terrain::TEXTURE[0].at_shader_location(4),
                crate::terrain::TEXTURE[1].at_shader_location(12),
                crate::terrain::TEXTURE[2].at_shader_location(13),
                crate::terrain::TEXTURE[3].at_shader_location(14),
                crate::terrain::TEXTURE[4].at_shader_location(15),
            ])?;
            descriptor.vertex.buffers[0]
                .attributes
                .extend(extra.attributes);
        }
        Ok(())
    }
}

#[derive(Asset, TypePath, AsBindGroup, ShaderType, Clone, PartialEq)]
struct FogExtension {
    #[uniform(100)]
    color: Vec4,
    #[uniform(100)]
    clipping: Vec4,
    #[uniform(100)]
    table: [Vec4; 16],
    #[uniform(100)]
    water: Vec4,
    #[uniform(100)]
    ambient: Vec4,
    #[uniform(100)]
    directional: Vec4,
    #[uniform(100)]
    direction: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(ModelKey)]
struct ModelExtension {
    #[uniform(100)]
    fog: FogExtension,
    #[texture(101, sample_type = "float", filterable = false)]
    lights: Handle<Image>,
    #[uniform(102)]
    placement: Mat4,
    #[uniform(102)]
    lighting: Vec4,
    #[uniform(103)]
    compositing: Vec4,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ModelKey {
    flat: bool,
}

impl From<&ModelExtension> for ModelKey {
    fn from(extension: &ModelExtension) -> Self {
        Self {
            flat: extension.compositing.x >= 0.0,
        }
    }
}

impl MaterialExtension for ModelExtension {
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if key.bind_group_data.flat
            && let Some(depth) = &mut descriptor.depth_stencil
        {
            depth.depth_write_enabled = Some(true);
            depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Greater);
        }
        Ok(())
    }
    fn vertex_shader() -> ShaderRef {
        "embedded://cyrus_render/scene_fog.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://cyrus_render/scene_fog.wesl".into()
    }
}

#[derive(Component)]
struct RejectedLighting;

pub(crate) struct FogPlugin;
impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        texture::register(app);
        compositing::register(app);
        load_shader_library!(app, "glide_fog.wesl");
        load_shader_library!(app, "scene_shading.wesl");
        load_shader_library!(app, "model_lights.wesl");
        load_shader_library!(app, "sampling.wesl");
        load_shader_library!(app, "terrain_texture.wesl");
        embedded_asset!(app, "scene_fog.wesl");
        embedded_asset!(app, "terrain.wesl");
        app.init_resource::<RenderDistance>()
            .add_plugins((
                MaterialPlugin::<SceneMaterial>::default(),
                MaterialPlugin::<TerrainMaterial>::default(),
            ))
            .add_systems(
                Update,
                (apply_scene_fog, prepare_flat_compositing).after(crate::flat::PrepareFlats),
            );
    }
}

fn prepare_flat_compositing(
    mut commands: Commands,
    mut cameras: Query<(Entity, Option<&mut bevy::pbr::ScreenSpaceTransmission>), With<ViewCamera>>,
    flats: Query<&crate::flat::Flat>,
) {
    let steps = flats.iter().len().max(1);
    for (camera, transmission) in &mut cameras {
        if let Some(mut transmission) = transmission {
            if transmission.steps != steps {
                transmission.steps = steps;
            }
        } else {
            commands
                .entity(camera)
                .insert(bevy::pbr::ScreenSpaceTransmission { steps, ..default() });
        }
    }
}

// This system joins the active environment, camera and GLTF material assets.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply_scene_fog(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    distance: Res<RenderDistance>,
    fidelity: Res<Fidelity>,
    mut cameras: Query<(&mut Projection, Option<&Transform>), With<ViewCamera>>,
    mut entities: Query<
        (
            Entity,
            &MeshMaterial3d<StandardMaterial>,
            &mut Mesh3d,
            Option<&bevy::gltf::GltfExtras>,
        ),
        Without<RejectedLighting>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut fog_materials: ResMut<Assets<SceneMaterial>>,
    mut converted: Local<MaterialCache>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut terrain_cache: Local<HashMap<AssetId<StandardMaterial>, Handle<TerrainMaterial>>>,
    runtime: (
        Local<Option<(AssetId<Environment>, FogExtension)>>,
        Local<crate::lighting::runtime::RuntimeLights>,
        ResMut<Assets<Image>>,
        Query<&MeshMaterial3d<SceneMaterial>>,
        Query<&MeshMaterial3d<TerrainMaterial>>,
        Query<&bevy::gltf::GltfExtras>,
        Query<&crate::flat::Flat>,
    ),
) {
    let (mut previous, mut lights, mut images, scene_handles, terrain_handles, node_extras, flats) =
        runtime;
    let (Some(environment), Some([near, far])) =
        (environments.get(&request.0), distance.effective(&fidelity))
    else {
        return;
    };
    let position = cameras
        .iter()
        .find_map(|(_, transform)| transform.map(|t| t.translation))
        .unwrap_or_default();
    let light_texture = lights.update(request.0.id(), environment, position, &mut images);
    for (mut projection, _) in &mut cameras {
        if let Projection::Custom(custom) = &mut *projection
            && let Some(crate::camera::projection::GameProjection(perspective)) = custom.get_mut()
            && (perspective.near != near / 20.0 || perspective.far != far / 20.0)
        {
            perspective.near = near / 20.0;
            perspective.far = far / 20.0;
        }
    }
    let extension = FogExtension {
        ambient: Vec3::from(environment.model_lighting.ambient).extend(0.0),
        directional: Vec3::from(environment.model_lighting.directional).extend(0.0),
        direction: Vec3::from(environment.model_lighting.direction).extend(0.0),
        color: Vec3::from_array(environment.background.map(|v| v as f32 / 255.0)).extend(0.0),
        clipping: Vec4::new(
            near,
            far,
            u8::from(fidelity.mode(Area::Fog) == Mode::Modern) as f32,
            0.0,
        ),
        water: Vec4::new(
            u8::from(fidelity.mode(Area::Water) == Mode::Modern) as f32,
            0.0,
            0.0,
            0.0,
        ),
        table: environment
            .fog_table
            .map(|values| Vec4::from_array(values.map(|value| value as f32))),
    };
    if request.is_changed()
        || previous
            .as_ref()
            .is_none_or(|(source, _)| *source != request.0.id())
    {
        converted.clear();
        terrain_cache.clear();
    }
    if previous.as_ref().is_none_or(|(_, old)| *old != extension) {
        // Retained assets from a departed world must not trigger fresh GPU uploads.
        for id in scene_handles
            .iter()
            .map(|h| h.0.id())
            .collect::<HashSet<_>>()
        {
            let Some(mut material) = fog_materials.get_mut(id) else {
                continue;
            };
            let terrain = material.extension.fog.water.y;
            material.extension.fog = extension.clone();
            material.extension.fog.water.y = terrain;
        }
        for id in terrain_handles
            .iter()
            .map(|h| h.0.id())
            .collect::<HashSet<_>>()
        {
            let Some(mut material) = terrain_materials.get_mut(id) else {
                continue;
            };
            material.extension.fog = extension.clone();
            material.extension.fog.water.y = 1.0;
        }
    }
    *previous = Some((request.0.id(), extension.clone()));
    for (entity, handle, mut mesh, extras) in &mut entities {
        let Some(base) = materials.get(&handle.0) else {
            continue;
        };
        let terrain = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|ancestor| {
                names
                    .get(ancestor)
                    .is_ok_and(|name| name.as_str() == "Terrain")
            });
        if terrain && environment.terrain_lighting.is_some() {
            let material = terrain_cache
                .entry(handle.0.id())
                .or_insert_with(|| {
                    let mut fog = extension.clone();
                    fog.water.y = 1.0;
                    terrain_materials.add(TerrainMaterial {
                        base: base.clone(),
                        extension: TerrainExtension { fog },
                    })
                })
                .clone();
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert(MeshMaterial3d(material));
            continue;
        }
        let instance = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .find_map(|ancestor| {
                names.get(ancestor).ok().and_then(|name| {
                    environment
                        .prelighting
                        .instances
                        .get(name.as_str())
                        .map(|instance| (ancestor, instance))
                })
            });
        let mut radius = 0;
        let mut attributes = None;
        if !terrain {
            let Some(original) = meshes.get(&mesh.0) else {
                continue;
            };
            let prepared = extras
                .ok_or_else(|| anyhow::anyhow!("missing model lighting metadata"))
                .and_then(|extras| {
                    attributes = crate::sampling::attributes(
                        std::iter::once(entity)
                            .chain(parents.iter_ancestors(entity))
                            .filter_map(|ancestor| {
                                node_extras
                                    .get(ancestor)
                                    .ok()
                                    .map(|extras| extras.value.as_str())
                            }),
                    )?;
                    if flats.contains(entity) {
                        // The material sampling word uses point/chroma bits shared with ROB flags.
                        attributes = Some(2);
                    }
                    crate::lighting::model_mesh(
                        original,
                        &extras.value,
                        instance
                            .filter(|_| !environment.prelighting.lights.is_empty())
                            .map(|(_, instance)| (environment, instance)),
                    )
                });
            match prepared {
                Ok((mut private, bound)) => {
                    if let Some(brightness) =
                        flats.get(entity).ok().and_then(|flat| flat.brightness)
                        && let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
                            private.attribute_mut(Mesh::ATTRIBUTE_UV_1)
                    {
                        for value in uv {
                            value[0] = f32::from(brightness);
                        }
                    }
                    mesh.0 = meshes.add(private);
                    radius = bound;
                }
                Err(error) => {
                    error!(
                        "world {} mesh {:?}: {error}; re-export the world",
                        request
                            .0
                            .path()
                            .map_or("unknown".into(), ToString::to_string),
                        names.get(entity).map(Name::as_str)
                    );
                    commands
                        .entity(entity)
                        .insert((RejectedLighting, Visibility::Hidden));
                    continue;
                }
            }
        }
        let material = converted
            .entry((
                handle.0.id(),
                radius,
                instance.map(|(entity, _)| entity),
                attributes,
                flats.get(entity).ok().map(|flat| flat.alpha),
            ))
            .or_insert_with(|| {
                let mut extension = extension.clone();
                extension.water.y = u8::from(terrain) as f32;
                let mut base = base.clone();
                let flat = flats.get(entity).ok();
                if flat.is_some() {
                    base.specular_transmission = 1.0;
                }
                // shortcut: chroma-enabled meshes retain alpha masking until texture-key provenance is recovered.
                if attributes.is_some_and(|flags| flags & 2 == 0) {
                    base.alpha_mode = AlphaMode::Opaque;
                }
                fog_materials.add(SceneMaterial {
                    base,
                    extension: ModelExtension {
                        fog: extension,
                        compositing: Vec4::new(
                            flat.map_or(-1.0, |flat| f32::from(flat.alpha)),
                            0.0,
                            0.0,
                            0.0,
                        ),
                        lights: light_texture.clone(),
                        placement: instance.map_or(Mat4::IDENTITY, |(_, instance)| {
                            let m = instance.matrix.map(|value| value as f32 / 268_435_456.0);
                            Mat4::from_cols(
                                Vec4::new(m[0], m[3], m[6], 0.0),
                                Vec4::new(m[1], m[4], m[7], 0.0),
                                Vec4::new(m[2], m[5], m[8], 0.0),
                                Vec3::from_array(instance.position.map(|value| value as f32))
                                    .extend(1.0),
                            )
                        }),
                        lighting: Vec4::new(
                            flat.map_or(radius as f32 / 256.0, |flat| flat.radius),
                            u8::from(instance.is_some()) as f32,
                            u8::from(!terrain) as f32,
                            attributes.map_or(-1.0, f32::from),
                        ),
                    },
                })
            })
            .clone();
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(material));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_clipping_and_material_properties_survive_debug_override_and_reset() {
        let mut app = App::new();
        let mut environments = Assets::<Environment>::default();
        let environment = environments.add(Environment {
            object_lights: vec![cyrus_assets::environment::LocalLight {
                position: [512, 0, 512],
                radius: 100,
                rgb: [255, 0, -255],
            }],
            prelighting: cyrus_assets::environment::Prelighting {
                instances: [(
                    "static".into(),
                    cyrus_assets::environment::StaticLightingInstance {
                        position: [123, 456, 789],
                        matrix: [268_435_456, 0, 0, 0, 268_435_456, 0, 0, 0, 268_435_456],
                    },
                )]
                .into(),
                lights: Vec::new(),
            },
            model_lighting: cyrus_assets::environment::ModelLighting {
                ambient: [64.0; 3],
                directional: [255.0; 3],
                direction: [0.0, -1.0, 0.0],
            },
            sky: None,
            background: [200, 190, 250],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: Some(cyrus_assets::environment::TerrainLighting {
                side: 2,
                ambient: [64.0; 3],
                directional: [255.0; 3],
                direction: [0.0, -1.0, 0.0],
            }),
        });
        let mut materials = Assets::<StandardMaterial>::default();
        let base = materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.4, 0.6),
            alpha_mode: AlphaMode::Mask(0.4),
            unlit: true,
            cull_mode: None,
            ..default()
        });
        app.insert_resource(environments)
            .insert_resource(EnvironmentRequest(environment))
            .insert_resource(materials)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Fidelity>()
            .insert_resource(RenderDistance {
                original: Some([7.0, 3800.0]),
                override_far: Some(8000.0),
            })
            .add_systems(Update, apply_scene_fog);
        let camera = app
            .world_mut()
            .spawn((
                ViewCamera,
                Transform::from_xyz(0.0, 0.0, -3276.7998),
                Projection::custom(crate::camera::projection::GameProjection::default()),
            ))
            .id();
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let extras = bevy::gltf::GltfExtras {
            value: r#"{"rgpre_render":{"version":1,"rob_attributes":null},"rgpre_lighting":{"version":3,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[64,0],[255,0]]}}"#.into(),
        };
        let first = app
            .world_mut()
            .spawn((
                MeshMaterial3d(base.clone()),
                Mesh3d(mesh.clone()),
                extras.clone(),
            ))
            .id();
        let second = app
            .world_mut()
            .spawn((
                MeshMaterial3d(base.clone()),
                Mesh3d(mesh.clone()),
                extras.clone(),
            ))
            .id();
        let static_root = app.world_mut().spawn(Name::new("static")).id();
        let placed = app
            .world_mut()
            .spawn((
                MeshMaterial3d(base.clone()),
                Mesh3d(mesh.clone()),
                extras.clone(),
                ChildOf(static_root),
            ))
            .id();
        let larger = app
            .world_mut()
            .spawn((
                MeshMaterial3d(base.clone()),
                Mesh3d(mesh.clone()),
                bevy::gltf::GltfExtras {
                    value: extras
                        .value
                        .replace("\"radius_fixed\":0", "\"radius_fixed\":256"),
                },
            ))
            .id();
        let flagged: Vec<_> = [0, 3]
            .into_iter()
            .map(|flags| {
                let root = app
                    .world_mut()
                    .spawn(bevy::gltf::GltfExtras {
                        value: format!(
                            r#"{{"rgpre_render":{{"version":1,"rob_attributes":{flags}}}}}"#
                        ),
                    })
                    .id();
                app.world_mut()
                    .spawn((
                        MeshMaterial3d(base.clone()),
                        Mesh3d(mesh.clone()),
                        ChildOf(root),
                        bevy::gltf::GltfExtras {
                            value: extras.value.replace(
                                r#""rgpre_render":{"version":1,"rob_attributes":null},"#,
                                "",
                            ),
                        },
                    ))
                    .id()
            })
            .collect();
        let terrain_root = app.world_mut().spawn(Name::new("Terrain")).id();
        let terrain = app
            .world_mut()
            .spawn((MeshMaterial3d(base), Mesh3d(mesh), ChildOf(terrain_root)))
            .id();
        app.update();
        let handle = app
            .world()
            .get::<MeshMaterial3d<SceneMaterial>>(first)
            .unwrap()
            .0
            .clone();
        let placed_handle = &app
            .world()
            .get::<MeshMaterial3d<SceneMaterial>>(placed)
            .unwrap()
            .0;
        let larger_handle = &app
            .world()
            .get::<MeshMaterial3d<SceneMaterial>>(larger)
            .unwrap()
            .0;
        assert_ne!(*placed_handle, handle);
        assert_ne!(*larger_handle, handle);
        let flag_handles: Vec<_> = flagged
            .iter()
            .map(|&entity| {
                app.world()
                    .get::<MeshMaterial3d<SceneMaterial>>(entity)
                    .unwrap()
                    .0
                    .clone()
            })
            .collect();
        assert_ne!(flag_handles[0], flag_handles[1]);
        let scene_materials = app.world().resource::<Assets<SceneMaterial>>();
        for (handle, flags, alpha) in [
            (&flag_handles[0], 0.0, AlphaMode::Opaque),
            (&flag_handles[1], 3.0, AlphaMode::Mask(0.4)),
        ] {
            let material = scene_materials.get(handle).unwrap();
            assert_eq!(material.extension.lighting.w, flags);
            assert_eq!(material.base.alpha_mode, alpha);
        }

        let placement = &scene_materials.get(placed_handle).unwrap().extension;
        assert_eq!(
            placement.placement.w_axis,
            Vec4::new(123.0, 456.0, 789.0, 1.0)
        );
        assert_eq!(placement.lighting.y, 1.0);
        assert_eq!(
            scene_materials
                .get(larger_handle)
                .unwrap()
                .extension
                .lighting
                .x,
            1.0
        );
        let light_image = placement.lights.clone();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&light_image)
                .unwrap()
                .height(),
            257
        );
        let terrain_handle = app
            .world()
            .get::<MeshMaterial3d<TerrainMaterial>>(terrain)
            .unwrap()
            .0
            .clone();
        let terrain_material = app
            .world()
            .resource::<Assets<TerrainMaterial>>()
            .get(&terrain_handle)
            .unwrap();
        assert_eq!(terrain_material.base.alpha_mode, AlphaMode::Mask(0.4));
        assert_eq!(terrain_material.extension.fog.water.y, 1.0);
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<SceneMaterial>>(second)
                .unwrap()
                .0,
            handle
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(first)
                .is_none()
        );
        let material = app
            .world()
            .resource::<Assets<SceneMaterial>>()
            .get(&handle)
            .unwrap();
        assert_eq!(
            material.extension.fog.ambient,
            Vec4::new(64.0, 64.0, 64.0, 0.0)
        );
        assert_eq!(
            material.extension.fog.directional,
            Vec4::new(255.0, 255.0, 255.0, 0.0)
        );
        assert_eq!(
            material.extension.fog.direction,
            Vec4::new(0.0, -1.0, 0.0, 0.0)
        );
        let mesh = app.world().get::<Mesh3d>(first).unwrap();
        assert!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&mesh.0)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_UV_1)
                .is_some()
        );
        assert_eq!(material.base.base_color, Color::srgb(0.2, 0.4, 0.6));
        assert_eq!(material.base.alpha_mode, AlphaMode::Mask(0.4));
        assert!(material.base.unlit);
        assert_eq!(material.base.cull_mode, None);
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation
            .x = -1000.0;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&light_image)
                .unwrap()
                .height(),
            256
        );
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation
            .x = 0.0;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&light_image)
                .unwrap()
                .height(),
            257
        );
        for (mode, override_far, expected) in [
            (Mode::Faithful, Some(8000.0), 190.0),
            (Mode::Modern, Some(8000.0), 400.0),
            (Mode::Modern, None, 760.0),
        ] {
            app.world_mut()
                .resource_mut::<Fidelity>()
                .set(Area::Fog, mode);
            app.world_mut()
                .resource_mut::<RenderDistance>()
                .override_far = override_far;
            app.update();
            let Projection::Custom(custom) = app.world().get::<Projection>(camera).unwrap() else {
                panic!("expected game camera projection");
            };
            let crate::camera::projection::GameProjection(projection) = custom.get().unwrap();
            assert_eq!(projection.near, 0.35);
            assert_eq!(projection.far, expected);
            let material = app
                .world()
                .resource::<Assets<SceneMaterial>>()
                .get(&handle)
                .unwrap();
            assert_eq!(material.extension.fog.clipping.y, expected * 20.0);
            assert_eq!(material.extension.fog.table, [Vec4::ZERO; 16]);
            let terrain_material = app
                .world()
                .resource::<Assets<TerrainMaterial>>()
                .get(&terrain_handle)
                .unwrap();
            assert_eq!(
                terrain_material.extension.fog.clipping,
                material.extension.fog.clipping
            );
            assert_eq!(terrain_material.extension.fog.water.y, 1.0);
        }
        for entity in [first, second, terrain] {
            app.world_mut().despawn(entity);
        }
        app.world_mut()
            .resource_mut::<RenderDistance>()
            .override_far = Some(9000.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<SceneMaterial>>()
                .get(&handle)
                .unwrap()
                .extension
                .fog
                .clipping
                .y,
            15200.0,
            "retained materials without scene entities must not be uploaded again"
        );
        assert_eq!(
            app.world()
                .resource::<Assets<TerrainMaterial>>()
                .get(&terrain_handle)
                .unwrap()
                .extension
                .fog
                .clipping
                .y,
            15200.0
        );
    }
}
