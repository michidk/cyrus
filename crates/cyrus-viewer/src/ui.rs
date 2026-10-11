//! Egui asset browser: content navigation and modern inspection controls.
use crate::{loading, preview};
use bevy::prelude::*;
use cyrus_app::bevy_egui::{EguiContexts, egui};
use cyrus_assets::{Catalog, Category, Content, Preview};

#[derive(Resource)]
pub struct Viewer {
    pub catalog: Option<Catalog>,
    pub category: Category,
    pub search: String,
    pub selected: Option<usize>,
    pub preview: Option<Preview>,
    pub error: Option<String>,
    pub pending: Option<Handle<loading::Blob>>,
    pub loading_catalog: bool,
    pub preparing: bool,
    pub textures: Vec<egui::TextureHandle>,
    pub texture_key: Option<(usize, Option<usize>)>,
    pub font_textures: std::collections::HashMap<usize, egui::TextureHandle>,
    pub frame: usize,
    pub palette: Option<usize>,
    pub zoom: f32,
    pub fit: bool,
    pub wireframe: bool,
    pub scene_dirty: bool,
    pub audio_dirty: bool,
    pub needs_frame: bool,
    pub viewport: egui::Rect,
    pub pixels_per_point: f32,
    pub center: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub playing: bool,
    pub looping: bool,
    pub position: f64,
    pub volume: f32,
    pub seek_dirty: bool,
    pub movie: Option<cyrus_assets::media::Movie>,
    pub movie_blob: Option<Handle<loading::Blob>>,
    pub decoding: bool,
    details: bool,
    sample: String,
    data_filter: String,
    styled: bool,
}
impl Viewer {
    pub fn set_catalog(&mut self, mut catalog: Catalog) {
        catalog
            .entries
            .retain(|entry| entry.category != Category::Other);
        self.catalog = Some(catalog);
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            catalog: None,
            category: Category::Models,
            search: String::new(),
            selected: None,
            preview: None,
            error: None,
            pending: None,
            loading_catalog: false,
            preparing: false,
            textures: Vec::new(),
            texture_key: None,
            font_textures: Default::default(),
            frame: 0,
            palette: None,
            zoom: 1.,
            fit: true,
            wireframe: false,
            scene_dirty: false,
            audio_dirty: false,
            needs_frame: false,
            pixels_per_point: 1.,
            viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.)),
            center: Vec3::ZERO,
            distance: 6.,
            yaw: 0.4,
            pitch: -0.2,
            playing: false,
            looping: false,
            position: 0.,
            volume: 0.7,
            seek_dirty: false,
            movie: None,
            movie_blob: None,
            decoding: false,
            details: true,
            sample: "The Elder Scrolls Adventures: Redguard".into(),
            data_filter: String::new(),
            styled: false,
        }
    }
}

pub fn draw(
    mut contexts: EguiContexts,
    mut viewer: ResMut<Viewer>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    #[cfg(not(target_arch = "wasm32"))] library: Option<Res<loading::NativeLibrary>>,
    #[cfg(target_arch = "wasm32")] worker: NonSend<crate::browser::BrowserWorker>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    if !viewer.styled {
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = egui::vec2(10., 8.);
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = egui::Color32::from_rgb(21, 27, 35);
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(43, 85, 77);
        style.visuals.selection.stroke.color = egui::Color32::from_rgb(179, 231, 214);
        ctx.set_style_of(egui::Theme::Dark, style);
        ctx.set_theme(egui::Theme::Dark);
        viewer.styled = true;
    }
    if viewer.catalog.is_none() && viewer.error.is_none() {
        return Ok(());
    }
    let mut viewport_ui = egui::Ui::new(
        ctx.clone(),
        "viewer".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let catalog = viewer.catalog.take();
    let mut select = None;
    egui::Panel::top("title").show(&mut viewport_ui, |ui| {
        ui.add_space(6.);
        ui.horizontal(|ui| {
            ui.heading("cyrus");
            ui.label(
                egui::RichText::new("ASSET VIEWER").color(egui::Color32::from_rgb(144, 198, 188)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut viewer.details, "Details");
                ui.label("Original assets · read only");
            });
        });
        ui.add_space(6.);
    });
    egui::Panel::bottom("status").show(&mut viewport_ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(catalog) = &catalog {
                ui.label(format!("{} catalog entries", catalog.entries.len()));
            }
            if let Some(error) = &viewer.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        });
    });
    egui::Panel::left("categories")
        .default_size(150.)
        .min_size(110.)
        .show(&mut viewport_ui, |ui| {
            ui.add_space(10.);
            ui.weak("LIBRARY");
            ui.separator();
            for category in Category::ALL
                .into_iter()
                .filter(|category| *category != Category::Other)
            {
                let count = catalog.as_ref().map_or(0, |c| {
                    c.entries.iter().filter(|e| e.category == category).count()
                });
                if ui
                    .selectable_label(
                        viewer.category == category,
                        format!("{}  {count}", category.label()),
                    )
                    .clicked()
                {
                    viewer.category = category;
                }
            }
        });
    egui::Panel::left("entries")
        .default_size(260.)
        .min_size(170.)
        .resizable(true)
        .show(&mut viewport_ui, |ui| {
            ui.add_space(8.);
            ui.add(
                egui::TextEdit::singleline(&mut viewer.search)
                    .id(egui::Id::new("asset-search"))
                    .hint_text("Search names, archives, formats…")
                    .desired_width(f32::INFINITY),
            );
            let Some(catalog) = &catalog else {
                ui.spinner();
                ui.label("Reading catalog…");
                return;
            };
            let query = viewer.search.to_lowercase();
            let matching: Vec<_> = catalog
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    e.category == viewer.category
                        && (query.is_empty()
                            || e.name.to_lowercase().contains(&query)
                            || e.path.to_lowercase().contains(&query)
                            || e.format.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
            ui.weak(format!("{} results", matching.len()));
            ui.separator();
            if !ctx.memory(|memory| {
                ["asset-search", "font-sample", "data-filter"]
                    .iter()
                    .any(|id| memory.has_focus(egui::Id::new(id)))
            }) {
                let delta = ctx.input(|i| {
                    if i.key_pressed(egui::Key::ArrowDown) {
                        1
                    } else if i.key_pressed(egui::Key::ArrowUp) {
                        -1
                    } else {
                        0
                    }
                });
                if delta != 0 && !matching.is_empty() {
                    let position = viewer
                        .selected
                        .and_then(|s| matching.iter().position(|i| *i == s));
                    let next = position.map_or(0, |p| {
                        (p as isize + delta).clamp(0, matching.len() as isize - 1) as usize
                    });
                    select = Some(matching[next]);
                }
            }
            let mut scroll = egui::ScrollArea::vertical().id_salt("asset-list");
            if let Some(row) =
                select.and_then(|selected| matching.iter().position(|i| *i == selected))
            {
                scroll = scroll.vertical_scroll_offset(
                    (row as f32 * (48. + ui.spacing().item_spacing.y)
                        - ui.available_height() * 0.5)
                        .max(0.),
                );
            }
            scroll.show_rows(ui, 48., matching.len(), |ui, range| {
                for index in range {
                    let index = matching[index];
                    let e = &catalog.entries[index];
                    let label = format!("{}\n{}", e.name, e.path);
                    let response = ui.add_sized(
                        [ui.available_width(), 46.],
                        egui::Button::new(label).selected(viewer.selected == Some(index)),
                    );
                    if select == Some(index) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() {
                        select = Some(index);
                    }
                    if let Some(error) = &e.error {
                        response.on_hover_text(error);
                    }
                }
            });
        });
    let preview = viewer.preview.take();
    if viewer.details {
        egui::Panel::right("details")
            .default_size(235.)
            .min_size(160.)
            .show(&mut viewport_ui, |ui| {
                ui.add_space(10.);
                ui.weak("ASSET DETAILS");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(entry) = catalog
                        .as_ref()
                        .and_then(|c| viewer.selected.and_then(|i| c.entries.get(i)))
                    {
                        ui.strong(&entry.name);
                        ui.label(&entry.path);
                        ui.weak(format!("{} · {} source bytes", entry.format, entry.bytes));
                    }
                    if let Some(preview) = &preview {
                        for (key, value) in &preview.details {
                            ui.separator();
                            ui.weak(key);
                            ui.label(value);
                        }
                        if !preview.related.is_empty() {
                            ui.separator();
                            ui.strong("Related assets");
                            if let Some(catalog) = &catalog {
                                let related: std::collections::HashSet<_> =
                                    preview.related.iter().collect();
                                for (i, entry) in catalog
                                    .entries
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, e)| related.contains(&e.id))
                                {
                                    if ui.link(&entry.name).on_hover_text(&entry.path).clicked() {
                                        select = Some(i);
                                        viewer.category = entry.category;
                                        viewer.search.clear();
                                    }
                                }
                            }
                        }
                    }
                });
            });
    }
    let model = preview
        .as_ref()
        .is_some_and(|p| matches!(p.content, Content::Model { .. }));
    egui::CentralPanel::default()
        .frame(if model {
            egui::Frame::NONE
        } else {
            egui::Frame::central_panel(&ctx.style_of(egui::Theme::Dark))
        })
        .show(&mut viewport_ui, |ui| {
            if let Some(preview) = &preview {
                match &preview.content {
                    Content::Model { frames } => {
                        egui::Frame::new()
                            .fill(egui::Color32::from_black_alpha(200))
                            .inner_margin(8.)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut viewer.wireframe, "Edges");
                                    if ui.button("Frame object").clicked() {
                                        viewer.needs_frame = true;
                                    }
                                    if frames.len() > 1 {
                                        let previous = viewer.frame;
                                        ui.add(
                                            egui::Slider::new(
                                                &mut viewer.frame,
                                                0..=frames.len() - 1,
                                            )
                                            .text("Model frame"),
                                        );
                                        if previous != viewer.frame {
                                            viewer.scene_dirty = true;
                                        }
                                    }
                                    ui.weak("Textured · orbit / pan / zoom");
                                });
                            });
                        let rect = ui.available_rect_before_wrap();
                        viewer.viewport = rect;
                        viewer.pixels_per_point = ctx.pixels_per_point();
                        let response = ui.allocate_rect(rect, egui::Sense::drag());
                        if response.dragged_by(egui::PointerButton::Primary) {
                            let delta = response.drag_delta();
                            viewer.yaw -= delta.x * 0.008;
                            viewer.pitch = (viewer.pitch - delta.y * 0.008).clamp(-1.5, 1.5);
                        }
                        if response.dragged_by(egui::PointerButton::Secondary) {
                            let delta = response.drag_delta();
                            let scale = viewer.distance * 0.002;
                            let rotation = Quat::from_rotation_y(viewer.yaw);
                            viewer.center +=
                                rotation * Vec3::new(-delta.x * scale, delta.y * scale, 0.);
                        }
                        if response.hovered() {
                            viewer.distance = (viewer.distance
                                * (-ctx.input(|i| i.smooth_scroll_delta.y) * 0.002).exp())
                            .clamp(0.02, 1_000_000.);
                        }
                    }
                    Content::Images { frames } => {
                        let indexed = frames[viewer.frame].indices.is_some();
                        image_controls(ui, &mut viewer, frames.len(), catalog.as_ref(), indexed);
                        let picture = &frames[viewer.frame];
                        let key = (viewer.frame, viewer.palette);
                        if viewer.texture_key != Some(key) || viewer.textures.is_empty() {
                            let palette = viewer
                                .palette
                                .and_then(|i| catalog.as_ref()?.palettes.get(i));
                            let rgba = picture.pixels(palette);
                            preview::texture(
                                &ctx,
                                &mut viewer,
                                rgba,
                                picture.width,
                                picture.height,
                            );
                            viewer.texture_key = Some(key);
                        }
                        if let Some(texture) = viewer.textures.first() {
                            picture_view(
                                ui,
                                texture.id(),
                                egui::vec2(picture.width as f32, picture.height as f32),
                                viewer.zoom,
                                viewer.fit,
                            );
                        }
                    }
                    Content::Palette { colors } => {
                        ui.heading("Palette");
                        ui.weak("Hover a swatch for its index and RGB channels.");
                        ui.separator();
                        egui::Grid::new("palette")
                            .spacing(egui::vec2(3., 3.))
                            .show(ui, |ui| {
                                for (i, &[r, g, b]) in colors.iter().enumerate() {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(23., 23.),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        2.,
                                        egui::Color32::from_rgb(r, g, b),
                                    );
                                    response.on_hover_text(format!("Index {i}\nRGB {r}, {g}, {b}"));
                                    if i % 16 == 15 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                    Content::Font {
                        glyphs,
                        first,
                        line_height,
                    } => {
                        ui.heading("Font specimen");
                        ui.add(
                            egui::TextEdit::singleline(&mut viewer.sample)
                                .id(egui::Id::new("font-sample")),
                        );
                        ui.add(egui::Slider::new(&mut viewer.zoom, 0.5..=5.).text("Scale"));
                        let text = viewer.sample.clone();
                        let scale = viewer.zoom;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                            for c in text.chars() {
                                let Some(index) = (c as usize)
                                    .checked_sub(*first as usize)
                                    .filter(|i| *i < glyphs.len())
                                else {
                                    continue;
                                };
                                let glyph = &glyphs[index];
                                let texture =
                                    viewer.font_textures.entry(index).or_insert_with(|| {
                                        ctx.load_texture(
                                            format!("glyph-{index}"),
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [glyph.width as usize, glyph.height as usize],
                                                &glyph.rgba,
                                            ),
                                            egui::TextureOptions::NEAREST,
                                        )
                                    });
                                let size = egui::vec2(
                                    glyph.width as f32 * scale,
                                    glyph.height as f32 * scale,
                                );
                                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                ui.painter().image(
                                    texture.id(),
                                    rect,
                                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                    egui::Color32::WHITE,
                                );
                            }
                        });
                        ui.separator();
                        ui.weak(format!(
                            "{} glyphs · first character {} · line height {}",
                            glyphs.len(),
                            first,
                            line_height
                        ));
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (i, glyph) in glyphs.iter().enumerate() {
                                    let texture =
                                        viewer.font_textures.entry(i).or_insert_with(|| {
                                            ctx.load_texture(
                                                format!("glyph-{i}"),
                                                egui::ColorImage::from_rgba_unmultiplied(
                                                    [glyph.width as usize, glyph.height as usize],
                                                    &glyph.rgba,
                                                ),
                                                egui::TextureOptions::NEAREST,
                                            )
                                        });
                                    ui.add(egui::Image::new((
                                        texture.id(),
                                        egui::vec2(
                                            glyph.width as f32 * 2.,
                                            glyph.height as f32 * 2.,
                                        ),
                                    )))
                                    .on_hover_text(format!(
                                        "{} · {}",
                                        *first as usize + i,
                                        glyph.label
                                    ));
                                }
                            });
                        });
                    }
                    Content::Audio { peaks, seconds, .. } => {
                        ui.heading("Audio");
                        ui.add_space(24.);
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 180.),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_filled(rect, 6., egui::Color32::from_rgb(14, 20, 27));
                        for (i, [low, high]) in peaks.iter().enumerate() {
                            let x =
                                rect.left() + rect.width() * i as f32 / peaks.len().max(1) as f32;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(x, rect.center().y - low * 80.),
                                    egui::pos2(x, rect.center().y - high * 80.),
                                ],
                                egui::Stroke::new(1., egui::Color32::from_rgb(144, 198, 188)),
                            );
                        }
                        transport(ui, &mut viewer, *seconds);
                    }
                    Content::Movie { seconds, .. } => {
                        transport(ui, &mut viewer, *seconds);
                        if let Some(movie) = &viewer.movie {
                            let frame = movie.frame as usize;
                            let (width, height, display_height) =
                                (movie.width, movie.height, movie.display_height);
                            let pixels = if viewer.texture_key != Some((frame, None))
                                || viewer.textures.is_empty()
                            {
                                Some(movie.rgba())
                            } else {
                                None
                            };
                            if let Some(pixels) = pixels {
                                preview::texture(&ctx, &mut viewer, pixels, width, height);
                                viewer.texture_key = Some((frame, None));
                            }
                            if let Some(texture) = viewer.textures.first() {
                                picture_view(
                                    ui,
                                    texture.id(),
                                    egui::vec2(width as f32, display_height as f32),
                                    1.,
                                    true,
                                );
                            }
                        } else {
                            ui.spinner();
                            ui.label("Loading movie…");
                        }
                    }
                    Content::Data { value } => {
                        ui.heading("Decoded data");
                        ui.add(
                            egui::TextEdit::singleline(&mut viewer.data_filter)
                                .id(egui::Id::new("data-filter"))
                                .hint_text("Find fields or values…"),
                        );
                        let filter = viewer.data_filter.to_lowercase();
                        egui::ScrollArea::both().show(ui, |ui| {
                            data_tree(ui, "Asset", value, &filter, 0);
                        });
                    }
                    Content::Unavailable { reason } => {
                        ui.heading("Preview unavailable");
                        ui.label(reason);
                    }
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.5 - 40.).max(0.));
                    if viewer.preparing || viewer.pending.is_some() {
                        ui.add(egui::Spinner::new().size(32.));
                        ui.heading("Loading selected asset…");
                    } else if let Some(error) = &viewer.error {
                        ui.heading("Preview could not load");
                        ui.label(error);
                    } else {
                        ui.heading("Explore Redguard");
                        ui.label("Choose a category and an asset to inspect.");
                    }
                });
            }
        });
    viewer.preview = preview;
    viewer.catalog = catalog;
    if let Some(index) = select {
        loading::select(
            index,
            &mut viewer,
            &mut commands,
            &assets,
            #[cfg(not(target_arch = "wasm32"))]
            library.as_deref(),
            #[cfg(target_arch = "wasm32")]
            &worker,
        );
    }
    Ok(())
}
fn image_controls(
    ui: &mut egui::Ui,
    viewer: &mut Viewer,
    count: usize,
    catalog: Option<&Catalog>,
    indexed: bool,
) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut viewer.fit, "Fit");
        if ui.button("1:1").clicked() {
            viewer.fit = false;
            viewer.zoom = 1.;
        }
        if !viewer.fit {
            ui.add(egui::Slider::new(&mut viewer.zoom, 0.1..=10.).text("Zoom"));
        }
        if count > 1 {
            ui.add(egui::Slider::new(&mut viewer.frame, 0..=count - 1).text("Frame"));
        }
    });
    if indexed {
        egui::ComboBox::from_id_salt("palette-choice")
            .selected_text(
                viewer
                    .palette
                    .and_then(|i| catalog?.palettes.get(i))
                    .map_or("Embedded / indices", |p| p.name.as_str()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut viewer.palette, None, "Embedded / indices");
                if let Some(catalog) = catalog {
                    for (i, p) in catalog.palettes.iter().enumerate() {
                        ui.selectable_value(&mut viewer.palette, Some(i), &p.name);
                    }
                }
            });
    }
    ui.separator();
}
fn picture_view(ui: &mut egui::Ui, id: egui::TextureId, size: egui::Vec2, zoom: f32, fit: bool) {
    let available = ui.available_size();
    let scale = if fit {
        (available.x / size.x).min(available.y / size.y).max(0.01)
    } else {
        zoom
    };
    egui::ScrollArea::both().show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(size * scale, egui::Sense::hover());
        let clipped = rect.intersect(ui.clip_rect());
        let first = (clipped.min - rect.min) / 16.;
        let last = (clipped.max - rect.min) / 16.;
        for y in first.y.max(0.) as usize..last.y.ceil().max(0.) as usize {
            for x in first.x.max(0.) as usize..last.x.ceil().max(0.) as usize {
                let tile = egui::Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * 16., y as f32 * 16.),
                    egui::vec2(16., 16.),
                )
                .intersect(rect);
                ui.painter().rect_filled(
                    tile,
                    0.,
                    egui::Color32::from_gray(if (x + y) % 2 == 0 { 36 } else { 46 }),
                );
            }
        }
        ui.painter().image(
            id,
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
    });
}
fn transport(ui: &mut egui::Ui, viewer: &mut Viewer, duration: f64) {
    ui.horizontal(|ui| {
        if ui
            .button(if viewer.playing { "Pause" } else { "Play" })
            .clicked()
        {
            if viewer.position >= duration {
                viewer.position = 0.;
                viewer.seek_dirty = true;
                viewer.audio_dirty = true;
            }
            viewer.playing = !viewer.playing;
        }
        if ui.button("Stop").clicked() {
            viewer.playing = false;
            viewer.position = 0.;
            viewer.seek_dirty = true;
            viewer.audio_dirty = true;
        }
        if viewer.movie.is_some() && ui.button("Next frame").clicked() {
            viewer.playing = false;
            viewer.position = (viewer.position
                + viewer.movie.as_ref().map_or(0., |m| m.seconds_per_frame))
            .min(duration);
            viewer.seek_dirty = true;
        }
        ui.checkbox(&mut viewer.looping, "Loop");
        ui.add(egui::Slider::new(&mut viewer.volume, 0.0..=1.0).text("Volume"));
    });
    if ui
        .add(egui::Slider::new(&mut viewer.position, 0.0..=duration).text("seconds"))
        .changed()
    {
        viewer.seek_dirty = true;
    }
    ui.separator();
}
fn data_tree(ui: &mut egui::Ui, key: &str, value: &serde_json::Value, filter: &str, depth: usize) {
    if depth > 20 {
        ui.label("Nested data beyond display depth");
        return;
    }
    if !filter.is_empty()
        && !key.to_lowercase().contains(filter)
        && !value.to_string().to_lowercase().contains(filter)
    {
        return;
    }
    match value {
        serde_json::Value::Object(fields) => {
            egui::CollapsingHeader::new(format!("{key} · {} fields", fields.len()))
                .id_salt((key, depth))
                .default_open(depth == 0 || !filter.is_empty())
                .show(ui, |ui| {
                    for (name, value) in fields {
                        data_tree(ui, name, value, filter, depth + 1);
                    }
                });
        }
        serde_json::Value::Array(items) => {
            egui::CollapsingHeader::new(format!("{key} · {} entries", items.len()))
                .id_salt((key, depth))
                .default_open(!filter.is_empty())
                .show(ui, |ui| {
                    for (i, value) in items.iter().enumerate() {
                        ui.push_id(i, |ui| {
                            data_tree(ui, &i.to_string(), value, filter, depth + 1)
                        });
                    }
                });
        }
        _ => {
            ui.horizontal_wrapped(|ui| {
                ui.weak(key);
                ui.label(
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned),
                );
            });
        }
    }
}
