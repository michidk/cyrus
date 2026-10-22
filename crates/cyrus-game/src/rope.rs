//! Retained ropes from the installed `MPRP` records: construction, swing, chain placement and the
//! attachment list. The rules live in preservation's engine/SOUP.md ("Rope construction and swing",
//! "Rope attachment", "Rope control while attached", "Rope amplitude updates").
//!
//! Angles are engine units (2048 per turn). Positions are the exported world units, one per 20 map
//! units. The engine frame maps to the world frame by negating X and Y and keeping Z (the importer's
//! position decode), so link deltas are computed with the engine's matrices and mapped, and a
//! link's world rotation is the engine matrix conjugated by that mapping.
use serde_json::Value;

/// Engine angle units per full turn.
const TURN: i32 = 2048;
/// The engine keeps at most this many ropes; one more is a fatal load error.
pub const MAX_ROPES: usize = 255;

/// `trunc(sin(i × π / 1024) × 65536)` for `i` in 0..2048, the engine's integer sine table.
fn sine(index: i32) -> i32 {
    let index = index.rem_euclid(TURN);
    if index > 1024 {
        -sine(TURN - index)
    } else if index == 1024 {
        0
    } else {
        (f64::from(index) * std::f64::consts::PI / 1024.0)
            .sin()
            .mul_add(65536.0, 0.0) as i32
    }
}

/// An `MPRP` record as exported in the world metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct RopeRecord {
    /// World-unit base position.
    pub position: [f32; 3],
    pub heading: i32,
    pub kind: u8,
    pub swing: i32,
    pub speed: i32,
    pub links: usize,
    pub has_end_model: bool,
    /// Effect numbers for low, medium and high swing; zero is none.
    pub sounds: [i32; 3],
}

impl RopeRecord {
    /// Reads one entry of the exported `ropes` metadata list.
    pub fn from_metadata(value: &Value) -> anyhow::Result<Self> {
        let number = |key: &str| {
            value[key]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("rope record: {key} missing"))
        };
        let position = value["position"]
            .as_array()
            .filter(|p| p.len() == 3)
            .ok_or_else(|| anyhow::anyhow!("rope record: position missing"))?;
        let coordinate = |i: usize| {
            position[i]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("rope record: position axis {i}"))
        };
        let sounds = &value["unknown1"];
        let sound = |i: usize| sounds[i].as_i64().unwrap_or(0) as i32;
        Ok(Self {
            position: [
                coordinate(0)? as f32,
                coordinate(1)? as f32,
                coordinate(2)? as f32,
            ],
            heading: number("angle_y")? as i32,
            kind: number("rope_type")? as u8,
            swing: number("swing")? as i32,
            speed: number("speed")? as i32,
            links: usize::try_from(number("length")?.max(0))?,
            has_end_model: value["static_model"]
                .as_str()
                .is_some_and(|m| !m.is_empty()),
            sounds: [sound(0), sound(1), sound(2)],
        })
    }
}

/// One actor hanging on a rope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub actor: usize,
    /// Link index the actor sits on.
    pub node: i32,
    /// Planar speed carried at the link when it was last placed, used by the detach motion.
    pub motion: i32,
}

/// Pose of one link: position in world units and the yaw and pitch of its matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkPose {
    pub position: [f32; 3],
    pub yaw: i32,
    pub pitch: i32,
}

impl LinkPose {
    /// The link's rotation in the world frame as rows (`v' = R · v`): the engine matrix with the
    /// X/Z, Y/Z entries negated by the frame mapping.
    pub fn world_rotation(&self) -> [[f32; 3]; 3] {
        let mut m = matrix(self.yaw, self.pitch);
        for (row, column) in [(0, 2), (2, 0), (1, 2), (2, 1)] {
            m[row][column] = -m[row][column];
        }
        m
    }
}

/// A sound the swing asks for at a zero crossing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwingSound {
    pub effect: i32,
}

#[derive(Clone, Debug)]
pub struct Rope {
    pub position: [f32; 3],
    pub heading: i32,
    pub kind: u8,
    pub maximum: i32,
    pub speed: i32,
    pub links: usize,
    /// Link model height in world units (shipped link models are 0.8 tall).
    pub link_length: f32,
    pub amplitude: i32,
    pub phase: i32,
    /// Step angle added per link (offset 0x66).
    pub step: i32,
    /// Pending amplitude push (offset 0x62).
    pub push: i32,
    /// Swing direction byte (offset 0x38).
    pub direction: u8,
    /// Previous sum of link steps (offset 0x5e).
    pub previous_sum: i32,
    pub sounds: [i32; 3],
    pub attachments: Vec<Attachment>,
    has_end_model: bool,
}

impl Rope {
    /// Whether the record names an end model.
    pub fn has_end_model(&self) -> bool {
        self.has_end_model
    }

    pub fn new(record: &RopeRecord, link_length: f32) -> Self {
        let mut kind = record.kind;
        if record.has_end_model {
            kind |= 0x10;
        }
        Self {
            position: record.position,
            heading: record.heading,
            kind,
            maximum: record.swing,
            speed: if record.speed < 1 { 199 } else { record.speed },
            links: record.links,
            link_length,
            amplitude: if kind & 0x20 != 0 { record.swing } else { 0 },
            phase: 0,
            step: 0,
            push: 0,
            direction: 0,
            previous_sum: 0,
            sounds: record.sounds,
            attachments: Vec::new(),
            has_end_model: record.has_end_model,
        }
    }

    /// One update: the swing step, then each attachment's climb. `group_of` is the actor's current
    /// animation group and `limit` the lowest allowed node for a downward step (the hand-height link
    /// choice); `frame_scale` is the Q8 frame scale (256 per nominal tick) and `ticks` the whole tick count.
    pub fn update(
        &mut self,
        frame_scale: i32,
        ticks: i32,
        group_of: impl Fn(usize) -> i32,
        limit: impl Fn(&Self, usize, i32) -> i32,
    ) {
        if self.kind & 0xc == 0 {
            self.amplitude = self.amplitude.clamp(0, self.maximum.max(0));
            if self.amplitude == 0 {
                self.step = 0;
            } else {
                self.phase += (self.speed * 0x40 * frame_scale) >> 8;
                self.step = (sine((self.phase >> 8) & 0x7ff) * self.amplitude) >> 9;
            }
        } else {
            // The swinging types hold the maximum amplitude; their step angle is the table's zero
            // entry, so they do not tilt.
            self.amplitude = self.maximum;
            self.phase += (self.speed * 0x40 * frame_scale) >> 8;
            self.step = 0;
        }
        for index in 0..self.attachments.len() {
            let Attachment { actor, node, .. } = self.attachments[index];
            let group = group_of(actor);
            let kick = |rope: &mut Self| {
                if rope.amplitude == 0 && rope.push == 0 {
                    rope.phase = 0;
                    rope.amplitude = 2;
                }
            };
            if group == 0x52 && ticks != 0 {
                self.attachments[index].node = node - 1;
                kick(self);
            } else if group == 0x53 && ticks != 0 && node < limit(self, actor, node + 1) {
                self.attachments[index].node = node + 1;
                kick(self);
            }
        }
    }

    /// Clamps each attachment's node into `1..links`, as the chain update does before placement.
    pub fn clamp_nodes(&mut self) {
        let last = i32::try_from(self.links).unwrap_or(i32::MAX) - 1;
        for attachment in &mut self.attachments {
            attachment.node = attachment.node.clamp(1, last.max(1));
        }
    }

    /// Link poses from the base: link 0 sits at the rope position; each later link adds the
    /// previous link's matrix applied to one link length, and pitches by the accumulated step.
    pub fn chain(&self) -> Vec<LinkPose> {
        let yaw = (-self.heading).rem_euclid(TURN);
        let mut poses = Vec::with_capacity(self.links);
        let mut position = self.position.map(f64::from);
        let mut accumulated = 0i32;
        for link in 0..self.links {
            let pitch = if link == 0 {
                (-(self.amplitude >> 8)).rem_euclid(TURN)
            } else {
                let value = (-(accumulated >> 8)).rem_euclid(TURN);
                accumulated += self.step;
                value
            };
            poses.push(LinkPose {
                position: position.map(|v| v as f32),
                yaw,
                pitch,
            });
            let m = matrix(yaw, pitch);
            let length = f64::from(self.link_length);
            // The engine adds `M · (0, length, 0)` in its frame; the world delta negates X and Y.
            position[0] -= f64::from(m[0][1]) * length;
            position[1] -= f64::from(m[1][1]) * length;
            position[2] += f64::from(m[2][1]) * length;
        }
        poses
    }

    /// The end model's pose: one link length below the last link, with the same yaw and the pitch
    /// of the accumulated step angles. `None` for a rope without links.
    pub fn end_pose(&self) -> Option<LinkPose> {
        let last = *self.chain().last()?;
        let m = matrix(last.yaw, last.pitch);
        let length = f64::from(self.link_length);
        let position = [
            f64::from(last.position[0]) - f64::from(m[0][1]) * length,
            f64::from(last.position[1]) - f64::from(m[1][1]) * length,
            f64::from(last.position[2]) + f64::from(m[2][1]) * length,
        ];
        let accumulated = self.step * i32::try_from(self.links.saturating_sub(1)).unwrap_or(0);
        Some(LinkPose {
            position: position.map(|v| v as f32),
            yaw: last.yaw,
            pitch: (-(accumulated >> 8)).rem_euclid(TURN),
        })
    }

    /// Sum of the link step angles, the argument of [`Self::amplitude_service`].
    pub fn step_sum(&self) -> i32 {
        i32::try_from(self.links.saturating_sub(1)).unwrap_or(0) * self.step
    }

    /// The amplitude service at the end of a chain update (see preservation "Rope amplitude updates").
    pub fn amplitude_service(&mut self, sum: i32) -> Option<SwingSound> {
        let previous = self.previous_sum;
        let mut sound = None;
        let crossing =
            (sum >= 0 && previous < 0) || (sum < 0 && previous >= 0) || (sum == 0 && previous == 0);
        if crossing {
            let quarter = self.maximum / 4;
            if !self.attachments.is_empty() && quarter > 2 {
                let [low, mid, high] = self.sounds;
                let effect = if self.amplitude >= quarter * 3 && high != 0 {
                    high
                } else if self.amplitude >= quarter * 2 && mid != 0 {
                    mid
                } else if self.amplitude >= quarter && low != 0 {
                    low
                } else {
                    0
                };
                if effect != 0 {
                    sound = Some(SwingSound { effect });
                }
            }
            let u = self.maximum / 5;
            if self.push > 0 {
                let p = self.push;
                let step = if p >= 5 && p > 4 * u {
                    5
                } else if p >= 4 && p > 3 * u {
                    4
                } else if p >= 2 && p > 2 * u {
                    2
                } else {
                    1
                };
                self.amplitude += step;
                self.push -= step;
            } else if self.push < 0 {
                let n = -self.push;
                let step = if n <= 3 * u {
                    if n > 2 || n <= 2 * u { 1 } else { 2 }
                } else if n <= 4 * u {
                    4
                } else {
                    5
                };
                self.amplitude -= step;
                self.push += step;
            } else if sum != 0 || previous != 0 {
                self.amplitude -= if self.amplitude > 4 * u {
                    5
                } else if self.amplitude > 3 * u {
                    4
                } else if self.amplitude > 2 * u {
                    2
                } else {
                    i32::from(self.amplitude != 0)
                };
            }
        }
        self.direction = u8::from(!(sum != 0 && previous < sum));
        self.previous_sum = sum;
        sound
    }
}

/// `Yaw · Pitch` built with the engine's helpers: identity, then the yaw rotation of columns 0
/// and 2, then the pitch rotation of columns 1 and 2, each applied from the right.
fn matrix(yaw: i32, pitch: i32) -> [[f32; 3]; 3] {
    let angle = |units: i32| {
        let radians = f64::from(units) * std::f64::consts::TAU / f64::from(TURN);
        (radians.sin() as f32, radians.cos() as f32)
    };
    let mut m = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let (s, c) = angle(yaw);
    for row in &mut m {
        let old = row[2];
        row[2] = row[0] * s + row[2] * c;
        row[0] = row[0] * c - old * s;
    }
    let (s, c) = angle(pitch);
    for row in &mut m {
        let old = row[2];
        row[2] = row[2] * c - row[1] * s;
        row[1] = old * s + row[1] * c;
    }
    m
}

/// Keys the pump service reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct PumpKeys {
    pub left: bool,
    pub right: bool,
}

impl Rope {
    /// The pump service (preservation "Rope control while attached"): `difference` is the absolute
    /// heading difference between the attached actor and the rope heading.
    pub fn pump(&mut self, keys: PumpKeys, difference: i32) {
        if self.kind & 0x8 != 0 {
            return;
        }
        if !keys.left && !keys.right {
            self.push = 0;
            return;
        }
        if self.amplitude == 0 {
            self.push += 3;
            self.amplitude = 2;
            let (first, second) = if difference < 16 {
                (keys.left, keys.right)
            } else {
                (keys.right, keys.left)
            };
            if first {
                self.phase = 0x40000;
            } else if second {
                self.phase = 0;
            }
            return;
        }
        let mut direction = self.direction;
        if self.kind & 0x1 != 0 && difference > 16 {
            direction = (direction + 1) & 1;
        }
        let (push, release) = if direction == 1 {
            (keys.left, keys.right)
        } else {
            (keys.right, keys.left)
        };
        if push {
            self.push += 3;
        } else if release && self.amplitude > 2 {
            self.push -= 3;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(links: usize, maximum: i32) -> Rope {
        Rope::new(
            &RopeRecord {
                position: [0.0, 10.0, 0.0],
                heading: 0,
                kind: 0,
                swing: maximum,
                speed: 100,
                links,
                has_end_model: false,
                sounds: [60, 61, 62],
            },
            0.8,
        )
    }

    #[test]
    fn the_sine_table_matches_the_documented_construction() {
        assert_eq!(sine(0), 0);
        assert_eq!(sine(512), 65536);
        assert_eq!(sine(1024), 0);
        assert_eq!(sine(1536), -65536);
        assert_eq!(sine(2048 - 100), -sine(100));
    }

    #[test]
    fn a_rope_at_rest_hangs_straight_from_its_base() {
        let chain = rope(3, 10).chain();
        let ys: Vec<f32> = chain.iter().map(|l| l.position[1]).collect();
        assert!(
            (ys[0] - 10.0).abs() < 1e-4 && (ys[1] - 9.2).abs() < 1e-4 && (ys[2] - 8.4).abs() < 1e-4
        );
        assert!(
            chain
                .iter()
                .all(|l| l.position[0].abs() < 1e-4 && l.position[2].abs() < 1e-4)
        );
    }

    #[test]
    fn the_world_rotation_keeps_the_engine_yaw_convention_of_placed_objects() {
        // Engine yaw minus the heading, mapped to the world frame, is the importer's positive yaw.
        let pose = LinkPose {
            position: [0.0; 3],
            yaw: (-512i32).rem_euclid(2048),
            pitch: 0,
        };
        let r = pose.world_rotation();
        // A quarter turn: [[0, 0, 1], [0, 1, 0], [-1, 0, 0]].
        assert!(
            r[0][0].abs() < 1e-6 && (r[0][2] - 1.0).abs() < 1e-6 && (r[2][0] + 1.0).abs() < 1e-6
        );
    }

    #[test]
    fn the_end_model_hangs_one_link_below_the_last() {
        let r = rope(3, 10);
        let end = r.end_pose().unwrap();
        assert!((end.position[1] - 7.6).abs() < 1e-4);
        assert!(rope(0, 10).end_pose().is_none());
    }

    #[test]
    fn a_swinging_rope_bends_and_the_base_stays_put() {
        let mut r = rope(10, 20);
        r.amplitude = 20;
        r.phase = 0x100 * 300;
        r.update(256, 1, |_| 0, |_, _, _| 0);
        assert!(r.step != 0);
        let chain = r.chain();
        assert_eq!(chain[0].position, [0.0, 10.0, 0.0]);
        assert!(chain.last().unwrap().position[2].abs() > 0.01);
    }

    #[test]
    fn constructor_follows_the_record() {
        let slow = Rope::new(
            &RopeRecord {
                speed: 0,
                kind: 0x20,
                has_end_model: true,
                ..rope(1, 7).as_record()
            },
            0.8,
        );
        assert_eq!(slow.speed, 199);
        assert_eq!(slow.amplitude, 7);
        assert_eq!(slow.kind, 0x30);
    }

    impl Rope {
        fn as_record(&self) -> RopeRecord {
            RopeRecord {
                position: self.position,
                heading: self.heading,
                kind: self.kind,
                swing: self.maximum,
                speed: self.speed,
                links: self.links,
                has_end_model: false,
                sounds: self.sounds,
            }
        }
    }

    #[test]
    fn climbing_moves_the_node_and_kicks_a_resting_rope() {
        let mut r = rope(10, 10);
        r.attachments.push(Attachment {
            actor: 1,
            node: 5,
            motion: 0,
        });
        r.update(256, 1, |_| 0x52, |_, _, _| 0);
        assert_eq!((r.attachments[0].node, r.amplitude, r.phase), (4, 2, 0));
        let mut down = rope(10, 10);
        down.attachments.push(Attachment {
            actor: 1,
            node: 5,
            motion: 0,
        });
        down.update(256, 1, |_| 0x53, |_, _, wanted| wanted + 1);
        assert_eq!(down.attachments[0].node, 6);
        let mut blocked = rope(10, 10);
        blocked.attachments.push(Attachment {
            actor: 1,
            node: 5,
            motion: 0,
        });
        blocked.update(256, 1, |_| 0x53, |_, _, _| 5);
        assert_eq!(
            blocked.attachments[0].node, 5,
            "the limit routine blocks the step"
        );
    }

    #[test]
    fn the_amplitude_service_follows_the_push_in_tiers_and_decays_without_one() {
        let mut r = rope(10, 20);
        r.amplitude = 10;
        r.push = 6;
        r.amplitude_service(0);
        // u = 4: p = 6 is at least 5 but not above 4u = 16, so the step is 1.
        assert_eq!((r.amplitude, r.push), (11, 5));
        let mut big = rope(10, 5);
        big.amplitude = 1;
        big.push = 6;
        big.amplitude_service(0);
        assert_eq!(
            (big.amplitude, big.push),
            (6, 1),
            "u = 1: p = 6 exceeds 4u, step 5"
        );
        let mut down = rope(10, 20);
        down.amplitude = 10;
        down.push = -2;
        down.amplitude_service(0);
        assert_eq!((down.amplitude, down.push), (9, -1));
        let mut decay = rope(10, 20);
        decay.amplitude = 18;
        decay.previous_sum = 3;
        decay.amplitude_service(-3);
        assert_eq!(
            decay.amplitude, 13,
            "above 4u = 16 the amplitude falls by 5"
        );
        assert_eq!(decay.previous_sum, -3);
    }

    #[test]
    fn crossings_play_the_swing_sound_for_the_amplitude() {
        let mut r = rope(10, 20);
        r.attachments.push(Attachment {
            actor: 1,
            node: 3,
            motion: 0,
        });
        r.amplitude = 5;
        assert_eq!(
            r.amplitude_service(0),
            Some(SwingSound { effect: 60 }),
            "a quarter plays the low sound"
        );
        r.amplitude = 10;
        r.previous_sum = 0;
        assert_eq!(
            r.amplitude_service(0),
            Some(SwingSound { effect: 61 }),
            "half plays the middle sound"
        );
        r.amplitude = 15;
        r.amplitude = 15;
        r.previous_sum = 0;
        assert_eq!(
            r.amplitude_service(0),
            Some(SwingSound { effect: 62 }),
            "three quarters plays the high sound"
        );
        r.amplitude = 20;
        r.previous_sum = 1;
        assert_eq!(r.amplitude_service(5), None, "no zero crossing, no sound");
    }

    #[test]
    fn pumping_starts_a_resting_rope_and_then_builds_on_its_direction() {
        let mut r = rope(10, 20);
        r.pump(
            PumpKeys {
                left: true,
                right: false,
            },
            0,
        );
        assert_eq!((r.push, r.amplitude, r.phase), (3, 2, 0x40000));
        r.direction = 1;
        r.pump(
            PumpKeys {
                left: true,
                right: false,
            },
            0,
        );
        assert_eq!(r.push, 6);
        r.pump(
            PumpKeys {
                left: false,
                right: true,
            },
            0,
        );
        r.amplitude = 5;
        r.pump(
            PumpKeys {
                left: false,
                right: true,
            },
            0,
        );
        assert_eq!(
            r.push, 3,
            "the other key takes it back while the amplitude exceeds 2"
        );
        r.pump(PumpKeys::default(), 0);
        assert_eq!(r.push, 0);
    }

    #[test]
    fn records_read_from_the_exported_metadata() {
        let value = serde_json::json!({
            "angle_y": 512, "length": 4, "position": [1.0, 2.0, 3.0], "rope_model": "ROPELINK",
            "rope_type": 1, "speed": 100, "static_model": "CC#CHAND", "swing": 9,
            "unknown1": [62, 62, 63, 0, 0, 0, 0]
        });
        let record = RopeRecord::from_metadata(&value).unwrap();
        assert_eq!((record.heading, record.links, record.kind), (512, 4, 1));
        assert!(record.has_end_model);
        assert_eq!(record.sounds, [62, 62, 63]);
    }
}
