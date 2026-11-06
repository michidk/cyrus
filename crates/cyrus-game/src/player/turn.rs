//! Ordinary ground-turn arithmetic from redguard-preservation's
//! `docs/engine/player.md`. Eligibility and the simulation clock are separate.
use cyrus_script::AnimationGroup;
use cyrus_world::heading::wrap;

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

    /// Modern steering: hold the one side whose [`Self::delta`] at `frame_scale` turns
    /// `heading` toward `target` this update, by at most `limit` scaled.
    pub fn steer(&mut self, heading: u16, target: u16, limit: u32, frame_scale: u32) {
        let half = i64::from(cyrus_world::heading::HALF_TURN);
        // Positive turns right, which lowers the heading (see [`turned`]).
        let difference = wrap(i64::from(heading) - i64::from(target) + half) - half;
        let speed = ((difference.unsigned_abs() << 8) / u64::from(frame_scale.max(1)))
            .min(u64::from(limit));
        (self.left, self.right) = if difference > 0 {
            (0, speed)
        } else {
            (speed, 0)
        };
    }
}

/// Whether a direction's accumulator adds the player's fight-turn attributes (accel up to
/// speed) instead of SYSTEM.INI's `turn_speed` up to `turn_max_speed`: the player holds a weapon
/// (the shared weapon predicate) and plays a group other than `anim_run` or `anim_fight_run`, or its hand item is 51.
pub fn fight_values_selected(
    holding: bool,
    group: Option<AnimationGroup>,
    hand_item: Option<u8>,
) -> bool {
    (holding && !matches!(group, Some(AnimationGroup::RUN | AnimationGroup::FIGHT_RUN)))
        || hand_item == Some(51)
}

/// Heading after subtracting a signed turn amount, wrapped to 2048.
pub fn turned(heading: u16, delta: i64) -> u16 {
    wrap(i64::from(heading) - delta) as u16
}

/// Roll after one ordinary turn update, in 1/2048 turns.
///
/// While `running` (current `anim_run` or `anim_run_torch`) a left turn banks upward by 4 and a
/// right turn downward by 4, both stopping at 32 either side of zero; 8 per update
/// on the far side. Otherwise roll relaxes toward zero by 8. `delta` is the signed
/// amount from [`GroundTurn::delta`]; zero relaxes. Eligibility is the caller's.
pub fn lean(roll: u16, delta: i64, running: bool) -> u16 {
    let wrap_roll = |value: i32| wrap(value) as u16;
    let roll = i32::from(roll);
    if running && delta < 0 && !(0x20..=0x400).contains(&roll) {
        return wrap_roll(roll + if roll > 0x400 { 8 } else { 4 });
    }
    if running && delta > 0 && !(0x400..=0x7e0).contains(&roll) {
        return wrap_roll(roll - if roll < 0x400 { 8 } else { 4 });
    }
    if running && delta != 0 {
        return wrap_roll(roll);
    }
    relax(wrap_roll(roll))
}

/// Roll relaxing toward zero by 8 per update, snapping to zero inside one step.
pub fn relax(roll: u16) -> u16 {
    let roll = i32::from(roll);
    if roll > 0x400 {
        if roll > 0x7f8 {
            0
        } else {
            wrap(roll + 8) as u16
        }
    } else if roll > 0 {
        if roll < 8 { 0 } else { (roll - 8) as u16 }
    } else {
        0
    }
}

/// Inputs of one ordinary keyboard turn update for the sheathed grounded player.
#[derive(Clone, Copy, Debug)]
pub struct GroundTurnInput {
    /// Scaled right minus scaled left amount, from [`GroundTurn::delta`].
    pub delta: i64,
    /// The current animation group is `anim_run` or `anim_run_torch`.
    pub running: bool,
    pub torch: bool,
    /// The walk key (installed key_a) is held.
    pub walk_key: bool,
    /// Forward or backward is held.
    pub forward_or_backward: bool,
    /// Living state 0 or 27 with the falling and queued-motion bytes zero.
    pub eligible: bool,
    /// The player holds a drawn weapon with a combat record, or stands in deep water: the
    /// heading still turns but no turn animation is requested.
    pub animation_barred: bool,
}

/// Heading, roll and turn animation group chosen by one keyboard turn update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundTurnResult {
    pub heading: u16,
    pub roll: u16,
    pub group: Option<AnimationGroup>,
}

/// The keyboard turn rule of preservation's player.md "Ground-turn eligibility and animation".
/// The held walk key turns only with forward or backward, silently and without banking.
pub fn ground_turn(heading: u16, roll: u16, input: GroundTurnInput) -> GroundTurnResult {
    let (turns, animates) = if input.walk_key {
        (input.eligible && input.forward_or_backward, false)
    } else {
        (input.eligible, true)
    };
    let roll = if turns && animates {
        if input.running {
            lean(roll, input.delta, true)
        } else if input.delta == 0 {
            // The no-turn branch relaxes, then the general relaxation after it runs too.
            relax(relax(roll))
        } else {
            relax(roll)
        }
    } else if input.running {
        roll
    } else {
        relax(roll)
    };
    GroundTurnResult {
        heading: if turns {
            turned(heading, input.delta)
        } else {
            heading
        },
        roll,
        group: (turns && animates && !input.animation_barred && input.delta != 0).then_some(
            match (input.torch, input.delta > 0) {
                (false, false) => AnimationGroup::TURN_LEFT,
                (false, true) => AnimationGroup::TURN_RIGHT,
                (true, false) => AnimationGroup::TURN_LEFT_TORCH,
                (true, true) => AnimationGroup::TURN_RIGHT_TORCH,
            },
        ),
    }
}
