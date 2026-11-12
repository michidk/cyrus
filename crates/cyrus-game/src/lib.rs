//! Gameplay state shared by hosts: fidelity policy, preferences, cheats, the session random stream, application state, movie unlocks and ropes.
pub mod cheats;
pub mod fidelity;
pub mod preferences;
pub mod random;
pub mod rope;
use bevy::prelude::*;
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
    /// A fixed gameplay information screen suspends simulation while retaining the world.
    GameplayOverlay,
}
/// A complete replacement was published before a menu-to-game state transition.
/// Exit systems must not restore checkpoints belonging to the replaced session.
#[derive(Resource)]
pub struct RestoredSessionEntry;

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

/// Whether the debug flycam is enabled. While it is, the game and the book leave the camera alone.
#[derive(Resource, Default)]
pub struct FlycamEngaged(pub bool);

/// The camera that renders the book menu's 3D scene.
#[derive(Component)]
pub struct BookCamera;

/// The book opened from live gameplay (ordinary in-game context, as opposed to death).
/// Retained until the book returns to gameplay.
#[derive(Resource)]
pub struct PauseBookEntry {
    /// Destination book page: 0 for Escape, otherwise the page of the pressed book shortcut key.
    pub page: u8,
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

/// Fixed information screens entered from a live world, independent of the book menu.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameplayOverlay {
    Version,
    Credits,
}
