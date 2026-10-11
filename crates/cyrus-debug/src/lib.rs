use bevy::prelude::*;
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiPreUpdateSet, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};
use bevy_inspector_egui::{DefaultInspectorConfigPlugin, bevy_inspector};
use cyrus_game::{
    fidelity::{Area, Fidelity, Mode},
    fog::RenderDistance,
    player::{FlycamSettings, Player},
};

pub struct DebugMenuPlugin;

#[derive(Resource, Default)]
struct DebugMenu {
    open: bool,
    inspector_open: bool,
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
        app.add_plugins((cyrus_app::EguiPlugin, DefaultInspectorConfigPlugin))
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
                    world_inspector_window.run_if(|menu: Res<DebugMenu>| menu.inspector_open),
                )
                    .chain(),
            );

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

// The menu edits independent game resources and reads the player pose.
#[allow(clippy::too_many_arguments)]
fn debug_menu(
    mut contexts: EguiContexts,
    mut menu: ResMut<DebugMenu>,
    mut egui_settings: ResMut<EguiGlobalSettings>,
    mut world: ResMut<WorldPicker>,
    mut flycam: ResMut<FlycamSettings>,
    mut fidelity: ResMut<Fidelity>,
    mut distance: ResMut<RenderDistance>,
    player: Query<&Transform, With<Player>>,
) -> Result {
    let player_position = player.single().ok().map(|transform| transform.translation);

    let window = egui::Window::new("cyrus debug").vscroll(true);
    window.show(contexts.ctx_mut()?, |ui| {
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

        if let Some([near, original]) = distance.original {
            let mut far = distance.effective(&fidelity).unwrap()[1];
            ui.horizontal(|ui| {
                ui.label("Render distance");
                if ui
                    .add(
                        egui::DragValue::new(&mut far)
                            .range((near + 1.0)..=f32::MAX)
                            .speed(20.0),
                    )
                    .on_hover_text("Engine units. Editing switches Fog to Modern.")
                    .changed()
                {
                    distance.override_far = Some(far);
                    fidelity.set(Area::Fog, Mode::Modern);
                }
                if ui.button("Original").clicked() {
                    distance.override_far = None;
                    fidelity.set(Area::Fog, Mode::Faithful);
                }
            });
            ui.label(format!(
                "Original render distance: {original:.0} game units"
            ));
        }

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
        fidelity_control(ui, &mut fidelity, Area::Water, "Water");
        fidelity_control(ui, &mut fidelity, Area::Fog, "Fog");
        fidelity_control(ui, &mut fidelity, Area::Sky, "Sky");

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

fn world_inspector_window(world: &mut World) {
    let Ok(context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single(world)
    else {
        return;
    };
    let mut context = context.clone();
    let mut open = world.resource::<DebugMenu>().inspector_open;
    egui::Window::new("World Inspector")
        .open(&mut open)
        .default_size((420.0, 500.0))
        .show(context.get_mut(), |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                bevy_inspector::ui_for_world(world, ui);
            });
        });
    let mut menu = world.resource_mut::<DebugMenu>();
    menu.inspector_open = open;
    let absorb_input = menu.open || menu.inspector_open;
    world
        .resource_mut::<EguiGlobalSettings>()
        .enable_absorb_bevy_input_system = absorb_input;
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
    use super::*;

    #[test]
    fn world_options_are_sorted_and_unique() {
        assert_eq!(
            parse_worlds("START,2,ISLAND,START"),
            ["2", "ISLAND", "START"]
        );
    }

    #[test]
    fn world_inspector_renders_the_full_world_tree() {
        let mut app = App::new();
        app.add_plugins(DefaultInspectorConfigPlugin)
            .register_type::<Name>()
            .register_type::<Transform>()
            .insert_resource(DebugMenu {
                open: false,
                inspector_open: true,
            })
            .init_resource::<EguiGlobalSettings>();
        let mut context = EguiContext::default();
        app.world_mut().spawn((context.clone(), PrimaryEguiContext));
        let entity = app
            .world_mut()
            .spawn((Name::new("Inspector test entity"), Transform::IDENTITY))
            .id();
        let mut output = egui::FullOutput::default();
        // The first pass measures a newly opened egui window before painting it.
        for _ in 0..2 {
            output = context.get_mut().run_ui(egui::RawInput::default(), |_| {
                world_inspector_window(app.world_mut());
            });
            // This headless test has no renderer to upload font textures.
            output.textures_delta.clear();
        }
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for label in ["World Inspector", "Entities", "Resources", "Assets"] {
            assert!(labels.contains(&label), "missing {label}: {labels:?}");
        }
        assert!(
            labels
                .iter()
                .any(|label| label.contains("Inspector test entity"))
        );
        assert_eq!(
            app.world().get::<Transform>(entity),
            Some(&Transform::IDENTITY)
        );
        assert!(
            app.world()
                .resource::<EguiGlobalSettings>()
                .enable_absorb_bevy_input_system
        );
    }

    #[test]
    fn f1_keeps_the_independent_inspector_open() {
        let mut app = App::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::F1);
        app.insert_resource(keys)
            .insert_resource(DebugMenu {
                open: true,
                inspector_open: true,
            })
            .init_resource::<EguiGlobalSettings>()
            .add_systems(Update, toggle_menu);
        app.update();
        let menu = app.world().resource::<DebugMenu>();
        assert!(!menu.open);
        assert!(menu.inspector_open);
        assert!(
            app.world()
                .resource::<EguiGlobalSettings>()
                .enable_absorb_bevy_input_system
        );
    }
}
