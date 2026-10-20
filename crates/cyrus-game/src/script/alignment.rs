//! `PlayerLineUp` arithmetic from preservation's engine/SOUP.md "Player alignment ownership and
//! targets" and "Alignment distance thresholds". The functions are pure; the native handler that
//! owns the mode, fallback flag and reference, and the player's movement, are not implemented.

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
