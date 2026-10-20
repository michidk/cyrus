//! Player alignment arithmetic from preservation's engine/SOUP.md "Player alignment ownership and
//! targets", "Alignment distance thresholds" and "PlayerFaceObj". The functions are pure.
//! `PlayerFaceObj` runs on them as a native task; the `PlayerLineUp` native that owns the mode,
//! fallback flag and reference, and the player's movement, is not implemented.

/// Attribute index of the speed byte for the player's current animation group, or `None` when
/// the group selects no speed (the speed is then zero).
pub fn speed_attribute(group: u16) -> Option<usize> {
    match group {
        18 | 30 | 142 => Some(8),
        19 | 143 => Some(9),
        20 | 21 => Some(12),
        22 | 23 => Some(13),
        24..=27 => Some(10),
        31 => Some(19),
        92 | 93 => Some(17),
        _ => None,
    }
}

/// Speed byte for `group`: zero without a current group or attribute table, and for groups that
/// select no attribute.
pub fn speed(group: Option<u16>, attributes: Option<&[u8]>) -> u8 {
    group
        .and_then(speed_attribute)
        .zip(attributes)
        .map_or(0, |(index, attributes)| attributes[index])
}

fn scaled(value: i32, scale: i32) -> i32 {
    value.wrapping_mul(scale) >> 8
}

/// Arrival test: a horizontal distance at or below this snaps the player to the target.
pub fn arrival_threshold(speed: u8, scale: i32) -> i32 {
    scaled((2 * i32::from(speed) - 1).wrapping_shl(8), scale)
}

/// The facing helper's arrival threshold.
pub fn facing_threshold(speed: u8, scale: i32) -> i32 {
    scaled((i32::from(speed) + 1).wrapping_shl(8), scale)
}

/// The movement-distance threshold, a third test that never snaps.
pub fn movement_threshold(speed: u8, scale: i32) -> i32 {
    (i32::from(speed) * 512 - 1).wrapping_mul(scale) >> 8
}

/// Whether a movement attempt is permitted: actor state 7, speed zero, a nonpositive threshold,
/// or a distance strictly greater than the threshold.
pub fn movement_permitted(state: u8, speed: u8, scale: i32, distance: i32) -> bool {
    let threshold = movement_threshold(speed, scale);
    state == 7 || speed == 0 || threshold <= 0 || distance > threshold
}

/// Bearing from the player to the target relative to the player's heading, wrapped to 2048:
/// `b` of the movement-step selection. `displacement` is the target minus the player, X then Z.
pub fn relative_bearing(displacement: [i32; 2], heading: u16) -> u16 {
    let direction = i32::from(super::spatial::query_bearing(
        displacement[0],
        displacement[1],
    ));
    ((-((-direction & 0x7ff) - i32::from(heading))) & 0x7ff) as u16
}

/// Close alignment's directional movement helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseStep {
    Right,
    Backward,
    Left,
    Forward,
}

/// Helper chosen by the pre-step relative bearing: right for 257 through 768, backward for 769
/// through 1280, left for 1281 through 1792, forward otherwise.
pub fn close_step(bearing: u16) -> CloseStep {
    match bearing {
        257..=768 => CloseStep::Right,
        769..=1280 => CloseStep::Backward,
        1281..=1792 => CloseStep::Left,
        _ => CloseStep::Forward,
    }
}

/// Whether a directional helper keeps its step, given the bearing measured again after it
/// (`after`) and, for backward, the distances before and after. Forward is also approach mode's
/// test.
pub fn keeps_step(step: CloseStep, after: u16, distance: (i32, i32)) -> bool {
    match step {
        CloseStep::Right => (257..=767).contains(&after),
        CloseStep::Left => (1281..=1791).contains(&after),
        CloseStep::Forward => !(256..=1792).contains(&after),
        CloseStep::Backward => distance.1 <= distance.0,
    }
}

/// Roll after the alignment's roll leveling by `ticks`: upper-half rolls advance toward the wrap and
/// lower-half rolls fall toward zero by the tick count, reaching zero when within one step. The
/// helper's return is ignored by alignment.
pub fn level_roll(roll: u16, ticks: u32) -> u16 {
    let roll = u32::from(roll);
    if roll > 0x400 {
        if roll + 0x800 < ticks {
            0
        } else {
            ((roll + ticks) & 0x7ff) as u16
        }
    } else if roll > 0 {
        if roll < ticks {
            0
        } else {
            ((roll - ticks) & 0x7ff) as u16
        }
    } else {
        0
    }
}

/// Whether the player's packed collision-response word carries a mark that makes alignment fall
/// back: the `0x01e2` mask over the body-response byte followed by the transition byte.
pub fn fallback_response(body: u8, transition: u8) -> bool {
    body & 0xe2 != 0 || transition & 1 != 0
}

/// Whether a collision response older than eight updates abandons alignment: a nonzero packed
/// response word with an age above eight.
pub fn response_expired(word_nonzero: bool, age: u8) -> bool {
    word_nonzero && age > 8
}

/// The facing helper's inputs: the player's pose and animation-derived values, and the target.
#[derive(Clone, Copy, Debug)]
pub struct FacingInput {
    pub position: [i32; 3],
    pub heading: u16,
    pub target: [i32; 3],
    /// [`speed`] of the player's current group.
    pub speed: u8,
    pub frame_scale: u32,
    /// Attribute 21 holding a weapon or in combat mode, otherwise attribute 15.
    pub rate_attribute: u8,
    pub holding_weapon: bool,
    pub torch: bool,
}

/// One facing step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    /// Within the facing arrival threshold: the player's X/Z snap to the target's. Reports zero.
    Arrived,
    /// The heading after the step and the turn animation it requests. No requested group means the
    /// heading reached the target, which reports zero; otherwise the step reports nonzero.
    Turning { heading: u16, group: Option<u16> },
}

impl Facing {
    /// Whether the facing helper reports an unfinished turn.
    pub fn ongoing(self) -> bool {
        matches!(self, Self::Turning { group: Some(_), .. })
    }
}

/// The facing helper behind `PlayerFaceObj`: snaps onto the target within the facing arrival
/// threshold of the horizontal distance, otherwise turns toward the target's bearing by the
/// [support-response turn](super::ordinary_slope_turn).
pub fn face(input: FacingInput) -> anyhow::Result<Facing> {
    let FacingInput {
        position,
        heading,
        target,
        ..
    } = input;
    let distance = super::spatial::rounded_distance([
        position[0].wrapping_sub(target[0]),
        0,
        position[2].wrapping_sub(target[2]),
    ])?;
    if distance <= facing_threshold(input.speed, input.frame_scale as i32) {
        return Ok(Facing::Arrived);
    }
    let bearing = i32::from(super::spatial::query_bearing(
        target[0].wrapping_sub(position[0]),
        target[2].wrapping_sub(position[2]),
    ));
    let turn = super::ordinary_slope_turn(
        heading,
        (-bearing & 0x7ff) as u16,
        input.rate_attribute,
        input.frame_scale,
        input.holding_weapon,
        input.torch,
    );
    Ok(Facing::Turning {
        heading: turn.heading,
        group: turn.group,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_follows_the_current_group_and_missing_state_is_zero() {
        let mut attributes = [0u8; 256];
        for (index, value) in [(8, 1), (9, 2), (10, 3), (12, 4), (13, 5), (17, 6), (19, 7)] {
            attributes[index] = value;
        }
        let expected = [
            (18, 1),
            (30, 1),
            (142, 1),
            (19, 2),
            (143, 2),
            (24, 3),
            (27, 3),
            (20, 4),
            (21, 4),
            (22, 5),
            (23, 5),
            (92, 6),
            (93, 6),
            (31, 7),
            (28, 0),
            (0, 0),
        ];
        for (group, value) in expected {
            assert_eq!(
                speed(Some(group), Some(&attributes)),
                value,
                "group {group}"
            );
        }
        assert_eq!(speed(None, Some(&attributes)), 0);
        assert_eq!(speed(Some(18), None), 0);
    }

    #[test]
    fn thresholds_have_inclusive_boundaries_and_zero_speed_rejects_arrival() {
        // Speed 3 at scale 256: ((2 * 3 - 1) << 8) = 1280 arrives at or below 1280.
        assert_eq!(arrival_threshold(3, 256), 1280);
        assert_eq!(facing_threshold(3, 256), 1024);
        assert_eq!(movement_threshold(3, 256), 1535);
        // Half scale halves each product.
        assert_eq!(arrival_threshold(3, 128), 640);
        // Zero speed: the arrival threshold is negative and cannot admit a distance, the facing
        // helper's still snaps, and the movement check admits through the speed itself.
        assert!(arrival_threshold(0, 256) < 0);
        assert_eq!(facing_threshold(0, 256), 256);
        assert!(movement_permitted(0, 0, 256, 0));
    }

    #[test]
    fn movement_permission_needs_state_seven_zero_speed_a_nonpositive_threshold_or_distance() {
        // Speed 3 at scale 256: threshold 1535.
        assert!(!movement_permitted(0, 3, 256, 1535));
        assert!(movement_permitted(0, 3, 256, 1536));
        assert!(movement_permitted(7, 3, 256, 0));
        // A zero scale makes the threshold zero: nonpositive, so movement is permitted.
        assert!(movement_permitted(0, 3, 0, 0));
    }

    fn facing(position: [i32; 3], heading: u16, target: [i32; 3], speed: u8) -> Facing {
        face(FacingInput {
            position,
            heading,
            target,
            speed,
            frame_scale: 256,
            rate_attribute: 12,
            holding_weapon: false,
            torch: false,
        })
        .unwrap()
    }

    #[test]
    fn facing_snaps_within_the_facing_threshold_and_otherwise_turns_toward_the_target() {
        // Speed zero at scale 256: the facing threshold is 256, inclusive.
        assert_eq!(facing([0; 3], 0, [0, 90, 256], 0), Facing::Arrived);
        assert!(matches!(
            facing([0; 3], 0, [0, 0, 257], 0),
            Facing::Turning { .. }
        ));
        // The target straight toward negative Z has heading 1024; the turn is one rate step.
        for (heading, expected) in [(0, (12, 14)), (2047, (2035, 15)), (1100, (1088, 15))] {
            assert_eq!(
                facing([0; 3], heading, [0, 0, -25600], 0),
                Facing::Turning {
                    heading: expected.0,
                    group: Some(expected.1),
                },
                "heading {heading}"
            );
        }
        // Already at the target heading: no animation request, so the helper reports zero.
        let aligned = facing([0; 3], 1024, [0, 0, -25600], 0);
        assert_eq!(
            aligned,
            Facing::Turning {
                heading: 1024,
                group: None
            }
        );
        assert!(!aligned.ongoing());
    }

    #[test]
    fn collision_responses_gate_alignment() {
        assert!(!fallback_response(0, 0));
        assert!(!fallback_response(0x1d, 0xfe));
        for mark in [0x02, 0x20, 0x40, 0x80] {
            assert!(fallback_response(mark, 0), "body mark {mark:#x}");
        }
        assert!(fallback_response(0, 1));
        assert!(!response_expired(true, 8));
        assert!(response_expired(true, 9));
        assert!(!response_expired(false, 200));
    }

    #[test]
    fn the_close_helper_follows_the_relative_bearing_bands() {
        for (bearing, expected) in [
            (0, CloseStep::Forward),
            (256, CloseStep::Forward),
            (257, CloseStep::Right),
            (768, CloseStep::Right),
            (769, CloseStep::Backward),
            (1280, CloseStep::Backward),
            (1281, CloseStep::Left),
            (1792, CloseStep::Left),
            (1793, CloseStep::Forward),
            (2047, CloseStep::Forward),
        ] {
            assert_eq!(close_step(bearing), expected, "bearing {bearing}");
        }
    }

    #[test]
    fn each_helper_keeps_its_step_only_inside_its_band() {
        assert!(keeps_step(CloseStep::Right, 257, (0, 0)));
        assert!(!keeps_step(CloseStep::Right, 256, (0, 0)));
        assert!(!keeps_step(CloseStep::Right, 768, (0, 0)));
        assert!(keeps_step(CloseStep::Left, 1281, (0, 0)));
        assert!(!keeps_step(CloseStep::Left, 1792, (0, 0)));
        assert!(keeps_step(CloseStep::Forward, 1793, (0, 0)));
        assert!(keeps_step(CloseStep::Forward, 255, (0, 0)));
        assert!(!keeps_step(CloseStep::Forward, 256, (0, 0)));
        assert!(!keeps_step(CloseStep::Forward, 1792, (0, 0)));
        assert!(keeps_step(CloseStep::Backward, 0, (500, 500)));
        assert!(!keeps_step(CloseStep::Backward, 0, (500, 501)));
    }

    #[test]
    fn roll_leveling_steps_by_the_tick_count_and_snaps_within_one_step() {
        assert_eq!(level_roll(0, 3), 0);
        assert_eq!(level_roll(10, 3), 7);
        // Exactly one tick away reaches zero, while a smaller roll snaps to zero.
        assert_eq!(level_roll(3, 3), 0);
        assert_eq!(level_roll(2, 3), 0);
        // The upper half advances toward the wrap.
        assert_eq!(level_roll(0x500, 3), 0x503);
        assert_eq!(level_roll(0x7ff, 3), 2);
    }

    /// Rows run through the original roll-leveling routine over synthetic actors (research oracle):
    /// `(ticks, roll, result)`.
    #[test]
    fn roll_leveling_matches_the_original_routine() {
        let rows: &[(u32, u16, u16)] = &[
            (1, 0, 0),
            (1, 1, 0),
            (1, 2, 1),
            (1, 3, 2),
            (1, 5, 4),
            (1, 10, 9),
            (1, 31, 30),
            (1, 32, 31),
            (1, 1023, 1022),
            (1, 1024, 1023),
            (1, 1025, 1026),
            (1, 1280, 1281),
            (1, 2032, 2033),
            (1, 2045, 2046),
            (1, 2046, 2047),
            (1, 2047, 0),
            (3, 0, 0),
            (3, 1, 0),
            (3, 2, 0),
            (3, 3, 0),
            (3, 5, 2),
            (3, 10, 7),
            (3, 31, 28),
            (3, 32, 29),
            (3, 1023, 1020),
            (3, 1024, 1021),
            (3, 1025, 1028),
            (3, 1280, 1283),
            (3, 2032, 2035),
            (3, 2045, 0),
            (3, 2046, 1),
            (3, 2047, 2),
            (6, 0, 0),
            (6, 1, 0),
            (6, 2, 0),
            (6, 3, 0),
            (6, 5, 0),
            (6, 10, 4),
            (6, 31, 25),
            (6, 32, 26),
            (6, 1023, 1017),
            (6, 1024, 1018),
            (6, 1025, 1031),
            (6, 1280, 1286),
            (6, 2032, 2038),
            (6, 2045, 3),
            (6, 2046, 4),
            (6, 2047, 5),
            (24, 0, 0),
            (24, 1, 0),
            (24, 2, 0),
            (24, 3, 0),
            (24, 5, 0),
            (24, 10, 0),
            (24, 31, 7),
            (24, 32, 8),
            (24, 1023, 999),
            (24, 1024, 1000),
            (24, 1025, 1049),
            (24, 1280, 1304),
            (24, 2032, 8),
            (24, 2045, 21),
            (24, 2046, 22),
            (24, 2047, 23),
        ];
        for &(ticks, roll, expected) in rows {
            assert_eq!(
                level_roll(roll, ticks),
                expected,
                "roll {roll} ticks {ticks}"
            );
        }
    }

    #[test]
    fn the_relative_bearing_wraps_and_is_zero_straight_ahead() {
        // Heading zero faces +Z: a target straight ahead has bearing zero.
        assert_eq!(relative_bearing([0, 1000], 0), 0);
        assert!(relative_bearing([0, -1000], 0) != 0);
        for heading in [0u16, 512, 1024, 2047] {
            assert!(relative_bearing([500, 300], heading) < 2048);
        }
    }
}
