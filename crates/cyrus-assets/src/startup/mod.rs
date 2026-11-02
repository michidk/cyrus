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
