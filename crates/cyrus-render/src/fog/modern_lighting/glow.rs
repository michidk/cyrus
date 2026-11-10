//! Lamps and fires that glow under the modern lighting.
//!
//! **Modern only**, a tuning choice. The map's lights shine as point lights (`lights`), but the
//! lantern or fire model at a light keeps its original texture, which looks dark beside the
//! light it gives off. A small model whose bounds hold a map light gets its own copy of its
//! material with an emissive strength in the base material's `emissive`; the shader adds that
//! light on the bright texels only, so the glass and flame glow and the frame does not. Back in
//! Faithful every glowing model gets its original material again, brought up to date.
use super::super::SceneMaterial;
use bevy::{camera::primitives::Aabb, prelude::*};

/// Largest extent, in world units, of a model that can be a lamp; larger models that hold a
/// light, such as the building a lantern hangs on, stay unlit.
const LAMP_SIZE: f32 = 6.0;
/// Distance, in world units, a light may sit outside a lamp's bounds, such as above a flame.
const LAMP_MARGIN: f32 = 1.0;
/// Emissive strength of a lamp's brightest texels, relative to a fully sunlit surface.
pub(super) const LAMP_GLOW: f32 = 2.5;

/// Positions of the map lights that give off light, in world units.
#[derive(Resource, Default, Clone, PartialEq)]
pub(super) struct LampLights(pub(super) Vec<Vec3>);

/// A model that was checked for holding a light.
#[derive(Component)]
pub(super) struct GlowChecked;

/// A glowing model, and the material it had before.
#[derive(Component)]
pub(super) struct Glowing(Handle<SceneMaterial>);

/// Whether a light at `light` sits in a small model with local `bounds` placed by `transform`.
fn holds_light(bounds: &Aabb, transform: &GlobalTransform, light: Vec3) -> bool {
    let affine = transform.affine();
    let center = affine.transform_point3a(bounds.center);
    let half = affine.matrix3.abs() * bounds.half_extents;
    half.max_element() * 2.0 <= LAMP_SIZE
        && (Vec3A::from(light) - center)
            .abs()
            .cmple(half + Vec3A::splat(LAMP_MARGIN))
            .all()
}

/// Gives the models that hold a map light their glowing material copy.
// Bevy supplies the model query and material assets as separate parameters.
#[allow(clippy::type_complexity)]
pub(super) fn find_lamps(
    mut commands: Commands,
    lights: Res<LampLights>,
    mut materials: ResMut<Assets<SceneMaterial>>,
    mut models: Query<
        (
            Entity,
            &mut MeshMaterial3d<SceneMaterial>,
            &Aabb,
            &GlobalTransform,
        ),
        Without<GlowChecked>,
    >,
) {
    for (entity, mut handle, bounds, transform) in &mut models {
        commands.entity(entity).insert(GlowChecked);
        if !lights
            .0
            .iter()
            .any(|light| holds_light(bounds, transform, *light))
        {
            continue;
        }
        let Some(mut material) = materials.get(&handle.0).cloned() else {
            continue;
        };
        material.base.emissive = LinearRgba::rgb(LAMP_GLOW, LAMP_GLOW, LAMP_GLOW);
        let own = std::mem::replace(&mut handle.0, materials.add(material));
        commands.entity(entity).insert(Glowing(own));
    }
}

/// Gives every glowing model its own material back, with what the glowing copy received meanwhile.
// Bevy supplies the model query with its filter as one parameter.
#[allow(clippy::type_complexity)]
pub(super) fn restore(
    mut commands: Commands,
    mut materials: ResMut<Assets<SceneMaterial>>,
    mut models: Query<
        (Entity, &mut MeshMaterial3d<SceneMaterial>, Option<&Glowing>),
        With<GlowChecked>,
    >,
) {
    for (entity, mut handle, glowing) in &mut models {
        commands.entity(entity).remove::<(GlowChecked, Glowing)>();
        let Some(Glowing(own)) = glowing else {
            continue;
        };
        let emissive = materials.get(own).map(|original| original.base.emissive);
        if let (Some(mut current), Some(emissive)) = (materials.get(&handle.0).cloned(), emissive) {
            current.base.emissive = emissive;
            if let Some(mut original) = materials.get_mut(own) {
                *original = current;
            }
        }
        handle.0 = own.clone();
    }
}
