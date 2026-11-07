//! An actor's collision-response word (`last_collide`): step-status, body-response and
//! collision-transition marks. Bits carry the `SOUP386.DEF` `cf_*` names (its spelling kept);
//! preservation `docs/engine/player.md` (Collision-response word) lists each writer.
use serde::{Deserialize, Serialize};
use std::ops::{BitOr, BitOrAssign};

/// A set of collision-response marks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollisionFlags(u32);

impl CollisionFlags {
    pub const SLIDE_OFF_WALL: Self = Self(0x1);
    pub const HIT_WALL: Self = Self(0x2);
    pub const POLYGON_PUSH_OUT: Self = Self(0x4);
    pub const LAST_VALID_SET: Self = Self(0x8);
    pub const VALIDATE_SLOPE_FAIL: Self = Self(0x10);
    pub const CHECK_LEFT_FAIL: Self = Self(0x20);
    pub const CHECK_RIGHT_FAIL: Self = Self(0x40);
    pub const CHECK_FORWARD_FAIL: Self = Self(0x80);
    pub const CHECK_BACKWARD_FAIL: Self = Self(0x100);
    pub const CHECK_RUN_FORWARD_FAIL: Self = Self(0x200);
    pub const MANAGED_COLLISION: Self = Self(0x400);
    pub const VALIDATE_FORWARD_FAILED: Self = Self(0x800);
    pub const PREMETURE_JUMP_END: Self = Self(0x1000);
    pub const PREMETURE_FALL: Self = Self(0x2000);
    pub const CROSS_POLYGON_PUSH: Self = Self(0x4000);
    /// Landing on another actor; the DEF leaves this bit unnamed.
    pub const LANDED_ON_ACTOR: Self = Self(0x8000);
    /// Stable recovery and water rollback; the DEF leaves this bit unnamed.
    pub const ROLLBACK: Self = Self(0x1_0000);

    /// The marks a data-supplied word holds.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The word.
    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every mark of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any mark of `other` is set.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for CollisionFlags {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOrAssign for CollisionFlags {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}
