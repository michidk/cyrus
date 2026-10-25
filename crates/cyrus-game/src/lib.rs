//! Gameplay state, fidelity and player rules, independent of presentation.
pub mod cheats;
pub mod clock;
pub mod console;
pub mod fidelity;
pub mod inventory_screen;
pub mod logbook;
pub mod player;
pub mod preferences;
pub mod random;
pub mod rope;
pub mod script;
use bevy::prelude::*;
use log::info;
/// Engine-to-export unit divisor, matching preservation's GLB conversion.
pub const ENGINE_UNIT_SCALE: f32 = 20.0;
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Native host is selecting and preparing an installation in the main window.
    SelectingInstall,
    #[default]
    Loading,
    Menu,
    InGame,
}
/// A complete replacement was published before a menu-to-game state transition.
/// Exit systems must not restore checkpoints belonging to the replaced session.
#[derive(Resource)]
pub struct RestoredSessionEntry;

/// Gameplay continuation retained while an eligible book menu owns presentation.
/// Startup and terminal death menus must not create this admission token.
#[derive(Resource)]
pub struct PausedGameplay {
    pub clock: clock::SimulationClock,
    pub active_cameras: Vec<Entity>,
}

pub struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        logbook::register(app);
        inventory_screen::register(app);
        app.init_resource::<preferences::Preferences>()
            .init_resource::<random::EngineRandom>()
            .init_resource::<cheats::Cheats>()
            .init_resource::<cheats::CheatTransfer>()
            .init_state::<AppState>()
            .init_resource::<fidelity::Fidelity>()
            .add_plugins((
                player::PlayerPlugin,
                script::ScriptPlugin,
                console::GameConsolePlugin,
            ))
            .add_systems(Update, log_state_changes)
            .add_systems(
                Last,
                clear_restored_entry.run_if(in_state(AppState::InGame)),
            );
    }
}

fn clear_restored_entry(mut commands: Commands) {
    commands.remove_resource::<RestoredSessionEntry>();
}

fn log_state_changes(mut transitions: MessageReader<StateTransitionEvent<AppState>>) {
    for transition in transitions.read() {
        info!("state: {:?} -> {:?}", transition.exited, transition.entered);
    }
}
