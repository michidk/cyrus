//! The effects panel: the parts of the modern lighting and its optional effects.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use cyrus_game::fidelity::{Area, Fidelity, Mode};
use cyrus_render::fog::{Effect, ModernEffects, Tonemapper};

use crate::DebugMenu;

pub(crate) fn effects_window(
    mut contexts: EguiContexts,
    mut menu: ResMut<DebugMenu>,
    mut fidelity: ResMut<Fidelity>,
    effects: Option<ResMut<ModernEffects>>,
) -> Result {
    let mut open = menu.effects_open;
    egui::Window::new("Effects")
        .id(egui::Id::new("Effects"))
        .open(&mut open)
        .default_width(380.0)
        .show(contexts.ctx_mut()?, |ui| {
            let Some(mut effects) = effects else {
                ui.label("Modern lighting is not available in this build.");
                return;
            };
            if fidelity.mode(Area::Lighting) != Mode::Modern {
                ui.label("The effects need Modern lighting.");
                if ui.button("Switch lighting to Modern").clicked() {
                    fidelity.set(Area::Lighting, Mode::Modern);
                }
                ui.separator();
            }
            ui.heading("Lighting");
            for effect in Effect::LIGHTING {
                ui.checkbox(effects.switch(effect), effect.label());
            }
            ui.separator();
            ui.heading("Effects");
            for effect in Effect::ALL
                .into_iter()
                .filter(|effect| !Effect::LIGHTING.contains(effect))
            {
                ui.checkbox(effects.switch(effect), effect.label());
                ui.indent(effect.label(), |ui| settings(ui, &mut effects, effect));
            }
            ui.separator();
            if ui.button("Reset to defaults").clicked() {
                *effects = ModernEffects::default();
            }
        });
    menu.effects_open = open;
    Ok(())
}

/// The settings beneath one effect's switch, greyed out while it is off.
fn settings(ui: &mut egui::Ui, effects: &mut ModernEffects, effect: Effect) {
    let enabled = *effects.switch(effect);
    ui.add_enabled_ui(enabled, |ui| match effect {
        Effect::Grading => {
            egui::ComboBox::from_label("Tonemapper")
                .selected_text(effects.tonemapper.label())
                .show_ui(ui, |ui| {
                    for tonemapper in Tonemapper::ALL {
                        ui.selectable_value(
                            &mut effects.tonemapper,
                            tonemapper,
                            tonemapper.label(),
                        );
                    }
                });
            ui.add(egui::Slider::new(&mut effects.exposure, -3.0..=3.0).text("Exposure (stops)"));
            ui.add(egui::Slider::new(&mut effects.saturation, 0.0..=2.0).text("Saturation"));
            ui.add(egui::Slider::new(&mut effects.contrast, 0.8..=1.2).text("Contrast"));
        }
        Effect::GodRays => {
            ui.add(
                egui::Slider::new(&mut effects.god_ray_density, 0.0001..=0.02)
                    .logarithmic(true)
                    .text("Haze density"),
            );
        }
        Effect::DepthOfField => {
            ui.add(
                egui::Slider::new(&mut effects.aperture, 0.02..=2.0)
                    .logarithmic(true)
                    .text("Aperture (f-stops)"),
            )
            .on_hover_text("Lower blurs more. Focus follows the player.");
        }
        Effect::AutoExposure => {
            ui.label("Adapts within 1 stop darker to 1.5 stops brighter.");
        }
        Effect::WaterReflections => {
            ui.label("Reflects what is on screen; the horizon colour elsewhere.");
        }
        Effect::LensFlare | Effect::Sky | Effect::Foliage | Effect::MotionBlur => {}
        Effect::Shadows
        | Effect::AmbientOcclusion
        | Effect::ContactShadows
        | Effect::AntiAliasing
        | Effect::Bloom
        | Effect::ObjectLights
        | Effect::WaterGloss => {}
    });
}
