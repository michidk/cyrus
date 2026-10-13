//! RGM inspection of authored placements, navigation and actor command records.
use bevy::prelude::*;
use cyrus_app::bevy_egui::{EguiContexts, egui};
use cyrus_assets::Content;
use serde_json::Value;

use crate::{preview::ViewCamera, ui::Viewer};

#[derive(Default)]
pub struct State {
    open: bool,
    fields: std::collections::HashMap<egui::Id, String>,
    markers: bool,
    lights: bool,
    objects: bool,
    selected: Option<(&'static str, usize)>,
    actor: usize,
    group: usize,
    node: Option<(usize, usize)>,
}

fn records<'a>(data: &'a Value, key: &str) -> &'a [Value] {
    data[key].as_array().map(Vec::as_slice).unwrap_or_default()
}

fn vector(value: &Value) -> Option<Vec3> {
    let v = value.as_array()?;
    if v.len() != 3 {
        return None;
    }
    let result = Vec3::new(
        v[0].as_f64()? as f32,
        v[1].as_f64()? as f32,
        v[2].as_f64()? as f32,
    );
    result.is_finite().then_some(result)
}

fn label(key: &str, index: usize, record: &Value) -> String {
    let name = record["script_name"].as_str().filter(|s| !s.is_empty());
    format!(
        "{key} {index}{}",
        name.map(|s| format!(" · {s}")).unwrap_or_default()
    )
}

/// Draw the inspector; return a requested camera center and orbit distance.
pub fn toolbar(
    ui: &mut egui::Ui,
    state: &mut State,
    data: &Value,
    current_distance: f32,
) -> Option<(Vec3, f32)> {
    data.get("walk_node_maps")?;
    let mut focus = None;
    ui.toggle_value(&mut state.open, "World inspection");
    let mut open = state.open;
    egui::Window::new("World inspection").open(&mut open).default_width(440.).default_height(420.).vscroll(true).show(ui.ctx(), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut state.markers, "Markers");
            ui.checkbox(&mut state.lights, "Lights");
            ui.checkbox(&mut state.objects, "Objects");
        });
        ui.weak("Lights show authored positions and signed RGB strengths.");
        egui::ComboBox::from_id_salt("world-record")
            .selected_text(state.selected.map(|(key, i)| label(key, i, &data[key][i])).unwrap_or_else(|| "Select a placement".into()))
            .show_ui(ui, |ui| {
                for key in ["markers", "lights", "mpob_objects"] {
                    for (index, record) in records(data, key).iter().enumerate() {
                        if ui.selectable_value(&mut state.selected, Some((key, index)), label(key, index, record)).changed() { state.fields.clear(); }
                    }
                }
            });
        if let Some((key, index)) = state.selected {
            let record = &data[key][index];
            if ui.button("Focus selected").clicked() {
                focus = focus_record(record, data, current_distance);
            }
            if key == "lights" { ui.monospace(format!("Signed RGB {} · radius {}", record["rgb"], record["radius"])); }
            ui.collapsing("Selected fields", |ui| { fields(ui, &mut state.fields, record); });
        }
        ui.collapsing("Navigation maps", |ui| {
            for (index, map) in records(data, "walk_node_maps").iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.collapsing(format!("Map {index} · {} nodes", records(map, "walk_nodes").len()), |ui| {
                        if ui.button("Focus map").clicked() { focus = focus_record(map, data, current_distance); }
                        navigation_diagram(ui, state, index, map);
                        ui.collapsing("Node records and route costs", |ui| fields(ui, &mut state.fields, &map["walk_nodes"]));
                    });
                });
            }
        });
        ui.collapsing("Actor relationships and animation commands", |ui| {
            let actors = records(data, "actors");
            if actors.is_empty() { ui.weak("No actor definitions."); return; }
            state.actor = state.actor.min(actors.len() - 1);
            egui::ComboBox::from_id_salt("world-actor").selected_text(label("Actor", state.actor, &actors[state.actor])).show_ui(ui, |ui| {
                for (index, actor) in actors.iter().enumerate() {
                    if ui.selectable_value(&mut state.actor, index, label("Actor", index, actor)).changed() { state.group = 0; state.fields.clear(); }
                }
            });
            let actor = &actors[state.actor];
            let script = actor["script_name"].as_str();
            ui.label(format!("Actor {} → script {}", state.actor, script.unwrap_or("unknown")));
            for (index, object) in records(data, "mpob_objects").iter().enumerate().filter(|(_, o)| script.zip(o["script_name"].as_str()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))) {
                if ui.button(format!("Placement {index} → {}", object["model_name"].as_str().unwrap_or("no model"))).clicked() {
                    state.selected = Some(("mpob_objects", index)); state.fields.clear(); focus = focus_record(object, data, current_distance);
                }
            }
            for key in ["raan_entries", "rahk_hooks", "attributes", "raex"] {
                if !actor[key].is_null() { ui.collapsing(key, |ui| fields(ui, &mut state.fields, &actor[key])); }
            }
            for key in ["mpsz_bounds_primary", "mpsz_bounds_0", "mpsz_bounds_1"] {
                if let Some(index) = actor[key].as_u64().and_then(|n| usize::try_from(n).ok()) {
                    ui.collapsing(format!("{key} → bounds {index}"), |ui| {
                        if let Some(bounds) = records(data, "mpsz_entries").get(index) { bounds_diagram(ui, bounds); fields(ui, &mut state.fields, bounds); }
                        else { ui.label("Referenced bounds record unavailable."); }
                    });
                }
            }
            if let Some(volumes) = actor["collision_volumes"].as_array() {
                ui.collapsing(format!("Vertex collision spheres · {}", volumes.len()), |ui| {
                    ui.weak("Radius comparison in source units; positions on this chart are not world positions.");
                    let largest = volumes.iter().filter_map(|v| v["radius"].as_f64()).fold(1., f64::max);
                    for volume in volumes {
                        let radius = volume["radius"].as_f64().unwrap_or(0.);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 60.), egui::Sense::hover());
                        ui.painter().circle_stroke(egui::pos2(rect.left() + 30., rect.center().y), (radius / largest) as f32 * 26., egui::Stroke::new(1., egui::Color32::LIGHT_GREEN));
                        ui.painter().text(egui::pos2(rect.left() + 65., rect.center().y), egui::Align2::LEFT_CENTER, format!("Vertex {} · radius {} · offset [{}, {}, {}]", volume["vertex"], volume["radius"], volume["offset_x"], volume["offset_y"], volume["offset_z"]), egui::FontId::monospace(11.), egui::Color32::WHITE);
                    }
                });
            }
            let groups = records(actor, "animation_groups");
            if !groups.is_empty() {
                state.group = state.group.min(groups.len() - 1);
                egui::ComboBox::from_id_salt("world-animation-group").selected_text(format!("Group {}", groups[state.group]["group_index"])).show_ui(ui, |ui| {
                    for (index, group) in groups.iter().enumerate() { ui.selectable_value(&mut state.group, index, format!("Group {} · animation {}", group["group_index"], group["anim_id"])); }
                });
                ui.weak("Authored command order; scripts are not executed and durations are not inferred.");
                egui::ScrollArea::vertical().id_salt("world-commands").max_height(180.).show(ui, |ui| {
                    for (index, command) in records(&groups[state.group], "commands").iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!("{index:04}"));
                            ui.strong(command["name"].as_str().unwrap_or("Unknown"));
                            if let Some(reference) = command["frame_reference"].as_u64() {
                                match frame_target(actor, reference) {
                                    Some((model, frame)) => { ui.label(format!("→ {model} frame {frame}")); }
                                    None => { ui.colored_label(egui::Color32::YELLOW, "Unresolved frame reference"); }
                                }
                            }
                            if let Some(target) = command["target_group"].as_i64()
                                && let Some(index) = groups.iter().position(|g| g["group_index"].as_i64() == Some(target))
                                && ui.small_button(format!("Go to group {target}")).clicked()
                            { state.group = index; }
                            if let Some(object) = command.as_object() {
                                for (key, value) in object { if !matches!(key.as_str(), "name" | "opcode") { ui.label(format!("{key}: {value}")); } }
                            }
                        });
                    }
                });
            }
        });
        ui.collapsing("Collision sphere records", |ui| {
            ui.weak("Vertex attachments are shown as authored records; no world pose is inferred.");
            fields(ui, &mut state.fields, &data["collision_volumes"]);
        });
    });
    state.open = open;
    focus
}

fn position(record: &Value) -> Option<Vec3> {
    if let Some(engine) = record.get("engine_position") {
        let coordinates = engine.as_array()?;
        if coordinates.len() != 3 {
            return None;
        }
        let x = coordinates[0].as_f64()?;
        let y = coordinates[1].as_f64()?;
        let z = coordinates[2].as_f64()?;
        let position = Vec3::new(
            -((x * 256.0) as f32) / 5120.0,
            -((y * 256.0) as f32) / 5120.0,
            -((16_777_215.0 - z * 256.0) as f32) / 5120.0,
        );
        return position.is_finite().then_some(position);
    }
    vector(&record["position"])
}

// Preservation's animation rules concatenate RAAN frame ranges in entry order.
fn frame_target(actor: &Value, mut reference: u64) -> Option<(&str, u64)> {
    for entry in records(actor, "raan_entries") {
        let count = entry["frame_count"].as_u64()?;
        if reference < count {
            return Some((entry["file_path"].as_str()?, reference));
        }
        reference -= count;
    }
    None
}

fn fields(
    ui: &mut egui::Ui,
    cache: &mut std::collections::HashMap<egui::Id, String>,
    value: &Value,
) {
    let text = cache
        .entry(ui.id())
        .or_insert_with(|| serde_json::to_string_pretty(value).unwrap_or_default());
    egui::ScrollArea::vertical()
        .max_height(180.)
        .show(ui, |ui| {
            ui.monospace(text.as_str());
        });
}

fn focus_record(record: &Value, data: &Value, current_distance: f32) -> Option<(Vec3, f32)> {
    let center = position(record)?;
    let nearest = ["markers", "lights", "mpob_objects", "walk_node_maps"]
        .into_iter()
        .flat_map(|key| records(data, key))
        .filter_map(position)
        .map(|position| position.distance(center))
        .filter(|distance| distance.is_finite() && *distance > 0.)
        .reduce(f32::min);
    // Inspection framing uses decoded neighbors, never raw source-unit radii.
    let distance = nearest
        .map(|distance| distance * 1.5)
        .unwrap_or(current_distance * 0.1);
    Some((center, distance.max(0.1)))
}

fn bounds_diagram(ui: &mut egui::Ui, bounds: &Value) {
    let (Some(negative), Some(positive), Some(center)) = (
        vector(&bounds["neg_extent"]),
        vector(&bounds["pos_extent"]),
        vector(&bounds["center_offset"]),
    ) else {
        return;
    };
    ui.weak("Authored local bounds · source coordinates · dot marks the local origin");
    for (axis, title) in [(1, "X/Y: +X right, +Y up"), (2, "X/Z: +X right, +Z up")] {
        ui.label(title);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 100.), egui::Sense::hover());
        let minimum = center - negative;
        let maximum = center + positive;
        let left = minimum.x.min(0.);
        let right = maximum.x.max(0.);
        let bottom = minimum[axis].min(0.);
        let top = maximum[axis].max(0.);
        let scale = (rect.width() / (right - left).max(1.))
            .min(rect.height() / (top - bottom).max(1.))
            * 0.85;
        let origin =
            rect.center() + egui::vec2(-(left + right) * 0.5, (top + bottom) * 0.5) * scale;
        let a = origin + egui::vec2(minimum.x, -maximum[axis]) * scale;
        let b = origin + egui::vec2(maximum.x, -minimum[axis]) * scale;
        ui.painter().rect_stroke(
            egui::Rect::from_two_pos(a, b),
            0.,
            egui::Stroke::new(1., egui::Color32::LIGHT_GREEN),
            egui::StrokeKind::Inside,
        );
        ui.painter().circle_filled(origin, 3., egui::Color32::WHITE);
    }
}

fn navigation_diagram(ui: &mut egui::Ui, state: &mut State, map_index: usize, map: &Value) {
    let nodes = records(map, "walk_nodes");
    let point = |node: &Value| -> Option<egui::Pos2> {
        Some(egui::pos2(
            node["node_pos_x"].as_f64()? as f32,
            node["node_pos_z"].as_f64()? as f32,
        ))
    };
    let mut bounds = egui::Rect::NOTHING;
    for p in nodes.iter().filter_map(point) {
        bounds.extend_with(p);
    }
    if !bounds.is_finite() {
        return;
    }
    ui.weak("Local X/Z node coordinates · arrows show route direction");
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 180.), egui::Sense::hover());
    let scale =
        (rect.width() / bounds.width().max(1.)).min(rect.height() / bounds.height().max(1.)) * 0.85;
    let project = |p: egui::Pos2| rect.center() + (p - bounds.center()) * scale;
    for (index, node) in nodes.iter().enumerate() {
        let Some(start) = point(node).map(project) else {
            continue;
        };
        for route in records(node, "routes") {
            if let Some(end) = route["target_node_id"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| nodes.get(n))
                .and_then(point)
                .map(project)
            {
                ui.painter().arrow(
                    start,
                    end - start,
                    egui::Stroke::new(1., egui::Color32::DARK_GREEN),
                );
            }
        }
        let response = ui.interact(
            egui::Rect::from_center_size(start, egui::vec2(12., 12.)),
            ui.id().with(("node", index)),
            egui::Sense::click(),
        );
        if response.clicked() {
            state.node = Some((map_index, index));
            state.fields.clear();
        }
        response.on_hover_ui(|ui| {
            ui.label(format!("Node {index}"));
            ui.monospace(serde_json::to_string_pretty(node).unwrap_or_default());
        });
        let selected = state.node == Some((map_index, index));
        ui.painter().circle_filled(
            start,
            if selected { 5. } else { 3. },
            if selected {
                egui::Color32::YELLOW
            } else {
                egui::Color32::LIGHT_GREEN
            },
        );
        ui.painter().text(
            start,
            egui::Align2::LEFT_BOTTOM,
            index.to_string(),
            egui::FontId::monospace(10.),
            egui::Color32::WHITE,
        );
    }
    if let Some((selected_map, index)) = state.node
        && selected_map == map_index
        && let Some(node) = nodes.get(index)
    {
        ui.label(format!("Selected node {index}"));
        fields(ui, &mut state.fields, node);
    }
}

/// Draw decoded world positions and labels in the same camera as exported geometry.
pub fn draw(
    viewer: Res<Viewer>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewCamera>>,
    mut gizmos: Gizmos,
    mut contexts: EguiContexts,
) {
    let Some(preview) = &viewer.preview else {
        return;
    };
    if !viewer.shows_3d(&preview.content) {
        return;
    }
    let Content::Model {
        data: Some(data), ..
    } = &preview.content
    else {
        return;
    };
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("world-labels"),
        ))
        .with_clip_rect(viewer.viewport);
    let size = (viewer.distance * 0.008).max(0.01);
    for (key, enabled, color) in [
        (
            "markers",
            viewer.world_inspection.markers,
            Color::srgb(0.3, 1., 0.4),
        ),
        (
            "lights",
            viewer.world_inspection.lights,
            Color::srgb(1., 0.9, 0.2),
        ),
        (
            "mpob_objects",
            viewer.world_inspection.objects,
            Color::srgb(0.2, 0.7, 1.),
        ),
    ] {
        for (index, record) in records(data, key).iter().enumerate() {
            if !enabled {
                continue;
            }
            let Some(position) = position(record) else {
                continue;
            };
            gizmos.cross(position, size, color);
            if let Ok(p) = camera.world_to_viewport(transform, position) {
                let Some(camera_rect) = camera.logical_viewport_rect() else {
                    continue;
                };
                let relative = (p - camera_rect.min) / camera_rect.size();
                let p = viewer.viewport.min
                    + egui::vec2(
                        relative.x * viewer.viewport.width(),
                        relative.y * viewer.viewport.height(),
                    );
                painter.text(
                    p,
                    egui::Align2::LEFT_BOTTOM,
                    label(key, index, record),
                    egui::FontId::monospace(11.),
                    egui::Color32::WHITE,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_light_positions_share_the_exported_scene_origin() {
        let record = serde_json::json!({"engine_position": [20, -40, 65536]});
        assert_eq!(position(&record), Some(Vec3::new(-1.0, 2.0, 0.0001953125)));
        assert!(position(&serde_json::json!({"engine_position": [1, 2]})).is_none());
    }

    #[test]
    fn focus_uses_decoded_neighbors_and_ignores_source_unit_radii() {
        let record = serde_json::json!({"position": [0., 0., 0.], "radius": 500000});
        let data = serde_json::json!({"markers": [record, {"position": [4., 0., 0.]}, {"position": [20., 0., 0.]}]});
        assert_eq!(focus_record(&record, &data, 100.), Some((Vec3::ZERO, 6.)));
        assert_eq!(
            focus_record(&record, &serde_json::json!({}), 100.),
            Some((Vec3::ZERO, 10.))
        );
        assert_eq!(focus_record(&serde_json::json!({}), &data, 100.), None);
    }

    #[test]
    fn animation_references_cross_model_boundaries_without_wrapping() {
        let actor = serde_json::json!({"raan_entries": [
            {"frame_count": 2, "file_path": "FIRST"},
            {"frame_count": 0, "file_path": "EMPTY"},
            {"frame_count": 3, "file_path": "SECOND"}
        ]});
        assert_eq!(frame_target(&actor, 0), Some(("FIRST", 0)));
        assert_eq!(frame_target(&actor, 2), Some(("SECOND", 0)));
        assert_eq!(frame_target(&actor, 4), Some(("SECOND", 2)));
        assert_eq!(frame_target(&actor, 5), None);
    }
    #[test]
    fn decoded_positions_require_three_finite_components() {
        assert_eq!(
            vector(&serde_json::json!([1., -2., 3.])),
            Some(Vec3::new(1., -2., 3.))
        );
        assert_eq!(vector(&serde_json::json!([1., 2.])), None);
        assert_eq!(vector(&serde_json::json!([1., 2., 1e100])), None);
        assert_eq!(label("markers", 2, &serde_json::json!({})), "markers 2");
    }
}
