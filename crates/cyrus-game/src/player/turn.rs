//! Ordinary ground-turn arithmetic from redguard-preservation's
//! `docs/engine/player.md`. Eligibility and the simulation clock are separate.

/// Independent held-direction acceleration for ordinary ground turning.
/// Combat and other player states must select their own movement rules.
#[derive(Default, Debug, serde::Serialize)]
pub struct GroundTurn {
    left: u64,
    right: u64,
}

impl GroundTurn {
    /// One original input update: a released direction resets, a held one adds
    /// `increment` while below `limit`. `increment` and `limit` are the nonnegative
    /// SYSTEM.INI `turn_speed` and `turn_max_speed`.
    pub fn update(&mut self, held: [bool; 2], increment: u32, limit: u32) {
        for (speed, pressed) in [&mut self.left, &mut self.right].into_iter().zip(held) {
            if !pressed {
                *speed = 0;
            } else if *speed < u64::from(limit) {
                *speed += u64::from(increment);
            }
        }
    }

    /// Scaled right minus scaled left amount in 1/2048 turns; negative turns left.
    /// `frame_scale` has eight fractional bits and each side shifts separately.
    pub fn delta(&self, frame_scale: u32) -> i64 {
        let scaled = |speed: u64| ((u128::from(speed) * u128::from(frame_scale)) >> 8) as i64;
        scaled(self.right) - scaled(self.left)
    }

    /// Advance one original input update and return heading in 1/2048 turns.
    ///
    /// This method does not derive a scale from elapsed seconds or decide turn
    /// eligibility.
    pub fn advance(
        &mut self,
        heading: u16,
        held: [bool; 2],
        increment: u32,
        limit: u32,
        frame_scale: u32,
    ) -> u16 {
        self.update(held, increment, limit);
        turned(heading, self.delta(frame_scale))
    }
}

/// Heading after subtracting a signed turn amount, wrapped to 2048.
pub fn turned(heading: u16, delta: i64) -> u16 {
    (i64::from(heading) - delta).rem_euclid(2048) as u16
}

/// Roll after one ordinary turn update, in 1/2048 turns.
///
/// While `running` (current group 18 or 30) a left turn banks upward by 4 and a
/// right turn downward by 4, both stopping at 32 either side of zero; 8 per update
/// on the far side. Otherwise roll relaxes toward zero by 8. `delta` is the signed
/// amount from [`GroundTurn::delta`]; zero relaxes. Eligibility is the caller's.
pub fn lean(roll: u16, delta: i64, running: bool) -> u16 {
    let wrap = |value: i32| (value & 0x7ff) as u16;
    let roll = i32::from(roll);
    if running && delta < 0 && !(0x20..=0x400).contains(&roll) {
        return wrap(roll + if roll > 0x400 { 8 } else { 4 });
    }
    if running && delta > 0 && !(0x400..=0x7e0).contains(&roll) {
        return wrap(roll - if roll < 0x400 { 8 } else { 4 });
    }
    if running && delta != 0 {
        return wrap(roll);
    }
    relax(wrap(roll))
}

/// Roll relaxing toward zero by 8 per update, snapping to zero inside one step.
pub fn relax(roll: u16) -> u16 {
    let roll = i32::from(roll);
    if roll > 0x400 {
        if roll > 0x7f8 {
            0
        } else {
            ((roll + 8) & 0x7ff) as u16
        }
    } else if roll > 0 {
        if roll < 8 { 0 } else { (roll - 8) as u16 }
    } else {
        0
    }
}
