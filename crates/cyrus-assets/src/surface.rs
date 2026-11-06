//! SURFACE.INI surface classes: the ground an actor stands on, which also selects its sound
//! remap row (preservation `docs/config/surface-ini.md`).
use serde::{Deserialize, Serialize};

/// A surface class code. Codes outside the documented table are retained.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Surface(u8);

impl Surface {
    pub const UNKNOWN: Self = Self(0);
    pub const WATER: Self = Self(1);
    pub const DEEPWATER: Self = Self(2);
    pub const SCAPEWATER: Self = Self(3);
    pub const SCAPEDEEPWATER: Self = Self(4);
    pub const LAVA: Self = Self(5);
    pub const SAND: Self = Self(6);
    pub const WOOD: Self = Self(7);
    pub const TILE: Self = Self(8);
    pub const SCAPE: Self = Self(9);
    pub const ROCK: Self = Self(10);
    pub const GLOOP: Self = Self(11);

    pub const fn from_byte(code: u8) -> Self {
        Self(code)
    }

    pub const fn byte(self) -> u8 {
        self.0
    }

    /// Any water class: water, deep water and their landscape variants.
    pub const fn is_water(self) -> bool {
        matches!(
            self,
            Self::WATER | Self::DEEPWATER | Self::SCAPEWATER | Self::SCAPEDEEPWATER
        )
    }

    /// Deep water or landscape deep water, where actors swim.
    pub const fn is_deep_water(self) -> bool {
        matches!(self, Self::DEEPWATER | Self::SCAPEDEEPWATER)
    }
}

impl std::fmt::Display for Surface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}
