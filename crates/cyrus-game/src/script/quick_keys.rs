//! The quick sword and quick health keys, sampled from the installed key bindings.
use bevy::prelude::*;

/// Binding slots of the quick sword and quick health keys among the 15 menu control codes.
const SWORD_SLOT: usize = 9;
const HEALTH_SLOT: usize = 10;

/// Quick keys for the ordinary update's key handler. The sword key reads as held; the health key
/// reads once per press, so a held key heals once.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct QuickKeys {
    pub sword: bool,
    pub health: bool,
}

/// The letter and modifier keys the `InKey` native reads, sampled under the same locks as the
/// quick keys. Bit `n` of `letters` is the letter `A + n`.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptKeys {
    pub letters: u32,
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
}

impl ScriptKeys {
    /// `InKey`'s test: the key of an ASCII letter or digit code (either case) is down and every
    /// modifier named by the higher code bytes (`s`, `c`, `a`, either case) is held. `None` for
    /// a character without a recovered scan code.
    pub fn in_key(&self, code: u32) -> Option<bool> {
        let key = (code & 0xff) as u8;
        if !key.is_ascii_alphabetic() {
            return None;
        }
        let letter = key.to_ascii_lowercase() - b'a';
        let (mut shift, mut control, mut alt) = (false, false, false);
        for byte in [code >> 8, code >> 16, code >> 24] {
            match (byte as u8).to_ascii_lowercase() {
                b's' => shift = true,
                b'c' => control = true,
                b'a' => alt = true,
                _ => {}
            }
        }
        if (control && !self.control) || (shift && !self.shift) || (alt && !self.alt) {
            return Some(false);
        }
        Some(self.letters >> letter & 1 != 0)
    }
}

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

/// Sample the keys under the same locks that suspend the player controller's input.
#[allow(clippy::too_many_arguments)] // Bevy supplies each independent input lock as a parameter.
pub(super) fn sample(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    suppressed: Option<Res<crate::player::input::InputSuppressed>>,
    pending_world: Option<Res<cyrus_world::lifecycle::WorldPreparationPending>>,
    logbook: Option<Res<crate::logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    preferences: Option<Res<crate::preferences::Preferences>>,
    mut quick: ResMut<QuickKeys>,
    mut script_keys: ResMut<ScriptKeys>,
) {
    let Some(keys) = keys else {
        *quick = QuickKeys::default();
        *script_keys = ScriptKeys::default();
        return;
    };
    if suppressed.is_some_and(|suppressed| suppressed.0)
        || pending_world.is_some()
        || logbook.as_ref().is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open)
    {
        *quick = QuickKeys::default();
        *script_keys = ScriptKeys::default();
        return;
    }
    *script_keys = ScriptKeys {
        letters: LETTERS
            .iter()
            .enumerate()
            .filter(|(_, key)| keys.pressed(**key))
            .fold(0, |mask, (index, _)| mask | 1 << index),
        shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        control: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
    };
    let bindings = preferences
        .as_ref()
        .and_then(|preferences| {
            preferences
                .menu()
                .as_ref()
                .ok()?
                .as_ref()
                .map(|menu| menu.bindings)
        })
        .or_else(|| logbook.as_ref().map(|screen| screen.bindings));
    let Some(bindings) = bindings else {
        *quick = QuickKeys::default();
        return;
    };
    let key = |slot: usize| crate::player::input::scan_key(bindings[slot]);
    quick.sword = key(SWORD_SLOT).is_some_and(|key| keys.pressed(key));
    quick.health = key(HEALTH_SLOT).is_some_and(|key| keys.just_pressed(key));
}

#[cfg(test)]
mod tests {
    use super::ScriptKeys;

    #[test]
    fn in_key_tests_the_letter_and_the_modifiers_named_by_the_higher_bytes() {
        let r = 1 << (b'r' - b'a');
        let q = 1 << (b'q' - b'a');
        let keys = ScriptKeys {
            letters: r | q,
            ..Default::default()
        };
        // Either case of the letter code names the same key.
        assert_eq!(keys.in_key(u32::from(b'r')), Some(true));
        assert_eq!(keys.in_key(u32::from(b'R')), Some(true));
        assert_eq!(keys.in_key(u32::from(b'p')), Some(false));
        // 'q' with alt required (the installed 0x6171): refused until alt is held.
        let alt_q = u32::from(b'q') | u32::from(b'a') << 8;
        assert_eq!(keys.in_key(alt_q), Some(false));
        let held = ScriptKeys { alt: true, ..keys };
        assert_eq!(held.in_key(alt_q), Some(true));
        // Characters without a recovered scan code are not guessed.
        assert_eq!(keys.in_key(u32::from(b'1')), None);
    }
}
