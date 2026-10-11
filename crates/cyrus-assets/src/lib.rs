//! Portable validated cache records; optional native install preparation.
pub mod animation;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod animation_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod audio;
pub mod collision;
pub mod environment;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod environment_export;
pub mod hud;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod hud_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod install;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod loading;
pub mod menu;
pub mod movie;
pub mod player;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod player_export;
pub mod preferences;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod scene;
pub mod script;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod script_export;
pub mod sound;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod sound_export;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod startup;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod music;

/// Number of cached Windows Glide splash frames, including the authored fade.
pub const SPLASH_FRAMES: usize = 150;
