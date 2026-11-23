use cyrus_assets::script::ClockSettings;
use cyrus_script::clock::{ClockStep, SimulationClock, bios_tick};

#[test]
fn host_duration_uses_bios_tick_boundaries() {
    use std::time::Duration;
    assert_eq!(bios_tick(Duration::ZERO), 0);
    assert_eq!(bios_tick(Duration::from_millis(54)), 0);
    assert_eq!(bios_tick(Duration::from_millis(55)), 1);
    assert_eq!(bios_tick(Duration::from_secs(1)), 18);
    assert_eq!(bios_tick(Duration::from_secs(60)), 1092);
}

#[test]
fn simulated_bios_tick_follows_simulated_time_not_the_host_baseline() {
    use std::time::Duration;
    let step = Duration::from_millis(40);
    let mut early = SimulationClock::new(settings(false)).unwrap();
    let mut late = SimulationClock::new(settings(false)).unwrap();
    // Two hosts that started the same game at different absolute BIOS ticks (a longer load).
    early.rebase(100);
    late.rebase(107);
    let mut previous = 0;
    for _ in 0..60 {
        early.advance_elapsed(early.simulated_bios_tick(), step);
        late.advance_elapsed(late.simulated_bios_tick(), step);
        let tick = early.simulated_bios_tick();
        assert_eq!(tick, late.simulated_bios_tick());
        assert!(tick >= previous);
        previous = tick;
    }
    // 60 updates of 40 ms is 2.4 s of simulated time: 43 BIOS ticks (18.2 per second).
    assert_eq!(previous, 43);
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
fn recorded_frames_keep_exact_q8_deltas_and_tick_carries() {
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    let mut consumed = 0;
    for scale in [122, 99, 105, 118, 0, 102, 121] {
        let previous = consumed;
        consumed += scale;
        let step = clock
            .advance_recorded(clock.simulated_bios_tick(), scale)
            .unwrap();
        assert_eq!(step.frame_scale, scale);
        assert_eq!(step.nominal_ticks, consumed / 256 - previous / 256);
        assert_eq!(step.fine_ticks, consumed / 32 - previous / 32);
        assert_eq!(clock.elapsed_frame_scale(), u64::from(consumed));
    }
    let saved = serde_json::to_value(clock.snapshot()).unwrap();
    assert!(clock.advance_recorded(100, 3073).is_err());
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), saved);
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

#[test]
fn game_seconds_sum_every_step_as_frame_scale_over_256_nominal_ticks() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    assert_eq!(clock.game_seconds(), 0.0);
    let mut scale = 0u64;
    for tick in 0..40 {
        scale += u64::from(clock.advance(18 + tick / 2).frame_scale);
    }
    // One nominal tick is 1/12 second: normal_frame_rate in `settings`.
    assert!((clock.game_seconds() - scale as f64 / 256.0 / 12.0).abs() < 1e-9);
    assert!(clock.game_seconds() > 1.0);
}

#[test]
fn exact_elapsed_scale_tracks_consumed_steps_without_float_reconstruction() {
    let mut clock = SimulationClock::new(settings(false)).unwrap();
    let mut consumed = 0;
    for sample in [0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 56, 60, 61, 62, 90] {
        consumed += u64::from(clock.advance(sample).frame_scale);
        assert_eq!(clock.elapsed_frame_scale(), consumed);
    }
}

#[test]
fn elapsed_gameplay_has_no_warmup_and_tracks_cadence_changes() {
    use std::time::Duration;
    for smoothing in [false, true] {
        let mut clock = SimulationClock::new(settings(smoothing)).unwrap();
        assert_eq!(clock.advance_elapsed(0, Duration::ZERO).frame_scale, 0);
        let mut nanos = 0u64;
        for interval in [1_000_000, 4_000_000, 16_000_000, 50_000_000, 8_000_000] {
            for _ in 0..120 {
                nanos += interval;
                clock.advance_elapsed(
                    bios_tick(Duration::from_nanos(nanos)),
                    Duration::from_nanos(interval),
                );
                assert_eq!(
                    clock.elapsed_frame_scale(),
                    nanos * 12 * 256 / 1_000_000_000
                );
            }
        }
    }
}

#[test]
fn elapsed_gameplay_excludes_suspension_and_survives_snapshot_restore() {
    use std::time::Duration;
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    clock.advance_elapsed(0, Duration::from_nanos(1_234_567));
    let mut uninterrupted = clock.clone();
    let checkpoint = clock.checkpoint();
    for tick in 1..20 {
        clock.advance(tick);
    }
    clock.advance_elapsed(20, Duration::from_nanos(7_654_321));
    clock.restore(checkpoint, 1000);
    let snapshot = serde_json::to_value(clock.snapshot()).unwrap();
    let mut restored =
        SimulationClock::from_snapshot(serde_json::from_value(snapshot).unwrap(), 1000).unwrap();
    for interval in [0, 1_234_567, 16_666_667, 100_000_000] {
        let elapsed = Duration::from_nanos(interval);
        let expected = uninterrupted.advance_elapsed(0, elapsed);
        assert_eq!(clock.advance_elapsed(1000, elapsed), expected);
        assert_eq!(restored.advance_elapsed(1000, elapsed), expected);
    }
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_clock_restores_carries_settings_and_exposed_step_before_resuming() {
    let mut bytes = [0u8; 1542];
    for (offset, value) in [
        (0x98, 5u32),
        (0x9c, 25),
        (0xa0, 99),
        (0xb8, 7),
        (0xc0, 24),
        (0xc4, 122),
        (0xc8, 1),
        (0xd0, 255),
        (0xd8, 4),
        (0xe0, 31),
        (0xe8, 1),
        (0xec, 60),
        (0xf0, 1),
        (0xf4, 1),
        (0xf8, 3072),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let saved = rgpre::import::savegame::parse_save_clock(&bytes).unwrap();
    let mut clock = SimulationClock::from_original(&saved, 900).unwrap();
    assert_eq!(clock.bios_sample(), 900);
    assert_eq!(clock.step().frame_scale, 122);
    assert_eq!(clock.step().nominal_ticks, 1);
    assert_eq!(clock.step().fine_ticks, 4);
    assert_eq!(clock.fps(), 25);
    let state = serde_json::to_value(clock.snapshot()).unwrap();
    assert_eq!(state["frames"], 7);
    assert_eq!(state["remainder"], 5);
    assert_eq!(state["smoothed"], 24);
    assert_eq!(state["next_scale"], 99);
    let step = clock.advance_recorded(900, 1).unwrap();
    assert_eq!((step.nominal_ticks, step.fine_ticks), (1, 1));
    assert_eq!(clock.elapsed_frame_scale(), 1);
    let mut invalid = saved.clone();
    invalid.warmup = 13;
    assert!(
        SimulationClock::from_original(&invalid, 0)
            .unwrap_err()
            .to_string()
            .contains("SVGV clock")
    );
}

#[cfg(feature = "original-save-import")]
#[test]
fn settled_original_import_consumes_only_the_first_explicit_frame() {
    let mut bytes = [0u8; 1542];
    for (offset, value) in [
        (0x9c, 26u32),
        (0xa0, 118),
        (0xc0, 26),
        (0xc4, 118),
        (0xd0, 134),
        (0xd8, 4),
        (0xe0, 6),
        (0xe8, 6),
        (0xec, 300),
        (0xf0, 1),
        (0xf4, 1),
        (0xf8, 3072),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let saved = rgpre::import::savegame::parse_save_clock(&bytes).unwrap();
    let mut clock = SimulationClock::from_original(&saved, 900).unwrap();
    assert_eq!(clock.elapsed_frame_scale(), 0);
    assert_eq!(
        clock.step(),
        ClockStep {
            frame_scale: 118,
            nominal_ticks: 0,
            fine_ticks: 4
        }
    );
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap()["warmup"], 0);
    assert_eq!(
        clock.advance_recorded(900, 118).unwrap(),
        ClockStep {
            frame_scale: 118,
            nominal_ticks: 0,
            fine_ticks: 3
        }
    );
    let first = serde_json::to_value(clock.snapshot()).unwrap();
    assert_eq!(first["warmup"], 0);
    assert_eq!(first["nominal_fraction"], 252);
    assert_eq!(first["fine_fraction"], 28);
    assert_eq!(clock.elapsed_frame_scale(), 118);
    assert_eq!(
        clock.advance_recorded(900, 118).unwrap(),
        ClockStep {
            frame_scale: 118,
            nominal_ticks: 1,
            fine_ticks: 4
        }
    );
    assert_eq!(clock.elapsed_frame_scale(), 236);
}
