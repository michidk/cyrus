//! Session steering state: automatic camera follow must not turn held movement into a circle.

const TURN_RATE: f32 = 5120.0;

/// User-selected mod behavior; these are presentation preferences, not original-engine rules.
pub struct Options {
    pub follow: bool,
    pub delay: f32,
    pub rate: f32,
}

pub struct Frame {
    pub heading: f32,
    pub view: f32,
    pub direction: [f32; 2],
    pub armed: bool,
    pub airborne: bool,
    pub manual: bool,
    pub seconds: f32,
}

#[derive(Default)]
pub struct SteeringState {
    direction: [f32; 2],
    travel: f32,
    facing: f32,
    quiet: f32,
    initialized: bool,
}

pub struct Answer {
    pub facing: u32,
    pub travel: u32,
    pub camera_turn: f32,
}

impl cyrus_mod_sdk::Persist for SteeringState {
    fn pack(&self) -> Vec<f32> {
        self.save()
    }

    fn unpack(values: &[f32]) -> Option<Self> {
        Some(Self::load(values))
    }
}

impl SteeringState {
    pub fn load(values: &[f32]) -> Self {
        match values {
            &[x, y, travel, facing, quiet] if values.iter().all(|v| v.is_finite()) => Self {
                direction: [x, y],
                travel,
                facing,
                quiet,
                initialized: true,
            },
            _ => Self::default(),
        }
    }

    pub fn save(&self) -> Vec<f32> {
        if !self.initialized {
            return Vec::new();
        }
        vec![
            self.direction[0],
            self.direction[1],
            self.travel,
            self.facing,
            self.quiet,
        ]
    }

    pub fn step(&mut self, frame: Frame, options: Options) -> Answer {
        let moving = frame.direction != [0.0; 2];
        let seconds = frame.seconds.clamp(0.0, 0.1);
        if !self.initialized || !moving || frame.airborne {
            self.facing = frame.heading;
        }
        if !moving || frame.airborne {
            self.direction = [0.0; 2];
            self.quiet = 0.0;
            self.initialized = false;
            return Answer {
                facing: wrap(frame.heading),
                travel: wrap(frame.heading),
                camera_turn: 0.0,
            };
        }
        let [x, y] = frame.direction;
        let offset = (-x).atan2(y) * 2048.0 / std::f32::consts::TAU;
        // Hold a world-space direction while the camera automatically catches up. Rebasing on
        // every camera publication would continually rotate an unchanged sideways key.
        if !self.initialized
            || self.direction != frame.direction
            || frame.manual
            || frame.armed
            || !options.follow
        {
            self.travel = (frame.view + offset).rem_euclid(2048.0);
        }
        if frame.manual || frame.armed || self.direction != frame.direction {
            self.quiet = 0.0;
        } else {
            self.quiet += seconds;
        }
        self.direction = frame.direction;
        self.initialized = true;
        let target = if frame.armed { frame.view } else { self.travel };
        // Integrate our last command, rather than a delayed engine snapshot that can alternate
        // between two previous headings. Idle and airborne frames resynchronize with the game.
        self.facing +=
            difference(target, self.facing).clamp(-TURN_RATE * seconds, TURN_RATE * seconds);
        self.facing = self.facing.rem_euclid(2048.0);
        let camera_turn =
            if options.follow && !frame.armed && !frame.manual && self.quiet >= options.delay {
                difference(self.travel, frame.view) * (1.0 - (-options.rate * seconds).exp())
            } else {
                0.0
            };
        Answer {
            facing: wrap(self.facing),
            travel: wrap(self.travel),
            camera_turn,
        }
    }
}

fn difference(target: f32, current: f32) -> f32 {
    (target - current + 1024.0).rem_euclid(2048.0) - 1024.0
}

fn wrap(value: f32) -> u32 {
    value.round().rem_euclid(2048.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame {
            heading: 0.0,
            view: 0.0,
            direction: [-1.0, 0.0],
            armed: false,
            airborne: false,
            manual: false,
            seconds: 1.0 / 60.0,
        }
    }
    fn options() -> Options {
        Options {
            follow: true,
            delay: 0.5,
            rate: 2.0,
        }
    }

    #[test]
    fn held_left_has_stable_facing_and_travel_as_camera_catches_up() {
        let mut state = SteeringState::default();
        let mut view = 0.0;
        let mut facing = 0;
        for _ in 0..300 {
            // Reproduce delayed snapshots by keeping observed heading stale.
            let answer = state.step(Frame { view, ..frame() }, options());
            assert!(answer.facing >= facing && answer.facing <= 512);
            assert_eq!(answer.travel, 512);
            facing = answer.facing;
            view += answer.camera_turn;
            state = SteeringState::load(&state.save());
        }
        assert_eq!(facing, 512);
        assert!((view - 512.0).abs() < 1.0);
    }

    #[test]
    fn manual_look_rebases_travel_and_delays_automatic_follow() {
        let mut state = SteeringState::default();
        for _ in 0..100 {
            state.step(frame(), options());
        }
        let answer = state.step(
            Frame {
                view: 256.0,
                manual: true,
                ..frame()
            },
            options(),
        );
        assert_eq!(answer.travel, 768);
        assert_eq!(answer.camera_turn, 0.0);
        for _ in 0..20 {
            assert_eq!(
                state
                    .step(
                        Frame {
                            view: 256.0,
                            ..frame()
                        },
                        options()
                    )
                    .camera_turn,
                0.0
            );
        }
    }

    #[test]
    fn idle_airborne_armed_and_disabled_follow_do_not_recenter() {
        for frame in [
            Frame {
                direction: [0.0; 2],
                ..frame()
            },
            Frame {
                airborne: true,
                ..frame()
            },
            Frame {
                armed: true,
                ..frame()
            },
        ] {
            let answer = SteeringState::default().step(
                frame,
                Options {
                    delay: 0.0,
                    ..options()
                },
            );
            assert_eq!(answer.camera_turn, 0.0);
        }
        let mut state = SteeringState::default();
        for view in [0.0, 256.0, 512.0] {
            let answer = state.step(
                Frame { view, ..frame() },
                Options {
                    follow: false,
                    ..options()
                },
            );
            assert_eq!(answer.travel, wrap(view + 512.0));
            assert_eq!(answer.camera_turn, 0.0);
        }
    }

    #[test]
    fn turns_take_the_short_path_across_zero() {
        let answer = SteeringState::default().step(
            Frame {
                heading: 2040.0,
                view: 10.0,
                direction: [0.0, 1.0],
                ..frame()
            },
            options(),
        );
        assert_eq!(answer.facing, 10);
    }

    #[test]
    fn stopping_resynchronizes_and_a_new_press_uses_the_new_camera_direction() {
        let mut state = SteeringState::default();
        for _ in 0..60 {
            state.step(frame(), options());
        }
        state.step(
            Frame {
                direction: [0.0; 2],
                heading: 512.0,
                ..frame()
            },
            options(),
        );
        state = SteeringState::load(&state.save());
        let answer = state.step(
            Frame {
                view: 512.0,
                heading: 512.0,
                ..frame()
            },
            options(),
        );
        assert_eq!(answer.travel, 1024);
    }
}
