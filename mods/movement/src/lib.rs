//! Moves the player relative to the camera with the game's own key bindings, or with WASD,
//! Space and Shift. The walk key runs (or walks, with the `run_by_default` setting).
mod bindings {
    wasvy_macros::guest_bindings!({
        path: ["../../crates/cyrus-mods/wit", "wit"],
        world: "cyrus:movement/guest",
        with: {
            "wasvy:ecs/app@0.0.11": generate,
            "cyrus:mods/components@0.1.0": generate,
        }
    });
}

use bindings::cyrus::mods::components::{Input, Movement, Player, Settings};
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};

/// Stick tilt that walks: below the host's run threshold of 0.75.
const WALK: f32 = 0.5;

struct Steering;

impl bindings::Guest for Steering {
    fn setup(app: App) {
        let steer = System::new("steer");
        steer.add_query(&[
            QueryFor::Ref(Input::type_path()),
            QueryFor::Mut(Movement::type_path()),
            QueryFor::Ref(Settings::type_path()),
            QueryFor::Ref(Player::type_path()),
        ]);
        app.add_systems(&Schedule::Update, &[&steer]);
    }

    fn steer(query: Query) {
        while let Some(row) = query.iter() {
            let input = Input::new(row.component(0));
            let movement = Movement::new(row.component(1));
            let settings = Settings::new(row.component(2));
            let player = Player::new(row.component(3));
            let run_by_default = settings.flag("movement", "run_by_default");
            let wasd = settings.choice("movement", "keys") == "wasd";
            // Either the game's own bindings (KEYS.INI), or W, A, S and D with Space and Shift.
            let held = |action, key: &str| {
                if wasd {
                    input.pressed(key)
                } else {
                    input.held(action)
                }
            };
            let axis = |plus: bool, minus: bool| f32::from(plus) - f32::from(minus);
            let x = axis(held("right", "KeyD"), held("left", "KeyA"));
            let y = axis(held("forward", "KeyW"), held("back", "KeyS"));
            let modifier = if wasd {
                input.pressed("ShiftLeft") || input.pressed("ShiftRight")
            } else {
                input.held("walk")
            };
            let tilt = if modifier != run_by_default {
                1.0
            } else {
                WALK
            };
            let length = x.hypot(y);
            let (x, y) = if length > 0.0 {
                (x / length * tilt, y / length * tilt)
            } else {
                (0.0, 0.0)
            };
            let jump = held("jump", "Space");
            if settings.flag("movement", "own_movement") {
                // Owning the locomotion: face the way the keys point relative to the camera and
                // move at the player's own walk or run speed, scaled by the setting.
                let heading = if length > 0.0 {
                    // The same offset the game uses: right of the view lowers the heading.
                    let offset = (-x).atan2(y) * 2048.0 / std::f32::consts::TAU;
                    (player.view() as i32 + offset.round() as i32).rem_euclid(2048) as u32
                } else {
                    player.heading()
                };
                let run = tilt >= 0.75;
                let speed = if length > 0.0 {
                    player.speed(run) * settings.number("movement", "speed_scale")
                } else {
                    0.0
                };
                movement.claim(heading, speed, run, jump);
            } else {
                movement.steer(x, y, jump, wasd);
            }
        }
    }
}

bindings::export!(Steering with_types_in bindings);
