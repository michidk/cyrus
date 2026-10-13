//! Documented Glide fog and clipping for exported scene materials.

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
use std::collections::HashMap;

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

type SceneMaterial = ExtendedMaterial<StandardMaterial, FogExtension>;
type MaterialCache = HashMap<AssetId<StandardMaterial>, Handle<SceneMaterial>>;
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

impl MaterialExtension for FogExtension {
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
        load_shader_library!(app, "glide_fog.wesl");
        load_shader_library!(app, "scene_shading.wesl");
        embedded_asset!(app, "scene_fog.wesl");
        embedded_asset!(app, "terrain.wesl");
        app.init_resource::<RenderDistance>()
            .add_plugins((
                MaterialPlugin::<SceneMaterial>::default(),
                MaterialPlugin::<TerrainMaterial>::default(),
            ))
            .add_systems(Update, apply_scene_fog);
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
    mut cameras: Query<&mut Projection, With<ViewCamera>>,
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
    mut previous: Local<Option<(AssetId<Environment>, FogExtension)>>,
) {
    let (Some(environment), Some([near, far])) =
        (environments.get(&request.0), distance.effective(&fidelity))
    else {
        return;
    };
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = &mut *projection
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
    if previous
        .as_ref()
        .is_none_or(|(source, _)| *source != request.0.id())
    {
        converted.clear();
        terrain_cache.clear();
    }
    if previous.as_ref().is_none_or(|(_, old)| *old != extension) {
        for (_, material) in fog_materials.iter_mut() {
            let terrain = material.extension.water.y;
            material.extension = extension.clone();
            material.extension.water.y = terrain;
        }
    }
    if previous.as_ref().is_none_or(|(_, old)| *old != extension) {
        for (_, material) in terrain_materials.iter_mut() {
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
        if !terrain {
            let Some(original) = meshes.get(&mesh.0) else {
                continue;
            };
            let prepared = extras
                .ok_or_else(|| anyhow::anyhow!("missing model lighting metadata"))
                .and_then(|extras| {
                    let instance = (!environment.prelighting.lights.is_empty())
                        .then(|| {
                            std::iter::once(entity)
                                .chain(parents.iter_ancestors(entity))
                                .find_map(|ancestor| {
                                    names.get(ancestor).ok().and_then(|name| {
                                        environment.prelighting.instances.get(name.as_str())
                                    })
                                })
                        })
                        .flatten();
                    crate::lighting::model_mesh(
                        original,
                        &extras.value,
                        instance.map(|instance| (environment, instance)),
                    )
                });
            match prepared {
                Ok(private) => mesh.0 = meshes.add(private),
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
            .entry(handle.0.id())
            .or_insert_with(|| {
                let mut extension = extension.clone();
                extension.water.y = u8::from(terrain) as f32;
                fog_materials.add(SceneMaterial {
                    base: base.clone(),
                    extension,
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
            prelighting: Default::default(),
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
            .spawn((ViewCamera, Projection::default()))
            .id();
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let extras = bevy::gltf::GltfExtras {
            value: r#"{"rgpre_lighting":{"version":3,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[64,0],[255,0]]}}"#.into(),
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
            .spawn((MeshMaterial3d(base.clone()), Mesh3d(mesh.clone()), extras))
            .id();
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
        assert_eq!(material.extension.ambient, Vec4::new(64.0, 64.0, 64.0, 0.0));
        assert_eq!(
            material.extension.directional,
            Vec4::new(255.0, 255.0, 255.0, 0.0)
        );
        assert_eq!(material.extension.direction, Vec4::new(0.0, -1.0, 0.0, 0.0));
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
            let Projection::Perspective(projection) =
                app.world().get::<Projection>(camera).unwrap()
            else {
                panic!("expected perspective camera");
            };
            assert_eq!(projection.near, 0.35);
            assert_eq!(projection.far, expected);
            let material = app
                .world()
                .resource::<Assets<SceneMaterial>>()
                .get(&handle)
                .unwrap();
            assert_eq!(material.extension.clipping.y, expected * 20.0);
            assert_eq!(material.extension.table, [Vec4::ZERO; 16]);
            let terrain_material = app
                .world()
                .resource::<Assets<TerrainMaterial>>()
                .get(&terrain_handle)
                .unwrap();
            assert_eq!(
                terrain_material.extension.fog.clipping,
                material.extension.clipping
            );
            assert_eq!(terrain_material.extension.fog.water.y, 1.0);
        }
    }
}
