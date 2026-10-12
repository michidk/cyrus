//! Selected PVO cells highlight the statically mapped placed objects.
use crate::ui::Viewer;
use bevy::{
    mesh::{PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};

pub fn draw(
    viewer: Res<Viewer>,
    sources: Query<(Entity, &Mesh3d, &GlobalTransform, &InheritedVisibility)>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    meshes: Res<Assets<Mesh>>,
    mut gizmos: Gizmos,
) {
    let Some(preview) = &viewer.preview else {
        return;
    };
    if !viewer.shows_3d(&preview.content) {
        return;
    }
    let cyrus_assets::Content::Spatial { cells, value, .. } = &preview.content else {
        return;
    };
    let Some(cell) = viewer.selected_cell.and_then(|i| cells.get(i)) else {
        return;
    };
    gizmos.cube(
        Transform::from_translation(Vec3::from(cell.center))
            .with_scale(Vec3::splat(cell.half_extent * 2.)),
        Color::srgb(1., 0.3, 0.7),
    );
    let objects = value["static_objects"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let selected: std::collections::HashSet<String> = cell
        .visibility_ids
        .iter()
        .filter_map(|id| {
            let object = objects.get(usize::from(*id))?;
            Some(format!("S{id:03}_{}", object["model_name"].as_str()?))
        })
        .collect();
    for (entity, handle, transform, visible) in &sources {
        if !visible.get() {
            continue;
        }
        let selected_object = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|e| {
                names
                    .get(e)
                    .is_ok_and(|name| selected.contains(name.as_str()))
            });
        if !selected_object {
            continue;
        }
        let Some(mesh) = meshes.get(handle) else {
            continue;
        };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            continue;
        }
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let indices: Vec<usize> = mesh
            .indices()
            .map_or_else(|| (0..positions.len()).collect(), |i| i.iter().collect());
        for ids in indices.as_chunks::<3>().0 {
            let [Some(a), Some(b), Some(c)] = ids.map(|i| {
                positions
                    .get(i)
                    .map(|p| transform.transform_point(Vec3::from(*p)))
            }) else {
                continue;
            };
            gizmos.linestrip([a, b, c, a], Color::srgb(1., 0.8, 0.2));
        }
    }
}
