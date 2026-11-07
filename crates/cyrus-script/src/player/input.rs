//! Keyboard and mouse-button input. Binding semantics, scan codes and the input state
//! table are documented in redguard-preservation's `docs/config/keys-ini.md`.
//! Joystick codes 132 to 139 read a [`JoystickState`] that the gamepad adapter fills in the
//! original's digital terms; the analog axes are reduced to the original's dead-zone test.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use cyrus_game::fidelity::{Fidelity, Mode};
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

/// The keys Modern movement reads, forward, backward, left and right.
pub const MODERN_MOVEMENT: [KeyCode; 4] =
    [KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD];

impl PlayerInput {
    /// Modern camera-relative movement: the held directions, or a tilted `stick` (x right,
    /// y forward, beyond the dead zone), read relative to `view_heading`, become forward plus
    /// the heading to steer toward. Walk stays the Modern walking request, which a stick tilted
    /// less than [`RUN_TILT`] also makes, so the controller must see a cleared walk-mode toggle.
    pub fn camera_relative(
        &self,
        view_heading: u16,
        stick: Option<Vec2>,
    ) -> (PlayerInput, Option<u16>) {
        let across = i16::from(self.right) - i16::from(self.left);
        let along = i16::from(self.forward) - i16::from(self.backward);
        // Turning right lowers an engine heading, so right of the view is a quarter turn below it.
        let offset = match (stick, across, along) {
            // Wrapping into u16 keeps the heading modulo a turn.
            (Some(stick), ..) => {
                Some((f32::atan2(-stick.x, stick.y) * HEADING_PER_RADIAN).round() as i32 as u16)
            }
            (None, 0, 0) => None,
            (None, 0, 1) => Some(0),
            (None, -1, 1) => Some(256),
            (None, -1, 0) => Some(512),
            (None, -1, -1) => Some(768),
            (None, 0, -1) => Some(1024),
            (None, 1, -1) => Some(1280),
            (None, 1, 0) => Some(1536),
            _ => Some(1792),
        };
        let steer =
            offset.map(|offset| cyrus_world::heading::wrap(view_heading.wrapping_add(offset)));
        (
            PlayerInput {
                forward: steer.is_some(),
                backward: false,
                left: false,
                right: false,
                action: self.action,
                jump: self.jump,
                look: self.look,
                walk: self.walk && stick.is_none_or(|stick| stick.length() < RUN_TILT),
            },
            steer,
        )
    }

    /// Modern held directions for a topic menu, which reads forward and back as up and down:
    /// the keys as held, plus the left stick's dominant vertical tilt.
    pub fn menu_directions(&self, stick: Option<Vec2>) -> PlayerInput {
        let vertical = stick.filter(|stick| stick.y.abs() > stick.x.abs());
        PlayerInput {
            forward: self.forward || vertical.is_some_and(|stick| stick.y > 0.0),
            backward: self.backward || vertical.is_some_and(|stick| stick.y < 0.0),
            ..*self
        }
    }
}

/// Modern analog input not yet taken by an ordinary update.
#[derive(Resource, Default, Debug)]
pub struct ModernLook {
    /// Camera turn from mouse travel and the right stick, in 1/2048 turns, positive to the left
    /// as engine headings turn.
    pub turn: f32,
    /// The left stick beyond the dead zone (x right, y forward), for Modern movement.
    pub stick: Option<Vec2>,
    /// A mod's [`MovementWish`] is steering, so movement reads camera-relative whatever the
    /// controller mode.
    pub driven: bool,
}

/// Movement a mod asks for this frame, replacing the original keys. `None` leaves movement
/// to the original controls (or Modern, when selected), so no mod active means no change.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct MovementWish(pub Option<Wish>);

/// How fast a claimed player moves: the walk or the run animation and motion class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    Walk,
    Run,
}

/// A mod's ownership of the selected player's ground locomotion for one update. While it is
/// claimed the movement bindings read as released and the original turn, walk, run, sidestep and
/// jump selection do not run; the controller still handles death, dialogue, combat and idle, and
/// the engine's ground, slope and collision stages still resolve the displacement. A claim
/// does not apply while the player is airborne, sliding, hanging or under script control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocomotionClaim {
    /// Heading to face, in 1/2048 turns as engine headings count.
    pub heading: u16,
    /// Distance to travel along the heading per nominal update, in map units like the actor's
    /// speed attributes. Zero or less stands still.
    pub speed: f32,
    pub gait: Gait,
    /// Starts the original jump entry for the current gait.
    pub jump: bool,
    pub tuning: PlayerTuning,
}

/// Scales of the player's jump, applied while a mod claims locomotion. They take effect only
/// while the player is on the ground, so a jump in flight keeps the values it started with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerTuning {
    /// Times `attr_jump_height`, the height of a jump and the descent of a fall, clamped to the
    /// attribute's byte range.
    pub jump_height: f32,
    /// Times the forward speed of a walking or running jump; its distance cap is unchanged.
    pub jump_speed: f32,
}

impl Default for PlayerTuning {
    fn default() -> Self {
        Self {
            jump_height: 1.0,
            jump_speed: 1.0,
        }
    }
}

/// The claim a mod made for the next update; the game consumes it and hands control back as
/// soon as a mod stops claiming.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct LocomotionClaimed(pub Option<LocomotionClaim>);

/// A mod's movement request, read like a tilted left stick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wish {
    /// Direction relative to the camera (x right, y forward). Zero asks for no movement.
    /// Below [`RUN_TILT`] the player walks, from it the player runs.
    pub direction: Vec2,
    pub jump: bool,
    /// The mod reads W, A, S and D for this, so an installed quick action on one of them (the
    /// sword's S by default) stays quiet.
    pub wasd: bool,
}

const HEADING_PER_RADIAN: f32 = 2048.0 / std::f32::consts::TAU;
/// Heading units per pixel of mouse travel: the inspection flycam's 0.0025 radians.
const MOUSE_TURN: f32 = 0.0025 * HEADING_PER_RADIAN;
/// Heading units per second of a fully tilted right stick: half a turn.
const STICK_TURN: f32 = 1024.0;
/// Left-stick tilt from which Modern movement runs.
pub const RUN_TILT: f32 = 0.75;

/// Retained gameplay toggles, initialized from installed HUD data or saved preferences.
/// The HUD projects these values; world replacement must not reset them.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PlayerToggles {
    pub walk_mode: bool,
    pub auto_defend: bool,
}

/// The fifteen rebindable actions, in `KEYS.INI` `[input]` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    /// `key_a`: the walk/run modifier.
    Walk,
    /// `key_b`.
    Jump,
    /// `key_c`: look.
    View,
    /// `key_d`: activate or attack.
    Use,
    Inventory,
    QuickSword,
    QuickHealth,
    Map,
    Log,
    NextItem,
    PreviousItem,
}

impl Action {
    pub const ALL: [Self; 15] = [
        Self::Forward,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Walk,
        Self::Jump,
        Self::View,
        Self::Use,
        Self::Inventory,
        Self::QuickSword,
        Self::QuickHealth,
        Self::Map,
        Self::Log,
        Self::NextItem,
        Self::PreviousItem,
    ];

    /// The action's `KEYS.INI` key; the eight movement actions also have a `[1]` joystick slot.
    pub fn keys_ini_name(self) -> &'static str {
        match self {
            Self::Forward => "key_up",
            Self::Back => "key_down",
            Self::Left => "key_left",
            Self::Right => "key_right",
            Self::Walk => "key_a",
            Self::Jump => "key_b",
            Self::View => "key_c",
            Self::Use => "key_d",
            Self::Inventory => "inventory_key",
            Self::QuickSword => "quick_sword_key",
            Self::QuickHealth => "quick_health_key",
            Self::Map => "map_key",
            Self::Log => "log_key",
            Self::NextItem => "next_key",
            Self::PreviousItem => "prev_key",
        }
    }
}

/// A keyboard scan code (1 to 127) that names a physical key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScanCode(u8);

impl ScanCode {
    pub fn key(self) -> KeyCode {
        scan_key(self.0).expect("a ScanCode is only built from a mapped code")
    }
}

impl TryFrom<u8> for ScanCode {
    type Error = UnsupportedInput;
    fn try_from(code: u8) -> Result<Self, Self::Error> {
        scan_key(code)
            .map(|_| Self(code))
            .ok_or(UnsupportedInput(code))
    }
}

impl TryFrom<KeyCode> for ScanCode {
    type Error = KeyCode;
    fn try_from(key: KeyCode) -> Result<Self, Self::Error> {
        (1..=127)
            .find(|code| scan_key(*code) == Some(key))
            .map(Self)
            .ok_or(key)
    }
}

/// A mouse button the input table reads: true for the update in which it is released.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mouse {
    Left,
    Right,
}

impl Mouse {
    pub const ALL: [Self; 2] = [Self::Left, Self::Right];
    pub fn button(self) -> MouseButton {
        match self {
            Self::Left => MouseButton::Left,
            Self::Right => MouseButton::Right,
        }
    }
}

/// A digital joystick entry: four buttons, then the stick beyond the dead zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Joystick {
    Button1,
    Button2,
    Button3,
    Button4,
    Left,
    Right,
    Up,
    Down,
}

impl Joystick {
    /// In input-code order, 132 to 139.
    pub const ALL: [Self; 8] = [
        Self::Button1,
        Self::Button2,
        Self::Button3,
        Self::Button4,
        Self::Left,
        Self::Right,
        Self::Up,
        Self::Down,
    ];
    pub const BUTTONS: [Self; 4] = [Self::Button1, Self::Button2, Self::Button3, Self::Button4];
}

/// One entry of the original's input table, as `KEYS.INI` and saved bindings name it by code:
/// a scan code, mouse release 128 or 129, or joystick 132 to 139.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    Key(ScanCode),
    Mouse(Mouse),
    Joystick(Joystick),
}

/// An input code with no table entry (zero means unbound and is not an input).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedInput(pub u8);

impl std::fmt::Display for UnsupportedInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported input code {}", self.0)
    }
}

impl std::error::Error for UnsupportedInput {}

impl TryFrom<u8> for Input {
    type Error = UnsupportedInput;
    fn try_from(code: u8) -> Result<Self, Self::Error> {
        Ok(match code {
            128 => Self::Mouse(Mouse::Left),
            129 => Self::Mouse(Mouse::Right),
            132..=139 => Self::Joystick(Joystick::ALL[usize::from(code - 132)]),
            _ => Self::Key(ScanCode::try_from(code)?),
        })
    }
}

impl TryFrom<KeyCode> for Input {
    type Error = KeyCode;
    fn try_from(key: KeyCode) -> Result<Self, Self::Error> {
        ScanCode::try_from(key).map(Self::Key)
    }
}

impl From<Input> for u8 {
    fn from(input: Input) -> Self {
        match input {
            Input::Key(ScanCode(code)) => code,
            Input::Mouse(Mouse::Left) => 128,
            Input::Mouse(Mouse::Right) => 129,
            Input::Joystick(joystick) => 132 + joystick as u8,
        }
    }
}

impl Input {
    /// Parses a stored binding, where zero means unbound.
    pub fn from_binding(code: u8) -> Result<Option<Self>, UnsupportedInput> {
        (code != 0).then(|| Self::try_from(code)).transpose()
    }

    /// Keys the Controls page refuses to bind.
    pub fn reserved(self) -> bool {
        matches!(
            self,
            Self::Key(ScanCode(12 | 13 | 15 | 28 | 59..=68 | 87 | 88))
        )
    }

    /// The input a Controls page capture takes this update, as the original's wait-for-key
    /// accepts it: the lowest pressed scan code, else a mouse release, else a joystick button.
    pub fn captured(devices: Devices) -> Option<Self> {
        let key = devices.keys.and_then(|keys| {
            keys.get_just_pressed()
                .filter_map(|key| ScanCode::try_from(*key).ok())
                .min_by_key(|scan| scan.0)
                .map(Self::Key)
        });
        key.or_else(|| {
            Mouse::ALL
                .into_iter()
                .map(Self::Mouse)
                .find(|input| devices.just_pressed(*input))
        })
        .or_else(|| {
            Joystick::BUTTONS
                .into_iter()
                .map(Self::Joystick)
                .find(|input| devices.pressed(*input))
        })
    }
}

/// Digital joystick input table entries, written by the gamepad adapter before sampling.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoystickState {
    held: [bool; 8],
    /// Entries at the previous poll, for press edges.
    previous: [bool; 8],
}

impl JoystickState {
    pub fn pressed(&self, entry: Joystick) -> bool {
        self.held[entry as usize]
    }

    pub fn just_pressed(&self, entry: Joystick) -> bool {
        self.pressed(entry) && !self.previous[entry as usize]
    }

    /// A state holding `entries`, with nothing held at the previous poll.
    pub fn holding(entries: impl IntoIterator<Item = Joystick>) -> Self {
        let mut state = Self::default();
        for entry in entries {
            state.held[entry as usize] = true;
        }
        state
    }
}

/// The input devices one update reads. Mouse entries are the original's release edge: true
/// for the update in which the button goes up. A joystick press edge is **Inferred** from the
/// table: the original's consumers clear an entry once they consume it.
#[derive(Clone, Copy, Default)]
pub struct Devices<'a> {
    pub keys: Option<&'a ButtonInput<KeyCode>>,
    pub mouse: Option<&'a ButtonInput<MouseButton>>,
    pub joystick: Option<&'a JoystickState>,
}

impl Devices<'_> {
    /// Whether the table entry for `input` is set this update.
    pub fn pressed(&self, input: Input) -> bool {
        match input {
            Input::Key(scan) => self.keys.is_some_and(|keys| keys.pressed(scan.key())),
            Input::Mouse(mouse) => self.released(mouse),
            Input::Joystick(entry) => self.joystick.is_some_and(|state| state.pressed(entry)),
        }
    }

    /// Whether `input` was set this update and not at the previous one.
    pub fn just_pressed(&self, input: Input) -> bool {
        match input {
            Input::Key(scan) => self.keys.is_some_and(|keys| keys.just_pressed(scan.key())),
            Input::Mouse(mouse) => self.released(mouse),
            Input::Joystick(entry) => self.joystick.is_some_and(|state| state.just_pressed(entry)),
        }
    }

    /// Whether `mouse` is held down now, for a consumer that polls on its own clock.
    pub fn mouse_held(&self, mouse: Mouse) -> bool {
        self.mouse
            .is_some_and(|buttons| buttons.pressed(mouse.button()))
    }

    fn released(&self, mouse: Mouse) -> bool {
        self.mouse
            .is_some_and(|buttons| buttons.just_released(mouse.button()))
    }
}

/// The input devices and the player's bindings as a system parameter. Without loaded
/// bindings every action reads as unbound.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Inputs<'w> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    joystick: Option<Res<'w, JoystickState>>,
    bindings: Option<Res<'w, PlayerBindings>>,
}

static UNBOUND: PlayerBindings = PlayerBindings {
    keyboard: [None; 15],
    joystick: [None; 8],
    tolerance: None,
};

impl Inputs<'_> {
    pub fn devices(&self) -> Devices<'_> {
        Devices {
            keys: self.keys.as_deref(),
            mouse: self.mouse.as_deref(),
            joystick: self.joystick.as_deref(),
        }
    }

    pub fn bindings(&self) -> &PlayerBindings {
        self.bindings.as_deref().unwrap_or(&UNBOUND)
    }

    /// Whether any input bound to `action` is set this update.
    pub fn pressed(&self, action: Action) -> bool {
        self.bindings().pressed(action, self.devices())
    }

    /// Whether any input bound to `action` was set this update and not at the previous one.
    pub fn just_pressed(&self, action: Action) -> bool {
        self.bindings().just_pressed(action, self.devices())
    }
}

/// The installed or saved binding slots of every action. Each action reads its keyboard-slot
/// input OR its joystick-slot input (`key_*[1]`, movement actions only). Empty until loaded.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct PlayerBindings {
    keyboard: [Option<Input>; 15],
    joystick: [Option<Input>; 8],
    /// Installed `joy_tolerance` in gameport loop counts; `None` when no joystick is configured.
    tolerance: Option<i32>,
}

/// Keyboard-slot bindings for the listed actions; every other action is unbound.
impl FromIterator<(Action, Input)> for PlayerBindings {
    fn from_iter<I: IntoIterator<Item = (Action, Input)>>(iter: I) -> Self {
        let mut bindings = Self::default();
        for (action, input) in iter {
            bindings.keyboard[action as usize] = Some(input);
        }
        bindings
    }
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
    /// Binds the keyboard slot from stored codes in [`Action::ALL`] order; zero is unbound and
    /// any codes past the fifteenth are ignored. An unsupported code is an error naming its
    /// `KEYS.INI` key, rather than a silently substituted control scheme.
    pub fn from_codes(codes: impl IntoIterator<Item = u8>) -> Result<Self, String> {
        let mut bindings = Self::default();
        bindings.set_keyboard(codes)?;
        Ok(bindings)
    }

    /// Replaces the keyboard slot, keeping the joystick slot and dead zone.
    pub fn set_keyboard(&mut self, codes: impl IntoIterator<Item = u8>) -> Result<(), String> {
        let mut keyboard = [None; 15];
        for ((slot, action), code) in keyboard.iter_mut().zip(Action::ALL).zip(codes) {
            *slot = Self::parse(action, 0, code)?;
        }
        self.keyboard = keyboard;
        Ok(())
    }

    /// Adds the installed joystick slot (`key_*[1]`) and dead zone to these bindings.
    pub fn with_joystick(mut self, codes: [u8; 8], tolerance: i32) -> Result<Self, String> {
        for ((slot, action), code) in self.joystick.iter_mut().zip(Action::ALL).zip(codes) {
            *slot = Self::parse(action, 1, code)?;
        }
        self.tolerance = Some(tolerance);
        Ok(self)
    }

    fn parse(action: Action, slot: u8, code: u8) -> Result<Option<Input>, String> {
        Input::from_binding(code).map_err(|error| {
            format!(
                "KEYS.INI [input] {}[{slot}]: {error}",
                action.keys_ini_name()
            )
        })
    }

    /// The keyboard-slot input of `action`.
    pub fn keyboard(&self, action: Action) -> Option<Input> {
        self.keyboard[action as usize]
    }

    /// The inputs bound to `action`.
    pub fn inputs(&self, action: Action) -> impl Iterator<Item = Input> + '_ {
        let index = action as usize;
        [self.keyboard.get(index), self.joystick.get(index)]
            .into_iter()
            .flatten()
            .flatten()
            .copied()
    }

    pub fn pressed(&self, action: Action, devices: Devices) -> bool {
        self.inputs(action).any(|input| devices.pressed(input))
    }

    pub fn just_pressed(&self, action: Action, devices: Devices) -> bool {
        self.inputs(action).any(|input| devices.just_pressed(input))
    }

    /// Stick fraction of full deflection beyond which the original's dead-zone test succeeds.
    pub fn stick_threshold(&self) -> Option<f32> {
        self.tolerance
            .map(|tolerance| (tolerance as f32 / NOMINAL_CENTER_COUNT).clamp(0.0, 1.0))
    }

    /// Sample held keys without imposing movement priorities or cancelling
    /// opposing inputs; those belong to the original controller state machine.
    pub fn sample(&self, keys: &ButtonInput<KeyCode>) -> PlayerInput {
        self.sample_devices(Devices {
            keys: Some(keys),
            ..default()
        })
    }

    /// As [`Self::sample`], also reading mouse-button bindings from `mouse` when supplied.
    pub fn sample_with_mouse(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> PlayerInput {
        self.sample_devices(Devices {
            keys: Some(keys),
            mouse,
            joystick: None,
        })
    }

    /// The eight movement actions as read from `devices`.
    pub fn sample_devices(&self, devices: Devices) -> PlayerInput {
        let held = |action| self.pressed(action, devices);
        PlayerInput {
            forward: held(Action::Forward),
            backward: held(Action::Back),
            left: held(Action::Left),
            right: held(Action::Right),
            action: held(Action::Use),
            jump: held(Action::Jump),
            look: held(Action::View),
            walk: held(Action::Walk),
        }
    }
}

fn scan_key(code: u8) -> Option<KeyCode> {
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

/// Mouse code 128 or 129 read by a screen loop that polls input once per update: the button
/// was held at the previous poll and is up at this one, as the original derives release edges
/// between polls.
pub fn polled_release(previous: [bool; 2], now: [bool; 2], mouse: Mouse) -> bool {
    previous[mouse as usize] && !now[mouse as usize]
}

// Bevy supplies the independent input locks and output resources.
#[allow(clippy::too_many_arguments)]
pub(super) fn sample(
    keys: Res<ButtonInput<KeyCode>>,
    inputs: Inputs,
    suppressed: Res<InputSuppressed>,
    pending_world: Option<Res<cyrus_world::lifecycle::WorldPreparationPending>>,
    logbook: Option<Res<crate::logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    fidelity: Res<Fidelity>,
    motion: Option<Res<bevy::input::mouse::AccumulatedMouseMotion>>,
    state: Option<Res<State<cyrus_game::AppState>>>,
    gamepads: Query<&Gamepad>,
    time: Option<Res<Time>>,
    mut input: ResMut<PlayerInput>,
    mut dialogue_skip: ResMut<DialogueSkip>,
    mut look: ResMut<ModernLook>,
    wish: Res<MovementWish>,
    mut claimed: ResMut<LocomotionClaimed>,
) {
    if suppressed.0
        || pending_world.is_some()
        || logbook.is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open)
    {
        *input = PlayerInput::default();
        dialogue_skip.0 = false;
        *look = ModernLook::default();
        // Menus, the console and loading take the player back from a mod.
        claimed.0 = None;
        return;
    }
    // The installed dead zone; an install without a joystick reads no stick, as Faithful does.
    let tilted = |stick: Vec2| {
        inputs
            .bindings()
            .stick_threshold()
            .is_some_and(|threshold| stick.length() > threshold)
            .then_some(stick)
    };
    // Mouse travel over a menu does not turn the camera on return.
    if fidelity.camera == Mode::Modern
        && state.is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
    {
        if let Some(motion) = motion {
            look.turn -= motion.delta.x * MOUSE_TURN;
        }
        let seconds = time.map_or(0.0, |time| time.delta_secs());
        for pad in &gamepads {
            if let Some(stick) = tilted(pad.right_stick()) {
                look.turn -= stick.x * STICK_TURN * seconds;
            }
        }
    }
    look.stick = (fidelity.player_controller == Mode::Modern)
        .then(|| gamepads.iter().find_map(|pad| tilted(pad.left_stick())))
        .flatten();
    *input = inputs.bindings().sample_devices(inputs.devices());
    dialogue_skip.0 = input.walk;
    if fidelity.player_controller == Mode::Modern {
        [input.forward, input.backward, input.left, input.right] =
            MODERN_MOVEMENT.map(|key| keys.pressed(key));
        input.jump |= keys.pressed(KeyCode::Space);
        input.walk = !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    }
    if claimed.0.is_some() {
        // The mod owns locomotion: the original movement bindings read as released.
        let input = &mut *input;
        for held in [
            &mut input.forward,
            &mut input.backward,
            &mut input.left,
            &mut input.right,
            &mut input.jump,
            &mut input.walk,
        ] {
            *held = false;
        }
    }
    look.driven = claimed.0.is_none() && wish.0.is_some();
    if let Some(wish) = wish.0.filter(|_| claimed.0.is_none()) {
        input.jump |= wish.jump;
        if wish.direction != Vec2::ZERO {
            look.stick = Some(wish.direction);
            // The stick's tilt chooses walking or running.
            input.walk = true;
        }
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
    let mut held = [false; 8];
    for pad in &gamepads {
        let stick = pad.left_stick();
        // Bevy's stick Y is positive upward; the original's up is the reading below center.
        let entries = [
            (Joystick::Button1, pad.pressed(GamepadButton::South)),
            (Joystick::Button2, pad.pressed(GamepadButton::East)),
            (Joystick::Button3, pad.pressed(GamepadButton::West)),
            (Joystick::Button4, pad.pressed(GamepadButton::North)),
            (
                Joystick::Left,
                stick.x < -threshold || pad.pressed(GamepadButton::DPadLeft),
            ),
            (
                Joystick::Right,
                stick.x > threshold || pad.pressed(GamepadButton::DPadRight),
            ),
            (
                Joystick::Up,
                stick.y > threshold || pad.pressed(GamepadButton::DPadUp),
            ),
            (
                Joystick::Down,
                stick.y < -threshold || pad.pressed(GamepadButton::DPadDown),
            ),
        ];
        for (entry, pressed) in entries {
            held[entry as usize] |= pressed;
        }
    }
    let next = JoystickState {
        held,
        previous: state.held,
    };
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

    fn held(state: &JoystickState) -> Vec<Joystick> {
        Joystick::ALL
            .into_iter()
            .filter(|entry| state.pressed(*entry))
            .collect()
    }

    #[test]
    fn input_codes_round_trip_through_the_typed_table() {
        for code in (1..=139).filter(|code| Input::try_from(*code).is_ok()) {
            assert_eq!(u8::from(Input::try_from(code).unwrap()), code);
        }
        assert_eq!(Input::try_from(132), Ok(Input::Joystick(Joystick::Button1)));
        assert_eq!(Input::try_from(129), Ok(Input::Mouse(Mouse::Right)));
        assert_eq!(Input::try_from(130), Err(UnsupportedInput(130)));
        assert_eq!(Input::from_binding(0), Ok(None));
        assert!(Input::try_from(28).unwrap().reserved());
        assert!(!Input::try_from(30).unwrap().reserved());
    }

    #[test]
    fn stick_beyond_the_dead_zone_reads_as_the_original_digital_entries() {
        let threshold = bindings().stick_threshold().unwrap();
        assert!((threshold - 60.0 / 574.0).abs() < 1e-6);
        let stick = |x: f32, y: f32| {
            held(&poll(|pad| {
                pad.analog_mut().set(GamepadAxis::LeftStickX, x);
                pad.analog_mut().set(GamepadAxis::LeftStickY, y);
            }))
        };
        assert!(stick(threshold - 0.01, -(threshold - 0.01)).is_empty());
        assert_eq!(
            stick(-(threshold + 0.01), threshold + 0.01),
            [Joystick::Left, Joystick::Up]
        );
        assert_eq!(
            stick(threshold + 0.01, -(threshold + 0.01)),
            [Joystick::Right, Joystick::Down]
        );
    }

    #[test]
    fn face_buttons_and_disconnection_follow_the_input_table() {
        let state = poll(|pad| {
            pad.digital_mut().press(GamepadButton::South);
            pad.digital_mut().press(GamepadButton::North);
        });
        assert_eq!(held(&state), [Joystick::Button1, Joystick::Button4]);
        let mut app = App::new();
        app.insert_resource(bindings())
            .insert_resource(JoystickState::holding([Joystick::Left]))
            .add_systems(Update, poll_gamepads);
        app.update();
        let state = *app.world().resource::<JoystickState>();
        assert!(held(&state).is_empty());
    }

    #[test]
    fn devices_read_keys_mouse_releases_and_joystick_edges() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut mouse = ButtonInput::<MouseButton>::default();
        let read = |devices: Devices, input| (devices.pressed(input), devices.just_pressed(input));
        let right = Input::Mouse(Mouse::Right);
        keys.press(KeyCode::KeyI);
        mouse.press(MouseButton::Right);
        let devices = Devices {
            keys: Some(&keys),
            mouse: Some(&mouse),
            joystick: None,
        };
        assert_eq!(read(devices, Input::try_from(23).unwrap()), (true, true));
        assert_eq!(read(devices, right), (false, false));
        mouse.release(MouseButton::Right);
        let devices = Devices {
            mouse: Some(&mouse),
            ..default()
        };
        assert_eq!(read(devices, right), (true, true));
        // A joystick button held across polls is pressed on both and new only on the first.
        let button = Input::Joystick(Joystick::Button2);
        let first = JoystickState::holding([Joystick::Button2]);
        let devices = Devices {
            joystick: Some(&first),
            ..default()
        };
        assert_eq!(read(devices, button), (true, true));
        let second = JoystickState {
            previous: first.held,
            ..first
        };
        let devices = Devices {
            joystick: Some(&second),
            ..default()
        };
        assert_eq!(read(devices, button), (true, false));
    }

    #[test]
    fn controls_capture_takes_a_key_then_a_mouse_release_then_a_joystick_button() {
        let joystick = JoystickState::holding([Joystick::Button3, Joystick::Button2]);
        let capture = |keys: &ButtonInput<KeyCode>| {
            Input::captured(Devices {
                keys: Some(keys),
                mouse: None,
                joystick: Some(&joystick),
            })
        };
        let mut keys = ButtonInput::<KeyCode>::default();
        assert_eq!(capture(&keys), Some(Input::Joystick(Joystick::Button2)));
        keys.press(KeyCode::KeyS);
        keys.press(KeyCode::KeyW);
        assert_eq!(capture(&keys), Some(Input::try_from(17).unwrap()));
    }

    #[test]
    fn an_action_reads_its_keyboard_or_joystick_binding() {
        let bindings = bindings();
        let keys = ButtonInput::<KeyCode>::default();
        let joystick = JoystickState::holding([Joystick::Up, Joystick::Button1]);
        let devices = |keys| Devices {
            keys: Some(keys),
            mouse: None,
            joystick: Some(&joystick),
        };
        let input = bindings.sample_devices(devices(&keys));
        assert!(input.forward && input.action && !input.backward && !input.jump);
        assert_eq!(bindings.sample(&keys), PlayerInput::default());
        let mut pressed = ButtonInput::<KeyCode>::default();
        pressed.press(KeyCode::ArrowDown);
        assert!(bindings.sample_devices(devices(&pressed)).backward);
    }

    #[test]
    fn rebinding_the_keyboard_slot_keeps_the_joystick_slot() {
        let mut bindings = bindings();
        let mut codes = [0; 15];
        codes[Action::Inventory as usize] = 23;
        bindings.set_keyboard(codes).unwrap();
        assert_eq!(
            bindings.inputs(Action::Forward).collect::<Vec<_>>(),
            [Input::Joystick(Joystick::Up)]
        );
        assert_eq!(
            bindings.inputs(Action::Inventory).collect::<Vec<_>>(),
            [Input::try_from(23).unwrap()]
        );
    }

    #[test]
    fn a_polled_release_needs_the_button_held_at_the_previous_poll() {
        assert!(polled_release([true, false], [false, false], Mouse::Left));
        assert!(!polled_release([true, false], [false, false], Mouse::Right));
        assert!(!polled_release([false, false], [false, false], Mouse::Left));
        assert!(!polled_release([true, true], [true, true], Mouse::Right));
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
