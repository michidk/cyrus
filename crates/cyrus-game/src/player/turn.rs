//! Ordinary ground-turn arithmetic from redguard-preservation's
//! `docs/engine/player.md`. Eligibility and the simulation clock are separate.

/// Independent held-direction acceleration for ordinary ground turning.
/// Combat and other player states must select their own movement rules.
#[derive(Default)]
pub struct GroundTurn {
    left: u64,
    right: u64,
}

impl GroundTurn {
    /// Advance one original input update and return heading in 1/2048 turns.
    ///
    /// `increment` and `limit` are the nonnegative SYSTEM.INI `turn_speed` and
    /// `turn_max_speed`. `frame_scale` has eight fractional bits. This method
    /// does not derive a scale from elapsed seconds or decide turn eligibility.
    pub fn advance(
        &mut self,
        heading: u16,
        held: [bool; 2],
        increment: u32,
        limit: u32,
        frame_scale: u32,
    ) -> u16 {
        for (speed, pressed) in [&mut self.left, &mut self.right].into_iter().zip(held) {
            if !pressed {
                *speed = 0;
            } else if *speed < u64::from(limit) {
                *speed += u64::from(increment);
            }
        }
        let left = ((u128::from(self.left) * u128::from(frame_scale)) >> 8) % 2048;
        let right = ((u128::from(self.right) * u128::from(frame_scale)) >> 8) % 2048;
        ((u128::from(heading) + left + 2048 - right) % 2048) as u16
    }
}
