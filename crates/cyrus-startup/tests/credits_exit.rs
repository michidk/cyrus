use bevy::prelude::{ButtonInput, KeyCode};
use cyrus_startup::{CreditsExit, CreditsExitState};

#[test]
fn credits_retain_the_screen_until_escape_release_and_consume_only_that_key() {
    let mut exit = CreditsExit::default();
    let mut keys = ButtonInput::default();
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Scrolling);
    keys.press(KeyCode::ArrowUp);
    keys.press(KeyCode::Escape);
    assert_eq!(exit.poll(&mut keys), CreditsExitState::WaitingForRelease);
    keys.clear();
    assert_eq!(exit.poll(&mut keys), CreditsExitState::WaitingForRelease);
    assert!(keys.pressed(KeyCode::Escape));
    keys.release(KeyCode::Escape);
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Finished);
    assert!(!keys.just_released(KeyCode::Escape));
    assert!(!keys.just_pressed(KeyCode::Escape));
    assert!(keys.pressed(KeyCode::ArrowUp));
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Finished);
}

#[test]
fn a_movie_skip_held_on_entry_requires_a_new_press_to_exit() {
    let mut exit = CreditsExit::default();
    let mut keys = ButtonInput::default();
    keys.press(KeyCode::Escape);
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Scrolling);
    keys.clear();
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Scrolling);
    keys.release(KeyCode::Escape);
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Scrolling);
    keys.press(KeyCode::Escape);
    assert_eq!(exit.poll(&mut keys), CreditsExitState::WaitingForRelease);
    keys.release(KeyCode::Escape);
    assert_eq!(exit.poll(&mut keys), CreditsExitState::Finished);
}
