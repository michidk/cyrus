//! Shared materials for the models under modern lighting, so the camera's passes batch their draws.
//!
//! Every model has its own material because the material carries the original's per-object
//! placement, light radius and light texture, which only the original's vertex lighting reads.
//! Modern lighting replaces that lighting, so models whose remaining material values are equal
//! look the same through one material. Bevy batches draws by material; with one material per
//! model, every visible model was a batch of its own in the depth prepass and the opaque pass.
//! While `Area::Lighting` is Modern each static scene model draws with the shared material for its
//! values, and back in Faithful it gets its own again, brought up to date with the fog, light
//! texture and animation frame the shared one received meanwhile. Script actors keep their own:
//! their texture sets change per actor.
use super::super::SceneMaterial;
use super::super::texture::Binding;
use crate::camera::ViewCamera;
use crate::environment::EnvironmentRequest;
use crate::persistence::StagedRenderRoot;
use bevy::{camera::visibility::RenderLayers, platform::collections::HashMap, prelude::*};
use cyrus_script::script::ScriptBinding;

/// The shared material for each set of modern-relevant material values in the current world.
#[derive(Resource, Default)]
pub(super) struct SharedMaterials {
    environment: Option<AssetId<cyrus_assets::environment::Environment>>,
    by_key: HashMap<String, Handle<SceneMaterial>>,
}

/// A model drawing with a shared material, and its own material for Faithful.
#[derive(Component)]
pub(super) struct Unshared(Handle<SceneMaterial>);

/// A model that keeps its own material under modern lighting.
#[derive(Component)]
pub(super) struct KeepsOwn;

/// The values of `material` that modern lighting reads, with the animation it follows; models with
/// equal keys look the same through one material. Flats keep the original's shading and return
/// `None`.
fn key(material: &SceneMaterial, animation: Option<&Binding>) -> Option<String> {
    let model = &material.extension;
    if model.compositing.x >= 0.0 {
        return None;
    }
    Some(format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{:?}|{}",
        material.base,
        model.solid_color,
        model.chroma_key,
        model.compositing,
        model.lighting.w,
        model.texture_layout,
        model.texture_scale,
        model.fog.water.y,
        animation.map(|binding| &binding.key),
        model.calm_water,
    ))
}

/// Moves visible static scene models onto shared materials.
// Bevy supplies the model, hierarchy and camera queries as separate parameters.
#[allow(clippy::type_complexity)]
pub(super) fn share(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    mut shared: ResMut<SharedMaterials>,
    mut materials: ResMut<Assets<SceneMaterial>>,
    mut models: Query<
        (
            Entity,
            &mut MeshMaterial3d<SceneMaterial>,
            Option<&RenderLayers>,
            Option<&Binding>,
        ),
        (
            Without<Unshared>,
            Without<KeepsOwn>,
            With<super::glow::GlowChecked>,
        ),
    >,
    (parents, excluded): (
        Query<&ChildOf>,
        Query<(), Or<(With<ScriptBinding>, With<StagedRenderRoot>)>>,
    ),
    cameras: Query<Option<&RenderLayers>, With<ViewCamera>>,
) {
    if shared.environment != Some(request.0.id()) {
        shared.environment = Some(request.0.id());
        shared.by_key.clear();
    }
    let seen = cameras.iter().fold(RenderLayers::none(), |seen, layers| {
        seen.union(layers.unwrap_or(&RenderLayers::default()))
    });
    for (entity, mut handle, layers, animation) in &mut models {
        // Clipped models' camera-space draws stay hidden under modern lighting; they are skipped
        // until the camera sees them.
        if !layers.unwrap_or(&RenderLayers::default()).intersects(&seen) {
            continue;
        }
        let Some(material) = materials.get(&handle.0) else {
            continue;
        };
        let actor = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|node| excluded.contains(node));
        let Some(key) = key(material, animation).filter(|_| !actor) else {
            commands.entity(entity).insert(KeepsOwn);
            continue;
        };
        let material = material.clone();
        let target = shared
            .by_key
            .entry(key)
            .or_insert_with(|| materials.add(material))
            .clone();
        let own = std::mem::replace(&mut handle.0, target);
        commands.entity(entity).insert(Unshared(own));
    }
}

/// Gives every model its own material back, with what the shared one received meanwhile.
pub(super) fn unshare(
    commands: &mut Commands,
    shared: &mut SharedMaterials,
    materials: &mut Assets<SceneMaterial>,
    models: &mut Query<(Entity, &mut MeshMaterial3d<SceneMaterial>, &Unshared)>,
    kept: &Query<Entity, With<KeepsOwn>>,
) {
    for (entity, mut handle, Unshared(own)) in models.iter_mut() {
        if let Some(current) = materials.get(&handle.0).cloned()
            && let Some(mut material) = materials.get_mut(own)
        {
            let terrain = material.extension.fog.water.y;
            material.base.base_color_texture = current.base.base_color_texture;
            material.extension.fog = current.extension.fog;
            material.extension.fog.water.y = terrain;
            material.extension.lights = current.extension.lights;
        }
        handle.0 = own.clone();
        commands.entity(entity).remove::<Unshared>();
    }
    for entity in kept {
        commands.entity(entity).remove::<KeepsOwn>();
    }
    shared.by_key.clear();
}

#[cfg(test)]
mod tests {
    use super::super::super::{FogExtension, model_material};
    use super::*;

    fn fog(lights_on: f32) -> FogExtension {
        FogExtension {
            color: Vec4::ZERO,
            clipping: Vec4::ZERO,
            table: [Vec4::ZERO; 16],
            water: Vec4::ZERO,
            ambient: Vec4::ZERO,
            directional: Vec4::ZERO,
            direction: Vec4::ZERO,
            lighting: Vec4::new(lights_on, 0.0, 0.0, 0.0),
        }
    }

    /// A model whose per-object light radius differs, which modern lighting does not read.
    fn model(world: &mut World, radius: i32, chroma_key: Vec4) -> Entity {
        let material = model_material(
            &StandardMaterial::default(),
            &fog(1.0),
            false,
            None,
            None,
            radius,
            None,
            None,
            chroma_key,
            UVec4::ZERO,
            Vec4::ZERO,
            &Handle::default(),
        );
        let handle = world.resource_mut::<Assets<SceneMaterial>>().add(material);
        world
            .spawn((MeshMaterial3d(handle), super::super::glow::GlowChecked))
            .id()
    }

    fn material(world: &World, entity: Entity) -> AssetId<SceneMaterial> {
        world
            .get::<MeshMaterial3d<SceneMaterial>>(entity)
            .unwrap()
            .0
            .id()
    }

    #[test]
    fn equal_modern_values_share_a_material_until_faithful_restores_each_own() {
        let mut world = World::new();
        world.init_resource::<Assets<SceneMaterial>>();
        world.init_resource::<SharedMaterials>();
        world.insert_resource(EnvironmentRequest(Handle::default()));
        world.spawn(ViewCamera);
        let first = model(&mut world, 10, Vec4::ZERO);
        let own = material(&world, first);
        let second = model(&mut world, 20, Vec4::ZERO);
        let keyed = model(&mut world, 10, Vec4::ONE);
        let staged = world.spawn(StagedRenderRoot).id();
        let actor = model(&mut world, 10, Vec4::ZERO);
        world.entity_mut(actor).insert(ChildOf(staged));

        world.run_system_cached(share).unwrap();
        assert_eq!(material(&world, first), material(&world, second));
        assert_ne!(material(&world, first), own);
        assert_ne!(material(&world, keyed), material(&world, first));
        assert!(world.entity(actor).contains::<KeepsOwn>());

        // Faithful lighting arrives through the shared material; the restored one must carry it.
        let shared = world
            .get::<MeshMaterial3d<SceneMaterial>>(first)
            .unwrap()
            .0
            .clone();
        world
            .resource_mut::<Assets<SceneMaterial>>()
            .get_mut(&shared)
            .unwrap()
            .extension
            .fog = fog(0.0);
        world
            .run_system_cached(
                |mut commands: Commands,
                 mut shared: ResMut<SharedMaterials>,
                 mut materials: ResMut<Assets<SceneMaterial>>,
                 mut models: Query<(Entity, &mut MeshMaterial3d<SceneMaterial>, &Unshared)>,
                 kept: Query<Entity, With<KeepsOwn>>| {
                    unshare(
                        &mut commands,
                        &mut shared,
                        &mut materials,
                        &mut models,
                        &kept,
                    );
                },
            )
            .unwrap();
        assert_eq!(material(&world, first), own);
        assert!(!world.entity(first).contains::<Unshared>());
        assert!(!world.entity(actor).contains::<KeepsOwn>());
        let restored = world.resource::<Assets<SceneMaterial>>().get(own).unwrap();
        assert!(!restored.extension.fog.modern_lighting());
        // The per-object values the original lighting reads stay the model's own.
        assert_eq!(restored.extension.lighting.x, 10.0 / 256.0);
    }
}
