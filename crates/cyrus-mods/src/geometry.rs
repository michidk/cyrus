//! Validated owner-scoped raster mesh uploads. Guest code owns all placement and LOD policy.
use crate::Catalog;
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use cyrus_render::fog::geometry::{GeometryContext, GeometryMaterial};
use cyrus_script::script::ScriptActors;
use std::collections::{BTreeMap, HashMap};
use wasvy::prelude::*;

const OWNER_VERTICES: usize = 1_500_000;
const TOTAL_VERTICES: usize = 3_000_000;
#[derive(Clone, Copy, PartialEq, Eq, Reflect)]
struct Lease {
    instance: Entity,
    revision: u64,
}
#[derive(Clone, Reflect)]
struct Upload {
    vertices: Vec<f32>,
    indices: Vec<u32>,
    parameters: Vec4,
    padding: f32,
    lease: Lease,
    world: (u32, u64),
}
/// Mesh storage keyed by owner and guest key. Uploads replace atomically before render allocation.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Geometry {
    owners: BTreeMap<String, Lease>,
    world: Option<(u32, u64)>,
    uploads: BTreeMap<(String, String), Upload>,
    dirty: Vec<(String, String)>,
}
#[wasvy::methods]
impl Geometry {
    /// Returns 0 accepted, 1 unavailable, 2 invalid, 3 budget exceeded.
    pub fn replace(
        &mut self,
        owner: String,
        key: String,
        revision: u64,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        parameters: Vec<f32>,
    ) -> u32 {
        let (Some(lease), Some(world)) = (self.owners.get(&owner).copied(), self.world) else {
            return 1;
        };
        if revision != world.1 {
            return 1;
        }
        let Ok(parameters) = <[f32; 5]>::try_from(parameters) else {
            return 2;
        };
        // Twelve floats per vertex: position, normal, UV, linear RGBA.
        let count = vertices.len() / 12;
        if key.is_empty()
            || key.len() > 96
            || count == 0
            || !vertices.len().is_multiple_of(12)
            || !indices.len().is_multiple_of(3)
            || indices.is_empty()
            || indices.len() > count * 6
            || vertices.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || parameters.iter().any(|v| !v.is_finite() || v.abs() > 100.0)
            || parameters[4] < 0.0
            || indices.iter().any(|&i| i as usize >= count)
            || vertices.as_chunks::<12>().0.iter().any(|v| {
                Vec3::new(v[3], v[4], v[5]).length_squared() < 0.01
                    || v[8..12].iter().any(|c| !(0.0..=1.0).contains(c))
            })
        {
            return 2;
        }
        let id = (owner.clone(), key);
        let total: usize = self
            .uploads
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(_, v)| v.vertices.len() / 12)
            .sum();
        let owned: usize = self
            .uploads
            .iter()
            .filter(|(k, _)| **k != id && k.0 == owner)
            .map(|(_, v)| v.vertices.len() / 12)
            .sum();
        if total + count > TOTAL_VERTICES
            || owned + count > OWNER_VERTICES
            || (!self.uploads.contains_key(&id)
                && self.uploads.keys().filter(|k| k.0 == owner).count() >= 256)
        {
            return 3;
        }
        self.uploads.insert(
            id.clone(),
            Upload {
                vertices,
                indices,
                parameters: Vec4::new(parameters[0], parameters[1], parameters[2], parameters[3]),
                padding: parameters[4],
                lease,
                world,
            },
        );
        if !self.dirty.contains(&id) {
            self.dirty.push(id);
        }
        0
    }
    pub fn remove(&mut self, owner: String, key: String) {
        let id = (owner, key);
        if self.uploads.remove(&id).is_some() && !self.dirty.contains(&id) {
            self.dirty.push(id);
        }
    }
    pub fn contains(&mut self, owner: String, key: String) -> bool {
        self.uploads
            .get(&(owner.clone(), key))
            .is_some_and(|upload| {
                self.owners.get(&owner) == Some(&upload.lease) && Some(upload.world) == self.world
            })
    }
    pub fn generation(&mut self) -> Option<u64> {
        self.world.map(|w| w.1)
    }
}
struct Live {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<GeometryMaterial>,
}
#[derive(Resource, Default)]
pub(crate) struct GeometryState {
    live: BTreeMap<(String, String), Live>,
}

pub(crate) fn publish(
    mut host: Query<&mut Geometry>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    mods: Query<&wasvy::prelude::Mod>,
    mut events: MessageReader<AssetEvent<ModAsset>>,
    mut revisions: Local<HashMap<AssetId<ModAsset>, u64>>,
) {
    for event in events.read() {
        if let AssetEvent::LoadedWithDependencies { id } = event {
            *revisions.entry(*id).or_default() += 1;
        }
    }
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    host.world = actors
        .as_deref()
        .and_then(|a| a.current.as_ref())
        .map(|w| (w.world, w.generation()));
    host.owners = catalog
        .entries
        .iter()
        .filter(|e| e.active && !e.gone)
        .filter_map(|e| {
            let instance = e.instance?;
            let asset = mods.get(instance).ok()?.asset().id();
            Some((
                e.id.clone(),
                Lease {
                    instance,
                    revision: revisions.get(&asset).copied().unwrap_or_default(),
                },
            ))
        })
        .collect();
}
#[allow(clippy::too_many_arguments)] // ECS asset stores and lease inputs are independent system parameters.
pub(crate) fn update(
    mut commands: Commands,
    mut host: Query<&mut Geometry>,
    mut state: ResMut<GeometryState>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    context: Res<GeometryContext>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GeometryMaterial>>,
) {
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    let world = actors
        .as_deref()
        .and_then(|a| a.current.as_ref())
        .map(|w| (w.world, w.generation()));
    let owners = host.owners.clone();
    host.uploads.retain(|(owner, _), upload| {
        Some(upload.world) == world
            && owners.get(owner) == Some(&upload.lease)
            && catalog
                .entries
                .iter()
                .any(|e| &e.id == owner && e.active && !e.gone)
    });
    let stale: Vec<_> = state
        .live
        .keys()
        .filter(|key| !host.uploads.contains_key(*key) || host.dirty.contains(*key))
        .cloned()
        .collect();
    for key in stale {
        if let Some(live) = state.live.remove(&key) {
            commands.entity(live.entity).despawn();
            meshes.remove(live.mesh.id());
            materials.remove(live.material.id());
        }
    }
    let pending = std::mem::take(&mut host.dirty);
    for key in pending {
        let Some(upload) = host.uploads.get(&key) else {
            continue;
        };
        let Some(material) = context.material(upload.parameters) else {
            host.dirty.push(key);
            continue;
        };
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut colors = Vec::new();
        for v in upload.vertices.as_chunks::<12>().0 {
            positions.push([v[0], v[1], v[2]]);
            normals.push([v[3], v[4], v[5]]);
            uvs.push([v[6], v[7]]);
            colors.push([v[8], v[9], v[10], v[11]]);
        }
        let (low, high) = positions
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(low, high), point| {
                (low.min(Vec3::from(*point)), high.max(Vec3::from(*point)))
            });
        let bounds = bevy::camera::primitives::Aabb::from_min_max(
            low - Vec3::splat(upload.padding),
            high + Vec3::splat(upload.padding),
        );
        let mesh = meshes.add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(upload.indices.clone())),
        );
        debug!(
            "mods geometry: {}/{}: {} vertices",
            key.0,
            key.1,
            upload.vertices.len() / 12
        );
        let material = materials.add(material);
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                NotShadowCaster,
                bounds,
            ))
            .id();
        state.live.insert(
            key,
            Live {
                entity,
                mesh,
                material,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host() -> Geometry {
        Geometry {
            world: Some((1, 4)),
            owners: BTreeMap::from([(
                "test".into(),
                Lease {
                    instance: Entity::PLACEHOLDER,
                    revision: 1,
                },
            )]),
            ..default()
        }
    }
    fn vertices() -> Vec<f32> {
        [
            [0., 0., 0., 0., 1., 0., 0., 0., 0.2, 0.4, 0.1, 1.],
            [1., 0., 0., 0., 1., 0., 0., 0., 0.2, 0.4, 0.1, 1.],
            [0., 1., 0., 0., 1., 0., 0., 0., 0.2, 0.4, 0.1, 1.],
        ]
        .concat()
    }
    #[test]
    fn invalid_uploads_preserve_previous_mesh_and_reject_stale_generation() {
        let mut host = host();
        assert_eq!(
            host.replace(
                "test".into(),
                "chunk".into(),
                4,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            0
        );
        let original = host.uploads[&("test".into(), "chunk".into())]
            .vertices
            .clone();
        for indices in [vec![0, 1, 3], vec![0, 1], vec![]] {
            assert_eq!(
                host.replace(
                    "test".into(),
                    "chunk".into(),
                    4,
                    vertices(),
                    indices,
                    vec![0.; 5]
                ),
                2
            );
        }
        let mut nan = vertices();
        nan[0] = f32::NAN;
        assert_eq!(
            host.replace(
                "test".into(),
                "chunk".into(),
                4,
                nan,
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            2
        );
        assert_eq!(
            host.replace(
                "test".into(),
                "chunk".into(),
                3,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            1
        );
        assert_eq!(
            host.replace(
                "absent".into(),
                "chunk".into(),
                4,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            1
        );
        assert_eq!(
            host.uploads[&("test".into(), "chunk".into())].vertices,
            original
        );
    }
    #[test]
    fn key_budget_rejects_atomically_but_allows_replacement_and_removal() {
        let mut host = host();
        for index in 0..256 {
            assert_eq!(
                host.replace(
                    "test".into(),
                    format!("{index}"),
                    4,
                    vertices(),
                    vec![0, 1, 2],
                    vec![0.; 5]
                ),
                0
            );
        }
        assert_eq!(
            host.replace(
                "test".into(),
                "overflow".into(),
                4,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            3
        );
        assert_eq!(host.uploads.len(), 256);
        assert_eq!(
            host.replace(
                "test".into(),
                "0".into(),
                4,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            0
        );
        host.remove("test".into(), "0".into());
        assert!(!host.contains("test".into(), "0".into()));
        assert_eq!(
            host.replace(
                "test".into(),
                "overflow".into(),
                4,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            0
        );
    }
    #[test]
    fn completed_reload_and_world_replacement_invalidate_retained_keys() {
        let mut host = host();
        assert_eq!(
            host.replace(
                "test".into(),
                "chunk".into(),
                4,
                vertices(),
                vec![0, 1, 2],
                vec![0.; 5]
            ),
            0
        );
        assert!(host.contains("test".into(), "chunk".into()));
        host.owners.get_mut("test").unwrap().revision += 1;
        assert!(!host.contains("test".into(), "chunk".into()));
        host.owners.get_mut("test").unwrap().revision -= 1;
        host.world = Some((1, 5));
        assert!(!host.contains("test".into(), "chunk".into()));
    }
}
