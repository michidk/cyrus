//! Palm trees and plants sway in the wind: placed static models whose installed name contains
//! `PALM`, `TREE` or `PLANT` (such as `PALMTR01`, `PLANT002`, `NCTREE02`) tilt about their base.
//! The original draws them still; collision keeps the installed placement.
use cyrus_mod_sdk::{Cx, Mod, Persist, Quat, Vec3};

/// Peak tilt of a tree and of a smaller plant, in radians.
const TREE_SWAY: f32 = 0.02;
const PLANT_SWAY: f32 = 0.04;
/// Seconds per sway.
const SWAY_PERIOD: f32 = 4.5;
/// Distance from the camera, in scene units, within which plants sway; farther sway is not visible.
const SWAY_RANGE: f32 = 200.0;
/// The tilt axis: horizontal, across the wind heading (0.94, 0, 0.34). One wind for every world.
const AXIS: Vec3 = Vec3 {
    x: 0.34,
    y: 0.0,
    z: -0.94,
};

/// The peak tilt of a placed model by its installed name, or `None` for one that is not a plant.
fn amplitude(model: &str) -> Option<f32> {
    if model.contains("PLANT") {
        Some(PLANT_SWAY)
    } else if model.contains("PALM") || model.contains("TREE") {
        Some(TREE_SWAY)
    } else {
        None
    }
}

/// The tilt at `seconds` of a plant at `base`: a slow lean with a quicker flutter on top.
fn tilt(amplitude: f32, base: Vec3, seconds: f32) -> f32 {
    let phase = (base.x * 0.37 + base.z * 0.23).rem_euclid(std::f32::consts::TAU);
    let angle = seconds / SWAY_PERIOD * std::f32::consts::TAU + phase;
    amplitude * (0.75 * angle.sin() + 0.25 * (angle * 2.7 + 1.3).sin())
}

/// The plants of one scenery revision: index and peak tilt, so names are read once per world.
#[derive(Default)]
struct Plants {
    revision: f32,
    plants: Vec<(u32, f32)>,
}
impl Persist for Plants {
    fn pack(&self) -> Vec<f32> {
        let mut values = vec![self.revision];
        for &(index, amplitude) in &self.plants {
            values.extend([index as f32, amplitude]);
        }
        values
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        let (&revision, rest) = values.split_first()?;
        rest.len().is_multiple_of(2).then(|| Plants {
            revision,
            plants: rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[index, amplitude]| (index as u32, amplitude))
                .collect(),
        })
    }
}

struct Sway;
impl Mod for Sway {
    const ID: &'static str = "sway";
    fn frame(cx: &Cx) {
        let revision = cx.scenery.revision() as f32;
        let plants = match cx.memory.load::<Plants>("plants") {
            Some(plants) if plants.revision == revision => plants,
            _ => {
                let plants = Plants {
                    revision,
                    plants: (0..cx.scenery.count())
                        .filter_map(|i| amplitude(&cx.scenery.model(i)).map(|a| (i, a)))
                        .collect(),
                };
                cx.memory.save("plants", Some(&plants));
                plants
            }
        };
        let strength = cx.settings.number("strength").clamp(0.0, 3.0);
        let Some(eye) = cx.camera.pose().map(|pose| pose.position) else {
            return;
        };
        let seconds = cx.environment.seconds();
        for &(index, peak) in &plants.plants {
            let Some(base) = cx.scenery.base(index) else {
                continue;
            };
            if (base - eye).length() > SWAY_RANGE {
                continue;
            }
            let angle = tilt(peak * strength, base, seconds);
            cx.scenery.tilt(index, Quat::from_axis_angle(AXIS, angle));
        }
    }
}
cyrus_mod_sdk::export_mod!(Sway);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plants_are_read_from_the_installed_model_names() {
        assert_eq!(amplitude("PALMTR01"), Some(TREE_SWAY));
        assert_eq!(amplitude("PLANT003"), Some(PLANT_SWAY));
        assert_eq!(amplitude("NCTREE02"), Some(TREE_SWAY));
        assert_eq!(amplitude("BARREL"), None);
    }

    #[test]
    fn the_tilt_stays_within_the_peak_and_the_plant_list_round_trips() {
        for step in 0..100 {
            assert!(
                tilt(TREE_SWAY, Vec3::new(3.0, 0.0, 7.0), step as f32 * 0.1).abs() <= TREE_SWAY
            );
        }
        let plants = Plants {
            revision: 4.0,
            plants: vec![(2, TREE_SWAY), (9, PLANT_SWAY)],
        };
        let back = Plants::unpack(&plants.pack()).unwrap();
        assert_eq!((back.revision, back.plants), (4.0, plants.plants));
    }
}
