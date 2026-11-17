//! The launcher's look: the colours of the original launcher's leather and gold, and the few widgets
//! the dialogs are built from. Styling after the maiko launcher (MIT, same author).
use eframe::egui::{self, Align2, Color32, Pos2, Sense, Vec2};

pub const INK: Color32 = Color32::from_rgb(26, 18, 12);
pub const LEATHER: Color32 = Color32::from_rgb(56, 40, 26);
pub const GOLD: Color32 = Color32::from_rgb(207, 168, 96);
pub const GOLD_DIM: Color32 = Color32::from_rgb(122, 96, 56);
pub const PARCHMENT: Color32 = Color32::from_rgb(238, 224, 192);
pub const MUTED: Color32 = Color32::from_rgb(170, 150, 118);
pub const RED: Color32 = Color32::from_rgb(148, 30, 20);
pub const ERROR: Color32 = Color32::from_rgb(232, 112, 92);
pub const DIALOG_WIDTH: f32 = 520.0;

pub fn muted(text: &str) -> egui::RichText {
    egui::RichText::new(text).color(MUTED).size(12.5)
}

/// A centred dialog with a gold title and rule; the main screen is dimmed behind it.
/// The gap kept between a dialog and the top and bottom of the window.
const DIALOG_MARGIN: f32 = 24.0;

/// Room below a dialog's scrolling list for its footer buttons.
const FOOTER_ROOM: f32 = 64.0;

/// A scrolling list that takes the height the dialog has left, with its scrollbar always shown.
pub fn list(ui: &mut egui::Ui) -> egui::ScrollArea {
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - FOOTER_ROOM).max(120.0))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
}

pub fn dialog(context: &egui::Context, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .min_width(DIALOG_WIDTH)
        .max_width(DIALOG_WIDTH)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .max_height(context.content_rect().height() - 2.0 * DIALOG_MARGIN)
        .frame(
            egui::Frame::window(&context.global_style())
                .fill(INK)
                .stroke(egui::Stroke::new(1.0, GOLD_DIM))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(20, 16)),
        )
        .show(context, |ui| {
            ui.label(egui::RichText::new(title).size(22.0).color(GOLD));
            ui.add_space(2.0);
            let line = ui.available_rect_before_wrap();
            ui.painter()
                .hline(line.x_range(), line.top(), egui::Stroke::new(1.0, GOLD_DIM));
            ui.add_space(8.0);
            contents(ui);
        });
}

pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(11.5)
            .color(GOLD)
            .strong(),
    );
}

/// One setting: its name and a short explanation on the left, its control on the right.
pub fn row(ui: &mut egui::Ui, title: &str, detail: &str, control: impl FnOnce(&mut egui::Ui)) {
    titled_row(ui, title, None, detail, control);
}

/// A [`row`] whose title is followed by a muted `note`, such as a version.
pub fn titled_row(
    ui: &mut egui::Ui,
    title: &str,
    note: Option<&str>,
    detail: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(33.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(egui::RichText::new(title).color(PARCHMENT));
                    if let Some(note) = note {
                        ui.label(muted(note));
                    }
                });
                ui.add(egui::Label::new(muted(detail)).truncate());
            });
        });
    });
}

/// The buttons along the bottom edge, from the right.
pub fn footer(ui: &mut egui::Ui, buttons: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(12.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), buttons);
}

pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(PARCHMENT).strong())
            .fill(RED)
            .stroke(egui::Stroke::new(1.0, GOLD_DIM))
            .min_size(Vec2::new(96.0, 30.0)),
    )
}

pub fn secondary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(0.0, 30.0)))
}

/// A path in a recessed box.
pub fn path(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(14, 10, 6))
        .corner_radius(4.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .monospace()
                        .size(12.0)
                        .color(MUTED),
                )
                .truncate(),
            );
        });
}

/// An on/off switch, drawn instead of a checkbox.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(40.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let position = ui.ctx().animate_bool_responsive(response.id, *on);
    let radius = rect.height() / 2.0;
    let track = if *on { RED } else { LEATHER };
    let stroke = if response.hovered() { GOLD } else { GOLD_DIM };
    ui.painter().rect(
        rect,
        radius,
        track,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), position);
    ui.painter().circle_filled(
        Pos2::new(x, rect.center().y),
        radius - 4.0,
        if *on { PARCHMENT } else { MUTED },
    );
    response
}

/// Applies the launcher's colours and text sizes to `context`.
pub fn apply(context: &egui::Context) {
    let mut style = (*context.global_style()).clone();
    use egui::{FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(22.0)),
        (TextStyle::Body, FontId::proportional(14.5)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
    style.spacing.button_padding = Vec2::new(12.0, 5.0);
    style.spacing.item_spacing = Vec2::new(8.0, 4.0);
    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::dark();
    visuals.weak_text_color = Some(MUTED);
    visuals.window_fill = INK;
    visuals.panel_fill = INK;
    visuals.extreme_bg_color = Color32::from_rgb(14, 10, 6);
    visuals.hyperlink_color = GOLD;
    visuals.selection.bg_fill = RED;
    visuals.selection.stroke = egui::Stroke::new(1.0, GOLD);
    visuals.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(160),
    };
    let widgets = &mut visuals.widgets;
    for (widget, fill, stroke) in [
        (&mut widgets.inactive, LEATHER, GOLD_DIM),
        (&mut widgets.hovered, Color32::from_rgb(78, 56, 36), GOLD),
        (&mut widgets.active, Color32::from_rgb(98, 70, 44), GOLD),
        (&mut widgets.open, Color32::from_rgb(78, 56, 36), GOLD),
    ] {
        widget.weak_bg_fill = fill;
        widget.bg_fill = fill;
        widget.bg_stroke = egui::Stroke::new(1.0, stroke);
        widget.fg_stroke = egui::Stroke::new(1.0, PARCHMENT);
        widget.corner_radius = egui::CornerRadius::same(4);
    }
    // Labels take their colour from the non-interactive widget style.
    widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, PARCHMENT);
    context.set_global_style(style);
}
