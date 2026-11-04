//! The actor's living-state byte (`extra_flag`): its current special action. Names follow
//! preservation `docs/engine/player.md` (Living states); a script may write any byte.
use serde::{Deserialize, Serialize};
use std::fmt;

/// An actor's living state. Engine code names states through the constants;
/// [`LivingState::from_byte`] is for bytes that are data, such as a script's `extra_flag` write.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct LivingState(u8);

impl LivingState {
    pub const ORDINARY: Self = Self(0);
    pub const TURNING_TO_HIGH_LEDGE: Self = Self(10);
    pub const HIGH_CLIMB: Self = Self(11);
    pub const TURNING_TO_LOW_LEDGE: Self = Self(12);
    pub const LOW_CLIMB: Self = Self(13);
    pub const CATCHING_HANG: Self = Self(14);
    pub const HANGING: Self = Self(15);
    pub const CLIMB_FROM_HANG: Self = Self(16);
    pub const DROPPING: Self = Self(17);
    pub const CLIMB_FINISHED: Self = Self(18);
    pub const ENTERING_LEDGE: Self = Self(19);
    pub const DRAWING_SWORD: Self = Self(20);
    pub const SHEATHING_SWORD: Self = Self(21);
    pub const LANDED_ON_ACTOR: Self = Self(27);
    pub const BOUNCING_FROM_ACTOR: Self = Self(28);
    pub const FALLING_FROM_ACTOR: Self = Self(29);
    pub const BOUNCE_ENTRY: Self = Self(30);
    pub const BOUNCE_RISING: Self = Self(31);
    pub const BOUNCE_FALLING: Self = Self(32);
    pub const LIGHTING_TORCH: Self = Self(33);
    pub const DOUSING_TORCH: Self = Self(34);

    /// The state a data-supplied byte holds.
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The state byte.
    pub const fn byte(self) -> u8 {
        self.0
    }

    /// States 10 to 19: entering, holding or leaving a ledge.
    pub fn is_ledge(self) -> bool {
        (Self::TURNING_TO_HIGH_LEDGE..=Self::ENTERING_LEDGE).contains(&self)
    }

    /// States 27 to 29: the bounce after landing on a bouncing actor.
    pub fn is_actor_bounce(self) -> bool {
        (Self::LANDED_ON_ACTOR..=Self::FALLING_FROM_ACTOR).contains(&self)
    }

    /// States 27 to 32: the actor bounce and `BounceObject`.
    pub fn is_bounce(self) -> bool {
        (Self::LANDED_ON_ACTOR..=Self::BOUNCE_FALLING).contains(&self)
    }
}

impl fmt::Display for LivingState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
