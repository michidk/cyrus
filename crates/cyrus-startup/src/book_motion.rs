//! Authored book turns and camera requests from preservation's engine/menu.md.
//! Forward turns target each model's frame count, one past its last authored frame
//! (redguard-preservation PR #81).
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
    frame_counts: [usize; 5],
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
    /// Death context three uses the default short main-page entry, without a cover turn.
    pub fn death_entry(frame_counts: [usize; 5]) -> Self {
        let mut motion = Self {
            pose: BookPose {
                camera: [-4.0, -100.0, 240.0],
                ..BookPose::default()
            },
            frame_counts,
            turns: std::array::from_fn(|_| None),
            camera: VecDeque::new(),
        };
        motion.request([0.0; 3], 0, 0.15);
        motion
    }

    /// A gameplay shortcut enters its page from the settled main-page camera.
    pub fn settled(frame_counts: [usize; 5]) -> Self {
        Self {
            pose: BookPose::default(),
            frame_counts,
            turns: std::array::from_fn(|_| None),
            camera: VecDeque::new(),
        }
    }

    pub fn opening(frame_counts: [usize; 5]) -> Self {
        let mut motion = Self {
            pose: BookPose {
                frames: [frame_counts[0].saturating_sub(1), 0, 0, 0, 0],
                camera: [408.0, 0.0, 0.0],
                ..BookPose::default()
            },
            frame_counts,
            turns: std::array::from_fn(|_| None),
            camera: VecDeque::new(),
        };
        // Preservation menu entry context 1 opens the cover through a three-part sweep.
        motion.request([172.0, 204.0, -152.0], -116, 0.15);
        motion.request([172.0, 204.0, -152.0], 0, 0.15);
        motion.request([0.0; 3], 116, 0.15);
        motion.turn(0, 0, 1);
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
        self.request([192.0, 284.0, -96.0], -152, 0.15);
        self.request([192.0, 284.0, -96.0], 0, 0.12);
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
            (0, 1) => self.request([336.0, 0.0, 0.0], 0, 0.15),
            (1, 0) => self.request([0.0; 3], 0, 0.15),
            (0, 2) => {
                self.sweep([0.0; 3]);
                self.turn(1, self.frame_counts[1], 4);
            }
            (2, 0) => {
                self.sweep([0.0; 3]);
                self.turn(1, 0, 4);
            }
            (0, 3) => {
                self.sweep([336.0, 0.0, 0.0]);
                self.turn(1, self.frame_counts[1], 4);
            }
            (3, 0) => {
                self.sweep([0.0; 3]);
                self.turn(1, 0, 4);
            }
            (0, 4) => {
                self.sweep([0.0; 3]);
                self.turn(1, self.frame_counts[1], 3);
                self.turn(3, self.frame_counts[3], 5);
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
                self.turn(4, self.frame_counts[4], 4);
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

    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn close(&mut self) {
        debug_assert!(!self.busy());
        self.request([232.0, 284.0, -96.0], -152, 0.15);
        self.request([232.0, 284.0, -96.0], 0, 0.12);
        self.request([408.0, 0.0, 0.0], 152, 0.15);
        self.turn(0, self.frame_counts[0], 2);
    }

    /// Advances one presentation update and dispatches installed effect indices.
    /// True includes the final settling update. Cue rules are preservation PR #34.
    pub fn tick(&mut self, mut sound: impl FnMut(usize)) -> bool {
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
                    match model {
                        1 => sound(66),
                        3 => sound(67),
                        4 => sound(68),
                        _ => {}
                    }
                }
            } else {
                let forward = self.pose.frames[model] < request.target;
                if forward {
                    self.pose.frames[model] += 1;
                } else {
                    self.pose.frames[model] -= 1;
                }
                if model == 0 {
                    if forward && self.pose.frames[model] == 7 {
                        sound(65);
                    } else if !forward && self.pose.frames[model] == 0 {
                        sound(64);
                    }
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
        let mut motion = BookMotion::opening([9, 9, 0, 9, 9]);
        while motion.tick(|_| {}) {}
        motion
    }

    fn cues(motion: &mut BookMotion) -> Vec<(usize, usize)> {
        let mut cues = Vec::new();
        for update in 1..=64 {
            if !motion.tick(|effect| cues.push((update, effect))) {
                return cues;
            }
        }
        panic!("book motion did not settle");
    }

    #[test]
    fn book_cues_follow_delayed_starts_and_cover_frame_crossings_once() {
        let mut motion = BookMotion::opening([9, 9, 0, 9, 9]);
        assert_eq!(cues(&mut motion), [(9, 64)]);
        for (from, to, expected) in [
            (0, 3, vec![(4, 66)]),
            (3, 0, vec![(4, 66)]),
            (0, 4, vec![(3, 66), (5, 67)]),
            (4, 5, vec![]),
            (5, 4, vec![]),
            (4, 6, vec![]),
            (6, 4, vec![]),
            (4, 7, vec![(4, 68)]),
            (7, 4, vec![(4, 68)]),
            (4, 0, vec![(3, 67), (5, 66)]),
        ] {
            motion.navigate(from, to).unwrap();
            assert_eq!(cues(&mut motion), expected, "page {from} to {to}");
            assert!(cues(&mut motion).is_empty(), "settled page {to}");
        }
        motion.close();
        assert_eq!(cues(&mut motion), [(9, 65)]);
        assert_eq!(motion.pose.frames[0], 9);
        motion.turn(0, 0, 2);
        assert_eq!(cues(&mut motion), [(11, 64)]);
    }

    #[test]
    fn save_camera_moves_without_turning_and_load_turns_the_first_page() {
        let mut motion = settled();
        motion.navigate(0, 1).unwrap();
        assert!(motion.tick(|_| panic!("Save must not turn a page")));
        assert_eq!(motion.pose.camera, [50.0, 0.0, 0.0]);
        assert!(cues(&mut motion).is_empty());
        assert_eq!(motion.pose.camera, [336.0, 0.0, 0.0]);
        assert_eq!(motion.pose.frames, [0; 5]);
        motion.navigate(1, 0).unwrap();
        assert!(cues(&mut motion).is_empty());
        assert_eq!(motion.pose.camera, [0.0; 3]);
        assert_eq!(motion.pose.frames, [0; 5]);

        motion.navigate(0, 2).unwrap();
        assert!(motion.tick(|_| panic!("Load page sound must wait four updates")));
        assert_eq!(motion.pose.camera, [28.0, 42.0, -14.0]);
        assert_eq!(motion.pose.pitch, 2026);
        assert_eq!(cues(&mut motion), [(3, 66)]);
        assert_eq!(motion.pose.camera, [0.0; 3]);
        assert_eq!(motion.pose.pitch, 0);
        assert_eq!(motion.pose.frames, [0, 9, 0, 0, 0]);
        motion.navigate(2, 0).unwrap();
        assert_eq!(cues(&mut motion), [(4, 66)]);
        assert_eq!(motion.pose.camera, [0.0; 3]);
        assert_eq!(motion.pose.pitch, 0);
        assert_eq!(motion.pose.frames, [0; 5]);
    }

    #[test]
    fn options_return_turns_one_update_longer_than_its_authored_frames() {
        let mut motion = settled();
        motion.navigate(0, 4).unwrap();
        while motion.tick(|_| {}) {}
        motion.navigate(4, 0).unwrap();
        let mut last_page_update = 0;
        for update in 1..=64 {
            let before = motion.pose.frames;
            if !motion.tick(|_| {}) {
                break;
            }
            if motion.pose.frames != before {
                last_page_update = update;
            }
        }
        // Delay five, then nine steps from the frame-count index.
        assert_eq!(last_page_update, 14);
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
    fn death_entry_keeps_the_open_cover_and_settles_without_a_sound() {
        let mut motion = BookMotion::death_entry([9, 9, 0, 9, 9]);
        assert_eq!(motion.pose.frames, [0; 5]);
        assert_eq!(motion.pose.camera, [-4.0, -100.0, 240.0]);
        assert!(motion.tick(|_| panic!("death entry must not turn the cover")));
        assert_eq!(motion.pose.camera, [-3.0, -84.0, 203.0]);
        for _ in 0..5 {
            assert!(motion.tick(|_| panic!("death entry cue")));
        }
        assert_eq!(motion.pose.camera, [0.0; 3]);
        assert!(!motion.tick(|_| panic!("settled death entry cue")));
    }

    #[test]
    fn startup_entry_sweeps_holds_and_opens_the_cover_before_accepting_input() {
        let mut motion = BookMotion::opening([9, 9, 0, 9, 9]);
        assert_eq!(motion.pose.camera, [408.0, 0.0, 0.0]);
        assert_eq!(motion.pose.frames, [8, 0, 0, 0, 0]);
        let mut sounds = Vec::new();
        for update in 1..=18 {
            assert!(motion.tick(|effect| sounds.push((update, effect))));
            match update {
                1 => {
                    assert_eq!(motion.pose.camera, [372.0, 30.0, -22.0]);
                    assert_eq!(motion.pose.pitch, 2031);
                    assert_eq!(motion.pose.frames[0], 8);
                }
                6 | 12 => {
                    assert_eq!(motion.pose.camera, [172.0, 204.0, -152.0]);
                    assert_eq!(motion.pose.pitch, 1932);
                }
                18 => {
                    assert_eq!(motion.pose.camera, [0.0; 3]);
                    assert_eq!(motion.pose.pitch, 0);
                }
                _ => {}
            }
        }
        assert_eq!(sounds, [(9, 64)]);
        assert_eq!(motion.pose.frames, [0; 5]);
        assert!(!motion.tick(|_| panic!("settled book dispatched a sound")));
    }

    #[test]
    fn movies_turn_uses_authored_frames_delays_and_reversible_depth() {
        let mut motion = settled();
        motion.navigate(0, 3).unwrap();
        assert!(motion.tick(|_| {}));
        assert_eq!(motion.pose.camera, [28.0, 42.0, -14.0]);
        assert_eq!(motion.pose.pitch, 2026);
        for _ in 0..3 {
            motion.tick(|_| {});
        }
        assert_eq!(motion.pose.frames[1], 0);
        for frame in 1..=9 {
            motion.tick(|_| {});
            assert_eq!(motion.pose.frames[1], frame);
            assert_eq!(motion.pose.depth[1], if frame < 4 { 0.0 } else { 4.0 });
        }
        while motion.tick(|_| {}) {}
        assert_eq!(motion.pose.camera, [336.0, 0.0, 0.0]);
        assert_eq!(motion.pose.pitch, 0);
        motion.navigate(3, 0).unwrap();
        while motion.tick(|_| {}) {}
        assert_eq!(motion.pose.frames, [0; 5]);
        assert_eq!(motion.pose.depth, [0.0; 5]);
        assert_eq!(motion.pose.camera, [0.0; 3]);
    }

    #[test]
    fn controls_and_options_restore_the_main_book_and_quit_closes_the_cover() {
        let mut motion = settled();
        for (from, to) in [(0, 4), (4, 5), (5, 4), (4, 6), (6, 4), (4, 7)] {
            motion.navigate(from, to).unwrap();
            while motion.tick(|_| {}) {}
        }
        assert_eq!(motion.pose.frames, [0, 9, 0, 9, 9]);
        assert_eq!(motion.pose.depth, [0.0, 4.0, 0.0, 0.0, -4.0]);
        for (from, to) in [(7, 4), (4, 0)] {
            motion.navigate(from, to).unwrap();
            while motion.tick(|_| {}) {}
        }
        assert_eq!(motion.pose.frames, [0; 5]);
        assert_eq!(motion.pose.depth, [0.0; 5]);
        motion.close();
        while motion.tick(|_| {}) {}
        assert_eq!(motion.pose.frames, [9, 0, 0, 0, 0]);
        assert_eq!(motion.pose.camera, [408.0, 0.0, 0.0]);
        assert_eq!(motion.pose.pitch, 0);
    }
}
