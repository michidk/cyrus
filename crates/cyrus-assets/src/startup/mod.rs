//! Shared startup preparation and native streaming cinematic input.
mod dos_raster;
mod export;
pub(crate) mod menu_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod movie_native;
mod raster;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use export::export_startup;
pub(crate) use export::prepare_startup;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use menu_export::export_menu;
pub(crate) use menu_export::prepare_menu;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use movie_native::export_movies;

/// Startup destination from installed flags and the host's validated autosave state.
/// Follows preservation's engine/startup.md startup-entry rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupEntry {
    Book,
    Introduction,
    World,
}

/// Read installed startup policy without changing the install or preparing presentation assets.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn startup_entry(
    install: &crate::install::GameInstall,
    valid_autosave: bool,
) -> anyhow::Result<StartupEntry> {
    use anyhow::Context;
    let source = crate::native::Files::scan(install, false)?;
    let read = |name| -> anyhow::Result<String> {
        let bytes = crate::preparation::read(&source, name)?;
        String::from_utf8(bytes).with_context(|| format!("startup: {name} is not UTF-8"))
    };
    let system = rgpre::import::system_ini::SystemIni::parse(&read("SYSTEM.INI")?)
        .context("startup: SYSTEM.INI")?;
    let menu = rgpre::import::menu_ini::MenuGeneral::parse(&read("MENU.INI")?)
        .map_err(anyhow::Error::msg)?;
    let final_version = system
        .debug
        .final_version
        .context("startup: SYSTEM.INI [debug] final_version is missing")?;
    Ok(if !final_version || menu.ignore_autosave {
        StartupEntry::World
    } else if valid_autosave {
        StartupEntry::Book
    } else {
        StartupEntry::Introduction
    })
}
