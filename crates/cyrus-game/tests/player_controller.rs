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
fn unsupported_actions_are_not_silently_treated_as_idle_or_forward() {
    for (input, reason) in [
        (
            PlayerInput {
                forward: true,
                left: true,
                ..Default::default()
            },
            "turning",
        ),
        (
            PlayerInput {
                forward: true,
                right: true,
                ..Default::default()
            },
            "turning",
        ),
        (
            PlayerInput {
                forward: true,
                jump: true,
                ..Default::default()
            },
            "jumping",
        ),
        (
            PlayerInput {
                forward: true,
                action: true,
                ..Default::default()
            },
            "activation",
        ),
        (
            PlayerInput {
                backward: true,
                ..Default::default()
            },
            "backward",
        ),
    ] {
        let error = select_sheathed_action(&input, state())
            .unwrap_err()
            .to_string();
        assert!(error.contains("sheathed player controller"), "{error}");
        assert!(error.contains(reason), "{error}");
    }
}
