//! The code shared by an actor's queued-motion (`jump_flag`) and falling (`fall_flag`) bytes.
//! Names follow preservation `docs/engine/player.md` (Jump motion values); a script may write any
//! byte.
use serde::{Deserialize, Serialize};
use std::fmt;

/// A queued-motion or falling code: the jump in its low seven bits and the raised-arms bit.
/// Engine code names codes through the constants; [`JumpMotion::from_byte`] is for bytes that are
/// data, such as a script's `jump_flag` or `fall_flag` write.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct JumpMotion(u8);

impl JumpMotion {
    pub const NONE: Self = Self(0);
    /// A standing jump, or a plain fall.
    pub const STANDING: Self = Self(1);
    pub const FORWARD: Self = Self(2);
    pub const BACKWARD: Self = Self(3);
    pub const RUNNING: Self = Self(8);
    pub const RUNNING_LEFT_LEAD: Self = Self(9);
    pub const ROPE: Self = Self(24);

    const ARMS_RAISED: u8 = 0x80;

    /// The code a data-supplied byte holds.
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The code byte.
    pub const fn byte(self) -> u8 {
        self.0
    }

    /// The jump without the raised-arms bit.
    pub const fn kind(self) -> Self {
        Self(self.0 & !Self::ARMS_RAISED)
    }

    /// Whether the raised-arms bit is set.
    pub const fn arms_raised(self) -> bool {
        self.0 & Self::ARMS_RAISED != 0
    }

    /// This code with the raised-arms bit set or cleared.
    pub const fn with_arms_raised(self, raised: bool) -> Self {
        Self(if raised {
            self.0 | Self::ARMS_RAISED
        } else {
            self.0 & !Self::ARMS_RAISED
        })
    }
}

impl fmt::Display for JumpMotion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raised_arms_bit_is_separate_from_the_jump() {
        let raised = JumpMotion::RUNNING.with_arms_raised(true);
        assert_eq!(raised.byte(), 0x88);
        assert!(raised.arms_raised());
        assert_eq!(raised.kind(), JumpMotion::RUNNING);
        assert_eq!(raised.with_arms_raised(false), JumpMotion::RUNNING);
    }
}
