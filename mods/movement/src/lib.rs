//! Third-person movement in the style of current action games. The mouse orbits the camera
//! around the player on its own; the keys move the player relative to the camera, at full speed
//! from the first frame, with the animations only showing the movement. With the weapon sheathed
//! the player turns quickly to face where it moves; with it drawn the player keeps facing where
//! the camera looks and backs off or circles instead. Jumping and falling stay the original's.
//! The game's own key bindings, or WASD with Space and Shift; the walk key runs (or walks, with
//! the `run_by_default` setting).
mod steering;

use cyrus_mod_sdk::{Action, Claim, Cx, Key, Mod};
use steering::SteeringState;

/// Where the steering state is kept between frames.
const STEERING: &str = "steering";

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

impl Mod for Steering {
    const ID: &'static str = "movement";

    fn frame(cx: &Cx) {
        cx.camera.orbit();
        if cx.world_changed {
            cx.memory.save::<SteeringState>(STEERING, None);
        }
        // No player yet (menus, loading): leave everything to the game.
        if cx.player.position().is_none() {
            cx.memory.save::<SteeringState>(STEERING, None);
            return;
        }
        let run_by_default = cx.settings.flag("run_by_default");
        let wasd = cx.settings.choice("keys") == "wasd";
        // Either the game's own bindings (KEYS.INI), or W, A, S and D with Space and Shift.
        let held = |action, key| {
            if wasd {
                cx.input.down(key)
            } else {
                cx.input.action(action)
            }
        };
        let axis = |plus: bool, minus: bool| f32::from(plus) - f32::from(minus);
        let x = axis(held(Action::Right, Key::D), held(Action::Left, Key::A));
        let y = axis(held(Action::Forward, Key::W), held(Action::Back, Key::S));
        let modifier = if wasd {
            cx.input.down(Key::SHIFT_LEFT) || cx.input.down(Key::SHIFT_RIGHT)
        } else {
            cx.input.action(Action::Walk)
        };
        let run = modifier != run_by_default;
        let jump = held(Action::Jump, Key::SPACE);
        if let Some(current) = cx.player.animation()
            && JUMPING.iter().any(|groups| groups.contains(&current))
        {
            cx.movement.tempo(JUMP_TEMPO);
        }
        let heading = cx.player.heading();
        let mut state: SteeringState = cx.memory.load(STEERING).unwrap_or_default();
        let answer = state.step(
            steering::Frame {
                heading: heading as f32,
                view: cx.player.view() as f32,
                direction: [x, y],
                armed: cx.player.armed(),
                airborne: cx.player.airborne(),
                manual: cx.camera.manual(),
                seconds: cx.input.delta(),
            },
            steering::Options {
                follow: cx.settings.flag("camera_follow"),
                delay: cx.settings.number("camera_follow_delay"),
                rate: cx.settings.number("camera_follow_speed"),
            },
        );
        cx.memory.save(STEERING, Some(&state));
        cx.camera.turn(answer.camera_turn);
        if cx.player.airborne() {
            // A jump keeps the direction it started with.
            cx.movement.claim(Claim {
                heading,
                travel: heading,
                speed: 0.0,
                run,
                jump: false,
            });
            return;
        }
        let moving = x != 0.0 || y != 0.0;
        let speed = if moving { cx.player.speed(run) } else { 0.0 };
        cx.movement.claim(Claim {
            heading: answer.facing,
            travel: answer.travel,
            speed,
            run,
            jump,
        });
        cut_animation(cx, moving);
    }
}

/// The animation keeps up with the movement: a movement animation gives way to the next one at
/// once instead of finishing its cycle, and to the idle one as soon as the player stops.
/// Jumps, landings, combat and scripted animations are left alone.
fn cut_animation(cx: &Cx, moving: bool) {
    let Some(current) = cx.player.animation() else {
        return;
    };
    let turning = TURNING.contains(&current);
    if !turning && !MOVING.contains(&current) {
        return;
    }
    let idle = cx.player.idle();
    match cx.player.pending() {
        Some(next)
            if next != current
                && (TURNING.contains(&next) || MOVING.contains(&next) || Some(next) == idle) =>
        {
            cx.movement.play(next, true);
        }
        // The idle group is the sheathed stance; a drawn weapon keeps its own.
        None if !moving && !turning && !cx.player.armed() => {
            if let Some(idle) = idle {
                cx.movement.play(idle, true);
            }
        }
        _ => {}
    }
}

cyrus_mod_sdk::export_mod!(Steering);
