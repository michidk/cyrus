//! Shared window, cache source, egui and progress presentation for Cyrus applications.
use bevy::{asset::io::AssetSourceBuilder, prelude::*};

pub use bevy_egui;
pub mod launch;
pub mod loading;
#[cfg(not(target_arch = "wasm32"))]
pub mod logging;
#[cfg(not(target_arch = "wasm32"))]
mod native_install;
/// Register before AssetPlugin; native paths are independent of the executable location.
pub fn register_cache(app: &mut App) -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let path = {
        let path = std::path::absolute(cyrus_assets::CACHE_DIR)?;
        path.to_str()
            .ok_or_else(|| anyhow::anyhow!("cache path is not UTF-8"))?
            .to_owned()
    };
    #[cfg(target_arch = "wasm32")]
    let path = browser_source("cyrus-cache")?;
    app.register_asset_source("cache", AssetSourceBuilder::platform_default(&path, None));
    Ok(())
}

/// Bevy's default log filter plus dependency messages that are expected here and numerous:
/// the renderer warns about every empty mesh, which the exported worlds contain by the
/// thousand, and the preservation importer reports each section of each file at info level.
/// `RUST_LOG` can still turn either back on.
pub fn log_filter() -> String {
    format!(
        "{},bevy_render::mesh::allocator=error,rgpre=warn",
        bevy::log::DEFAULT_FILTER
    )
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
