//! Stand-in entities that give Solari the world meshes in the layout it accepts.
use super::super::{SceneMaterial, TerrainMaterial, clipping::Draw};
use super::{
    Proxy,
    plugin::{Hidden, StandInOf, Traced, unhide},
};
use crate::flat::Flat;
use bevy::{
    asset::{RenderAssetUsages, UntypedAssetId},
    camera::visibility::RenderLayers,
    ecs::system::SystemParam,
    material::OpaqueRendererMethod,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    solari::prelude::RaytracingMesh3d,
};
use std::collections::{HashMap, HashSet};
#[derive(Default)]
pub(super) struct StandIns {
    meshes: HashMap<AssetId<Mesh>, Handle<Mesh>>,
    materials: HashMap<UntypedAssetId, Handle<StandardMaterial>>,
}
/// The traceable material of a source: asset identity, base material and solid tint.
/// Meshes whose look depends on shader-side alpha or chroma keying are not traceable.
fn traceable<'a>(
    scene: Option<&MeshMaterial3d<SceneMaterial>>,
    terrain: Option<&MeshMaterial3d<TerrainMaterial>>,
    scene_materials: &'a Assets<SceneMaterial>,
    terrain_materials: &'a Assets<TerrainMaterial>,
) -> Option<(UntypedAssetId, &'a StandardMaterial, Vec4)> {
    let (id, base, solid, keyed) = match (scene, terrain) {
        (Some(handle), _) => {
            let material = scene_materials.get(&handle.0)?;
            (
                handle.0.id().untyped(),
                &material.base,
                material.extension.solid_color,
                material.extension.chroma_key != Vec4::ZERO,
            )
        }
        (None, Some(handle)) => {
            let material = terrain_materials.get(&handle.0)?;
            (handle.0.id().untyped(), &material.base, Vec4::ZERO, false)
        }
        (None, None) => return None,
    };
    (!keyed && matches!(base.alpha_mode, AlphaMode::Opaque)).then_some((id, base, solid))
}

/// A copy of `source` with the exact vertex layout Solari accepts: position, normal, UV 0
/// and tangent, as an indexed triangle list. Tangents are zero because no material here
/// has a normal map.
fn standin_mesh(source: &Mesh) -> Option<Mesh> {
    if source.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let positions = source.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
    let normals = source.attribute(Mesh::ATTRIBUTE_NORMAL)?.as_float3()?;
    let count = positions.len();
    let uvs = match source.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uvs)) => uvs.clone(),
        _ => vec![[0.0; 2]; count],
    };
    let indices = match source.indices() {
        Some(indices) => indices.iter().map(|index| index as u32).collect(),
        None => (0..count as u32).collect(),
    };
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.to_vec())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0; 4]; count])
        .with_inserted_indices(Indices::U32(indices)),
    )
}

fn standin_material(base: &StandardMaterial, solid: Vec4) -> StandardMaterial {
    StandardMaterial {
        base_color: if solid.w != 0.0 {
            Color::srgb(solid.x / 256.0, solid.y / 256.0, solid.z / 256.0)
        } else {
            base.base_color
        },
        base_color_texture: base.base_color_texture.clone(),
        cull_mode: base.cull_mode,
        double_sided: base.double_sided,
        // The original has no specular term.
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        opaque_render_method: OpaqueRendererMethod::Deferred,
        ..default()
    }
}

/// The asset stores a stand-in is built from; all four are needed together.
#[derive(SystemParam)]
pub(super) struct StandInAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    standard: ResMut<'w, Assets<StandardMaterial>>,
    scene: Res<'w, Assets<SceneMaterial>>,
    terrain: Res<'w, Assets<TerrainMaterial>>,
}

impl StandIns {
    /// Animated and re-prepared source meshes are edited in place; rebuild their stand-ins.
    fn rebuild_edited(
        &mut self,
        meshes: &mut Assets<Mesh>,
        events: &mut MessageReader<AssetEvent<Mesh>>,
    ) {
        for event in events.read() {
            if let AssetEvent::Modified { id } = event
                && let Some(handle) = self.meshes.get(id)
                && let Some(mesh) = meshes.get(*id).and_then(standin_mesh)
                && let Err(error) = meshes.insert(handle.id(), mesh)
            {
                warn!("ray tracing stand-in mesh {id}: {error}");
            }
        }
        self.meshes.retain(|id, _| meshes.contains(*id));
    }

    /// The stand-in mesh of a source mesh, built on first use.
    fn mesh(&mut self, meshes: &mut Assets<Mesh>, source: &Handle<Mesh>) -> Option<Handle<Mesh>> {
        if let Some(mesh) = self.meshes.get(&source.id()) {
            return Some(mesh.clone());
        }
        let handle = meshes.add(meshes.get(source).and_then(standin_mesh)?);
        self.meshes.insert(source.id(), handle.clone());
        Some(handle)
    }

    /// The stand-in material shared by every mesh using one source material.
    fn material(
        &mut self,
        standard: &mut Assets<StandardMaterial>,
        id: UntypedAssetId,
        base: &StandardMaterial,
        solid: Vec4,
    ) -> Handle<StandardMaterial> {
        let handle = self
            .materials
            .entry(id)
            .or_insert_with(|| standard.add(standin_material(base, solid)))
            .clone();
        // Texture animation edits the source material's base texture in place.
        if standard
            .get(&handle)
            .is_some_and(|stand_in| stand_in.base_color_texture != base.base_color_texture)
            && let Some(mut stand_in) = standard.get_mut(&handle)
        {
            stand_in.base_color_texture = base.base_color_texture.clone();
        }
        handle
    }
}

type SourceData = (
    Entity,
    &'static Mesh3d,
    &'static GlobalTransform,
    &'static InheritedVisibility,
    Option<&'static MeshMaterial3d<SceneMaterial>>,
    Option<&'static MeshMaterial3d<TerrainMaterial>>,
    Has<Traced>,
    Option<&'static Hidden>,
    Option<&'static RenderLayers>,
);
type SourceFilter = (
    Without<Proxy>,
    Without<Draw>,
    Without<Flat>,
    Or<(
        With<MeshMaterial3d<SceneMaterial>>,
        With<MeshMaterial3d<TerrainMaterial>>,
    )>,
);
type ProxyData = (
    Entity,
    &'static StandInOf,
    &'static mut Transform,
    &'static mut GlobalTransform,
    &'static mut Mesh3d,
    &'static mut RaytracingMesh3d,
    &'static mut MeshMaterial3d<StandardMaterial>,
);

pub(super) fn sync_proxies(
    mut commands: Commands,
    mut assets: StandInAssets,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    sources: Query<SourceData, SourceFilter>,
    mut proxies: Query<ProxyData>,
    mut stand_ins: Local<StandIns>,
) {
    stand_ins.rebuild_edited(&mut assets.meshes, &mut mesh_events);
    let live = update_proxies(
        &mut commands,
        &mut assets,
        &mut stand_ins,
        &sources,
        &mut proxies,
    );
    spawn_proxies(&mut commands, &mut assets, &mut stand_ins, &sources, &live);
}

/// Follows each stand-in to its source, and drops those whose source stopped being traceable.
/// Returns the sources that still have one.
fn update_proxies(
    commands: &mut Commands,
    assets: &mut StandInAssets,
    stand_ins: &mut StandIns,
    sources: &Query<SourceData, SourceFilter>,
    proxies: &mut Query<ProxyData>,
) -> HashSet<Entity> {
    let mut live = HashSet::new();
    for (entity, stand_in, mut transform, mut global, mut mesh, mut tracing, mut material) in
        proxies
    {
        let traceable = sources.get(stand_in.0).ok().and_then(
            |(_, source_mesh, source_global, inherited, scene, terrain, ..)| {
                let (id, base, solid) = traceable(scene, terrain, &assets.scene, &assets.terrain)?;
                inherited
                    .get()
                    .then_some((source_mesh, source_global, id, base, solid))
            },
        );
        let Some((source_mesh, source_global, id, base, solid)) = traceable else {
            commands.entity(entity).despawn();
            continue;
        };
        live.insert(stand_in.0);
        if *global != *source_global {
            *global = *source_global;
            *transform = source_global.compute_transform();
        }
        if let Some(handle) = stand_ins.meshes.get(&source_mesh.0.id())
            && mesh.0 != *handle
        {
            mesh.0 = handle.clone();
            tracing.0 = handle.clone();
        }
        let handle = stand_ins.material(&mut assets.standard, id, base, solid);
        if material.0 != handle {
            material.0 = handle;
        }
    }
    live
}

/// Gives every traceable source without a stand-in one, and hides the source's own draw.
fn spawn_proxies(
    commands: &mut Commands,
    assets: &mut StandInAssets,
    stand_ins: &mut StandIns,
    sources: &Query<SourceData, SourceFilter>,
    live: &HashSet<Entity>,
) {
    for (entity, source_mesh, global, inherited, scene, terrain, traced, hidden, layers) in sources
    {
        let traceable = inherited
            .get()
            .then(|| traceable(scene, terrain, &assets.scene, &assets.terrain))
            .flatten();
        let Some((id, base, solid)) = traceable else {
            if traced && let Some(hidden) = hidden {
                unhide(commands, entity, hidden);
            }
            continue;
        };
        if live.contains(&entity) {
            continue;
        }
        let Some(mesh) = stand_ins.mesh(&mut assets.meshes, &source_mesh.0) else {
            continue;
        };
        let material = stand_ins.material(&mut assets.standard, id, base, solid);
        commands.spawn((
            Proxy,
            StandInOf(entity),
            Mesh3d(mesh.clone()),
            RaytracingMesh3d(mesh),
            MeshMaterial3d(material),
            global.compute_transform(),
            *global,
        ));
        let mut source = commands.entity(entity);
        source.insert(Traced);
        if hidden.is_none() {
            source.insert((Hidden(layers.cloned()), RenderLayers::none()));
        }
    }
}

/// Clipped model draws are children of their source; they stop drawing with it.
#[allow(clippy::type_complexity)] // Bevy query filters.
pub(super) fn hide_draws(
    mut commands: Commands,
    draws: Query<(Entity, &ChildOf, Option<&RenderLayers>), (With<Draw>, Without<Hidden>)>,
    traced: Query<(), With<Traced>>,
) {
    for (draw, parent, layers) in &draws {
        if traced.contains(parent.parent()) {
            commands
                .entity(draw)
                .insert((Hidden(layers.cloned()), RenderLayers::none()));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_in_mesh_has_exactly_the_layout_solari_accepts() {
        let source = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0; 2]; 3])
        .with_inserted_indices(Indices::U16(vec![0, 1, 2]));
        let stand_in = standin_mesh(&source).expect("triangle list with normals");
        let attributes: Vec<_> = stand_in
            .attributes()
            .map(|(attribute, _)| attribute.id)
            .collect();
        assert_eq!(
            attributes,
            [
                Mesh::ATTRIBUTE_POSITION.id,
                Mesh::ATTRIBUTE_NORMAL.id,
                Mesh::ATTRIBUTE_UV_0.id,
                Mesh::ATTRIBUTE_TANGENT.id,
            ]
        );
        assert!(matches!(stand_in.indices(), Some(Indices::U32(indices)) if indices == &[0, 1, 2]));
    }

    #[test]
    fn meshes_without_normals_get_no_stand_in() {
        let source = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3]);
        assert!(standin_mesh(&source).is_none());
    }
}
