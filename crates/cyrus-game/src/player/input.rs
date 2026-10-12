//! Keyboard input only. Binding semantics and scan codes are documented in
//! redguard-preservation's `docs/config/keys-ini.md`.

use crate::fidelity::{Fidelity, Mode};
use bevy::prelude::*;
use serde::Serialize;

/// Held player actions, before movement/animation state interpretation.
/// Faithful walk is the held walk/run modifier, separate from the walk-mode toggle.
/// Modern walk requests walking while neither Shift key is held.
#[derive(Resource, Default, Debug, PartialEq, Eq, Serialize)]
pub struct PlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub action: bool,
    pub jump: bool,
    pub look: bool,
    pub walk: bool,
}

/// Keyboard bindings in up/down/left/right/a/b/c/d order. Empty until loaded.
#[derive(Resource, Default)]
pub struct PlayerBindings([Option<KeyCode>; 8]);

impl PlayerBindings {
    /// Translate documented PC AT input codes into Bevy physical keys.
    /// Zero is unbound. Unsupported device/unknown codes return an error,
    /// rather than silently substituting a different control scheme.
    pub fn from_codes(codes: [u8; 8]) -> Result<Self, String> {
        let mut keys = [None; 8];
        for (i, code) in codes.into_iter().enumerate() {
            keys[i] = if code == 0 {
                None
            } else {
                Some(scan_key(code).ok_or_else(|| {
                    format!(
                        "KEYS.INI [input] key_{}[0]: unsupported input code {code}",
                        ["up", "down", "left", "right", "a", "b", "c", "d"][i]
                    )
                })?)
            };
        }
        Ok(Self(keys))
    }

    /// Sample held keys without imposing movement priorities or cancelling
    /// opposing inputs; those belong to the original controller state machine.
    pub fn sample(&self, keys: &ButtonInput<KeyCode>) -> PlayerInput {
        let [forward, backward, left, right, walk, jump, look, action] =
            self.0.map(|key| key.is_some_and(|key| keys.pressed(key)));
        PlayerInput {
            forward,
            backward,
            left,
            right,
            action,
            jump,
            look,
            walk,
        }
    }
}

pub fn scan_key(code: u8) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match code {
        1 => Escape,
        2..=11 => [
            Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
        ][usize::from(code - 2)],
        12 => Minus,
        13 => Equal,
        14 => Backspace,
        15 => Tab,
        16..=25 => {
            [KeyQ, KeyW, KeyE, KeyR, KeyT, KeyY, KeyU, KeyI, KeyO, KeyP][usize::from(code - 16)]
        }
        26 => BracketLeft,
        27 => BracketRight,
        28 => Enter,
        29 => ControlLeft,
        30..=38 => [KeyA, KeyS, KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL][usize::from(code - 30)],
        39 => Semicolon,
        40 => Quote,
        41 => Backquote,
        42 => ShiftLeft,
        43 => Backslash,
        44..=50 => [KeyZ, KeyX, KeyC, KeyV, KeyB, KeyN, KeyM][usize::from(code - 44)],
        51 => Comma,
        52 => Period,
        53 => Slash,
        54 => ShiftRight,
        55 => PrintScreen,
        56 => AltLeft,
        57 => Space,
        58 => CapsLock,
        59..=68 => [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10][usize::from(code - 59)],
        69 => NumLock,
        70 => ScrollLock,
        71 => Home,
        72 => ArrowUp,
        73 => PageUp,
        74 => NumpadSubtract,
        75 => ArrowLeft,
        77 => ArrowRight,
        78 => NumpadAdd,
        79 => End,
        80 => ArrowDown,
        81 => PageDown,
        82 => Insert,
        83 => Delete,
        87 => F11,
        88 => F12,
        _ => return None,
    })
}

/// Reverse physical-key mapping for the original Controls capture path.
pub fn scan_code(key: KeyCode) -> Option<u8> {
    (1..=127).find(|code| scan_key(*code) == Some(key))
}

pub(super) fn sample(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<PlayerBindings>,
    suppressed: Res<InputSuppressed>,
    fidelity: Res<Fidelity>,
    mut input: ResMut<PlayerInput>,
) {
    if suppressed.0 {
        *input = PlayerInput::default();
        return;
    }
    *input = bindings.sample(&keys);
    if fidelity.player_controller == Mode::Modern {
        input.forward = keys.pressed(KeyCode::KeyW);
        input.backward = keys.pressed(KeyCode::KeyS);
        input.left = keys.pressed(KeyCode::KeyA);
        input.right = keys.pressed(KeyCode::KeyD);
        input.jump = keys.pressed(KeyCode::Space);
        input.walk = !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    }
}

/// Explicit inspection/UI input lock, sampled before gameplay input.
#[derive(Resource, Default)]
pub struct InputSuppressed(pub bool);
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputSample;
