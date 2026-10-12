//! Shared window, cache source, egui and progress presentation for Cyrus applications.
use bevy::{asset::io::AssetSourceBuilder, prelude::*};

pub use bevy_egui;
pub mod launch;
/// Register before AssetPlugin; native paths are independent of the executable location.
pub fn register_cache(app: &mut App) -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let path = {
        let path = std::path::absolute(cyrus_assets::CACHE_DIR)?;
        std::fs::create_dir_all(&path)?;
        path.to_str()
            .ok_or_else(|| anyhow::anyhow!("cache path is not UTF-8"))?
            .to_owned()
    };
    #[cfg(target_arch = "wasm32")]
    let path = browser_source("cyrus-cache")?;
    app.register_asset_source("cache", AssetSourceBuilder::platform_default(&path, None));
    Ok(())
}

pub fn window(title: &str) -> WindowPlugin {
    WindowPlugin {
        primary_window: Some(Window {
            title: title.into(),
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }
}

/// Centralizes egui's renderer setup, including the Bevy 0.20 WebGL compatibility fix.
pub struct EguiPlugin;
impl Plugin for EguiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy_egui::EguiPlugin::default());
        #[cfg(target_arch = "wasm32")]
        if let bevy::shader::Source::Wesl(source) = &mut app
            .world_mut()
            .resource_mut::<Assets<bevy::shader::Shader>>()
            .get_mut(&bevy_egui::render::EGUI_SHADER_HANDLE)
            .expect("egui installs its shader")
            .source
        {
            *source = source.replace("enable wgpu_binding_array;", "").into();
        }
    }
}

/// Progress has no application-state transition or filesystem dependency.
#[derive(Resource, Default, Clone)]
pub struct LoadingProgress {
    pub visible: bool,
    pub label: String,
    pub fraction: Option<f32>,
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct LoadingUi;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct Bar;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct Label;
pub struct LoadingOverlayPlugin;
impl Plugin for LoadingOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadingProgress>()
            .add_systems(PostUpdate, loading);
    }
}
#[cfg(target_arch = "wasm32")]
fn loading(progress: Res<LoadingProgress>) {
    if !progress.is_changed() {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(root) = document.get_element_by_id("loading") {
        let _ = root.set_attribute("style", if progress.visible { "" } else { "display:none" });
    }
    if let Some(label) = document.get_element_by_id("label") {
        label.set_text_content(Some(&progress.label));
    }
    if let Some(fill) = document.get_element_by_id("fill") {
        let _ = fill.set_attribute(
            "class",
            if progress.fraction.is_none() {
                "indeterminate"
            } else {
                ""
            },
        );
        let _ = fill.set_attribute(
            "style",
            &format!(
                "width:{}%",
                progress.fraction.unwrap_or(0.).clamp(0., 1.) * 100.
            ),
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn loading(
    mut commands: Commands,
    progress: Res<LoadingProgress>,
    time: Res<Time>,
    roots: Query<Entity, With<LoadingUi>>,
    mut bars: Query<&mut Node, With<Bar>>,
    mut labels: Query<&mut Text, With<Label>>,
) {
    if !progress.is_changed() && !(progress.visible && progress.fraction.is_none()) {
        return;
    }
    if !progress.visible {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        return;
    }
    if roots.is_empty() {
        commands.spawn((
            LoadingUi,
            GlobalZIndex(100),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            children![
                (
                    Label,
                    Text::new(&progress.label),
                    TextFont {
                        font_size: FontSize::Px(18.),
                        ..default()
                    }
                ),
                (
                    Node {
                        width: px(400),
                        height: px(16),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    children![(
                        Bar,
                        Node {
                            width: percent(
                                progress.fraction.map_or(25., |fraction| fraction * 100.)
                            ),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(Color::WHITE)
                    )]
                )
            ],
        ));
    } else {
        for mut bar in &mut bars {
            bar.width = percent(progress.fraction.map_or(25., |fraction| fraction * 100.));
            bar.margin.left = percent(if progress.fraction.is_none() {
                (time.elapsed_secs() * 0.75).sin().mul_add(0.5, 0.5) * 75.
            } else {
                0.
            });
        }
        for mut label in &mut labels {
            label.0.clone_from(&progress.label);
        }
    }
}

/// Validated HTTP source installed by the shared browser bootstrap.
#[cfg(target_arch = "wasm32")]
pub fn browser_source(name: &str) -> anyhow::Result<String> {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| {
            document
                .query_selector(&format!("meta[name={name}]"))
                .ok()
                .flatten()
        })
        .and_then(|element| element.get_attribute("content"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("select your game files before starting the application"))
}
