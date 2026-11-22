//! Insert toggles a free first-person camera. While it flies, the host suppresses player input.
//! With "Leave the player where the camera stops" on, landing moves the player there, so scripts
//! and triggers near that spot run as if the player had walked there. With "Move the player with
//! the camera" on, the player follows the camera's map position every frame, so the view window,
//! triggers and scripts follow the flight.
use cyrus_mod_sdk::{Cx, Input, Key, Mod, Persist, Pose, Quat, Vec3};

/// Radians per pixel of mouse motion.
const SENSITIVITY: f32 = 0.0025;
/// Where the flight is kept between frames.
const FLIGHT: &str = "flight";

/// A flight in progress: where the camera is and which way it looks, in radians.
struct Fly {
    position: Vec3,
    yaw: f32,
    pitch: f32,
}

impl Persist for Fly {
    fn pack(&self) -> Vec<f32> {
        let Vec3 { x, y, z } = self.position;
        vec![x, y, z, self.yaw, self.pitch]
    }

    fn unpack(values: &[f32]) -> Option<Self> {
        let &[x, y, z, yaw, pitch] = values else {
            return None;
        };
        Some(Fly {
            position: Vec3::new(x, y, z),
            yaw,
            pitch,
        })
    }
}

impl Fly {
    /// Starts flying from `pose`.
    fn from_pose(pose: Pose) -> Self {
        let (yaw, pitch) = pose.rotation.yaw_pitch();
        Fly {
            position: pose.position,
            yaw,
            pitch,
        }
    }

    /// Optional player destination when this flight ends; ordinary ground support settles it.
    fn landing(&self, carry_player: bool) -> Option<Vec3> {
        carry_player.then(|| self.position.scene_to_map())
    }

    /// Where a following player goes this frame: under the camera, at the player's own height,
    /// which ordinary ground support then lifts onto higher ground. Keeping the height avoids a long
    /// fall (and its damage) when the flight ends high above the ground.
    fn follow(&self, player: Vec3) -> Vec3 {
        let camera = self.position.scene_to_map();
        Vec3::new(camera.x, player.y, camera.z)
    }

    fn pose(&self) -> Pose {
        Pose {
            position: self.position,
            rotation: Quat::from_yaw_pitch(self.yaw, self.pitch),
        }
    }

    /// One frame of mouse look and WASD, Space and Left Shift flight at `speed` scene units per
    /// second.
    fn steer(&mut self, input: &Input, speed: f32) {
        let [dx, dy] = input.mouse();
        self.yaw -= dx * SENSITIVITY;
        self.pitch = (self.pitch - dy * SENSITIVITY).clamp(-1.5, 1.5);
        let forward = self.pose().forward();
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let mut direction = Vec3::ZERO;
        for (key, axis, sign) in [
            (Key::W, forward, 1.0),
            (Key::S, forward, -1.0),
            (Key::D, right, 1.0),
            (Key::A, right, -1.0),
            (Key::SPACE, Vec3::UP, 1.0),
            (Key::SHIFT_LEFT, Vec3::UP, -1.0),
        ] {
            if input.down(key) {
                direction += axis * sign;
            }
        }
        if let Some(direction) = direction.normalized() {
            self.position += direction * (speed * input.delta());
        }
    }
}

struct Flycam;

impl Mod for Flycam {
    const ID: &'static str = "flycam";

    fn frame(cx: &Cx) {
        // A new world starts with the camera back on the player.
        let mut fly: Option<Fly> = if cx.world_changed {
            None
        } else {
            cx.memory.load(FLIGHT)
        };
        if cx.input.tapped(Key::INSERT) {
            fly = match fly {
                Some(fly) => {
                    // A following player is already under the camera.
                    let carry = cx.settings.flag("carry_player") && !cx.settings.flag("follow");
                    if let Some(position) = fly.landing(carry) {
                        cx.player.place(position, cx.player.heading());
                    }
                    None
                }
                None => cx.camera.pose().map(Fly::from_pose),
            };
        }
        if let Some(fly) = fly.as_mut() {
            // Scene units per second; the manifest's `speed` setting.
            fly.steer(&cx.input, cx.settings.number("speed"));
            cx.camera.drive(fly.pose());
            if cx.settings.flag("follow")
                && let Some(player) = cx.player.position()
            {
                cx.player.place(fly.follow(player), cx.player.heading());
            }
        }
        cx.memory.save(FLIGHT, fly.as_ref());
    }
}

cyrus_mod_sdk::export_mod!(Flycam);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_following_player_keeps_its_height_under_the_camera() {
        let fly = Fly {
            position: Vec3::new(-2000.0, 50.0, -1200.0),
            yaw: 0.0,
            pitch: 0.0,
        };
        let target = fly.follow(Vec3::new(1.0, -744.0, 3.0));
        assert_eq!((target.x, target.y, target.z), (40000.0, -744.0, 41535.996));
    }

    #[test]
    fn leaving_flight_only_carries_the_player_when_enabled() {
        let fly = Fly {
            position: Vec3::new(-2000.0, 50.0, -1200.0),
            yaw: 1.0,
            pitch: -0.5,
        };
        assert!(fly.landing(false).is_none());
        let landing = fly.landing(true).unwrap();
        assert_eq!(landing.x, 40000.0);
        assert_eq!(landing.y, -1000.0);
        assert_eq!(landing.z, 41535.996);
        assert!(
            fly.landing(false).is_none(),
            "turning the option off takes effect immediately"
        );
    }
}
