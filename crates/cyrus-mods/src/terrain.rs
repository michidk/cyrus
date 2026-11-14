//! Bounded, revision-scoped reads of retained terrain geometry and installed image pixels.
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use cyrus_render::{WaterPlayback, fog::geometry::terrain_texture};
use cyrus_script::script::ScriptActors;
use wasvy::prelude::*;

#[derive(Clone, Reflect)]
enum SnapshotFailure {
    Unsupported,
    OverBudget,
}

#[derive(Clone, Reflect)]
struct Tile {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Snapshot shared read-only by guests. No camera-clipped meshes are exposed.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Terrain {
    revision: u64,
    ready: bool,
    failure: Option<SnapshotFailure>,
    triangles: Vec<Vec<f32>>,
    tiles: Vec<Tile>,
    world: Option<(u32, u64)>,
    entities: Vec<Entity>,
}
#[wasvy::methods]
impl Terrain {
    /// 0 ready, 1 loading, 2 no world/terrain, 3 stale, 4 unsupported, 5 over budget.
    pub fn status(&self, revision: u64) -> u32 {
        if self.world.is_none() || self.entities.is_empty() {
            2
        } else if let Some(failure) = &self.failure {
            match failure {
                SnapshotFailure::Unsupported => 4,
                SnapshotFailure::OverBudget => 5,
            }
        } else if !self.ready {
            1
        } else if revision != self.revision {
            3
        } else {
            0
        }
    }
    pub fn revision(&self) -> Option<u64> {
        self.ready.then_some(self.revision)
    }
    /// Bounds are [min x,min z,max x,max z]. Each record is texture, stable triangle
    /// index, then three corners of [scene x,y,z,u,v]. At most 512 records per page.
    pub fn triangles(
        &self,
        revision: u64,
        bounds: Vec<f32>,
        offset: u32,
        limit: u32,
    ) -> Option<Vec<f32>> {
        let [x, z, right, bottom]: [f32; 4] = bounds.try_into().ok()?;
        if !self.ready
            || revision != self.revision
            || limit == 0
            || limit > 512
            || ![x, z, right, bottom].iter().all(|v| v.is_finite())
            || right < x
            || bottom < z
            || right - x > 512.0
            || bottom - z > 512.0
        {
            return None;
        }
        Some(
            self.triangles
                .iter()
                .filter(|t| {
                    let cx = (t[2] + t[7] + t[12]) / 3.0;
                    let cz = (t[4] + t[9] + t[14]) / 3.0;
                    cx >= x && cx < right && cz >= z && cz < bottom
                })
                .skip(offset as usize)
                .take(limit as usize)
                .flatten()
                .copied()
                .collect(),
        )
    }
    pub fn dimensions(&self, revision: u64, texture: u32) -> Option<Vec<u32>> {
        if !self.ready || revision != self.revision {
            return None;
        }
        let tile = self.tiles.get(texture as usize)?;
        Some(vec![tile.width, tile.height])
    }
    pub fn pixels(
        &self,
        revision: u64,
        texture: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Option<Vec<u8>> {
        if !self.ready
            || revision != self.revision
            || width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > 65536
        {
            return None;
        }
        let tile = self.tiles.get(texture as usize)?;
        if x.checked_add(width)? > tile.width || y.checked_add(height)? > tile.height {
            return None;
        }
        Some(
            (y..y + height)
                .flat_map(|row| {
                    let start = ((row * tile.width + x) * 4) as usize;
                    tile.pixels[start..start + (width * 4) as usize]
                        .iter()
                        .copied()
                })
                .collect(),
        )
    }
}

/// Refresh only when the world or retained mesh bindings change. Runs before guest Update.
pub(crate) fn publish(world: &mut World) {
    let identity = world
        .get_resource::<ScriptActors>()
        .and_then(|a| a.current.as_ref())
        .map(|w| (w.world, w.generation()));
    let sources: Vec<_> = world
        .get_resource::<WaterPlayback>()
        .filter(|playback| {
            world
                .get_resource::<cyrus_render::environment::EnvironmentRequest>()
                .is_some_and(|request| playback.owns_environment(request.0.id()))
        })
        .map(|w| w.source_meshes().map(|(e, h)| (e, h.clone())).collect())
        .unwrap_or_default();
    let entities: Vec<_> = sources.iter().map(|(e, _)| *e).collect();
    let mut query = world.query::<&mut Terrain>();
    let Some(host) = query.iter(world).next() else {
        return;
    };
    if host.world == identity && host.entities == entities && (host.ready || host.failure.is_some())
    {
        return;
    }
    let revision = host.revision.wrapping_add(1);
    let mut snapshot = Terrain {
        world: identity,
        revision,
        entities,
        ..default()
    };
    if identity.is_some() && !sources.is_empty() {
        let mut handles = Vec::new();
        let mut complete = true;
        for (entity, handle) in sources {
            let Some(texture) = terrain_texture(world, entity) else {
                complete = false;
                break;
            };
            let tile = if let Some(index) = handles.iter().position(|h| h == &texture) {
                index
            } else {
                let Some(image) = world.resource::<Assets<Image>>().get(&texture) else {
                    complete = false;
                    break;
                };
                let size = image.size();
                if size.x == 0
                    || size.y == 0
                    || size.x > 512
                    || size.y > 512
                    || handles.len() >= 256
                {
                    snapshot.failure = Some(SnapshotFailure::OverBudget);
                    complete = false;
                    break;
                }
                let mut pixels = Vec::with_capacity((size.x * size.y * 4) as usize);
                for y in 0..size.y {
                    for x in 0..size.x {
                        let Ok(color) = image.get_color_at(x, y) else {
                            snapshot.failure = Some(SnapshotFailure::Unsupported);
                            complete = false;
                            continue;
                        };
                        pixels.extend(Srgba::from(color).to_u8_array());
                    }
                }
                snapshot.tiles.push(Tile {
                    width: size.x,
                    height: size.y,
                    pixels,
                });
                handles.push(texture);
                handles.len() - 1
            };
            let Some(mesh) = world.resource::<Assets<Mesh>>().get(&handle) else {
                complete = false;
                break;
            };
            let Some(positions) = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|v| v.as_float3())
            else {
                snapshot.failure = Some(SnapshotFailure::Unsupported);
                complete = false;
                break;
            };
            let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
            else {
                snapshot.failure = Some(SnapshotFailure::Unsupported);
                complete = false;
                break;
            };
            let transform = world
                .get::<GlobalTransform>(entity)
                .copied()
                .unwrap_or_default();
            let indices: Vec<_> = mesh
                .indices()
                .map(|i| i.iter().collect())
                .unwrap_or_else(|| (0..positions.len()).collect());
            for tri in indices.as_chunks::<3>().0 {
                let mut record = vec![tile as f32, snapshot.triangles.len() as f32];
                for &index in tri {
                    let (Some(point), Some(uv)) = (positions.get(index), uvs.get(index)) else {
                        complete = false;
                        break;
                    };
                    record.extend(transform.transform_point(Vec3::from(*point)).to_array());
                    record.extend(uv);
                }
                if record.len() == 17 {
                    if snapshot.triangles.len() >= 1_000_000 {
                        snapshot.failure = Some(SnapshotFailure::OverBudget);
                        complete = false;
                        break;
                    }
                    if record.iter().any(|v| !v.is_finite()) {
                        snapshot.failure = Some(SnapshotFailure::Unsupported);
                        complete = false;
                        break;
                    }
                    snapshot.triangles.push(record);
                }
            }
        }
        snapshot.ready = complete && snapshot.failure.is_none();
        if snapshot.failure.is_some() {
            warn!(
                "mods terrain: unsupported or over-budget snapshot for world {:?}",
                snapshot.world
            );
        }
        if complete {
            info!(
                "mods terrain: world {:?}, revision {}, {} triangles, {} textures",
                snapshot.world,
                revision,
                snapshot.triangles.len(),
                snapshot.tiles.len()
            );
        }
    }
    if let Some(mut host) = query.iter_mut(world).next() {
        *host = snapshot;
    }
}

#[cfg(test)]
impl Terrain {
    pub(crate) fn fixture() -> Self {
        Terrain {
            revision: 1,
            ready: true,
            triangles: vec![vec![
                0., 0., 0., 0., 0., 0., 0., 4., 0., 0., 1., 0., 0., 0., 4., 0., 1.,
            ]],
            tiles: vec![Tile {
                width: 2,
                height: 2,
                pixels: [40, 120, 50, 255].repeat(4),
            }],
            ..default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_preserve_rotated_uvs_and_assign_chunk_edges_once() {
        let triangle = vec![
            0., 7., 24., 0., 0., 1., 0., 24., 0., 3., 0., 0., 27., 0., 0., 1., 1.,
        ];
        let host = Terrain {
            revision: 3,
            ready: true,
            triangles: vec![triangle.clone()],
            tiles: vec![Tile {
                width: 2,
                height: 1,
                pixels: vec![1, 2, 3, 255, 4, 5, 6, 255],
            }],
            ..default()
        };
        assert_eq!(
            host.triangles(3, vec![24., 0., 48., 24.], 0, 512),
            Some(triangle)
        );
        assert_eq!(
            host.triangles(3, vec![0., 0., 24., 24.], 0, 512),
            Some(vec![])
        );
        assert_eq!(
            host.triangles(3, vec![24., 0., 48., 24.], 1, 512),
            Some(vec![])
        );
        assert_eq!(host.triangles(2, vec![24., 0., 48., 24.], 0, 512), None);
        assert_eq!(host.pixels(3, 0, 1, 0, 1, 1), Some(vec![4, 5, 6, 255]));
        assert_eq!(host.pixels(3, 0, 2, 0, 1, 1), None);
        assert_eq!(host.pixels(3, 0, u32::MAX, 0, 1, 1), None);
    }
}
