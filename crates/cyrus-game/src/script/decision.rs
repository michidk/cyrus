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
}
