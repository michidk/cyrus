use bevy::prelude::*;
use bevy::{
    camera::{CameraOutputMode, visibility::RenderLayers},
    input::InputSystems,
    render::render_resource::BlendState,
};
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiPreUpdateSet, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};
use bevy_inspector_egui::{DefaultInspectorConfigPlugin, bevy_inspector};
use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;
use cyrus_game::player::Player;
use cyrus_render::camera::FlycamSettings;
use cyrus_render::fog::RenderDistance;

pub struct DebugMenuPlugin;

#[derive(Resource, Default)]
struct DebugMenu {
    open: bool,
    inspector_open: bool,
}

#[derive(Resource, Default)]
pub struct WorldPicker {
    pub current: String,
    pub selected: String,
    pub options: Vec<String>,
    pub pending: Option<String>,
}

impl Plugin for DebugMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((cyrus_app::EguiPlugin, DefaultInspectorConfigPlugin))
            .init_resource::<DebugMenu>()
            .init_resource::<cyrus_game::cheats::CheatTransfer>()
            .configure_sets(
                PreUpdate,
                cyrus_game::console::ConsoleInput
                    .after(toggle_menu)
                    .before(EguiPreUpdateSet::ProcessInput),
            )
            .init_resource::<WorldPicker>()
            .add_systems(PreStartup, setup_debug_camera)
            .add_systems(
                PreUpdate,
                toggle_menu
                    .after(InputSystems)
                    .before(EguiPreUpdateSet::ProcessInput),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    debug_menu.run_if(|menu: Res<DebugMenu>| menu.open),
                    world_inspector_window.run_if(|menu: Res<DebugMenu>| menu.inspector_open),
                )
                    .chain(),
            );
        app.world_mut()
            .resource_mut::<EguiGlobalSettings>()
            .auto_create_primary_context = false;
    }
}

fn setup_debug_camera(mut commands: Commands) {
    // Startup and world cameras are replaced; the debug context must outlive both.
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<DebugMenu>,
    mut console: Option<ResMut<cyrus_game::console::Console>>,
    mut egui: ResMut<EguiGlobalSettings>,
) {
    if keys.just_pressed(KeyCode::F1) && !console.as_ref().is_some_and(|console| console.open) {
        menu.open = !menu.open;
        egui.enable_absorb_bevy_input_system = menu.open || menu.inspector_open;
    }
    if let Some(console) = console.as_mut()
        && console.blocked != (menu.open || menu.inspector_open)
    {
        console.blocked = menu.open || menu.inspector_open;
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
    mut transfers: ResMut<cyrus_game::cheats::CheatTransfer>,
    player: Query<&Transform, With<Player>>,
) -> Result {
    let player_position = player.single().ok().map(|transform| transform.translation);

    let window = egui::Window::new("cyrus debug").vscroll(true);
    window.show(contexts.ctx_mut()?, |ui| {
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

        if transfers.available {
            ui.collapsing("Cheat files", |ui| {
                ui.label("F12 opens the cheat console while in a world.");
                if ui.button("Import REDGUARD.CHT").clicked() {
                    transfers.action = Some(cyrus_game::cheats::CheatFileAction::Import);
                }
                if ui.button("Export REDGUARD.CHT").clicked() {
                    transfers.action = Some(cyrus_game::cheats::CheatFileAction::Export);
                }
                if let Some(status) = &transfers.status {
                    ui.label(status);
                }
            });
        }
        ui.checkbox(&mut flycam.enabled, "Flycam");
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
        fidelity_control(
            ui,
            &mut fidelity,
            Area::Lighting,
            "Lighting (Modern uses Faithful)",
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f1_survives_replacing_the_startup_camera() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::window::WindowPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            bevy::image::ImagePlugin::default(),
        ))
        .init_asset::<bevy::shader::Shader>()
        .add_plugins(DebugMenuPlugin)
        .init_resource::<FlycamSettings>()
        .init_resource::<Fidelity>()
        .init_resource::<RenderDistance>();
        let startup_camera = app.world_mut().spawn(Camera2d).id();
        app.finish();
        app.cleanup();
        app.update();
        let context_entity = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryEguiContext>>()
            .single(app.world())
            .expect("debug overlay has a primary context");
        assert_ne!(context_entity, startup_camera);
        app.world_mut().despawn(startup_camera);
        app.world_mut().spawn(Camera3d::default());
        for expected_open in [true, false, true] {
            app.world_mut()
                .write_message(bevy::input::keyboard::KeyboardInput {
                    key_code: KeyCode::F1,
                    logical_key: bevy::input::keyboard::Key::F1,
                    state: bevy::input::ButtonState::Pressed,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            app.update();
            assert_eq!(app.world().resource::<DebugMenu>().open, expected_open);
            let mut context = app
                .world()
                .get::<EguiContext>(context_entity)
                .unwrap()
                .clone();
            let mut output = egui::FullOutput::default();
            for _ in 0..2 {
                output = context.get_mut().run_ui(egui::RawInput::default(), |_| {
                    app.world_mut().run_schedule(EguiPrimaryContextPass);
                });
                output.textures_delta.clear();
            }
            assert_eq!(
                output.shapes.iter().any(|shape| matches!(
                    &shape.shape,
                    egui::epaint::Shape::Text(text) if text.galley.job.text == "cyrus debug"
                )),
                expected_open,
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::F1);
        }
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
