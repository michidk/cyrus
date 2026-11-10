//! Palm trees and plants that sway in the wind.
//!
//! **Modern only**, a tuning choice. The original draws them still. A placed static model whose
//! installed model name contains `PALM`, `TREE` or `PLANT` (such as `PALMTR01`, `PLANT002`,
//! `NCTREE02`) tilts about the bottom of its bounds, so its shadow and depth follow it. Only the
//! drawn transform moves; collision reads the installed placement.
use super::effects::ModernEffects;
use bevy::{camera::primitives::Aabb, prelude::*};

/// Installed model-name parts that mark a plant.
const PLANT_NAMES: [&str; 3] = ["PALM", "TREE", "PLANT"];
/// Peak tilt of a tree and of a smaller plant, in radians.
const TREE_SWAY: f32 = 0.02;
const PLANT_SWAY: f32 = 0.04;
/// Seconds per sway.
const SWAY_PERIOD: f32 = 4.5;

/// A placed static model, named `S<index>_<model>` by the scene export, that is a plant.
fn plant_sway(name: &str) -> Option<f32> {
    let (index, model) = name.split_once('_')?;
    if !index.starts_with('S') || !index[1..].bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let model = model.to_ascii_uppercase();
    PLANT_NAMES
        .iter()
        .any(|part| model.contains(part))
        .then(|| {
            if model.contains("PLANT") {
                PLANT_SWAY
            } else {
                TREE_SWAY
            }
        })
}

/// A plant's placement at rest and how it sways.
#[derive(Component)]
pub(super) struct Sway {
    rest: Transform,
    /// Point it tilts about, in its own space.
    pivot: Vec3,
    amplitude: f32,
    phase: f32,
}

/// A plant whose bounds are not known yet.
#[derive(Component)]
pub(super) struct Plant(f32);

/// Marks plants as their scene nodes appear.
pub(super) fn find_plants(mut commands: Commands, named: Query<(Entity, &Name), Added<Name>>) {
    for (entity, name) in &named {
        if let Some(amplitude) = plant_sway(name.as_str()) {
            commands.entity(entity).insert(Plant(amplitude));
        }
    }
}

/// The plants swaying, as `sway` moves them and `settle` puts them back.
pub(super) type Swaying<'w, 's> = Query<'w, 's, (Entity, &'static Sway, &'static mut Transform)>;

/// What `sway` reads besides the swaying plants: plants still to start, their children's bounds,
/// the swaying plants' places and the view camera.
// Bevy query filters keep the plant, bounds and camera queries disjoint.
#[allow(clippy::type_complexity)]
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Plants<'w, 's> {
    new: Query<
        'w,
        's,
        (
            Entity,
            &'static Plant,
            &'static Transform,
            &'static Children,
        ),
        Without<Sway>,
    >,
    bounds: Query<'w, 's, (&'static Aabb, &'static Transform), Without<Sway>>,
    places: Query<'w, 's, &'static GlobalTransform, With<Sway>>,
    cameras:
        Query<'w, 's, &'static GlobalTransform, (With<crate::camera::ViewCamera>, Without<Sway>)>,
}

/// Sways the plants while the effect is on, and puts them back at rest once it is off.
pub(super) fn sway(
    mut commands: Commands,
    effects: Res<ModernEffects>,
    time: Res<Time>,
    plants: Plants,
    mut swaying: Swaying,
) {
    if !effects.foliage {
        settle(&mut commands, &mut swaying);
        return;
    }
    let Plants {
        new,
        bounds,
        places,
        cameras,
    } = plants;
    for (entity, plant, transform, children) in &new {
        let Some(bottom) = children
            .iter()
            .filter_map(|child| bounds.get(child).ok())
            .map(|(aabb, local)| {
                let center = Vec3::from(aabb.center);
                local.transform_point(center - Vec3::Y * aabb.half_extents.y)
            })
            .reduce(|a, b| if a.y < b.y { a } else { b })
        else {
            continue;
        };
        let place = transform.translation;
        commands.entity(entity).insert(Sway {
            rest: *transform,
            pivot: bottom,
            amplitude: plant.0,
            phase: (place.x * 0.37 + place.z * 0.23).rem_euclid(std::f32::consts::TAU),
        });
    }
    let seconds = time.elapsed_secs_wrapped();
    let eye = cameras.iter().next().map(GlobalTransform::translation);
    for (entity, sway, mut transform) in &mut swaying {
        // Far plants hold their pose: their sway is not visible, and still plants cost nothing.
        let near = eye.zip(places.get(entity).ok()).is_none_or(|(eye, place)| {
            eye.distance_squared(place.translation()) <= SWAY_RANGE * SWAY_RANGE
        });
        if near {
            transform.set_if_neq(swayed(sway, seconds));
        }
    }
}

/// Puts every swaying plant back at rest.
pub(super) fn settle(commands: &mut Commands, swaying: &mut Swaying) {
    for (entity, sway, mut transform) in swaying {
        transform.set_if_neq(sway.rest);
        commands.entity(entity).remove::<Sway>();
    }
}

/// Distance from the camera, in world units, within which plants sway.
const SWAY_RANGE: f32 = 200.0;

/// The wind's heading in world space; one direction for every world.
const WIND: Vec3 = Vec3::new(0.94, 0.0, 0.34);

fn swayed(sway: &Sway, seconds: f32) -> Transform {
    let angle = seconds / SWAY_PERIOD * std::f32::consts::TAU + sway.phase;
    // A slow lean with a quicker flutter on top.
    let tilt = sway.amplitude * (0.75 * angle.sin() + 0.25 * (angle * 2.7 + 1.3).sin());
    let axis = sway.rest.rotation.inverse() * Vec3::Y.cross(WIND);
    let rotation = Quat::from_axis_angle(axis.normalize_or(Vec3::X), tilt);
    let local = Transform::from_translation(sway.pivot)
        * Transform::from_rotation(rotation)
        * Transform::from_translation(-sway.pivot);
    sway.rest * local
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plants_are_read_from_the_installed_model_names() {
        assert_eq!(plant_sway("S001_PALMTR01"), Some(TREE_SWAY));
        assert_eq!(plant_sway("S012_PLANT003"), Some(PLANT_SWAY));
        assert_eq!(plant_sway("S040_NCTREE02"), Some(TREE_SWAY));
        assert_eq!(plant_sway("S007_BARREL"), None);
        assert_eq!(plant_sway("B_015_FIRESND"), None);
        assert_eq!(plant_sway("F350_PALM"), None);
    }

    #[test]
    fn the_pivot_stays_in_place() {
        let sway = Sway {
            rest: Transform::from_xyz(10.0, 2.0, -4.0).with_rotation(Quat::from_rotation_y(0.7)),
            pivot: Vec3::new(0.5, -3.0, 0.2),
            amplitude: TREE_SWAY,
            phase: 1.0,
        };
        let moved = swayed(&sway, 1.3);
        assert!(
            moved
                .transform_point(sway.pivot)
                .abs_diff_eq(sway.rest.transform_point(sway.pivot), 1e-4)
        );
        assert!(!moved.rotation.abs_diff_eq(sway.rest.rotation, 1e-6));
    }
}
