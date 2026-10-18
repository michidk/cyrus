//! Nonplayer decision-stage executors. The pure state machines follow the original's attack
//! executor; callers own the animation request and the combat record fields they pass in.
use super::combat::CombatDecision;
use anyhow::{Context, Result, ensure};

/// Signed difference of two headings, each masked to 0..2047 and wrapped into -1024..=1024.
pub fn heading_difference(first: i32, second: i32) -> i32 {
    let mut difference = (first & 0x7ff) - (second & 0x7ff);
    if difference > 0x400 {
        difference -= 0x800;
    } else if difference < -0x400 {
        difference += 0x800;
    }
    difference
}

/// The facing test of the counter-attack selection: the target's heading minus the actor's is
/// more than 0x300 from zero, so the two face each other.
pub fn facing_each_other(target_heading: i32, own_heading: i32) -> bool {
    heading_difference(target_heading, own_heading).abs() > 0x300
}

/// The same-facing test of the defence selection: the headings differ by less than 0x100.
pub fn facing_alike(own_heading: i32, target_heading: i32) -> bool {
    heading_difference(own_heading, target_heading).abs() < 0x100
}

/// Facts read from the first motion of one populated attack slot for a defence pick. Timing values are nominal animation frames;
/// range values use the same signed distance units as the actor-to-target distance query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefenceCandidate {
    pub min_range: i32,
    pub max_range: i32,
    pub near_timing: i32,
    /// Authored one-based counter-attack reference, before wrapping subtraction.
    pub counter_attack: i32,
}

impl DefenceCandidate {
    /// A defence admits only distances strictly inside its authored range window.
    pub fn usable_at(self, distance: i32) -> bool {
        distance > self.min_range && distance < self.max_range
    }
}

/// Best usable defence kind from nine slots, following preservation's
/// [picker rules](https://github.com/michidk/redguard-preservation/pull/112).
/// Slots six through eight return kind four; equal timings retain the first slot.
/// The caller supplies the original actor-to-target distance query result.
pub fn best_defence(slots: &[Option<DefenceCandidate>; 9], distance: i32) -> Option<i32> {
    pick_defence(slots, distance, None)
}

/// Best usable defence excluding the receiver's current counter-attack index.
/// `current_counter_attack` is already zero-based, as returned by the current-move query.
pub fn open_defence(
    slots: &[Option<DefenceCandidate>; 9],
    distance: i32,
    current_counter_attack: i32,
) -> Option<i32> {
    pick_defence(slots, distance, Some(current_counter_attack))
}

fn pick_defence(
    slots: &[Option<DefenceCandidate>; 9],
    distance: i32,
    excluded_counter_attack: Option<i32>,
) -> Option<i32> {
    let mut best_timing = 65536;
    let mut best_kind = None;
    for (slot, candidate) in slots.iter().enumerate() {
        let Some(candidate) = candidate else { continue };
        if candidate.usable_at(distance)
            && excluded_counter_attack != Some(candidate.counter_attack.wrapping_sub(1))
            && candidate.near_timing < best_timing
        {
            best_timing = candidate.near_timing;
            best_kind = Some(if slot >= 6 { 4 } else { slot as i32 });
        }
    }
    best_kind
}

/// One slot's paced-selection facts. Geometry queries must use this receiver and target.
#[derive(Clone, Copy, Debug)]
pub struct PacedDefenceCandidate {
    pub defence: DefenceCandidate,
    /// Signed accumulated attack damage read by the same-facing/unarmed-target branch.
    pub damage: i32,
    pub in_arc: bool,
    pub in_reach: bool,
}

/// Shared values sampled for one paced pick. The random draw is inclusive 0..=32767.
#[derive(Clone, Copy, Debug)]
pub struct PacedDefenceInput {
    pub distance: i32,
    pub random_draw: u16,
    pub pacing_sum: i32,
    pub facing_alike: bool,
    pub target_in_combat_stance: bool,
    pub attribute_58: u8,
}

/// Paced defence choice following preservation's
/// [picker rules](https://github.com/michidk/redguard-preservation/pull/112).
/// Returns zero even when no slot qualifies. The armed, differently-facing branch
/// updates the counter and timing threshold without replacing the fallback choice.
/// Divisor admission is required only when a candidate consumes a remainder.
pub fn paced_defence(
    decision: &mut CombatDecision,
    slots: &[Option<PacedDefenceCandidate>; 9],
    input: PacedDefenceInput,
) -> Result<i32> {
    ensure!(
        input.random_draw <= 32767,
        "paced defence: random draw exceeds 32767"
    );
    let fallback = usize::from(input.random_draw) * 6 / 32767;
    let mut chosen = if slots[fallback].is_some_and(|slot| slot.defence.usable_at(input.distance)) {
        fallback as i32
    } else {
        0
    };
    let mut damage = 0;
    let mut near_timing = 65536;
    for (index, slot) in slots.iter().enumerate().take(6) {
        let Some(slot) = slot else { continue };
        if !slot.defence.usable_at(input.distance) || !slot.in_arc || !slot.in_reach {
            continue;
        }
        if input.facing_alike || !input.target_in_combat_stance {
            if slot.damage > damage
                && input
                    .pacing_sum
                    .checked_rem(decision.defence_pacing)
                    .context("paced defence: invalid defence pacing divisor")?
                    == 0
            {
                damage = slot.damage;
                chosen = index as i32;
            }
        } else {
            decision.defence_counter = 129 - i32::from(input.attribute_58);
            if slot.defence.near_timing < near_timing
                && input
                    .pacing_sum
                    .checked_rem(decision.defence_pacing)
                    .context("paced defence: invalid defence pacing divisor")?
                    == 0
            {
                near_timing = slot.defence.near_timing;
            }
        }
    }
    Ok(chosen)
}

/// One coordinate of the combat bounds query, in signed fixed-point position units.
/// Extents are integer model units. Arithmetic wraps before division toward zero.
pub fn combat_bounds_coordinate(position: i32, negative_extent: i32, positive_extent: i32) -> i32 {
    position
        .wrapping_sub(negative_extent.wrapping_shl(8))
        .wrapping_add(positive_extent.wrapping_shl(8))
        / 2
}

/// Elevation admission from the first motion of a receiver-owned attack slot.
/// The y inputs are bounds-query coordinates, not actor origins. The x extents
/// are integer model units. Follows preservation's
/// [defence geometry rules](https://github.com/michidk/redguard-preservation/pull/112).
pub fn defence_elevation(
    elevation: i32,
    own_bounds_y: i32,
    target_bounds_y: i32,
    positive_x: i32,
    negative_x: i32,
) -> bool {
    let delta = target_bounds_y.wrapping_sub(own_bounds_y);
    let width = positive_x
        .wrapping_shl(8)
        .wrapping_add(negative_x.wrapping_shl(8));
    let half = width / 2;
    let outer = width.wrapping_add(half);
    let required = if half.wrapping_neg() > delta && outer.wrapping_neg() < delta {
        1
    } else if delta > half && outer > delta {
        2
    } else {
        0
    };
    elevation == required
}

/// Strict arc admission from a first motion. Headings use 2048 units per turn.
/// The host supplies the bearing of the wrapping target-minus-receiver position
/// vector after each coordinate is arithmetic-shifted right eight bits.
pub fn defence_arc(own_heading: i32, bearing: i32, arc: i32, arc_center: i32) -> bool {
    let angle = heading_difference(own_heading, bearing).abs();
    let center = arc_center.wrapping_add(512);
    center.wrapping_add(arc) > angle && center.wrapping_sub(arc) < angle
}

/// Receiver facts consumed by one defence starter. Slot presence refers to the
/// requested kind's attack slot. Weapon queries are needed only for kinds four/eight.
#[derive(Clone, Copy, Debug)]
pub struct DefenceStartInput {
    pub slot_present: bool,
    pub selected_player: bool,
    pub cooldown: u32,
    pub drawing: bool,
    pub sheathing: bool,
    pub attribute_16: u8,
}

/// Start an admitted defence synchronously, following preservation's
/// [starter rules](https://github.com/michidk/redguard-preservation/pull/112).
/// Selected-player kind four calls movement with attribute 16 shifted eight bits
/// and mode one before either selector write. Movement failure is an unsupported
/// host error; a supported movement result never gates admission. The executor
/// receives request zero and may change both selectors before this call returns.
pub fn start_defence(
    kind: i32,
    input: DefenceStartInput,
    selected: &mut i32,
    defence_selector: &mut i32,
    movement: impl FnOnce(i32, i32) -> Result<()>,
    execute: impl FnOnce(&mut i32, &mut i32, i32) -> Result<()>,
) -> Result<bool> {
    if !input.slot_present {
        return Ok(false);
    }
    let admitted = match kind {
        0 | 3 | 5 | 6 => *selected == -1,
        1 | 2 => !input.selected_player || *selected == kind - 1,
        4 | 8 => *selected == -1 && input.cooldown == 0 && !input.drawing && !input.sheathing,
        _ => false,
    };
    if !admitted {
        return Ok(false);
    }
    if kind == 4 && input.selected_player {
        movement(i32::from(input.attribute_16) << 8, 1)?;
    }
    *defence_selector = kind;
    *selected = kind;
    execute(selected, defence_selector, 0)?;
    Ok(true)
}

/// What the attack executor asks its caller to do besides updating the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackEffect {
    /// Nothing beyond the record changes.
    None,
    /// Request `group` with flag zero, then mark the animation record: set its flag byte at
    /// offset 15 to one and its word at offset 2 to 0xffff.
    Start { group: i32 },
}

/// Result of one attack executor invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackStep {
    /// True while an attack is in progress: the state is nonzero and a move is pending.
    pub active: bool,
    pub effect: AttackEffect,
}

/// The attack executor (`move` is the requested move, -1 for cleanup).
///
/// `selected` is the selected move index (the action selector), `pending` the pending move
/// index, `current_group` the actor's current animation group and `pending_group` the animation
/// group of the pending move's attack record. Idle with a move requested and the actor not in the
/// pending move's group, it starts the attack: state 2 and the aggression budget drops by 10.
/// While attacking, once the actor leaves the pending move's group the attack ends: state 0, a
/// selected index of 9 resets to -1 and the pending move clears.
pub fn attack_step(
    decision: &mut CombatDecision,
    selected: &mut i32,
    pending: &mut i32,
    requested: i32,
    current_group: Option<i32>,
    pending_group: impl FnOnce() -> Option<i32>,
) -> Result<AttackStep> {
    let mut effect = AttackEffect::None;
    if decision.attack_state == 0 {
        if requested != -1 {
            let group = pending_group();
            ensure!(
                group.is_some(),
                "attack executor: pending move {pending} has no attack record"
            );
            if current_group != group {
                effect = AttackEffect::Start {
                    group: group.expect("checked"),
                };
                decision.attack_state = 2;
                decision.aggression = decision.aggression.wrapping_sub(10);
            }
        }
    } else if *pending != -1 {
        let group = pending_group();
        ensure!(
            group.is_some(),
            "attack executor: pending move {pending} has no attack record"
        );
        if current_group != group {
            decision.attack_state = 0;
            if *selected == 9 {
                *selected = -1;
            }
            *pending = -1;
        }
    }
    Ok(AttackStep {
        active: decision.attack_state != 0 && *pending != -1,
        effect,
    })
}

/// What the defence executor asks its caller to do besides updating the record and flag byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefenceEffect {
    /// Request this animation group with flag zero.
    pub request_group: Option<i32>,
    /// Mark the animation record: its flag byte at offset 15 becomes one.
    pub mark_animation: bool,
    /// The original calls an undecoded move-bookkeeping helper (with argument zero) here.
    pub bookkeeping: bool,
    /// The taunt request of kind one follows a defence start.
    pub taunt_kind_one: bool,
}

/// Receiver and move-table facts the defence executor reads. The closures answer for a move
/// selector value because the executor advances the selector between reads.
pub struct DefenceView<'a> {
    pub current_group: Option<i32>,
    /// The animation record's pending group.
    pub pending_group: Option<i32>,
    /// Animation group of the selected defence for a move selector.
    pub defence_group: &'a dyn Fn(i32) -> Option<i32>,
    /// Whether the selected defence's chain entry for a move selector has type one.
    pub entry_type_one: &'a dyn Fn(i32) -> bool,
}

/// Result of one defence executor invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefenceStep {
    /// True while a defence is in progress: the state is nonzero, a defence selector is set and
    /// the selected index is not 9.
    pub active: bool,
    pub effect: DefenceEffect,
}

/// Sets the two-bit intent field of the combat flag byte to its current value or one.
fn raise_intent(flags: &mut u8) {
    *flags = (*flags & 0xfc) | ((*flags & 3) | 1);
}

/// Ends a defence: state 0, the defence selector -1 and a selected index other than 9 reset.
fn end_defence(decision: &mut CombatDecision, selected: &mut i32, defence_selector: &mut i32) {
    decision.attack_state = 0;
    *defence_selector = -1;
    if *selected != 9 {
        *selected = -1;
    }
}

/// The defence executor (`requested` is the requested defence, -1 for cleanup). `flags` is the
/// combat flag byte, `defence_selector` the second selector and `selected` the selected index.
///
/// Idle with a request, it resets the move selector and, when the actor is not in the defence's
/// group, starts it: state 1, the aggression budget drops by 10, the intent field gains bit one
/// and the target mark (bit `0x10`) clears. While defending it follows the chain of up to three
/// moves, ending the defence when the actor leaves the expected group.
pub fn defence_step(
    decision: &mut CombatDecision,
    flags: &mut u8,
    selected: &mut i32,
    defence_selector: &mut i32,
    requested: i32,
    view: &DefenceView<'_>,
) -> Result<DefenceStep> {
    let mut effect = DefenceEffect::default();
    let group_for = |selector: i32| -> Result<i32> {
        (view.defence_group)(selector)
            .ok_or_else(|| anyhow::anyhow!("defence executor: no group for move {selector}"))
    };
    let mut start = |decision: &mut CombatDecision, flags: &mut u8, group: i32| {
        effect = DefenceEffect {
            request_group: Some(group),
            mark_animation: true,
            bookkeeping: true,
            taunt_kind_one: false,
        };
        *flags &= !0x10;
        raise_intent(flags);
        decision.aggression = decision.aggression.wrapping_sub(10);
    };
    if decision.attack_state == 0 {
        if requested != -1 {
            decision.move_selector = 0;
            let group = group_for(0)?;
            if view.current_group != Some(group) {
                start(decision, flags, group);
                decision.attack_state = 1;
                effect.taunt_kind_one = true;
            }
        }
    } else if *defence_selector != -1 {
        let expected = group_for(decision.move_selector)?;
        if view.current_group != Some(expected) {
            if decision.move_selector + 1 < 3 && (view.entry_type_one)(decision.move_selector) {
                decision.move_selector += 1;
                let next = group_for(decision.move_selector)?;
                if *flags & 0x10 != 0 && matches!(next, 0x6b | 0x6c) {
                    end_defence(decision, selected, defence_selector);
                } else {
                    start(decision, flags, next);
                }
            } else if view.pending_group != Some(expected) {
                end_defence(decision, selected, defence_selector);
            }
        }
    }
    Ok(DefenceStep {
        active: decision.attack_state != 0 && *defence_selector != -1 && *selected != 9,
        effect,
    })
}

/// Facts the counter-attack selection reads, sampled by the caller from the combat records.
#[derive(Clone, Copy, Debug)]
pub struct CounterAttackInput {
    /// The actor is armed (weapon drawn, not sheathing).
    pub armed: bool,
    /// The distance to the combat target and the target's two window bounds.
    pub distance: i32,
    pub target_window: (i32, i32),
    /// The facing and range test of the actor against its target passed.
    pub facing_ok: bool,
    /// The target's and the actor's selected move indices.
    pub target_selected: i32,
    pub own_selected: i32,
    /// The actor has an attack in its first four slots.
    pub has_attack: bool,
    /// The target already carries the attacked mark (flag `0x10`).
    pub target_marked: bool,
    /// Frames until the target's selected move reaches its near and far range values.
    pub gap_near: i32,
    pub gap_far: i32,
    /// The counter attack index the target's selected move names, and that attack's ranges.
    pub counter_index: i32,
    pub attack_min: i32,
    pub attack_max: i32,
    /// The actor's per-actor value plus the shared tick counter, and the attack pacing divisor.
    pub pacing_sum: i32,
    pub attack_pacing: i32,
    /// The target's second selector.
    pub target_selector: i32,
}

/// A sideways or backward step the selection falls back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sidestep {
    Right,
    Left,
    Back,
}

/// Outcome of the counter-attack selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterAttack {
    /// Nothing is chosen and nothing moves.
    Idle,
    /// Attack with the move `index`: the intent field becomes two, the pending move is set,
    /// the attack starts and the target gains the attacked mark.
    Attack { index: i32 },
    /// The window did not fit: step as given (move one, with argument one).
    Sidestep(Sidestep),
}

/// The counter-attack selection. `draw` yields the original's random values (zero through 0x7fff).
///
/// With the pacing remainder at most one, the chosen attack must fit both gaps: its minimum range
/// below the far gap (or the far gap not positive), its reach (minimum plus maximum) beyond both
/// gaps and its minimum below the near gap. A miss steps sideways when the target's second
/// selector is 3 or 4 (right on an even pacing sum, left on an odd one) and backward otherwise.
/// With a larger remainder two random extensions of zero through four must keep the minimum plus
/// the first extension below the near gap and the near gap plus the second below the reach,
/// compared unsigned; a miss does nothing.
pub fn counter_attack(
    input: &CounterAttackInput,
    mut draw: impl FnMut() -> i32,
) -> Result<CounterAttack> {
    let i = input;
    if !i.armed
        || i.distance <= i.target_window.0
        || i.distance >= i.target_window.1
        || !i.facing_ok
        || matches!(i.target_selected, -1 | 9)
        || i.own_selected != -1
        || !i.has_attack
        || i.target_marked
    {
        return Ok(CounterAttack::Idle);
    }
    ensure!(
        i.attack_pacing != 0,
        "counter attack: pacing divisor is zero"
    );
    let reach = i.attack_min.wrapping_add(i.attack_max);
    let attack = CounterAttack::Attack {
        index: i.counter_index,
    };
    if i.pacing_sum % i.attack_pacing <= 1 {
        let fits = (i.attack_min < i.gap_far || i.gap_far <= 0)
            && reach > i.gap_far
            && i.attack_min < i.gap_near
            && reach > i.gap_near;
        if fits {
            return Ok(attack);
        }
        return Ok(CounterAttack::Sidestep(
            if matches!(i.target_selector, 3 | 4) {
                if i.pacing_sum % 2 == 0 {
                    Sidestep::Right
                } else {
                    Sidestep::Left
                }
            } else {
                Sidestep::Back
            },
        ));
    }
    let mut extension = || -> u32 {
        let scaled = (draw() as u32).wrapping_mul(i.attack_pacing as u32) / 0x7fff;
        scaled.wrapping_mul(5) / (i.attack_pacing as u32)
    };
    let first = extension();
    if (i.attack_min as u32).wrapping_add(first) >= i.gap_near as u32 {
        return Ok(CounterAttack::Idle);
    }
    let second = extension();
    if (i.gap_near as u32).wrapping_add(second) >= reach as u32 {
        return Ok(CounterAttack::Idle);
    }
    Ok(attack)
}

/// Facts the defence selection reads, sampled by the caller from the combat records.
#[derive(Clone, Copy, Debug)]
pub struct DefenceSelectionInput {
    /// The actor is armed and the global lockout word is clear.
    pub armed: bool,
    pub lockout_clear: bool,
    pub own_selected: i32,
    /// The target is inside the actor's forward arc, and the distance is below the actor's own
    /// window bound.
    pub target_in_arc: bool,
    pub distance: i32,
    pub own_window_far: i32,
    /// The headings differ by less than 256 heading units.
    pub facing_alike: bool,
    /// Combat stance is the armed predicate or combat flag mask 0x04.
    pub target_in_combat_stance: bool,
    /// Actor attribute 58, used when facing differs and the target is in combat stance.
    pub attribute_58: u8,
    /// Per-actor value plus shared tick counter, with signed 32-bit wrapping addition.
    pub pacing_sum: i32,
    /// The defence pacing divisor, required only on the admitted no-move branch.
    pub defence_pacing: i32,
    pub target_selected: i32,
    /// The target's current animation frame and its selected move's near and third range values.
    pub target_frame: i32,
    pub target_near: i32,
    pub target_third: i32,
    /// The actor's defence selector.
    pub own_selector: i32,
}

/// A defence picker reads the current decision state and combat flags.
pub type DefencePicker<'a> = &'a mut dyn FnMut(&CombatDecision, u8) -> Option<i32>;

/// Synchronous defence starts may update the decision state and combat flags.
pub type DefenceStarter<'a> = &'a mut dyn FnMut(&mut CombatDecision, &mut u8, i32);

/// Host services used by the selection. A start completes before the next picker runs.
pub struct DefencePickers<'a> {
    /// The best usable defence.
    pub best: DefencePicker<'a>,
    /// An open defence.
    pub open: DefencePicker<'a>,
    /// The paced defence.
    pub paced: DefencePicker<'a>,
    /// Whether the chosen defence kind is within reach.
    pub in_reach: &'a mut dyn FnMut(i32) -> bool,
    /// Apply the defence starter immediately, including its decision and flag changes.
    pub start: DefenceStarter<'a>,
}

/// Select and synchronously start defences using preservation's
/// [defence-selection rules](https://github.com/michidk/redguard-preservation/pull/112).
///
/// Armed, with the lockout clear and no move selected, it sets the defence counter to one when
/// the two face alike or the target is outside combat stance, and to `129 - attribute 58`
/// otherwise. On a zero pacing remainder it picks a defence when the target is mid-move and its swing has passed the
/// near plus third range values (the best usable one) or when the target's selected move is
/// exactly 9 (an open one), starting it when it is in reach; the intent field gains bit one in
/// the first case. Independently, on a zero remainder of the defence counter it raises the
/// intent field and starts the paced pick when in reach. With a move already selected, a zero
/// counter remainder starts kind one while the defence selector is zero and kind two while it is one.
pub fn defence_selection(
    decision: &mut CombatDecision,
    flags: &mut u8,
    input: &DefenceSelectionInput,
    pickers: DefencePickers<'_>,
) -> Result<()> {
    let i = input;
    if !i.armed || !i.lockout_clear {
        return Ok(());
    }
    if i.own_selected == -1 {
        if !i.target_in_arc || i.distance >= i.own_window_far {
            return Ok(());
        }
        decision.defence_counter = if i.facing_alike || !i.target_in_combat_stance {
            1
        } else {
            129 - i32::from(i.attribute_58)
        };
        if i.pacing_sum
            .checked_rem(i.defence_pacing)
            .context("defence selection: invalid pacing divisor")?
            == 0
        {
            if !matches!(i.target_selected, -1 | 9) {
                if i.target_frame > i.target_near.wrapping_add(i.target_third) {
                    raise_intent(flags);
                    if let Some(kind) = (pickers.best)(decision, *flags)
                        && (pickers.in_reach)(kind)
                    {
                        (pickers.start)(decision, flags, kind);
                    }
                }
            } else if i.target_selected == 9
                && let Some(kind) = (pickers.open)(decision, *flags)
                && (pickers.in_reach)(kind)
            {
                (pickers.start)(decision, flags, kind);
            }
        }
        if i.pacing_sum
            .checked_rem(decision.defence_counter)
            .context("defence selection: invalid defence counter")?
            == 0
        {
            raise_intent(flags);
            if let Some(kind) = (pickers.paced)(decision, *flags)
                && (pickers.in_reach)(kind)
            {
                (pickers.start)(decision, flags, kind);
            }
        }
    } else {
        if i.pacing_sum
            .checked_rem(decision.defence_counter)
            .context("defence selection: invalid defence counter")?
            == 0
        {
            match i.own_selector {
                0 => {
                    raise_intent(flags);
                    (pickers.start)(decision, flags, 1);
                }
                1 => {
                    raise_intent(flags);
                    (pickers.start)(decision, flags, 2);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn defence_geometry_uses_bounds_centres_and_strict_band_edges() {
        assert_eq!(super::combat_bounds_coordinate(101, 3, 1), -205);
        assert_eq!(
            super::combat_bounds_coordinate(i32::MAX, -1, 1),
            -1073741568
        );
        assert!(super::defence_elevation(0, 0, -6400, 25, 25));
        assert!(super::defence_elevation(1, 0, -6401, 25, 25));
        assert!(super::defence_elevation(0, 0, -19200, 25, 25));
        assert!(super::defence_elevation(2, 0, 6401, 25, 25));
        assert!(super::defence_elevation(0, 0, 19200, 25, 25));
        assert!(!super::defence_arc(0, 448, 64, 0));
        assert!(super::defence_arc(0, 449, 64, 0));
        assert!(super::defence_arc(2047, 511, 64, 0));
        assert!(!super::defence_arc(0, 576, 64, 0));
    }

    #[test]
    fn defence_start_runs_the_executor_before_returning() {
        let mut decision = CombatDecision::default();
        let (mut selected, mut selector, mut flags) = (-1, -1, 0x10);
        let input = super::DefenceStartInput {
            slot_present: true,
            selected_player: false,
            cooldown: 0,
            drawing: false,
            sheathing: false,
            attribute_16: 18,
        };
        let mut effect = None;
        assert!(
            super::start_defence(
                3,
                input,
                &mut selected,
                &mut selector,
                |_, _| panic!("NPC start must not move the player"),
                |selected, selector, requested| {
                    assert_eq!((*selected, *selector, requested), (3, 3, 0));
                    effect = Some(
                        defence_step(
                            &mut decision,
                            &mut flags,
                            selected,
                            selector,
                            requested,
                            &view(None, None, &|_| Some(97), &|_| false),
                        )?
                        .effect,
                    );
                    Ok(())
                },
            )
            .unwrap()
        );
        assert_eq!(
            (
                selected,
                selector,
                decision.attack_state,
                decision.aggression,
                flags
            ),
            (3, 3, 1, 40, 1)
        );
        assert_eq!(effect.unwrap().request_group, Some(97));
        assert!(
            !super::start_defence(
                0,
                input,
                &mut selected,
                &mut selector,
                |_, _| panic!("rejected start must not move"),
                |_, _, _| panic!("rejected start must not execute"),
            )
            .unwrap()
        );
    }

    #[test]
    fn player_defence_movement_precedes_executor_and_errors_stop_before_writes() {
        let input = super::DefenceStartInput {
            slot_present: true,
            selected_player: true,
            cooldown: 0,
            drawing: false,
            sheathing: false,
            attribute_16: 18,
        };
        let (mut selected, mut selector) = (-1, -77);
        let moved = std::cell::Cell::new(false);
        assert!(
            super::start_defence(
                4,
                input,
                &mut selected,
                &mut selector,
                |distance, mode| {
                    assert_eq!((distance, mode), (4608, 1));
                    moved.set(true);
                    Ok(())
                },
                |selected, selector, requested| {
                    assert!(moved.get());
                    assert_eq!((*selected, *selector, requested), (4, 4, 0));
                    *selected = -1;
                    Ok(())
                },
            )
            .unwrap()
        );
        assert_eq!((selected, selector), (-1, 4));
        assert!(
            super::start_defence(
                4,
                input,
                &mut selected,
                &mut selector,
                |_, _| anyhow::bail!("movement unavailable"),
                |_, _, _| panic!("unavailable movement must stop before executor"),
            )
            .is_err()
        );
        assert_eq!((selected, selector), (-1, 4));
    }

    fn paced_slot(damage: i32) -> super::PacedDefenceCandidate {
        super::PacedDefenceCandidate {
            defence: super::DefenceCandidate {
                min_range: 0,
                max_range: 100,
                near_timing: 10,
                counter_attack: 0,
            },
            damage,
            in_arc: true,
            in_reach: true,
        }
    }

    fn paced_input() -> super::PacedDefenceInput {
        super::PacedDefenceInput {
            distance: 50,
            random_draw: 0,
            pacing_sum: 10,
            facing_alike: true,
            target_in_combat_stance: true,
            attribute_58: 29,
        }
    }

    #[test]
    fn paced_defence_maximum_draw_can_retain_slot_six_without_geometry_admission() {
        let mut slots = [None; 9];
        slots[6] = Some(super::PacedDefenceCandidate {
            in_arc: false,
            in_reach: false,
            ..paced_slot(1)
        });
        let mut decision = super::CombatDecision::default();
        assert_eq!(
            super::paced_defence(
                &mut decision,
                &slots,
                super::PacedDefenceInput {
                    random_draw: 32767,
                    ..paced_input()
                }
            )
            .unwrap(),
            6
        );
        assert_eq!(decision.defence_counter, 0);
    }

    #[test]
    fn paced_defence_priority_branch_keeps_the_first_highest_positive_priority() {
        let mut slots = [None; 9];
        slots[0] = Some(paced_slot(-1));
        slots[1] = Some(paced_slot(3));
        slots[2] = Some(paced_slot(3));
        slots[3] = Some(paced_slot(2));
        let mut decision = super::CombatDecision {
            defence_pacing: 5,
            ..Default::default()
        };
        assert_eq!(
            super::paced_defence(&mut decision, &slots, paced_input()).unwrap(),
            1
        );
        assert_eq!(
            super::paced_defence(
                &mut decision,
                &slots,
                super::PacedDefenceInput {
                    pacing_sum: 9,
                    ..paced_input()
                }
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn paced_defence_armed_branch_changes_counter_but_retains_random_choice() {
        let mut slots = [None; 9];
        slots[1] = Some(paced_slot(3));
        slots[6] = Some(paced_slot(1));
        let mut decision = super::CombatDecision {
            defence_pacing: 5,
            ..Default::default()
        };
        for pacing_sum in [9, 10] {
            assert_eq!(
                super::paced_defence(
                    &mut decision,
                    &slots,
                    super::PacedDefenceInput {
                        random_draw: 32767,
                        facing_alike: false,
                        pacing_sum,
                        ..paced_input()
                    }
                )
                .unwrap(),
                6
            );
            assert_eq!(decision.defence_counter, 100);
        }
        assert_eq!(
            super::paced_defence(
                &mut decision,
                &slots,
                super::PacedDefenceInput {
                    random_draw: 32767,
                    facing_alike: false,
                    target_in_combat_stance: false,
                    ..paced_input()
                }
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn paced_defence_checks_only_consumed_divisors_and_rejects_invalid_draws() {
        let mut slots = [None; 9];
        let mut decision = super::CombatDecision::default();
        assert_eq!(
            super::paced_defence(&mut decision, &slots, paced_input()).unwrap(),
            0
        );
        slots[1] = Some(paced_slot(0));
        assert!(super::paced_defence(&mut decision, &slots, paced_input()).is_ok());
        slots[1] = Some(paced_slot(1));
        assert!(super::paced_defence(&mut decision, &slots, paced_input()).is_err());
        decision.defence_pacing = -1;
        assert!(
            super::paced_defence(
                &mut decision,
                &slots,
                super::PacedDefenceInput {
                    pacing_sum: i32::MIN,
                    ..paced_input()
                }
            )
            .is_err()
        );
        assert!(
            super::paced_defence(
                &mut decision,
                &slots,
                super::PacedDefenceInput {
                    random_draw: 32768,
                    ..paced_input()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn defence_range_is_strict_and_signed() {
        let candidate = super::DefenceCandidate {
            min_range: -10,
            max_range: 10,
            near_timing: 0,
            counter_attack: 1,
        };
        assert!(!candidate.usable_at(-10));
        assert!(candidate.usable_at(-9));
        assert!(candidate.usable_at(9));
        assert!(!candidate.usable_at(10));
    }

    #[test]
    fn best_defence_retains_first_tie_and_rejects_timing_ceiling() {
        let candidate = super::DefenceCandidate {
            min_range: 0,
            max_range: 100,
            near_timing: 65536,
            counter_attack: 1,
        };
        let mut slots = [None; 9];
        assert_eq!(super::best_defence(&slots, 50), None);
        slots[0] = Some(candidate);
        assert_eq!(super::best_defence(&slots, 50), None);
        slots[2] = Some(super::DefenceCandidate {
            near_timing: -1,
            ..candidate
        });
        slots[3] = slots[2];
        assert_eq!(super::best_defence(&slots, 50), Some(2));
        assert_eq!(super::best_defence(&slots, 100), None);
    }

    #[test]
    fn extended_defence_slots_map_to_kind_four() {
        for slot in 6..9 {
            let mut slots = [None; 9];
            slots[slot] = Some(super::DefenceCandidate {
                min_range: 0,
                max_range: 100,
                near_timing: 1,
                counter_attack: 2,
            });
            assert_eq!(super::best_defence(&slots, 50), Some(4));
            assert_eq!(super::open_defence(&slots, 50, -1), Some(4));
        }
    }

    #[test]
    fn open_defence_excludes_current_counter_with_wrapping_subtraction() {
        let candidate = super::DefenceCandidate {
            min_range: 0,
            max_range: 100,
            near_timing: 0,
            counter_attack: i32::MIN,
        };
        let mut slots = [None; 9];
        slots[0] = Some(candidate);
        slots[5] = Some(super::DefenceCandidate {
            near_timing: 1,
            counter_attack: 3,
            ..candidate
        });
        assert_eq!(super::open_defence(&slots, 50, i32::MAX), Some(5));
        assert_eq!(super::open_defence(&slots, 50, 2), Some(0));
        slots[5] = None;
        assert_eq!(super::open_defence(&slots, 50, i32::MAX), None);
    }

    use super::*;

    fn run(
        decision: &mut CombatDecision,
        selected: &mut i32,
        pending: &mut i32,
        requested: i32,
        current: Option<i32>,
        group: i32,
    ) -> AttackStep {
        attack_step(decision, selected, pending, requested, current, || {
            Some(group)
        })
        .unwrap()
    }

    #[test]
    fn an_idle_request_outside_the_move_group_starts_the_attack_and_spends_the_budget() {
        let mut decision = CombatDecision::default();
        let (mut selected, mut pending) = (0, 3);
        let step = run(&mut decision, &mut selected, &mut pending, 0, Some(1), 120);
        assert_eq!(step.effect, AttackEffect::Start { group: 120 });
        assert!(step.active);
        assert_eq!((decision.attack_state, decision.aggression), (2, 40));
        assert_eq!((selected, pending), (0, 3));
    }

    #[test]
    fn an_idle_request_already_in_the_move_group_does_nothing() {
        let mut decision = CombatDecision::default();
        let (mut selected, mut pending) = (0, 3);
        let step = run(
            &mut decision,
            &mut selected,
            &mut pending,
            0,
            Some(120),
            120,
        );
        assert_eq!(step.effect, AttackEffect::None);
        assert!(!step.active);
        assert_eq!((decision.attack_state, decision.aggression), (0, 50));
    }

    #[test]
    fn cleanup_without_a_request_never_reads_the_attack_list() {
        let mut decision = CombatDecision::default();
        let (mut selected, mut pending) = (-1, -1);
        let step = attack_step(&mut decision, &mut selected, &mut pending, -1, None, || {
            panic!("no attack record is read")
        })
        .unwrap();
        assert!(!step.active);
    }

    #[test]
    fn an_attack_continues_while_the_actor_stays_in_the_move_group() {
        let mut decision = CombatDecision {
            attack_state: 2,
            ..Default::default()
        };
        let (mut selected, mut pending) = (9, 3);
        let step = run(
            &mut decision,
            &mut selected,
            &mut pending,
            -1,
            Some(120),
            120,
        );
        assert!(step.active);
        assert_eq!((decision.attack_state, selected, pending), (2, 9, 3));
    }

    #[test]
    fn leaving_the_move_group_ends_the_attack_and_clears_the_selection() {
        let mut decision = CombatDecision {
            attack_state: 2,
            ..Default::default()
        };
        let (mut selected, mut pending) = (9, 3);
        let step = run(&mut decision, &mut selected, &mut pending, -1, Some(1), 120);
        assert!(!step.active);
        assert_eq!((decision.attack_state, selected, pending), (0, -1, -1));
        // Another selected index survives the end of the attack.
        let mut decision = CombatDecision {
            attack_state: 2,
            ..Default::default()
        };
        let (mut selected, mut pending) = (4, 3);
        run(&mut decision, &mut selected, &mut pending, -1, Some(1), 120);
        assert_eq!((selected, pending), (4, -1));
    }

    #[test]
    fn a_missing_attack_record_is_an_error_not_a_guess() {
        let mut decision = CombatDecision::default();
        let (mut selected, mut pending) = (0, 3);
        assert!(
            attack_step(
                &mut decision,
                &mut selected,
                &mut pending,
                0,
                Some(1),
                || None
            )
            .is_err()
        );
    }

    fn view<'a>(
        current: Option<i32>,
        pending: Option<i32>,
        groups: &'a dyn Fn(i32) -> Option<i32>,
        chain: &'a dyn Fn(i32) -> bool,
    ) -> DefenceView<'a> {
        DefenceView {
            current_group: current,
            pending_group: pending,
            defence_group: groups,
            entry_type_one: chain,
        }
    }

    #[test]
    fn an_idle_defence_request_outside_the_group_starts_it() {
        let mut decision = CombatDecision {
            move_selector: 2,
            ..Default::default()
        };
        let (mut flags, mut selected, mut selector) = (0x13u8, 0, 0);
        let groups = |_: i32| Some(130);
        let chain = |_: i32| false;
        let step = defence_step(
            &mut decision,
            &mut flags,
            &mut selected,
            &mut selector,
            0,
            &view(Some(1), None, &groups, &chain),
        )
        .unwrap();
        assert_eq!(step.effect.request_group, Some(130));
        assert!(
            step.effect.mark_animation && step.effect.bookkeeping && step.effect.taunt_kind_one
        );
        assert_eq!(decision.attack_state, 1);
        assert_eq!(decision.move_selector, 0);
        assert_eq!(decision.aggression, 40);
        // The target mark clears and the intent field gains bit one: 0x13 -> 0x03.
        assert_eq!(flags, 0x03);
        // The starters set the defence selector before calling, so the defence is active.
        assert!(step.active);
    }

    #[test]
    fn an_idle_request_already_in_the_group_only_resets_the_move_selector() {
        let mut decision = CombatDecision {
            move_selector: 2,
            ..Default::default()
        };
        let (mut flags, mut selected, mut selector) = (0u8, 0, 0);
        let groups = |_: i32| Some(130);
        let chain = |_: i32| false;
        let step = defence_step(
            &mut decision,
            &mut flags,
            &mut selected,
            &mut selector,
            0,
            &view(Some(130), None, &groups, &chain),
        )
        .unwrap();
        assert_eq!(step.effect, DefenceEffect::default());
        assert_eq!((decision.attack_state, decision.move_selector), (0, 0));
    }

    #[test]
    fn an_active_defence_follows_the_chain_to_the_next_group() {
        let mut decision = CombatDecision {
            attack_state: 1,
            move_selector: 0,
            ..Default::default()
        };
        let (mut flags, mut selected, mut selector) = (0u8, 0, 0);
        let groups = |selector: i32| Some(130 + selector);
        let chain = |_: i32| true;
        let step = defence_step(
            &mut decision,
            &mut flags,
            &mut selected,
            &mut selector,
            -1,
            &view(Some(7), None, &groups, &chain),
        )
        .unwrap();
        assert_eq!(decision.move_selector, 1);
        assert_eq!(step.effect.request_group, Some(131));
        assert!(!step.effect.taunt_kind_one);
        assert_eq!(flags, 0x01);
        assert!(step.active);
    }

    #[test]
    fn being_attacked_in_the_last_chain_groups_ends_the_defence() {
        let mut decision = CombatDecision {
            attack_state: 1,
            move_selector: 0,
            ..Default::default()
        };
        let (mut flags, mut selected, mut selector) = (0x10u8, 4, 2);
        let groups = |selector: i32| Some(if selector == 0 { 130 } else { 0x6b });
        let chain = |_: i32| true;
        let step = defence_step(
            &mut decision,
            &mut flags,
            &mut selected,
            &mut selector,
            -1,
            &view(Some(7), None, &groups, &chain),
        )
        .unwrap();
        assert!(!step.active);
        assert_eq!((decision.attack_state, selector, selected), (0, -1, -1));
        assert_eq!(step.effect.request_group, None);
    }

    #[test]
    fn a_defence_without_a_chain_ends_when_the_pending_group_differs() {
        for (pending, ends) in [(Some(130), false), (Some(5), true)] {
            let mut decision = CombatDecision {
                attack_state: 1,
                move_selector: 2,
                ..Default::default()
            };
            let (mut flags, mut selected, mut selector) = (0u8, 9, 3);
            let groups = |_: i32| Some(130);
            let chain = |_: i32| true;
            let step = defence_step(
                &mut decision,
                &mut flags,
                &mut selected,
                &mut selector,
                -1,
                &view(Some(7), pending, &groups, &chain),
            )
            .unwrap();
            assert_eq!(decision.attack_state == 0, ends);
            // A selected index of 9 is never reset by ending a defence.
            assert_eq!(selected, 9);
            assert!(!step.active);
        }
    }

    fn counter_input() -> CounterAttackInput {
        CounterAttackInput {
            armed: true,
            distance: 500,
            target_window: (100, 900),
            facing_ok: true,
            target_selected: 2,
            own_selected: -1,
            has_attack: true,
            target_marked: false,
            gap_near: 6,
            gap_far: 12,
            counter_index: 3,
            attack_min: 4,
            attack_max: 20,
            pacing_sum: 10,
            attack_pacing: 10,
            target_selector: 0,
        }
    }

    fn never() -> i32 {
        panic!("no random draw expected")
    }

    #[test]
    fn the_counter_attack_needs_every_admission_gate() {
        let base = counter_input();
        for blocked in [
            CounterAttackInput {
                armed: false,
                ..base
            },
            CounterAttackInput {
                distance: 100,
                ..base
            },
            CounterAttackInput {
                distance: 900,
                ..base
            },
            CounterAttackInput {
                facing_ok: false,
                ..base
            },
            CounterAttackInput {
                target_selected: -1,
                ..base
            },
            CounterAttackInput {
                target_selected: 9,
                ..base
            },
            CounterAttackInput {
                own_selected: 0,
                ..base
            },
            CounterAttackInput {
                has_attack: false,
                ..base
            },
            CounterAttackInput {
                target_marked: true,
                ..base
            },
        ] {
            assert_eq!(
                counter_attack(&blocked, never).unwrap(),
                CounterAttack::Idle
            );
        }
        assert!(
            counter_attack(
                &CounterAttackInput {
                    attack_pacing: 0,
                    ..base
                },
                never
            )
            .is_err()
        );
    }

    #[test]
    fn a_fitting_window_attacks_with_the_counter_index() {
        // Remainder 0: min 4 < far 12, reach 24 beyond both gaps, min below the near gap.
        assert_eq!(
            counter_attack(&counter_input(), never).unwrap(),
            CounterAttack::Attack { index: 3 }
        );
        // A far gap that is not positive also satisfies the first test.
        assert_eq!(
            counter_attack(
                &CounterAttackInput {
                    gap_far: -2,
                    attack_min: 9,
                    gap_near: 20,
                    ..counter_input()
                },
                never
            )
            .unwrap(),
            CounterAttack::Attack { index: 3 }
        );
    }

    #[test]
    fn a_missed_window_sidesteps_by_the_targets_selector_and_pacing_parity() {
        let miss = CounterAttackInput {
            attack_min: 30,
            ..counter_input()
        };
        assert_eq!(
            counter_attack(&miss, never).unwrap(),
            CounterAttack::Sidestep(Sidestep::Back)
        );
        for (sum, selector, expected) in [
            (10, 3, Sidestep::Right),
            (10, 4, Sidestep::Right),
            (11, 3, Sidestep::Left),
            (11, 4, Sidestep::Left),
        ] {
            // Remainder 0 (sum 10) and 1 (sum 11) both take the first branch.
            let input = CounterAttackInput {
                pacing_sum: sum,
                target_selector: selector,
                ..miss
            };
            assert_eq!(
                counter_attack(&input, never).unwrap(),
                CounterAttack::Sidestep(expected)
            );
        }
    }

    #[test]
    fn a_larger_remainder_uses_two_bounded_random_extensions() {
        let base = CounterAttackInput {
            pacing_sum: 12,
            ..counter_input()
        };
        // Remainder 2: both draws zero give extensions of zero: 4 < 6 and 6 + 0 < 24.
        let mut zeros = [0, 0].into_iter();
        assert_eq!(
            counter_attack(&base, || zeros.next().unwrap()).unwrap(),
            CounterAttack::Attack { index: 3 }
        );
        // The largest draw gives an extension of four: 4 + 4 = 8 is not below the near gap 6.
        assert_eq!(
            counter_attack(&base, || 0x7fff).unwrap(),
            CounterAttack::Idle
        );
        // A reach no larger than the near gap plus the second extension idles.
        let short = CounterAttackInput {
            attack_max: 1,
            ..base
        };
        let mut zeros = [0, 0].into_iter();
        assert_eq!(
            counter_attack(&short, || zeros.next().unwrap()).unwrap(),
            CounterAttack::Idle
        );
    }

    #[test]
    fn heading_differences_wrap_into_a_half_turn() {
        assert_eq!(heading_difference(10, 2), 8);
        assert_eq!(heading_difference(2, 10), -8);
        assert_eq!(heading_difference(2040, 8), -16);
        assert_eq!(heading_difference(8, 2040), 16);
        assert_eq!(heading_difference(0x400, 0), 0x400);
        assert_eq!(heading_difference(0x401, 0), -0x3ff);
        // Values outside 0..2047 are masked first.
        assert_eq!(heading_difference(2048 + 5, 5), 0);
    }

    #[test]
    fn facing_tests_use_the_original_bands() {
        assert!(facing_each_other(0, 0x400));
        assert!(facing_each_other(0x301, 0));
        assert!(!facing_each_other(0x300, 0));
        assert!(!facing_each_other(0, 0));
        assert!(facing_alike(100, 100 + 0xff));
        assert!(!facing_alike(100, 100 + 0x100));
        assert!(facing_alike(2040, 8));
    }

    fn defence_input() -> DefenceSelectionInput {
        DefenceSelectionInput {
            armed: true,
            lockout_clear: true,
            own_selected: -1,
            target_in_arc: true,
            distance: 50,
            own_window_far: 400,
            facing_alike: false,
            target_in_combat_stance: true,
            attribute_58: 29,
            pacing_sum: 10,
            defence_pacing: 5,
            target_selected: 2,
            target_frame: 20,
            target_near: 6,
            target_third: 8,
            own_selector: 0,
        }
    }

    fn pick(
        decision: &mut CombatDecision,
        flags: &mut u8,
        input: &DefenceSelectionInput,
        best: Option<i32>,
        open: Option<i32>,
        paced: Option<i32>,
        reach: bool,
    ) -> Vec<i32> {
        let mut started = Vec::new();
        defence_selection(
            decision,
            flags,
            input,
            DefencePickers {
                best: &mut |_, _| best,
                open: &mut |_, _| open,
                paced: &mut |_, _| paced,
                in_reach: &mut |_| reach,
                start: &mut |_, _, kind| started.push(kind),
            },
        )
        .unwrap();
        started
    }

    #[test]
    fn the_defence_selection_needs_arming_and_a_free_actor_in_range() {
        let base = defence_input();
        for blocked in [
            DefenceSelectionInput {
                armed: false,
                ..base
            },
            DefenceSelectionInput {
                lockout_clear: false,
                ..base
            },
            DefenceSelectionInput {
                target_in_arc: false,
                ..base
            },
            DefenceSelectionInput {
                distance: 400,
                ..base
            },
        ] {
            let mut decision = CombatDecision::default();
            let mut flags = 0;
            let started = pick(
                &mut decision,
                &mut flags,
                &blocked,
                Some(1),
                Some(1),
                Some(1),
                true,
            );
            assert!(started.is_empty());
            assert_eq!((flags, decision.defence_counter), (0, 0));
        }
    }

    #[test]
    fn a_swing_past_the_ranges_starts_the_best_defence_and_the_counter_follows_the_facing() {
        let mut decision = CombatDecision::default();
        let mut flags = 0;
        // Pacing 10 % 5 == 0 and frame 20 > 6 + 8; the counter is 129 - 29 = 100 (10 % 100 != 0).
        let started = pick(
            &mut decision,
            &mut flags,
            &defence_input(),
            Some(3),
            None,
            Some(4),
            true,
        );
        assert_eq!(started, vec![3]);
        assert_eq!(decision.defence_counter, 100);
        assert_eq!(flags, 1);
        // Facing alike sets the counter to one, so the paced pick also starts.
        let mut decision = CombatDecision::default();
        let mut flags = 0;
        let alike = DefenceSelectionInput {
            facing_alike: true,
            ..defence_input()
        };
        let started = pick(
            &mut decision,
            &mut flags,
            &alike,
            Some(3),
            None,
            Some(4),
            true,
        );
        assert_eq!((started, decision.defence_counter), (vec![3, 4], 1));
        // A pick out of reach starts nothing but the intent still rises.
        let mut decision = CombatDecision::default();
        let mut flags = 0;
        let started = pick(
            &mut decision,
            &mut flags,
            &defence_input(),
            Some(3),
            None,
            Some(4),
            false,
        );
        assert!(started.is_empty());
        assert_eq!(flags, 1);
    }

    #[test]
    fn a_target_on_move_nine_gets_the_open_pick_and_an_idle_target_none() {
        let nine = DefenceSelectionInput {
            target_selected: 9,
            ..defence_input()
        };
        let mut decision = CombatDecision::default();
        let mut flags = 0;
        assert_eq!(
            pick(
                &mut decision,
                &mut flags,
                &nine,
                Some(3),
                Some(5),
                None,
                true
            ),
            vec![5]
        );
        let idle = DefenceSelectionInput {
            target_selected: -1,
            ..defence_input()
        };
        let mut decision = CombatDecision::default();
        let mut flags = 0;
        assert!(
            pick(
                &mut decision,
                &mut flags,
                &idle,
                Some(3),
                Some(5),
                None,
                true
            )
            .is_empty()
        );
    }

    #[test]
    fn with_a_move_already_selected_the_selector_chains_to_the_next_kind() {
        for (selector, expected) in [(0, vec![1]), (1, vec![2]), (2, vec![])] {
            let mut decision = CombatDecision {
                defence_counter: 5,
                ..Default::default()
            };
            let mut flags = 0;
            let input = DefenceSelectionInput {
                own_selected: 0,
                own_selector: selector,
                ..defence_input()
            };
            let started = pick(&mut decision, &mut flags, &input, None, None, None, true);
            assert_eq!(started, expected);
            assert_eq!(flags, u8::from(!expected.is_empty()));
        }
        // A nonzero counter remainder does nothing.
        let mut decision = CombatDecision {
            defence_counter: 3,
            ..Default::default()
        };
        let mut flags = 0;
        let input = DefenceSelectionInput {
            own_selected: 0,
            ..defence_input()
        };
        assert!(pick(&mut decision, &mut flags, &input, None, None, None, true).is_empty());
    }

    #[test]
    fn defence_counter_uses_facing_or_a_target_outside_combat_stance() {
        for (alike, stance, counter) in [
            (false, false, 1),
            (false, true, 100),
            (true, false, 1),
            (true, true, 1),
        ] {
            let input = DefenceSelectionInput {
                facing_alike: alike,
                target_in_combat_stance: stance,
                ..defence_input()
            };
            let mut decision = CombatDecision::default();
            let mut flags = 0xa2;
            pick(&mut decision, &mut flags, &input, None, None, None, true);
            assert_eq!(decision.defence_counter, counter);
            assert_eq!(flags, 0xa3);
        }
    }

    #[test]
    fn defence_selection_applies_a_start_before_the_next_picker_reads_state() {
        let input = DefenceSelectionInput {
            facing_alike: true,
            ..defence_input()
        };
        let mut decision = CombatDecision::default();
        let mut flags = 0xa0;
        let mut starts = Vec::new();
        defence_selection(
            &mut decision,
            &mut flags,
            &input,
            DefencePickers {
                best: &mut |_, flags| {
                    assert_eq!(flags, 0xa1);
                    Some(3)
                },
                open: &mut |_, _| panic!("open pick is not admitted"),
                paced: &mut |decision, flags| {
                    assert_eq!((decision.attack_state, flags), (1, 0xb1));
                    Some(4)
                },
                in_reach: &mut |_| true,
                start: &mut |decision, flags, kind| {
                    decision.attack_state = 1;
                    *flags |= 0x10;
                    starts.push(kind);
                },
            },
        )
        .unwrap();
        assert_eq!(starts, [3, 4]);
        assert_eq!(flags, 0xb1);
    }

    #[test]
    fn defence_selection_checks_only_divisors_used_by_the_admitted_branch() {
        for input in [
            DefenceSelectionInput {
                armed: false,
                ..defence_input()
            },
            DefenceSelectionInput {
                target_in_arc: false,
                ..defence_input()
            },
            DefenceSelectionInput {
                own_selected: 0,
                ..defence_input()
            },
        ] {
            let input = DefenceSelectionInput {
                defence_pacing: 0,
                ..input
            };
            let mut decision = CombatDecision {
                defence_counter: 5,
                ..Default::default()
            };
            let mut flags = 0;
            let starts = pick(&mut decision, &mut flags, &input, None, None, None, true);
            assert_eq!(
                starts,
                if input.own_selected == 0 {
                    vec![1]
                } else {
                    vec![]
                }
            );
        }
        for (sum, pacing, selected, counter) in [
            (10, 0, -1, 5),
            (i32::MIN, -1, -1, 5),
            (10, 5, 0, 0),
            (i32::MIN, 5, 0, -1),
        ] {
            let input = DefenceSelectionInput {
                pacing_sum: sum,
                defence_pacing: pacing,
                own_selected: selected,
                ..defence_input()
            };
            let mut decision = CombatDecision {
                defence_counter: counter,
                ..Default::default()
            };
            let mut flags = 0;
            let result = defence_selection(
                &mut decision,
                &mut flags,
                &input,
                DefencePickers {
                    best: &mut |_, _| panic!("invalid division must not pick"),
                    open: &mut |_, _| panic!("invalid division must not pick"),
                    paced: &mut |_, _| panic!("invalid division must not pick"),
                    in_reach: &mut |_| panic!("invalid division must not test reach"),
                    start: &mut |_, _, _| panic!("invalid division must not start"),
                },
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("defence selection")
            );
        }
    }

    #[test]
    fn defence_selection_requires_a_strictly_later_frame_and_keeps_independent_pacing() {
        for (frame, sum, expected, flags) in [
            (14, 10, vec![], 0xa0),
            (15, 10, vec![3], 0xa1),
            (14, 100, vec![4], 0xa1),
            (20, 3, vec![], 0xa0),
        ] {
            let input = DefenceSelectionInput {
                target_frame: frame,
                pacing_sum: sum,
                ..defence_input()
            };
            let mut decision = CombatDecision::default();
            let mut actual_flags = 0xa0;
            assert_eq!(
                pick(
                    &mut decision,
                    &mut actual_flags,
                    &input,
                    Some(3),
                    Some(5),
                    Some(4),
                    true
                ),
                expected
            );
            assert_eq!(actual_flags, flags);
        }
    }
}
