//! Authored model lighting inputs and quantized terrain normals from preservation.

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

/// Copy authored per-face inputs into a private mesh's unused second UV channel.
pub(crate) fn model_mesh(mesh: &Mesh, extras: &str) -> anyhow::Result<Mesh> {
    #[derive(serde::Deserialize)]
    struct Inputs {
        version: u32,
        vertices: Vec<[u8; 2]>,
    }
    #[derive(serde::Deserialize)]
    struct Extras {
        rgpre_lighting: Inputs,
    }
    let inputs = serde_json::from_str::<Extras>(extras)?.rgpre_lighting;
    anyhow::ensure!(
        inputs.version == 1,
        "unsupported model lighting version {}",
        inputs.version
    );
    anyhow::ensure!(
        inputs.vertices.len() == mesh.count_vertices(),
        "model lighting vertex count mismatch"
    );
    anyhow::ensure!(
        inputs.vertices.iter().all(|v| v[1] <= 1),
        "invalid solid-color model lighting flag"
    );
    let mut private = mesh.clone();
    private.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        inputs
            .vertices
            .into_iter()
            .map(|[brightness, solid]| {
                [
                    f32::from(brightness),
                    if solid == 1 { 255.0 / 256.0 } else { 1.0 },
                ]
            })
            .collect::<Vec<_>>(),
    );
    Ok(private)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_inputs_preserve_face_brightness_and_reject_incompatible_exports() {
        use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        let prepared = model_mesh(
            &mesh,
            r#"{"rgpre_lighting":{"version":1,"vertices":[[0,0],[64,0],[255,1]]}}"#,
        )
        .unwrap();
        let Some(VertexAttributeValues::Float32x2(values)) =
            prepared.attribute(Mesh::ATTRIBUTE_UV_1)
        else {
            panic!("missing model inputs")
        };
        assert_eq!(values, &vec![[0.0, 1.0], [64.0, 1.0], [255.0, 0.99609375]]);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_none());
        for invalid in [
            r#"{}"#,
            r#"{"rgpre_lighting":{"version":2,"vertices":[[0,0],[0,0],[0,0]]}}"#,
            r#"{"rgpre_lighting":{"version":1,"vertices":[[0,0]]}}"#,
            r#"{"rgpre_lighting":{"version":1,"vertices":[[0,0],[0,0],[0,2]]}}"#,
        ] {
            assert!(model_mesh(&mesh, invalid).is_err());
        }
    }

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
