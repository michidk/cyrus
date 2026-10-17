//! Animation group classes follow preservation's `engine/animation.md#animation-group-classes`.

/// Class of an animation group: 1 for fighting groups, 2 for hit-reaction and death
/// groups, 0 for every other group number (including those outside 0 to 177).
pub fn group_class(group: u16) -> u8 {
    match group {
        8 | 31 | 91..=120 | 129..=133 | 135..=140 | 175..=177 => 1,
        74 | 121..=128 | 134 => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::group_class;

    #[test]
    fn fighting_and_reaction_groups_are_classified_and_others_are_zero() {
        for group in [8, 31, 91, 120, 129, 133, 135, 140, 175, 177] {
            assert_eq!(group_class(group), 1, "{group}");
        }
        for group in [74, 121, 128, 134] {
            assert_eq!(group_class(group), 2, "{group}");
        }
        for group in [0, 7, 9, 30, 90, 141, 174, 178, 500] {
            assert_eq!(group_class(group), 0, "{group}");
        }
    }
}
