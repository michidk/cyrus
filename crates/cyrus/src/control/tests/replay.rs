//! Replay admission contracts.
use super::*;

#[test]
fn replay_world_pauses_unless_a_schedule_owns_the_frame_loop() {
    use super::replay_gate_open;
    // An in-game world past its first two updates is held between requests.
    assert!(!replay_gate_open(
        false,
        false,
        true,
        Some(2),
        false,
        false,
        false
    ));
    assert!(!replay_gate_open(
        false,
        false,
        true,
        Some(900),
        false,
        false,
        false
    ));
    // A pending scheduled boundary, startup, loading and a fresh world keep time running.
    assert!(replay_gate_open(
        true,
        false,
        true,
        Some(900),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false,
        false,
        false,
        Some(900),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false,
        false,
        true,
        Some(1),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false,
        false,
        true,
        Some(0),
        false,
        false,
        false
    ));
    assert!(replay_gate_open(
        false, false, true, None, false, false, false
    ));
    // An open examine view closes on real-time ticks, so a held world must keep time running.
    assert!(replay_gate_open(
        false,
        true,
        true,
        Some(900),
        false,
        false,
        false
    ));
    // A speech line still exporting or loading holds a started world even inside a schedule,
    // but never loading, a fresh world, a world-less game or an open examine view.
    assert!(!replay_gate_open(
        true,
        false,
        true,
        Some(900),
        true,
        false,
        false
    ));
    assert!(replay_gate_open(
        true,
        false,
        false,
        Some(900),
        true,
        false,
        false
    ));
    assert!(replay_gate_open(
        true,
        false,
        true,
        Some(1),
        true,
        false,
        false
    ));
    assert!(replay_gate_open(
        true, false, true, None, true, false, false
    ));
    assert!(replay_gate_open(
        true,
        true,
        true,
        Some(900),
        true,
        false,
        false
    ));
}

#[test]
fn restored_replay_starts_held_without_consuming_the_saved_clock() {
    use cyrus_script::clock::SimulationClock;
    let mut clock = SimulationClock::new(cyrus_assets::script::ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 6,
        max_frame_rate: 300,
        use_smooth_fps: true,
        use_smooth_divisor: true,
    })
    .unwrap();
    clock.advance_recorded(0, 118).unwrap();
    let before = serde_json::to_value(clock.snapshot()).unwrap();
    // This is the restored world's first host update: its diagnostic count is
    // zero, but a complete continuation needs no fresh-world startup frames.
    if super::replay_gate_open(false, false, true, Some(0), false, true, false) {
        clock.advance_elapsed(0, std::time::Duration::ZERO);
    }
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), before);
    assert!(super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        false,
        true,
        false
    ));
    assert!(!super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        true,
        true,
        false
    ));
}

#[test]
fn exact_entry_requires_a_schedule_from_the_first_ordinary_body() {
    for updates in [0, 1, 2] {
        assert!(!super::replay_gate_open(
            false,
            false,
            true,
            Some(updates),
            false,
            false,
            true
        ));
        assert!(super::replay_gate_open(
            true,
            false,
            true,
            Some(updates),
            false,
            false,
            true
        ));
    }
    assert!(super::replay_gate_open(
        false,
        false,
        false,
        Some(0),
        false,
        false,
        true
    ));
}

#[test]
fn exact_entry_holds_the_loading_to_game_transition_before_any_random_draw() {
    let mut app = script_app();
    app.insert_resource(super::ReplayStep(std::time::Duration::from_millis(40)))
        .insert_resource(super::ReplayEntryHold)
        .init_resource::<cyrus_script::script::ReplayHold>();
    app.world_mut()
        .resource_mut::<NextState<cyrus_game::AppState>>()
        .set(cyrus_game::AppState::InGame);
    app.add_systems(
        Update,
        |state: Res<State<cyrus_game::AppState>>,
         hold: Res<cyrus_script::script::ReplayHold>,
         mut random: ResMut<cyrus_game::random::EngineRandom>| {
            if *state.get() == cyrus_game::AppState::InGame && !hold.0 {
                random.draw();
            }
        },
    );
    let initial = app
        .world()
        .resource::<cyrus_game::random::EngineRandom>()
        .state();
    for _ in 0..3 {
        app.update();
        assert_eq!(
            *app.world().resource::<State<cyrus_game::AppState>>().get(),
            cyrus_game::AppState::InGame
        );
        assert!(app.world().resource::<cyrus_script::script::ReplayHold>().0);
        assert_eq!(
            app.world()
                .resource::<cyrus_game::random::EngineRandom>()
                .state(),
            initial
        );
    }
}
