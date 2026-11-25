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

/// The angle in radians, for any heading in engine units (2048 per turn), unwrapped.
pub fn radians(angle: i32) -> f64 {
    f64::from(angle) * std::f64::consts::TAU / f64::from(FULL_TURN)
}

/// The angle in radians using the original's rotation table period, 6.2831852 rather than 2π.
/// Use it where a result must match the original's rotation table to the last digit.
#[allow(clippy::approx_constant)] // The original's decimal period is deliberately not TAU.
pub fn table_radians(angle: i32) -> f64 {
    f64::from(angle) * 6.2831852 / f64::from(FULL_TURN)
}
