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
    /// Attribute byte 10 gives an ordinary (outside combat stance) strafe's step length.
    pub lateral_step_attribute: u8,
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

    /// Animation group of the ordinary (outside combat stance, dry) strafe.
    pub fn ordinary_group(self, torch_burning: bool) -> u16 {
        match (self, torch_burning) {
            (Self::Left, false) => 24,
            (Self::Left, true) => 25,
            (Self::Right, false) => 26,
            (Self::Right, true) => 27,
        }
    }

    /// Both ordinary groups of this direction, which end the wait for the requested group.
    pub fn ordinary_groups(self) -> [u16; 2] {
        [self.ordinary_group(false), self.ordinary_group(true)]
    }

    /// Bit of the shared direction-called flags the stepper sets once it moves.
    pub fn called_flag(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
        }
    }

    /// Shift of this direction's packed step counter.
    pub fn counter_shift(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 2,
        }
    }
}

/// Outcome of a strafe's admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafeAdmission {
    /// The call completes immediately without changing the receiver.
    Refused,
    /// Deep water (edge probe) or hanging at an edge: not implemented.
    Unsupported,
    /// Outside combat stance, on dry ground: after the lateral step check passes, request `group`
    /// and move by the attribute byte 10 step.
    OutsideCombat { group: u16 },
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
    } else if state.living_state == 15 || matches!(state.surface, 2 | 4) {
        StrafeAdmission::Unsupported
    } else {
        StrafeAdmission::OutsideCombat {
            group: strafe.ordinary_group(state.torch_burning),
        }
    }
}

/// One combat-stance strafe step: the receiver's new heading (the left call subtracts the scaled
/// turn rate, the right call adds it) and the translation (movement heading and distance), or
/// `None` when no movement happens. The movement heading is the new heading plus 512 for left and
/// minus 512 for right. `distance` is the Q8 step after the packed step counter's adjustment.
pub fn strafe_motion(
    strafe: Strafe,
    angle_y: u16,
    turn: u8,
    distance: i32,
    frame_scale: i32,
    translation_suppressed: bool,
) -> (u16, Option<(u16, i32)>) {
    let delta = i32::from(turn).wrapping_mul(frame_scale) >> 8;
    let signed = match strafe {
        Strafe::Left => -delta,
        Strafe::Right => delta,
    };
    let angle = (i32::from(angle_y).wrapping_add(signed) & 2047) as u16;
    let movement = strafe_translation(strafe, angle, distance, frame_scale, translation_suppressed);
    (angle, movement)
}

/// One ordinary strafe step: the movement heading (the receiver's heading plus 512 for left and
/// minus 512 for right, wrapped) and the Q8 `distance` scaled by the frame scale, or `None` when
/// the translation-suppression marker or a nonpositive distance means no movement. The ordinary
/// strafe never turns the receiver.
pub fn strafe_translation(
    strafe: Strafe,
    angle_y: u16,
    distance: i32,
    frame_scale: i32,
    translation_suppressed: bool,
) -> Option<(u16, i32)> {
    if translation_suppressed {
        return None;
    }
    let offset = match strafe {
        Strafe::Left => 512,
        Strafe::Right => -512,
    };
    let heading = (i32::from(angle_y).wrapping_add(offset) & 2047) as u16;
    let distance = distance.wrapping_mul(frame_scale) >> 8;
    (distance > 0).then_some((heading, distance))
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
            StrafeAdmission::OutsideCombat { group: 24 }
        );
        assert_eq!(
            strafe_admission(
                BackwardState {
                    torch_burning: true,
                    ..ready()
                },
                Strafe::Right
            ),
            StrafeAdmission::OutsideCombat { group: 27 }
        );
        for unsupported in [
            BackwardState {
                living_state: 15,
                ..ready()
            },
            BackwardState {
                surface: 2,
                ..ready()
            },
            BackwardState {
                surface: 4,
                ..ready()
            },
        ] {
            assert_eq!(
                strafe_admission(unsupported, Strafe::Right),
                StrafeAdmission::Unsupported
            );
        }
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

    /// Rows run through the original step-ramp routine over synthetic actors (research oracle, not
    /// installed data): `(attribute 11, counter, distance, result)`.
    #[test]
    fn the_step_ramp_matches_the_original_routine() {
        let rows: &[(u8, u8, i32, i32)] = &[
            (0, 0, 0, 0),
            (0, 0, 1, 1),
            (0, 0, 7, 7),
            (0, 0, 256, 256),
            (0, 0, 1024, 1024),
            (0, 0, 2048, 2048),
            (0, 0, 3000, 3000),
            (0, 0, 65536, 65536),
            (0, 1, 0, 0),
            (0, 1, 1, 1),
            (0, 1, 7, 7),
            (0, 1, 256, 256),
            (0, 1, 1024, 1024),
            (0, 1, 2048, 2048),
            (0, 1, 3000, 3000),
            (0, 1, 65536, 65536),
            (0, 2, 0, 0),
            (0, 2, 1, 1),
            (0, 2, 7, 7),
            (0, 2, 256, 256),
            (0, 2, 1024, 1024),
            (0, 2, 2048, 2048),
            (0, 2, 3000, 3000),
            (0, 2, 65536, 65536),
            (1, 0, 0, 0),
            (1, 0, 1, 0),
            (1, 0, 7, 1),
            (1, 0, 256, 64),
            (1, 0, 1024, 256),
            (1, 0, 2048, 512),
            (1, 0, 3000, 750),
            (1, 0, 65536, 16384),
            (1, 1, 0, 0),
            (1, 1, 1, 0),
            (1, 1, 7, 2),
            (1, 1, 256, 85),
            (1, 1, 1024, 341),
            (1, 1, 2048, 682),
            (1, 1, 3000, 1000),
            (1, 1, 65536, 21845),
            (1, 2, 0, 0),
            (1, 2, 1, 0),
            (1, 2, 7, 3),
            (1, 2, 256, 128),
            (1, 2, 1024, 512),
            (1, 2, 2048, 1024),
            (1, 2, 3000, 1500),
            (1, 2, 65536, 32768),
            (2, 0, 0, 0),
            (2, 0, 1, 0),
            (2, 0, 7, 0),
            (2, 0, 256, 0),
            (2, 0, 1024, 0),
            (2, 0, 2048, 0),
            (2, 0, 3000, 0),
            (2, 0, 65536, 0),
            (2, 1, 0, 0),
            (2, 1, 1, 0),
            (2, 1, 7, 2),
            (2, 1, 256, 85),
            (2, 1, 1024, 340),
            (2, 1, 2048, 680),
            (2, 1, 3000, 996),
            (2, 1, 65536, 21760),
            (2, 2, 0, 0),
            (2, 2, 1, 0),
            (2, 2, 7, 4),
            (2, 2, 256, 170),
            (2, 2, 1024, 680),
            (2, 2, 2048, 1360),
            (2, 2, 3000, 1992),
            (2, 2, 65536, 43520),
            (3, 0, 0, 0),
            (3, 0, 1, 0),
            (3, 0, 7, 0),
            (3, 0, 256, 0),
            (3, 0, 1024, 0),
            (3, 0, 2048, 0),
            (3, 0, 3000, 0),
            (3, 0, 65536, 0),
            (3, 1, 0, 0),
            (3, 1, 1, 0),
            (3, 1, 7, 0),
            (3, 1, 256, 0),
            (3, 1, 1024, 0),
            (3, 1, 2048, 0),
            (3, 1, 3000, 0),
            (3, 1, 65536, 0),
            (3, 2, 0, 0),
            (3, 2, 1, 0),
            (3, 2, 7, 3),
            (3, 2, 256, 128),
            (3, 2, 1024, 512),
            (3, 2, 2048, 1024),
            (3, 2, 3000, 1500),
            (3, 2, 65536, 32768),
            (4, 0, 0, 0),
            (4, 0, 1, 1),
            (4, 0, 7, 7),
            (4, 0, 256, 256),
            (4, 0, 1024, 1024),
            (4, 0, 2048, 2048),
            (4, 0, 3000, 3000),
            (4, 0, 65536, 65536),
            (4, 1, 0, 0),
            (4, 1, 1, 1),
            (4, 1, 7, 7),
            (4, 1, 256, 256),
            (4, 1, 1024, 1024),
            (4, 1, 2048, 2048),
            (4, 1, 3000, 3000),
            (4, 1, 65536, 65536),
            (4, 2, 0, 0),
            (4, 2, 1, 1),
            (4, 2, 7, 7),
            (4, 2, 256, 256),
            (4, 2, 1024, 1024),
            (4, 2, 2048, 2048),
            (4, 2, 3000, 3000),
            (4, 2, 65536, 65536),
            (5, 0, 0, 0),
            (5, 0, 1, 1),
            (5, 0, 7, 7),
            (5, 0, 256, 256),
            (5, 0, 1024, 1024),
            (5, 0, 2048, 2048),
            (5, 0, 3000, 3000),
            (5, 0, 65536, 65536),
            (5, 1, 0, 0),
            (5, 1, 1, 1),
            (5, 1, 7, 7),
            (5, 1, 256, 256),
            (5, 1, 1024, 1024),
            (5, 1, 2048, 2048),
            (5, 1, 3000, 3000),
            (5, 1, 65536, 65536),
            (5, 2, 0, 0),
            (5, 2, 1, 1),
            (5, 2, 7, 7),
            (5, 2, 256, 256),
            (5, 2, 1024, 1024),
            (5, 2, 2048, 2048),
            (5, 2, 3000, 3000),
            (5, 2, 65536, 65536),
        ];
        for &(mode, counter, distance, expected) in rows {
            let mut packed = counter;
            assert_eq!(
                initial_step_distance(&mut packed, 0, mode, distance),
                expected,
                "mode {mode} counter {counter} distance {distance}"
            );
            let mut packed = counter << 6;
            assert_eq!(
                initial_step_distance(&mut packed, 6, mode, distance),
                expected,
                "shifted: mode {mode} counter {counter} distance {distance}"
            );
        }
    }

    /// Rows run through the original `MoveForward`, `MoveBackward`, `MoveLeft` and `MoveRight`
    /// routines (duration 1) over synthetic actors with the published rotation table (research
    /// oracle): `(stepper, heading, step attribute, attribute 11, counter, scale, dx, dz,
    /// packed counter after)`. Steppers are 0 forward, 1 backward, 2 left, 3 right.
    #[test]
    fn the_steppers_match_the_original_routines() {
        type Row = (u8, u16, u8, u8, u8, i32, i32, i32, u8);
        let rows: &[Row] = &[
            (0, 0, 8, 0, 0, 256, 0, 2048, 16),
            (0, 0, 8, 0, 0, 128, 0, 1024, 16),
            (0, 0, 8, 0, 0, 100, 0, 800, 16),
            (0, 0, 8, 0, 1, 256, 0, 2048, 32),
            (0, 0, 8, 0, 1, 128, 0, 1024, 32),
            (0, 0, 8, 0, 1, 100, 0, 800, 32),
            (0, 0, 8, 0, 2, 256, 0, 2048, 48),
            (0, 0, 8, 0, 2, 128, 0, 1024, 48),
            (0, 0, 8, 0, 2, 100, 0, 800, 48),
            (0, 0, 8, 1, 0, 256, 0, 512, 16),
            (0, 0, 8, 1, 0, 128, 0, 256, 16),
            (0, 0, 8, 1, 0, 100, 0, 200, 16),
            (0, 0, 8, 1, 1, 256, 0, 682, 32),
            (0, 0, 8, 1, 1, 128, 0, 341, 32),
            (0, 0, 8, 1, 1, 100, 0, 266, 32),
            (0, 0, 8, 1, 2, 256, 0, 1024, 48),
            (0, 0, 8, 1, 2, 128, 0, 512, 48),
            (0, 0, 8, 1, 2, 100, 0, 400, 48),
            (0, 0, 8, 2, 0, 256, 0, 0, 16),
            (0, 0, 8, 2, 0, 128, 0, 0, 16),
            (0, 0, 8, 2, 0, 100, 0, 0, 16),
            (0, 0, 8, 2, 1, 256, 0, 680, 32),
            (0, 0, 8, 2, 1, 128, 0, 340, 32),
            (0, 0, 8, 2, 1, 100, 0, 265, 32),
            (0, 0, 8, 2, 2, 256, 0, 1360, 48),
            (0, 0, 8, 2, 2, 128, 0, 680, 48),
            (0, 0, 8, 2, 2, 100, 0, 531, 48),
            (0, 0, 8, 3, 0, 256, 0, 0, 16),
            (0, 0, 8, 3, 0, 128, 0, 0, 16),
            (0, 0, 8, 3, 0, 100, 0, 0, 16),
            (0, 0, 8, 3, 1, 256, 0, 0, 32),
            (0, 0, 8, 3, 1, 128, 0, 0, 32),
            (0, 0, 8, 3, 1, 100, 0, 0, 32),
            (0, 0, 8, 3, 2, 256, 0, 1024, 48),
            (0, 0, 8, 3, 2, 128, 0, 512, 48),
            (0, 0, 8, 3, 2, 100, 0, 400, 48),
            (0, 0, 24, 0, 0, 256, 0, 6144, 16),
            (0, 0, 24, 0, 0, 128, 0, 3072, 16),
            (0, 0, 24, 0, 0, 100, 0, 2400, 16),
            (0, 0, 24, 0, 1, 256, 0, 6144, 32),
            (0, 0, 24, 0, 1, 128, 0, 3072, 32),
            (0, 0, 24, 0, 1, 100, 0, 2400, 32),
            (0, 0, 24, 0, 2, 256, 0, 6144, 48),
            (0, 0, 24, 0, 2, 128, 0, 3072, 48),
            (0, 0, 24, 0, 2, 100, 0, 2400, 48),
            (0, 0, 24, 1, 0, 256, 0, 1536, 16),
            (0, 0, 24, 1, 0, 128, 0, 768, 16),
            (0, 0, 24, 1, 0, 100, 0, 600, 16),
            (0, 0, 24, 1, 1, 256, 0, 2048, 32),
            (0, 0, 24, 1, 1, 128, 0, 1024, 32),
            (0, 0, 24, 1, 1, 100, 0, 800, 32),
            (0, 0, 24, 1, 2, 256, 0, 3072, 48),
            (0, 0, 24, 1, 2, 128, 0, 1536, 48),
            (0, 0, 24, 1, 2, 100, 0, 1200, 48),
            (0, 0, 24, 2, 0, 256, 0, 0, 16),
            (0, 0, 24, 2, 0, 128, 0, 0, 16),
            (0, 0, 24, 2, 0, 100, 0, 0, 16),
            (0, 0, 24, 2, 1, 256, 0, 2040, 32),
            (0, 0, 24, 2, 1, 128, 0, 1020, 32),
            (0, 0, 24, 2, 1, 100, 0, 796, 32),
            (0, 0, 24, 2, 2, 256, 0, 4080, 48),
            (0, 0, 24, 2, 2, 128, 0, 2040, 48),
            (0, 0, 24, 2, 2, 100, 0, 1593, 48),
            (0, 0, 24, 3, 0, 256, 0, 0, 16),
            (0, 0, 24, 3, 0, 128, 0, 0, 16),
            (0, 0, 24, 3, 0, 100, 0, 0, 16),
            (0, 0, 24, 3, 1, 256, 0, 0, 32),
            (0, 0, 24, 3, 1, 128, 0, 0, 32),
            (0, 0, 24, 3, 1, 100, 0, 0, 32),
            (0, 0, 24, 3, 2, 256, 0, 3072, 48),
            (0, 0, 24, 3, 2, 128, 0, 1536, 48),
            (0, 0, 24, 3, 2, 100, 0, 1200, 48),
            (0, 100, 8, 0, 0, 256, -618, 1952, 16),
            (0, 100, 8, 0, 0, 128, -309, 976, 16),
            (0, 100, 8, 0, 0, 100, -241, 762, 16),
            (0, 100, 8, 0, 1, 256, -618, 1952, 32),
            (0, 100, 8, 0, 1, 128, -309, 976, 32),
            (0, 100, 8, 0, 1, 100, -241, 762, 32),
            (0, 100, 8, 0, 2, 256, -618, 1952, 48),
            (0, 100, 8, 0, 2, 128, -309, 976, 48),
            (0, 100, 8, 0, 2, 100, -241, 762, 48),
            (0, 100, 8, 1, 0, 256, -154, 488, 16),
            (0, 100, 8, 1, 0, 128, -77, 244, 16),
            (0, 100, 8, 1, 0, 100, -60, 190, 16),
            (0, 100, 8, 1, 1, 256, -205, 650, 32),
            (0, 100, 8, 1, 1, 128, -102, 325, 32),
            (0, 100, 8, 1, 1, 100, -80, 253, 32),
            (0, 100, 8, 1, 2, 256, -309, 976, 48),
            (0, 100, 8, 1, 2, 128, -154, 488, 48),
            (0, 100, 8, 1, 2, 100, -120, 381, 48),
            (0, 100, 8, 2, 0, 256, 0, 0, 16),
            (0, 100, 8, 2, 0, 128, 0, 0, 16),
            (0, 100, 8, 2, 0, 100, 0, 0, 16),
            (0, 100, 8, 2, 1, 256, -205, 648, 32),
            (0, 100, 8, 2, 1, 128, -102, 324, 32),
            (0, 100, 8, 2, 1, 100, -80, 252, 32),
            (0, 100, 8, 2, 2, 256, -410, 1296, 48),
            (0, 100, 8, 2, 2, 128, -205, 648, 48),
            (0, 100, 8, 2, 2, 100, -160, 506, 48),
            (0, 100, 8, 3, 0, 256, 0, 0, 16),
            (0, 100, 8, 3, 0, 128, 0, 0, 16),
            (0, 100, 8, 3, 0, 100, 0, 0, 16),
            (0, 100, 8, 3, 1, 256, 0, 0, 32),
            (0, 100, 8, 3, 1, 128, 0, 0, 32),
            (0, 100, 8, 3, 1, 100, 0, 0, 32),
            (0, 100, 8, 3, 2, 256, -309, 976, 48),
            (0, 100, 8, 3, 2, 128, -154, 488, 48),
            (0, 100, 8, 3, 2, 100, -120, 381, 48),
            (0, 100, 24, 0, 0, 256, -1855, 5857, 16),
            (0, 100, 24, 0, 0, 128, -927, 2928, 16),
            (0, 100, 24, 0, 0, 100, -724, 2287, 16),
            (0, 100, 24, 0, 1, 256, -1855, 5857, 32),
            (0, 100, 24, 0, 1, 128, -927, 2928, 32),
            (0, 100, 24, 0, 1, 100, -724, 2287, 32),
            (0, 100, 24, 0, 2, 256, -1855, 5857, 48),
            (0, 100, 24, 0, 2, 128, -927, 2928, 48),
            (0, 100, 24, 0, 2, 100, -724, 2287, 48),
            (0, 100, 24, 1, 0, 256, -463, 1464, 16),
            (0, 100, 24, 1, 0, 128, -231, 732, 16),
            (0, 100, 24, 1, 0, 100, -181, 571, 16),
            (0, 100, 24, 1, 1, 256, -618, 1952, 32),
            (0, 100, 24, 1, 1, 128, -309, 976, 32),
            (0, 100, 24, 1, 1, 100, -241, 762, 32),
            (0, 100, 24, 1, 2, 256, -927, 2928, 48),
            (0, 100, 24, 1, 2, 128, -463, 1464, 48),
            (0, 100, 24, 1, 2, 100, -362, 1143, 48),
            (0, 100, 24, 2, 0, 256, 0, 0, 16),
            (0, 100, 24, 2, 0, 128, 0, 0, 16),
            (0, 100, 24, 2, 0, 100, 0, 0, 16),
            (0, 100, 24, 2, 1, 256, -616, 1944, 32),
            (0, 100, 24, 2, 1, 128, -308, 972, 32),
            (0, 100, 24, 2, 1, 100, -240, 758, 32),
            (0, 100, 24, 2, 2, 256, -1232, 3889, 48),
            (0, 100, 24, 2, 2, 128, -616, 1944, 48),
            (0, 100, 24, 2, 2, 100, -481, 1518, 48),
            (0, 100, 24, 3, 0, 256, 0, 0, 16),
            (0, 100, 24, 3, 0, 128, 0, 0, 16),
            (0, 100, 24, 3, 0, 100, 0, 0, 16),
            (0, 100, 24, 3, 1, 256, 0, 0, 32),
            (0, 100, 24, 3, 1, 128, 0, 0, 32),
            (0, 100, 24, 3, 1, 100, 0, 0, 32),
            (0, 100, 24, 3, 2, 256, -927, 2928, 48),
            (0, 100, 24, 3, 2, 128, -463, 1464, 48),
            (0, 100, 24, 3, 2, 100, -362, 1143, 48),
            (0, 1500, 8, 0, 0, 256, 2035, -225, 16),
            (0, 1500, 8, 0, 0, 128, 1017, -112, 16),
            (0, 1500, 8, 0, 0, 100, 795, -88, 16),
            (0, 1500, 8, 0, 1, 256, 2035, -225, 32),
            (0, 1500, 8, 0, 1, 128, 1017, -112, 32),
            (0, 1500, 8, 0, 1, 100, 795, -88, 32),
            (0, 1500, 8, 0, 2, 256, 2035, -225, 48),
            (0, 1500, 8, 0, 2, 128, 1017, -112, 48),
            (0, 1500, 8, 0, 2, 100, 795, -88, 48),
            (0, 1500, 8, 1, 0, 256, 508, -56, 16),
            (0, 1500, 8, 1, 0, 128, 254, -28, 16),
            (0, 1500, 8, 1, 0, 100, 198, -22, 16),
            (0, 1500, 8, 1, 1, 256, 677, -75, 32),
            (0, 1500, 8, 1, 1, 128, 338, -37, 32),
            (0, 1500, 8, 1, 1, 100, 264, -29, 32),
            (0, 1500, 8, 1, 2, 256, 1017, -112, 48),
            (0, 1500, 8, 1, 2, 128, 508, -56, 48),
            (0, 1500, 8, 1, 2, 100, 397, -44, 48),
            (0, 1500, 8, 2, 0, 256, 0, 0, 16),
            (0, 1500, 8, 2, 0, 128, 0, 0, 16),
            (0, 1500, 8, 2, 0, 100, 0, 0, 16),
            (0, 1500, 8, 2, 1, 256, 675, -74, 32),
            (0, 1500, 8, 2, 1, 128, 337, -37, 32),
            (0, 1500, 8, 2, 1, 100, 263, -29, 32),
            (0, 1500, 8, 2, 2, 256, 1351, -149, 48),
            (0, 1500, 8, 2, 2, 128, 675, -74, 48),
            (0, 1500, 8, 2, 2, 100, 527, -58, 48),
            (0, 1500, 8, 3, 0, 256, 0, 0, 16),
            (0, 1500, 8, 3, 0, 128, 0, 0, 16),
            (0, 1500, 8, 3, 0, 100, 0, 0, 16),
            (0, 1500, 8, 3, 1, 256, 0, 0, 32),
            (0, 1500, 8, 3, 1, 128, 0, 0, 32),
            (0, 1500, 8, 3, 1, 100, 0, 0, 32),
            (0, 1500, 8, 3, 2, 256, 1017, -112, 48),
            (0, 1500, 8, 3, 2, 128, 508, -56, 48),
            (0, 1500, 8, 3, 2, 100, 397, -44, 48),
            (0, 1500, 24, 0, 0, 256, 6106, -677, 16),
            (0, 1500, 24, 0, 0, 128, 3053, -338, 16),
            (0, 1500, 24, 0, 0, 100, 2385, -264, 16),
            (0, 1500, 24, 0, 1, 256, 6106, -677, 32),
            (0, 1500, 24, 0, 1, 128, 3053, -338, 32),
            (0, 1500, 24, 0, 1, 100, 2385, -264, 32),
            (0, 1500, 24, 0, 2, 256, 6106, -677, 48),
            (0, 1500, 24, 0, 2, 128, 3053, -338, 48),
            (0, 1500, 24, 0, 2, 100, 2385, -264, 48),
            (0, 1500, 24, 1, 0, 256, 1526, -169, 16),
            (0, 1500, 24, 1, 0, 128, 763, -84, 16),
            (0, 1500, 24, 1, 0, 100, 596, -66, 16),
            (0, 1500, 24, 1, 1, 256, 2035, -225, 32),
            (0, 1500, 24, 1, 1, 128, 1017, -112, 32),
            (0, 1500, 24, 1, 1, 100, 795, -88, 32),
            (0, 1500, 24, 1, 2, 256, 3053, -338, 48),
            (0, 1500, 24, 1, 2, 128, 1526, -169, 48),
            (0, 1500, 24, 1, 2, 100, 1192, -132, 48),
            (0, 1500, 24, 2, 0, 256, 0, 0, 16),
            (0, 1500, 24, 2, 0, 128, 0, 0, 16),
            (0, 1500, 24, 2, 0, 100, 0, 0, 16),
            (0, 1500, 24, 2, 1, 256, 2027, -224, 32),
            (0, 1500, 24, 2, 1, 128, 1013, -112, 32),
            (0, 1500, 24, 2, 1, 100, 791, -87, 32),
            (0, 1500, 24, 2, 2, 256, 4055, -449, 48),
            (0, 1500, 24, 2, 2, 128, 2027, -224, 48),
            (0, 1500, 24, 2, 2, 100, 1583, -175, 48),
            (0, 1500, 24, 3, 0, 256, 0, 0, 16),
            (0, 1500, 24, 3, 0, 128, 0, 0, 16),
            (0, 1500, 24, 3, 0, 100, 0, 0, 16),
            (0, 1500, 24, 3, 1, 256, 0, 0, 32),
            (0, 1500, 24, 3, 1, 128, 0, 0, 32),
            (0, 1500, 24, 3, 1, 100, 0, 0, 32),
            (0, 1500, 24, 3, 2, 256, 3053, -338, 48),
            (0, 1500, 24, 3, 2, 128, 1526, -169, 48),
            (0, 1500, 24, 3, 2, 100, 1192, -132, 48),
            (1, 0, 8, 0, 0, 256, 0, -2048, 64),
            (1, 0, 8, 0, 0, 128, 0, -1024, 64),
            (1, 0, 8, 0, 0, 100, 0, -800, 64),
            (1, 0, 8, 0, 1, 256, 0, -2048, 128),
            (1, 0, 8, 0, 1, 128, 0, -1024, 128),
            (1, 0, 8, 0, 1, 100, 0, -800, 128),
            (1, 0, 8, 0, 2, 256, 0, -2048, 192),
            (1, 0, 8, 0, 2, 128, 0, -1024, 192),
            (1, 0, 8, 0, 2, 100, 0, -800, 192),
            (1, 0, 8, 1, 0, 256, 0, -512, 64),
            (1, 0, 8, 1, 0, 128, 0, -256, 64),
            (1, 0, 8, 1, 0, 100, 0, -200, 64),
            (1, 0, 8, 1, 1, 256, 0, -682, 128),
            (1, 0, 8, 1, 1, 128, 0, -341, 128),
            (1, 0, 8, 1, 1, 100, 0, -266, 128),
            (1, 0, 8, 1, 2, 256, 0, -1024, 192),
            (1, 0, 8, 1, 2, 128, 0, -512, 192),
            (1, 0, 8, 1, 2, 100, 0, -400, 192),
            (1, 0, 8, 2, 0, 256, 0, 0, 64),
            (1, 0, 8, 2, 0, 128, 0, 0, 64),
            (1, 0, 8, 2, 0, 100, 0, 0, 64),
            (1, 0, 8, 2, 1, 256, 0, -680, 128),
            (1, 0, 8, 2, 1, 128, 0, -340, 128),
            (1, 0, 8, 2, 1, 100, 0, -265, 128),
            (1, 0, 8, 2, 2, 256, 0, -1360, 192),
            (1, 0, 8, 2, 2, 128, 0, -680, 192),
            (1, 0, 8, 2, 2, 100, 0, -531, 192),
            (1, 0, 8, 3, 0, 256, 0, 0, 64),
            (1, 0, 8, 3, 0, 128, 0, 0, 64),
            (1, 0, 8, 3, 0, 100, 0, 0, 64),
            (1, 0, 8, 3, 1, 256, 0, 0, 128),
            (1, 0, 8, 3, 1, 128, 0, 0, 128),
            (1, 0, 8, 3, 1, 100, 0, 0, 128),
            (1, 0, 8, 3, 2, 256, 0, -1024, 192),
            (1, 0, 8, 3, 2, 128, 0, -512, 192),
            (1, 0, 8, 3, 2, 100, 0, -400, 192),
            (1, 0, 24, 0, 0, 256, 0, -6144, 64),
            (1, 0, 24, 0, 0, 128, 0, -3072, 64),
            (1, 0, 24, 0, 0, 100, 0, -2400, 64),
            (1, 0, 24, 0, 1, 256, 0, -6144, 128),
            (1, 0, 24, 0, 1, 128, 0, -3072, 128),
            (1, 0, 24, 0, 1, 100, 0, -2400, 128),
            (1, 0, 24, 0, 2, 256, 0, -6144, 192),
            (1, 0, 24, 0, 2, 128, 0, -3072, 192),
            (1, 0, 24, 0, 2, 100, 0, -2400, 192),
            (1, 0, 24, 1, 0, 256, 0, -1536, 64),
            (1, 0, 24, 1, 0, 128, 0, -768, 64),
            (1, 0, 24, 1, 0, 100, 0, -600, 64),
            (1, 0, 24, 1, 1, 256, 0, -2048, 128),
            (1, 0, 24, 1, 1, 128, 0, -1024, 128),
            (1, 0, 24, 1, 1, 100, 0, -800, 128),
            (1, 0, 24, 1, 2, 256, 0, -3072, 192),
            (1, 0, 24, 1, 2, 128, 0, -1536, 192),
            (1, 0, 24, 1, 2, 100, 0, -1200, 192),
            (1, 0, 24, 2, 0, 256, 0, 0, 64),
            (1, 0, 24, 2, 0, 128, 0, 0, 64),
            (1, 0, 24, 2, 0, 100, 0, 0, 64),
            (1, 0, 24, 2, 1, 256, 0, -2040, 128),
            (1, 0, 24, 2, 1, 128, 0, -1020, 128),
            (1, 0, 24, 2, 1, 100, 0, -796, 128),
            (1, 0, 24, 2, 2, 256, 0, -4080, 192),
            (1, 0, 24, 2, 2, 128, 0, -2040, 192),
            (1, 0, 24, 2, 2, 100, 0, -1593, 192),
            (1, 0, 24, 3, 0, 256, 0, 0, 64),
            (1, 0, 24, 3, 0, 128, 0, 0, 64),
            (1, 0, 24, 3, 0, 100, 0, 0, 64),
            (1, 0, 24, 3, 1, 256, 0, 0, 128),
            (1, 0, 24, 3, 1, 128, 0, 0, 128),
            (1, 0, 24, 3, 1, 100, 0, 0, 128),
            (1, 0, 24, 3, 2, 256, 0, -3072, 192),
            (1, 0, 24, 3, 2, 128, 0, -1536, 192),
            (1, 0, 24, 3, 2, 100, 0, -1200, 192),
            (1, 100, 8, 0, 0, 256, 618, -1952, 64),
            (1, 100, 8, 0, 0, 128, 309, -976, 64),
            (1, 100, 8, 0, 0, 100, 241, -762, 64),
            (1, 100, 8, 0, 1, 256, 618, -1952, 128),
            (1, 100, 8, 0, 1, 128, 309, -976, 128),
            (1, 100, 8, 0, 1, 100, 241, -762, 128),
            (1, 100, 8, 0, 2, 256, 618, -1952, 192),
            (1, 100, 8, 0, 2, 128, 309, -976, 192),
            (1, 100, 8, 0, 2, 100, 241, -762, 192),
            (1, 100, 8, 1, 0, 256, 154, -488, 64),
            (1, 100, 8, 1, 0, 128, 77, -244, 64),
            (1, 100, 8, 1, 0, 100, 60, -190, 64),
            (1, 100, 8, 1, 1, 256, 205, -650, 128),
            (1, 100, 8, 1, 1, 128, 102, -325, 128),
            (1, 100, 8, 1, 1, 100, 80, -253, 128),
            (1, 100, 8, 1, 2, 256, 309, -976, 192),
            (1, 100, 8, 1, 2, 128, 154, -488, 192),
            (1, 100, 8, 1, 2, 100, 120, -381, 192),
            (1, 100, 8, 2, 0, 256, 0, 0, 64),
            (1, 100, 8, 2, 0, 128, 0, 0, 64),
            (1, 100, 8, 2, 0, 100, 0, 0, 64),
            (1, 100, 8, 2, 1, 256, 205, -648, 128),
            (1, 100, 8, 2, 1, 128, 102, -324, 128),
            (1, 100, 8, 2, 1, 100, 80, -252, 128),
            (1, 100, 8, 2, 2, 256, 410, -1296, 192),
            (1, 100, 8, 2, 2, 128, 205, -648, 192),
            (1, 100, 8, 2, 2, 100, 160, -506, 192),
            (1, 100, 8, 3, 0, 256, 0, 0, 64),
            (1, 100, 8, 3, 0, 128, 0, 0, 64),
            (1, 100, 8, 3, 0, 100, 0, 0, 64),
            (1, 100, 8, 3, 1, 256, 0, 0, 128),
            (1, 100, 8, 3, 1, 128, 0, 0, 128),
            (1, 100, 8, 3, 1, 100, 0, 0, 128),
            (1, 100, 8, 3, 2, 256, 309, -976, 192),
            (1, 100, 8, 3, 2, 128, 154, -488, 192),
            (1, 100, 8, 3, 2, 100, 120, -381, 192),
            (1, 100, 24, 0, 0, 256, 1855, -5857, 64),
            (1, 100, 24, 0, 0, 128, 927, -2928, 64),
            (1, 100, 24, 0, 0, 100, 724, -2287, 64),
            (1, 100, 24, 0, 1, 256, 1855, -5857, 128),
            (1, 100, 24, 0, 1, 128, 927, -2928, 128),
            (1, 100, 24, 0, 1, 100, 724, -2287, 128),
            (1, 100, 24, 0, 2, 256, 1855, -5857, 192),
            (1, 100, 24, 0, 2, 128, 927, -2928, 192),
            (1, 100, 24, 0, 2, 100, 724, -2287, 192),
            (1, 100, 24, 1, 0, 256, 463, -1464, 64),
            (1, 100, 24, 1, 0, 128, 231, -732, 64),
            (1, 100, 24, 1, 0, 100, 181, -571, 64),
            (1, 100, 24, 1, 1, 256, 618, -1952, 128),
            (1, 100, 24, 1, 1, 128, 309, -976, 128),
            (1, 100, 24, 1, 1, 100, 241, -762, 128),
            (1, 100, 24, 1, 2, 256, 927, -2928, 192),
            (1, 100, 24, 1, 2, 128, 463, -1464, 192),
            (1, 100, 24, 1, 2, 100, 362, -1143, 192),
            (1, 100, 24, 2, 0, 256, 0, 0, 64),
            (1, 100, 24, 2, 0, 128, 0, 0, 64),
            (1, 100, 24, 2, 0, 100, 0, 0, 64),
            (1, 100, 24, 2, 1, 256, 616, -1944, 128),
            (1, 100, 24, 2, 1, 128, 308, -972, 128),
            (1, 100, 24, 2, 1, 100, 240, -758, 128),
            (1, 100, 24, 2, 2, 256, 1232, -3889, 192),
            (1, 100, 24, 2, 2, 128, 616, -1944, 192),
            (1, 100, 24, 2, 2, 100, 481, -1518, 192),
            (1, 100, 24, 3, 0, 256, 0, 0, 64),
            (1, 100, 24, 3, 0, 128, 0, 0, 64),
            (1, 100, 24, 3, 0, 100, 0, 0, 64),
            (1, 100, 24, 3, 1, 256, 0, 0, 128),
            (1, 100, 24, 3, 1, 128, 0, 0, 128),
            (1, 100, 24, 3, 1, 100, 0, 0, 128),
            (1, 100, 24, 3, 2, 256, 927, -2928, 192),
            (1, 100, 24, 3, 2, 128, 463, -1464, 192),
            (1, 100, 24, 3, 2, 100, 362, -1143, 192),
            (1, 1500, 8, 0, 0, 256, -2035, 225, 64),
            (1, 1500, 8, 0, 0, 128, -1017, 112, 64),
            (1, 1500, 8, 0, 0, 100, -795, 88, 64),
            (1, 1500, 8, 0, 1, 256, -2035, 225, 128),
            (1, 1500, 8, 0, 1, 128, -1017, 112, 128),
            (1, 1500, 8, 0, 1, 100, -795, 88, 128),
            (1, 1500, 8, 0, 2, 256, -2035, 225, 192),
            (1, 1500, 8, 0, 2, 128, -1017, 112, 192),
            (1, 1500, 8, 0, 2, 100, -795, 88, 192),
            (1, 1500, 8, 1, 0, 256, -508, 56, 64),
            (1, 1500, 8, 1, 0, 128, -254, 28, 64),
            (1, 1500, 8, 1, 0, 100, -198, 22, 64),
            (1, 1500, 8, 1, 1, 256, -677, 75, 128),
            (1, 1500, 8, 1, 1, 128, -338, 37, 128),
            (1, 1500, 8, 1, 1, 100, -264, 29, 128),
            (1, 1500, 8, 1, 2, 256, -1017, 112, 192),
            (1, 1500, 8, 1, 2, 128, -508, 56, 192),
            (1, 1500, 8, 1, 2, 100, -397, 44, 192),
            (1, 1500, 8, 2, 0, 256, 0, 0, 64),
            (1, 1500, 8, 2, 0, 128, 0, 0, 64),
            (1, 1500, 8, 2, 0, 100, 0, 0, 64),
            (1, 1500, 8, 2, 1, 256, -675, 74, 128),
            (1, 1500, 8, 2, 1, 128, -337, 37, 128),
            (1, 1500, 8, 2, 1, 100, -263, 29, 128),
            (1, 1500, 8, 2, 2, 256, -1351, 149, 192),
            (1, 1500, 8, 2, 2, 128, -675, 74, 192),
            (1, 1500, 8, 2, 2, 100, -527, 58, 192),
            (1, 1500, 8, 3, 0, 256, 0, 0, 64),
            (1, 1500, 8, 3, 0, 128, 0, 0, 64),
            (1, 1500, 8, 3, 0, 100, 0, 0, 64),
            (1, 1500, 8, 3, 1, 256, 0, 0, 128),
            (1, 1500, 8, 3, 1, 128, 0, 0, 128),
            (1, 1500, 8, 3, 1, 100, 0, 0, 128),
            (1, 1500, 8, 3, 2, 256, -1017, 112, 192),
            (1, 1500, 8, 3, 2, 128, -508, 56, 192),
            (1, 1500, 8, 3, 2, 100, -397, 44, 192),
            (1, 1500, 24, 0, 0, 256, -6106, 677, 64),
            (1, 1500, 24, 0, 0, 128, -3053, 338, 64),
            (1, 1500, 24, 0, 0, 100, -2385, 264, 64),
            (1, 1500, 24, 0, 1, 256, -6106, 677, 128),
            (1, 1500, 24, 0, 1, 128, -3053, 338, 128),
            (1, 1500, 24, 0, 1, 100, -2385, 264, 128),
            (1, 1500, 24, 0, 2, 256, -6106, 677, 192),
            (1, 1500, 24, 0, 2, 128, -3053, 338, 192),
            (1, 1500, 24, 0, 2, 100, -2385, 264, 192),
            (1, 1500, 24, 1, 0, 256, -1526, 169, 64),
            (1, 1500, 24, 1, 0, 128, -763, 84, 64),
            (1, 1500, 24, 1, 0, 100, -596, 66, 64),
            (1, 1500, 24, 1, 1, 256, -2035, 225, 128),
            (1, 1500, 24, 1, 1, 128, -1017, 112, 128),
            (1, 1500, 24, 1, 1, 100, -795, 88, 128),
            (1, 1500, 24, 1, 2, 256, -3053, 338, 192),
            (1, 1500, 24, 1, 2, 128, -1526, 169, 192),
            (1, 1500, 24, 1, 2, 100, -1192, 132, 192),
            (1, 1500, 24, 2, 0, 256, 0, 0, 64),
            (1, 1500, 24, 2, 0, 128, 0, 0, 64),
            (1, 1500, 24, 2, 0, 100, 0, 0, 64),
            (1, 1500, 24, 2, 1, 256, -2027, 224, 128),
            (1, 1500, 24, 2, 1, 128, -1013, 112, 128),
            (1, 1500, 24, 2, 1, 100, -791, 87, 128),
            (1, 1500, 24, 2, 2, 256, -4055, 449, 192),
            (1, 1500, 24, 2, 2, 128, -2027, 224, 192),
            (1, 1500, 24, 2, 2, 100, -1583, 175, 192),
            (1, 1500, 24, 3, 0, 256, 0, 0, 64),
            (1, 1500, 24, 3, 0, 128, 0, 0, 64),
            (1, 1500, 24, 3, 0, 100, 0, 0, 64),
            (1, 1500, 24, 3, 1, 256, 0, 0, 128),
            (1, 1500, 24, 3, 1, 128, 0, 0, 128),
            (1, 1500, 24, 3, 1, 100, 0, 0, 128),
            (1, 1500, 24, 3, 2, 256, -3053, 338, 192),
            (1, 1500, 24, 3, 2, 128, -1526, 169, 192),
            (1, 1500, 24, 3, 2, 100, -1192, 132, 192),
            (2, 0, 8, 0, 0, 256, -2048, 0, 1),
            (2, 0, 8, 0, 0, 128, -1024, 0, 1),
            (2, 0, 8, 0, 0, 100, -800, 0, 1),
            (2, 0, 8, 0, 1, 256, -2048, 0, 2),
            (2, 0, 8, 0, 1, 128, -1024, 0, 2),
            (2, 0, 8, 0, 1, 100, -800, 0, 2),
            (2, 0, 8, 0, 2, 256, -2048, 0, 3),
            (2, 0, 8, 0, 2, 128, -1024, 0, 3),
            (2, 0, 8, 0, 2, 100, -800, 0, 3),
            (2, 0, 8, 1, 0, 256, -512, 0, 1),
            (2, 0, 8, 1, 0, 128, -256, 0, 1),
            (2, 0, 8, 1, 0, 100, -200, 0, 1),
            (2, 0, 8, 1, 1, 256, -682, 0, 2),
            (2, 0, 8, 1, 1, 128, -341, 0, 2),
            (2, 0, 8, 1, 1, 100, -266, 0, 2),
            (2, 0, 8, 1, 2, 256, -1024, 0, 3),
            (2, 0, 8, 1, 2, 128, -512, 0, 3),
            (2, 0, 8, 1, 2, 100, -400, 0, 3),
            (2, 0, 8, 2, 0, 256, 0, 0, 1),
            (2, 0, 8, 2, 0, 128, 0, 0, 1),
            (2, 0, 8, 2, 0, 100, 0, 0, 1),
            (2, 0, 8, 2, 1, 256, -680, 0, 2),
            (2, 0, 8, 2, 1, 128, -340, 0, 2),
            (2, 0, 8, 2, 1, 100, -265, 0, 2),
            (2, 0, 8, 2, 2, 256, -1360, 0, 3),
            (2, 0, 8, 2, 2, 128, -680, 0, 3),
            (2, 0, 8, 2, 2, 100, -531, 0, 3),
            (2, 0, 8, 3, 0, 256, 0, 0, 1),
            (2, 0, 8, 3, 0, 128, 0, 0, 1),
            (2, 0, 8, 3, 0, 100, 0, 0, 1),
            (2, 0, 8, 3, 1, 256, 0, 0, 2),
            (2, 0, 8, 3, 1, 128, 0, 0, 2),
            (2, 0, 8, 3, 1, 100, 0, 0, 2),
            (2, 0, 8, 3, 2, 256, -1024, 0, 3),
            (2, 0, 8, 3, 2, 128, -512, 0, 3),
            (2, 0, 8, 3, 2, 100, -400, 0, 3),
            (2, 0, 24, 0, 0, 256, -6144, 0, 1),
            (2, 0, 24, 0, 0, 128, -3072, 0, 1),
            (2, 0, 24, 0, 0, 100, -2400, 0, 1),
            (2, 0, 24, 0, 1, 256, -6144, 0, 2),
            (2, 0, 24, 0, 1, 128, -3072, 0, 2),
            (2, 0, 24, 0, 1, 100, -2400, 0, 2),
            (2, 0, 24, 0, 2, 256, -6144, 0, 3),
            (2, 0, 24, 0, 2, 128, -3072, 0, 3),
            (2, 0, 24, 0, 2, 100, -2400, 0, 3),
            (2, 0, 24, 1, 0, 256, -1536, 0, 1),
            (2, 0, 24, 1, 0, 128, -768, 0, 1),
            (2, 0, 24, 1, 0, 100, -600, 0, 1),
            (2, 0, 24, 1, 1, 256, -2048, 0, 2),
            (2, 0, 24, 1, 1, 128, -1024, 0, 2),
            (2, 0, 24, 1, 1, 100, -800, 0, 2),
            (2, 0, 24, 1, 2, 256, -3072, 0, 3),
            (2, 0, 24, 1, 2, 128, -1536, 0, 3),
            (2, 0, 24, 1, 2, 100, -1200, 0, 3),
            (2, 0, 24, 2, 0, 256, 0, 0, 1),
            (2, 0, 24, 2, 0, 128, 0, 0, 1),
            (2, 0, 24, 2, 0, 100, 0, 0, 1),
            (2, 0, 24, 2, 1, 256, -2040, 0, 2),
            (2, 0, 24, 2, 1, 128, -1020, 0, 2),
            (2, 0, 24, 2, 1, 100, -796, 0, 2),
            (2, 0, 24, 2, 2, 256, -4080, 0, 3),
            (2, 0, 24, 2, 2, 128, -2040, 0, 3),
            (2, 0, 24, 2, 2, 100, -1593, 0, 3),
            (2, 0, 24, 3, 0, 256, 0, 0, 1),
            (2, 0, 24, 3, 0, 128, 0, 0, 1),
            (2, 0, 24, 3, 0, 100, 0, 0, 1),
            (2, 0, 24, 3, 1, 256, 0, 0, 2),
            (2, 0, 24, 3, 1, 128, 0, 0, 2),
            (2, 0, 24, 3, 1, 100, 0, 0, 2),
            (2, 0, 24, 3, 2, 256, -3072, 0, 3),
            (2, 0, 24, 3, 2, 128, -1536, 0, 3),
            (2, 0, 24, 3, 2, 100, -1200, 0, 3),
            (2, 100, 8, 0, 0, 256, -1952, -618, 1),
            (2, 100, 8, 0, 0, 128, -976, -309, 1),
            (2, 100, 8, 0, 0, 100, -762, -241, 1),
            (2, 100, 8, 0, 1, 256, -1952, -618, 2),
            (2, 100, 8, 0, 1, 128, -976, -309, 2),
            (2, 100, 8, 0, 1, 100, -762, -241, 2),
            (2, 100, 8, 0, 2, 256, -1952, -618, 3),
            (2, 100, 8, 0, 2, 128, -976, -309, 3),
            (2, 100, 8, 0, 2, 100, -762, -241, 3),
            (2, 100, 8, 1, 0, 256, -488, -154, 1),
            (2, 100, 8, 1, 0, 128, -244, -77, 1),
            (2, 100, 8, 1, 0, 100, -190, -60, 1),
            (2, 100, 8, 1, 1, 256, -650, -205, 2),
            (2, 100, 8, 1, 1, 128, -325, -102, 2),
            (2, 100, 8, 1, 1, 100, -253, -80, 2),
            (2, 100, 8, 1, 2, 256, -976, -309, 3),
            (2, 100, 8, 1, 2, 128, -488, -154, 3),
            (2, 100, 8, 1, 2, 100, -381, -120, 3),
            (2, 100, 8, 2, 0, 256, 0, 0, 1),
            (2, 100, 8, 2, 0, 128, 0, 0, 1),
            (2, 100, 8, 2, 0, 100, 0, 0, 1),
            (2, 100, 8, 2, 1, 256, -648, -205, 2),
            (2, 100, 8, 2, 1, 128, -324, -102, 2),
            (2, 100, 8, 2, 1, 100, -252, -80, 2),
            (2, 100, 8, 2, 2, 256, -1296, -410, 3),
            (2, 100, 8, 2, 2, 128, -648, -205, 3),
            (2, 100, 8, 2, 2, 100, -506, -160, 3),
            (2, 100, 8, 3, 0, 256, 0, 0, 1),
            (2, 100, 8, 3, 0, 128, 0, 0, 1),
            (2, 100, 8, 3, 0, 100, 0, 0, 1),
            (2, 100, 8, 3, 1, 256, 0, 0, 2),
            (2, 100, 8, 3, 1, 128, 0, 0, 2),
            (2, 100, 8, 3, 1, 100, 0, 0, 2),
            (2, 100, 8, 3, 2, 256, -976, -309, 3),
            (2, 100, 8, 3, 2, 128, -488, -154, 3),
            (2, 100, 8, 3, 2, 100, -381, -120, 3),
            (2, 100, 24, 0, 0, 256, -5857, -1855, 1),
            (2, 100, 24, 0, 0, 128, -2928, -927, 1),
            (2, 100, 24, 0, 0, 100, -2287, -724, 1),
            (2, 100, 24, 0, 1, 256, -5857, -1855, 2),
            (2, 100, 24, 0, 1, 128, -2928, -927, 2),
            (2, 100, 24, 0, 1, 100, -2287, -724, 2),
            (2, 100, 24, 0, 2, 256, -5857, -1855, 3),
            (2, 100, 24, 0, 2, 128, -2928, -927, 3),
            (2, 100, 24, 0, 2, 100, -2287, -724, 3),
            (2, 100, 24, 1, 0, 256, -1464, -463, 1),
            (2, 100, 24, 1, 0, 128, -732, -231, 1),
            (2, 100, 24, 1, 0, 100, -571, -181, 1),
            (2, 100, 24, 1, 1, 256, -1952, -618, 2),
            (2, 100, 24, 1, 1, 128, -976, -309, 2),
            (2, 100, 24, 1, 1, 100, -762, -241, 2),
            (2, 100, 24, 1, 2, 256, -2928, -927, 3),
            (2, 100, 24, 1, 2, 128, -1464, -463, 3),
            (2, 100, 24, 1, 2, 100, -1143, -362, 3),
            (2, 100, 24, 2, 0, 256, 0, 0, 1),
            (2, 100, 24, 2, 0, 128, 0, 0, 1),
            (2, 100, 24, 2, 0, 100, 0, 0, 1),
            (2, 100, 24, 2, 1, 256, -1944, -616, 2),
            (2, 100, 24, 2, 1, 128, -972, -308, 2),
            (2, 100, 24, 2, 1, 100, -758, -240, 2),
            (2, 100, 24, 2, 2, 256, -3889, -1232, 3),
            (2, 100, 24, 2, 2, 128, -1944, -616, 3),
            (2, 100, 24, 2, 2, 100, -1518, -481, 3),
            (2, 100, 24, 3, 0, 256, 0, 0, 1),
            (2, 100, 24, 3, 0, 128, 0, 0, 1),
            (2, 100, 24, 3, 0, 100, 0, 0, 1),
            (2, 100, 24, 3, 1, 256, 0, 0, 2),
            (2, 100, 24, 3, 1, 128, 0, 0, 2),
            (2, 100, 24, 3, 1, 100, 0, 0, 2),
            (2, 100, 24, 3, 2, 256, -2928, -927, 3),
            (2, 100, 24, 3, 2, 128, -1464, -463, 3),
            (2, 100, 24, 3, 2, 100, -1143, -362, 3),
            (2, 1500, 8, 0, 0, 256, 225, 2035, 1),
            (2, 1500, 8, 0, 0, 128, 112, 1017, 1),
            (2, 1500, 8, 0, 0, 100, 88, 795, 1),
            (2, 1500, 8, 0, 1, 256, 225, 2035, 2),
            (2, 1500, 8, 0, 1, 128, 112, 1017, 2),
            (2, 1500, 8, 0, 1, 100, 88, 795, 2),
            (2, 1500, 8, 0, 2, 256, 225, 2035, 3),
            (2, 1500, 8, 0, 2, 128, 112, 1017, 3),
            (2, 1500, 8, 0, 2, 100, 88, 795, 3),
            (2, 1500, 8, 1, 0, 256, 56, 508, 1),
            (2, 1500, 8, 1, 0, 128, 28, 254, 1),
            (2, 1500, 8, 1, 0, 100, 22, 198, 1),
            (2, 1500, 8, 1, 1, 256, 75, 677, 2),
            (2, 1500, 8, 1, 1, 128, 37, 338, 2),
            (2, 1500, 8, 1, 1, 100, 29, 264, 2),
            (2, 1500, 8, 1, 2, 256, 112, 1017, 3),
            (2, 1500, 8, 1, 2, 128, 56, 508, 3),
            (2, 1500, 8, 1, 2, 100, 44, 397, 3),
            (2, 1500, 8, 2, 0, 256, 0, 0, 1),
            (2, 1500, 8, 2, 0, 128, 0, 0, 1),
            (2, 1500, 8, 2, 0, 100, 0, 0, 1),
            (2, 1500, 8, 2, 1, 256, 74, 675, 2),
            (2, 1500, 8, 2, 1, 128, 37, 337, 2),
            (2, 1500, 8, 2, 1, 100, 29, 263, 2),
            (2, 1500, 8, 2, 2, 256, 149, 1351, 3),
            (2, 1500, 8, 2, 2, 128, 74, 675, 3),
            (2, 1500, 8, 2, 2, 100, 58, 527, 3),
            (2, 1500, 8, 3, 0, 256, 0, 0, 1),
            (2, 1500, 8, 3, 0, 128, 0, 0, 1),
            (2, 1500, 8, 3, 0, 100, 0, 0, 1),
            (2, 1500, 8, 3, 1, 256, 0, 0, 2),
            (2, 1500, 8, 3, 1, 128, 0, 0, 2),
            (2, 1500, 8, 3, 1, 100, 0, 0, 2),
            (2, 1500, 8, 3, 2, 256, 112, 1017, 3),
            (2, 1500, 8, 3, 2, 128, 56, 508, 3),
            (2, 1500, 8, 3, 2, 100, 44, 397, 3),
            (2, 1500, 24, 0, 0, 256, 677, 6106, 1),
            (2, 1500, 24, 0, 0, 128, 338, 3053, 1),
            (2, 1500, 24, 0, 0, 100, 264, 2385, 1),
            (2, 1500, 24, 0, 1, 256, 677, 6106, 2),
            (2, 1500, 24, 0, 1, 128, 338, 3053, 2),
            (2, 1500, 24, 0, 1, 100, 264, 2385, 2),
            (2, 1500, 24, 0, 2, 256, 677, 6106, 3),
            (2, 1500, 24, 0, 2, 128, 338, 3053, 3),
            (2, 1500, 24, 0, 2, 100, 264, 2385, 3),
            (2, 1500, 24, 1, 0, 256, 169, 1526, 1),
            (2, 1500, 24, 1, 0, 128, 84, 763, 1),
            (2, 1500, 24, 1, 0, 100, 66, 596, 1),
            (2, 1500, 24, 1, 1, 256, 225, 2035, 2),
            (2, 1500, 24, 1, 1, 128, 112, 1017, 2),
            (2, 1500, 24, 1, 1, 100, 88, 795, 2),
            (2, 1500, 24, 1, 2, 256, 338, 3053, 3),
            (2, 1500, 24, 1, 2, 128, 169, 1526, 3),
            (2, 1500, 24, 1, 2, 100, 132, 1192, 3),
            (2, 1500, 24, 2, 0, 256, 0, 0, 1),
            (2, 1500, 24, 2, 0, 128, 0, 0, 1),
            (2, 1500, 24, 2, 0, 100, 0, 0, 1),
            (2, 1500, 24, 2, 1, 256, 224, 2027, 2),
            (2, 1500, 24, 2, 1, 128, 112, 1013, 2),
            (2, 1500, 24, 2, 1, 100, 87, 791, 2),
            (2, 1500, 24, 2, 2, 256, 449, 4055, 3),
            (2, 1500, 24, 2, 2, 128, 224, 2027, 3),
            (2, 1500, 24, 2, 2, 100, 175, 1583, 3),
            (2, 1500, 24, 3, 0, 256, 0, 0, 1),
            (2, 1500, 24, 3, 0, 128, 0, 0, 1),
            (2, 1500, 24, 3, 0, 100, 0, 0, 1),
            (2, 1500, 24, 3, 1, 256, 0, 0, 2),
            (2, 1500, 24, 3, 1, 128, 0, 0, 2),
            (2, 1500, 24, 3, 1, 100, 0, 0, 2),
            (2, 1500, 24, 3, 2, 256, 338, 3053, 3),
            (2, 1500, 24, 3, 2, 128, 169, 1526, 3),
            (2, 1500, 24, 3, 2, 100, 132, 1192, 3),
            (3, 0, 8, 0, 0, 256, 2048, 0, 4),
            (3, 0, 8, 0, 0, 128, 1024, 0, 4),
            (3, 0, 8, 0, 0, 100, 800, 0, 4),
            (3, 0, 8, 0, 1, 256, 2048, 0, 8),
            (3, 0, 8, 0, 1, 128, 1024, 0, 8),
            (3, 0, 8, 0, 1, 100, 800, 0, 8),
            (3, 0, 8, 0, 2, 256, 2048, 0, 12),
            (3, 0, 8, 0, 2, 128, 1024, 0, 12),
            (3, 0, 8, 0, 2, 100, 800, 0, 12),
            (3, 0, 8, 1, 0, 256, 512, 0, 4),
            (3, 0, 8, 1, 0, 128, 256, 0, 4),
            (3, 0, 8, 1, 0, 100, 200, 0, 4),
            (3, 0, 8, 1, 1, 256, 682, 0, 8),
            (3, 0, 8, 1, 1, 128, 341, 0, 8),
            (3, 0, 8, 1, 1, 100, 266, 0, 8),
            (3, 0, 8, 1, 2, 256, 1024, 0, 12),
            (3, 0, 8, 1, 2, 128, 512, 0, 12),
            (3, 0, 8, 1, 2, 100, 400, 0, 12),
            (3, 0, 8, 2, 0, 256, 0, 0, 4),
            (3, 0, 8, 2, 0, 128, 0, 0, 4),
            (3, 0, 8, 2, 0, 100, 0, 0, 4),
            (3, 0, 8, 2, 1, 256, 680, 0, 8),
            (3, 0, 8, 2, 1, 128, 340, 0, 8),
            (3, 0, 8, 2, 1, 100, 265, 0, 8),
            (3, 0, 8, 2, 2, 256, 1360, 0, 12),
            (3, 0, 8, 2, 2, 128, 680, 0, 12),
            (3, 0, 8, 2, 2, 100, 531, 0, 12),
            (3, 0, 8, 3, 0, 256, 0, 0, 4),
            (3, 0, 8, 3, 0, 128, 0, 0, 4),
            (3, 0, 8, 3, 0, 100, 0, 0, 4),
            (3, 0, 8, 3, 1, 256, 0, 0, 8),
            (3, 0, 8, 3, 1, 128, 0, 0, 8),
            (3, 0, 8, 3, 1, 100, 0, 0, 8),
            (3, 0, 8, 3, 2, 256, 1024, 0, 12),
            (3, 0, 8, 3, 2, 128, 512, 0, 12),
            (3, 0, 8, 3, 2, 100, 400, 0, 12),
            (3, 0, 24, 0, 0, 256, 6144, 0, 4),
            (3, 0, 24, 0, 0, 128, 3072, 0, 4),
            (3, 0, 24, 0, 0, 100, 2400, 0, 4),
            (3, 0, 24, 0, 1, 256, 6144, 0, 8),
            (3, 0, 24, 0, 1, 128, 3072, 0, 8),
            (3, 0, 24, 0, 1, 100, 2400, 0, 8),
            (3, 0, 24, 0, 2, 256, 6144, 0, 12),
            (3, 0, 24, 0, 2, 128, 3072, 0, 12),
            (3, 0, 24, 0, 2, 100, 2400, 0, 12),
            (3, 0, 24, 1, 0, 256, 1536, 0, 4),
            (3, 0, 24, 1, 0, 128, 768, 0, 4),
            (3, 0, 24, 1, 0, 100, 600, 0, 4),
            (3, 0, 24, 1, 1, 256, 2048, 0, 8),
            (3, 0, 24, 1, 1, 128, 1024, 0, 8),
            (3, 0, 24, 1, 1, 100, 800, 0, 8),
            (3, 0, 24, 1, 2, 256, 3072, 0, 12),
            (3, 0, 24, 1, 2, 128, 1536, 0, 12),
            (3, 0, 24, 1, 2, 100, 1200, 0, 12),
            (3, 0, 24, 2, 0, 256, 0, 0, 4),
            (3, 0, 24, 2, 0, 128, 0, 0, 4),
            (3, 0, 24, 2, 0, 100, 0, 0, 4),
            (3, 0, 24, 2, 1, 256, 2040, 0, 8),
            (3, 0, 24, 2, 1, 128, 1020, 0, 8),
            (3, 0, 24, 2, 1, 100, 796, 0, 8),
            (3, 0, 24, 2, 2, 256, 4080, 0, 12),
            (3, 0, 24, 2, 2, 128, 2040, 0, 12),
            (3, 0, 24, 2, 2, 100, 1593, 0, 12),
            (3, 0, 24, 3, 0, 256, 0, 0, 4),
            (3, 0, 24, 3, 0, 128, 0, 0, 4),
            (3, 0, 24, 3, 0, 100, 0, 0, 4),
            (3, 0, 24, 3, 1, 256, 0, 0, 8),
            (3, 0, 24, 3, 1, 128, 0, 0, 8),
            (3, 0, 24, 3, 1, 100, 0, 0, 8),
            (3, 0, 24, 3, 2, 256, 3072, 0, 12),
            (3, 0, 24, 3, 2, 128, 1536, 0, 12),
            (3, 0, 24, 3, 2, 100, 1200, 0, 12),
            (3, 100, 8, 0, 0, 256, 1952, 618, 4),
            (3, 100, 8, 0, 0, 128, 976, 309, 4),
            (3, 100, 8, 0, 0, 100, 762, 241, 4),
            (3, 100, 8, 0, 1, 256, 1952, 618, 8),
            (3, 100, 8, 0, 1, 128, 976, 309, 8),
            (3, 100, 8, 0, 1, 100, 762, 241, 8),
            (3, 100, 8, 0, 2, 256, 1952, 618, 12),
            (3, 100, 8, 0, 2, 128, 976, 309, 12),
            (3, 100, 8, 0, 2, 100, 762, 241, 12),
            (3, 100, 8, 1, 0, 256, 488, 154, 4),
            (3, 100, 8, 1, 0, 128, 244, 77, 4),
            (3, 100, 8, 1, 0, 100, 190, 60, 4),
            (3, 100, 8, 1, 1, 256, 650, 205, 8),
            (3, 100, 8, 1, 1, 128, 325, 102, 8),
            (3, 100, 8, 1, 1, 100, 253, 80, 8),
            (3, 100, 8, 1, 2, 256, 976, 309, 12),
            (3, 100, 8, 1, 2, 128, 488, 154, 12),
            (3, 100, 8, 1, 2, 100, 381, 120, 12),
            (3, 100, 8, 2, 0, 256, 0, 0, 4),
            (3, 100, 8, 2, 0, 128, 0, 0, 4),
            (3, 100, 8, 2, 0, 100, 0, 0, 4),
            (3, 100, 8, 2, 1, 256, 648, 205, 8),
            (3, 100, 8, 2, 1, 128, 324, 102, 8),
            (3, 100, 8, 2, 1, 100, 252, 80, 8),
            (3, 100, 8, 2, 2, 256, 1296, 410, 12),
            (3, 100, 8, 2, 2, 128, 648, 205, 12),
            (3, 100, 8, 2, 2, 100, 506, 160, 12),
            (3, 100, 8, 3, 0, 256, 0, 0, 4),
            (3, 100, 8, 3, 0, 128, 0, 0, 4),
            (3, 100, 8, 3, 0, 100, 0, 0, 4),
            (3, 100, 8, 3, 1, 256, 0, 0, 8),
            (3, 100, 8, 3, 1, 128, 0, 0, 8),
            (3, 100, 8, 3, 1, 100, 0, 0, 8),
            (3, 100, 8, 3, 2, 256, 976, 309, 12),
            (3, 100, 8, 3, 2, 128, 488, 154, 12),
            (3, 100, 8, 3, 2, 100, 381, 120, 12),
            (3, 100, 24, 0, 0, 256, 5857, 1855, 4),
            (3, 100, 24, 0, 0, 128, 2928, 927, 4),
            (3, 100, 24, 0, 0, 100, 2287, 724, 4),
            (3, 100, 24, 0, 1, 256, 5857, 1855, 8),
            (3, 100, 24, 0, 1, 128, 2928, 927, 8),
            (3, 100, 24, 0, 1, 100, 2287, 724, 8),
            (3, 100, 24, 0, 2, 256, 5857, 1855, 12),
            (3, 100, 24, 0, 2, 128, 2928, 927, 12),
            (3, 100, 24, 0, 2, 100, 2287, 724, 12),
            (3, 100, 24, 1, 0, 256, 1464, 463, 4),
            (3, 100, 24, 1, 0, 128, 732, 231, 4),
            (3, 100, 24, 1, 0, 100, 571, 181, 4),
            (3, 100, 24, 1, 1, 256, 1952, 618, 8),
            (3, 100, 24, 1, 1, 128, 976, 309, 8),
            (3, 100, 24, 1, 1, 100, 762, 241, 8),
            (3, 100, 24, 1, 2, 256, 2928, 927, 12),
            (3, 100, 24, 1, 2, 128, 1464, 463, 12),
            (3, 100, 24, 1, 2, 100, 1143, 362, 12),
            (3, 100, 24, 2, 0, 256, 0, 0, 4),
            (3, 100, 24, 2, 0, 128, 0, 0, 4),
            (3, 100, 24, 2, 0, 100, 0, 0, 4),
            (3, 100, 24, 2, 1, 256, 1944, 616, 8),
            (3, 100, 24, 2, 1, 128, 972, 308, 8),
            (3, 100, 24, 2, 1, 100, 758, 240, 8),
            (3, 100, 24, 2, 2, 256, 3889, 1232, 12),
            (3, 100, 24, 2, 2, 128, 1944, 616, 12),
            (3, 100, 24, 2, 2, 100, 1518, 481, 12),
            (3, 100, 24, 3, 0, 256, 0, 0, 4),
            (3, 100, 24, 3, 0, 128, 0, 0, 4),
            (3, 100, 24, 3, 0, 100, 0, 0, 4),
            (3, 100, 24, 3, 1, 256, 0, 0, 8),
            (3, 100, 24, 3, 1, 128, 0, 0, 8),
            (3, 100, 24, 3, 1, 100, 0, 0, 8),
            (3, 100, 24, 3, 2, 256, 2928, 927, 12),
            (3, 100, 24, 3, 2, 128, 1464, 463, 12),
            (3, 100, 24, 3, 2, 100, 1143, 362, 12),
            (3, 1500, 8, 0, 0, 256, -225, -2035, 4),
            (3, 1500, 8, 0, 0, 128, -112, -1017, 4),
            (3, 1500, 8, 0, 0, 100, -88, -795, 4),
            (3, 1500, 8, 0, 1, 256, -225, -2035, 8),
            (3, 1500, 8, 0, 1, 128, -112, -1017, 8),
            (3, 1500, 8, 0, 1, 100, -88, -795, 8),
            (3, 1500, 8, 0, 2, 256, -225, -2035, 12),
            (3, 1500, 8, 0, 2, 128, -112, -1017, 12),
            (3, 1500, 8, 0, 2, 100, -88, -795, 12),
            (3, 1500, 8, 1, 0, 256, -56, -508, 4),
            (3, 1500, 8, 1, 0, 128, -28, -254, 4),
            (3, 1500, 8, 1, 0, 100, -22, -198, 4),
            (3, 1500, 8, 1, 1, 256, -75, -677, 8),
            (3, 1500, 8, 1, 1, 128, -37, -338, 8),
            (3, 1500, 8, 1, 1, 100, -29, -264, 8),
            (3, 1500, 8, 1, 2, 256, -112, -1017, 12),
            (3, 1500, 8, 1, 2, 128, -56, -508, 12),
            (3, 1500, 8, 1, 2, 100, -44, -397, 12),
            (3, 1500, 8, 2, 0, 256, 0, 0, 4),
            (3, 1500, 8, 2, 0, 128, 0, 0, 4),
            (3, 1500, 8, 2, 0, 100, 0, 0, 4),
            (3, 1500, 8, 2, 1, 256, -74, -675, 8),
            (3, 1500, 8, 2, 1, 128, -37, -337, 8),
            (3, 1500, 8, 2, 1, 100, -29, -263, 8),
            (3, 1500, 8, 2, 2, 256, -149, -1351, 12),
            (3, 1500, 8, 2, 2, 128, -74, -675, 12),
            (3, 1500, 8, 2, 2, 100, -58, -527, 12),
            (3, 1500, 8, 3, 0, 256, 0, 0, 4),
            (3, 1500, 8, 3, 0, 128, 0, 0, 4),
            (3, 1500, 8, 3, 0, 100, 0, 0, 4),
            (3, 1500, 8, 3, 1, 256, 0, 0, 8),
            (3, 1500, 8, 3, 1, 128, 0, 0, 8),
            (3, 1500, 8, 3, 1, 100, 0, 0, 8),
            (3, 1500, 8, 3, 2, 256, -112, -1017, 12),
            (3, 1500, 8, 3, 2, 128, -56, -508, 12),
            (3, 1500, 8, 3, 2, 100, -44, -397, 12),
            (3, 1500, 24, 0, 0, 256, -677, -6106, 4),
            (3, 1500, 24, 0, 0, 128, -338, -3053, 4),
            (3, 1500, 24, 0, 0, 100, -264, -2385, 4),
            (3, 1500, 24, 0, 1, 256, -677, -6106, 8),
            (3, 1500, 24, 0, 1, 128, -338, -3053, 8),
            (3, 1500, 24, 0, 1, 100, -264, -2385, 8),
            (3, 1500, 24, 0, 2, 256, -677, -6106, 12),
            (3, 1500, 24, 0, 2, 128, -338, -3053, 12),
            (3, 1500, 24, 0, 2, 100, -264, -2385, 12),
            (3, 1500, 24, 1, 0, 256, -169, -1526, 4),
            (3, 1500, 24, 1, 0, 128, -84, -763, 4),
            (3, 1500, 24, 1, 0, 100, -66, -596, 4),
            (3, 1500, 24, 1, 1, 256, -225, -2035, 8),
            (3, 1500, 24, 1, 1, 128, -112, -1017, 8),
            (3, 1500, 24, 1, 1, 100, -88, -795, 8),
            (3, 1500, 24, 1, 2, 256, -338, -3053, 12),
            (3, 1500, 24, 1, 2, 128, -169, -1526, 12),
            (3, 1500, 24, 1, 2, 100, -132, -1192, 12),
            (3, 1500, 24, 2, 0, 256, 0, 0, 4),
            (3, 1500, 24, 2, 0, 128, 0, 0, 4),
            (3, 1500, 24, 2, 0, 100, 0, 0, 4),
            (3, 1500, 24, 2, 1, 256, -224, -2027, 8),
            (3, 1500, 24, 2, 1, 128, -112, -1013, 8),
            (3, 1500, 24, 2, 1, 100, -87, -791, 8),
            (3, 1500, 24, 2, 2, 256, -449, -4055, 12),
            (3, 1500, 24, 2, 2, 128, -224, -2027, 12),
            (3, 1500, 24, 2, 2, 100, -175, -1583, 12),
            (3, 1500, 24, 3, 0, 256, 0, 0, 4),
            (3, 1500, 24, 3, 0, 128, 0, 0, 4),
            (3, 1500, 24, 3, 0, 100, 0, 0, 4),
            (3, 1500, 24, 3, 1, 256, 0, 0, 8),
            (3, 1500, 24, 3, 1, 128, 0, 0, 8),
            (3, 1500, 24, 3, 1, 100, 0, 0, 8),
            (3, 1500, 24, 3, 2, 256, -338, -3053, 12),
            (3, 1500, 24, 3, 2, 128, -169, -1526, 12),
            (3, 1500, 24, 3, 2, 100, -132, -1192, 12),
        ];
        for &(stepper, heading, step, mode, counter, scale, dx, dz, after) in rows {
            let shift = [4, 6, 0, 2][usize::from(stepper)];
            let mut packed = counter << shift;
            let distance = initial_step_distance(&mut packed, shift, mode, i32::from(step) << 8);
            let movement = match stepper {
                0 => forward_translation(heading, distance, scale, false),
                1 => backward_translation(heading, distance, scale, false),
                2 => strafe_translation(Strafe::Left, heading, distance, scale, false),
                _ => strafe_translation(Strafe::Right, heading, distance, scale, false),
            };
            let position = movement.map_or([0; 3], |(heading, distance)| {
                cyrus_world::transform::translate_forward([0; 3], heading, distance).unwrap()
            });
            assert_eq!(
                (position, packed),
                ([dx, 0, dz], after),
                "stepper {stepper} heading {heading} step {step} mode {mode} counter {counter} scale {scale}"
            );
        }
    }

    #[test]
    fn ordinary_strafe_translates_a_quarter_turn_off_the_unturned_heading() {
        assert_eq!(
            strafe_translation(Strafe::Left, 100, 2048, 256, false),
            Some((612, 2048))
        );
        assert_eq!(
            strafe_translation(Strafe::Right, 100, 2048, 128, false),
            Some((1636, 1024))
        );
        assert_eq!(strafe_translation(Strafe::Left, 100, 2048, 256, true), None);
        assert_eq!(strafe_translation(Strafe::Right, 100, 0, 256, false), None);
        assert_eq!(
            (Strafe::Left.called_flag(), Strafe::Right.called_flag()),
            (1, 2)
        );
        assert_eq!(Strafe::Right.ordinary_groups(), [26, 27]);
    }

    #[test]
    fn strafe_turns_by_the_scaled_rate_and_moves_a_quarter_turn_off_the_new_heading() {
        // Left: heading 100 minus (4 * 256 >> 8 = 4) = 96; movement heading 96 + 512.
        assert_eq!(
            strafe_motion(Strafe::Left, 100, 4, 6 << 8, 256, false),
            (96, Some((608, 1536)))
        );
        // Right: heading 100 plus 4 = 104; movement heading 104 - 512 wraps to 1640.
        assert_eq!(
            strafe_motion(Strafe::Right, 100, 4, 6 << 8, 256, false),
            (104, Some((1640, 1536)))
        );
        // The turn wraps, and suppression or a zero distance only removes the movement.
        assert_eq!(
            strafe_motion(Strafe::Left, 2, 8, 6 << 8, 256, true),
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
