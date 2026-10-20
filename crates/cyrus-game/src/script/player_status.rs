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
}
