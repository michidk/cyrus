use cyrus_game::player::{
    controller::{
        ControllerReturn, SheathedAction, SheathedControllerState, select_sheathed_action,
    },
    input::PlayerInput,
};

fn state() -> SheathedControllerState {
    SheathedControllerState {
        lockout: 0,
        dialogue_state: 0,
        menu_state: 0,
        walk_mode: false,
    }
}

#[test]
fn held_modifier_inverts_the_independent_walk_toggle() {
    for (walk_mode, held, expected) in [
        (false, false, SheathedAction::RunForward),
        (false, true, SheathedAction::WalkForward),
        (true, false, SheathedAction::WalkForward),
        (true, true, SheathedAction::RunForward),
    ] {
        let known = SheathedControllerState {
            walk_mode,
            ..state()
        };
        for backward in [false, true] {
            let input = PlayerInput {
                forward: true,
                backward,
                walk: held,
                ..Default::default()
            };
            assert_eq!(select_sheathed_action(&input, known).unwrap(), expected);
        }
    }
}

#[test]
fn releasing_forward_selects_idle_without_a_movement_request() {
    for walk_mode in [false, true] {
        for walk in [false, true] {
            let input = PlayerInput {
                walk,
                ..Default::default()
            };
            assert_eq!(
                select_sheathed_action(
                    &input,
                    SheathedControllerState {
                        walk_mode,
                        ..state()
                    }
                )
                .unwrap(),
                SheathedAction::Idle
            );
        }
    }
}

#[test]
fn every_nonzero_lockout_returns_before_other_dispatch() {
    let input = PlayerInput {
        forward: true,
        backward: true,
        left: true,
        right: true,
        action: true,
        jump: true,
        look: true,
        walk: true,
    };
    for lockout in [1, 2, u16::MAX] {
        assert_eq!(
            select_sheathed_action(
                &input,
                SheathedControllerState {
                    lockout,
                    dialogue_state: 1,
                    menu_state: 1,
                    ..state()
                },
            )
            .unwrap(),
            SheathedAction::Returned(ControllerReturn::Lockout)
        );
    }
}

#[test]
fn dialogue_and_topic_menu_return_only_for_exactly_one() {
    let input = PlayerInput {
        forward: true,
        walk: true,
        ..Default::default()
    };
    for word in [0, 1, 2, u16::MAX] {
        let dialogue = select_sheathed_action(
            &input,
            SheathedControllerState {
                dialogue_state: word,
                ..state()
            },
        )
        .unwrap();
        let menu = select_sheathed_action(
            &input,
            SheathedControllerState {
                menu_state: word,
                ..state()
            },
        )
        .unwrap();
        if word == 1 {
            assert_eq!(
                dialogue,
                SheathedAction::Returned(ControllerReturn::Dialogue)
            );
            assert_eq!(menu, SheathedAction::Returned(ControllerReturn::TopicMenu));
        } else {
            assert_eq!(dialogue, SheathedAction::WalkForward);
            assert_eq!(menu, SheathedAction::WalkForward);
        }
    }
    assert_eq!(
        select_sheathed_action(
            &input,
            SheathedControllerState {
                dialogue_state: 1,
                menu_state: 1,
                ..state()
            },
        )
        .unwrap(),
        SheathedAction::Returned(ControllerReturn::Dialogue)
    );
}

#[test]
fn view_returns_before_unsupported_actions() {
    let input = PlayerInput {
        forward: true,
        left: true,
        action: true,
        jump: true,
        look: true,
        ..Default::default()
    };
    assert_eq!(
        select_sheathed_action(&input, state()).unwrap(),
        SheathedAction::Returned(ControllerReturn::View)
    );
}

#[test]
fn turning_alone_or_with_forward_is_left_to_the_caller() {
    for (left, right, forward, expected) in [
        (true, false, false, SheathedAction::Idle),
        (false, true, false, SheathedAction::Idle),
        (true, true, false, SheathedAction::Idle),
        (true, false, true, SheathedAction::RunForward),
    ] {
        let input = PlayerInput {
            left,
            right,
            forward,
            ..Default::default()
        };
        assert_eq!(select_sheathed_action(&input, state()).unwrap(), expected);
    }
}

#[test]
fn the_walk_key_with_only_left_or_right_selects_a_sidestep() {
    use cyrus_game::script::locomotion::Strafe;
    for (left, right, forward, expected) in [
        (true, false, false, Some(Strafe::Left)),
        (false, true, false, Some(Strafe::Right)),
        (true, true, false, Some(Strafe::Left)),
        (true, false, true, None),
    ] {
        let input = PlayerInput {
            left,
            right,
            forward,
            walk: true,
            ..Default::default()
        };
        let action = select_sheathed_action(&input, state()).unwrap();
        match expected {
            Some(strafe) => assert_eq!(action, SheathedAction::Sidestep(strafe)),
            None => assert!(!matches!(action, SheathedAction::Sidestep(_))),
        }
    }
}

#[test]
fn jumping_is_rejected_but_activation_reaches_the_script_hook() {
    let error = select_sheathed_action(
        &PlayerInput {
            forward: true,
            jump: true,
            ..Default::default()
        },
        state(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("sheathed player controller"), "{error}");
    assert!(error.contains("jumping"), "{error}");

    assert_eq!(
        select_sheathed_action(
            &PlayerInput {
                forward: true,
                action: true,
                ..Default::default()
            },
            state(),
        )
        .unwrap(),
        SheathedAction::RunForward
    );
}

#[test]
fn backward_alone_selects_the_walk_or_run_helper_by_the_same_rule_as_forward() {
    for (walk_mode, held, expected) in [
        (false, false, SheathedAction::RunBackward),
        (false, true, SheathedAction::WalkBackward),
        (true, false, SheathedAction::WalkBackward),
        (true, true, SheathedAction::RunBackward),
    ] {
        let known = SheathedControllerState {
            walk_mode,
            ..state()
        };
        let input = PlayerInput {
            backward: true,
            walk: held,
            ..Default::default()
        };
        assert_eq!(select_sheathed_action(&input, known).unwrap(), expected);
    }
}
