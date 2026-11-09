//! Terrain source binding shared by ordinary entry and off-live preparation.
use super::{CELL_SIZE, TerrainMesh, WaterPlayback};
use anyhow::{Context, Result, ensure};
use bevy::camera::primitives::Aabb;
use bevy::camera::primitives::MeshAabb;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{AssetId, Assets, Entity, Handle, Mesh};
use cyrus_assets::environment::{Environment, Water};

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
        targets: &[(Entity, Handle<Mesh>)],
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
        for (entity, original) in targets {
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
            animated.insert_attribute(Mesh::ATTRIBUTE_UV_1, water_channel(water, &vertices));
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
        let mut playback = Self {
            source: Some(source),
            base,
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

/// The terrain's water channel for the mesh vertices at grid `vertices`: x is 1 on the water's wave
/// vertices, y the distance to the shore (`shore_distances`) the Modern shore effects read.
pub(super) fn water_channel(water: Option<&Water>, vertices: &[usize]) -> Vec<[f32; 2]> {
    let shore = water.map(|water| shore_distances(&water.animated_vertices, water.side));
    vertices
        .iter()
        .map(|&vertex| match (water, &shore) {
            (Some(water), Some(shore)) => [
                f32::from(u8::from(water.animated_vertices[vertex])),
                shore[vertex],
            ],
            // Without water every vertex is land, far from any shore.
            _ => [0.0, -4.0],
        })
        .collect()
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
    use super::shore_distances;

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
