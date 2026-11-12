//! Keys and the game's own key-bound actions.

/// A keyboard key by its Bevy `KeyCode` name. Constants cover the common keys; `Key::code` names
/// any other, such as "KeyQ" or "ArrowUp".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(&'static str);

impl Key {
    pub const fn code(name: &'static str) -> Key {
        Key(name)
    }

    pub const fn name(self) -> &'static str {
        self.0
    }

    pub const W: Key = Key("KeyW");
    pub const A: Key = Key("KeyA");
    pub const S: Key = Key("KeyS");
    pub const D: Key = Key("KeyD");
    pub const SPACE: Key = Key("Space");
    pub const SHIFT_LEFT: Key = Key("ShiftLeft");
    pub const SHIFT_RIGHT: Key = Key("ShiftRight");
    pub const CONTROL_LEFT: Key = Key("ControlLeft");
    pub const ESCAPE: Key = Key("Escape");
    pub const F11: Key = Key("F11");
    pub const F12: Key = Key("F12");
}

/// An action under the game's own key bindings (KEYS.INI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    /// The walk/run modifier.
    Walk,
    Jump,
}

impl Action {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Walk => "walk",
            Action::Jump => "jump",
        }
    }
}
