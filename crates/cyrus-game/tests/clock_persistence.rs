use cyrus_assets::script::ClockSettings;
use cyrus_game::clock::{ClockSnapshot, ClockStep, SimulationClock};
use serde_json::{Value, json};

fn settings(smoothing: bool) -> ClockSettings {
    ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 1,
        max_frame_rate: 300,
        use_smooth_fps: smoothing,
        use_smooth_divisor: smoothing,
    }
}

fn encoded(clock: &SimulationClock) -> Value {
    serde_json::to_value(clock.snapshot()).unwrap()
}

fn restored(value: Value, baseline: u32) -> anyhow::Result<SimulationClock> {
    let snapshot: ClockSnapshot = serde_json::from_value(value)?;
    SimulationClock::from_snapshot(snapshot, baseline)
}

#[test]
fn fractional_continuation_and_exposed_step_survive_a_new_epoch() {
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
    let saved = encoded(&clock);
    let mut resumed = restored(saved, 1_000_000).unwrap();
    assert_eq!(resumed.step(), clock.step());
    assert_eq!(
        resumed.advance(1_000_000),
        ClockStep {
            frame_scale: 236,
            nominal_ticks: 1,
            fine_ticks: 7
        }
    );
    clock.advance(18);
    for delta in [0, 1, 2, 17, 18, 18, 36, 100, 100, 101] {
        assert_eq!(
            resumed.advance(1_000_000 + delta),
            clock.advance(18 + delta)
        );
        assert_eq!(encoded(&resumed), encoded(&clock));
    }
}

#[test]
fn every_warmup_and_smoothing_boundary_resumes_exactly() {
    for smooth_fps in [false, true] {
        for smooth_divisor in [false, true] {
            let mut config = settings(smooth_fps);
            config.use_smooth_divisor = smooth_divisor;
            let mut clock = SimulationClock::new(config).unwrap();
            let mut sample = 0u32;
            for index in 0..100 {
                let mut resumed = restored(encoded(&clock), 9000).unwrap();
                let delta = [0, 0, 1, 17, 0, 18, 2, 0, 36][index % 9];
                sample += delta;
                assert_eq!(resumed.advance(9000 + delta), clock.advance(sample));
                assert_eq!(encoded(&resumed), encoded(&clock));
            }
        }
    }
}

#[test]
fn smoothing_has_a_literal_post_restore_result() {
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    for _ in 0..12 {
        clock.advance(0);
    }
    clock.advance(18);
    for _ in 0..17 {
        clock.advance(18);
    }
    let mut resumed = restored(encoded(&clock), 1000).unwrap();
    assert_eq!(resumed.advance(1018).frame_scale, 192);
    assert_eq!(resumed.advance(1018).frame_scale, 180);
    assert_eq!(resumed.advance(1018).frame_scale, 170);
}

#[test]
fn saved_settings_are_retained_instead_of_installed_defaults() {
    let mut config = settings(true);
    config.normal_frame_rate = 24;
    config.min_frame_rate = 2;
    config.max_frame_rate = 80;
    let clock = SimulationClock::new(config).unwrap();
    let resumed = restored(encoded(&clock), 999).unwrap();
    assert_eq!(resumed.settings(), config);
    assert_eq!(
        resumed.step(),
        ClockStep {
            frame_scale: 256,
            nominal_ticks: 0,
            fine_ticks: 0
        }
    );
}

#[test]
fn menu_checkpoint_semantics_and_world_warmup_survive_full_restore() {
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    let checkpoint = clock.checkpoint();
    for _ in 0..12 {
        clock.advance(18);
    }
    clock.restore(checkpoint, 100);
    let saved = encoded(&clock);
    let mut resumed = restored(saved, 3000).unwrap();
    assert_eq!(
        resumed.advance(3000),
        ClockStep {
            frame_scale: 277,
            nominal_ticks: 1,
            fine_ticks: 8
        }
    );
    clock.advance(100);
    resumed.restart_warmup();
    clock.restart_warmup();
    for tick in 0..14 {
        assert_eq!(resumed.advance(3000 + tick), clock.advance(100 + tick));
        assert_eq!(encoded(&resumed), encoded(&clock));
    }
}

#[test]
fn wrap_and_backward_samples_keep_the_rebased_continuation() {
    let mut clock = SimulationClock::new(settings(true)).unwrap();
    clock.rebase(u32::MAX - 3);
    clock.advance(u32::MAX);
    let mut resumed = restored(encoded(&clock), 200).unwrap();
    for (original, rebased) in [(0, 201), (5, 206), (2, 203), (20, 221)] {
        assert_eq!(resumed.advance(rebased), clock.advance(original));
        assert_eq!(encoded(&resumed), encoded(&clock));
    }
}

#[test]
fn malformed_state_fails_before_replacing_the_live_clock() {
    let clock = SimulationClock::new(settings(true)).unwrap();
    let saved = encoded(&clock);
    for (field, invalid) in [
        ("version", json!(2)),
        ("remainder", json!(-1)),
        ("remainder", json!(18)),
        ("warmup", json!(13)),
        ("nominal_fraction", json!(256)),
        ("fine_fraction", json!(32)),
        ("next_scale", json!(u32::MAX)),
    ] {
        let mut corrupt = saved.clone();
        corrupt[field] = invalid;
        let error = restored(corrupt, 0).unwrap_err().to_string();
        assert!(error.contains(field), "{error}");
    }
    for field in ["frame_scale", "nominal_ticks", "fine_ticks"] {
        let mut corrupt = saved.clone();
        corrupt["step"][field] = json!(u32::MAX);
        assert!(
            restored(corrupt, 0)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
    for (field, invalid) in [
        ("normal_frame_rate", 0),
        ("normal_frame_rate", u32::MAX),
        ("min_frame_rate", 0),
        ("max_frame_rate", 0),
    ] {
        let mut corrupt = saved.clone();
        corrupt["settings"][field] = json!(invalid);
        assert!(
            restored(corrupt, 0)
                .unwrap_err()
                .to_string()
                .contains("settings")
        );
    }
    let mut missing = saved.clone();
    missing.as_object_mut().unwrap().remove("smoothed");
    assert!(restored(missing, 0).is_err());
    assert_eq!(encoded(&clock), saved);
}

#[test]
fn extreme_valid_settings_and_zero_scale_can_resume() {
    for config in [
        ClockSettings {
            normal_frame_rate: i32::MAX as u32 / 256,
            min_frame_rate: 1,
            max_frame_rate: 1,
            ..settings(true)
        },
        ClockSettings {
            normal_frame_rate: 1,
            min_frame_rate: i32::MAX as u32,
            max_frame_rate: i32::MAX as u32,
            ..settings(false)
        },
    ] {
        let mut clock = SimulationClock::new(config).unwrap();
        for tick in 0..30 {
            let mut resumed = restored(encoded(&clock), 500).unwrap();
            assert_eq!(resumed.advance(501), clock.advance(tick + 1));
            assert_eq!(encoded(&resumed), encoded(&clock));
        }
    }
}
