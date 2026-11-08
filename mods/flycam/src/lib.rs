//! F11 toggles a free first-person camera. While it flies, the host suppresses player input.
//! A line in the corner shows whether it is on.
mod bindings {
    wasvy_macros::guest_bindings!({
        path: ["../../crates/cyrus-mods/wit", "wit"],
        world: "cyrus:flycam/guest",
        with: {
            "wasvy:ecs/app@0.0.11": generate,
            "cyrus:mods/components@0.1.0": generate,
        }
    });
}

use bindings::cyrus::mods::components::{Camera, Hud, Input, Memory, Settings};
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};

/// Radians per pixel of mouse motion.
const SENSITIVITY: f32 = 0.0025;
/// Where the flight is kept between frames: every system call runs in a fresh instance.
const FLIGHT: &str = "flycam/flight";

struct Fly {
    position: [f32; 3],
    yaw: f32,
    pitch: f32,
}

impl Fly {
    fn load(memory: &Memory) -> Option<Self> {
        let &[x, y, z, yaw, pitch] = &memory.load(FLIGHT)[..] else {
            return None;
        };
        Some(Fly {
            position: [x, y, z],
            yaw,
            pitch,
        })
    }

    fn save(fly: Option<&Self>, memory: &Memory) {
        let values = fly.map_or_else(Vec::new, |fly| {
            let [x, y, z] = fly.position;
            vec![x, y, z, fly.yaw, fly.pitch]
        });
        memory.save(FLIGHT, &values);
    }
}

struct Flycam;

impl bindings::Guest for Flycam {
    fn setup(app: App) {
        let fly = System::new("fly");
        fly.add_query(&[
            QueryFor::Ref(Input::type_path()),
            QueryFor::Mut(Camera::type_path()),
            QueryFor::Ref(Settings::type_path()),
            QueryFor::Mut(Hud::type_path()),
            QueryFor::Mut(Memory::type_path()),
        ]);
        app.add_systems(&Schedule::Update, &[&fly]);
    }

    fn fly(query: Query) {
        while let Some(row) = query.iter() {
            let input = Input::new(row.component(0));
            let camera = Camera::new(row.component(1));
            // Scene units per second; the manifest's `speed` setting.
            let speed = Settings::new(row.component(2)).number("flycam", "speed");
            let hud = Hud::new(row.component(3));
            let memory = Memory::new(row.component(4));
            let mut fly = Fly::load(&memory);
            if input.tapped("F11") {
                fly = match fly {
                    Some(_) => None,
                    None => start(&camera.pose()),
                };
            }
            if let Some(fly) = fly.as_mut() {
                hud.show("Fly camera: ON (F11)");
                steer(fly, &input, speed);
                // Rotation = yaw about Y, then pitch about X; quaternion from half angles.
                let (sy, cy) = (fly.yaw * 0.5).sin_cos();
                let (sp, cp) = (fly.pitch * 0.5).sin_cos();
                let [x, y, z] = fly.position;
                camera.drive(&[x, y, z, cy * sp, sy * cp, -sy * sp, cy * cp]);
            } else {
                hud.show("Fly camera: OFF (F11)");
            }
            Fly::save(fly.as_ref(), &memory);
        }
    }
}

/// One frame of mouse look and WASD, Space and Left Shift flight.
fn steer(fly: &mut Fly, input: &Input, speed: f32) {
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
        let step = speed * input.delta() / length;
        for i in 0..3 {
            fly.position[i] += direction[i] * step;
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
