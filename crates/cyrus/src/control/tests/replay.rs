//! Replay admission contracts.
use super::*;

#[test]
fn replay_world_pauses_unless_a_schedule_owns_the_frame_loop() {
    use super::replay_gate_open;
    // An in-game world past its first two updates is held between requests.
    assert!(!replay_gate_open(false, false, true, Some(2), false, false));
    assert!(!replay_gate_open(
        false,
        false,
        true,
        Some(900),
        false,
        false
    ));
    // A pending scheduled boundary, startup, loading and a fresh world keep time running.
    assert!(replay_gate_open(true, false, true, Some(900), false, false));
    assert!(replay_gate_open(
        false,
        false,
        false,
        Some(900),
        false,
        false
    ));
    assert!(replay_gate_open(false, false, true, Some(1), false, false));
    assert!(replay_gate_open(false, false, true, Some(0), false, false));
    assert!(replay_gate_open(false, false, true, None, false, false));
    // An open examine view closes on real-time ticks, so a held world must keep time running.
    assert!(replay_gate_open(false, true, true, Some(900), false, false));
    // A speech line still exporting or loading holds a started world even inside a schedule,
    // but never loading, a fresh world, a world-less game or an open examine view.
    assert!(!replay_gate_open(true, false, true, Some(900), true, false));
    assert!(replay_gate_open(true, false, false, Some(900), true, false));
    assert!(replay_gate_open(true, false, true, Some(1), true, false));
    assert!(replay_gate_open(true, false, true, None, true, false));
    assert!(replay_gate_open(true, true, true, Some(900), true, false));
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
    if super::replay_gate_open(false, false, true, Some(0), false, true) {
        clock.advance_elapsed(0, std::time::Duration::ZERO);
    }
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), before);
    assert!(super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        false,
        true
    ));
    assert!(!super::replay_gate_open(
        true,
        false,
        true,
        Some(0),
        true,
        true
    ));
}
