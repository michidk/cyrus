//! Private terrain mesh preparation after camera projection and wave updates.
mod prepare;

use bevy::mesh::{MeshVertexAttribute, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::VertexFormat;
pub(super) use prepare::{Preparation, Vertex as ClipVertex};

pub(super) const PROJECTED: MeshVertexAttribute =
    MeshVertexAttribute::new("Terrain_Projected", 83001, VertexFormat::Float32x4);
pub(super) const RED: MeshVertexAttribute =
    MeshVertexAttribute::new("Terrain_Red", 83002, VertexFormat::Sint32x4);
pub(super) const GREEN: MeshVertexAttribute =
    MeshVertexAttribute::new("Terrain_Green", 83003, VertexFormat::Sint32x4);
pub(super) const BLUE: MeshVertexAttribute =
    MeshVertexAttribute::new("Terrain_Blue", 83004, VertexFormat::Sint32x4);

/// Packed low/high words of nine signed texture-plane coefficients.
pub(super) const TEXTURE: [MeshVertexAttribute; 5] = [
    MeshVertexAttribute::new("Terrain_Texture0", 83005, VertexFormat::Uint32x4),
    MeshVertexAttribute::new("Terrain_Texture1", 83006, VertexFormat::Uint32x4),
    MeshVertexAttribute::new("Terrain_Texture2", 83007, VertexFormat::Uint32x4),
    MeshVertexAttribute::new("Terrain_Texture3", 83008, VertexFormat::Uint32x4),
    MeshVertexAttribute::new("Terrain_Texture4", 83009, VertexFormat::Uint32x4),
];

pub(super) struct Source {
    pub prepared: Mesh,
    uv: Vec<[f32; 2]>,
    triangles: Vec<[usize; 3]>,
}

impl Source {
    pub fn new(mesh: &Mesh, vertex_count: usize) -> Result<Self, &'static str> {
        let uv = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uv)) => uv.clone(),
            _ => return Err("missing two-component texture coordinates"),
        };
        if uv.len() != vertex_count || uv.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid terrain texture coordinates");
        }
        // Export UVs identify tile corners in vertically flipped images.
        // Keep original coordinates through clipping and fixed-point texture setup.
        // https://github.com/michidk/redguard-preservation/blob/aa70734/docs/engine/terrain-lighting.md#texture-coordinates-and-setup
        let uv = uv
            .into_iter()
            .map(|[u, v]| [1.0 + u * 253.0, 254.0 - v * 253.0])
            .collect();
        let indices: Vec<_> = mesh
            .indices()
            .map(|indices| indices.iter().collect())
            .unwrap_or_else(|| (0..vertex_count).collect());
        if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i >= vertex_count) {
            return Err("invalid terrain triangle indices");
        }
        let triangles = indices.as_chunks::<3>().0.to_vec();
        Ok(Self {
            prepared: mesh.clone(),
            uv,
            triangles,
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct View {
    pub camera_from_local: Mat4,
    pub scale: Vec2,
    pub viewport: [f32; 2],
    pub clipping: [f32; 2],
}

/// Camera-centered draw/admission window from preservation's water terrain rules.
/// Coordinates are raw WLD columns/rows; positive engine Z reverses rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Window {
    center: [i32; 2],
    side: usize,
    radius: i32,
}

impl Window {
    pub fn new(camera_local: Vec3, side: usize) -> Self {
        let engine_x = -camera_local.x * 20.0;
        let engine_z = camera_local.z * 20.0 + side as f32 * 256.0;
        Self {
            center: [
                (engine_x / 256.0).round_ties_even() as i32,
                side as i32 - (engine_z / 256.0).round_ties_even() as i32,
            ],
            side,
            radius: 16,
        }
    }

    /// Modern fog admits the whole far frustum, including its horizontal corners.
    pub fn with_far(mut self, far: f32, scale: Vec2) -> Self {
        let extent = far * (1.0 + (Vec2::ONE / scale).length_squared()).sqrt();
        self.radius = ((extent / 256.0).ceil() as i32).saturating_add(1).max(16);
        self
    }

    pub fn contains(self, vertex: usize) -> bool {
        let x = (vertex % self.side) as i32 - self.center[0];
        let row = (vertex / self.side) as i32 - self.center[1];
        (-self.radius..self.radius).contains(&x) && (-self.radius + 1..=self.radius).contains(&row)
    }
}

pub(super) struct Grid<'a> {
    pub window: Window,
    pub local: &'a [[f32; 3]],
    pub camera: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    pub rgb: &'a [[f32; 3]],
    pub water: Option<&'a [bool]>,
}

/// Original texture admission, independent of projected area and raster visibility.
#[derive(Component, Default)]
pub(crate) struct TextureAdmission(pub bool);

pub(super) struct Prepared {
    pub visible: bool,
    pub texture_admitted: bool,
}

pub(super) fn update(
    source: &mut Source,
    mapping: &[usize],
    grid: Grid<'_>,
    view: View,
    preparation: &mut Preparation,
) -> Prepared {
    let camera = view
        .camera_from_local
        .inverse()
        .transform_point3(Vec3::ZERO)
        * 20.0;
    let mut texture_admitted = false;
    let mesh = &mut source.prepared;
    // Reuse the previous frame's allocations when returning attributes to the mesh.
    let take3 = |mesh: &mut Mesh, attribute| match mesh.remove_attribute(attribute) {
        Some(VertexAttributeValues::Float32x3(mut values)) => {
            values.clear();
            values
        }
        _ => Vec::new(),
    };
    let take2 = |mesh: &mut Mesh, attribute| match mesh.remove_attribute(attribute) {
        Some(VertexAttributeValues::Float32x2(mut values)) => {
            values.clear();
            values
        }
        _ => Vec::new(),
    };
    let take4i = |mesh: &mut Mesh, attribute| match mesh.remove_attribute(attribute) {
        Some(VertexAttributeValues::Sint32x4(mut values)) => {
            values.clear();
            values
        }
        _ => Vec::new(),
    };
    let mut positions = take3(mesh, Mesh::ATTRIBUTE_POSITION);
    let mut normals = take3(mesh, Mesh::ATTRIBUTE_NORMAL);
    let mut uv = take2(mesh, Mesh::ATTRIBUTE_UV_0);
    let mut water = take2(mesh, Mesh::ATTRIBUTE_UV_1);
    let mut projected = match mesh.remove_attribute(PROJECTED) {
        Some(VertexAttributeValues::Float32x4(mut values)) => {
            values.clear();
            values
        }
        _ => Vec::new(),
    };
    let mut red = take4i(mesh, RED);
    let mut green = take4i(mesh, GREEN);
    let mut blue = take4i(mesh, BLUE);
    let mut texture = TEXTURE.map(|attribute| match mesh.remove_attribute(attribute) {
        Some(VertexAttributeValues::Uint32x4(mut values)) => {
            values.clear();
            values
        }
        _ => Vec::new(),
    });
    for &triangle in &source.triangles {
        if !triangle
            .iter()
            .all(|&index| grid.window.contains(mapping[index]))
        {
            continue;
        }
        let vertices = triangle.map(|index| {
            let vertex = mapping[index];
            prepare::Vertex {
                camera: grid.camera[vertex],
                rgb: grid.rgb[vertex],
                uv: source.uv[index],
                local: grid.local[vertex],
                normal: grid.normals[vertex],
                water: u8::from(grid.water.is_some_and(|w| w[vertex])) as f32,
            }
        });
        if !texture_admitted {
            let indices = triangle.map(|index| mapping[index]);
            let anchor = *indices.iter().min().unwrap();
            let relative = Vec3::new(
                -((anchor % grid.window.side) as f32) * 256.0,
                grid.local[anchor][1] * 20.0,
                -((anchor / grid.window.side) as f32) * 256.0,
            ) - camera;
            let normal = crate::lighting::grid_face_normal(grid.local, grid.window.side, indices);
            let facing = (relative.x * normal.x + relative.y * normal.y) + relative.z * normal.z;
            texture_admitted = facing < 0.0 && preparation.admits(vertices, view.clipping);
        }
        preparation.triangles(vertices, view.clipping, view.viewport, |triangle| {
            let [r, g, b] = triangle.planes;
            let mut words = [0; 20];
            for (value, pair) in triangle
                .texture
                .into_iter()
                .flatten()
                .zip(words.as_chunks_mut::<2>().0)
            {
                pair.copy_from_slice(&[value as u32, (value >> 32) as u32]);
            }
            for vertex in triangle.vertices {
                positions.push(vertex.vertex.local);
                normals.push(vertex.vertex.normal);
                let [s, t] = vertex.vertex.uv;
                uv.push([s / 256.0, 1.0 - t / 256.0]);
                for (values, chunk) in texture.iter_mut().zip(words.as_chunks::<4>().0) {
                    values.push(*chunk);
                }
                water.push([vertex.vertex.water, 0.0]);
                projected.push([
                    vertex.screen[0],
                    vertex.screen[1],
                    vertex.vertex.camera[2] / 20.0,
                    0.0,
                ]);
                red.push([r[0], r[1], r[2], triangle.anchor[0]]);
                green.push([g[0], g[1], g[2], triangle.anchor[1]]);
                blue.push([b[0], b[1], b[2], 0]);
            }
        });
    }
    let visible = !positions.is_empty();
    mesh.remove_indices();
    mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
    mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, water);
    mesh.insert_attribute(PROJECTED, projected);
    mesh.insert_attribute(RED, red);
    mesh.insert_attribute(GREEN, green);
    mesh.insert_attribute(BLUE, blue);
    for (attribute, values) in TEXTURE.into_iter().zip(texture) {
        mesh.insert_attribute(attribute, values);
    }
    Prepared {
        visible,
        texture_admitted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_admission_uses_facing_window_and_clipping_before_raster_area() {
        use bevy::{
            asset::RenderAssetUsages,
            mesh::{Indices, PrimitiveTopology},
        };
        let local = [
            [0., 0., 0.],
            [-12.8, 0., 0.],
            [0., 0., -12.8],
            [-12.8, 0., -12.8],
        ];
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, local.to_vec())
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
        )
        .with_inserted_indices(Indices::U32(vec![0, 3, 1, 3, 0, 2]));
        let mut source = Source::new(&mesh, 4).unwrap();
        let mut preparation = Preparation::default();
        let mut check = |height: f32, far: f32, center: [i32; 2], viewport: [f32; 2]| {
            let transform = Transform::from_xyz(-6.4, height, -6.4)
                .looking_at(Vec3::new(-6.4, 0., -6.4), Vec3::Z);
            let matrix = transform.to_matrix().inverse();
            let camera = local.map(|p| {
                let p = matrix.transform_point3(Vec3::from(p)) * 20.;
                [p.x, -p.y, -p.z]
            });
            update(
                &mut source,
                &[0, 1, 2, 3],
                Grid {
                    window: Window {
                        center,
                        side: 2,
                        radius: 16,
                    },
                    local: &local,
                    camera: &camera,
                    normals: &[[0., 1., 0.]; 4],
                    rgb: &[[255.; 3]; 4],
                    water: None,
                },
                View {
                    camera_from_local: matrix,
                    scale: Vec2::new(1., -1.),
                    viewport,
                    clipping: [1., far],
                },
                &mut preparation,
            )
        };
        let ordinary = check(10., 1000., [0, 0], [640., 480.]);
        assert!(ordinary.visible && ordinary.texture_admitted);
        let subpixel = check(10., 1000., [0, 0], [0.0001, 0.0001]);
        assert!(!subpixel.visible && subpixel.texture_admitted);
        assert!(!check(-10., 1000., [0, 0], [640., 480.]).texture_admitted);
        assert!(!check(10., 50., [0, 0], [640., 480.]).texture_admitted);
        assert!(!check(10., 1000., [100, 100], [640., 480.]).texture_admitted);
        assert!(check(10., 1000., [0, 0], [640., 480.]).visible);
        assert_eq!(
            source.prepared.attribute(Mesh::ATTRIBUTE_UV_0),
            Some(&VertexAttributeValues::Float32x2(vec![
                [0.00390625, 0.0078125],
                [0.9921875, 0.99609375],
                [0.9921875, 0.0078125],
                [0.9921875, 0.99609375],
                [0.00390625, 0.0078125],
                [0.00390625, 0.99609375],
            ]))
        );
    }

    #[test]
    fn modern_window_reaches_distant_water_and_restores_original_admission() {
        let original = Window::new(Vec3::new(-32.0 * 12.8, 0.0, -32.0 * 12.8), 64);
        let distant = 32 * 64 + 55;
        assert!(!original.contains(distant));
        assert!(!original.with_far(3000.0, Vec2::ONE).contains(distant));
        assert!(original.with_far(7600.0, Vec2::ONE).contains(distant));
        assert!(!original.contains(distant));
    }

    #[test]
    fn camera_window_rounds_ties_even_and_reverses_source_rows() {
        let camera = |x: f32, z: f32| Vec3::new(-x / 20.0, 0.0, (z - 16384.0) / 20.0);
        for (x, expected) in [
            (127.99, 0),
            (128.0, 0),
            (128.01, 1),
            (384.0, 2),
            (-128.0, 0),
            (-384.0, -2),
        ] {
            assert_eq!(Window::new(camera(x, 8192.0), 64).center, [expected, 32]);
        }
        let window = Window::new(camera(8192.0, 8192.0), 64);
        let admitted: Vec<_> = (0..64 * 64).filter(|&v| window.contains(v)).collect();
        assert_eq!(admitted.len(), 1024);
        assert_eq!(admitted[0], 17 * 64 + 16);
        assert_eq!(*admitted.last().unwrap(), 48 * 64 + 47);
        let cells = (0..63)
            .flat_map(|row| (0..63).map(move |x| row * 64 + x))
            .filter(|&a| {
                [a, a + 1, a + 64, a + 65]
                    .into_iter()
                    .all(|v| window.contains(v))
            })
            .count();
        assert_eq!(cells, 961);
    }
}
