//! Terrain source binding shared by ordinary entry and off-live preparation.
use super::{CELL_SIZE, TerrainMesh, WaterPlayback};
use anyhow::{Context, Result, ensure};
use bevy::camera::primitives::Aabb;
use bevy::camera::primitives::MeshAabb;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{AssetId, Assets, Entity, Handle, Mesh};
use cyrus_assets::environment::Environment;

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
            animated.insert_attribute(
                Mesh::ATTRIBUTE_UV_1,
                vertices
                    .iter()
                    .map(|&vertex| {
                        [
                            u8::from(water.is_some_and(|water| water.animated_vertices[vertex]))
                                as f32,
                            0.0,
                        ]
                    })
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
