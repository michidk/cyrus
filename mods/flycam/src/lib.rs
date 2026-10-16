//! F3 toggles a free first-person camera. While it flies, the host suppresses player input.
mod bindings {
    wasvy_macros::guest_bindings!({
        path: ["../../crates/cyrus-mods/wit", "wit"],
        world: "cyrus:flycam/guest",
        with: {
            "wasvy:ecs/app@0.0.9": generate,
            "cyrus:mods/components@0.1.0": generate,
        }
    });
}

use bindings::cyrus::mods::components::{Camera, Input};
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};
use std::cell::RefCell;

/// Scene units per second, the debug menu flycam's default.
const SPEED: f32 = 150.0;
/// Radians per pixel of mouse motion.
const SENSITIVITY: f32 = 0.0025;

struct Fly {
    position: [f32; 3],
    yaw: f32,
    pitch: f32,
}

thread_local! {
    static FLY: RefCell<Option<Fly>> = const { RefCell::new(None) };
}

struct Flycam;

impl bindings::Guest for Flycam {
    fn setup(app: App) {
        let fly = System::new("fly");
        fly.add_query(&[
            QueryFor::Ref(Input::type_path()),
            QueryFor::Mut(Camera::type_path()),
        ]);
        app.add_systems(&Schedule::Update, &[&fly]);
    }

    fn fly(query: Query) {
        while let Some(row) = query.iter() {
            let input = Input::new(row.component(0));
            let camera = Camera::new(row.component(1));
            FLY.with_borrow_mut(|fly| {
                if input.tapped("F3") {
                    *fly = match fly.take() {
                        Some(_) => None,
                        None => start(&camera.pose()),
                    };
                }
                let Some(fly) = fly else {
                    return;
                };
                if let [dx, dy] = input.mouse()[..] {
                    fly.yaw -= dx * SENSITIVITY;
                    fly.pitch = (fly.pitch - dy * SENSITIVITY).clamp(-1.5, 1.5);
                }
                let (sy, cy) = fly.yaw.sin_cos();
                let (sp, cp) = fly.pitch.sin_cos();
                let forward = [-sy * cp, sp, -cy * cp];
                let right = [cy, 0.0, -sy];
                let mut direction = [0.0f32; 3];
                for (key, axis, sign) in [
                    ("KeyW", forward, 1.0),
                    ("KeyS", forward, -1.0),
                    ("KeyD", right, 1.0),
                    ("KeyA", right, -1.0),
                    ("Space", [0.0, 1.0, 0.0], 1.0),
                    ("ShiftLeft", [0.0, 1.0, 0.0], -1.0),
                ] {
                    if input.pressed(key) {
                        for i in 0..3 {
                            direction[i] += axis[i] * sign;
                        }
                    }
                }
                let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
                if length > 0.0 {
                    let step = SPEED * input.delta() / length;
                    for i in 0..3 {
                        fly.position[i] += direction[i] * step;
                    }
                }
                // Rotation = yaw about Y, then pitch about X; quaternion from half angles.
                let (sy, cy) = (fly.yaw * 0.5).sin_cos();
                let (sp, cp) = (fly.pitch * 0.5).sin_cos();
                let [x, y, z] = fly.position;
                camera.drive(&[x, y, z, cy * sp, sy * cp, -sy * sp, cy * cp]);
            });
        }
    }
}

/// Starts flying from the current view pose: x, y, z, then quaternion x, y, z, w.
fn start(pose: &[f32]) -> Option<Fly> {
    let &[x, y, z, qx, qy, qz, qw] = pose else {
        return None;
    };
    // Rotate the view direction (0, 0, -1): v + 2w(u×v) + 2u×(u×v).
    let u = [qx, qy, qz];
    let v = [0.0, 0.0, -1.0];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    let f: [f32; 3] = std::array::from_fn(|i| v[i] + 2.0 * (qw * uv[i] + uuv[i]));
    Some(Fly {
        position: [x, y, z],
        yaw: (-f[0]).atan2(-f[2]),
        pitch: f[1].clamp(-1.0, 1.0).asin(),
    })
}

bindings::export!(Flycam with_types_in bindings);
