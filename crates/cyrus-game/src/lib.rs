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

/// Movie-table unlock flags: bit `n` is MENU.INI movie `n` (fifteen records exist). Playing a movie
/// sets its bit and New Game clears them all; the Movies page hides entries 302 through 315 (movies 1
/// through 14) while their bit is clear (preservation engine/movies.md).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovieUnlocks(u16);

impl MovieUnlocks {
    /// Records in the original movie table.
    pub const RECORDS: usize = 15;

    pub fn is_unlocked(self, movie: usize) -> bool {
        movie < Self::RECORDS && self.0 >> movie & 1 == 1
    }

    /// Records out-of-range indices as no-ops, like an index the table does not hold.
    pub fn unlock(&mut self, movie: usize) {
        if movie < Self::RECORDS {
            self.0 |= 1 << movie;
        }
    }

    pub fn reset(&mut self) {
        self.0 = 0;
    }

    /// The flags as saved with a game: one bit per record.
    pub fn bits(self) -> u16 {
        self.0
    }

    /// Flags read back from a save; bits beyond the table's fifteen records are dropped.
    pub fn from_bits(bits: u16) -> Self {
        Self(bits & ((1 << Self::RECORDS) - 1))
    }
}

/// The book opened from live gameplay (ordinary in-game context, as opposed to death).
/// Retained until the book returns to gameplay.
#[derive(Resource)]
pub struct PauseBookEntry {
    /// Destination book page: 0 for Escape, otherwise the page of the pressed `BOOK_SHORTCUTS` key.
    pub page: u8,
}

pub struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        logbook::register(app);
        inventory_screen::register(app);
        island_map::register(app);
        app.init_resource::<preferences::Preferences>()
            .init_resource::<MovieUnlocks>()
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

/// Gameplay book shortcuts in sampling order: Save, Load, Display, Sound, Controls.
const BOOK_SHORTCUTS: [(KeyCode, u8); 5] = [
    (KeyCode::F2, 1),
    (KeyCode::F3, 2),
    (KeyCode::F4, 5),
    (KeyCode::F5, 6),
    (KeyCode::F6, 7),
];

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
    // Escape outranks the shortcuts (preservation engine/menu.md).
    let page = if escape_opens_book(&keys, &mut armed, held_elsewhere) {
        Some(0)
    } else {
        BOOK_SHORTCUTS
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

#[cfg(test)]
mod movie_unlock_tests {
    use super::MovieUnlocks;

    #[test]
    fn unlocking_is_per_record_bounded_and_reset_clears_every_flag() {
        let mut unlocks = MovieUnlocks::default();
        assert!(!unlocks.is_unlocked(4));
        unlocks.unlock(4);
        unlocks.unlock(MovieUnlocks::RECORDS);
        assert!(unlocks.is_unlocked(4) && !unlocks.is_unlocked(3));
        assert!(!unlocks.is_unlocked(MovieUnlocks::RECORDS));
        unlocks.reset();
        assert_eq!(unlocks, MovieUnlocks::default());
        assert_eq!(MovieUnlocks::from_bits(0xffff).bits(), 0x7fff);
    }
}
