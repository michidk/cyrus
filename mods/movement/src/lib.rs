//! WASD moves the player relative to the camera, Space jumps and Left Shift runs.
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

use bindings::cyrus::mods::components::{Input, Movement};
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
        ]);
        app.add_systems(&Schedule::Update, &[&steer]);
    }

    fn steer(query: Query) {
        while let Some(row) = query.iter() {
            let input = Input::new(row.component(0));
            let movement = Movement::new(row.component(1));
            let axis =
                |plus, minus| f32::from(input.pressed(plus)) - f32::from(input.pressed(minus));
            let (x, y) = (axis("KeyD", "KeyA"), axis("KeyW", "KeyS"));
            let tilt = if input.pressed("ShiftLeft") || input.pressed("ShiftRight") {
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
            movement.steer(x, y, input.pressed("Space"));
        }
    }
}

bindings::export!(Steering with_types_in bindings);
