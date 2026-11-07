//! The day/night panel: the modern lighting's time of day.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use cyrus_game::fidelity::{Area, Fidelity, Mode};
use cyrus_render::fog::DayNight;

use crate::DebugMenu;

pub(crate) fn day_night_window(
    mut contexts: EguiContexts,
    mut menu: ResMut<DebugMenu>,
    mut fidelity: ResMut<Fidelity>,
    cycle: Option<ResMut<DayNight>>,
) -> Result {
    let mut open = menu.day_night_open;
    egui::Window::new("Day/night")
        .id(egui::Id::new("Day/night"))
        .open(&mut open)
        .default_width(300.0)
        .show(contexts.ctx_mut()?, |ui| {
            let Some(mut cycle) = cycle else {
                ui.label("Modern lighting is not available in this build.");
                return;
            };
            if fidelity.mode(Area::Lighting) != Mode::Modern {
                ui.label("The cycle needs Modern lighting.");
                if ui.button("Switch lighting to Modern").clicked() {
                    fidelity.set(Area::Lighting, Mode::Modern);
                }
                ui.separator();
            }
            ui.checkbox(&mut cycle.enabled, "Day/night cycle")
                .on_hover_text("Off keeps the world's installed sun.");
            ui.add_enabled_ui(cycle.enabled, |ui| {
                ui.heading(cycle.clock());
                ui.add(
                    egui::Slider::new(&mut cycle.hour, 0.0..=24.0)
                        .text("Hour")
                        .step_by(0.25),
                )
                .on_hover_text("12:00 is the world's installed sun.");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut cycle.running, "Run");
                    ui.add(
                        egui::DragValue::new(&mut cycle.minutes_per_second)
                            .range(0.0..=1440.0)
                            .speed(1.0)
                            .suffix(" min/s"),
                    )
                    .on_hover_text("In-game minutes per real second.");
                });
                ui.horizontal(|ui| {
                    for (label, hour) in [
                        ("Dawn", 6.0),
                        ("Noon", 12.0),
                        ("Dusk", 18.0),
                        ("Night", 0.0),
                    ] {
                        if ui.button(label).clicked() {
                            cycle.hour = hour;
                        }
                    }
                });
            });
            ui.label("Worlds without an installed sun, such as interiors, are not affected.");
        });
    menu.day_night_open = open;
    Ok(())
}
