//! Terrain RGB and quantized normals from preservation's terrain-lighting contract.

use bevy::prelude::*;
pub use cyrus_assets::environment::TerrainLighting;
use std::sync::OnceLock;

fn normalize(normal: Vec3) -> Vec3 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        std::array::from_fn(|index| {
            let sample =
                f32::from_bits(((127 + (index >> 7)) as u32) << 23 | ((index as u32 & 127) << 16));
            ((1.0 / f64::from(sample).sqrt()) as f32).to_bits() & (127 << 16)
        })
    });
    let squared = normal.x * normal.x + (normal.y * normal.y + normal.z * normal.z);
    if squared == 0.0 {
        return Vec3::ZERO;
    }
    let bits = squared.to_bits();
    let exponent = ((bits >> 23) & 255) as i32 - 127;
    let index = ((bits >> 16) & 127) | (((exponent & 1) as u32) << 7);
    let fraction = table[index as usize];
    let bias = if fraction == 0 { 254 } else { 253 };
    normal * f32::from_bits(fraction | (((bias - exponent) / 2) as u32) << 23)
}

/// Equal-face-weight shading normals. Positions are in exported units.
pub(crate) fn grid_normals(grid: &[[f32; 3]], side: usize) -> Vec<[f32; 3]> {
    let mut normals = vec![Vec3::ZERO; grid.len()];
    let position = |vertex: usize| {
        Vec3::new(
            -((vertex % side) as f32) * 256.0,
            grid[vertex][1] * 20.0,
            -((vertex / side) as f32) * 256.0,
        )
    };
    for row in 0..side - 1 {
        for x in 0..side - 1 {
            let tl = x + row * side;
            let tr = tl + 1;
            let bl = tl + side;
            let br = bl + 1;
            for [a, b, c] in [[tl, br, tr], [br, tl, bl]] {
                let face = normalize((position(b) - position(a)).cross(position(c) - position(a)));
                for vertex in [a, b, c] {
                    normals[vertex] += face;
                }
            }
        }
    }
    normals
        .into_iter()
        .map(|normal| normalize(normal).to_array())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_day_flat_terrain_preserves_quantized_normal_magnitude() {
        let grid: Vec<_> = (0..9)
            .map(|i| [-(i % 3) as f32 * 12.8, 16.0, -(i / 3) as f32 * 12.8])
            .collect();
        let normals = grid_normals(&grid, 3);
        assert_eq!(normals[4], [0.0, 0.99609375, 0.0]);
        let light = TerrainLighting {
            side: 3,
            ambient: [64.0; 3],
            directional: [255.0; 3],
            direction: [-0.49998432, -0.49998432, 0.7071289],
        };
        assert!((light.color(normals[4])[0] * 255.0 - 190.99797).abs() < 0.0001);
        assert_eq!(
            light.color([0.0, -1.0, 0.0]),
            [64.0 / 255.0, 64.0 / 255.0, 64.0 / 255.0, 1.0]
        );
        assert_eq!(light.color([0.0, 3.0, 0.0]), [1.0; 4]);
    }
    #[test]
    fn terrain_shading_weights_faces_equally_before_quantized_normalization() {
        let grid: Vec<_> = (0..9)
            .map(|i| {
                [
                    -(i % 3) as f32 * 12.8,
                    if i == 1 { 2.0 } else { 0.0 },
                    -(i / 3) as f32 * 12.8,
                ]
            })
            .collect();
        let actual = grid_normals(&grid, 3)[4];
        for (actual, expected) in actual
            .into_iter()
            .zip([0.025480986, 0.9960823, -0.051267743])
        {
            assert!((actual - expected).abs() < 0.000001);
        }
    }
}
