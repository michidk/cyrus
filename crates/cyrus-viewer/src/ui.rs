//! Egui asset browser: content navigation and modern inspection controls.
use crate::{loading, preview};
use bevy::prelude::*;
use cyrus_app::bevy_egui::{EguiContexts, egui};
use cyrus_assets::{Catalog, Category, Content, Preview};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImagePalette {
    Embedded,
    Catalog(usize),
    Indices,
}

#[derive(Resource)]
pub struct Viewer {
    pub catalog: Option<Catalog>,
    pub format: String,
    pub search: String,
    pub selected: Option<usize>,
    pub archive: Option<String>,
    pub indexing: bool,
    pub preview: Option<Preview>,
    pub error: Option<String>,
    pub pending: Option<Handle<loading::Blob>>,
    pub loading_catalog: bool,
    pub preparing: bool,
    pub textures: Vec<egui::TextureHandle>,
    pub texture_key: Option<(usize, ImagePalette)>,
    pub font_textures: std::collections::HashMap<usize, egui::TextureHandle>,
    pub frame: usize,
    pub frame_ms: f64,
    pub frame_elapsed: f64,
    pub load_status: String,
    pub data_mode: usize,
    pub data_text: String,
    pub spatial_depth: u32,
    pub slice_y: f32,
    pub palette: ImagePalette,
    pub model_palette: Option<String>,
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
    pub fn set_preview(&mut self, preview: Preview) {
        if let Content::Spatial { cells, .. } = &preview.content {
            self.spatial_depth = 3.min(cells.iter().map(|cell| cell.depth).max().unwrap_or(0));
            self.slice_y = cells
                .iter()
                .filter(|cell| cell.depth == self.spatial_depth)
                .max_by_key(|cell| cell.visible_indices)
                .map_or(0., |cell| cell.center[1]);
        }
        if let Content::Images { frames } = &preview.content
            && frames
                .first()
                .is_some_and(|picture| picture.embedded_palette)
        {
            self.palette = ImagePalette::Embedded;
        }
        self.frame_ms = preview.frame_interval_ms.unwrap_or(100.);
        self.data_text.clear();
        self.preview = Some(preview);
    }
    pub fn set_catalog(&mut self, catalog: Catalog) {
        let selected = self
            .selected
            .and_then(|i| self.catalog.as_ref()?.entries.get(i))
            .map(|entry| &entry.id);
        self.selected =
            selected.and_then(|id| catalog.entries.iter().position(|entry| &entry.id == id));
        if !catalog
            .entries
            .iter()
            .any(|entry| entry_format(entry) == self.format)
            && let Some(entry) = catalog.entries.first()
        {
            self.format = entry_format(entry).into();
        }
        self.catalog = Some(catalog);
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            catalog: None,
            format: "3DC".into(),
            search: String::new(),
            selected: None,
            archive: None,
            indexing: false,
            preview: None,
            error: None,
            pending: None,
            loading_catalog: false,
            preparing: false,
            textures: Vec::new(),
            texture_key: None,
            font_textures: Default::default(),
            frame: 0,
            frame_ms: 100.,
            frame_elapsed: 0.,
            load_status: String::new(),
            data_mode: 0,
            data_text: String::new(),
            spatial_depth: 3,
            slice_y: 0.,
            palette: ImagePalette::Embedded,
            model_palette: None,
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
    let mut select = if !viewer.indexing && viewer.error.is_none() && viewer.preview.is_none() {
        catalog.as_ref().and_then(|catalog| {
            let entry = catalog.entries.get(viewer.selected?)?;
            if !matches!(entry.source, cyrus_assets::Source::Archive)
                || entry.category != Category::Images
            {
                return None;
            }
            let mut members = catalog
                .entries
                .iter()
                .enumerate()
                .filter(|(_, member)| member.path == entry.path && is_member(member));
            let (index, _) = members.next()?;
            members.next().is_none().then_some(index)
        })
    } else {
        None
    };
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
            let mut formats = std::collections::BTreeMap::<&str, usize>::new();
            if let Some(catalog) = &catalog {
                for entry in &catalog.entries {
                    if !is_member(entry) {
                        *formats.entry(entry_format(entry)).or_default() += 1;
                    }
                }
            }
            egui::ScrollArea::vertical()
                .id_salt("formats")
                .show(ui, |ui| {
                    for (format, count) in formats {
                        if ui
                            .selectable_label(viewer.format == format, format!("{format}  {count}"))
                            .clicked()
                        {
                            viewer.format = format.into();
                            viewer.archive = None;
                            viewer.search.clear();
                        }
                    }
                });
        });
    egui::Panel::left("entries")
        .default_size(260.)
        .min_size(170.)
        .resizable(true)
        .show(&mut viewport_ui, |ui| {
            ui.add_space(8.);
            if let Some(path) = viewer.archive.clone() {
                if ui.button("← All archives and files").clicked() {
                    viewer.archive = None;
                    viewer.search.clear();
                }
                ui.label(path);
            }
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
                    in_listing(e, &viewer.format, viewer.archive.as_deref())
                        && (query.is_empty()
                            || e.name.to_lowercase().contains(&query)
                            || e.path.to_lowercase().contains(&query)
                            || e.format.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
            if viewer.indexing {
                ui.label("Reading archive index…");
            } else if matching.is_empty() {
                ui.label(if viewer.error.is_some() {
                    "Archive could not load"
                } else if !query.is_empty() {
                    "No matching entries"
                } else {
                    "No entries in this archive"
                });
            } else {
                ui.weak(format!("{} results", matching.len()));
            }
            if viewer.format == "WAV" {
                ui.weak("Standalone WAV files. More audio is in SFX, RTX and CD audio.");
            }
            ui.separator();
            if !ctx.egui_wants_keyboard_input() {
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
                    let label = if matches!(e.source, cyrus_assets::Source::Archive) {
                        format!("{}\nOpen archive →", e.path)
                    } else {
                        format!("{}\n{}", e.name, e.path)
                    };
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
                                        viewer.format = entry_format(entry).into();
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
        .frame(if model && viewer.data_mode == 0 {
            egui::Frame::NONE
        } else {
            egui::Frame::central_panel(&ctx.style_of(egui::Theme::Dark))
        })
        .show(&mut viewport_ui, |ui| {
            if let Some(preview) = &preview {
                if !matches!(
                    &preview.content,
                    Content::Model { data: Some(_), .. }
                        | Content::Spatial { .. }
                        | Content::Data { .. }
                ) && let Some(metadata) = &preview.metadata
                {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut viewer.data_mode, 0, "Preview");
                        ui.selectable_value(&mut viewer.data_mode, 1, "JSON");
                    });
                    if viewer.data_mode == 1 {
                        json_view(ui, &mut viewer, metadata);
                        return;
                    }
                }
                match &preview.content {
                    Content::Model {
                        frames,
                        palettes,
                        data,
                    } => {
                        let panel_rect = ui.available_rect_before_wrap();
                        if let Some(data) = data {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut viewer.data_mode, 0, "Visualization");
                                ui.selectable_value(&mut viewer.data_mode, 1, "JSON");
                            });
                            if viewer.data_mode == 1 {
                                json_view(ui, &mut viewer, data);
                                return;
                            }
                        }
                        egui::Frame::new()
                            .fill(egui::Color32::from_black_alpha(200))
                            .inner_margin(8.)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
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
                                frame_transport(ui, &mut viewer, frames.len());
                                if catalog
                                    .as_ref()
                                    .and_then(|c| viewer.selected.and_then(|i| c.entries.get(i)))
                                    .is_some_and(|entry| entry.category == Category::Models)
                                {
                                    let previous = viewer.model_palette.clone();
                                    egui::ComboBox::from_id_salt("model-palette")
                                        .selected_text(
                                            viewer
                                                .model_palette
                                                .as_deref()
                                                .filter(|p| !p.is_empty())
                                                .or_else(|| {
                                                    if viewer.model_palette.is_none() {
                                                        palettes.first().map(String::as_str)
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or("Without palette"),
                                        )
                                        .show_ui(ui, |ui| {
                                            for palette in palettes {
                                                ui.selectable_value(
                                                    &mut viewer.model_palette,
                                                    Some(palette.clone()),
                                                    palette,
                                                );
                                            }
                                            ui.selectable_value(
                                                &mut viewer.model_palette,
                                                Some(String::new()),
                                                "Without palette",
                                            );
                                        });
                                    if previous != viewer.model_palette {
                                        select = viewer.selected;
                                    }
                                }
                                ui.horizontal(|ui| {
                                    if let Some((_, palette)) =
                                        preview.details.iter().find(|(key, _)| key == "Palette")
                                    {
                                        ui.weak(palette);
                                    }
                                });
                            });
                        let rect = ui.available_rect_before_wrap().intersect(panel_rect);
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
                        image_controls(
                            ui,
                            &mut viewer,
                            frames.len(),
                            catalog.as_ref(),
                            indexed,
                            frames[0].embedded_palette,
                        );
                        let picture = &frames[viewer.frame];
                        let key = (viewer.frame, viewer.palette);
                        if viewer.texture_key != Some(key) || viewer.textures.is_empty() {
                            let palette = match viewer.palette {
                                ImagePalette::Catalog(i) => {
                                    catalog.as_ref().and_then(|c| c.palettes.get(i))
                                }
                                _ => None,
                            };
                            let mut rgba = picture.pixels(palette);
                            if viewer.palette == ImagePalette::Indices
                                && let Some(indices) = &picture.indices
                            {
                                for (pixel, index) in
                                    rgba.as_chunks_mut::<4>().0.iter_mut().zip(indices)
                                {
                                    pixel[..3].fill(*index);
                                }
                            }
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
                            let pixels = if viewer.texture_key
                                != Some((frame, ImagePalette::Embedded))
                                || viewer.textures.is_empty()
                            {
                                Some(movie.rgba())
                            } else {
                                None
                            };
                            if let Some(pixels) = pixels {
                                preview::texture(&ctx, &mut viewer, pixels, width, height);
                                viewer.texture_key = Some((frame, ImagePalette::Embedded));
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
                        } else if let Some(error) = &viewer.error {
                            ui.label(error);
                        } else {
                            ui.spinner();
                            ui.label("Loading movie…");
                        }
                    }
                    Content::Spatial { cells, value } => {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut viewer.data_mode, 0, "Visualization");
                            ui.selectable_value(&mut viewer.data_mode, 1, "JSON");
                        });
                        if viewer.data_mode == 1 {
                            json_view(ui, &mut viewer, value);
                        } else {
                            visibility_view(ui, &mut viewer, cells);
                        }
                    }
                    Content::Data { value } => {
                        ui.heading("Asset data");
                        let source = value["source_lines"].as_array().map(|lines| {
                            lines
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        });
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut viewer.data_mode,
                                0,
                                if source.is_some() { "Source" } else { "Tree" },
                            );
                            ui.selectable_value(&mut viewer.data_mode, 1, "JSON");
                            if source.is_some() {
                                ui.selectable_value(&mut viewer.data_mode, 2, "Decoded fields");
                            }
                        });
                        ui.add(
                            egui::TextEdit::singleline(&mut viewer.data_filter)
                                .id(egui::Id::new("data-filter"))
                                .hint_text("Find fields or values…"),
                        );
                        let filter = viewer.data_filter.to_lowercase();
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                if viewer.data_mode == 1
                                    || (viewer.data_mode == 0 && source.is_some())
                                {
                                    let text = if viewer.data_mode == 1 {
                                        if viewer.data_text.is_empty() {
                                            viewer.data_text = serde_json::to_string_pretty(value)
                                                .unwrap_or_default();
                                        }
                                        viewer.data_text.clone()
                                    } else {
                                        source.unwrap_or_default()
                                    };
                                    let mut text = text.as_str();
                                    ui.add(
                                        egui::TextEdit::multiline(&mut text)
                                            .code_editor()
                                            .desired_width(f32::INFINITY),
                                    );
                                } else {
                                    data_tree(ui, "Asset", value, &filter, 0);
                                }
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
                    if viewer.indexing || viewer.preparing || viewer.pending.is_some() {
                        ui.add(egui::Spinner::new().size(32.));
                        ui.heading(if viewer.indexing {
                            "Reading archive index…"
                        } else {
                            "Loading selected asset…"
                        });
                        ui.label(&viewer.load_status);
                    } else if let Some(error) = &viewer.error {
                        ui.heading("Preview could not load");
                        ui.label(error);
                        if ui.button("Retry").clicked() {
                            select = viewer.selected;
                        }
                    } else if viewer.archive.is_some() {
                        ui.heading("Archive contents");
                        ui.label("Select a member from the list to inspect it.");
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
        if viewer.selected != Some(index) {
            viewer.model_palette = None;
        }
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
    embedded: bool,
) {
    ui.horizontal_wrapped(|ui| {
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
    frame_transport(ui, viewer, count);
    if indexed {
        egui::ComboBox::from_id_salt("palette-choice")
            .selected_text(match viewer.palette {
                ImagePalette::Embedded => "Embedded palette",
                ImagePalette::Catalog(i) => catalog
                    .and_then(|c| c.palettes.get(i))
                    .map_or("Without palette", |p| p.name.as_str()),
                ImagePalette::Indices => "Without palette",
            })
            .show_ui(ui, |ui| {
                if embedded {
                    ui.selectable_value(
                        &mut viewer.palette,
                        ImagePalette::Embedded,
                        "Embedded palette",
                    );
                }
                if let Some(catalog) = catalog {
                    for (i, p) in catalog.palettes.iter().enumerate() {
                        ui.selectable_value(&mut viewer.palette, ImagePalette::Catalog(i), &p.name);
                    }
                }
                ui.selectable_value(
                    &mut viewer.palette,
                    ImagePalette::Indices,
                    "Without palette",
                );
            });
    }
    ui.separator();
}
fn frame_transport(ui: &mut egui::Ui, viewer: &mut Viewer, count: usize) {
    if count < 2 {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(if viewer.playing { "Pause" } else { "Play" })
            .clicked()
        {
            if viewer.frame + 1 == count {
                viewer.frame = 0;
                viewer.scene_dirty = true;
            }
            viewer.playing = !viewer.playing;
            viewer.frame_elapsed = 0.;
        }
        ui.checkbox(&mut viewer.looping, "Loop");
        ui.label("Frame time");
        ui.add(
            egui::DragValue::new(&mut viewer.frame_ms)
                .range(10.0..=10000.0)
                .suffix(" ms"),
        );
        ui.weak(format!("{} / {count}", viewer.frame + 1));
    });
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
    ui.horizontal_wrapped(|ui| {
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

/// Numbered texture banks share a format; derived previews are not source files.
pub fn entry_format(entry: &cyrus_assets::Entry) -> &str {
    match entry.source {
        cyrus_assets::Source::World(_) => "World previews",
        cyrus_assets::Source::CdTrack(_) => "CD audio",
        _ if entry
            .path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.to_ascii_uppercase().starts_with("TEXBSI.")) =>
        {
            "TEXBSI"
        }
        _ if entry.category == Category::Other => "Other",
        _ => &entry.format,
    }
}
fn in_listing(entry: &cyrus_assets::Entry, format: &str, archive: Option<&str>) -> bool {
    match archive {
        Some(path) => entry.path == path && is_member(entry),
        None => !is_member(entry) && entry_format(entry) == format,
    }
}

fn is_member(entry: &cyrus_assets::Entry) -> bool {
    matches!(
        entry.source,
        cyrus_assets::Source::Image(_)
            | cyrus_assets::Source::Model(_)
            | cyrus_assets::Source::Effect(_)
            | cyrus_assets::Source::Speech(_)
    )
}

fn json_view(ui: &mut egui::Ui, viewer: &mut Viewer, value: &serde_json::Value) {
    if viewer.data_text.is_empty() {
        viewer.data_text = serde_json::to_string_pretty(value).unwrap_or_default();
    }
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let mut text = viewer.data_text.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .desired_width(f32::INFINITY),
            );
        });
}

fn visibility_view(ui: &mut egui::Ui, viewer: &mut Viewer, cells: &[cyrus_assets::SpatialCell]) {
    let Some(root) = cells.first() else {
        ui.label("No visibility cells");
        return;
    };
    ui.label("Visibility octree · top view (X / Z), sliced at Y. Brighter cells contain more visibility indices.");
    let depth = cells.iter().map(|c| c.depth).max().unwrap_or(0);
    ui.add(egui::Slider::new(&mut viewer.spatial_depth, 0..=depth).text("Depth"));
    let min_y = root.center[1] - root.half_extent;
    let max_y = root.center[1] + root.half_extent;
    viewer.slice_y = viewer.slice_y.clamp(min_y, max_y);
    ui.add(egui::Slider::new(&mut viewer.slice_y, min_y..=max_y).text("Y slice"));
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let scale = rect.width().min(rect.height()) / (root.half_extent * 2.).max(1.);
    let project = |x: f32, z: f32| {
        rect.center() + egui::vec2((x - root.center[0]) * scale, (z - root.center[2]) * scale)
    };
    for cell in cells.iter().filter(|c| {
        c.depth == viewer.spatial_depth
            && viewer.slice_y <= c.center[1] + c.half_extent
            && (viewer.slice_y > c.center[1] - c.half_extent
                || (viewer.slice_y == min_y && c.center[1] - c.half_extent == min_y))
    }) {
        let center = project(cell.center[0], cell.center[2]);
        let bounds =
            egui::Rect::from_center_size(center, egui::Vec2::splat(cell.half_extent * 2. * scale));
        let shade = (cell.visible_indices as f32 + 1.).ln().min(10.) / 10.;
        ui.painter().rect_filled(
            bounds,
            0.,
            egui::Color32::from_rgb(25, (50. + shade * 180.) as u8, (60. + shade * 170.) as u8),
        );
        ui.painter().rect_stroke(
            bounds,
            0.,
            egui::Stroke::new(0.5, egui::Color32::GRAY),
            egui::StrokeKind::Inside,
        );
        if response.hover_pos().is_some_and(|p| bounds.contains(p)) {
            response.clone().on_hover_text(format!(
                "Center {:?}\nHalf extent {}\n{} visibility indices",
                cell.center, cell.half_extent, cell.visible_indices
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_palette_defaults_preserve_embedded_artwork() {
        let mut viewer = Viewer {
            palette: ImagePalette::Catalog(0),
            ..default()
        };
        let mut preview = Preview {
            content: Content::Images {
                frames: vec![cyrus_assets::Picture {
                    label: "test".into(),
                    width: 1,
                    height: 1,
                    rgba: vec![10, 20, 30, 255],
                    indices: Some(vec![1]),
                    embedded_palette: true,
                }],
            },
            details: vec![],
            frame_interval_ms: None,
            metadata: None,
            related: vec![],
        };
        preview.frame_interval_ms = Some(125.);
        viewer.set_preview(preview.clone());
        assert_eq!(viewer.frame_ms, 125.);
        assert_eq!(viewer.palette, ImagePalette::Embedded);
        if let Content::Images { frames } = &mut preview.content {
            frames[0].embedded_palette = false;
        }
        viewer.palette = ImagePalette::Catalog(0);
        preview.frame_interval_ms = None;
        viewer.set_preview(preview);
        assert_eq!(viewer.frame_ms, 100.);
        assert_eq!(viewer.palette, ImagePalette::Catalog(0));
    }
    #[test]
    fn spatial_preview_opens_on_a_populated_layer() {
        let mut viewer = Viewer::default();
        viewer.set_preview(Preview {
            content: Content::Spatial {
                cells: vec![
                    cyrus_assets::SpatialCell {
                        center: [0.; 3],
                        half_extent: 8.,
                        depth: 0,
                        visible_indices: 0,
                    },
                    cyrus_assets::SpatialCell {
                        center: [1., 5., 1.],
                        half_extent: 1.,
                        depth: 3,
                        visible_indices: 4,
                    },
                ],
                value: serde_json::Value::Null,
            },
            details: vec![],
            frame_interval_ms: None,
            metadata: None,
            related: vec![],
        });
        assert_eq!(viewer.spatial_depth, 3);
        assert_eq!(viewer.slice_y, 5.);
    }
    #[test]
    fn archive_members_are_visible_independently_of_format_labels() {
        let member = cyrus_assets::Entry {
            id: String::new(),
            name: "member".into(),
            path: "fxart/DRINT.ROB".into(),
            category: Category::Models,
            format: "Model".into(),
            source: cyrus_assets::Source::Model(0),
            bytes: 0,
            error: None,
        };
        assert!(in_listing(&member, "ROB", Some("fxart/DRINT.ROB")));
        assert!(!in_listing(&member, "ROB", Some("fxart/OTHER.ROB")));
        assert!(!in_listing(&member, "Model", None));
        let other = cyrus_assets::Entry {
            category: Category::Other,
            source: cyrus_assets::Source::File,
            ..member
        };
        let mut viewer = Viewer::default();
        viewer.set_catalog(Catalog {
            version: cyrus_assets::CATALOG_VERSION,
            generation: String::new(),
            entries: vec![other],
            palettes: vec![],
        });
        assert_eq!(viewer.catalog.as_ref().unwrap().entries.len(), 1);
        assert_eq!(viewer.format, "Other");
    }
    #[test]
    fn browsing_separates_extensions_and_groups_numbered_texture_banks() {
        let mut entry = cyrus_assets::Entry {
            id: String::new(),
            name: String::new(),
            path: "maps/ISLAND.RGM".into(),
            category: Category::Worlds,
            format: "RGM".into(),
            source: cyrus_assets::Source::File,
            bytes: 0,
            error: None,
        };
        assert_eq!(entry_format(&entry), "RGM");
        entry.path = "maps/ISLAND.WLD".into();
        entry.format = "WLD".into();
        assert_eq!(entry_format(&entry), "WLD");
        entry.path = "fxart/TEXBSI.168".into();
        entry.format = "168".into();
        assert_eq!(entry_format(&entry), "TEXBSI");
        entry.source = cyrus_assets::Source::Image(0);
        assert_eq!(entry_format(&entry), "TEXBSI");
        assert!(is_member(&entry));
        entry.source = cyrus_assets::Source::World(1);
        assert_eq!(entry_format(&entry), "World previews");
        assert!(!is_member(&entry));
    }
}
