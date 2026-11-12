//! Read-only transient mesh, material and generation observations.
use super::{EffectBillboard, LiveEffects, SceneMaterial, TextureAnimations};
use crate::environment::EnvironmentRequest;
use bevy::prelude::{
    Assets, Entity, GlobalTransform, Image, Mesh, Mesh3d, MeshMaterial3d, Visibility, With, World,
};
use cyrus_assets::environment::Environment;
use cyrus_script::script::ScriptActors;

/// Observe actual transient meshes after PostUpdate, without advancing or repairing them.
/// Runtime identities are process-local; positions use exported scene units.
/// This is CPU binding evidence, not GPU readback or original visual parity.
pub fn inspect_effect_geometry(world: &mut World) -> anyhow::Result<serde_json::Value> {
    use anyhow::{Context, ensure};
    use bevy::mesh::VertexAttributeValues;
    use serde_json::json;
    let entities = world
        .query::<(Entity, &EffectBillboard)>()
        .iter(world)
        .map(|(entity, _)| format!("{entity:?}"))
        .collect::<Vec<_>>();
    let camera_rotation = world
        .query_filtered::<&GlobalTransform, With<crate::camera::ViewCamera>>()
        .single(world)
        .ok()
        .map(|camera| camera.rotation().to_array());
    let live = world
        .get_resource::<LiveEffects>()
        .context("effect geometry: service absent")?;
    let (environment, map, generation) = live
        .owner
        .context("effect geometry: current owner absent")?;
    let current = world
        .get_resource::<ScriptActors>()
        .and_then(|actors| actors.current.as_ref())
        .context("effect geometry: logical world absent")?;
    let request = world.resource::<EnvironmentRequest>();
    ensure!(
        (environment, map, generation) == (request.0.id(), current.world, current.generation()),
        "effect geometry: stale logical owner"
    );
    let textures = world.resource::<TextureAnimations>();
    ensure!(
        textures.effects_error.is_none(),
        "effect geometry: {:?}",
        textures.effects_error
    );
    let source = world
        .resource::<Assets<Environment>>()
        .get(&request.0)
        .and_then(|environment| environment.effects.as_ref())
        .context("effect geometry: installed source absent")?;
    let mut slots = Vec::new();
    for slot in &live.slots {
        let binding = world
            .get::<Mesh3d>(slot.entity)
            .context("effect geometry: mesh binding absent")?;
        let material_binding = world
            .get::<MeshMaterial3d<SceneMaterial>>(slot.entity)
            .context("effect geometry: material binding absent")?;
        ensure!(
            binding.0 == slot.mesh && material_binding.0 == slot.material,
            "effect geometry: slot binding differs from live entity"
        );
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&binding.0)
            .context("effect geometry: mesh absent")?;
        let Some(VertexAttributeValues::Float32x3(vertices)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            anyhow::bail!("effect geometry: positions absent");
        };
        let material = world
            .resource::<Assets<SceneMaterial>>()
            .get(&material_binding.0)
            .context("effect geometry: material absent")?;
        let billboard = world
            .get::<EffectBillboard>(slot.entity)
            .context("effect geometry: billboard absent")?;
        let transform = world
            .get::<GlobalTransform>(slot.entity)
            .context("effect geometry: transform absent")?;
        let texture = material
            .base
            .base_color_texture
            .as_ref()
            .and_then(|image| world.resource::<Assets<Image>>().get(image));
        let texture_hash = texture
            .and_then(|image| image.data.as_deref())
            .map(|data| blake3::hash(data).to_hex().to_string());
        slots.push(json!({"slot": billboard.slot, "entity": format!("{:?}", slot.entity),
            "mesh": format!("{:?}", binding.0.id()), "material": format!("{:?}", material_binding.0.id()),
            "texture": material.base.base_color_texture.as_ref().map(|image| format!("{:?}", image.id())),
            "admitted": billboard.admitted, "alpha": billboard.alpha,
            "vertices": vertices, "translation": transform.translation().to_array(),
            "rotation": transform.rotation().to_array(), "texture_hash": texture_hash,
            "visibility": format!("{:?}", world.get::<Visibility>(slot.entity))}));
    }
    Ok(
        json!({"world": map, "generation": generation, "environment": format!("{environment:?}"),
        "entities": entities, "camera_rotation": camera_rotation, "slots": slots, "retained": textures.effect_draws,
        "logical": current.environmental_effects().map(|pool| pool.active().map(|(slot, effect)|
            json!({"slot":slot,"descriptor":effect.descriptor,"position":effect.position,"frame":effect.frame})).collect::<Vec<_>>()),
        "images": source.images.iter().map(|(descriptor, image)| json!({"descriptor": descriptor,
            "name":image.name,"frames": image.frames.len(),"size":image.size})).collect::<Vec<_>>() }),
    )
}
