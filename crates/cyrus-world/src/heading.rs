//! Heading arithmetic. Engine headings use 2048 units per turn and wrap by masking
//! (preservation `docs/engine/SOUP.md`).
use std::ops::BitAnd;

/// Units in a full turn.
pub const FULL_TURN: i32 = 2048;
/// Units in a half turn.
pub const HALF_TURN: i32 = FULL_TURN / 2;

/// The angle wrapped into `0..FULL_TURN`, in any integer width that holds a turn.
pub fn wrap<T: BitAnd<Output = T> + From<u16>>(angle: T) -> T {
    angle & T::from((FULL_TURN - 1) as u16)
}

/// The heading facing the other way.
pub fn opposite(heading: u16) -> u16 {
    wrap(heading.wrapping_add(HALF_TURN as u16))
}
