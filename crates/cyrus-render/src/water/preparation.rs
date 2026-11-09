//! Terrain source binding shared by ordinary entry and off-live preparation.
use super::{CELL_SIZE, TerrainMesh, WaterPlayback};
use anyhow::{Context, Result, ensure};
use bevy::camera::primitives::Aabb;
use bevy::camera::primitives::MeshAabb;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{AssetId, Assets, Entity, Handle, Mesh};
use cyrus_assets::environment::{Environment, Water};

/// One terrain mesh to bind: its entity, its source mesh, and whether its texture is water by
/// SURFACE.INI (`crate::fog::is_water_texture`), which places the Modern shore effects.
pub struct TerrainTarget {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub water_texture: bool,
}

impl TerrainTarget {
    /// The target for a spawned terrain mesh entity, None when it has no mesh.
    pub fn of_entity(
        world: &bevy::prelude::World,
        entity: Entity,
        environment: &Environment,
    ) -> Option<Self> {
        Some(Self {
            entity,
            mesh: world.get::<bevy::prelude::Mesh3d>(entity)?.0.clone(),
            water_texture: crate::fog::is_water_texture(
                world.get::<bevy::gltf::GltfMaterialName>(entity),
                environment,
            ),
        })
    }
}

/// Private mesh binding produced without changing any entity or live playback.
pub struct PreparedTerrainMesh {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub bounds: Option<Aabb>,
}

impl WaterPlayback {
    /// Bind explicit source meshes without advancing waves, lighting or visibility.
    /// Callers own target-entity validation and publish only into their staged scene.
    /// All source validation finishes before private mesh assets are allocated.
    pub fn prepare_meshes(
        environment: &Environment,
        source: AssetId<Environment>,
        targets: &[TerrainTarget],
        meshes: &mut Assets<Mesh>,
    ) -> Result<(Self, Vec<PreparedTerrainMesh>)> {
        environment.validate()?;
        let water = environment
            .water
            .as_ref()
            .filter(|water| water.parameters[0] != 0.0);
        let side = environment
            .terrain_heights
            .as_ref()
            .map(|terrain| terrain.side);
        let Some(side) = side else {
            ensure!(
                targets.is_empty(),
                "terrain bindings supplied for environment without terrain playback"
            );
            return Ok((
                Self {
                    source: Some(source),
                    ..Self::default()
                },
                Vec::new(),
            ));
        };
        ensure!(
            !targets.is_empty(),
            "terrain environment has no prepared mesh targets"
        );
        let size = side
            .checked_mul(side)
            .context("terrain grid dimensions overflow")?;
        ensure!(size > 0, "terrain grid is empty");
        if let Some(water) = water {
            ensure!(
                water.side == side && water.animated_vertices.len() == size,
                "terrain water grid differs from engine-height grid"
            );
        }
        let heights = environment.terrain_heights.as_ref().unwrap();
        let base: Vec<_> = heights
            .heights
            .iter()
            .enumerate()
            .map(|(i, &height)| {
                [
                    -((i % side) as f32) * CELL_SIZE,
                    -height / 20.0,
                    -((i / side) as f32) * CELL_SIZE,
                ]
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut prepared = Vec::with_capacity(targets.len());
        // Per grid cell, indexed by its lowest corner: None where no mesh covers it, else whether
        // its texture is water.
        let mut water_cells = vec![None; size];
        for TerrainTarget {
            entity,
            mesh: original,
            water_texture,
        } in targets
        {
            ensure!(seen.insert(*entity), "terrain target {entity:?} duplicated");
            let mesh = meshes
                .get(original)
                .with_context(|| format!("terrain {entity:?}: source mesh unavailable"))?;
            let Ok(VertexAttributeValues::Float32x3(positions)) =
                mesh.try_attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                anyhow::bail!("terrain {entity:?}: missing positions");
            };
            let mut vertices = Vec::with_capacity(positions.len());
            for &position in positions {
                ensure!(
                    position.iter().all(|value| value.is_finite()),
                    "terrain {entity:?}: nonfinite position"
                );
                let x = (-position[0] / CELL_SIZE).round() as usize;
                let row = (-position[2] / CELL_SIZE).round() as usize;
                ensure!(
                    x < side && row < side,
                    "terrain {entity:?}: vertex outside {side}-wide grid"
                );
                let vertex = x + row * side;
                vertices.push(vertex);
            }
            mark_cells(&mut water_cells, mesh, &vertices, side, *water_texture);
            let mesh_source = crate::terrain::Source::new(mesh, vertices.len())
                .map_err(|error| anyhow::anyhow!("terrain {entity:?}: {error}"))?;
            let mut animated = mesh.clone();
            animated.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vertices
                    .iter()
                    .map(|&vertex| base[vertex])
                    .collect::<Vec<_>>(),
            );
            let bounds = animated.get_aabb().map(|mut bounds| {
                bounds.half_extents.y +=
                    water.map_or(0.0, |water| water.parameters[0].abs()) * 1.5 / 20.0;
                animated.final_aabb = Some(bounds.into());
                bounds
            });
            prepared.push((
                *entity,
                original.clone(),
                vertices,
                mesh_source,
                animated,
                bounds,
            ));
        }
        let shore = Shore::new(water_cells, water, side);
        for (_, _, vertices, _, animated, _) in &mut prepared {
            let channel = shore.channel(water, animated, vertices);
            animated.insert_attribute(Mesh::ATTRIBUTE_UV_1, channel);
        }
        let mut playback = Self {
            source: Some(source),
            base,
            shore,
            ..Default::default()
        };
        let mut bindings = Vec::with_capacity(prepared.len());
        for (entity, original, vertices, source, mesh, bounds) in prepared {
            let animated = meshes.add(mesh);
            bindings.push(PreparedTerrainMesh {
                entity,
                mesh: animated.clone(),
                bounds,
            });
            playback.terrain.push(TerrainMesh {
                entity,
                original,
                animated,
                vertices,
                source,
                view: None,
            });
        }
        Ok((playback, bindings))
    }
}

/// Where the terrain meets the water, for the Modern shore effects.
#[derive(Default)]
pub(super) struct Shore {
    /// Distance of every grid vertex to the shore (`shore_field`).
    distance: Vec<f32>,
    /// Per grid cell, by its lowest corner: None where no mesh covers it, else whether its texture
    /// is water.
    cells: Vec<Option<bool>>,
    side: usize,
}

impl Shore {
    fn new(cells: Vec<Option<bool>>, water: Option<&Water>, side: usize) -> Self {
        Self {
            distance: shore_field(&cells, water, side),
            cells,
            side,
        }
    }

    /// The terrain's water channel for `mesh`, whose vertices sit at grid `vertices`: x is 1 on the
    /// water's wave vertices; y is the grid vertex's shore distance plus ten times its cell's
    /// `land_borders` mask, which is the same on every vertex of the cell and so survives
    /// interpolation. The distance stays within ±4.5, so the shader splits the two exactly.
    pub(super) fn channel(
        &self,
        water: Option<&Water>,
        mesh: &Mesh,
        vertices: &[usize],
    ) -> Vec<[f32; 2]> {
        let borders = self.land_borders(mesh, vertices);
        vertices
            .iter()
            .zip(borders)
            .map(|(&vertex, mask)| {
                let wave = water.is_some_and(|water| water.animated_vertices[vertex]);
                let distance = self.distance.get(vertex).copied().unwrap_or(-4.0);
                [f32::from(u8::from(wave)), distance + 10.0 * f32::from(mask)]
            })
            .collect()
    }

    /// For each mesh vertex, which edges and corners of its water cell's texture tile border a land
    /// cell, in the tile's UV space, so the shader finds a waterline that runs along a cell border
    /// whatever the tile's rotation. Bits 0 to 3: the edges at u = 0, u = 1, v = 0 and v = 1; bits 4
    /// to 7: the corners at (0, 0), (1, 0), (0, 1) and (1, 1) where only the diagonal neighbour is land.
    /// Zero on land cells and on cells whose triangles are not whole.
    fn land_borders(&self, mesh: &Mesh, vertices: &[usize]) -> Vec<u8> {
        let mut masks = vec![0u8; vertices.len()];
        let (Some(indices), Ok(VertexAttributeValues::Float32x2(uvs))) =
            (mesh.indices(), mesh.try_attribute(Mesh::ATTRIBUTE_UV_0))
        else {
            return masks;
        };
        let side = self.side;
        let indices: Vec<usize> = indices.iter().collect();
        let cell_of = |triangle: &[usize; 3]| {
            let corners = triangle.iter().filter_map(|&index| vertices.get(index));
            let x = corners.clone().map(|&vertex| vertex % side).min()?;
            let row = corners.map(|&vertex| vertex / side).min()?;
            Some((x, row))
        };
        // Each cell's tile corner, as (u > 0.5, v > 0.5), at each of its grid corners.
        type TileCorners = [Option<(bool, bool)>; 4];
        let mut corners: std::collections::HashMap<(usize, usize), TileCorners> =
            std::collections::HashMap::new();
        for triangle in indices.as_chunks::<3>().0 {
            let Some((x, row)) = cell_of(triangle) else {
                continue;
            };
            for &index in triangle {
                let (Some(&vertex), Some(uv)) = (vertices.get(index), uvs.get(index)) else {
                    continue;
                };
                let corner = (vertex % side - x) + 2 * (vertex / side - row);
                if let Some(slot) = corners.entry((x, row)).or_default().get_mut(corner) {
                    *slot = Some((uv[0] > 0.5, uv[1] > 0.5));
                }
            }
        }
        let land = |x: isize, row: isize| {
            (0..side as isize).contains(&x)
                && (0..side as isize).contains(&row)
                && self.cells[x as usize + row as usize * side] == Some(false)
        };
        for triangle in indices.as_chunks::<3>().0 {
            let Some((x, row)) = cell_of(triangle) else {
                continue;
            };
            if self.cells[x + row * side] != Some(true) {
                continue;
            }
            let Some(tile) = corners
                .get(&(x, row))
                .and_then(|corners| corners.iter().copied().collect::<Option<Vec<_>>>())
            else {
                continue;
            };
            let (gx, grow) = (x as isize, row as isize);
            let mut mask = 0u8;
            // A grid edge is the tile edge where its two corners share u or v.
            let edge = |a: (bool, bool), b: (bool, bool)| match (a, b) {
                ((u, _), (w, _)) if u == w => 1u8 << u8::from(u),
                ((_, v), (_, w)) if v == w => 1u8 << (2 + u8::from(v)),
                _ => 0,
            };
            // Grid corners: 0 (x, row), 1 (x + 1, row), 2 (x, row + 1), 3 (x + 1, row + 1).
            let neighbours = [
                ((-1, 0), (0, 2)),
                ((1, 0), (1, 3)),
                ((0, -1), (0, 1)),
                ((0, 1), (2, 3)),
            ];
            for ((dx, dz), (a, b)) in neighbours {
                if land(gx + dx, grow + dz) {
                    mask |= edge(tile[a], tile[b]);
                }
            }
            let diagonals = [((-1, -1), 0), ((1, -1), 1), ((-1, 1), 2), ((1, 1), 3)];
            for ((dx, dz), corner) in diagonals {
                if land(gx + dx, grow + dz) {
                    let (u, v) = tile[corner];
                    mask |= 1 << (4 + u8::from(u) + 2 * u8::from(v));
                }
            }
            for &index in triangle {
                if let Some(slot) = masks.get_mut(index) {
                    *slot = mask;
                }
            }
        }
        masks
    }
}

/// Record the grid cells a terrain mesh's triangles cover, each by its lowest corner, as water or
/// not by the mesh's texture.
fn mark_cells(
    cells: &mut [Option<bool>],
    mesh: &Mesh,
    vertices: &[usize],
    side: usize,
    water_texture: bool,
) {
    let Some(indices) = mesh.indices() else {
        return;
    };
    let indices: Vec<usize> = indices.iter().collect();
    for triangle in indices.as_chunks::<3>().0 {
        let corners = triangle.iter().filter_map(|&index| vertices.get(index));
        let x = corners.clone().map(|&vertex| vertex % side).min();
        let row = corners.map(|&vertex| vertex / side).min();
        if let (Some(x), Some(row)) = (x, row) {
            cells[x + row * side] = Some(water_texture);
        }
    }
}

/// The shore distance at every grid vertex (`shore_distances`). A vertex is water when every
/// covered cell around it has a water texture, so a vertex on the border of a land cell is land
/// even where the waves reach it. The water textures include the beach transition tiles, whose
/// drawn waterline crosses the cell; the terrain shader finds that line in the texels. Caches that
/// predate the water textures fall back to the wave vertices; without either, every vertex is far
/// inland.
fn shore_field(cells: &[Option<bool>], water: Option<&Water>, side: usize) -> Vec<f32> {
    let around = |vertex: usize| {
        let (x, row) = (vertex % side, vertex / side);
        [(0, 0), (1, 0), (0, 1), (1, 1)]
            .into_iter()
            .filter(move |&(dx, dz)| x >= dx && row >= dz)
            .filter_map(move |(dx, dz)| cells[x - dx + (row - dz) * side])
    };
    if cells.contains(&Some(true)) {
        let textured: Vec<bool> = (0..cells.len())
            .map(|vertex| around(vertex).next().is_some() && around(vertex).all(|water| water))
            .collect();
        return shore_distances(&textured, side);
    }
    match water {
        Some(water) => shore_distances(&water.animated_vertices, side),
        None => vec![-4.0; cells.len()],
    }
}

/// Distance of every grid vertex to the shore, in grid cells, for the Modern shore effects: positive
/// on water (`animated` vertices), negative on land, zero halfway between a water vertex and its
/// land neighbour, and at most `SHORE_REACH` either way. Chamfer distances over the 8-neighbourhood.
pub(super) fn shore_distances(animated: &[bool], side: usize) -> Vec<f32> {
    const SHORE_REACH: f32 = 4.0;
    let steps = [
        (-1, -1, std::f32::consts::SQRT_2),
        (0, -1, 1.0),
        (1, -1, std::f32::consts::SQRT_2),
        (-1, 0, 1.0),
    ];
    // Distance from each vertex to the nearest vertex of the other kind.
    let mut distance: Vec<f32> = (0..animated.len())
        .map(|vertex| {
            let (x, row) = ((vertex % side) as isize, (vertex / side) as isize);
            let border = (-1..=1).any(|dz| {
                (-1..=1).any(|dx| {
                    let (nx, nz) = (x + dx, row + dz);
                    (0..side as isize).contains(&nx)
                        && (0..side as isize).contains(&nz)
                        && animated[nx as usize + nz as usize * side] != animated[vertex]
                })
            });
            if border { 1.0 } else { f32::INFINITY }
        })
        .collect();
    let index = |x: isize, row: isize| {
        ((0..side as isize).contains(&x) && (0..side as isize).contains(&row))
            .then(|| x as usize + row as usize * side)
    };
    for vertex in 0..animated.len() {
        let (x, row) = ((vertex % side) as isize, (vertex / side) as isize);
        for (dx, dz, cost) in steps {
            if let Some(neighbour) = index(x + dx, row + dz)
                && animated[neighbour] == animated[vertex]
            {
                distance[vertex] = distance[vertex].min(distance[neighbour] + cost);
            }
        }
    }
    for vertex in (0..animated.len()).rev() {
        let (x, row) = ((vertex % side) as isize, (vertex / side) as isize);
        for (dx, dz, cost) in steps {
            if let Some(neighbour) = index(x - dx, row - dz)
                && animated[neighbour] == animated[vertex]
            {
                distance[vertex] = distance[vertex].min(distance[neighbour] + cost);
            }
        }
    }
    distance
        .iter()
        .zip(animated)
        .map(|(distance, &water)| {
            let reach = (distance - 0.5).min(SHORE_REACH);
            if water { reach } else { -reach }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Shore, shore_distances, shore_field};
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    use bevy::prelude::Mesh;

    #[test]
    fn land_borders_name_the_tile_edge_whatever_its_rotation() {
        // Cell (0, 0) is water, its east neighbour (1, 0) land, on a 3-wide grid.
        let side = 3;
        let mut cells = vec![None; side * side];
        cells[0] = Some(true);
        cells[1] = Some(false);
        let shore = Shore {
            distance: vec![0.0; side * side],
            cells,
            side,
        };
        // Grid corners 0, 1, 3, 4 of cell (0, 0), as two unrolled triangles.
        let vertices = [0, 1, 3, 1, 4, 3];
        let mesh = |uvs: [[f32; 2]; 4]| {
            let corner = |vertex: usize| match vertex {
                0 => uvs[0],
                1 => uvs[1],
                3 => uvs[2],
                _ => uvs[3],
            };
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_0,
                vertices
                    .iter()
                    .map(|&vertex| corner(vertex))
                    .collect::<Vec<_>>(),
            )
            .with_inserted_indices(Indices::U32((0..6).collect()))
        };
        // Unrotated, the east grid edge is the tile's u = 1 edge; turned, its v = 0 edge.
        let upright = mesh([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]);
        assert_eq!(shore.land_borders(&upright, &vertices), vec![2; 6]);
        let turned = mesh([[0.0, 1.0], [0.0, 0.0], [1.0, 1.0], [1.0, 0.0]]);
        assert_eq!(shore.land_borders(&turned, &vertices), vec![4; 6]);
    }

    #[test]
    fn shore_follows_water_textured_cells_not_the_wave_vertices() {
        // A 6-wide grid whose left two cell columns are water-textured; no wave vertices at all.
        let side = 6;
        let cells: Vec<Option<bool>> = (0..side * side)
            .map(|cell| {
                (cell % side < side - 1 && cell / side < side - 1).then_some(cell % side < 2)
            })
            .collect();
        let shore = shore_field(&cells, None, side);
        // In row 2, vertices 0 and 1 touch only water cells; vertex 2 also touches land.
        assert_eq!(shore[2 * side..2 * side + 4], [1.5, 0.5, -0.5, -1.5]);
        // Wave vertices on land-textured cells stay land; without water textures they stand in.
        let water = cyrus_assets::environment::Water {
            parameters: [1.0, 1.0, 1.0],
            side,
            animated_vertices: (0..side * side).map(|vertex| vertex % side >= 4).collect(),
        };
        assert_eq!(shore_field(&cells, Some(&water), side), shore);
        let untextured = vec![Some(false); side * side];
        assert!(shore_field(&untextured, Some(&water), side)[2 * side + 5] > 0.0);
    }

    #[test]
    fn shore_distance_is_signed_and_halfway_between_water_and_land() {
        // Water on the left three columns of a 6-wide grid.
        let side = 6;
        let animated: Vec<bool> = (0..side * side).map(|vertex| vertex % side < 3).collect();
        let shore = shore_distances(&animated, side);
        let row: Vec<f32> = shore[2 * side..3 * side].to_vec();
        assert_eq!(row, vec![2.5, 1.5, 0.5, -0.5, -1.5, -2.5]);
        let land_only = shore_distances(&[false; 4], 2);
        assert!(
            land_only.iter().all(|&value| value == -4.0),
            "no water is far from shore"
        );
    }
}
