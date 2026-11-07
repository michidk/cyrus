//! The actor's AI type (`ai_type`), its lifecycle selector, and the adjacent AI mode (`ai_mode`).
//! Scripts read and write the type through `MyAiType`/`SetAiType`; see [`AiMode`] for the mode.
//! Types 0 to 5 are the installed `SOUP386.DEF` `at_*` equates; the engine also uses 6 for
//! combat, 7 for point navigation and 8 for a dead actor, which the DEF leaves unnamed
//! (preservation `docs/engine/SOUPPROPS.md`, `docs/engine/SOUP.md`). A script may write any byte.
use serde::{Deserialize, Serialize};
use std::fmt;

/// An actor's AI type. Engine code names types through the constants;
/// [`AiType::from_byte`] is for bytes that are data, such as a script's `SetAiType` argument.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AiType(u8);

impl AiType {
    pub const NONE: Self = Self(0);
    pub const FIND_PLAYER: Self = Self(1);
    pub const WANDER: Self = Self(2);
    pub const PLAYER_IN_SIGHT: Self = Self(3);
    pub const FIND_LOCATION: Self = Self(4);
    pub const LOCATION_IN_SIGHT: Self = Self(5);
    /// Set by combat entry; `beginCombat` skips its remaining work when already set.
    pub const COMBAT: Self = Self(6);
    /// Set while a point-navigation command runs; its completion returns the actor to `NONE`.
    pub const POINT_NAVIGATION: Self = Self(7);
    /// The type `IsDead` tests; set when an actor is killed.
    pub const DEAD: Self = Self(8);

    /// The type a data-supplied byte holds.
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The type byte.
    pub const fn byte(self) -> u8 {
        self.0
    }
}

impl fmt::Display for AiType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An actor's AI mode byte (`ai_mode`, `MyAiMode`/`SetAiMode`), separate from its [`AiType`].
/// The engine names only zero, the cleared mode, and 11, the direct-destination mode the AI
/// progress service selects once a straight path to its destination is clear (preservation
/// `docs/engine/SOUP.md`, AI destination clearance and AI mode eleven progress). A script may
/// write any byte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AiMode(u8);

impl AiMode {
    pub const NONE: Self = Self(0);
    /// Steering straight toward a destination whose path the clearance query found clear.
    pub const DIRECT_DESTINATION: Self = Self(11);

    /// The mode a data-supplied byte holds.
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The mode byte.
    pub const fn byte(self) -> u8 {
        self.0
    }
}

impl fmt::Display for AiMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
