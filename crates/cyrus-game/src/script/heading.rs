//! Heading arithmetic. Engine headings use 2048 units per turn and wrap by masking
//! (preservation `docs/engine/SOUP.md`).

/// Units in a full turn.
pub const FULL_TURN: i32 = 2048;
/// Units in a half turn.
pub const HALF_TURN: i32 = FULL_TURN / 2;

/// The heading wrapped into `0..FULL_TURN`.
pub const fn wrap(heading: i32) -> i32 {
    heading & (FULL_TURN - 1)
}

/// The heading facing the other way.
pub const fn opposite(heading: u16) -> u16 {
    wrap(heading as i32 + HALF_TURN) as u16
}
