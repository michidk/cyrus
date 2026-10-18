//! Nonplayer decision-stage executors. The pure state machines follow the original's attack
//! executor; callers own the animation request and the combat record fields they pass in.
use super::combat::CombatDecision;
use anyhow::{Result, ensure};

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

#[cfg(test)]
mod tests {
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
}
