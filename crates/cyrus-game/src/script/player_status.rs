//! `PlayerStatus`: the first matching class of the selected player's motion bytes, living state,
//! current animation group and movement this update.

/// The selected player's state as `PlayerStatus` reads it.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerStatusInput {
    pub queued_motion: u8,
    pub falling_state: u8,
    pub sliding_state: u8,
    pub living_state: u8,
    pub group: Option<u16>,
    /// Whether the local position differs from the one saved at the update's entry.
    pub moved: bool,
}

/// Status class 0 through 21; the first matching rule wins and zero means none matched.
pub fn player_status(input: PlayerStatusInput) -> i32 {
    let PlayerStatusInput {
        queued_motion,
        falling_state,
        sliding_state,
        living_state,
        group,
        moved,
    } = input;
    let by_motion = [
        (queued_motion, 1, 1),
        (queued_motion, 2, 2),
        (queued_motion, 8, 3),
        (queued_motion, 3, 4),
        (falling_state, 2, 5),
        (falling_state, 8, 6),
        (falling_state, 3, 7),
    ];
    if let Some((_, _, class)) = by_motion.into_iter().find(|(byte, value, _)| byte == value) {
        return class;
    }
    if sliding_state != 0 {
        return 8;
    }
    // The living-state classes 13 and 14 repeat the tests of 9 and 10 and are never reached.
    for (value, class) in [(0x0a, 9), (0x0b, 10), (0x14, 11), (0x15, 12)] {
        if living_state == value {
            return class;
        }
    }
    for (groups, class) in [
        ([0x12, 0x1e], 15),
        ([0x13, 0x17], 16),
        ([0x18, 0x19], 17),
        ([0x1a, 0x1b], 18),
        ([0x14, 0x15], 19),
        ([0x16, 0x16], 20),
    ] {
        if group.is_some_and(|group| groups.contains(&group)) {
            return class;
        }
    }
    if moved { 21 } else { 0 }
}

/// `PlayerTurned`: whether any of the selected player's angles differs from the angles saved at
/// the ordinary update's entry.
pub fn player_turned(angles: [u16; 3], history: [u16; 3]) -> i32 {
    i32::from(angles.map(|angle| angle & 2047) != history.map(|angle| angle & 2047))
}

/// `PlayerTurnedDirection`: 1, 0 or -1 for the sign of the change of the negated yaw since the saved
/// one, taking the shorter way round the circle (a change of more than 1024 inverts the sign).
pub fn player_turned_direction(heading: u16, history: u16) -> i32 {
    let now = (2048 - i32::from(heading)) & 2047;
    let before = (2048 - i32::from(history)) & 2047;
    let sign = (now - before).signum();
    if (before - now).abs() > 1024 {
        -sign
    } else {
        sign
    }
}

/// Attribute index `PlayerMovementSpeed` returns for the player's current animation group, or
/// `None` for groups without one (the native then returns zero).
pub fn movement_speed_attribute(group: u16) -> Option<usize> {
    match group {
        18 | 30 => Some(8),
        19 => Some(9),
        20 | 21 => Some(12),
        22 | 23 => Some(13),
        24..=27 => Some(10),
        _ => None,
    }
}

/// `PlayerArctan`: the arctangent angle of the displacement from the player to the receiver, the
/// negation of the query bearing.
pub fn player_arctan(displacement: [i32; 2]) -> i32 {
    (-i32::from(super::spatial::query_bearing(
        displacement[0],
        displacement[1],
    ))) & 2047
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(edit: impl FnOnce(&mut PlayerStatusInput)) -> i32 {
        let mut input = PlayerStatusInput::default();
        edit(&mut input);
        player_status(input)
    }

    #[test]
    fn the_first_matching_rule_wins() {
        assert_eq!(status(|_| {}), 0);
        assert_eq!(status(|i| i.moved = true), 21);
        for (value, class) in [(1, 1), (2, 2), (8, 3), (3, 4)] {
            assert_eq!(status(|i| i.queued_motion = value), class);
        }
        for (value, class) in [(2, 5), (8, 6), (3, 7)] {
            assert_eq!(status(|i| i.falling_state = value), class);
        }
        assert_eq!(status(|i| i.sliding_state = 1), 8);
        for (value, class) in [(0x0a, 9), (0x0b, 10), (0x14, 11), (0x15, 12)] {
            assert_eq!(status(|i| i.living_state = value), class);
        }
        for (group, class) in [
            (0x12, 15),
            (0x1e, 15),
            (0x13, 16),
            (0x17, 16),
            (0x18, 17),
            (0x19, 17),
            (0x1a, 18),
            (0x1b, 18),
            (0x14, 19),
            (0x15, 19),
            (0x16, 20),
        ] {
            assert_eq!(status(|i| i.group = Some(group)), class);
        }
        // Earlier rules shadow later ones.
        assert_eq!(
            status(|i| {
                i.queued_motion = 3;
                i.falling_state = 2;
                i.moved = true;
            }),
            4
        );
        assert_eq!(
            status(|i| {
                i.living_state = 0x0a;
                i.group = Some(0x12);
                i.moved = true;
            }),
            9
        );
        assert_eq!(
            status(|i| {
                i.group = Some(0x12);
                i.moved = true;
            }),
            15
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
        // Attributes 8, 9, 10, 12 and 13 hold 0x11, 0x22, 0x33, 0x44 and 0x55 in the emulation.
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
            let value = movement_speed_attribute(group).map_or(0, |index| match index {
                8 => 0x11,
                9 => 0x22,
                10 => 0x33,
                12 => 0x44,
                _ => 0x55,
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
