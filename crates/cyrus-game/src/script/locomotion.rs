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
    if state.rope_attached
        || state.sliding_state != 0
        || state.falling_state != 0
        || state.queued_motion != 0
        || !matches!(state.living_state, 0 | 15)
    {
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

/// One backward step: the heading (local Y angle plus 1024, wrapped) and the distance, or `None`
/// when no movement happens. `step` is in whole units; the translation-suppression marker blocks
/// movement; the scale is the shared frame scale applied with an arithmetic shift right of eight.
pub fn backward_translation(
    angle_y: u16,
    step: u8,
    frame_scale: i32,
    translation_suppressed: bool,
) -> Option<(u16, i32)> {
    if translation_suppressed {
        return None;
    }
    let heading = angle_y.wrapping_add(1024) & 2047;
    let distance = (i32::from(step) << 8).wrapping_mul(frame_scale) >> 8;
    (distance > 0).then_some((heading, distance))
}

/// The task result: one (continue) while `duration - 1 > elapsed`, zero otherwise.
pub fn continues(duration: i32, elapsed: i32) -> bool {
    duration.wrapping_sub(1) > elapsed
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(backward_translation(0, 8, 256, false), Some((1024, 2048)));
        assert_eq!(backward_translation(1500, 8, 128, false), Some((476, 1024)));
        assert_eq!(backward_translation(0, 8, 256, true), None);
        assert_eq!(backward_translation(0, 0, 256, false), None);
        assert_eq!(backward_translation(0, 8, 0, false), None);
    }

    #[test]
    fn the_task_continues_until_the_final_elapsed_tick() {
        assert!(continues(3, 0));
        assert!(continues(3, 1));
        assert!(!continues(3, 2));
        assert!(!continues(0, 0));
    }
}
