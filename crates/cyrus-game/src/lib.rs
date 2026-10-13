//! Gameplay state, fidelity and player rules, independent of presentation.
pub mod clock;
pub mod fidelity;
pub mod player;
pub mod preferences;
pub mod script;
use bevy::prelude::*;
/// Engine-to-export unit divisor, matching preservation's GLB conversion.
pub const ENGINE_UNIT_SCALE: f32 = 20.0;
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Loading,
    Menu,
    InGame,
}
pub struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<preferences::Preferences>()
            .init_state::<AppState>()
            .init_resource::<fidelity::Fidelity>()
            .add_plugins((player::PlayerPlugin, script::ScriptPlugin));
    }
}
