use bevy::prelude::*;
#[cfg(target_arch = "wasm32")]
use bevy::shader::Source;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPreUpdateSet, EguiPrimaryContextPass, egui,
};
use cyrus_game::{
    fidelity::{Area, Fidelity, Mode},
    player::{FlycamSettings, Player},
};

pub struct DebugMenuPlugin;

#[derive(Resource, Default)]
struct DebugMenu {
    open: bool,
}

#[derive(Resource, Default)]
struct WorldPicker {
    current: String,
    input: String,
    pending: Option<String>,
}

impl Plugin for DebugMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .init_resource::<DebugMenu>()
            .init_resource::<WorldPicker>()
            .add_systems(
                PreUpdate,
                toggle_menu.before(EguiPreUpdateSet::ProcessInput),
            )
            .add_systems(
                EguiPrimaryContextPass,
                debug_menu.run_if(|menu: Res<DebugMenu>| menu.open),
            );

        #[cfg(target_arch = "wasm32")]
        if let Source::Wesl(source) = &mut app
            .world_mut()
            .resource_mut::<Assets<bevy::shader::Shader>>()
            .get_mut(&bevy_egui::render::EGUI_SHADER_HANDLE)
            .expect("bevy_egui shader is loaded by EguiPlugin")
            .source
        {
            // WebGL cannot parse this bindless-only extension, even when bindless mode is disabled.
            *source = source.replace("enable wgpu_binding_array;", "").into();
        }

        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, update_native_world);
        #[cfg(target_arch = "wasm32")]
        app.add_systems(Startup, initialize_web_world)
            .add_systems(Update, update_web_world);
    }
}

fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<DebugMenu>,
    mut egui: ResMut<EguiGlobalSettings>,
) {
    if keys.just_pressed(KeyCode::F1) {
        menu.open = !menu.open;
        egui.enable_absorb_bevy_input_system = menu.open;
    }
}

fn debug_menu(
    mut contexts: EguiContexts,
    mut world: ResMut<WorldPicker>,
    player: Query<&Transform, With<Player>>,
    mut flycam: ResMut<FlycamSettings>,
    mut fidelity: ResMut<Fidelity>,
) -> Result {
    egui::Window::new("cyrus debug").show(contexts.ctx_mut()?, |ui| {
        ui.label("F1 toggles this menu");
        ui.separator();
        ui.label(format!("World: {}", world.current));
        ui.horizontal(|ui| {
            ui.label("WORLD.INI ID or map");
            ui.text_edit_singleline(&mut world.input);
            if ui.button("Load").clicked() {
                let requested = world.input.trim();
                if !requested.is_empty() {
                    world.pending = Some(requested.to_owned());
                }
            }
        });
        if let Ok(transform) = player.single() {
            let position = transform.translation;
            ui.label(format!(
                "Player: {:.1}, {:.1}, {:.1}",
                position.x, position.y, position.z
            ));
        }

        ui.checkbox(&mut flycam.enabled, "Flycam (leave player in place)");
        if flycam.enabled {
            ui.label("WASD, right mouse to look, Space/Shift for up/down");
        } else {
            ui.label("Player movement and follow camera are not implemented yet.");
        }
        ui.horizontal(|ui| {
            ui.label("Flycam speed");
            ui.add(
                egui::DragValue::new(&mut flycam.speed)
                    .range(0.0..=f32::MAX)
                    .speed(1.0),
            );
        });

        ui.separator();
        ui.heading("Fidelity");
        fidelity_control(
            ui,
            &mut fidelity,
            Area::PlayerController,
            "Player controller",
        );
        fidelity_control(ui, &mut fidelity, Area::Camera, "Camera");
        fidelity_control(ui, &mut fidelity, Area::Graphics, "Graphics");
    });
    Ok(())
}

fn fidelity_control(ui: &mut egui::Ui, fidelity: &mut Fidelity, area: Area, label: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        let current = fidelity.mode(area);
        for (mode, label) in [(Mode::Faithful, "Faithful"), (Mode::Modern, "Modern")] {
            if ui.selectable_label(current == mode, label).clicked() {
                fidelity.set(area, mode);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn update_native_world(
    mut picker: ResMut<WorldPicker>,
    current: Res<cyrus_game::scene::CurrentScene>,
    mut request: ResMut<cyrus_game::scene::SceneRequest>,
) {
    picker.current.clone_from(&current.name);
    if picker.input.is_empty() {
        picker.input.clone_from(&request.0);
    }
    if let Some(requested) = picker.pending.take() {
        request.0 = requested;
    }
}

#[cfg(target_arch = "wasm32")]
fn initialize_web_world(mut picker: ResMut<WorldPicker>) {
    let current = web_scene();
    picker.current.clone_from(&current);
    picker.input = current;
}

#[cfg(target_arch = "wasm32")]
fn update_web_world(mut picker: ResMut<WorldPicker>) {
    let Some(requested) = picker.pending.take() else {
        return;
    };
    if let Some(window) = web_sys::window()
        && let Err(error) = window.location().set_search(&format!("?scene={requested}"))
    {
        error!("load world {requested}: {error:?}");
    }
}

#[cfg(target_arch = "wasm32")]
fn web_scene() -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .find_map(|part| part.strip_prefix("scene="))
                .map(str::to_ascii_uppercase)
        })
        .unwrap_or_else(|| "ISLAND".into())
}
