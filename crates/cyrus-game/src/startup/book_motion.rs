//! Authored book turns and camera requests from preservation's engine/menu.md.
use std::collections::VecDeque;

#[derive(Clone, Default)]
pub(super) struct BookPose {
    pub frames: [usize; 5],
    pub depth: [f32; 5],
    pub camera: [f32; 3],
    pub pitch: i32,
}

pub(super) struct BookMotion {
    pub pose: BookPose,
    last: [usize; 5],
    turns: [Option<Turn>; 5],
    camera: VecDeque<CameraMove>,
}

struct Turn {
    initial: usize,
    target: usize,
    delay: u8,
}

struct CameraMove {
    start: [f32; 3],
    target: [f32; 3],
    pitch: i32,
    change: i32,
    step: f32,
    fraction: f32,
}

impl BookMotion {
    pub fn opening(last: [usize; 5]) -> Self {
        let mut motion = Self {
            pose: BookPose {
                camera: [-4.0, -100.0, 240.0],
                ..BookPose::default()
            },
            last,
            turns: std::array::from_fn(|_| None),
            camera: VecDeque::new(),
        };
        motion.request([0.0; 3], 0, 0.15);
        motion
    }

    pub fn busy(&self) -> bool {
        !self.camera.is_empty() || self.turns.iter().any(Option::is_some)
    }

    fn request(&mut self, target: [f32; 3], change: i32, step: f32) {
        let (start, pitch) = self
            .camera
            .back()
            .map_or((self.pose.camera, self.pose.pitch), |previous| {
                (previous.target, (previous.pitch + previous.change) & 2047)
            });
        self.camera.push_back(CameraMove {
            start,
            target,
            pitch,
            change,
            step,
            fraction: step,
        });
    }

    fn sweep(&mut self, target: [f32; 3]) {
        self.request([192.0, 284.0, -160.0], -152, 0.15);
        self.request([192.0, 284.0, -160.0], 0, 0.12);
        self.request(target, 152, 0.15);
    }

    fn turn(&mut self, model: usize, target: usize, delay: u8) {
        self.turns[model] = Some(Turn {
            initial: self.pose.frames[model],
            target,
            delay,
        });
    }

    /// Called only between settled supported pages.
    pub fn navigate(&mut self, from: u8, to: u8) -> Result<(), String> {
        debug_assert!(!self.busy());
        match (from, to) {
            (0, 3) => {
                self.sweep([336.0, 0.0, 0.0]);
                self.turn(1, self.last[1], 4);
            }
            (3, 0) => {
                self.sweep([0.0; 3]);
                self.turn(1, 0, 4);
            }
            (0, 4) => {
                self.sweep([0.0; 3]);
                self.turn(1, self.last[1], 3);
                self.turn(3, self.last[3], 5);
            }
            (4, 0) => {
                self.sweep([0.0; 3]);
                self.turn(1, 0, 5);
                self.turn(3, 0, 3);
            }
            (4, 5) => self.request([336.0, -128.0, 0.0], 0, 0.15),
            (4, 6) => self.request([336.0, 176.0, 0.0], 0, 0.15),
            (5 | 6, 4) => self.request([0.0; 3], 0, 0.15),
            (4, 7) => {
                self.sweep([0.0; 3]);
                self.turn(4, self.last[4], 4);
            }
            (7, 4) => {
                self.sweep([0.0; 3]);
                self.turn(4, 0, 4);
            }
            _ => {
                return Err(format!(
                    "MENU.INI: book transition from page {from} to page {to} is unavailable"
                ));
            }
        }
        Ok(())
    }

    pub fn close(&mut self) {
        debug_assert!(!self.busy());
        self.request([232.0, 284.0, -96.0], -152, 0.15);
        self.request([232.0, 284.0, -96.0], 0, 0.12);
        self.request([408.0, 0.0, 0.0], 152, 0.15);
        self.turn(0, self.last[0], 2);
    }

    /// Advances one presentation update; true includes the final settling update.
    pub fn tick(&mut self) -> bool {
        let busy = self.busy();
        if let Some(camera) = self.camera.front_mut() {
            self.pose.camera = std::array::from_fn(|i| {
                (f64::from(camera.start[i])
                    + f64::from(camera.target[i] - camera.start[i]) * f64::from(camera.fraction))
                    as i32 as f32
            });
            self.pose.pitch = (f64::from(camera.pitch)
                + f64::from(camera.change) * f64::from(camera.fraction))
                as i32
                & 2047;
            camera.fraction += camera.step;
            if camera.fraction > 1.0 {
                self.pose.camera = camera.target;
                self.pose.pitch = (camera.pitch + camera.change) & 2047;
                self.camera.pop_front();
            }
        }
        for (model, turn) in self.turns.iter_mut().enumerate() {
            let Some(request) = turn.as_mut() else {
                continue;
            };
            if request.delay != 0 {
                request.delay -= 1;
                if request.delay == 0 {
                    self.pose.frames[model] = request.initial;
                }
            } else {
                let forward = self.pose.frames[model] < request.target;
                if forward {
                    self.pose.frames[model] += 1;
                } else {
                    self.pose.frames[model] -= 1;
                }
                if self.pose.frames[model] == 4 && matches!(model, 1 | 4) {
                    let shift = if model == 1 { 4.0 } else { -4.0 };
                    self.pose.depth[model] += if forward { shift } else { -shift };
                }
            }
            if self.pose.frames[model] == request.target {
                *turn = None;
            }
        }
        busy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled() -> BookMotion {
        let mut motion = BookMotion::opening([8, 8, 0, 8, 8]);
        while motion.tick() {}
        motion
    }

    #[test]
    fn unsupported_navigation_preserves_the_settled_book() {
        let mut motion = settled();
        assert_eq!(
            motion.navigate(0, 7),
            Err("MENU.INI: book transition from page 0 to page 7 is unavailable".into())
        );
        assert!(!motion.busy());
        assert_eq!(motion.pose.frames, [0; 5]);
        assert_eq!(motion.pose.camera, [0.0; 3]);
    }

    #[test]
    fn opening_samples_truncate_and_settle_on_the_sixth_update() {
        let mut motion = BookMotion::opening([8, 8, 0, 8, 8]);
        assert!(motion.tick());
        assert_eq!(motion.pose.camera, [-3.0, -84.0, 203.0]);
        for _ in 0..5 {
            assert!(motion.tick());
        }
        assert_eq!(motion.pose.camera, [0.0; 3]);
        assert!(!motion.tick());
    }

    #[test]
    fn movies_turn_uses_authored_frames_delays_and_reversible_depth() {
        let mut motion = settled();
        motion.navigate(0, 3).unwrap();
        assert!(motion.tick());
        assert_eq!(motion.pose.camera, [28.0, 42.0, -24.0]);
        assert_eq!(motion.pose.pitch, 2026);
        for _ in 0..3 {
            motion.tick();
        }
        assert_eq!(motion.pose.frames[1], 0);
        for frame in 1..=8 {
            motion.tick();
            assert_eq!(motion.pose.frames[1], frame);
            assert_eq!(motion.pose.depth[1], if frame < 4 { 0.0 } else { 4.0 });
        }
        while motion.tick() {}
        assert_eq!(motion.pose.camera, [336.0, 0.0, 0.0]);
        assert_eq!(motion.pose.pitch, 0);
        motion.navigate(3, 0).unwrap();
        while motion.tick() {}
        assert_eq!(motion.pose.frames, [0; 5]);
        assert_eq!(motion.pose.depth, [0.0; 5]);
        assert_eq!(motion.pose.camera, [0.0; 3]);
    }

    #[test]
    fn controls_and_options_restore_the_main_book_and_quit_closes_the_cover() {
        let mut motion = settled();
        for (from, to) in [(0, 4), (4, 5), (5, 4), (4, 6), (6, 4), (4, 7)] {
            motion.navigate(from, to).unwrap();
            while motion.tick() {}
        }
        assert_eq!(motion.pose.frames, [0, 8, 0, 8, 8]);
        assert_eq!(motion.pose.depth, [0.0, 4.0, 0.0, 0.0, -4.0]);
        for (from, to) in [(7, 4), (4, 0)] {
            motion.navigate(from, to).unwrap();
            while motion.tick() {}
        }
        assert_eq!(motion.pose.frames, [0; 5]);
        assert_eq!(motion.pose.depth, [0.0; 5]);
        motion.close();
        while motion.tick() {}
        assert_eq!(motion.pose.frames, [8, 0, 0, 0, 0]);
        assert_eq!(motion.pose.camera, [408.0, 0.0, 0.0]);
        assert_eq!(motion.pose.pitch, 0);
    }
}
