//! Item viewer rules: the pose, fade and exit of the modal that `ShowItem` opens, and the
//! description line sequence. Independent of presentation and audio; the host feeds one update
//! per BIOS-timer tick. Rules follow preservation's `docs/engine/SOUP.md` "Item viewer".
use anyhow::{Result, ensure};

/// Held actions at one update, after the host resolves the installed bindings.
#[derive(Clone, Copy, Default)]
pub struct ViewerInput {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    /// `key_a`: pans.
    pub walk: bool,
    /// `key_b`: depth and roll.
    pub jump: bool,
    /// `key_c`, `key_d` or a mouse button: any of them requests the exit.
    pub exit: bool,
}

/// Where the model is drawn: rotation about the vertical axis, then the horizontal axis, then
/// the view axis (2048 units per turn), and the position in the original's map units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewerPose {
    pub turn: u16,
    pub pitch: u16,
    pub roll: u16,
    pub position: [f32; 3],
}

/// Distance the entry fade starts the model from its resting place along the view axis.
const ENTRY_DEPTH: f32 = 1490.0;
/// Fade factor step per update.
const FADE_STEP: f64 = 0.15;
const ANGLE_STEP: i32 = 80;
const PAN_STEP: f32 = 40.0;
const DEPTH_STEP: f32 = 80.0;

/// One open viewer. Create it with the item model's three bounding extents.
#[derive(Debug)]
pub struct ItemViewer {
    /// Largest bounding extent.
    largest: i32,
    depth_min: f32,
    depth_max: f32,
    aspect: f32,
    angles: [u16; 3],
    pan: [f32; 2],
    depth: f32,
    fade: f32,
    exiting: bool,
    released: bool,
    exit_angles: [i32; 3],
    exit_offset: [f32; 3],
}

impl ItemViewer {
    /// `extents` are the model's x, y and z bounding extents; `aspect` is screen height over
    /// width. Only the form every installed script uses is supported: both request words zero.
    pub fn open(extents: [i32; 3], words: [i32; 2], aspect: f32) -> Result<Self> {
        ensure!(
            words == [0, 0],
            "item viewer: request words {words:?} are not the scripted form (0, 0)"
        );
        let largest = extents.into_iter().max().unwrap_or(0);
        let depth_min = extents[2] as f32;
        let start = depth_min + 180.0;
        Ok(Self {
            largest,
            depth_min,
            depth_max: (8 * largest) as f32 + start,
            aspect,
            angles: [0; 3],
            pan: [0.0; 2],
            depth: start,
            fade: 1.0,
            exiting: false,
            released: false,
            exit_angles: [0; 3],
            exit_offset: [0.0; 3],
        })
    }

    fn scale(&self) -> f32 {
        let range = self.depth_max - self.depth_min;
        ((self.depth - self.depth_min) as f64 / range as f64 + 0.1) as f32
    }

    /// Pan limits for the current scale: horizontal and vertical half-ranges.
    fn pan_limits(&self) -> [f32; 2] {
        let half = (2 * self.largest) as f32 * self.scale();
        [
            half.round_ties_even(),
            (half * self.aspect).round_ties_even(),
        ]
    }

    /// One update. Returns true once the viewer has finished and must close.
    pub fn update(&mut self, input: ViewerInput) -> bool {
        let moving =
            input.left || input.right || input.up || input.down || input.walk || input.jump;
        if moving {
            let scale = self.scale();
            let step = |positive: bool, negative: bool| i32::from(positive) - i32::from(negative);
            let turn = |angle: u16, delta: i32| (i32::from(angle) + delta * ANGLE_STEP) as u16;
            if input.walk {
                self.pan[0] += step(input.right, input.left) as f32 * PAN_STEP * scale;
                self.pan[1] += step(input.down, input.up) as f32 * PAN_STEP * scale;
            } else if input.jump {
                self.depth += step(input.up, input.down) as f32 * DEPTH_STEP * scale;
                self.angles[2] = turn(self.angles[2], step(input.right, input.left));
            } else {
                self.angles[0] = turn(self.angles[0], step(input.left, input.right));
                self.angles[1] = turn(self.angles[1], step(input.down, input.up));
            }
            self.depth = self.depth.clamp(self.depth_min, self.depth_max);
            let limits = self.pan_limits();
            self.pan[0] = self.pan[0].clamp(-limits[0], limits[0]);
            self.pan[1] = self.pan[1].clamp(-limits[1], limits[1]);
        }
        self.angles = self.angles.map(|angle| angle & 0x7ff);
        if self.fade != 0.0 {
            if self.exiting {
                self.fade = (f64::from(self.fade) + FADE_STEP) as f32;
                if self.fade > 1.0 {
                    return true;
                }
            } else {
                self.fade = ((f64::from(self.fade) - FADE_STEP) as f32).max(0.0);
            }
            self.interpolate_angles();
        }
        if !input.exit || self.exiting {
            self.released = true;
        } else if self.released {
            self.begin_exit();
        }
        false
    }

    fn interpolate_angles(&mut self) {
        let remaining = f64::from(1.0 - self.fade);
        for (angle, from) in self.angles.iter_mut().zip(self.exit_angles) {
            *angle = ((f64::from(from) * remaining).round_ties_even() as i32 & 0x7ff) as u16;
        }
    }

    fn begin_exit(&mut self) {
        self.exiting = true;
        self.exit_angles = self
            .angles
            .map(|angle| i32::from(angle) - if angle > 0x400 { 0x800 } else { 0 });
        self.exit_offset = [-self.pan[0], -self.pan[1], ENTRY_DEPTH - self.depth];
        self.fade = (f64::from(self.fade) + FADE_STEP) as f32;
        self.interpolate_angles();
    }

    /// Whether the exit has started (the host stops the description speech then).
    pub fn exiting(&self) -> bool {
        self.exiting
    }

    /// The model pose to draw after the latest update.
    pub fn pose(&self) -> ViewerPose {
        let offset = if self.exiting {
            self.exit_offset
        } else {
            [0.0, 0.0, ENTRY_DEPTH]
        };
        ViewerPose {
            turn: self.angles[0],
            pitch: self.angles[1],
            roll: self.angles[2],
            position: [
                self.pan[0] + offset[0] * self.fade,
                self.pan[1] + offset[1] * self.fade,
                self.depth + offset[2] * self.fade,
            ],
        }
    }
}

/// The four-character RTX label for a formatted tag: longer text is cut to four characters.
fn label(text: &str) -> [u8; 4] {
    let mut label = [0; 4];
    for (slot, byte) in label.iter_mut().zip(text.bytes()) {
        *slot = byte;
    }
    label
}

/// The description lines the viewer plays for one item, in order. The host stops at the first
/// label that has no RTX record, as the original does.
#[derive(Debug)]
pub struct DescriptionLines {
    labels: std::vec::IntoIter<[u8; 4]>,
}

impl DescriptionLines {
    /// `description` is the item's ITEM.INI label; `potion` is the item's `GetPotion` counter.
    pub fn new(item: i32, description: Option<&str>, potion: i32) -> Self {
        let own = description.map(label);
        let numbered = |first: i32, last: i32, format: &dyn Fn(i32) -> String| {
            (first..=last)
                .map(|n| label(&format(n)))
                .collect::<Vec<_>>()
        };
        let labels = match item {
            20 | 22 | 24 | 26 => own
                .into_iter()
                .chain([label(&format!("cip{}", potion.wrapping_add(1)))])
                .collect(),
            36 => vec![label("cibe"), label("cibl")],
            53 => numbered(2, 8, &|n| format!("iij{n}")),
            55 => [label("cikj")]
                .into_iter()
                .chain(numbered(1, 11, &|n| format!("ny{n:02}")))
                .collect(),
            _ => own.into_iter().collect(),
        };
        Self {
            labels: labels.into_iter(),
        }
    }
}

impl Iterator for DescriptionLines {
    type Item = [u8; 4];
    fn next(&mut self) -> Option<Self::Item> {
        self.labels.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELD: ViewerInput = ViewerInput {
        left: false,
        right: false,
        up: false,
        down: false,
        walk: false,
        jump: false,
        exit: false,
    };

    fn viewer() -> ItemViewer {
        ItemViewer::open([100, 60, 40], [0, 0], 0.75).unwrap()
    }

    fn settle(viewer: &mut ItemViewer) {
        for _ in 0..7 {
            assert!(!viewer.update(HELD));
        }
    }

    #[test]
    fn only_the_scripted_request_form_opens() {
        assert!(ItemViewer::open([1, 1, 1], [1, 0], 0.75).is_err());
        assert!(ItemViewer::open([1, 1, 1], [0, 2], 0.75).is_err());
    }

    #[test]
    fn entry_flies_in_along_the_view_axis_with_zero_angles() {
        let mut viewer = viewer();
        assert_eq!(viewer.pose().position, [0.0, 0.0, 220.0 + 1490.0]);
        viewer.update(HELD);
        assert!((viewer.pose().position[2] - (220.0 + 1490.0 * 0.85)).abs() < 1e-2);
        settle(&mut viewer);
        let pose = viewer.pose();
        assert_eq!(pose.position, [0.0, 0.0, 220.0]);
        assert_eq!([pose.turn, pose.pitch, pose.roll], [0, 0, 0]);
    }

    #[test]
    fn plain_directions_turn_and_pitch_eighty_units_and_wrap() {
        let mut viewer = viewer();
        settle(&mut viewer);
        viewer.update(ViewerInput {
            left: true,
            up: true,
            ..HELD
        });
        let pose = viewer.pose();
        assert_eq!([pose.turn, pose.pitch], [80, 2048 - 80]);
        viewer.update(ViewerInput {
            right: true,
            down: true,
            ..HELD
        });
        let pose = viewer.pose();
        assert_eq!([pose.turn, pose.pitch], [0, 0]);
    }

    #[test]
    fn jump_zooms_and_rolls_and_walk_pans_within_the_limits() {
        let mut viewer = viewer();
        settle(&mut viewer);
        let scale = f64::from(180.0_f32) / 980.0 + 0.1;
        viewer.update(ViewerInput {
            jump: true,
            up: true,
            right: true,
            ..HELD
        });
        let pose = viewer.pose();
        assert_eq!(pose.roll, 80);
        assert!((f64::from(pose.position[2]) - (220.0 + 80.0 * scale)).abs() < 1e-3);
        for _ in 0..200 {
            viewer.update(ViewerInput {
                jump: true,
                down: true,
                ..HELD
            });
        }
        assert_eq!(viewer.pose().position[2], 40.0);
        for _ in 0..200 {
            viewer.update(ViewerInput {
                walk: true,
                right: true,
                down: true,
                jump: true,
                ..HELD
            });
        }
        let pose = viewer.pose();
        let scale = f64::from(0.0_f32) + 0.1;
        let half = (200.0 * scale).round_ties_even() as f32;
        assert_eq!(pose.position[0], half);
        assert_eq!(
            pose.position[1],
            (half as f64 * 0.75).round_ties_even() as f32
        );
    }

    #[test]
    fn exit_waits_for_a_release_then_flies_back_to_the_entry_depth() {
        let mut viewer = viewer();
        viewer.update(ViewerInput { exit: true, ..HELD });
        assert!(!viewer.exiting());
        settle(&mut viewer);
        viewer.update(ViewerInput { exit: true, ..HELD });
        assert!(viewer.exiting());
        // Fade 0.15 at the request, then one step per update until it passes one.
        let mut updates = 0;
        while !viewer.update(HELD) {
            updates += 1;
            assert!(updates < 20);
        }
        assert_eq!(updates, 5);
    }

    #[test]
    fn exit_interpolates_angles_back_the_short_way() {
        let mut viewer = viewer();
        settle(&mut viewer);
        for _ in 0..5 {
            viewer.update(ViewerInput { left: true, ..HELD });
        }
        assert_eq!(viewer.pose().turn, 400);
        viewer.update(ViewerInput { exit: true, ..HELD });
        let pose = viewer.pose();
        assert!(pose.turn > 0 && pose.turn < 400, "{}", pose.turn);
    }

    #[test]
    fn description_lines_follow_the_item_range() {
        let lines = |item, description, potion| {
            DescriptionLines::new(item, description, potion)
                .map(|label| String::from_utf8(label.to_vec()).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(20, Some("cibk"), 3), ["cibk", "cip4"]);
        assert_eq!(lines(26, None, 0), ["cip1"]);
        assert_eq!(lines(36, Some("xxxx"), 0), ["cibe", "cibl"]);
        assert_eq!(
            lines(53, None, 0),
            ["iij2", "iij3", "iij4", "iij5", "iij6", "iij7", "iij8"]
        );
        let sap = lines(55, None, 0);
        assert_eq!(&sap[..3], ["cikj", "ny01", "ny02"]);
        assert_eq!(sap.len(), 12);
        assert_eq!(lines(4, Some("cisw"), 0), ["cisw"]);
        assert!(lines(4, None, 0).is_empty());
        // A counter of nine reaches the two-digit tag, which is cut to four characters.
        assert_eq!(lines(22, None, 9), ["cip1"]);
    }
}
