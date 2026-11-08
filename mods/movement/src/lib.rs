//! Third-person movement in the style of current action games. The mouse orbits the camera
//! around the player on its own; the keys move the player relative to the camera, at full speed
//! from the first frame, with the animations only showing the movement. With the weapon sheathed
//! the player turns quickly to face where it moves; with it drawn the player keeps facing where
//! the camera looks and backs off or circles instead. Jumping and falling stay the original's.
//! The game's own key bindings, or WASD with Space and Shift; the walk key runs (or walks, with
//! the `run_by_default` setting).
mod steering;
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

use bindings::cyrus::mods::components::{Camera, Input, Memory, Movement, Player, Settings};
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};

const STEERING: &str = "movement/steering";

/// Animation groups (`SOUP386.DEF` numbers) of ground movement that may be cut short: turning,
/// running, walking and sidestepping with and without the torch, and the fight stance's run,
/// forward, back and circling.
const TURNING: [u32; 4] = [14, 15, 16, 17];
const MOVING: [u32; 16] = [
    18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 30, 31, 92, 93, 94, 95,
];

/// Jump take-offs, jumps and landings, sheathed and in the fight stance, and how much faster
/// they play.
const JUMPING: [std::ops::RangeInclusive<u32>; 4] = [32..=50, 65..=72, 111..=112, 114..=114];
const JUMP_TEMPO: f32 = 1.75;

struct Steering;

impl bindings::Guest for Steering {
    fn setup(app: App) {
        let steer = System::new("steer");
        steer.add_query(&[
            QueryFor::Ref(Input::type_path()),
            QueryFor::Mut(Movement::type_path()),
            QueryFor::Ref(Settings::type_path()),
            QueryFor::Ref(Player::type_path()),
            QueryFor::Mut(Camera::type_path()),
            QueryFor::Mut(Memory::type_path()),
        ]);
        app.add_systems(&Schedule::Update, &[&steer]);
    }

    fn steer(query: Query) {
        while let Some(row) = query.iter() {
            let input = Input::new(row.component(0));
            let movement = Movement::new(row.component(1));
            let settings = Settings::new(row.component(2));
            let player = Player::new(row.component(3));
            let camera = Camera::new(row.component(4));
            let memory = Memory::new(row.component(5));
            camera.orbit();
            // No player yet (menus, loading): leave everything to the game.
            if player.position().is_empty() {
                memory.save(STEERING, &[]);
                continue;
            }
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
            let run = modifier != run_by_default;
            let jump = held("jump", "Space");
            if let Some(current) = player.animation().first()
                && JUMPING.iter().any(|groups| groups.contains(current))
            {
                movement.tempo(JUMP_TEMPO);
            }
            let heading = player.heading();
            let mut state = steering::SteeringState::load(&memory.load(STEERING));
            let answer = state.step(
                steering::Frame {
                    heading: heading as f32,
                    view: player.view() as f32,
                    direction: [x, y],
                    armed: player.armed(),
                    airborne: player.airborne(),
                    manual: camera.manual(),
                    seconds: input.delta(),
                },
                steering::Options {
                    follow: settings.flag("movement", "camera_follow"),
                    delay: settings.number("movement", "camera_follow_delay"),
                    rate: settings.number("movement", "camera_follow_speed"),
                },
            );
            memory.save(STEERING, &state.save());
            camera.turn(answer.camera_turn);
            if player.airborne() {
                // A jump keeps the direction it started with.
                movement.claim(heading, heading, 0.0, run, false);
                continue;
            }
            let moving = x != 0.0 || y != 0.0;
            let speed = if moving { player.speed(run) } else { 0.0 };
            movement.claim(answer.facing, answer.travel, speed, run, jump);
            cut_animation(&movement, &player, moving);
        }
    }
}

/// The animation keeps up with the movement: a movement animation gives way to the next one at
/// once instead of finishing its cycle, and to the idle one as soon as the player stops.
/// Jumps, landings, combat and scripted animations are left alone.
fn cut_animation(movement: &Movement, player: &Player, moving: bool) {
    let first = |values: Vec<u32>| values.first().copied();
    let Some(current) = first(player.animation()) else {
        return;
    };
    let turning = TURNING.contains(&current);
    if !turning && !MOVING.contains(&current) {
        return;
    }
    let idle = first(player.idle());
    match first(player.pending()) {
        Some(next)
            if next != current
                && (TURNING.contains(&next) || MOVING.contains(&next) || Some(next) == idle) =>
        {
            movement.play(next, true);
        }
        // The idle group is the sheathed stance; a drawn weapon keeps its own.
        None if !moving && !turning && !player.armed() => {
            if let Some(idle) = idle {
                movement.play(idle, true);
            }
        }
        _ => {}
    }
}

bindings::export!(Steering with_types_in bindings);
