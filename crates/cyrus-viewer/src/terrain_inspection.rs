//! Terrain layers expose decoded WLD cells independently of scene materials.
use crate::ui::Viewer;
use bevy::prelude::*;
use cyrus_app::bevy_egui::egui;
use serde_json::Value;

pub struct State {
    pub open: bool,
    pub directions: bool,
    pub water: bool,
    layer: usize,
    column: usize,
    row: usize,
    stride: usize,
    texture: Option<(usize, egui::TextureHandle)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            directions: false,
            water: false,
            layer: 0,
            column: 0,
            row: 0,
            stride: 8,
            texture: None,
        }
    }
}
pub fn data(value: &Value) -> Option<&Value> {
    if value.get("combined").is_some() {
        Some(value)
    } else {
        value.get("terrain").filter(|v| !v.is_null())
    }
}
pub fn controls(ui: &mut egui::Ui, state: &mut State, value: &Value) {
    if data(value).is_none() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut state.open, "Terrain maps");
        ui.checkbox(&mut state.directions, "Tile directions");
        ui.checkbox(&mut state.water, "Wave vertices");
        if state.directions || state.water {
            ui.add(egui::Slider::new(&mut state.stride, 1..=32).text("Overlay spacing"));
        }
    });
}
fn array(value: &Value, key: &str, index: usize) -> Option<f64> {
    value[key].get(index)?.as_f64()
}
fn position(value: &Value, index: usize) -> Option<Vec3> {
    let width = value["width"].as_u64()? as usize;
    if width == 0 {
        return None;
    }
    Some(Vec3::new(
        array(value, "scene_x", index % width)? as f32,
        array(value, "scene_y", index)? as f32,
        array(value, "scene_z", index / width)? as f32,
    ))
}
pub fn draw(viewer: Res<Viewer>, mut gizmos: Gizmos) {
    let Some(preview) = &viewer.preview else {
        return;
    };
    if !viewer.shows_3d(&preview.content) {
        return;
    }
    let cyrus_assets::Content::Model {
        data: Some(value), ..
    } = &preview.content
    else {
        return;
    };
    let Some(value) = data(value) else {
        return;
    };
    let state = &viewer.terrain_inspection;
    if !state.directions && !state.water && !state.open {
        return;
    }
    let combined = &value["combined"];
    let width = combined["width"].as_u64().unwrap_or(0) as usize;
    let height = combined["height"].as_u64().unwrap_or(0) as usize;
    if width < 2 || height < 2 {
        return;
    }
    if state.directions || state.water {
        for row in (0..height - 1).step_by(state.stride.max(1)) {
            for column in (0..width - 1).step_by(state.stride.max(1)) {
                let index = row * width + column;
                let Some(p) = position(combined, index) else {
                    continue;
                };
                let Some(right) = position(combined, index + 1) else {
                    continue;
                };
                let Some(down) = position(combined, index + width) else {
                    continue;
                };
                let edge = (right - p).length().min((down - p).length());
                if state.directions {
                    let rotation = array(combined, "quarter_turns", index).unwrap_or(0.) as usize;
                    let x = (right - p).normalize_or_zero();
                    let z = (down - p).normalize_or_zero();
                    let direction = [x, -z, -x, z][rotation.min(3)];
                    let center = (right + down) * 0.5 + Vec3::Y * edge * 0.04;
                    gizmos.arrow(
                        center,
                        center + direction * edge * 0.4,
                        Color::srgb(1., 0.8, 0.2),
                    );
                }
                if state.water
                    && combined["wave_vertices"]
                        .get(index)
                        .and_then(Value::as_bool)
                        == Some(true)
                {
                    gizmos.cross(
                        Isometry3d::from_translation(p),
                        edge * 0.15,
                        Color::srgb(0.2, 0.6, 1.),
                    );
                }
            }
        }
    }
    if state.open {
        let index = state.row.min(height - 1) * width + state.column.min(width - 1);
        if state.row == height - 1 || state.column == width - 1 {
            if let Some(p) = position(combined, index) {
                gizmos.cross(
                    Isometry3d::from_translation(p),
                    viewer.distance * 0.003,
                    Color::srgb(1., 0.2, 0.6),
                );
            }
            return;
        }
        if let [Some(a), Some(b), Some(c), Some(d)] =
            [index, index + 1, index + width + 1, index + width].map(|i| position(combined, i))
        {
            gizmos.linestrip([a, b, c, d, a], Color::srgb(1., 0.2, 0.6));
        }
    }
}
pub fn window(
    mut contexts: cyrus_app::bevy_egui::EguiContexts,
    mut viewer: ResMut<Viewer>,
) -> Result {
    if !viewer.terrain_inspection.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let Some(preview) = viewer.preview.take() else {
        return Ok(());
    };
    if let cyrus_assets::Content::Model {
        data: Some(value), ..
    } = &preview.content
        && let Some(value) = data(value)
    {
        let state = &mut viewer.terrain_inspection;
        let combined = &value["combined"];
        let width = combined["width"].as_u64().unwrap_or(0) as usize;
        let height = combined["height"].as_u64().unwrap_or(0) as usize;
        if width > 0 && height > 0 {
            let mut open = state.open;
            egui::Window::new("Terrain maps").open(&mut open).default_size([550.,650.]).show(ctx,|ui|{
                let layers=["Heightmap","Material","Texture rotations","Wave vertices"];
                egui::ComboBox::from_id_salt("terrain-layer").selected_text(layers[state.layer.min(3)]).show_ui(ui,|ui|{for (i,name) in layers.iter().enumerate(){ui.selectable_value(&mut state.layer,i,*name);}});
                ui.weak("Columns increase right; rows increase down. Directions describe texture rotation, not water flow.");
                if state.layer == 0 {
                    ui.weak("Black = low terrain; white = high terrain. Click a cell to inspect its height and texture rotation.");
                } else if state.layer == 2 {
                    ui.horizontal_wrapped(|ui| {
                        for (color, label) in [(egui::Color32::RED,"0°"),(egui::Color32::GREEN,"90°"),(egui::Color32::BLUE,"180°"),(egui::Color32::YELLOW,"270°")] {
                            ui.colored_label(color, format!("■ {label}"));
                        }
                    });
                }
                if state.texture.as_ref().is_none_or(|(layer,_)|*layer!=state.layer) {
                    let heights=combined["height_engine_units"].as_array();
                    let max_height=heights.into_iter().flatten().filter_map(Value::as_f64).map(f64::abs).fold(1.,f64::max);
                    let colors=(0..width*height).map(|index|match state.layer {
                        0=>egui::Color32::from_gray((array(combined,"height_engine_units",index).unwrap_or(0.).abs()/max_height*255.) as u8),
                        1=>{let id=array(combined,"texture_indices",index).unwrap_or(0.) as u8; egui::Color32::from_rgb(id.wrapping_mul(73),id.wrapping_mul(151),id.wrapping_mul(211))},
                        2=>[egui::Color32::RED,egui::Color32::GREEN,egui::Color32::BLUE,egui::Color32::YELLOW][(array(combined,"quarter_turns",index).unwrap_or(0.) as usize).min(3)],
                        _=>if combined["wave_vertices"].get(index).and_then(Value::as_bool)==Some(true){egui::Color32::LIGHT_BLUE}else{egui::Color32::from_gray(30)},
                    }).collect();
                    state.texture=Some((state.layer,ctx.load_texture("terrain-layer",egui::ColorImage::new([width,height],colors),egui::TextureOptions::NEAREST)));
                }
                ui.horizontal(|ui|{
                    ui.add(egui::Slider::new(&mut state.column,0..=width-1).text("Column"));
                    ui.add(egui::Slider::new(&mut state.row,0..=height-1).text("Row"));
                });
                let index=state.row*width+state.column;
                ui.label(format!("Height: {} engine units · texture: {} · rotation: {}° · wave vertex: {}",combined["height_engine_units"][index],combined["texture_indices"][index],array(combined,"quarter_turns",index).unwrap_or(0.)*90.,combined["wave_vertices"][index]));
                if let Some((_,texture))=&state.texture {
                    let size=ui.available_width().min(ui.available_height()).max(1.);
                    let response=ui.add(egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(size)).sense(egui::Sense::click_and_drag()));
                    if (response.clicked()||response.dragged())&&let Some(p)=response.interact_pointer_pos(){let p=(p-response.rect.min)/response.rect.size();state.column=(p.x*width as f32) as usize;state.row=(p.y*height as f32) as usize;state.column=state.column.min(width-1);state.row=state.row.min(height-1);}
                    let min=response.rect.min+egui::vec2(state.column as f32/width as f32*size,state.row as f32/height as f32*size);
                    ui.painter().rect_stroke(egui::Rect::from_min_size(min,egui::vec2(size/width as f32,size/height as f32)),0.,egui::Stroke::new(2.,egui::Color32::from_rgb(255,50,180)),egui::StrokeKind::Outside);
                }
            });
            state.open = open;
        }
    }
    viewer.preview = Some(preview);
    Ok(())
}
