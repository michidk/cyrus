//! Receiver-owned AI mode bytes retain arbitrary script values.
use serde::{Deserialize, Serialize};
use std::fmt;

/// The `ai_mode` byte, independent of the AI type lifecycle selector.
/// Preservation `engine/SOUP.md` documents the direct-destination progress mode;
/// `engine/SOUPPROPS.md` specifies unsigned low-byte property access.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct AiMode(u8);

impl AiMode {
    /// AI destination clearance has admitted direct progress toward its destination.
    pub const DIRECT_DESTINATION: Self = Self(11);

    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    pub const fn byte(self) -> u8 {
        self.0
    }

    /// Raw storage for the low-byte script property boundary.
    pub(crate) fn byte_mut(&mut self) -> &mut u8 {
        &mut self.0
    }
}

impl fmt::Display for AiMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
