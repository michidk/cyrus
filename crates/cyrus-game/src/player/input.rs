//! Keyboard and mouse-button input. Binding semantics, scan codes and the input state
//! table are documented in redguard-preservation's `docs/config/keys-ini.md`.
//! Joystick codes 132 to 139 read a [`JoystickState`] that the gamepad adapter fills in the
//! original's digital terms; the analog axes are reduced to the original's dead-zone test.

use crate::fidelity::{Fidelity, Mode};
use bevy::input::mouse::MouseButton;
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

/// Retained gameplay toggles, initialized from installed HUD data or saved preferences.
/// The HUD projects these values; world replacement must not reset them.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PlayerToggles {
    pub walk_mode: bool,
    pub auto_defend: bool,
}

/// One bound input. Mouse buttons read as the original's release edge: true for the
/// update in which the button goes up, never while it is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Binding {
    Key(KeyCode),
    MouseRelease(MouseButton),
    /// Joystick input code 132 to 139: buttons 1 to 4, then left, right, up, down.
    Joystick(u8),
}

/// Digital joystick input table entries, written by the gamepad adapter before sampling.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoystickState {
    /// Buttons 1 to 4, codes 132 to 135.
    pub buttons: [bool; 4],
    /// Stick beyond the dead zone: left 136, right 137, up 138, down 139.
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    /// Codes 132 to 139 as bits 0 to 7 at the previous poll, for press edges.
    previous: u8,
}

impl JoystickState {
    fn mask(&self) -> u8 {
        (132..=139).fold(0, |mask, code| {
            mask | u8::from(self.pressed(code)) << (code - 132)
        })
    }

    fn just_pressed(&self, code: u8) -> bool {
        self.pressed(code) && self.previous >> (code - 132) & 1 == 0
    }

    /// Input code of the first held button (132 to 135), for Controls capture.
    pub fn button_code(&self) -> Option<u8> {
        (132..=135).find(|code| self.buttons[usize::from(code - 132)])
    }

    fn pressed(&self, code: u8) -> bool {
        match code {
            132..=135 => self.buttons[usize::from(code - 132)],
            136 => self.left,
            137 => self.right,
            138 => self.up,
            139 => self.down,
            _ => false,
        }
    }
}

/// Bindings in up/down/left/right/a/b/c/d order. Empty until loaded. An action reads its
/// keyboard-slot binding OR its joystick-slot binding.
#[derive(Resource, Default)]
pub struct PlayerBindings {
    keyboard: [Option<Binding>; 8],
    joystick: [Option<Binding>; 8],
    /// Installed `joy_tolerance` in gameport loop counts; `None` when no joystick is configured.
    tolerance: Option<i32>,
}

/// Loop count of the original gameport poll at a centered stick, from the nominal 100 kOhm
/// potentiometer (24.2 us + 0.011 us/Ohm at 50 kOhm = 574 us) and one microsecond per port read.
/// Inferred: the executable only compares loop counts, so this converts the installed dead zone
/// to a fraction of a modern stick's half range.
const NOMINAL_CENTER_COUNT: f32 = 574.0;

/// Configured keyboard key_a before Modern movement interpretation.
#[derive(Resource, Default)]
pub struct DialogueSkip(pub bool);

impl PlayerBindings {
    /// Translate documented input codes into Bevy physical keys and mouse buttons
    /// (128 left, 129 right). Zero is unbound. Unsupported device/unknown codes
    /// return an error, rather than silently substituting a different control scheme.
    pub fn from_codes(codes: [u8; 8]) -> Result<Self, String> {
        let mut keyboard = [None; 8];
        for (i, code) in codes.into_iter().enumerate() {
            keyboard[i] = Self::binding(code, i, 0)?;
        }
        Ok(Self {
            keyboard,
            ..Self::default()
        })
    }

    /// Adds the installed joystick slot (`key_*[1]`) and dead zone to these bindings.
    pub fn with_joystick(mut self, codes: [u8; 8], tolerance: i32) -> Result<Self, String> {
        for (i, code) in codes.into_iter().enumerate() {
            self.joystick[i] = Self::binding(code, i, 1)?;
        }
        self.tolerance = Some(tolerance);
        Ok(self)
    }

    fn binding(code: u8, action: usize, slot: u8) -> Result<Option<Binding>, String> {
        Ok(match code {
            0 => None,
            128 => Some(Binding::MouseRelease(MouseButton::Left)),
            129 => Some(Binding::MouseRelease(MouseButton::Right)),
            132..=139 => Some(Binding::Joystick(code)),
            _ => Some(Binding::Key(scan_key(code).ok_or_else(|| {
                format!(
                    "KEYS.INI [input] key_{}[{slot}]: unsupported input code {code}",
                    ["up", "down", "left", "right", "a", "b", "c", "d"][action]
                )
            })?)),
        })
    }

    /// Stick fraction of full deflection beyond which the original's dead-zone test succeeds.
    pub fn stick_threshold(&self) -> Option<f32> {
        self.tolerance
            .map(|tolerance| (tolerance as f32 / NOMINAL_CENTER_COUNT).clamp(0.0, 1.0))
    }

    /// Sample held keys without imposing movement priorities or cancelling
    /// opposing inputs; those belong to the original controller state machine.
    pub fn sample(&self, keys: &ButtonInput<KeyCode>) -> PlayerInput {
        self.sample_with_devices(keys, None, None)
    }

    /// As [`Self::sample`], also reading mouse-button bindings from `mouse` when supplied.
    pub fn sample_with_mouse(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> PlayerInput {
        self.sample_with_devices(keys, mouse, None)
    }

    /// As [`Self::sample_with_mouse`], also reading joystick codes from `joystick` when supplied.
    pub fn sample_with_devices(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
        joystick: Option<&JoystickState>,
    ) -> PlayerInput {
        let held = |binding: Option<Binding>| match binding {
            Some(Binding::Key(key)) => keys.pressed(key),
            Some(Binding::MouseRelease(button)) => {
                mouse.is_some_and(|mouse| mouse.just_released(button))
            }
            Some(Binding::Joystick(code)) => joystick.is_some_and(|state| state.pressed(code)),
            None => false,
        };
        let [forward, backward, left, right, walk, jump, look, action] =
            std::array::from_fn(|i| held(self.keyboard[i]) || held(self.joystick[i]));
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

/// Input code of a mouse button released this update, for Controls capture:
/// 128 for the left button, else 129 for the right, as the original checks them.
pub fn mouse_release_code(mouse: &ButtonInput<MouseButton>) -> Option<u8> {
    [(MouseButton::Left, 128), (MouseButton::Right, 129)]
        .into_iter()
        .find(|(button, _)| mouse.just_released(*button))
        .map(|(_, code)| code)
}

/// The original's shared input table as consumers read it: a keyboard scancode held, a mouse
/// button release (128 or 129, one update) or a joystick code (132 to 139, held). Every
/// rebindable action reads its code through this, whatever the device. A joystick press edge
/// is **Inferred** from the table: the original's consumers clear a code once they consume it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct InputCodes<'w> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    joystick: Option<Res<'w, JoystickState>>,
}

impl InputCodes<'_> {
    /// Whether the table entry for `code` is set this update.
    pub fn pressed(&self, code: u8) -> bool {
        self.read(
            code,
            |keys, key| keys.pressed(key),
            |state| state.pressed(code),
        )
    }

    /// Whether `code` was set this update and not at the previous one.
    pub fn just_pressed(&self, code: u8) -> bool {
        self.read(
            code,
            |keys, key| keys.just_pressed(key),
            |state| state.just_pressed(code),
        )
    }

    fn read(
        &self,
        code: u8,
        key: impl Fn(&ButtonInput<KeyCode>, KeyCode) -> bool,
        joystick: impl Fn(&JoystickState) -> bool,
    ) -> bool {
        match code {
            128 => self.mouse_released(MouseButton::Left),
            129 => self.mouse_released(MouseButton::Right),
            132..=139 => self.joystick.as_deref().is_some_and(joystick),
            _ => scan_key(code)
                .zip(self.keys.as_deref())
                .is_some_and(|(scan, keys)| key(keys, scan)),
        }
    }

    fn mouse_released(&self, button: MouseButton) -> bool {
        self.mouse
            .as_ref()
            .is_some_and(|mouse| mouse.just_released(button))
    }
}

// Bevy supplies the independent input locks and output resources.
#[allow(clippy::too_many_arguments)]
pub(super) fn sample(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    joystick: Res<JoystickState>,
    bindings: Res<PlayerBindings>,
    suppressed: Res<InputSuppressed>,
    pending_world: Option<Res<cyrus_world::lifecycle::WorldPreparationPending>>,
    logbook: Option<Res<crate::logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    fidelity: Res<Fidelity>,
    mut input: ResMut<PlayerInput>,
    mut dialogue_skip: ResMut<DialogueSkip>,
) {
    if suppressed.0
        || pending_world.is_some()
        || logbook.is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open)
    {
        *input = PlayerInput::default();
        dialogue_skip.0 = false;
        return;
    }
    *input = bindings.sample_with_devices(&keys, mouse.as_deref(), Some(&joystick));
    dialogue_skip.0 = input.walk;
    if fidelity.player_controller == Mode::Modern {
        input.forward = keys.pressed(KeyCode::KeyW);
        input.backward = keys.pressed(KeyCode::KeyS);
        input.left = keys.pressed(KeyCode::KeyA);
        input.right = keys.pressed(KeyCode::KeyD);
        input.jump = keys.pressed(KeyCode::Space);
        input.walk = !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    }
}

/// Reduces connected gamepads to the original's digital joystick table: the left stick or
/// D-pad against the installed dead zone, and the four face buttons as buttons 1 to 4.
/// With no gamepad every entry reads zero, as with the original's disabled joystick.
pub(super) fn poll_gamepads(
    gamepads: Query<&Gamepad>,
    bindings: Res<PlayerBindings>,
    mut state: ResMut<JoystickState>,
) {
    let threshold = bindings.stick_threshold().unwrap_or(1.0);
    let mut next = JoystickState::default();
    for pad in &gamepads {
        let stick = pad.left_stick();
        next.left |= stick.x < -threshold || pad.pressed(GamepadButton::DPadLeft);
        next.right |= stick.x > threshold || pad.pressed(GamepadButton::DPadRight);
        // Bevy's stick Y is positive upward; the original's up is the reading below center.
        next.up |= stick.y > threshold || pad.pressed(GamepadButton::DPadUp);
        next.down |= stick.y < -threshold || pad.pressed(GamepadButton::DPadDown);
        for (held, button) in next.buttons.iter_mut().zip([
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::West,
            GamepadButton::North,
        ]) {
            *held |= pad.pressed(button);
        }
    }
    next.previous = state.mask();
    if *state != next {
        *state = next;
    }
}

/// Explicit inspection/UI input lock, sampled before gameplay input.
#[derive(Resource, Default)]
pub struct InputSuppressed(pub bool);
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputSample;

#[cfg(test)]
mod tests {
    use super::*;

    const KEYBOARD: [u8; 8] = [72, 80, 75, 77, 42, 57, 56, 29];
    // The installed joystick slot: up, down, left, right, then buttons 4, 2, 3, 1.
    const JOYSTICK: [u8; 8] = [138, 139, 136, 137, 135, 133, 134, 132];

    fn bindings() -> PlayerBindings {
        PlayerBindings::from_codes(KEYBOARD)
            .unwrap()
            .with_joystick(JOYSTICK, 60)
            .unwrap()
    }

    fn poll(setup: impl FnOnce(&mut Gamepad)) -> JoystickState {
        let mut app = App::new();
        app.insert_resource(bindings())
            .init_resource::<JoystickState>()
            .add_systems(Update, poll_gamepads);
        let mut pad = Gamepad::default();
        setup(&mut pad);
        app.world_mut().spawn(pad);
        app.update();
        *app.world().resource::<JoystickState>()
    }

    #[test]
    fn stick_beyond_the_dead_zone_reads_as_the_original_digital_codes() {
        let threshold = bindings().stick_threshold().unwrap();
        assert!((threshold - 60.0 / 574.0).abs() < 1e-6);
        let stick = |x: f32, y: f32| {
            poll(|pad| {
                pad.analog_mut().set(GamepadAxis::LeftStickX, x);
                pad.analog_mut().set(GamepadAxis::LeftStickY, y);
            })
        };
        assert_eq!(
            stick(threshold - 0.01, -(threshold - 0.01)),
            JoystickState::default()
        );
        let left_up = stick(-(threshold + 0.01), threshold + 0.01);
        assert!(left_up.left && left_up.up && !left_up.right && !left_up.down);
        let right_down = stick(threshold + 0.01, -(threshold + 0.01));
        assert!(right_down.right && right_down.down && !right_down.left && !right_down.up);
    }

    #[test]
    fn face_buttons_and_disconnection_follow_the_input_table() {
        let state = poll(|pad| {
            pad.digital_mut().press(GamepadButton::South);
            pad.digital_mut().press(GamepadButton::North);
        });
        assert_eq!(state.buttons, [true, false, false, true]);
        let mut app = App::new();
        app.insert_resource(bindings())
            .insert_resource(JoystickState {
                left: true,
                ..default()
            })
            .add_systems(Update, poll_gamepads);
        app.update();
        assert_eq!(
            *app.world().resource::<JoystickState>(),
            JoystickState {
                previous: 1 << 4,
                ..default()
            }
        );
    }

    #[test]
    fn input_codes_read_keys_mouse_releases_and_joystick_edges() {
        fn read(world: &mut World, code: u8) -> (bool, bool) {
            let mut state = bevy::ecs::system::SystemState::<InputCodes>::new(world);
            let codes = state.get(world).unwrap();
            (codes.pressed(code), codes.just_pressed(code))
        }
        let mut world = World::new();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<JoystickState>();
        assert_eq!(read(&mut world, 129), (false, false));
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyI);
        assert_eq!(read(&mut world, 23), (true, true));
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        assert_eq!(read(&mut world, 129), (false, false));
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Right);
        assert_eq!(read(&mut world, 129), (true, true));
        // A joystick button held across polls is pressed on both and new only on the first.
        let mut state = JoystickState::default();
        state.buttons[1] = true;
        *world.resource_mut::<JoystickState>() = state;
        assert_eq!(read(&mut world, 133), (true, true));
        state.previous = state.mask();
        *world.resource_mut::<JoystickState>() = state;
        assert_eq!(read(&mut world, 133), (true, false));
    }

    #[test]
    fn controls_capture_reads_the_first_held_joystick_button() {
        let mut state = JoystickState::default();
        assert_eq!(state.button_code(), None);
        state.buttons[2] = true;
        state.buttons[1] = true;
        assert_eq!(state.button_code(), Some(133));
    }

    #[test]
    fn an_action_reads_its_keyboard_or_joystick_binding() {
        let bindings = bindings();
        let keys = ButtonInput::<KeyCode>::default();
        let held = JoystickState {
            up: true,
            buttons: [true, false, false, false],
            ..default()
        };
        let input = bindings.sample_with_devices(&keys, None, Some(&held));
        assert!(input.forward && input.action && !input.backward && !input.jump);
        assert_eq!(bindings.sample(&keys), PlayerInput::default());
        let mut pressed = ButtonInput::<KeyCode>::default();
        pressed.press(KeyCode::ArrowDown);
        assert!(
            bindings
                .sample_with_devices(&pressed, None, Some(&held))
                .backward
        );
    }

    #[test]
    fn joystick_slot_rejects_unknown_codes_with_the_slot_named() {
        let mut codes = JOYSTICK;
        codes[2] = 76;
        let error = PlayerBindings::default()
            .with_joystick(codes, 60)
            .err()
            .unwrap();
        assert!(error.contains("key_left[1]"), "{error}");
    }
}
