//! `fidelity`, `player` and `settings` compile everywhere; the rest reads the
//! install from disk and runs threads, so it is native only (`cyrus-web` reuses
//! the portable part).
#[cfg(not(target_arch = "wasm32"))]
pub mod control;
pub mod environment;
#[cfg(not(target_arch = "wasm32"))]
mod environment_export;
pub mod fidelity;
#[cfg(not(target_arch = "wasm32"))]
pub mod install;
#[cfg(not(target_arch = "wasm32"))]
pub mod loading;
pub mod player;
#[cfg(not(target_arch = "wasm32"))]
pub mod scene;
pub mod settings;

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Loading,
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
            ));
    }
}
