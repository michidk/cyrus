//! Documented Glide fog and clipping for exported scene materials.

use crate::{
    environment::{Environment, EnvironmentRequest},
    fidelity::{Area, Fidelity, Mode},
    player::ViewCamera,
};
use bevy::{
    asset::embedded_asset,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::{ShaderRef, load_shader_library},
};
use std::collections::HashMap;

/// Original scene clipping and a debug-only Modern graphics override, in engine units.
#[derive(Resource, Default)]
pub struct RenderDistance {
    /// Near/far distances from the active world's environment. None while loading.
    pub original: Option<[f32; 2]>,
    /// Optional far distance selected in the debug menu. Faithful graphics ignores it.
    pub override_far: Option<f32>,
}

impl RenderDistance {
    /// Effective near/far range in engine units; sky clipping remains independent.
    pub fn effective(&self, fidelity: &Fidelity) -> Option<[f32; 2]> {
        self.original.map(|[near, far]| {
            [
                near,
                if fidelity.mode(Area::Graphics) == Mode::Modern {
                    self.override_far.unwrap_or(far)
                } else {
                    far
                },
            ]
        })
    }
}

type SceneMaterial = ExtendedMaterial<StandardMaterial, FogExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone, PartialEq)]
struct FogExtension {
    #[uniform(100)]
    color: Vec4,
    #[uniform(100)]
    clipping: Vec4,
    #[uniform(100)]
    table: [Vec4; 16],
}

impl MaterialExtension for FogExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://cyrus_game/scene_fog.wesl".into()
    }
}

pub(crate) struct FogPlugin;
impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "glide_fog.wesl");
        embedded_asset!(app, "scene_fog.wesl");
        app.init_resource::<RenderDistance>()
            .add_plugins(MaterialPlugin::<SceneMaterial>::default())
            .add_systems(Update, apply_scene_fog);
    }
}

// This system joins the active environment, camera and GLTF material assets.
#[allow(clippy::too_many_arguments)]
fn apply_scene_fog(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    distance: Res<RenderDistance>,
    fidelity: Res<Fidelity>,
    mut cameras: Query<&mut Projection, With<ViewCamera>>,
    meshes: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
    materials: Res<Assets<StandardMaterial>>,
    mut fog_materials: ResMut<Assets<SceneMaterial>>,
    mut converted: Local<HashMap<AssetId<StandardMaterial>, Handle<SceneMaterial>>>,
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
        color: Vec3::from_array(environment.background.map(|v| v as f32 / 255.0)).extend(0.0),
        clipping: Vec4::new(near, far, 0.0, 0.0),
        table: environment
            .fog_table
            .map(|values| Vec4::from_array(values.map(|value| value as f32))),
    };
    if previous
        .as_ref()
        .is_none_or(|(source, _)| *source != request.0.id())
    {
        converted.clear();
    }
    if previous.as_ref().is_none_or(|(_, old)| *old != extension) {
        for (_, material) in fog_materials.iter_mut() {
            material.extension = extension.clone();
        }
    }
    *previous = Some((request.0.id(), extension.clone()));
    for (entity, handle) in &meshes {
        let Some(base) = materials.get(&handle.0) else {
            continue;
        };
        let material = converted
            .entry(handle.0.id())
            .or_insert_with(|| {
                fog_materials.add(SceneMaterial {
                    base: base.clone(),
                    extension: extension.clone(),
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
            sky: None,
            background: [200, 190, 250],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            water: None,
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
            .init_resource::<Assets<SceneMaterial>>()
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
        let first = app.world_mut().spawn(MeshMaterial3d(base.clone())).id();
        let second = app.world_mut().spawn(MeshMaterial3d(base)).id();
        app.update();
        let handle = app
            .world()
            .get::<MeshMaterial3d<SceneMaterial>>(first)
            .unwrap()
            .0
            .clone();
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
        assert_eq!(material.base.base_color, Color::srgb(0.2, 0.4, 0.6));
        assert_eq!(material.base.alpha_mode, AlphaMode::Mask(0.4));
        assert!(material.base.unlit);
        assert_eq!(material.base.cull_mode, None);
        for (mode, override_far, expected) in [
            (Mode::Faithful, Some(8000.0), 190.0),
            (Mode::Modern, Some(8000.0), 400.0),
            (Mode::Modern, None, 190.0),
        ] {
            app.world_mut()
                .resource_mut::<Fidelity>()
                .set(Area::Graphics, mode);
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
        }
    }
}
