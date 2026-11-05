//! `PlayerStatus`: the first matching class of the selected player's motion bytes, living state,
//! current animation group and movement this update.
use crate::script::JumpMotion;
use crate::script::LivingState;
use cyrus_script::{AnimationGroup, Attribute};
use cyrus_world::heading::wrap;

/// `PlayerStatus` return codes, named after the `ps_*` list of the installed `SOUP386.DEF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PlayerStatus {
    Idle,
    Jump,
    JumpForward,
    JumpRunForward,
    JumpBackward,
    FallForward,
    FallRunForward,
    FallBackward,
    Sliding,
    StartPullUp,
    PullUp,
    DrawSword,
    SheathSword,
    LedgeStartPullUp,
    LedgePullUp,
    RunForward,
    RunBackward,
    StepLeft,
    StepRight,
    WalkForward,
    WalkBackward,
    Moving,
}

/// The selected player's state as `PlayerStatus` reads it.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerStatusInput {
    pub queued_motion: JumpMotion,
    pub falling_state: JumpMotion,
    pub sliding_state: u8,
    pub living_state: LivingState,
    pub group: Option<AnimationGroup>,
    /// Whether the local position differs from the one saved at the update's entry.
    pub moved: bool,
}

/// The first matching rule wins; `Idle` means none matched.
pub fn player_status(input: PlayerStatusInput) -> PlayerStatus {
    let PlayerStatusInput {
        queued_motion,
        falling_state,
        sliding_state,
        living_state,
        group,
        moved,
    } = input;
    let by_motion = [
        (queued_motion, JumpMotion::STANDING, PlayerStatus::Jump),
        (
            queued_motion,
            JumpMotion::FORWARD,
            PlayerStatus::JumpForward,
        ),
        (
            queued_motion,
            JumpMotion::RUNNING,
            PlayerStatus::JumpRunForward,
        ),
        (
            queued_motion,
            JumpMotion::BACKWARD,
            PlayerStatus::JumpBackward,
        ),
        (
            falling_state,
            JumpMotion::FORWARD,
            PlayerStatus::FallForward,
        ),
        (
            falling_state,
            JumpMotion::RUNNING,
            PlayerStatus::FallRunForward,
        ),
        (
            falling_state,
            JumpMotion::BACKWARD,
            PlayerStatus::FallBackward,
        ),
    ];
    if let Some((_, _, class)) = by_motion.into_iter().find(|(byte, value, _)| byte == value) {
        return class;
    }
    if sliding_state != 0 {
        return PlayerStatus::Sliding;
    }
    // `LedgeStartPullUp` and `LedgePullUp` repeat the tests of `StartPullUp` and `PullUp` and are
    // never returned.
    for (value, class) in [
        (
            LivingState::TURNING_TO_HIGH_LEDGE,
            PlayerStatus::StartPullUp,
        ),
        (LivingState::HIGH_CLIMB, PlayerStatus::PullUp),
        (LivingState::DRAWING_SWORD, PlayerStatus::DrawSword),
        (LivingState::SHEATHING_SWORD, PlayerStatus::SheathSword),
    ] {
        if living_state == value {
            return class;
        }
    }
    for (groups, class) in [
        (
            [AnimationGroup::RUN, AnimationGroup::RUN_TORCH],
            PlayerStatus::RunForward,
        ),
        (
            [
                AnimationGroup::RUN_BACKWARD,
                AnimationGroup::WALK_BACKWARD_TORCH,
            ],
            PlayerStatus::RunBackward,
        ),
        (
            [AnimationGroup::STEP_LEFT, AnimationGroup::STEP_LEFT_TORCH],
            PlayerStatus::StepLeft,
        ),
        (
            [AnimationGroup::STEP_RIGHT, AnimationGroup::STEP_RIGHT_TORCH],
            PlayerStatus::StepRight,
        ),
        (
            [AnimationGroup::WALK, AnimationGroup::WALK_FORWARD_TORCH],
            PlayerStatus::WalkForward,
        ),
        (
            [AnimationGroup::WALK_BACKWARD, AnimationGroup::WALK_BACKWARD],
            PlayerStatus::WalkBackward,
        ),
    ] {
        if group.is_some_and(|group| groups.contains(&group)) {
            return class;
        }
    }
    if moved {
        PlayerStatus::Moving
    } else {
        PlayerStatus::Idle
    }
}

/// `PlayerTurned`: whether any of the selected player's angles differs from the angles saved at
/// the ordinary update's entry.
pub fn player_turned(angles: [u16; 3], history: [u16; 3]) -> i32 {
    i32::from(angles.map(wrap) != history.map(wrap))
}

/// `PlayerTurnedDirection`: 1, 0 or -1 for the sign of the change of the negated yaw since the saved
/// one, taking the shorter way round the circle (a change of more than 1024 inverts the sign).
pub fn player_turned_direction(heading: u16, history: u16) -> i32 {
    let now = wrap(2048 - i32::from(heading));
    let before = wrap(2048 - i32::from(history));
    let sign = (now - before).signum();
    if (before - now).abs() > 1024 {
        -sign
    } else {
        sign
    }
}

/// Attribute `PlayerMovementSpeed` returns for the player's current animation group, or
/// `None` for groups without one (the native then returns zero).
pub fn movement_speed_attribute(group: AnimationGroup) -> Option<Attribute> {
    match group {
        AnimationGroup::RUN | AnimationGroup::RUN_TORCH => Some(Attribute::ForwardSpeed),
        AnimationGroup::RUN_BACKWARD => Some(Attribute::BackwardSpeed),
        AnimationGroup::WALK | AnimationGroup::WALK_FORWARD_TORCH => Some(Attribute::ForwardWalk),
        AnimationGroup::WALK_BACKWARD | AnimationGroup::WALK_BACKWARD_TORCH => {
            Some(Attribute::BackwardWalk)
        }
        AnimationGroup::STEP_LEFT
        | AnimationGroup::STEP_LEFT_TORCH
        | AnimationGroup::STEP_RIGHT
        | AnimationGroup::STEP_RIGHT_TORCH => Some(Attribute::SideSpeed),
        _ => None,
    }
}

/// `PlayerArctan`: the arctangent angle of the displacement from the player to the receiver, the
/// negation of the query bearing.
pub fn player_arctan(displacement: [i32; 2]) -> i32 {
    wrap(-i32::from(super::spatial::query_bearing(
        displacement[0],
        displacement[1],
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(edit: impl FnOnce(&mut PlayerStatusInput)) -> PlayerStatus {
        let mut input = PlayerStatusInput::default();
        edit(&mut input);
        player_status(input)
    }

    #[test]
    fn the_first_matching_rule_wins() {
        assert_eq!(status(|_| {}), PlayerStatus::Idle);
        assert_eq!(status(|i| i.moved = true), PlayerStatus::Moving);
        for (value, class) in [
            (JumpMotion::STANDING, PlayerStatus::Jump),
            (JumpMotion::FORWARD, PlayerStatus::JumpForward),
            (JumpMotion::RUNNING, PlayerStatus::JumpRunForward),
            (JumpMotion::BACKWARD, PlayerStatus::JumpBackward),
        ] {
            assert_eq!(status(|i| i.queued_motion = value), class);
        }
        for (value, class) in [
            (JumpMotion::FORWARD, PlayerStatus::FallForward),
            (JumpMotion::RUNNING, PlayerStatus::FallRunForward),
            (JumpMotion::BACKWARD, PlayerStatus::FallBackward),
        ] {
            assert_eq!(status(|i| i.falling_state = value), class);
        }
        assert_eq!(status(|i| i.sliding_state = 1), PlayerStatus::Sliding);
        for (value, class) in [
            (
                LivingState::TURNING_TO_HIGH_LEDGE,
                PlayerStatus::StartPullUp,
            ),
            (LivingState::HIGH_CLIMB, PlayerStatus::PullUp),
            (LivingState::DRAWING_SWORD, PlayerStatus::DrawSword),
            (LivingState::SHEATHING_SWORD, PlayerStatus::SheathSword),
        ] {
            assert_eq!(status(|i| i.living_state = value), class);
        }
        for (group, class) in [
            (0x12, PlayerStatus::RunForward),
            (0x1e, PlayerStatus::RunForward),
            (0x13, PlayerStatus::RunBackward),
            (0x17, PlayerStatus::RunBackward),
            (0x18, PlayerStatus::StepLeft),
            (0x19, PlayerStatus::StepLeft),
            (0x1a, PlayerStatus::StepRight),
            (0x1b, PlayerStatus::StepRight),
            (0x14, PlayerStatus::WalkForward),
            (0x15, PlayerStatus::WalkForward),
            (0x16, PlayerStatus::WalkBackward),
        ] {
            assert_eq!(
                status(|i| i.group = Some(AnimationGroup::from_index(group))),
                class
            );
        }
        // Earlier rules shadow later ones.
        assert_eq!(
            status(|i| {
                i.queued_motion = JumpMotion::BACKWARD;
                i.falling_state = JumpMotion::FORWARD;
                i.moved = true;
            }),
            PlayerStatus::JumpBackward
        );
        assert_eq!(
            status(|i| {
                i.living_state = LivingState::TURNING_TO_HIGH_LEDGE;
                i.group = Some(AnimationGroup::RUN);
                i.moved = true;
            }),
            PlayerStatus::StartPullUp
        );
        assert_eq!(
            status(|i| {
                i.group = Some(AnimationGroup::RUN);
                i.moved = true;
            }),
            PlayerStatus::RunForward
        );
    }

    #[test]
    fn the_turn_speed_and_arctan_queries_match_the_original_routines() {
        // Rows from the original routines in an emulator.
        let turned: Vec<(u16, u16, u16, i32)> = serde_json::from_str(
            r#"[
[0,0,0,0],
[5,0,0,1],
[0,0,1,1],
[5,0,1,1],
[0,0,100,1],
[5,0,100,1],
[0,0,1023,1],
[5,0,1023,1],
[0,0,1024,1],
[5,0,1024,1],
[0,0,1025,1],
[5,0,1025,1],
[0,0,1500,1],
[5,0,1500,1],
[0,0,2047,1],
[5,0,2047,1],
[0,1,0,1],
[5,1,0,1],
[0,1,1,0],
[5,1,1,1],
[0,1,100,1],
[5,1,100,1],
[0,1,1023,1],
[5,1,1023,1],
[0,1,1024,1],
[5,1,1024,1],
[0,1,1025,1],
[5,1,1025,1],
[0,1,1500,1],
[5,1,1500,1],
[0,1,2047,1],
[5,1,2047,1],
[0,100,0,1],
[5,100,0,1],
[0,100,1,1],
[5,100,1,1],
[0,100,100,0],
[5,100,100,1],
[0,100,1023,1],
[5,100,1023,1],
[0,100,1024,1],
[5,100,1024,1],
[0,100,1025,1],
[5,100,1025,1],
[0,100,1500,1],
[5,100,1500,1],
[0,100,2047,1],
[5,100,2047,1],
[0,1023,0,1],
[5,1023,0,1],
[0,1023,1,1],
[5,1023,1,1],
[0,1023,100,1],
[5,1023,100,1],
[0,1023,1023,0],
[5,1023,1023,1],
[0,1023,1024,1],
[5,1023,1024,1],
[0,1023,1025,1],
[5,1023,1025,1],
[0,1023,1500,1],
[5,1023,1500,1],
[0,1023,2047,1],
[5,1023,2047,1],
[0,1024,0,1],
[5,1024,0,1],
[0,1024,1,1],
[5,1024,1,1],
[0,1024,100,1],
[5,1024,100,1],
[0,1024,1023,1],
[5,1024,1023,1],
[0,1024,1024,0],
[5,1024,1024,1],
[0,1024,1025,1],
[5,1024,1025,1],
[0,1024,1500,1],
[5,1024,1500,1],
[0,1024,2047,1],
[5,1024,2047,1],
[0,1025,0,1],
[5,1025,0,1],
[0,1025,1,1],
[5,1025,1,1],
[0,1025,100,1],
[5,1025,100,1],
[0,1025,1023,1],
[5,1025,1023,1],
[0,1025,1024,1],
[5,1025,1024,1],
[0,1025,1025,0],
[5,1025,1025,1],
[0,1025,1500,1],
[5,1025,1500,1],
[0,1025,2047,1],
[5,1025,2047,1],
[0,1500,0,1],
[5,1500,0,1],
[0,1500,1,1],
[5,1500,1,1],
[0,1500,100,1],
[5,1500,100,1],
[0,1500,1023,1],
[5,1500,1023,1],
[0,1500,1024,1],
[5,1500,1024,1],
[0,1500,1025,1],
[5,1500,1025,1],
[0,1500,1500,0],
[5,1500,1500,1],
[0,1500,2047,1],
[5,1500,2047,1],
[0,2047,0,1],
[5,2047,0,1],
[0,2047,1,1],
[5,2047,1,1],
[0,2047,100,1],
[5,2047,100,1],
[0,2047,1023,1],
[5,2047,1023,1],
[0,2047,1024,1],
[5,2047,1024,1],
[0,2047,1025,1],
[5,2047,1025,1],
[0,2047,1500,1],
[5,2047,1500,1],
[0,2047,2047,0],
[5,2047,2047,1]
]"#,
        )
        .unwrap();
        for (pitch, heading, history, expected) in turned {
            let history_angles = [0, history, 0];
            assert_eq!(
                player_turned([pitch, heading, 0], history_angles),
                expected,
                "pitch {pitch} heading {heading} history {history}"
            );
        }
        let direction: Vec<(u16, u16, i32)> = serde_json::from_str(
            r#"[
[0,0,0],
[0,1,1],
[0,100,1],
[0,1023,1],
[0,1024,-1],
[0,1025,-1],
[0,1500,-1],
[0,2047,-1],
[1,0,-1],
[1,1,0],
[1,100,1],
[1,1023,1],
[1,1024,1],
[1,1025,1],
[1,1500,-1],
[1,2047,-1],
[100,0,-1],
[100,1,-1],
[100,100,0],
[100,1023,1],
[100,1024,1],
[100,1025,1],
[100,1500,-1],
[100,2047,-1],
[1023,0,-1],
[1023,1,-1],
[1023,100,-1],
[1023,1023,0],
[1023,1024,1],
[1023,1025,1],
[1023,1500,1],
[1023,2047,1],
[1024,0,1],
[1024,1,-1],
[1024,100,-1],
[1024,1023,-1],
[1024,1024,0],
[1024,1025,1],
[1024,1500,1],
[1024,2047,1],
[1025,0,1],
[1025,1,-1],
[1025,100,-1],
[1025,1023,-1],
[1025,1024,-1],
[1025,1025,0],
[1025,1500,1],
[1025,2047,1],
[1500,0,1],
[1500,1,1],
[1500,100,1],
[1500,1023,-1],
[1500,1024,-1],
[1500,1025,-1],
[1500,1500,0],
[1500,2047,1],
[2047,0,1],
[2047,1,1],
[2047,100,1],
[2047,1023,-1],
[2047,1024,-1],
[2047,1025,-1],
[2047,1500,-1],
[2047,2047,0]
]"#,
        )
        .unwrap();
        for (heading, history, expected) in direction {
            assert_eq!(
                player_turned_direction(heading, history),
                expected,
                "heading {heading} history {history}"
            );
        }
        // `attr_forward_speed`, `attr_backward_speed`, `attr_side_speed`, `attr_forward_walk` and `attr_backward_walk` hold 0x11, 0x22, 0x33, 0x44 and 0x55 in the emulation.
        let speed: Vec<(u16, i32)> = serde_json::from_str(
            r#"[
[0,0],
[1,0],
[2,0],
[3,0],
[4,0],
[5,0],
[6,0],
[7,0],
[8,0],
[9,0],
[10,0],
[11,0],
[12,0],
[13,0],
[14,0],
[15,0],
[16,0],
[17,0],
[18,17],
[19,34],
[20,68],
[21,68],
[22,85],
[23,85],
[24,51],
[25,51],
[26,51],
[27,51],
[28,0],
[29,0],
[30,17],
[31,0],
[32,0],
[33,0],
[34,0],
[35,0],
[36,0],
[37,0],
[38,0],
[39,0],
[92,0],
[93,0],
[142,0],
[143,0]
]"#,
        )
        .unwrap();
        for (group, expected) in speed {
            let value =
                movement_speed_attribute(AnimationGroup::from_index(group)).map_or(0, |index| {
                    match index {
                        Attribute::ForwardSpeed => 0x11,
                        Attribute::BackwardSpeed => 0x22,
                        Attribute::SideSpeed => 0x33,
                        Attribute::ForwardWalk => 0x44,
                        _ => 0x55,
                    }
                });
            assert_eq!(value, expected, "group {group}");
        }
        let arctan: Vec<(i32, i32, i32)> = serde_json::from_str(
            r#"[
[0,5000,0],
[5000,0,512],
[0,-5000,1024],
[-5000,0,1536],
[3000,4000,209],
[-3000,4000,1839],
[4000,-3000,721],
[-2500,-2500,1280],
[7,100000,0],
[100000,7,512],
[0,0,0]
]"#,
        )
        .unwrap();
        for (x, z, expected) in arctan {
            assert_eq!(player_arctan([x, z]), expected, "({x},{z})");
        }
    }
}
