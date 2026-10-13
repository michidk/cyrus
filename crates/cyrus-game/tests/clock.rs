use cyrus_assets::script::ClockSettings;
use cyrus_game::clock::{ClockStep, SimulationClock, bios_tick};

#[test]
fn host_duration_uses_bios_tick_boundaries() {
    use std::time::Duration;
    assert_eq!(bios_tick(Duration::ZERO), 0);
    assert_eq!(bios_tick(Duration::from_millis(54)), 0);
    assert_eq!(bios_tick(Duration::from_millis(55)), 1);
    assert_eq!(bios_tick(Duration::from_secs(1)), 18);
    assert_eq!(bios_tick(Duration::from_secs(60)), 1092);
}

fn settings(smoothing: bool) -> ClockSettings {
    ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 1,
        max_frame_rate: 300,
        use_smooth_fps: smoothing,
        use_smooth_divisor: smoothing,
    }
}

#[test]
fn startup_exposes_one_tick_while_measurement_continues() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        assert_eq!(
            clock.advance(18),
            ClockStep {
                frame_scale: 256,
                nominal_ticks: 1,
                fine_ticks: 8
            }
        );
    }
    assert_eq!(
        clock.advance(18),
        ClockStep {
            frame_scale: 3072,
            nominal_ticks: 12,
            fine_ticks: 96
        }
    );
}

#[test]
fn fractional_ticks_are_carried_and_zero_is_not_forced_to_one() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    assert_eq!(
        clock.advance(18),
        ClockStep {
            frame_scale: 236,
            nominal_ticks: 0,
            fine_ticks: 7
        }
    );
    assert_eq!(
        clock.advance(18),
        ClockStep {
            frame_scale: 236,
            nominal_ticks: 1,
            fine_ticks: 7
        }
    );
}

#[test]
fn smoothing_accelerates_at_five_and_scale_recovers_asymmetrically() {
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    assert_eq!(clock.advance(18).frame_scale, 236);
    for _ in 0..17 {
        clock.advance(18);
    }
    assert_eq!(clock.advance(36).frame_scale, 192);
    assert_eq!(clock.advance(36).frame_scale, 180);
    assert_eq!(clock.advance(36).frame_scale, 170);
    for _ in 0..7 {
        clock.advance(36);
    }
    assert_eq!(clock.advance(54).frame_scale, 191);
    assert_eq!(clock.advance(54).frame_scale, 212);
}

#[test]
fn suspended_time_does_not_enter_the_next_measurement() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    clock.advance(18);
    let mut uninterrupted = clock.clone();
    clock.rebase(1000);
    assert_eq!(clock.advance(1000), uninterrupted.advance(18));
    assert_eq!(clock.advance(1001), uninterrupted.advance(19));
}

#[test]
fn backward_clock_sample_clamps_the_accumulated_remainder() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    clock.advance(18);
    assert_eq!(clock.advance(0).frame_scale, 236);
    assert_eq!(clock.advance(18).frame_scale, 1536);
}

#[test]
fn menu_restore_retains_measurement_remainder_but_restores_frame_count() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    let checkpoint = clock.checkpoint();
    clock.advance(17);
    clock.restore(checkpoint, 100);
    assert_eq!(
        clock.advance(101),
        ClockStep {
            frame_scale: 236,
            nominal_ticks: 0,
            fine_ticks: 7,
        }
    );
}

#[test]
fn menu_restore_keeps_smoothed_fps_and_consumed_warmup() {
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    assert_eq!(clock.step().nominal_ticks, 0);
    let checkpoint = clock.checkpoint();
    for _ in 0..12 {
        clock.advance(18);
    }
    let menu_step = clock.step();
    assert_eq!(menu_step.nominal_ticks, 1);
    clock.restore(checkpoint, 100);
    assert_eq!(clock.step(), menu_step);
    assert_eq!(
        clock.advance(100),
        ClockStep {
            frame_scale: 277,
            nominal_ticks: 1,
            fine_ticks: 8,
        }
    );
}

#[test]
fn menu_restore_rolls_back_both_fractional_accumulators() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    clock.advance(18);
    let checkpoint = clock.checkpoint();
    clock.advance(19);
    clock.restore(checkpoint, 100);
    assert_eq!(
        clock.advance(100),
        ClockStep {
            frame_scale: 236,
            nominal_ticks: 1,
            fine_ticks: 7,
        }
    );
}

#[test]
fn world_entry_warmup_retains_measurement_and_fractional_carry() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    clock.advance(18);
    clock.restart_warmup();
    for _ in 0..12 {
        assert_eq!(
            clock.advance(18),
            ClockStep {
                frame_scale: 256,
                nominal_ticks: 1,
                fine_ticks: 8,
            }
        );
    }
    assert_eq!(
        clock.advance(18),
        ClockStep {
            frame_scale: 236,
            nominal_ticks: 1,
            fine_ticks: 8,
        }
    );
}
