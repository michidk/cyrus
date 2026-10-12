//! Mesh diagnostics use the same transformed geometry that the viewer displays.
use crate::{preview::ViewCamera, ui::Viewer};
use bevy::{
    mesh::{PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use cyrus_app::bevy_egui::egui;

pub struct State {
    pub vertex_normals: bool,
    pub face_normals: bool,
    pub bounds: bool,
    pub subobjects: bool,
    pub stride: usize,
    pub picking: bool,
    pub uv: bool,
    pub normal_length: f32,
    pub pending_pick: Option<Vec2>,
    selected: Option<Selection>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            vertex_normals: false,
            face_normals: false,
            bounds: false,
            subobjects: false,
            stride: 8,
            picking: false,
            uv: false,
            normal_length: 0.01,
            pending_pick: None,
            selected: None,
        }
    }
}
struct Selection {
    entity: Entity,
    triangle: usize,
    positions: [Vec3; 3],
    uvs: Option<[[f32; 2]; 3]>,
}
pub fn controls(ui: &mut egui::Ui, state: &mut State, metadata: Option<&serde_json::Value>) {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut state.vertex_normals, "Vertex normals");
        ui.checkbox(&mut state.face_normals, "Triangle normals");
        ui.checkbox(&mut state.bounds, "Mesh bounds");
        if metadata.is_some_and(|m| {
            m["scene_subobjects"]
                .as_array()
                .is_some_and(|s| !s.is_empty())
        }) {
            ui.checkbox(&mut state.subobjects, "Authored subobjects");
        }
        ui.checkbox(&mut state.picking, "Pick faces");
        ui.checkbox(&mut state.uv, "UV layout");
        if state.vertex_normals || state.face_normals {
            ui.add(egui::Slider::new(&mut state.stride, 1..=128).text("Normal spacing"));
            ui.add(
                egui::Slider::new(&mut state.normal_length, 0.001..=0.1)
                    .logarithmic(true)
                    .text("Normal length / view"),
            );
        }
    });
    if state.picking {
        ui.weak("Click a face to inspect its rendered triangle and texture coordinates.");
    }
    if let Some(selected) = &state.selected {
        ui.label(format!(
            "Selected triangle {} · vertices {:?}",
            selected.triangle,
            selected.positions.map(|p| p.to_array())
        ));
        if let Some(uvs) = selected.uvs {
            ui.label(format!("Normalized UVs: {uvs:?}"));
        }
        if ui.small_button("Clear face selection").clicked() {
            state.selected = None;
        }
    }
}

/// Draw normals and accept a closest-hit ray against visible triangle meshes.
pub fn draw(
    mut viewer: ResMut<Viewer>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewCamera>>,
    sources: Query<(Entity, &Mesh3d, &GlobalTransform, &InheritedVisibility)>,
    meshes: Res<Assets<Mesh>>,
    windows: Query<&Window>,
    mut gizmos: Gizmos,
) {
    if !viewer
        .preview
        .as_ref()
        .is_some_and(|p| viewer.shows_3d(&p.content))
    {
        return;
    }
    let length = viewer.distance * viewer.mesh_inspection.normal_length.max(0.001);
    if viewer.mesh_inspection.subobjects
        && let Some(subobjects) = viewer
            .preview
            .as_ref()
            .and_then(|p| p.metadata.as_ref())
            .and_then(|m| m["scene_subobjects"].as_array())
    {
        for subobject in subobjects {
            if let (Ok(center), Some(radius)) = (
                serde_json::from_value::<[f32; 3]>(subobject["center"].clone()),
                subobject["radius"].as_f64(),
            ) {
                gizmos.sphere(
                    Isometry3d::from_translation(Vec3::from(center)),
                    radius as f32,
                    Color::srgb(0.8, 0.4, 1.),
                );
            }
        }
    }
    let state = &mut viewer.mesh_inspection;
    let ray = state.pending_pick.take().and_then(|point| {
        cameras.single().ok().and_then(|(camera, transform)| {
            let scale = windows.single().ok()?.scale_factor();
            camera.viewport_to_world(transform, point / scale).ok()
        })
    });
    let mut nearest = f32::INFINITY;
    if ray.is_some() {
        state.selected = None;
    }
    for (entity, handle, transform, visible) in &sources {
        if !visible.get() {
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
        let point = |i: usize| {
            positions
                .get(i)
                .map(|p| transform.transform_point(Vec3::from(*p)))
        };
        if state.bounds {
            let (min, max) = positions
                .iter()
                .map(|p| transform.transform_point(Vec3::from(*p)))
                .fold(
                    (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                    |(min, max), p| (min.min(p), max.max(p)),
                );
            if min.is_finite() && max.is_finite() {
                gizmos.cube(
                    Transform::from_translation((min + max) * 0.5).with_scale(max - min),
                    Color::srgb(1., 0.7, 0.2),
                );
            }
        }
        if state.vertex_normals
            && let Some(VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        {
            let normal_matrix = transform.affine().matrix3.inverse().transpose();
            for (p, n) in positions.iter().zip(normals).step_by(state.stride.max(1)) {
                let p = transform.transform_point(Vec3::from(*p));
                let n = (normal_matrix * Vec3::from(*n)).normalize_or_zero();
                if n.is_finite() {
                    gizmos.line(p, p + n * length, Color::srgb(0.2, 0.9, 0.8));
                }
            }
        }
        if !state.face_normals && ray.is_none() {
            continue;
        }
        let count = mesh.indices().map_or(positions.len(), |i| i.len());
        for triangle in 0..count / 3 {
            let ids = std::array::from_fn::<_, 3, _>(|corner| {
                let index = triangle * 3 + corner;
                match mesh.indices() {
                    Some(bevy::mesh::Indices::U16(values)) => values[index] as usize,
                    Some(bevy::mesh::Indices::U32(values)) => values[index] as usize,
                    None => index,
                }
            });
            let [Some(a), Some(b), Some(c)] = ids.map(point) else {
                continue;
            };
            if state.face_normals && triangle % state.stride.max(1) == 0 {
                let center = (a + b + c) / 3.;
                let normal = (b - a).cross(c - a).normalize_or_zero();
                gizmos.line(center, center + normal * length, Color::srgb(1., 0.5, 0.3));
            }
            if let Some(ray) = &ray
                && let Some(distance) = hit(ray.origin, *ray.direction, [a, b, c])
                && distance < nearest
            {
                nearest = distance;
                let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                    Some(VertexAttributeValues::Float32x2(uvs)) => {
                        match ids.map(|i| uvs.get(i).copied()) {
                            [Some(a), Some(b), Some(c)] => Some([a, b, c]),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                state.selected = Some(Selection {
                    entity,
                    triangle,
                    positions: [a, b, c],
                    uvs,
                });
            }
        }
    }
    if let Some(selected) = &state.selected {
        if sources
            .get(selected.entity)
            .is_ok_and(|(_, _, _, v)| v.get())
        {
            let [a, b, c] = selected.positions;
            gizmos.linestrip([a, b, c, a], Color::srgb(1., 0.9, 0.1));
        } else {
            state.selected = None;
        }
    }
}
fn hit(origin: Vec3, direction: Vec3, [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let ab = b - a;
    let ac = c - a;
    let p = direction.cross(ac);
    let determinant = ab.dot(p);
    if determinant.abs() < 1e-7 {
        return None;
    }
    let t = origin - a;
    let u = t.dot(p) / determinant;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = t.cross(ab);
    let v = direction.dot(q) / determinant;
    if v < 0. || u + v > 1. {
        return None;
    }
    let distance = ac.dot(q) / determinant;
    (distance >= 0.).then_some(distance)
}
/// Show normalized UV coordinates from the selected mesh without inventing texture dimensions.
pub fn uv_window(
    mut contexts: cyrus_app::bevy_egui::EguiContexts,
    viewer: Res<Viewer>,
    sources: Query<(Entity, &Mesh3d, &InheritedVisibility)>,
    meshes: Res<Assets<Mesh>>,
) -> Result {
    if !viewer.mesh_inspection.uv {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    egui::Window::new("UV layout")
        .default_size([420., 420.])
        .show(ctx, |ui| {
            ui.label("Normalized texture coordinates. Pick a face to isolate its material mesh.");
            let Some(selected) = viewer.mesh_inspection.selected.as_ref() else {
                ui.label("Pick a face in the scene to show its mesh UVs.");
                return;
            };
            let (rect, _) = ui.allocate_exact_size(
                ui.available_size().max(egui::vec2(100., 100.)),
                egui::Sense::hover(),
            );
            let mut triangles = Vec::new();
            for (entity, handle, visible) in &sources {
                if selected.entity != entity {
                    continue;
                }
                if !visible.get() {
                    continue;
                }
                let Some(mesh) = meshes.get(handle) else {
                    continue;
                };
                if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
                    continue;
                }
                let Some(VertexAttributeValues::Float32x2(uvs)) =
                    mesh.attribute(Mesh::ATTRIBUTE_UV_0)
                else {
                    continue;
                };
                let ids: Vec<usize> = mesh
                    .indices()
                    .map_or_else(|| (0..uvs.len()).collect(), |i| i.iter().collect());
                for ids in ids.as_chunks::<3>().0 {
                    if let [Some(a), Some(b), Some(c)] = ids.map(|i| uvs.get(i).copied()) {
                        triangles.push([a, b, c]);
                    }
                }
            }
            let (min, max) = triangles
                .iter()
                .flatten()
                .fold((Vec2::ZERO, Vec2::ONE), |(min, max), p| {
                    (min.min(Vec2::from(*p)), max.max(Vec2::from(*p)))
                });
            let extent = (max - min).max(Vec2::splat(0.001));
            let project = |p: [f32; 2]| {
                let p = (Vec2::from(p) - min) / extent;
                rect.min + egui::vec2(p.x * rect.width(), p.y * rect.height())
            };
            for triangle in triangles {
                let p = triangle.map(project);
                for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                    ui.painter().line_segment(
                        [p[a], p[b]],
                        egui::Stroke::new(0.6, egui::Color32::from_rgb(70, 140, 150)),
                    );
                }
            }
            if let Some(uvs) = selected.uvs {
                let p = uvs.map(project);
                for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                    ui.painter()
                        .line_segment([p[a], p[b]], egui::Stroke::new(2., egui::Color32::YELLOW));
                }
            }
        });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picking_rejects_misses_and_hits_both_sides() {
        let triangle = [Vec3::ZERO, Vec3::X, Vec3::Y];
        assert_eq!(
            hit(Vec3::new(0.2, 0.2, 2.), Vec3::NEG_Z, triangle),
            Some(2.)
        );
        assert_eq!(hit(Vec3::new(0.2, 0.2, -2.), Vec3::Z, triangle), Some(2.));
        assert_eq!(hit(Vec3::new(2., 2., 2.), Vec3::NEG_Z, triangle), None);
        assert_eq!(hit(Vec3::new(0.2, 0.2, 2.), Vec3::Z, triangle), None);
    }
}
