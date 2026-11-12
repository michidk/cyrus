//! Shared startup preparation and native streaming cinematic input.
mod export;
pub(crate) mod menu_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod movie_native;
mod splash_data;
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

/// Installed startup flags, shared by native preparation and the browser worker.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupPolicy {
    pub final_version: bool,
    pub ignore_autosave: bool,
}

impl StartupPolicy {
    /// Select the documented startup destination after the host validates its autosave.
    pub fn entry(self, valid_autosave: bool) -> StartupEntry {
        if !self.final_version || self.ignore_autosave {
            StartupEntry::World
        } else if valid_autosave {
            StartupEntry::Book
        } else {
            StartupEntry::Introduction
        }
    }
}

/// Read policy through preservation without preparing presentation assets or writing originals.
pub fn startup_policy(source: &dyn crate::source::ReadAssets) -> anyhow::Result<StartupPolicy> {
    use anyhow::Context;
    let system = rgpre::import::system_ini::SystemIni::parse(&crate::preparation::text(
        source,
        "SYSTEM.INI",
    )?)
    .context("startup: SYSTEM.INI")?;
    let menu =
        rgpre::import::menu_ini::MenuGeneral::parse(&crate::preparation::text(source, "MENU.INI")?)
            .map_err(anyhow::Error::msg)?;
    Ok(StartupPolicy {
        final_version: system
            .debug
            .final_version
            .context("startup: SYSTEM.INI [debug] final_version is missing")?,
        ignore_autosave: menu.ignore_autosave,
    })
}

/// Read installed startup policy without changing the install or preparing presentation assets.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn startup_entry(
    install: &crate::install::GameInstall,
    valid_autosave: bool,
) -> anyhow::Result<StartupEntry> {
    Ok(startup_policy(&crate::native::Files::scan(install, false)?)?.entry(valid_autosave))
}
