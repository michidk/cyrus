//! Gamepad controls the original's game port does not have, pressed as keyboard keys.
use bevy::prelude::*;
use cyrus_script::player::input::{Action, Input, PlayerBindings};

/// Stick tilt that presses an arrow key, and the lower tilt that keeps a pressed one held, so a
/// stick resting near the edge does not chatter.
const MENU_STICK_PRESS: f32 = 0.5;
const MENU_STICK_HOLD: f32 = 0.375;

/// A gamepad's controls the original's gameport does not have, pressed as the keys the game
/// already reads, the same in every fidelity mode. In the world, Start opens the book (Escape)
/// and Back the inventory (its installed key); the face buttons and the stick stay the joystick
/// codes the player's gamepad poll fills in, which the Controls page can rebind. Outside the
/// world, and over the logbook or the inventory, the D-pad or left stick holds the arrow keys, South holds
/// Enter, and East and Start hold Escape. Only keys this system pressed are released, also when
/// the window loses focus or the pad disconnects.
#[allow(clippy::too_many_arguments)] // Bevy supplies each independent input as a parameter.
pub(super) fn gamepad_keys(
    state: Option<Res<State<cyrus_game::AppState>>>,
    logbook: Option<Res<cyrus_script::logbook::LogbookScreen>>,
    inventory: Option<Res<cyrus_script::inventory_screen::InventoryScreen>>,
    bindings: Option<Res<PlayerBindings>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window>,
    keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mut held: Local<Vec<KeyCode>>,
) {
    let Some(mut keys) = keys else {
        return;
    };
    let in_world = state.is_some_and(|state| *state.get() == cyrus_game::AppState::InGame);
    let menu = !in_world
        || logbook.is_some_and(|screen| screen.suspended())
        || inventory.is_some_and(|screen| screen.open);
    // A headless host has no window; a windowed one reads the pad only while focused.
    let focused = windows.is_empty() || windows.iter().any(|window| window.focused);
    let inventory_key = match bindings.and_then(|bindings| bindings.keyboard(Action::Inventory)) {
        Some(Input::Key(scan)) => Some(scan.key()),
        _ => None,
    };
    let mut wanted = Vec::new();
    for pad in gamepads.iter().filter(|_| focused) {
        if pad.pressed(GamepadButton::Start) {
            wanted.push(KeyCode::Escape);
        }
        if in_world
            && pad.pressed(GamepadButton::Select)
            && let Some(key) = inventory_key
        {
            wanted.push(key);
        }
        if !menu {
            continue;
        }
        let stick = pad.left_stick();
        for (key, button, tilt) in [
            (KeyCode::ArrowUp, GamepadButton::DPadUp, stick.y),
            (KeyCode::ArrowDown, GamepadButton::DPadDown, -stick.y),
            (KeyCode::ArrowLeft, GamepadButton::DPadLeft, -stick.x),
            (KeyCode::ArrowRight, GamepadButton::DPadRight, stick.x),
        ] {
            let threshold = if held.contains(&key) {
                MENU_STICK_HOLD
            } else {
                MENU_STICK_PRESS
            };
            if pad.pressed(button) || tilt >= threshold {
                wanted.push(key);
            }
        }
        for (button, key) in [
            (GamepadButton::South, KeyCode::Enter),
            (GamepadButton::East, KeyCode::Escape),
        ] {
            if pad.pressed(button) {
                wanted.push(key);
            }
        }
    }
    held.retain(|key| {
        let keep = wanted.contains(key);
        if !keep {
            keys.release(*key);
        }
        keep
    });
    for key in wanted {
        // A key the keyboard already holds stays the keyboard's to release.
        if !keys.pressed(key) {
            keys.press(key);
            held.push(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_game::AppState;
    use cyrus_game::fidelity::{Fidelity, Mode};

    #[test]
    fn a_gamepad_presses_menu_keys_outside_the_world_and_start_and_back_in_it() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppState>()
            .init_resource::<Fidelity>()
            .init_resource::<ButtonInput<KeyCode>>()
            // The GOG bindings, with the inventory on I (scan code 23).
            .insert_resource(
                PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29, 23]).unwrap(),
            )
            .add_systems(Update, gamepad_keys);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        let pressed = |app: &App, key| app.world().resource::<ButtonInput<KeyCode>>().pressed(key);
        let set = |app: &mut App, change: &dyn Fn(&mut Gamepad)| {
            change(&mut app.world_mut().get_mut::<Gamepad>(pad).unwrap());
            app.update();
        };
        app.update();
        // Outside the world the pad navigates: the D-pad and South, with the keyboard's own Escape
        // left alone.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        set(&mut app, &|pad| {
            pad.digital_mut().press(GamepadButton::DPadDown);
            pad.digital_mut().press(GamepadButton::South);
        });
        assert!(pressed(&app, KeyCode::ArrowDown) && pressed(&app, KeyCode::Enter));
        set(&mut app, &|pad| {
            pad.digital_mut().release(GamepadButton::DPadDown);
            pad.digital_mut().release(GamepadButton::South);
        });
        assert!(!pressed(&app, KeyCode::ArrowDown) && !pressed(&app, KeyCode::Enter));
        assert!(
            pressed(&app, KeyCode::Escape),
            "the keyboard's Escape stays held"
        );
        // The stick presses past half its tilt and keeps the arrow down to three eighths.
        set(&mut app, &|pad| {
            pad.analog_mut().set(GamepadAxis::LeftStickX, 0.45);
        });
        assert!(!pressed(&app, KeyCode::ArrowRight));
        set(&mut app, &|pad| {
            pad.analog_mut().set(GamepadAxis::LeftStickX, 0.6);
        });
        assert!(pressed(&app, KeyCode::ArrowRight));
        set(&mut app, &|pad| {
            pad.analog_mut().set(GamepadAxis::LeftStickX, 0.4);
        });
        assert!(pressed(&app, KeyCode::ArrowRight));
        set(&mut app, &|pad| {
            pad.analog_mut().set(GamepadAxis::LeftStickX, 0.3);
        });
        assert!(!pressed(&app, KeyCode::ArrowRight));
        // In the world, every fidelity mode alike: South is the gameport's, Start is Escape and Back
        // the installed inventory key.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Escape);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        for area in cyrus_game::fidelity::Area::ALL {
            app.world_mut()
                .resource_mut::<Fidelity>()
                .set(area, Mode::Modern);
        }
        app.update();
        set(&mut app, &|pad| {
            pad.digital_mut().press(GamepadButton::South);
            pad.digital_mut().press(GamepadButton::Start);
            pad.digital_mut().press(GamepadButton::Select);
        });
        assert!(!pressed(&app, KeyCode::Enter));
        assert!(pressed(&app, KeyCode::Escape) && pressed(&app, KeyCode::KeyI));
        set(&mut app, &|pad| {
            pad.digital_mut().release(GamepadButton::Start);
            pad.digital_mut().release(GamepadButton::Select);
        });
        assert!(!pressed(&app, KeyCode::Escape) && !pressed(&app, KeyCode::KeyI));
    }
}
