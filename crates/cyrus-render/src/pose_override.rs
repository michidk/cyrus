//! Owned deformation of a frozen vertex-frame mesh. Gameplay poses remain authoritative.
use crate::animation::MeshFrameBinding;
use bevy::camera::primitives::MeshAabb;
use bevy::gltf::GltfExtras;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use serde::Deserialize;

/// A source vertex's normalized weighted rigid transforms, indexed into [`PoseOverride::matrices`].
#[derive(Clone, Debug)]
pub struct VertexWeights(pub Vec<(usize, f32)>);

/// Exclusive presentation claim on a model node. Matrices map frozen model-local vertices to
/// current model-local coordinates. Removing this component restores the original mesh handle.
/// The owner must release on visibility/removal, revival, world replacement and revision changes.
#[derive(Component)]
pub struct PoseOverride {
    /// Unique claim incarnation; changing it freezes a new source frame.
    pub key: u64,
    pub matrices: Vec<Mat4>,
    pub weights: std::sync::Arc<[VertexWeights]>,
    /// Exact installed source frame to freeze, rather than whichever frame last rendered.
    pub source: Option<(Handle<cyrus_assets::animation::Animations>, String, usize)>,
}

#[derive(Component)]
pub(crate) struct FrozenPrimitive {
    key: u64,
    original: Handle<Mesh>,
    private: Handle<Mesh>,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    vertices: Vec<usize>,
}

#[derive(Deserialize)]
struct Extras {
    rgpre_geometry: MeshFrameBinding,
}

fn deform(point: Vec3, normal: Vec3, weights: &VertexWeights, matrices: &[Mat4]) -> (Vec3, Vec3) {
    let mut position = Vec3::ZERO;
    let mut direction = Vec3::ZERO;
    for &(body, weight) in &weights.0 {
        let matrix = matrices[body];
        position += matrix.transform_point3(point) * weight;
        direction += Mat3::from_mat4(matrix).inverse().transpose() * normal * weight;
    }
    (position, direction.normalize_or_zero())
}

#[allow(clippy::type_complexity)] // Primitive mesh, source extras and lifetime cache are independent ECS inputs.
pub(crate) fn apply(
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    overrides: Query<&PoseOverride>,
    animations: Option<Res<Assets<cyrus_assets::animation::Animations>>>,
    parents: Query<&ChildOf>,
    mut primitives: Query<(
        Entity,
        &ChildOf,
        &mut Mesh3d,
        Option<&GltfExtras>,
        Option<&FrozenPrimitive>,
    )>,
) {
    let Some(mut meshes) = meshes else {
        return;
    };
    for (entity, parent, mut mesh, extras, frozen) in &mut primitives {
        let claim = std::iter::once(parent.parent())
            .chain(parents.iter_ancestors(parent.parent()))
            .find_map(|owner| overrides.get(owner).ok())
            .filter(|claim| {
                !claim.matrices.is_empty()
                    && claim
                        .matrices
                        .iter()
                        .all(|m| m.is_finite() && m.determinant().abs() > 1e-8)
                    && claim.weights.iter().all(|weights| {
                        !weights.0.is_empty()
                            && weights.0.iter().all(|&(body, weight)| {
                                body < claim.matrices.len() && weight.is_finite() && weight >= 0.
                            })
                            && (weights.0.iter().map(|(_, w)| w).sum::<f32>() - 1.).abs() < 0.001
                    })
            });
        if let Some(old) = frozen.filter(|old| claim.is_some_and(|c| c.key != old.key)) {
            mesh.0 = old.original.clone();
            meshes.remove(old.private.id());
            commands.entity(entity).remove::<FrozenPrimitive>();
        }
        if let Some(frozen) = frozen.filter(|old| claim.is_none_or(|c| c.key == old.key)) {
            let Some(claim) = claim else {
                mesh.0 = frozen.original.clone();
                meshes.remove(frozen.private.id());
                commands.entity(entity).remove::<FrozenPrimitive>();
                if let Some(bounds) = meshes.get(&mesh.0).and_then(Mesh::get_aabb) {
                    commands.entity(entity).insert(bounds);
                }
                continue;
            };
            if let Some(mut private) = meshes.get_mut(&frozen.private) {
                let mut positions = Vec::with_capacity(frozen.positions.len());
                let mut normals = Vec::with_capacity(frozen.normals.len());
                for ((position, normal), &vertex) in frozen
                    .positions
                    .iter()
                    .zip(&frozen.normals)
                    .zip(&frozen.vertices)
                {
                    let (position, normal) = deform(
                        Vec3::from_array(*position),
                        Vec3::from_array(*normal),
                        &claim.weights[vertex],
                        &claim.matrices,
                    );
                    positions.push(position.to_array());
                    normals.push(normal.to_array());
                }
                private.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                private.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
                private.final_aabb = None;
                if let Some(bounds) = private.get_aabb() {
                    commands.entity(entity).insert(bounds);
                }
            }
            continue;
        }
        let Some(claim) = claim else {
            continue;
        };
        let Some(source) = meshes.get(&mesh.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            source.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            source.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            continue;
        };
        let Some(mapping) = extras
            .and_then(|extras| serde_json::from_str::<Extras>(&extras.value).ok())
            .map(|extras| extras.rgpre_geometry)
        else {
            continue;
        };
        if mapping.version != 1
            || mapping.source_vertices.len() != positions.len()
            || normals.len() != positions.len()
            || mapping
                .source_vertices
                .iter()
                .any(|v| v[0] as usize >= claim.weights.len())
        {
            continue;
        }
        let (positions, normals) = if let Some((handle, name, frame)) = &claim.source {
            let Some(model) = animations
                .as_deref()
                .and_then(|assets| assets.get(handle))
                .and_then(|data| data.models.get(name))
            else {
                continue;
            };
            let Ok(geometry) = crate::animation::FrameGeometry::from_model(model, *frame, &mapping)
            else {
                continue;
            };
            (geometry.positions, geometry.normals)
        } else {
            (positions.clone(), normals.clone())
        };
        let mut source = source.clone();
        let (deformed_positions, deformed_normals): (Vec<_>, Vec<_>) = positions
            .iter()
            .zip(&normals)
            .zip(&mapping.source_vertices)
            .map(|((p, n), v)| {
                let (p, n) = deform(
                    Vec3::from_array(*p),
                    Vec3::from_array(*n),
                    &claim.weights[v[0] as usize],
                    &claim.matrices,
                );
                (p.to_array(), n.to_array())
            })
            .unzip();
        source.insert_attribute(Mesh::ATTRIBUTE_POSITION, deformed_positions);
        source.insert_attribute(Mesh::ATTRIBUTE_NORMAL, deformed_normals);
        source.final_aabb = None;
        if let Some(bounds) = source.get_aabb() {
            commands.entity(entity).insert(bounds);
        }
        let private = meshes.add(source);
        let frozen = FrozenPrimitive {
            key: claim.key,
            original: mesh.0.clone(),
            private: private.clone(),
            positions,
            normals,
            vertices: mapping
                .source_vertices
                .into_iter()
                .map(|v| v[0] as usize)
                .collect(),
        };
        mesh.0 = private;
        commands.entity(entity).insert(frozen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jointed_vertex_weights_preserve_normals_and_move_limbs_independently() {
        let matrices = [
            Mat4::IDENTITY,
            Mat4::from_rotation_translation(
                Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                Vec3::Y,
            ),
        ];
        let root = deform(Vec3::X, Vec3::Y, &VertexWeights(vec![(0, 1.)]), &matrices);
        let limb = deform(Vec3::X, Vec3::Y, &VertexWeights(vec![(1, 1.)]), &matrices);
        assert_eq!(root.0, Vec3::X);
        assert!((limb.0 - Vec3::Y * 2.).length() < 1e-5);
        assert!((limb.1 + Vec3::X).length() < 1e-5);
        let blend = deform(
            Vec3::X,
            Vec3::Y,
            &VertexWeights(vec![(0, 0.5), (1, 0.5)]),
            &matrices,
        );
        assert!((blend.0 - Vec3::new(0.5, 1., 0.)).length() < 1e-5);
        assert!((blend.1.length() - 1.).abs() < 1e-5);
    }
}

#[cfg(test)]
mod restoration {
    use super::*;
    use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
    #[test]
    fn frozen_mesh_is_private_and_release_restores_source_and_reclaims_the_copy() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .add_systems(Update, apply);
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 3]);
        let source = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let owner = app
            .world_mut()
            .spawn(PoseOverride {
                key: 1,
                matrices: vec![Mat4::from_translation(Vec3::Y)],
                weights: vec![VertexWeights(vec![(0, 1.)]); 3].into(),
                source: None,
            })
            .id();
        let intermediate = app.world_mut().spawn(ChildOf(owner)).id();
        let primitive=app.world_mut().spawn((ChildOf(intermediate),Mesh3d(source.clone()),GltfExtras{
            value:serde_json::json!({"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}).to_string()
        })).id();
        app.update();
        let private = app.world().get::<Mesh3d>(primitive).unwrap().0.clone();
        assert_ne!(private.id(), source.id());
        let meshes = app.world().resource::<Assets<Mesh>>();
        assert_eq!(
            meshes
                .get(&source)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION),
            Some(&VertexAttributeValues::Float32x3(vec![
                [0., 0., 0.],
                [1., 0., 0.],
                [0., 1., 0.]
            ]))
        );
        assert_eq!(
            meshes
                .get(&private)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION),
            Some(&VertexAttributeValues::Float32x3(vec![
                [0., 1., 0.],
                [1., 1., 0.],
                [0., 2., 0.]
            ]))
        );
        app.world_mut().entity_mut(owner).remove::<PoseOverride>();
        app.update();
        assert_eq!(
            app.world().get::<Mesh3d>(primitive).unwrap().0.id(),
            source.id()
        );
        assert!(
            !app.world()
                .resource::<Assets<Mesh>>()
                .contains(private.id())
        );
        assert!(app.world().get::<FrozenPrimitive>(primitive).is_none());
    }
}
