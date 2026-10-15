//! Authored model lighting inputs and quantized terrain normals from preservation.

mod prelight;
pub(crate) use prelight::distance as quantized_distance;
pub(crate) mod runtime;

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
pub use cyrus_assets::environment::TerrainLighting;
use cyrus_assets::environment::{Environment, StaticLightingInstance};
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
    for row in 0..side - 1 {
        for x in 0..side - 1 {
            let tl = x + row * side;
            let tr = tl + 1;
            let bl = tl + side;
            let br = bl + 1;
            for [a, b, c] in [[tl, br, tr], [br, tl, bl]] {
                let face = grid_face_normal(grid, side, [a, b, c]);
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

/// Stored terrain face normal in export axes, with original normalization precision.
pub(crate) fn grid_face_normal(grid: &[[f32; 3]], side: usize, [a, b, c]: [usize; 3]) -> Vec3 {
    let position = |vertex: usize| {
        Vec3::new(
            -((vertex % side) as f32) * 256.0,
            grid[vertex][1] * 20.0,
            -((vertex / side) as f32) * 256.0,
        )
    };
    normalize((position(b) - position(a)).cross(position(c) - position(a)))
}

/// Copy authored per-face inputs into a private mesh's unused second UV channel.
pub(crate) fn model_mesh(
    mesh: &Mesh,
    extras: &str,
    bake: Option<(&Environment, &StaticLightingInstance)>,
) -> anyhow::Result<(Mesh, i32)> {
    #[derive(serde::Deserialize)]
    struct Inputs {
        version: u32,
        vertices: Vec<[u8; 2]>,
        radius_fixed: i32,
        solid_rgb: Option<[u8; 3]>,
    }
    #[derive(serde::Deserialize)]
    struct Extras {
        rgpre_lighting: Inputs,
    }
    let inputs = serde_json::from_str::<Extras>(extras)?.rgpre_lighting;
    anyhow::ensure!(
        inputs.version == 5,
        "unsupported model lighting version {}",
        inputs.version
    );
    anyhow::ensure!(
        inputs.vertices.len() == mesh.count_vertices(),
        "model lighting vertex count mismatch"
    );
    anyhow::ensure!(
        inputs
            .vertices
            .iter()
            .all(|v| v[1] == u8::from(inputs.solid_rgb.is_some())),
        "invalid solid-color model lighting flag"
    );
    let mut private = mesh.clone();
    private.remove_attribute(Mesh::ATTRIBUTE_COLOR);
    if let Some((environment, instance)) = bake {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            anyhow::bail!("model lighting requires float vertex positions");
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            anyhow::bail!("model lighting requires authored float normals");
        };
        anyhow::ensure!(
            normals.len() == positions.len(),
            "model lighting normal count mismatch"
        );
        anyhow::ensure!(
            positions
                .iter()
                .chain(normals)
                .flatten()
                .all(|v| v.is_finite()),
            "nonfinite model lighting vertex or normal"
        );
        let instance = prelight::Instance {
            position: instance.position.map(|v| v as f32),
            matrix: std::array::from_fn(|row| {
                std::array::from_fn(|col| instance.matrix[row * 3 + col] as f32 / 268_435_456.0)
            }),
            radius: inputs.radius_fixed as f32 / 256.0,
        };
        let lights: Vec<_> = environment
            .prelighting
            .lights
            .iter()
            .map(|light| prelight::Light {
                position: light.position.map(|v| v as f32),
                radius: f32::from(light.radius),
                rgb: light.rgb.map(f32::from),
            })
            .collect();
        let lighting = &environment.model_lighting;
        let [x, y, z] = lighting.direction;
        let lighting = prelight::Lighting::new(
            &instance,
            lighting.ambient,
            lighting.directional,
            [-x, -y, z],
            &lights,
        );
        let colors: Vec<_> = positions
            .iter()
            .zip(normals)
            .zip(&inputs.vertices)
            .map(|((&[x, y, z], &[nx, ny, nz]), &[brightness, _])| {
                let [r, g, b] = lighting.color(
                    [-x * 20.0, -y * 20.0, z * 20.0],
                    [-nx, -ny, nz],
                    brightness,
                    inputs.solid_rgb,
                );
                [
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    f32::from(u8::from(inputs.solid_rgb.is_some())),
                ]
            })
            .collect();
        private.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
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
    Ok((private, inputs.radius_fixed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_instances_bake_private_colors_from_authored_engine_coordinates() {
        use cyrus_assets::environment::{LocalLight, ModelLighting, Prelighting};
        let environment = Environment {
            texture_scale: [1, 1],
            sky: None,
            background: [0; 3],
            clipping: [7.0, 3800.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
            model_lighting: ModelLighting {
                ambient: [64.0, 100.0, 180.0],
                directional: [0.0; 3],
                direction: [0.0; 3],
            },
            object_lights: Vec::new(),
            prelighting: Prelighting {
                instances: Default::default(),
                lights: vec![LocalLight {
                    position: [1, 0, 0],
                    radius: 100,
                    rgb: [255; 3],
                }],
            },
        };
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[-1.0, 0.0, 0.0]; 3]);
        let extras = r#"{"rgpre_lighting":{"version":5,"radius_fixed":256,"solid_rgb":[200,100,50],"vertices":[[16,1],[16,1],[16,1]]}}"#;
        let mut instance = StaticLightingInstance {
            position: [0; 3],
            matrix: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
        };
        for (x, expected) in [(0, [199, 100, 50]), (1000, [62, 45, 38])] {
            instance.position[0] = x;
            let (baked, _) = model_mesh(&mesh, extras, Some((&environment, &instance))).unwrap();
            let Some(VertexAttributeValues::Float32x4(colors)) =
                baked.attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("missing retained colors");
            };
            let [r, g, b] = expected.map(|v| v as f32 / 255.0);
            assert_eq!(colors, &vec![[r, g, b, 1.0]; 3]);
        }
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_none());
        assert!(
            model_mesh(&mesh, extras, None)
                .unwrap()
                .0
                .attribute(Mesh::ATTRIBUTE_COLOR)
                .is_none()
        );
        mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
        assert!(model_mesh(&mesh, extras, Some((&environment, &instance))).is_err());
    }

    #[test]
    fn model_inputs_preserve_face_brightness_and_reject_incompatible_exports() {
        use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        let (prepared, _) = model_mesh(
            &mesh,
            r#"{"rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":[200,100,50],"vertices":[[0,1],[64,1],[255,1]]}}"#,
            None,
        )
        .unwrap();
        let Some(VertexAttributeValues::Float32x2(values)) =
            prepared.attribute(Mesh::ATTRIBUTE_UV_1)
        else {
            panic!("missing model inputs")
        };
        assert_eq!(
            values,
            &vec![[0.0, 0.99609375], [64.0, 0.99609375], [255.0, 0.99609375]]
        );
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_none());
        for invalid in [
            r#"{}"#,
            r#"{"rgpre_lighting":{"version":3,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[0,0],[0,0]]}}"#,
            r#"{"rgpre_lighting":{"version":4,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[0,0],[0,0]]}}"#,
            r#"{"rgpre_lighting":{"version":2,"vertices":[[0,0],[0,0],[0,0]]}}"#,
            r#"{"rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0]]}}"#,
            r#"{"rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[0,0],[0,2]]}}"#,
            r#"{"rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":[1,2,3],"vertices":[[0,0],[0,0],[0,0]]}}"#,
        ] {
            assert!(model_mesh(&mesh, invalid, None).is_err());
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
