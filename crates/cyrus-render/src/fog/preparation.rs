//! Off-live material construction using the ordinary geometry and sampling rules.
use super::{
    SceneMaterial, TerrainExtension, TerrainMaterial, fog_extension, model_material,
    prepare_model_geometry,
};
use anyhow::{Context, Result, ensure};
use bevy::prelude::{
    AssetId, Assets, ChildOf, Children, Entity, Handle, Image, Mesh, Mesh3d, MeshMaterial3d, Name,
    StandardMaterial, UVec4, Vec3, Vec4, Visibility, World,
};
use cyrus_assets::environment::Environment;
use cyrus_game::fidelity::Fidelity;
use std::collections::HashSet;

enum PreparedMaterial {
    Model(SceneMaterial),
    Terrain(TerrainMaterial),
}
use bevy::gltf::{GltfExtras, GltfMaterialExtras};

/// Prepare private materials under hidden replacement roots using the destination environment.
/// All geometry and image validation completes before assets or entities are changed.
/// Flat geometry must already have passed normal flat preparation. Texture binding must
/// precede this call, since conversion removes the original standard material component.
#[allow(clippy::too_many_arguments)]
pub fn prepare_scene_materials(
    world: &mut World,
    roots: &[Entity],
    environment: &Environment,
    environment_id: AssetId<Environment>,
    fidelity: &Fidelity,
    clipping: [f32; 2],
    camera_position: Vec3,
    object_lights: &[cyrus_assets::environment::LocalLight],
    actor_prelighting: &std::collections::BTreeMap<usize, cyrus_game::script::ActorPrelight>,
) -> Result<()> {
    ensure!(
        camera_position.is_finite()
            && clipping.iter().all(|v| v.is_finite())
            && clipping[0] > 0.0
            && clipping[1] > clipping[0],
        "replacement material camera/clipping invalid"
    );
    ensure!(
        world.contains_resource::<Assets<SceneMaterial>>()
            && world.contains_resource::<Assets<TerrainMaterial>>(),
        "replacement material stores absent"
    );
    let meshes = world
        .get_resource::<Assets<Mesh>>()
        .context("replacement mesh store absent")?;
    let materials = world
        .get_resource::<Assets<StandardMaterial>>()
        .context("replacement source material store absent")?;
    let images = world
        .get_resource::<Assets<Image>>()
        .context("replacement image store absent")?;
    let extension = fog_extension(environment, fidelity, clipping);
    let mut pending = roots.to_vec();
    let mut seen = HashSet::new();
    let mut prepared = Vec::new();
    for &root in roots {
        ensure!(
            world
                .get::<crate::persistence::StagedRenderRoot>(root)
                .is_some()
                && world.get::<Visibility>(root) == Some(&Visibility::Hidden),
            "replacement material root {root:?} is not hidden staging"
        );
    }
    while let Some(entity) = pending.pop() {
        ensure!(
            seen.insert(entity),
            "replacement material hierarchy repeats {entity:?}"
        );
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
        let Some(mesh) = world.get::<Mesh3d>(entity) else {
            continue;
        };
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .with_context(|| format!("replacement mesh {entity:?} lacks original material"))?;
        let base = materials
            .get(&handle.0)
            .with_context(|| format!("replacement material {entity:?} absent"))?;
        if let Some(image) = &base.base_color_texture {
            ensure!(
                images.contains(image),
                "replacement material {entity:?} image absent"
            );
        }
        let mut ancestors = Vec::new();
        let mut cursor = Some(entity);
        while let Some(node) = cursor {
            ensure!(
                !ancestors.contains(&node),
                "replacement material parent cycle"
            );
            ancestors.push(node);
            cursor = world.get::<ChildOf>(node).map(ChildOf::parent);
        }
        let terrain = ancestors.iter().any(|&node| {
            world
                .get::<Name>(node)
                .is_some_and(|name| name.as_str() == "Terrain")
        });
        let static_instance = ancestors.iter().find_map(|&node| {
            world
                .get::<Name>(node)
                .and_then(|name| environment.prelighting.instances.get(name.as_str()))
                .map(crate::lighting::ModelLightingInstance::from)
        });
        let actor = ancestors.iter().find_map(|&node| {
            world
                .get::<Name>(node)?
                .as_str()
                .strip_prefix("B_")?
                .split_once('_')?
                .0
                .parse::<usize>()
                .ok()
        });
        let instance = if !environment.prelighting.instances.is_empty()
            && !environment.prelighting.lights.is_empty()
            && let Some(index) = actor
        {
            actor_prelighting
                .get(&index)
                .map(crate::lighting::ModelLightingInstance::from)
        } else {
            static_instance
        };
        if terrain && environment.terrain_lighting.is_some() {
            let mut fog = extension.clone();
            fog.water.y = 1.0;
            prepared.push((
                entity,
                None,
                PreparedMaterial::Terrain(TerrainMaterial {
                    base: base.clone(),
                    extension: TerrainExtension { fog },
                }),
            ));
            continue;
        }
        let flat = world.get::<crate::flat::Flat>(entity);
        let geometry = if terrain {
            None
        } else {
            let extras = world
                .get::<GltfExtras>(entity)
                .context("replacement model lighting metadata absent")?;
            Some(
                prepare_model_geometry(
                    environment,
                    base,
                    meshes
                        .get(&mesh.0)
                        .context("replacement model mesh absent")?,
                    &extras.value,
                    world
                        .get::<GltfMaterialExtras>(entity)
                        .map(|extras| extras.value.as_str()),
                    ancestors.iter().filter_map(|&node| {
                        world
                            .get::<GltfExtras>(node)
                            .map(|extras| extras.value.as_str())
                    }),
                    flat,
                    instance.as_ref(),
                    images,
                )
                .with_context(|| format!("replacement model {entity:?}"))?,
            )
        };
        let material = model_material(
            base,
            &extension,
            terrain,
            flat,
            instance.as_ref(),
            geometry.as_ref().map_or(0, |g| g.radius),
            geometry.as_ref().and_then(|g| g.solid_rgb),
            geometry.as_ref().and_then(|g| g.attributes),
            geometry.as_ref().map_or(Vec4::ZERO, |g| g.chroma_key),
            geometry.as_ref().map_or(UVec4::ZERO, |g| g.texture_layout),
            geometry.as_ref().map_or(Vec4::ZERO, |g| g.texture_scale),
            &Handle::default(),
        );
        prepared.push((
            entity,
            geometry.map(|g| g.mesh),
            PreparedMaterial::Model(material),
        ));
    }
    let light_texture = crate::lighting::runtime::RuntimeLights::default().update(
        environment_id,
        object_lights,
        camera_position,
        &mut world.resource_mut::<Assets<Image>>(),
    );
    for (entity, mesh, material) in prepared {
        if let Some(mesh) = mesh {
            let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
            world.entity_mut(entity).insert(Mesh3d(mesh));
        }
        match material {
            PreparedMaterial::Model(mut material) => {
                material.extension.lights = light_texture.clone();
                let handle = world.resource_mut::<Assets<SceneMaterial>>().add(material);
                world
                    .entity_mut(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert(MeshMaterial3d(handle));
            }
            PreparedMaterial::Terrain(material) => {
                let handle = world
                    .resource_mut::<Assets<TerrainMaterial>>()
                    .add(material);
                world
                    .entity_mut(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert(MeshMaterial3d(handle));
            }
        }
    }
    Ok(())
}

/// Recheck prepared geometry and material assets immediately before session publication.
/// Original standard materials indicate incomplete destination preparation and reject.
pub fn validate_scene_materials(world: &World, roots: &[Entity]) -> Result<()> {
    let meshes = world
        .get_resource::<Assets<Mesh>>()
        .context("prepared mesh store absent")?;
    let models = world
        .get_resource::<Assets<SceneMaterial>>()
        .context("prepared model material store absent")?;
    let terrain = world
        .get_resource::<Assets<TerrainMaterial>>()
        .context("prepared terrain material store absent")?;
    let images = world
        .get_resource::<Assets<Image>>()
        .context("prepared image store absent")?;
    let mut pending = roots.to_vec();
    let mut seen = HashSet::new();
    while let Some(entity) = pending.pop() {
        ensure!(
            seen.insert(entity),
            "prepared material hierarchy repeats {entity:?}"
        );
        ensure!(
            world.get_entity(entity).is_ok(),
            "prepared material entity {entity:?} disappeared"
        );
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
        let Some(mesh) = world.get::<Mesh3d>(entity) else {
            continue;
        };
        ensure!(
            meshes.contains(&mesh.0),
            "prepared mesh {entity:?} asset disappeared"
        );
        ensure!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .is_none(),
            "prepared mesh {entity:?} still has an original material"
        );
        let model = world.get::<MeshMaterial3d<SceneMaterial>>(entity);
        let ground = world.get::<MeshMaterial3d<TerrainMaterial>>(entity);
        let base = match (model, ground) {
            (Some(handle), None) => {
                let material = models
                    .get(&handle.0)
                    .context("prepared model material disappeared")?;
                ensure!(
                    images.contains(&material.extension.lights),
                    "prepared model light image disappeared"
                );
                &material.base
            }
            (None, Some(handle)) => {
                &terrain
                    .get(&handle.0)
                    .context("prepared terrain material disappeared")?
                    .base
            }
            _ => anyhow::bail!("prepared mesh {entity:?} material census differs"),
        };
        if let Some(image) = &base.base_color_texture {
            ensure!(
                images.contains(image),
                "prepared mesh {entity:?} texture disappeared"
            );
        }
    }
    Ok(())
}

/// Registers the material asset stores for CPU-only scene preparation consumers.
/// Windowed hosts already register these through their material rendering plugins.
pub struct SceneMaterialAssetsPlugin;
impl bevy::prelude::Plugin for SceneMaterialAssetsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        use bevy::asset::AssetApp;
        app.init_asset::<SceneMaterial>()
            .init_asset::<TerrainMaterial>()
            .init_asset::<crate::sky::SkyMaterial>();
    }
}
