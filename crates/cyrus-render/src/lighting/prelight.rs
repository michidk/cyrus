//! Retained static RGB from preservation's model-lighting arithmetic.

use std::sync::OnceLock;

type Vector = [f32; 3];

pub(super) struct Light {
    pub position: Vector,
    pub radius: f32,
    pub rgb: Vector,
}

pub(super) struct Instance {
    pub position: Vector,
    /// Row-major authored linear transform, without scale normalization.
    pub matrix: [Vector; 3],
    /// Original model bound in engine units, without instance scaling.
    pub radius: f32,
}

pub(super) struct Lighting {
    ambient: Vector,
    directional: Vector,
    direction: Vector,
    lights: Vec<Light>,
}

fn dot(a: Vector, b: Vector) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn transpose_transform(matrix: [Vector; 3], value: Vector) -> Vector {
    std::array::from_fn(|axis| dot([matrix[0][axis], matrix[1][axis], matrix[2][axis]], value))
}

pub(crate) fn distance(squared: f32) -> (f32, f32) {
    static TABLES: OnceLock<[[u32; 256]; 2]> = OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        std::array::from_fn(|kind| {
            std::array::from_fn(|index| {
                let sample = f32::from_bits(
                    ((127 + (index >> 7)) as u32) << 23 | ((index as u32 & 127) << 16),
                );
                let root = f64::from(sample).sqrt();
                let value = if kind == 0 { root } else { 1.0 / root };
                (value as f32).to_bits() & (127 << 16)
            })
        })
    });
    let bits = squared.to_bits();
    let exponent = ((bits >> 23) & 255) as i32 - 127;
    let index = (((bits >> 16) & 127) | ((exponent & 1) as u32) << 7) as usize;
    let root = f32::from_bits(tables[0][index] | ((exponent.div_euclid(2) + 127) as u32) << 23);
    let fraction = tables[1][index];
    let bias = if fraction == 0 { 254 } else { 253 };
    let inverse = f32::from_bits(fraction | (((bias - exponent) / 2) as u32) << 23);
    (root, inverse)
}

impl Lighting {
    pub(super) fn new(
        instance: &Instance,
        ambient: Vector,
        directional: Vector,
        direction: Vector,
        lights: &[Light],
    ) -> Self {
        Self {
            ambient,
            directional,
            direction: transpose_transform(instance.matrix, direction),
            lights: lights
                .iter()
                .filter_map(|light| {
                    let relative =
                        std::array::from_fn(|i| light.position[i] - instance.position[i]);
                    (dot(relative, relative) - instance.radius * instance.radius
                        <= light.radius * light.radius)
                        .then(|| Light {
                            position: transpose_transform(instance.matrix, relative),
                            radius: light.radius,
                            rgb: light.rgb,
                        })
                })
                .collect(),
        }
    }

    /// Engine-space vertex and supplied normal; palette tint precedes integer rounding.
    pub(super) fn color(
        &self,
        position: Vector,
        normal: Vector,
        brightness: u8,
        solid_rgb: Option<[u8; 3]>,
    ) -> [u8; 3] {
        let weight = (-dot(self.direction, normal)).max(0.0);
        let mut rgb: Vector = std::array::from_fn(|i| {
            (self.ambient[i] + f32::from(brightness)) + weight * self.directional[i]
        });
        for light in &self.lights {
            let delta = std::array::from_fn(|i| position[i] - light.position[i]);
            let squared = dot(delta, delta);
            if squared == 0.0 {
                continue;
            }
            let (distance, inverse) = distance(squared);
            let normal_dot = dot(delta.map(|v| v * inverse), normal);
            if normal_dot >= 0.0 {
                continue;
            }
            let radius = light.radius;
            let numerator = ((2.0 * radius + radius * radius) + radius) / 256.0;
            let denominator = (2.0 * distance + squared) + distance;
            let attenuation = numerator / denominator;
            if attenuation < 0.0 {
                continue;
            }
            let weight = -normal_dot * attenuation;
            for (channel, strength) in rgb.iter_mut().zip(light.rgb) {
                *channel += strength * weight;
            }
        }
        std::array::from_fn(|i| {
            let channel = rgb[i].clamp(0.0, 255.0);
            let channel = solid_rgb.map_or(channel, |tint| channel * f32::from(tint[i]) / 256.0);
            channel.round_ties_even() as u8
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> Instance {
        Instance {
            position: [0.0; 3],
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            radius: 0.0,
        }
    }

    #[test]
    fn retained_channels_round_ties_even_after_palette_tint() {
        for (ambient, tint, expected) in [
            ([0.5, 1.5, 2.5], None, [0, 2, 2]),
            ([3.5, 4.5, 5.5], None, [4, 4, 6]),
            ([0.49, 1.49, 2.49], None, [0, 1, 2]),
            ([0.51, 1.51, 2.51], None, [1, 2, 3]),
            ([255.0; 3], Some([200, 100, 50]), [199, 100, 50]),
        ] {
            let lighting = Lighting::new(&instance(), ambient, [0.0; 3], [0.0; 3], &[]);
            assert_eq!(
                lighting.color([0.0; 3], [0.0, -1.0, 0.0], 0, tint),
                expected
            );
        }
    }

    #[test]
    fn selection_uses_squared_bound_instead_of_sphere_overlap() {
        let mut instance = instance();
        instance.radius = 4.0;
        let light = |x| Light {
            position: [x, 0.0, 0.0],
            radius: 3.0,
            rgb: [255.0; 3],
        };
        let selected = Lighting::new(
            &instance,
            [0.0; 3],
            [0.0; 3],
            [0.0; 3],
            &[light(5.0), light(6.0)],
        );
        assert_eq!(selected.lights.len(), 1);
        assert_eq!(selected.lights[0].position, [5.0, 0.0, 0.0]);
    }

    #[test]
    fn selection_transposes_authored_matrix_without_normalizing_scale() {
        let instance = Instance {
            position: [10.0, 20.0, 30.0],
            matrix: [[2.0, 0.25, 0.5], [0.0, 3.0, 0.0], [0.125, 0.0, 4.0]],
            radius: 100.0,
        };
        let light = Light {
            position: [11.0, 22.0, 33.0],
            radius: 0.0,
            rgb: [255.0; 3],
        };
        let selected = Lighting::new(&instance, [0.0; 3], [0.0; 3], [1.0, 2.0, 3.0], &[light]);
        assert_eq!(selected.lights[0].position, [2.375, 6.25, 12.5]);
        assert_eq!(selected.direction, [2.375, 6.25, 12.5]);
    }
}
