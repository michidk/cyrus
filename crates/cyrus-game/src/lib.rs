//! Portable startup, animation, environment, fidelity, player and settings plugins
//! are shared with WASM. Install access, exports and control are native only.
pub mod animation;
#[cfg(not(target_arch = "wasm32"))]
mod animation_export;
#[cfg(not(target_arch = "wasm32"))]
pub mod control;
pub mod environment;
#[cfg(not(target_arch = "wasm32"))]
mod environment_export;
pub mod fidelity;
pub mod fog;
#[cfg(not(target_arch = "wasm32"))]
pub mod install;
#[cfg(not(target_arch = "wasm32"))]
pub mod loading;
#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(target_arch = "wasm32"))]
pub mod music;
pub mod player;
#[cfg(not(target_arch = "wasm32"))]
pub mod scene;
pub mod settings;
mod sky;
#[cfg(not(target_arch = "wasm32"))]
pub mod sound;
pub mod sound_bank;
#[cfg(not(target_arch = "wasm32"))]
mod sound_export;
pub mod startup;
#[cfg(not(target_arch = "wasm32"))]
pub mod subtitles;
mod water;

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Loading,
    Menu,
    InGame,
}

/// Everything except windowing/rendering plugins, so the same set runs headless.
#[cfg(not(target_arch = "wasm32"))]
pub struct GamePlugin {
    pub install: install::GameInstall,
    pub settings: settings::Settings,
}

#[cfg(not(target_arch = "wasm32"))]
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.install.clone())
            .insert_resource(self.settings.fidelity.clone())
            .insert_resource(scene::SceneRequest(self.settings.scene.clone()))
            .init_state::<AppState>()
            .add_plugins((
                loading::LoadingPlugin,
                scene::ScenePlugin,
                player::PlayerPlugin,
                control::ControlPlugin,
                sound::SoundPlugin,
                music::MusicPlugin,
                subtitles::SubtitlePlugin,
            ));
    }
}
