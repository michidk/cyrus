//! Documented Glide fog and clipping for exported scene materials.
mod clipping;
mod compositing;
mod effects;
mod preparation;
mod texture;
pub use preparation::{
    SceneMaterialAssetsPlugin, prepare_scene_materials, validate_scene_materials,
};
pub use texture::{
    EffectDraw, EffectDrawView, TextureAnimations, TextureFrame, TexturePlaybackSnapshot,
    TextureSourcesSnapshot,
};

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
#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
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

fn owns_prepared_environment(
    world: &cyrus_game::script::ScriptWorld,
    prepared: &cyrus_world::lifecycle::PreparedWorld,
    environment: &EnvironmentRequest,
    request: Option<&cyrus_world::animation::AnimationRequest>,
    animations: Option<&Assets<cyrus_assets::animation::Animations>>,
) -> bool {
    let Some(request) = request else { return false };
    let Some(animations) = animations.and_then(|assets| assets.get(&prepared.animations)) else {
        return false;
    };
    prepared.world.is_none_or(|id| id == world.world)
        && environment.0.path() == Some(&prepared.environment_path)
        && request.0 == prepared.animations
        && animations.world == world.world
        && animations.metadata["script_content_id"].as_str() == Some(world.content_id.as_str())
}

type MaterialCache = HashMap<
    (
        AssetId<StandardMaterial>,
        i32,
        Option<[u8; 3]>,
        Option<(Entity, u64)>,
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

#[derive(Asset, TypePath, AsBindGroup, Clone, PartialEq)]
#[bind_group_data(ModelKey)]
struct ModelExtension {
    #[uniform(100)]
    fog: FogExtension,
    #[texture(101, sample_type = "float", filterable = false)]
    lights: Handle<Image>,
    // Share one model buffer to stay within the minimum WebGL fragment binding budget.
    #[uniform(102)]
    placement: Mat4,
    #[uniform(102)]
    lighting: Vec4,
    #[uniform(102)]
    solid_color: Vec4,
    #[uniform(102)]
    compositing: Vec4,
    #[uniform(102)]
    chroma_key: Vec4,
    #[uniform(102)]
    texture_layout: UVec4,
    #[uniform(102)]
    texture_scale: Vec4,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ModelKey {
    flat: bool,
    write_depth: bool,
    always_pass: bool,
}

impl From<&ModelExtension> for ModelKey {
    fn from(extension: &ModelExtension) -> Self {
        Self {
            flat: extension.compositing.x >= 0.0,
            write_depth: extension.compositing.y as u16 & 0x200 == 0,
            always_pass: extension.compositing.y as u16 & 0x100 != 0,
        }
    }
}

impl MaterialExtension for ModelExtension {
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if layout.0.contains(clipping::PROJECTED) {
            descriptor.vertex.shader_defs.push("PREPARED_MODEL".into());
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader_defs.push("PREPARED_MODEL".into());
            }
            let extra = layout.0.get_layout(&[
                clipping::PROJECTED.at_shader_location(6),
                crate::terrain::RED.at_shader_location(7),
                crate::terrain::GREEN.at_shader_location(8),
                crate::terrain::BLUE.at_shader_location(9),
                crate::terrain::TEXTURE[0].at_shader_location(10),
                crate::terrain::TEXTURE[1].at_shader_location(11),
                crate::terrain::TEXTURE[2].at_shader_location(12),
                crate::terrain::TEXTURE[3].at_shader_location(13),
                crate::terrain::TEXTURE[4].at_shader_location(14),
            ])?;
            descriptor.vertex.buffers[0]
                .attributes
                .extend(extra.attributes);
        }
        if key.bind_group_data.flat
            && let Some(depth) = &mut descriptor.depth_stencil
        {
            depth.depth_write_enabled = Some(key.bind_group_data.write_depth);
            depth.depth_compare = Some(if key.bind_group_data.always_pass {
                bevy::render::render_resource::CompareFunction::Always
            } else {
                bevy::render::render_resource::CompareFunction::Greater
            });
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
        effects::register(app);
        clipping::register(app);
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
                (apply_scene_fog, prepare_flat_compositing)
                    .after(crate::flat::PrepareFlats)
                    .after(cyrus_game::script::OrdinaryUpdates),
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
            Option<&bevy::gltf::GltfMaterialExtras>,
        ),
        Without<RejectedLighting>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    names: Query<&Name>,
    hierarchy: (
        Query<&ChildOf>,
        Query<(), With<crate::persistence::StagedRenderRoot>>,
        Query<&cyrus_game::script::ScriptBinding>,
    ),
    mut fog_materials: ResMut<Assets<SceneMaterial>>,
    mut converted: Local<MaterialCache>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut terrain_cache: Local<HashMap<AssetId<StandardMaterial>, Handle<TerrainMaterial>>>,
    runtime: (
        Local<Option<(AssetId<Environment>, FogExtension)>>,
        Local<crate::lighting::runtime::RuntimeLights>,
        ResMut<Assets<Image>>,
        Query<(Entity, &MeshMaterial3d<SceneMaterial>)>,
        Query<(Entity, &MeshMaterial3d<TerrainMaterial>)>,
        Query<&bevy::gltf::GltfExtras>,
        Query<&crate::flat::Flat>,
        (
            Option<Res<cyrus_game::script::ScriptActors>>,
            Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
            Option<Res<cyrus_world::animation::AnimationRequest>>,
            Option<Res<Assets<cyrus_assets::animation::Animations>>>,
        ),
        crate::lighting::runtime::LightView,
    ),
) {
    let (parents, staged, actor_bindings) = hierarchy;
    let (
        mut previous,
        mut lights,
        mut images,
        scene_handles,
        terrain_handles,
        node_extras,
        flats,
        light_owners,
        light_view,
    ) = runtime;
    let (Some(environment), Some([near, far])) =
        (environments.get(&request.0), distance.effective(&fidelity))
    else {
        return;
    };
    let position = cameras
        .iter()
        .find_map(|(_, transform)| transform.map(|t| t.translation))
        .unwrap_or_default();
    let retained_world = light_owners
        .0
        .as_ref()
        .zip(light_owners.1.as_ref())
        .and_then(|(actors, prepared)| {
            let current = actors.current.as_ref()?;
            owns_prepared_environment(
                current,
                prepared,
                &request,
                light_owners.2.as_deref(),
                light_owners.3.as_deref(),
            )
            .then_some(current)
        });
    let retained_lights = retained_world.map(|current| current.object_lights());
    let light_texture = lights.update_for_map_target(
        request.0.id(),
        retained_lights
            .as_deref()
            .unwrap_or(&environment.object_lights),
        light_view.position(
            position,
            retained_world
                .and_then(|world| world.camera())
                .map(|camera| camera.current),
        ),
        &mut images,
    );
    for (mut projection, _) in &mut cameras {
        if let Projection::Custom(custom) = &mut *projection
            && let Some(crate::camera::projection::GameProjection(perspective)) = custom.get_mut()
            && (perspective.near != near / 20.0 || perspective.far != far / 20.0)
        {
            perspective.near = near / 20.0;
            perspective.far = far / 20.0;
        }
    }
    let extension = fog_extension(environment, &fidelity, [near, far]);
    if request.is_changed()
        || previous
            .as_ref()
            .is_none_or(|(source, _)| *source != request.0.id())
    {
        converted.clear();
        terrain_cache.clear();
    }
    let is_live = |entity| {
        !std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| staged.contains(node))
    };
    for (entity, handle) in &scene_handles {
        if !is_live(entity) {
            continue;
        }
        if let Some(mut material) = fog_materials.get_mut(&handle.0)
            && material.extension.lights != light_texture
        {
            material.extension.lights = light_texture.clone();
        }
    }
    if previous.as_ref().is_none_or(|(_, old)| *old != extension) {
        // Retained assets from a departed world must not trigger fresh GPU uploads.
        for id in scene_handles
            .iter()
            .filter(|(entity, _)| is_live(*entity))
            .map(|(_, h)| h.0.id())
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
            .filter(|(entity, _)| is_live(*entity))
            .map(|(_, h)| h.0.id())
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
    for (entity, handle, mut mesh, extras, material_extras) in &mut entities {
        if std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| staged.contains(node))
        {
            continue;
        }
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
        let static_instance = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .find_map(|ancestor| {
                names.get(ancestor).ok().and_then(|name| {
                    environment
                        .prelighting
                        .instances
                        .get(name.as_str())
                        .map(|instance| {
                            (
                                ancestor,
                                0,
                                crate::lighting::ModelLightingInstance::from(instance),
                            )
                        })
                })
            });
        let actor_binding = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .find_map(|ancestor| {
                actor_bindings
                    .get(ancestor)
                    .ok()
                    .map(|binding| (ancestor, binding))
            });
        // Static actor geometry can arrive before the logical binding. Keep its
        // source material until the entry checkpoint can be resolved.
        let actor_geometry = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|ancestor| {
                names.get(ancestor).is_ok_and(|name| {
                    name.as_str()
                        .strip_prefix("B_")
                        .and_then(|name| name.split_once('_'))
                        .is_some_and(|(index, _)| index.parse::<usize>().is_ok())
                })
            });
        if actor_geometry
            && actor_binding.is_none()
            && !environment.prelighting.instances.is_empty()
            && !environment.prelighting.lights.is_empty()
        {
            continue;
        }
        let instance = if !environment.prelighting.instances.is_empty()
            && !environment.prelighting.lights.is_empty()
            && let Some((ancestor, binding)) = actor_binding
        {
            let Some(world) = retained_world.filter(|world| {
                binding.actor.generation == world.generation()
                    && light_owners
                        .2
                        .as_ref()
                        .is_some_and(|request| binding.source == request.0)
            }) else {
                continue;
            };
            let Ok(input) = world.actor_prelight(binding.actor) else {
                continue;
            };
            input.map(|input| {
                (
                    ancestor,
                    world.generation(),
                    crate::lighting::ModelLightingInstance::from(input),
                )
            })
        } else {
            static_instance
        };
        let mut radius = 0;
        let mut solid_rgb = None;
        let mut attributes = None;
        let mut chroma_key = Vec4::ZERO;
        let mut texture_layout = UVec4::ZERO;
        let mut texture_scale = Vec4::ZERO;
        if base
            .base_color_texture
            .as_ref()
            .is_some_and(|image| !images.contains(image))
        {
            continue;
        }
        if !terrain {
            let Some(original) = meshes.get(&mesh.0) else {
                continue;
            };
            let prepared = extras
                .ok_or_else(|| anyhow::anyhow!("missing model lighting metadata"))
                .and_then(|extras| {
                    prepare_model_geometry(
                        environment,
                        base,
                        original,
                        &extras.value,
                        material_extras.map(|extras| extras.value.as_str()),
                        std::iter::once(entity)
                            .chain(parents.iter_ancestors(entity))
                            .filter_map(|ancestor| {
                                node_extras
                                    .get(ancestor)
                                    .ok()
                                    .map(|extras| extras.value.as_str())
                            }),
                        flats.get(entity).ok(),
                        instance.as_ref().map(|(_, _, instance)| instance),
                        &images,
                    )
                });
            match prepared {
                Ok(prepared) => {
                    solid_rgb = prepared.solid_rgb;
                    attributes = prepared.attributes;
                    chroma_key = prepared.chroma_key;
                    texture_layout = prepared.texture_layout;
                    texture_scale = prepared.texture_scale;
                    mesh.0 = meshes.add(prepared.mesh);
                    radius = prepared.radius;
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
                solid_rgb,
                instance
                    .as_ref()
                    .map(|(entity, generation, _)| (*entity, *generation)),
                attributes,
                flats.get(entity).ok().map(|flat| flat.alpha),
            ))
            .or_insert_with(|| {
                fog_materials.add(model_material(
                    base,
                    &extension,
                    terrain,
                    flats.get(entity).ok(),
                    instance.as_ref().map(|(_, _, instance)| instance),
                    radius,
                    solid_rgb,
                    attributes,
                    chroma_key,
                    texture_layout,
                    texture_scale,
                    &light_texture,
                ))
            })
            .clone();
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(material));
    }
}

#[allow(clippy::too_many_arguments)]
fn model_material(
    base: &StandardMaterial,
    extension: &FogExtension,
    terrain: bool,
    flat: Option<&crate::flat::Flat>,
    instance: Option<&crate::lighting::ModelLightingInstance>,
    radius: i32,
    solid_rgb: Option<[u8; 3]>,
    attributes: Option<u8>,
    chroma_key: Vec4,
    texture_layout: UVec4,
    texture_scale: Vec4,
    light_texture: &Handle<Image>,
) -> SceneMaterial {
    let mut extension = extension.clone();
    extension.water.y = u8::from(terrain) as f32;
    let mut base = base.clone();
    if flat.is_some() {
        base.specular_transmission = 1.0;
    }
    // Known opaque paths bypass the generic material alpha mask.
    if attributes.is_some_and(|flags| flags & 2 == 0) {
        base.alpha_mode = AlphaMode::Opaque;
    }
    SceneMaterial {
        base,
        extension: ModelExtension {
            solid_color: solid_rgb.map_or(Vec4::ZERO, |rgb| {
                Vec3::from_array(rgb.map(f32::from)).extend(1.0)
            }),
            chroma_key,
            texture_layout,
            texture_scale,
            fog: extension,
            compositing: Vec4::new(
                flat.map_or(-1.0, |flat| f32::from(flat.alpha)),
                0.0,
                0.0,
                0.0,
            ),
            lights: light_texture.clone(),
            placement: instance.map_or(Mat4::IDENTITY, |instance| {
                let m = instance.matrix;
                Mat4::from_cols(
                    Vec4::new(m[0][0], m[1][0], m[2][0], 0.0),
                    Vec4::new(m[0][1], m[1][1], m[2][1], 0.0),
                    Vec4::new(m[0][2], m[1][2], m[2][2], 0.0),
                    Vec3::from_array(instance.position).extend(1.0),
                )
            }),
            lighting: Vec4::new(
                flat.map_or(radius as f32 / 256.0, |flat| flat.radius),
                u8::from(instance.is_some()) as f32,
                u8::from(!terrain) as f32,
                attributes.map_or(-1.0, f32::from),
            ),
        },
    }
}

fn fog_extension(
    environment: &Environment,
    fidelity: &Fidelity,
    [near, far]: [f32; 2],
) -> FogExtension {
    FogExtension {
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
    }
}

struct PreparedModelGeometry {
    mesh: Mesh,
    radius: i32,
    solid_rgb: Option<[u8; 3]>,
    attributes: Option<u8>,
    chroma_key: Vec4,
    texture_layout: UVec4,
    texture_scale: Vec4,
}

// Shared validation is independent of asset allocation and entity publication.
#[allow(clippy::too_many_arguments)]
fn prepare_model_geometry<'a>(
    environment: &Environment,
    base: &StandardMaterial,
    original: &Mesh,
    extras: &str,
    material_extras: Option<&str>,
    ancestry: impl IntoIterator<Item = &'a str>,
    flat: Option<&crate::flat::Flat>,
    instance: Option<&crate::lighting::ModelLightingInstance>,
    images: &Assets<Image>,
) -> anyhow::Result<PreparedModelGeometry> {
    let mut chroma_key = Vec4::ZERO;
    let mut texture_layout = UVec4::ZERO;
    let mut texture_scale = Vec4::ZERO;
    let mut attributes = crate::sampling::attributes(ancestry.into_iter())?;
    if flat.is_some() {
        // The material sampling word uses point/chroma bits shared with ROB flags.
        attributes = Some(3);
    }
    if attributes.is_some()
        && base.base_color_texture.is_some()
        && let Some(key) = crate::sampling::chroma_key(material_extras, extras)?
    {
        chroma_key = Vec3::from_array(key.map(f32::from))
            .extend(u8::from(attributes.is_some_and(|flags| flags & 2 != 0)) as f32);
        if flat.is_none() && attributes.is_some_and(|flags| flags & 4 == 0) {
            let image = images
                .get(base.base_color_texture.as_ref().unwrap())
                .ok_or_else(|| anyhow::anyhow!("model texture image is not loaded"))?;
            let layout = rgpre::import::bsi::GlideTextureLayout::new(
                [
                    u16::try_from(image.width())?,
                    u16::try_from(image.height())?,
                ],
                Some(environment.texture_scale),
            )?;
            let [w, h] = layout.allocation_dimensions().map(u32::from);
            let [x, y] = layout.sampling_stride().map(u32::from);
            texture_layout = UVec4::new(w, h, x, y);
            let [u, v] = layout.coordinate_scale();
            texture_scale = Vec4::new(u, v, layout.sampling_level() as f32, 0.0);
        }
    }
    let (mut mesh, radius, solid_rgb) = crate::lighting::model_mesh_at(
        original,
        extras,
        instance
            .filter(|_| !environment.prelighting.lights.is_empty())
            .map(|instance| (environment, instance)),
        attributes.is_some() && flat.is_none(),
    )?;
    if let Some(brightness) = flat.and_then(|flat| flat.brightness)
        && let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_UV_1)
    {
        for value in uv {
            value[0] = f32::from(brightness);
        }
    }
    Ok(PreparedModelGeometry {
        mesh,
        radius,
        solid_rgb,
        attributes,
        chroma_key,
        texture_layout,
        texture_scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn retained_lights_require_matching_prepared_animation_content() -> anyhow::Result<()> {
        use cyrus_assets::{
            animation::Animations, install::GameInstall, native::Files, script::Scripts,
        };
        use cyrus_game::script::{ScriptActors, ScriptWorld};
        use cyrus_world::{animation::AnimationRequest, lifecycle::PreparedWorld};
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = GameInstall::open(&root) else {
            eprintln!("skipped: no install (retained light world binding)");
            return Ok(());
        };
        let files = Files::open(&install)?;
        let mut environment: Environment = serde_json::from_slice(
            &cyrus_assets::game::prepare(&files, "NECRISLE.environment")?["NECRISLE.environment"],
        )?;
        let scripts: Scripts = serde_json::from_slice(
            &cyrus_assets::game::prepare(&files, "NECRISLE.scripts")?["NECRISLE.scripts"],
        )?;
        let current = ScriptWorld::prepare(&scripts, 10)?;
        let light = current
            .object_lights()
            .into_iter()
            .find(|light| light.radius > 0)
            .expect("installed light");
        // An empty fallback makes the shader texture distinguish logical ownership.
        environment.object_lights.clear();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Environment>()
            .init_resource::<Assets<Animations>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Fidelity>()
            .insert_resource(RenderDistance {
                original: Some(environment.clipping),
                override_far: None,
            });
        let handle: Handle<Environment> = app
            .world()
            .resource::<AssetServer>()
            .load("NECRISLE.environment");
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .insert(handle.id(), environment)?;
        let animation = app
            .world_mut()
            .resource_mut::<Assets<Animations>>()
            .add(Animations {
                version: 2,
                world: scripts.world,
                metadata: serde_json::json!({"script_content_id": scripts.content_id}),
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            });
        let mut actors = ScriptActors::default();
        actors.current = Some(current);
        app.insert_resource(actors)
            .insert_resource(EnvironmentRequest(handle))
            .insert_resource(AnimationRequest(animation.clone()))
            .insert_resource(PreparedWorld {
                requested_name: "NECRISLE".into(),
                world: None,
                animations: animation.clone(),
                scripts: Handle::default(),
                player_path: "NECRISLE.player".into(),
                scene_path: "NECRISLE.glb".into(),
                environment_path: "NECRISLE.environment".into(),
            })
            .add_systems(Update, apply_scene_fog);
        app.world_mut().spawn((
            ViewCamera,
            Projection::Perspective(Default::default()),
            Transform::from_xyz(
                -light.position[0] as f32 / 20.0,
                0.0,
                ((light.position[2] as f64 - 65535.99609375) / 20.0) as f32,
            ),
        ));
        let height = |app: &App| {
            app.world()
                .resource::<Assets<Image>>()
                .iter()
                .next()
                .unwrap()
                .1
                .texture_descriptor
                .size
                .height
        };
        for declared_world in [None, Some(scripts.world)] {
            app.world_mut().resource_mut::<PreparedWorld>().world = declared_world;
            app.update();
            assert!(
                height(&app) > 256,
                "logical lights reach the shader texture"
            );
            let images = app.world().resource::<Assets<Image>>();
            let image = images.iter().next().unwrap().1;
            let rows = &image.data.as_ref().unwrap()[256 * 8 * 4..];
            let mut expected = Vec::new();
            for value in [
                light.position[0] as f32,
                light.position[1] as f32,
                light.position[2] as f32,
                f32::from(light.radius),
                f32::from(light.rgb[0]),
                f32::from(light.rgb[1]),
                f32::from(light.rgb[2]),
            ] {
                expected.extend(value.to_le_bytes());
            }
            assert!(
                rows.as_chunks::<32>()
                    .0
                    .iter()
                    .any(|row| row[..28] == expected)
            );
            app.world_mut()
                .resource_mut::<Assets<Animations>>()
                .get_mut(&animation)
                .unwrap()
                .metadata["script_content_id"] = serde_json::json!("stale");
            app.update();
            assert_eq!(
                height(&app),
                256,
                "stale script content uses the empty authored fallback"
            );
            app.world_mut()
                .resource_mut::<Assets<Animations>>()
                .get_mut(&animation)
                .unwrap()
                .metadata["script_content_id"] = serde_json::json!(scripts.content_id);
        }
        app.world_mut().resource_mut::<PreparedWorld>().world = Some(scripts.world + 1);
        app.update();
        assert_eq!(height(&app), 256);
        app.world_mut().resource_mut::<PreparedWorld>().world = None;
        app.world_mut().resource_mut::<AnimationRequest>().0 = Handle::default();
        app.update();
        assert_eq!(height(&app), 256);
        app.world_mut().resource_mut::<AnimationRequest>().0 = animation.clone();
        app.world_mut()
            .resource_mut::<Assets<Animations>>()
            .get_mut(&animation)
            .unwrap()
            .world = scripts.world + 1;
        app.update();
        assert_eq!(height(&app), 256);
        app.world_mut()
            .resource_mut::<Assets<Animations>>()
            .get_mut(&animation)
            .unwrap()
            .world = scripts.world;
        app.world_mut()
            .resource_mut::<PreparedWorld>()
            .environment_path = "START.environment".into();
        app.update();
        assert_eq!(height(&app), 256);
        app.world_mut()
            .resource_mut::<PreparedWorld>()
            .environment_path = "NECRISLE.environment".into();
        app.update();
        assert!(height(&app) > 256);
        Ok(())
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn logical_camera_selects_bound_light_rows_before_render_glide() -> anyhow::Result<()> {
        use cyrus_assets::{install::GameInstall, native::Files, script::Scripts};
        use cyrus_game::script::{CameraDrive, CameraFrameState, PublishedCamera, ScriptView};
        use cyrus_game::script::{ScriptActors, ScriptWorld};
        use cyrus_world::lifecycle::PreparedWorld;
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = GameInstall::open(&root) else {
            eprintln!("skipped: no install (logical camera light region)");
            return Ok(());
        };
        let files = Files::open(&install)?;
        let mut scripts: Scripts = serde_json::from_slice(
            &cyrus_assets::game::prepare(&files, "MGUILD.scripts")?["MGUILD.scripts"],
        )?;
        for light in scripts
            .instances
            .iter_mut()
            .filter(|light| light.object_type == 6)
        {
            light.light_radius = 0;
        }
        scripts.instances[0].map_position = [9216, 0, 9728];
        scripts.instances[0].light_radius = 100;
        scripts.instances[0].light_rgb = [255, 0, -255];
        scripts.content_id = scripts.content_identity()?;
        let mut current = ScriptWorld::prepare(&scripts, 10)?;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Environment>();
        let environment: Handle<Environment> = app
            .world()
            .resource::<AssetServer>()
            .load("MGUILD.environment");
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .insert(
                environment.id(),
                Environment {
                    effects: None,
                    texture_scale: [1, 1],
                    sky: None,
                    background: [0; 3],
                    clipping: [7.0, 3800.0],
                    fog_table: [[0; 4]; 16],
                    terrain_heights: None,
                    water: None,
                    terrain_lighting: None,
                    model_lighting: cyrus_assets::environment::ModelLighting {
                        ambient: [64.0; 3],
                        directional: [0.0; 3],
                        direction: [0.0, -1.0, 0.0],
                    },
                    prelighting: Default::default(),
                    object_lights: vec![cyrus_assets::environment::LocalLight {
                        position: [9216, 0, 9728],
                        radius: 100,
                        rgb: [255, 0, -255],
                    }],
                },
            )?;
        let mut animations = Assets::<cyrus_assets::animation::Animations>::default();
        let source = animations.add(cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        });
        let mut frame = CameraFrameState::default();
        frame.published = Some(PublishedCamera {
            pose: cyrus_game::script::camera::CameraPose {
                position: [5120 * 256, 0, 10000 * 256],
                angles: [0; 3],
            },
            render: cyrus_game::script::camera::RenderPose {
                position: [5119.0, 0.0, 10000.0],
                angles: [0; 3],
            },
            view_angles: [0; 3],
        });
        current.prepare_spatial()?;
        current.initialize_camera_pose(frame.published.unwrap().pose)?;
        let mut actors = ScriptActors::default();
        actors.current = Some(current);
        app.insert_resource(actors)
            .insert_resource(animations)
            .insert_resource(PreparedWorld {
                requested_name: "MGUILD".into(),
                world: Some(scripts.world),
                animations: source.clone(),
                scripts: Handle::default(),
                player_path: "MGUILD.player".into(),
                scene_path: "MGUILD.glb".into(),
                environment_path: "MGUILD.environment".into(),
            });
        app.insert_resource(EnvironmentRequest(environment))
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<Fidelity>()
            .insert_resource(RenderDistance {
                original: Some([7.0, 3800.0]),
                override_far: None,
            })
            .insert_resource(CameraDrive { enabled: true })
            .insert_resource(frame)
            .insert_resource(cyrus_world::animation::AnimationRequest(source.clone()))
            // The map-list view precedes this frame's camera update, so its position may differ.
            .insert_resource(ScriptView {
                source: source.clone(),
                position: [5118 * 256, 0, 10000 * 256],
            })
            .add_systems(Update, apply_scene_fog);
        app.world_mut().spawn((
            ViewCamera,
            Transform::from_xyz(
                -5119.0 / 20.0,
                0.0,
                ((10000.0_f64 - 65535.99609375) / 20.0) as f32,
            ),
            Projection::Perspective(Default::default()),
        ));
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let entity = app.world_mut().spawn((
            MeshMaterial3d(material), Mesh3d(mesh),
            bevy::gltf::GltfExtras {
                value: r#"{"rgpre_render":{"version":1,"rob_attributes":null},"rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[64,0],[255,0]]}}"#.into(),
            },
        )).id();
        let rows = |app: &App| {
            let material = &app
                .world()
                .get::<MeshMaterial3d<SceneMaterial>>(entity)
                .unwrap()
                .0;
            let materials = app.world().resource::<Assets<SceneMaterial>>();
            let image = &materials.get(material).unwrap().extension.lights;
            let images = app.world().resource::<Assets<Image>>();
            let image = images.get(image).unwrap();
            let data = image.data.as_ref().unwrap();
            data[256 * 8 * 4..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
                .collect::<Vec<_>>()
        };
        app.update();
        assert_eq!(
            rows(&app),
            vec![9216.0, 0.0, 9728.0, 100.0, 255.0, 0.0, -255.0, 40.234375]
        );
        let expected_pose = app
            .world()
            .resource::<CameraFrameState>()
            .published
            .unwrap()
            .pose;
        let mut different_pose = expected_pose;
        different_pose.position[0] = 5119 * 256;
        let mut replacement = ScriptWorld::prepare(&scripts, 11)?;
        replacement.prepare_spatial()?;
        replacement.initialize_camera_pose(different_pose)?;
        let original = app
            .world_mut()
            .resource_mut::<ScriptActors>()
            .current
            .replace(replacement);
        app.update();
        assert!(
            rows(&app).is_empty(),
            "same-scene replacement cannot consume the previous camera publication"
        );
        app.world_mut().resource_mut::<ScriptActors>().current = original;
        // Source mismatch must not read a previous world's publication.
        app.world_mut().resource_mut::<ScriptView>().source = Handle::default();
        app.update();
        assert!(rows(&app).is_empty());
        app.world_mut().resource_mut::<ScriptView>().source = source;
        app.insert_resource(crate::camera::FlycamSettings {
            enabled: true,
            ..default()
        });
        app.update();
        assert!(rows(&app).is_empty());
        app.world_mut()
            .resource_mut::<crate::camera::FlycamSettings>()
            .enabled = false;
        app.update();
        assert_eq!(rows(&app).len(), 8);
        app.world_mut().resource_mut::<CameraDrive>().enabled = false;
        app.update();
        assert!(rows(&app).is_empty());
        Ok(())
    }

    #[test]
    fn original_clipping_and_material_properties_survive_debug_override_and_reset() {
        let mut app = App::new();
        let mut environments = Assets::<Environment>::default();
        let environment = environments.add(Environment {
            effects: None,
            terrain_heights: Some(cyrus_assets::environment::TerrainHeights {
                side: 2,
                offset: 0,
                heights: vec![0.0; 4],
            }),
            texture_scale: [1, 1],
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
            value: r#"{"rgpre_render":{"version":1,"rob_attributes":null},"rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[64,0],[255,0]]}}"#.into(),
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
        let staged_root = app
            .world_mut()
            .spawn((crate::persistence::StagedRenderRoot, Visibility::Hidden))
            .id();
        let staged_mesh = app
            .world_mut()
            .spawn((
                ChildOf(staged_root),
                MeshMaterial3d(base.clone()),
                Mesh3d(mesh.clone()),
                extras.clone(),
            ))
            .id();
        let terrain_root = app.world_mut().spawn(Name::new("Terrain")).id();
        let terrain = app
            .world_mut()
            .spawn((MeshMaterial3d(base), Mesh3d(mesh), ChildOf(terrain_root)))
            .id();
        app.update();
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(staged_mesh)
                .is_some()
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<SceneMaterial>>(staged_mesh)
                .is_none()
        );

        assert!(validate_scene_materials(app.world(), &[staged_root]).is_err());
        let environments = app
            .world_mut()
            .remove_resource::<Assets<Environment>>()
            .unwrap();
        let environment_id = app.world().resource::<EnvironmentRequest>().0.id();
        let destination = environments.get(environment_id).unwrap();
        let before_meshes = app.world().resource::<Assets<Mesh>>().len();
        let invalid = app
            .world_mut()
            .spawn((
                ChildOf(staged_root),
                Mesh3d(Handle::default()),
                MeshMaterial3d::<StandardMaterial>(Handle::default()),
            ))
            .id();
        assert!(
            prepare_scene_materials(
                app.world_mut(),
                &[staged_root],
                destination,
                environment_id,
                &Fidelity::default(),
                [7.0, 3800.0],
                Vec3::ZERO,
                &destination.object_lights,
                &Default::default(),
            )
            .is_err()
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), before_meshes);
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(staged_mesh)
                .is_some()
        );
        app.world_mut().despawn(invalid);
        prepare_scene_materials(
            app.world_mut(),
            &[staged_root],
            destination,
            environment_id,
            &Fidelity::default(),
            [7.0, 3800.0],
            Vec3::ZERO,
            &destination.object_lights,
            &Default::default(),
        )
        .unwrap();
        validate_scene_materials(app.world(), &[staged_root]).unwrap();
        let staged_material = app
            .world()
            .get::<MeshMaterial3d<SceneMaterial>>(staged_mesh)
            .unwrap()
            .0
            .clone();
        let live_material = app
            .world()
            .get::<MeshMaterial3d<SceneMaterial>>(first)
            .unwrap()
            .0
            .clone();
        let store = app.world().resource::<Assets<SceneMaterial>>();
        assert!(
            store.get(&staged_material).unwrap().extension.fog
                == store.get(&live_material).unwrap().extension.fog
        );
        assert_eq!(
            store.get(&staged_material).unwrap().extension.lighting,
            store.get(&live_material).unwrap().extension.lighting
        );
        let removed = app
            .world_mut()
            .resource_mut::<Assets<SceneMaterial>>()
            .remove(staged_material.id())
            .unwrap();
        assert!(validate_scene_materials(app.world(), &[staged_root]).is_err());
        app.world_mut()
            .resource_mut::<Assets<SceneMaterial>>()
            .insert(staged_material.id(), removed)
            .unwrap();
        app.insert_resource(environments);
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

#[cfg(test)]
mod actor_prelight_tests {
    use super::*;

    #[test]
    fn installed_static_actor_binds_baked_rgb_to_private_mesh() -> anyhow::Result<()> {
        use cyrus_assets::{install::GameInstall, native::Files};
        use cyrus_game::script::ScriptWorld;
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = GameInstall::open(&root) else {
            eprintln!("skipped: no install (bound static actor prelighting)");
            return Ok(());
        };
        let files = Files::open(&install)?;
        let prepare = |name: &str| -> anyhow::Result<Vec<u8>> {
            Ok(cyrus_assets::game::prepare(&files, name)?
                .remove(name)
                .unwrap())
        };
        let scripts = serde_json::from_slice(&prepare("TAVERN.scripts")?)?;
        let animations = serde_json::from_slice(&prepare("TAVERN.animations")?)?;
        let scene = cyrus_game::player::data::PlayerScene::decode(&prepare("TAVERN.player")?)?;
        let models = cyrus_collision::actor_models::ActorModelCollision::new(
            scene.collision.actor_models.as_ref().unwrap(),
        )?;
        let mut logical = ScriptWorld::prepare(&scripts, 80)?;
        logical.prepare_model_resources(&animations, &models)?;
        logical.prepare_actor_prelighting()?;
        let inputs = std::collections::BTreeMap::from([(
            15,
            logical.actor_prelight(logical.id(15)?)?.unwrap().clone(),
        )]);
        let environment_bytes = prepare("TAVERN.environment")?;
        let environment: Environment = serde_json::from_slice(&environment_bytes)?;
        assert_eq!(environment.prelighting.instances.len(), 21);
        assert_eq!(environment.prelighting.lights.len(), 10);
        let glb = gltf::Gltf::from_slice(&prepare("TAVERN.glb")?)?;
        let node = glb
            .nodes()
            .find(|node| node.name() == Some("B_015_FIRESND"))
            .unwrap();
        let primitive = node.mesh().unwrap().primitives().next().unwrap();
        let read = |semantic| -> Vec<[f32; 3]> {
            let accessor = primitive.get(&semantic).unwrap();
            assert_eq!(accessor.data_type(), gltf::accessor::DataType::F32);
            assert_eq!(accessor.dimensions(), gltf::accessor::Dimensions::Vec3);
            let view = accessor.view().unwrap();
            let bytes = glb.blob.as_ref().unwrap();
            let start = view.offset() + accessor.offset();
            (0..accessor.count())
                .map(|i| {
                    std::array::from_fn(|axis| {
                        let offset = start + i * view.stride().unwrap_or(12) + axis * 4;
                        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
                    })
                })
                .collect()
        };
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, read(gltf::Semantic::Positions));
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, read(gltf::Semantic::Normals));
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<SceneMaterial>>();
        world.init_resource::<Assets<TerrainMaterial>>();
        let original = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let root = world
            .spawn((
                Name::new("B_015_FIRESND"),
                bevy::gltf::GltfExtras {
                    value: node.extras().as_ref().unwrap().get().to_owned(),
                },
                Visibility::Hidden,
            ))
            .id();
        let entity = world
            .spawn((
                ChildOf(root),
                Mesh3d(original.clone()),
                MeshMaterial3d(material),
                bevy::gltf::GltfExtras {
                    value: primitive.extras().as_ref().unwrap().get().to_owned(),
                },
            ))
            .id();
        let mut environments = Assets::<Environment>::default();
        let environment_handle =
            environments.add(serde_json::from_slice::<Environment>(&environment_bytes)?);
        world.insert_resource(environments);
        world.insert_resource(EnvironmentRequest(environment_handle));
        world.insert_resource(Fidelity::default());
        world.insert_resource(RenderDistance {
            original: Some([7.0, 3800.0]),
            override_far: None,
        });
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_scene_fog);
        schedule.run(&mut world);
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .is_some(),
            "unbound static actor converted before logical readiness"
        );
        assert!(world.get::<MeshMaterial3d<SceneMaterial>>(entity).is_none());
        world
            .entity_mut(root)
            .insert(crate::persistence::StagedRenderRoot);
        preparation::prepare_scene_materials(
            &mut world,
            &[root],
            &environment,
            Handle::<Environment>::default().id(),
            &Fidelity::default(),
            [7.0, 3800.0],
            Vec3::ZERO,
            &[],
            &inputs,
        )?;
        assert!(world.get::<MeshMaterial3d<SceneMaterial>>(entity).is_some());
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .is_none()
        );
        let bound = &world.get::<Mesh3d>(entity).unwrap().0;
        assert_ne!(bound.id(), original.id());
        let meshes = world.resource::<Assets<Mesh>>();
        assert!(
            meshes
                .get(&original)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_COLOR)
                .is_none()
        );
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            meshes.get(bound).unwrap().attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("static actor has no baked RGB")
        };
        assert!(!colors.is_empty());
        eprintln!(
            "FIRESND first RGB {:?}, vertices {}",
            colors[0],
            colors.len()
        );
        assert!(
            colors
                .iter()
                .all(|color| color.iter().all(|value| value.is_finite()))
        );
        Ok(())
    }
}
