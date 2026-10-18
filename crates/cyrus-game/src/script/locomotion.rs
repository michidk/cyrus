//! Nonplayer locomotion decisions follow preservation's engine/SOUP.md "MoveBackward".
//! The functions are pure: the caller owns the animation request, probe query and writes.

/// Receiver state read by `MoveBackward`'s admission and animation choice.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackwardState {
    pub rope_attached: bool,
    pub sliding_state: u8,
    pub falling_state: u8,
    pub queued_motion: u8,
    pub living_state: u8,
    pub holding_weapon: bool,
    /// Combat flag `0x04`.
    pub combat_mode: bool,
    /// Actor surface value; 2 and 4 are deep water.
    pub surface: u8,
    pub torch_burning: bool,
    /// Attribute bytes 9 (ordinary) and 18 (combat stance) give the step length.
    pub step_attribute: u8,
    pub combat_step_attribute: u8,
    /// Attribute bytes 83 and 84 give a combat-stance strafe's step length and turn rate.
    pub strafe_step_attribute: u8,
    pub strafe_turn_attribute: u8,
    /// Attribute bytes 8 (ordinary) and 17 (combat stance) give `MoveForward`'s step length.
    pub forward_step_attribute: u8,
    pub forward_combat_step_attribute: u8,
    /// Attribute bytes 12 and 19 give WalkForward ordinary and combat step lengths.
    pub walk_step_attribute: u8,
    pub walk_combat_step_attribute: u8,
    /// A combat move is already selected (the action selector is not -1).
    pub move_selected: bool,
    /// Attribute 11 selects the initial distance ramp, independently of animation suppression.
    pub step_adjustment: u8,
}

/// Outcome of `MoveBackward`'s admission and animation choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackwardAdmission {
    /// The call completes immediately without changing the receiver.
    Refused,
    /// The combat-stance probe was blocked: the call completes after setting motion direction 7.
    Blocked,
    /// Request `group`, then move by `step` whole units.
    Proceed { group: u16, step: u8 },
}

/// Motion-direction byte value `MoveBackward` stores once admitted.
pub const BACKWARD_MOTION_DIRECTION: u8 = 7;

/// Admission and group choice. `blocked_ahead_30` is evaluated only in combat stance, as the
/// original probes only then.
pub fn backward_admission(
    state: BackwardState,
    blocked_ahead_30: impl FnOnce() -> bool,
) -> BackwardAdmission {
    if !idle_admitted(&state) {
        return BackwardAdmission::Refused;
    }
    if state.holding_weapon || state.combat_mode {
        if blocked_ahead_30() {
            return BackwardAdmission::Blocked;
        }
        return BackwardAdmission::Proceed {
            group: 93,
            step: state.combat_step_attribute,
        };
    }
    let group = if matches!(state.surface, 2 | 4) {
        143
    } else if state.torch_burning {
        23
    } else {
        19
    };
    BackwardAdmission::Proceed {
        group,
        step: state.step_attribute,
    }
}

/// The admission shared by every nonplayer stepper: no rope, sliding, falling or queued motion,
/// and a lifecycle byte of 0 or 15.
fn idle_admitted(state: &BackwardState) -> bool {
    !(state.rope_attached
        || state.sliding_state != 0
        || state.falling_state != 0
        || state.queued_motion != 0
        || !matches!(state.living_state, 0 | 15))
}

/// Strafe direction of `MoveLeft` and `MoveRight`, per preservation's engine/SOUP.md.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Strafe {
    Left,
    Right,
}

impl Strafe {
    /// Motion-direction byte stored once admitted.
    pub fn motion_direction(self) -> u8 {
        match self {
            Self::Left => 4,
            Self::Right => 5,
        }
    }

    /// Animation group of the combat-stance strafe.
    pub fn combat_group(self) -> u16 {
        match self {
            Self::Left => 94,
            Self::Right => 95,
        }
    }

    /// Both strafe groups; a receiver already in either completes without moving.
    pub const COMBAT_GROUPS: [u16; 2] = [94, 95];
}

/// Outcome of a strafe's admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafeAdmission {
    /// The call completes immediately without changing the receiver.
    Refused,
    /// Outside combat stance the original runs a lateral model-contact step check, an edge probe
    /// and the hanging-edge test, which are not implemented.
    OutsideCombat,
    /// Combat stance: request `group`, turn by `turn` and move by `step` whole units.
    Combat { group: u16, step: u8, turn: u8 },
}

/// Admission for `MoveLeft` and `MoveRight`: the shared idle admission, then the combat stance.
pub fn strafe_admission(state: BackwardState, strafe: Strafe) -> StrafeAdmission {
    if !idle_admitted(&state) {
        return StrafeAdmission::Refused;
    }
    if state.holding_weapon || state.combat_mode {
        StrafeAdmission::Combat {
            group: strafe.combat_group(),
            step: state.strafe_step_attribute,
            turn: state.strafe_turn_attribute,
        }
    } else {
        StrafeAdmission::OutsideCombat
    }
}

/// One combat-stance strafe step: the receiver's new heading (the left call subtracts the scaled
/// turn rate, the right call adds it) and the translation (movement heading and distance), or
/// `None` when no movement happens. The movement heading is the new heading plus 512 for left and
/// minus 512 for right.
pub fn strafe_motion(
    strafe: Strafe,
    angle_y: u16,
    turn: u8,
    step: u8,
    frame_scale: i32,
    translation_suppressed: bool,
) -> (u16, Option<(u16, i32)>) {
    let delta = i32::from(turn).wrapping_mul(frame_scale) >> 8;
    let signed = match strafe {
        Strafe::Left => -delta,
        Strafe::Right => delta,
    };
    let angle = (i32::from(angle_y).wrapping_add(signed) & 2047) as u16;
    let offset = match strafe {
        Strafe::Left => 512,
        Strafe::Right => -512,
    };
    let heading = (i32::from(angle).wrapping_add(offset) & 2047) as u16;
    let distance = (i32::from(step) << 8).wrapping_mul(frame_scale) >> 8;
    let movement = (!translation_suppressed && distance > 0).then_some((heading, distance));
    (angle, movement)
}

/// Motion-direction byte `MoveForward` stores once admitted.
pub const FORWARD_MOTION_DIRECTION: u8 = 2;

/// Admission of `MoveForward` before its forward step check, per preservation's engine/SOUP.md:
/// no rope, sliding, falling or queued motion, and no lifecycle test.
pub fn forward_admitted(state: &BackwardState) -> bool {
    !(state.rope_attached
        || state.sliding_state != 0
        || state.falling_state != 0
        || state.queued_motion != 0)
}

/// What `MoveForward` does after its step check passed and the direction byte is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardPlan {
    /// Combat stance with a combat move already selected: the call completes without moving.
    Done,
    /// Deep water outside combat stance needs the edge probe, which is not available here.
    NeedsEdgeProbe,
    /// Request `group`, then move by `step` whole units (halved distance in deep water).
    Proceed { group: u16, step: u8 },
}

pub fn forward_plan(state: &BackwardState) -> ForwardPlan {
    if state.holding_weapon || state.combat_mode {
        if state.move_selected {
            return ForwardPlan::Done;
        }
        return ForwardPlan::Proceed {
            group: 92,
            step: state.forward_combat_step_attribute,
        };
    }
    if matches!(state.surface, 2 | 4) {
        return ForwardPlan::NeedsEdgeProbe;
    }
    let group = if state.torch_burning { 30 } else { 18 };
    ForwardPlan::Proceed {
        group,
        step: state.forward_step_attribute,
    }
}

/// One forward step: the heading is the receiver's local Y angle and the distance the step in
/// Q8 units, scaled by the frame scale; `None` when no movement happens.
pub fn forward_translation(
    angle_y: u16,
    distance: i32,
    frame_scale: i32,
    translation_suppressed: bool,
) -> Option<(u16, i32)> {
    if translation_suppressed {
        return None;
    }
    let distance = distance.wrapping_mul(frame_scale) >> 8;
    (distance > 0).then_some((angle_y & 2047, distance))
}

/// One backward step: the heading (local Y angle plus 1024, wrapped) and the distance, or `None`
/// when no movement happens. `distance` is in Q8 units; the translation-suppression marker blocks
/// movement; the scale is the shared frame scale applied with an arithmetic shift right of eight.
pub fn backward_translation(
    angle_y: u16,
    distance: i32,
    frame_scale: i32,
    translation_suppressed: bool,
) -> Option<(u16, i32)> {
    if translation_suppressed {
        return None;
    }
    let heading = angle_y.wrapping_add(1024) & 2047;
    let distance = distance.wrapping_mul(frame_scale) >> 8;
    (distance > 0).then_some((heading, distance))
}

/// Adjust and advance one packed directional counter before frame scaling.
/// `shift` is 0/2/4/6 for left/right/forward/backward. Arithmetic follows
/// preservation's SOUP movement contract; multiplication wraps before shifting.
pub fn initial_step_distance(packed: &mut u8, shift: u8, mode: u8, distance: i32) -> i32 {
    let counter = (*packed >> shift) & 3;
    if counter == 3 {
        return distance;
    }
    *packed = (*packed & !(3 << shift)) | ((counter + 1) << shift);
    match (mode, counter) {
        (1, 0) => distance >> 2,
        (1, 1) => distance / 3,
        (1, 2) => distance >> 1,
        (2, 0) | (3, 0..=1) => 0,
        (2, 1) => distance.wrapping_mul(85) >> 8,
        (2, 2) => distance.wrapping_mul(170) >> 8,
        (3, 2) => distance >> 1,
        _ => distance,
    }
}

/// Clear only directions not called during this ordinary wrapper.
pub fn finish_step_counters(packed: &mut u8, called: u8) {
    for direction in 0..4 {
        if called & (1 << direction) == 0 {
            *packed &= !(3 << (direction * 2));
        }
    }
}

/// The task result: one (continue) while `duration - 1 > elapsed`, zero otherwise.
pub fn continues(duration: i32, elapsed: i32) -> bool {
    duration.wrapping_sub(1) > elapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_distance_ramps_preserve_other_directions_and_stop_at_three() {
        for (mode, expected) in [
            (0, [2048, 2048, 2048, 2048]),
            (1, [512, 682, 1024, 2048]),
            (2, [0, 680, 1360, 2048]),
            (3, [0, 0, 1024, 2048]),
            (255, [2048; 4]),
        ] {
            let mut packed = 0b0000_1111;
            for distance in expected {
                assert_eq!(initial_step_distance(&mut packed, 6, mode, 2048), distance);
                assert_eq!(packed & 63, 15);
            }
            assert_eq!(packed, 207);
        }
        let mut packed = 0;
        assert_eq!(initial_step_distance(&mut packed, 4, 1, -1025), -257);
        assert_eq!(initial_step_distance(&mut packed, 4, 1, -1025), -341);
        assert_eq!(initial_step_distance(&mut packed, 4, 1, -1025), -513);
    }

    #[test]
    fn ordinary_cleanup_retains_only_called_directions() {
        let mut packed = 255;
        finish_step_counters(&mut packed, 0b1010);
        assert_eq!(packed, 0b1100_1100);
        finish_step_counters(&mut packed, 0);
        assert_eq!(packed, 0);
    }

    fn ready() -> BackwardState {
        BackwardState {
            step_attribute: 8,
            combat_step_attribute: 18,
            ..Default::default()
        }
    }

    #[test]
    fn admission_is_refused_by_any_motion_state_or_a_nonidle_lifecycle() {
        let never = || -> bool { panic!("probe must not run") };
        for state in [
            BackwardState {
                rope_attached: true,
                ..ready()
            },
            BackwardState {
                sliding_state: 1,
                ..ready()
            },
            BackwardState {
                falling_state: 1,
                ..ready()
            },
            BackwardState {
                queued_motion: 24,
                ..ready()
            },
            BackwardState {
                living_state: 20,
                ..ready()
            },
        ] {
            assert_eq!(backward_admission(state, never), BackwardAdmission::Refused);
        }
        for living_state in [0, 15] {
            assert!(matches!(
                backward_admission(
                    BackwardState {
                        living_state,
                        ..ready()
                    },
                    never
                ),
                BackwardAdmission::Proceed { .. }
            ));
        }
    }

    #[test]
    fn groups_follow_stance_water_and_torch() {
        let group = |state: BackwardState| match backward_admission(state, || false) {
            BackwardAdmission::Proceed { group, step } => (group, step),
            other => panic!("{other:?}"),
        };
        assert_eq!(group(ready()), (19, 8));
        assert_eq!(
            group(BackwardState {
                torch_burning: true,
                ..ready()
            }),
            (23, 8)
        );
        assert_eq!(
            group(BackwardState {
                surface: 2,
                ..ready()
            }),
            (143, 8)
        );
        assert_eq!(
            group(BackwardState {
                surface: 4,
                torch_burning: true,
                ..ready()
            }),
            (143, 8)
        );
        assert_eq!(
            group(BackwardState {
                holding_weapon: true,
                ..ready()
            }),
            (93, 18)
        );
        assert_eq!(
            group(BackwardState {
                combat_mode: true,
                surface: 2,
                ..ready()
            }),
            (93, 18)
        );
    }

    #[test]
    fn a_blocked_combat_probe_completes_without_a_request() {
        let state = BackwardState {
            combat_mode: true,
            ..ready()
        };
        assert_eq!(
            backward_admission(state, || true),
            BackwardAdmission::Blocked
        );
        // Outside combat stance the probe never runs.
        assert!(matches!(
            backward_admission(ready(), || panic!("no probe outside combat")),
            BackwardAdmission::Proceed { .. }
        ));
    }

    #[test]
    fn translation_scales_the_step_and_wraps_the_heading() {
        assert_eq!(
            backward_translation(0, 2048, 256, false),
            Some((1024, 2048))
        );
        assert_eq!(
            backward_translation(1500, 2048, 128, false),
            Some((476, 1024))
        );
        assert_eq!(backward_translation(0, 2048, 256, true), None);
        assert_eq!(backward_translation(0, 0, 256, false), None);
        assert_eq!(backward_translation(0, 2048, 0, false), None);
    }

    #[test]
    fn the_task_continues_until_the_final_elapsed_tick() {
        assert!(continues(3, 0));
        assert!(continues(3, 1));
        assert!(!continues(3, 2));
        assert!(!continues(0, 0));
    }

    #[test]
    fn strafes_share_the_idle_admission_and_need_combat_stance_here() {
        let combat = BackwardState {
            combat_mode: true,
            strafe_step_attribute: 6,
            strafe_turn_attribute: 4,
            ..ready()
        };
        assert_eq!(
            strafe_admission(combat, Strafe::Left),
            StrafeAdmission::Combat {
                group: 94,
                step: 6,
                turn: 4
            }
        );
        assert_eq!(
            strafe_admission(
                BackwardState {
                    holding_weapon: true,
                    ..combat
                },
                Strafe::Right
            ),
            StrafeAdmission::Combat {
                group: 95,
                step: 6,
                turn: 4
            }
        );
        assert_eq!(
            strafe_admission(ready(), Strafe::Left),
            StrafeAdmission::OutsideCombat
        );
        for refused in [
            BackwardState {
                rope_attached: true,
                ..combat
            },
            BackwardState {
                queued_motion: 1,
                ..combat
            },
            BackwardState {
                living_state: 20,
                ..combat
            },
        ] {
            assert_eq!(
                strafe_admission(refused, Strafe::Left),
                StrafeAdmission::Refused
            );
        }
    }

    #[test]
    fn strafe_turns_by_the_scaled_rate_and_moves_a_quarter_turn_off_the_new_heading() {
        // Left: heading 100 minus (4 * 256 >> 8 = 4) = 96; movement heading 96 + 512.
        assert_eq!(
            strafe_motion(Strafe::Left, 100, 4, 6, 256, false),
            (96, Some((608, 1536)))
        );
        // Right: heading 100 plus 4 = 104; movement heading 104 - 512 wraps to 1640.
        assert_eq!(
            strafe_motion(Strafe::Right, 100, 4, 6, 256, false),
            (104, Some((1640, 1536)))
        );
        // The turn wraps, and suppression or a zero distance only removes the movement.
        assert_eq!(
            strafe_motion(Strafe::Left, 2, 8, 6, 256, true),
            (2042, None)
        );
        assert_eq!(
            strafe_motion(Strafe::Right, 2047, 4, 0, 256, false),
            (3, None)
        );
    }

    #[test]
    fn forward_admission_has_no_lifecycle_test_and_stance_picks_the_group() {
        let base = BackwardState {
            forward_step_attribute: 8,
            forward_combat_step_attribute: 17,
            ..Default::default()
        };
        assert!(forward_admitted(&BackwardState {
            living_state: 20,
            ..base
        }));
        for refused in [
            BackwardState {
                rope_attached: true,
                ..base
            },
            BackwardState {
                sliding_state: 1,
                ..base
            },
            BackwardState {
                falling_state: 1,
                ..base
            },
            BackwardState {
                queued_motion: 8,
                ..base
            },
        ] {
            assert!(!forward_admitted(&refused));
        }
        assert_eq!(
            forward_plan(&base),
            ForwardPlan::Proceed { group: 18, step: 8 }
        );
        assert_eq!(
            forward_plan(&BackwardState {
                torch_burning: true,
                ..base
            }),
            ForwardPlan::Proceed { group: 30, step: 8 }
        );
        assert_eq!(
            forward_plan(&BackwardState { surface: 2, ..base }),
            ForwardPlan::NeedsEdgeProbe
        );
        let combat = BackwardState {
            combat_mode: true,
            ..base
        };
        assert_eq!(
            forward_plan(&combat),
            ForwardPlan::Proceed {
                group: 92,
                step: 17
            }
        );
        assert_eq!(
            forward_plan(&BackwardState {
                move_selected: true,
                ..combat
            }),
            ForwardPlan::Done
        );
        // A deep-water combat actor still takes the combat branch.
        assert_eq!(
            forward_plan(&BackwardState {
                surface: 4,
                ..combat
            }),
            ForwardPlan::Proceed {
                group: 92,
                step: 17
            }
        );
    }

    #[test]
    fn forward_translation_uses_the_unoffset_heading() {
        assert_eq!(
            forward_translation(300, 2048, 256, false),
            Some((300, 2048))
        );
        assert_eq!(
            forward_translation(2100, 2048, 128, false),
            Some((52, 1024))
        );
        assert_eq!(forward_translation(0, 2048, 256, true), None);
        assert_eq!(forward_translation(0, 0, 256, false), None);
    }
}
