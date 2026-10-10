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
    inspector_open: bool,
    selected_entity: Option<Entity>,
}

#[derive(Resource, Default)]
struct WorldPicker {
    current: String,
    selected: String,
    options: Vec<String>,
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
                (
                    debug_menu.run_if(|menu: Res<DebugMenu>| menu.open),
                    entity_inspector_window.run_if(|menu: Res<DebugMenu>| menu.inspector_open),
                )
                    .chain(),
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
        app.add_systems(Startup, initialize_native_worlds)
            .add_systems(Update, update_native_world);
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
        egui.enable_absorb_bevy_input_system = menu.open || menu.inspector_open;
    }
}

fn debug_menu(
    mut contexts: EguiContexts,
    mut menu: ResMut<DebugMenu>,
    mut egui_settings: ResMut<EguiGlobalSettings>,
    mut world: ResMut<WorldPicker>,
    mut flycam: ResMut<FlycamSettings>,
    mut fidelity: ResMut<Fidelity>,
    player: Query<&Transform, With<Player>>,
) -> Result {
    let player_position = player.single().ok().map(|transform| transform.translation);

    egui::Window::new("cyrus debug").show(contexts.ctx_mut()?, |ui| {
        ui.label("F1 toggles this menu");
        ui.separator();
        ui.label(format!("World: {}", world.current));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("world picker")
                .selected_text(&world.selected)
                .show_ui(ui, |ui| {
                    for index in 0..world.options.len() {
                        let option = world.options[index].clone();
                        ui.selectable_value(&mut world.selected, option.clone(), option);
                    }
                });
            if ui.button("Load").clicked() && !world.selected.is_empty() {
                world.pending = Some(world.selected.clone());
            }
        });
        if let Some(position) = player_position {
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
        ui.horizontal(|ui| {
            for (mode, label) in [
                (Mode::Faithful, "All faithful"),
                (Mode::Modern, "All modern"),
            ] {
                if ui.button(label).clicked() {
                    for area in Area::ALL {
                        fidelity.set(area, mode);
                    }
                }
            }
        });
        fidelity_control(
            ui,
            &mut fidelity,
            Area::PlayerController,
            "Player controller",
        );
        fidelity_control(ui, &mut fidelity, Area::Camera, "Camera");
        fidelity_control(ui, &mut fidelity, Area::Graphics, "Graphics");

        ui.separator();
        let inspector_label = if menu.inspector_open {
            "Close inspector"
        } else {
            "Open inspector"
        };
        if ui.button(inspector_label).clicked() {
            menu.inspector_open = !menu.inspector_open;
            egui_settings.enable_absorb_bevy_input_system = menu.open || menu.inspector_open;
        }
    });
    Ok(())
}

// shortcut: inspect spatial entities only; use bevy-inspector-egui once it supports Bevy 0.20.
#[allow(clippy::type_complexity)]
fn entity_inspector_window(
    mut contexts: EguiContexts,
    mut menu: ResMut<DebugMenu>,
    mut egui_settings: ResMut<EguiGlobalSettings>,
    mut spatial: ParamSet<(
        Query<(Entity, Option<&Name>), With<Transform>>,
        Query<&mut Transform>,
    )>,
) -> Result {
    let entities = spatial
        .p0()
        .iter()
        .map(|(entity, name)| {
            (
                entity,
                name.map_or_else(|| entity.to_string(), |name| name.as_str().to_owned()),
            )
        })
        .collect::<Vec<_>>();
    let mut open = menu.inspector_open;
    egui::Window::new("Entity inspector")
        .open(&mut open)
        .show(contexts.ctx_mut()?, |ui| {
            entity_inspector(ui, &entities, &mut menu.selected_entity, &mut spatial.p1());
        });
    menu.inspector_open = open;
    egui_settings.enable_absorb_bevy_input_system = menu.open || menu.inspector_open;
    Ok(())
}

fn entity_inspector(
    ui: &mut egui::Ui,
    entities: &[(Entity, String)],
    selected: &mut Option<Entity>,
    transforms: &mut Query<&mut Transform>,
) {
    let selected_name = selected
        .and_then(|selected| {
            entities
                .iter()
                .find(|(entity, _)| *entity == selected)
                .map(|(_, name)| name.as_str())
        })
        .unwrap_or("Select an entity");
    egui::ComboBox::from_id_salt("inspected entity")
        .selected_text(selected_name)
        .show_ui(ui, |ui| {
            for (entity, name) in entities {
                ui.selectable_value(selected, Some(*entity), name);
            }
        });

    let Some(entity) = *selected else {
        return;
    };
    let Ok(mut transform) = transforms.get_mut(entity) else {
        *selected = None;
        return;
    };
    ui.label(entity.to_string());
    ui.horizontal(|ui| {
        ui.label("Translation");
        ui.add(egui::DragValue::new(&mut transform.translation.x));
        ui.add(egui::DragValue::new(&mut transform.translation.y));
        ui.add(egui::DragValue::new(&mut transform.translation.z));
    });
    ui.horizontal(|ui| {
        ui.label("Scale");
        ui.add(egui::DragValue::new(&mut transform.scale.x).speed(0.01));
        ui.add(egui::DragValue::new(&mut transform.scale.y).speed(0.01));
        ui.add(egui::DragValue::new(&mut transform.scale.z).speed(0.01));
    });
    let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
    let mut rotation = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    ui.horizontal(|ui| {
        ui.label("Rotation °");
        ui.add(egui::DragValue::new(&mut rotation.x));
        ui.add(egui::DragValue::new(&mut rotation.y));
        ui.add(egui::DragValue::new(&mut rotation.z));
    });
    transform.rotation = Quat::from_euler(
        EulerRot::XYZ,
        rotation.x.to_radians(),
        rotation.y.to_radians(),
        rotation.z.to_radians(),
    );
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
fn initialize_native_worlds(mut picker: ResMut<WorldPicker>) {
    picker.options = std::fs::read_dir(cyrus_game::settings::Settings::cache_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("glb"))
                .then(|| path.file_stem()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect();
    picker.options.sort();
    picker.options.dedup();
}

#[cfg(not(target_arch = "wasm32"))]
fn update_native_world(
    mut picker: ResMut<WorldPicker>,
    current: Res<cyrus_game::scene::CurrentScene>,
    mut request: ResMut<cyrus_game::scene::SceneRequest>,
) {
    picker.current.clone_from(&current.name);
    if picker.selected.is_empty() {
        picker.selected.clone_from(&request.0);
    }
    if let Some(requested) = picker.pending.take() {
        request.0 = requested;
    }
}

#[cfg(target_arch = "wasm32")]
fn initialize_web_world(mut picker: ResMut<WorldPicker>) {
    let current = web_scene();
    picker.current.clone_from(&current);
    picker.selected.clone_from(&current);
    picker.options = web_worlds();
    if !picker.options.contains(&current) {
        picker.options.push(current);
    }
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

#[cfg(target_arch = "wasm32")]
fn web_worlds() -> Vec<String> {
    let worlds = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.query_selector("meta[name='cyrus-worlds']").ok()?)
        .and_then(|element| element.get_attribute("content"))
        .unwrap_or_default();
    parse_worlds(&worlds)
}

#[cfg(any(target_arch = "wasm32", test))]
fn parse_worlds(worlds: &str) -> Vec<String> {
    let mut worlds = worlds
        .split(',')
        .filter(|world| !world.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    worlds.sort();
    worlds.dedup();
    worlds
}

#[cfg(test)]
mod tests {
    use super::parse_worlds;

    #[test]
    fn world_options_are_sorted_and_unique() {
        assert_eq!(
            parse_worlds("START,2,ISLAND,START"),
            ["2", "ISLAND", "START"]
        );
    }
}
