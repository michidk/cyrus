//! Camera-facing MPSF geometry from preservation's placed-flat export contract.
use anyhow::ensure;
use bevy::{gltf::GltfExtras, mesh::VertexAttributeValues, prelude::*};
use serde::Deserialize;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PrepareFlats;

#[derive(Component)]
pub(crate) struct Flat {
    pub(crate) brightness: Option<u8>,
    pub(crate) alpha: u16,
    pub(crate) source_index: usize,
    pub(crate) radius: f32,
}

#[derive(Component)]
struct Inspected;

#[derive(Deserialize)]
struct Description {
    version: u32,
    dimensions: Option<[u32; 2]>,
    texture_scale: Option<f32>,
    flags: u8,
    alpha: u16,
}

impl Description {
    fn mesh(&self, source: &Mesh) -> anyhow::Result<Mesh> {
        ensure!(
            self.version == 1,
            "unsupported flat metadata version {}",
            self.version
        );
        ensure!(
            matches!(self.flags, 1 | 3),
            "unsupported flat geometry flags {}",
            self.flags
        );
        let [width, height] = self
            .dimensions
            .ok_or_else(|| anyhow::anyhow!("missing flat image dimensions"))?;
        ensure!(
            width <= 64 && height <= 64,
            "flat texture reduction is not yet supported"
        );
        let scale = self
            .texture_scale
            .ok_or_else(|| anyhow::anyhow!("missing flat texture scale"))?;
        ensure!(
            width > 0 && height > 0 && scale.is_finite() && scale > 0.0,
            "invalid flat image dimensions or scale"
        );
        let Some(VertexAttributeValues::Float32x2(uv)) = source.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            anyhow::bail!("flat mesh lacks two-component texture coordinates");
        };
        ensure!(
            !uv.is_empty() && uv.iter().flatten().all(|v| v.is_finite()),
            "invalid flat texture coordinates"
        );
        let low = uv
            .iter()
            .fold(Vec2::splat(f32::INFINITY), |a, &b| a.min(Vec2::from(b)));
        let high = uv
            .iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), |a, &b| a.max(Vec2::from(b)));
        ensure!(
            (high - low).min_element() > 0.0,
            "flat texture coordinates have no extent"
        );
        // The generic exported quad supplies logical corners. Its model UV scaling
        // does not describe sprite coordinates; the full source image fills the flat.
        let corners = uv
            .iter()
            .map(|&value| (Vec2::from(value) - low) / (high - low))
            .collect::<Vec<_>>();
        ensure!(
            corners
                .iter()
                .all(|p| [p.x, p.y].into_iter().all(|v| v == 0.0 || v == 1.0)),
            "flat mesh has non-corner texture coordinates"
        );
        let size = Vec2::new(width as f32, height as f32) * (scale / 20.0);
        let mut mesh = source.clone();
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            corners
                .iter()
                .map(|&uv| ((uv - Vec2::splat(0.5)) * size).extend(0.0).to_array())
                .collect::<Vec<_>>(),
        );
        // Camera-facing local +Z maps to the original generated normal (0, 0, -1).
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; corners.len()]);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            corners.iter().map(|uv| uv.to_array()).collect::<Vec<_>>(),
        );
        Ok(mesh)
    }
}

// Source-image diagonal, quantized before scaling, from preservation flat admission.
fn bound_radius(width: u32, height: u32, scale: f32) -> f32 {
    let width = width as f32;
    let height = height as f32;
    let half_diagonal =
        crate::lighting::quantized_distance(width * width + height * height).0 * 0.5;
    half_diagonal * scale
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, prepare.in_set(PrepareFlats))
        .add_systems(
            PostUpdate,
            face_camera
                .after(TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
}

fn prepare(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut candidates: Query<(Entity, &mut Mesh3d), Without<Inspected>>,
    parents: Query<&ChildOf>,
    extras: Query<&GltfExtras>,
    names: Query<&Name>,
) {
    for (entity, mut mesh) in &mut candidates {
        let Some(source) = meshes.get(&mesh.0) else {
            continue;
        };
        let result = (|| -> anyhow::Result<Option<(Mesh, Flat)>> {
            for ancestor in std::iter::once(entity).chain(parents.iter_ancestors(entity)) {
                let Ok(extras) = extras.get(ancestor) else {
                    continue;
                };
                #[derive(Deserialize)]
                struct Extras {
                    rgpre_flat: Option<Description>,
                }
                if let Some(flat) = serde_json::from_str::<Extras>(&extras.value)?.rgpre_flat {
                    let source_index = names
                        .get(ancestor)
                        .ok()
                        .and_then(|name| name.as_str().strip_prefix('F'))
                        .and_then(|name| name.split_once('_'))
                        .and_then(|(index, _)| index.parse().ok())
                        .ok_or_else(|| {
                            anyhow::anyhow!("flat lacks its exported placement index")
                        })?;
                    let mesh = flat.mesh(source)?;
                    let [width, height] = flat
                        .dimensions
                        .ok_or_else(|| anyhow::anyhow!("missing flat dimensions"))?;
                    let scale = flat
                        .texture_scale
                        .ok_or_else(|| anyhow::anyhow!("missing flat scale"))?;
                    return Ok(Some((
                        mesh,
                        Flat {
                            brightness: (flat.alpha < 254).then_some(255),
                            alpha: flat.alpha,
                            source_index,
                            radius: bound_radius(width, height, scale),
                        },
                    )));
                }
            }
            Ok(None)
        })();
        commands.entity(entity).insert(Inspected);
        match result {
            Ok(Some((private, flat))) => {
                mesh.0 = meshes.add(private);
                commands
                    .entity(entity)
                    .insert(flat)
                    .remove::<bevy::camera::primitives::Aabb>();
            }
            Ok(None) => {}
            Err(error) => error!(
                "flat mesh {:?} {entity:?}: {error:#}; re-export the world",
                mesh.0.path()
            ),
        }
    }
}

fn face_camera(
    camera: Query<&GlobalTransform, (With<crate::camera::ViewCamera>, Without<Flat>)>,
    parents: Query<&GlobalTransform, Without<Flat>>,
    mut flats: Query<(&ChildOf, &mut Transform, &mut GlobalTransform), With<Flat>>,
) {
    let Ok(camera) = camera.single() else { return };
    for (parent, mut local, mut global) in &mut flats {
        let Ok(parent) = parents.get(parent.parent()) else {
            continue;
        };
        local.rotation = parent.rotation().inverse() * camera.rotation();
        *global = parent.mul_transform(*local);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_bound_retains_original_square_root_quantization() {
        assert_eq!(bound_radius(9, 26, 0.5), 6.84375);
        assert_eq!(bound_radius(4, 8, 1.0), 4.46875);
        assert_ne!(bound_radius(9, 26, 0.5), (757.0_f32).sqrt() * 0.25);
    }

    #[test]
    fn billboard_tracks_camera_rotation_without_moving_its_placed_center() {
        let mut app = App::new();
        app.add_plugins(TransformPlugin)
            .add_systems(PostUpdate, face_camera.after(TransformSystems::Propagate));
        let parent_pose =
            Transform::from_xyz(3.0, 4.0, 5.0).with_rotation(Quat::from_rotation_y(0.5));
        let parent = app.world_mut().spawn(parent_pose).id();
        let flat = app
            .world_mut()
            .spawn((
                Flat {
                    brightness: Some(255),
                    alpha: 181,
                    source_index: 0,
                    radius: 1.0,
                },
                ChildOf(parent),
                Transform::IDENTITY,
            ))
            .id();
        let camera_pose =
            Transform::from_xyz(7.0, 8.0, 9.0).with_rotation(Quat::from_rotation_x(0.25));
        let camera = app
            .world_mut()
            .spawn((crate::camera::ViewCamera, camera_pose))
            .id();
        app.update();
        let actual = app.world().get::<GlobalTransform>(flat).unwrap();
        assert_eq!(actual.translation(), parent_pose.translation);
        assert!(
            actual
                .rotation()
                .abs_diff_eq(camera_pose.rotation, 0.000001)
        );
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .rotation = Quat::from_rotation_z(-0.75);
        app.update();
        let actual = app.world().get::<GlobalTransform>(flat).unwrap();
        assert_eq!(actual.translation(), parent_pose.translation);
        assert!(
            actual
                .rotation()
                .abs_diff_eq(Quat::from_rotation_z(-0.75), 0.000001)
        );
    }

    #[test]
    fn source_dimensions_replace_fixed_quad_and_repeated_uvs_without_mutating_source() {
        let mut source = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        source.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; 4]);
        source.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.0, 1.0], [2.0, 1.0], [2.0, 0.5], [0.0, 0.5]],
        );
        let description = Description {
            version: 1,
            dimensions: Some([4, 8]),
            texture_scale: Some(0.5),
            flags: 1,
            alpha: 181,
        };
        let mesh = description.mesh(&source).unwrap();
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap(),
            &[
                [-0.05, 0.1, 0.0],
                [0.05, 0.1, 0.0],
                [0.05, -0.1, 0.0],
                [-0.05, -0.1, 0.0]
            ]
        );
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
                .unwrap()
                .as_float3()
                .unwrap(),
            &[[0.0, 0.0, 1.0]; 4]
        );
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("missing UVs")
        };
        assert_eq!(uv, &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        assert_eq!(
            source
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap(),
            &[[0.0; 3]; 4]
        );
    }
}
