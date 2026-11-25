//! Runtime stage-suppression masks retain the complete script-control byte.
/// The runtime `disable_script` byte, following preservation `engine/SOUP.md` and
/// `engine/SOUPPROPS.md`. Unknown bits survive property writes and persistence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ScriptControl(u8);

impl ScriptControl {
    /// Suppress ordinary main processing, including entry and existing tasks.
    pub const SCRIPT_DISABLED: Self = Self(0x01);
    /// Suppress the native RAHK hook; also held during player alignment.
    pub const HOOK_DISABLED: Self = Self(0x02);
    /// Bypass the existing-task pass while retaining entry and main processing.
    pub const TASKS_DISABLED: Self = Self(0x04);
    /// Exclude the actor from the ordinary actor-update body.
    pub const OBJECT_DISABLED: Self = Self(0x08);
    /// The mask toggled by `DisableCombat` and `EnableCombat`.
    pub const COMBAT_DISABLED: Self = Self(0x10);
    /// The five documented masks cleared together by camera establishment.
    pub const KNOWN_MASKS: Self = Self(0x1f);

    /// Retain the full byte, including unnamed bits.
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, mask: Self) -> bool {
        self.0 & mask.0 == mask.0
    }

    pub fn insert(&mut self, mask: Self) {
        self.0 |= mask.0;
    }

    pub fn remove(&mut self, mask: Self) {
        self.0 &= !mask.0;
    }

    /// Raw storage for the low-byte script property boundary.
    pub(super) fn bits_mut(&mut self) -> &mut u8 {
        &mut self.0
    }
}
