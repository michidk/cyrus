//! Gameplay state, fidelity and player rules, independent of presentation.
pub mod cheats;
pub mod clock;
pub mod console;
pub mod fidelity;
pub mod inventory_screen;
pub mod island_map;
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

/// The book opened from live gameplay (ordinary in-game context, as opposed to death).
/// Retained until the book returns to gameplay.
#[derive(Resource)]
pub struct PauseBookEntry {
    /// Destination book page: 0 for Escape, 1 for the F2 Save and 2 for the F3 Load shortcut.
    pub page: u8,
}

pub struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        logbook::register(app);
        inventory_screen::register(app);
        island_map::register(app);
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
                Update,
                request_pause_book.run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                Last,
                clear_restored_entry.run_if(in_state(AppState::InGame)),
            );
    }
}

/// With `final_version` set, Escape is processed after its release (preservation
/// engine/menu.md); the original without it ignores Escape in a world.
fn request_pause_book(
    keys: Res<ButtonInput<KeyCode>>,
    mut armed: Local<bool>,
    logbook: Option<Res<logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    actors: Option<Res<script::ScriptActors>>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
) {
    let held_elsewhere = !actors.is_some_and(|actors| actors.final_version())
        || logbook.is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open || console.blocked)
        || matches!(*next, NextState::Pending(_));
    // Escape outranks the shortcuts (preservation engine/menu.md). F4–F6 need pages the
    // book cannot yet enter directly, so they stay unhandled.
    let page = if escape_opens_book(&keys, &mut armed, held_elsewhere) {
        Some(0)
    } else {
        [(KeyCode::F2, 1), (KeyCode::F3, 2)]
            .into_iter()
            .find(|(key, _)| !held_elsewhere && keys.just_pressed(*key))
            .map(|(_, page)| page)
    };
    if let Some(page) = page {
        commands.insert_resource(PauseBookEntry { page });
        next.set(AppState::Menu);
    }
}

/// The press must have started while no gate held the keyboard, so the release that closes a
/// screen or the console does not also open the book.
fn escape_opens_book(keys: &ButtonInput<KeyCode>, armed: &mut bool, held_elsewhere: bool) -> bool {
    if keys.just_pressed(KeyCode::Escape) {
        *armed = !held_elsewhere;
    }
    keys.just_released(KeyCode::Escape) && std::mem::take(armed) && !held_elsewhere
}

fn clear_restored_entry(mut commands: Commands) {
    commands.remove_resource::<RestoredSessionEntry>();
}

fn log_state_changes(mut transitions: MessageReader<StateTransitionEvent<AppState>>) {
    for transition in transitions.read() {
        info!("state: {:?} -> {:?}", transition.exited, transition.entered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(keys: &mut ButtonInput<KeyCode>, armed: &mut bool, held_during_press: bool) -> bool {
        keys.press(KeyCode::Escape);
        let early = escape_opens_book(keys, armed, held_during_press);
        keys.clear();
        keys.release(KeyCode::Escape);
        let opened = escape_opens_book(keys, armed, false);
        keys.clear();
        assert!(!early);
        opened
    }

    #[test]
    fn escape_release_opens_the_book_from_gameplay() {
        let mut keys = ButtonInput::<KeyCode>::default();
        assert!(tap(&mut keys, &mut false, false));
    }

    #[test]
    fn escape_that_closes_a_screen_does_not_open_the_book() {
        let mut keys = ButtonInput::<KeyCode>::default();
        assert!(!tap(&mut keys, &mut false, true));
    }
}
