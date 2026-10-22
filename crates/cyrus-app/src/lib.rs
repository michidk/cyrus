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

/// Parse a `WIDTHxHEIGHT` window size; both must be positive.
fn parse_window_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;
    let (width, height) = (width.parse().ok()?, height.parse().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// The shared primary window. `CYRUS_WINDOW_SIZE=WIDTHxHEIGHT` replaces the default size, so a
/// software-rendered run (for example in the dev container) can keep a playable frame rate.
pub fn window(title: &str) -> WindowPlugin {
    let mut window = Window {
        title: title.into(),
        fit_canvas_to_parent: true,
        ..default()
    };
    if let Some(size) = std::env::var("CYRUS_WINDOW_SIZE")
        .ok()
        .as_deref()
        .and_then(parse_window_size)
    {
        window.resolution = size.into();
    }
    WindowPlugin {
        primary_window: Some(window),
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

#[cfg(test)]
mod tests {
    use super::parse_window_size;

    #[test]
    fn window_size_needs_two_positive_integers() {
        assert_eq!(parse_window_size("640x480"), Some((640, 480)));
        for bad in ["", "640", "0x480", "640x0", "axb", "640x480x1", "-1x4"] {
            assert_eq!(parse_window_size(bad), None, "{bad}");
        }
    }
}
