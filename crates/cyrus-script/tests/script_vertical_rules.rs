//! The falling vertical writer, as recovered from the original: a positive signed timer sets the
//! height from the fall origin minus jump height times the factor for the timer (limited), a
//! nonpositive one adds jump height times factor entry zero times two more than the ticks
//! (limited); model-controlled motion changes nothing.
use cyrus_script::script::{
    JumpMotion, OrdinaryVerticalInputs, OrdinaryVerticalState, ordinary_falling_vertical,
    ordinary_vertical_factor,
};

fn state(timer: u8) -> OrdinaryVerticalState {
    OrdinaryVerticalState {
        y: 1000,
        origin_y: 0,
        timer,
        queued: JumpMotion::NONE,
        falling: JumpMotion::STANDING,
    }
}

#[test]
fn the_factor_sequence_rises_to_256_at_index_thirty() {
    assert_eq!(ordinary_vertical_factor(0).unwrap(), 16);
    assert_eq!(ordinary_vertical_factor(10).unwrap(), 153);
    assert_eq!(ordinary_vertical_factor(29).unwrap(), 256);
    assert_eq!(ordinary_vertical_factor(30).unwrap(), 256);
    assert_eq!(ordinary_vertical_factor(69).unwrap(), 256);
    assert!(ordinary_vertical_factor(70).is_err());
    assert!(ordinary_vertical_factor(-1).is_err());
}

#[test]
fn a_positive_timer_sets_the_height_from_the_fall_origin_and_counts_down() {
    // limit = post_collide_height * 512 - 256 = 3840; 32 * 153 = 4896 is below it once negated.
    let input = OrdinaryVerticalInputs::for_falling(10, 1, 0, 8, false).unwrap();
    let after = ordinary_falling_vertical(state(10), 32, input);
    assert_eq!((after.y, after.timer), (-4896, 9));
}

#[test]
fn a_nonpositive_timer_adds_the_entry_zero_step_scaled_by_the_ticks_plus_two() {
    let input = OrdinaryVerticalInputs::for_falling(0, 1, 0, 8, false).unwrap();
    let after = ordinary_falling_vertical(state(0), 32, input);
    // 32 * 16 * (1 + 2) = 1536, below the 3840 limit; the timer is untouched.
    assert_eq!((after.y, after.timer), (1000 + 1536, 0));
    // A negative signed timer takes the same branch; a large step is limited.
    let input = OrdinaryVerticalInputs::for_falling(200, 1, 0, 8, false).unwrap();
    let after = ordinary_falling_vertical(state(200), 255, input);
    assert_eq!((after.y, after.timer), (1000 + 3840, 200));
}

#[test]
fn model_controlled_vertical_motion_changes_nothing() {
    let input = OrdinaryVerticalInputs::for_falling(10, 1, 0, 8, true).unwrap();
    assert_eq!(ordinary_falling_vertical(state(10), 32, input), state(10));
}
