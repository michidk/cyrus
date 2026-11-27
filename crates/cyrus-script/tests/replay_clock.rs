use cyrus_assets::script::ClockSettings;
use cyrus_script::clock::SimulationClock;
use cyrus_script::script::ScriptActorId;
use cyrus_script::script::replay_clock::{
    DecisionClock, RecordedClockTimeline, RecordedDecisionClock,
};

fn clock() -> SimulationClock {
    SimulationClock::new(ClockSettings {
        normal_frame_rate: 12,
        min_frame_rate: 1,
        max_frame_rate: 300,
        use_smooth_fps: false,
        use_smooth_divisor: false,
    })
    .unwrap()
}

fn actor(index: usize) -> ScriptActorId {
    ScriptActorId {
        generation: 7,
        index,
    }
}

fn timeline() -> RecordedClockTimeline {
    RecordedClockTimeline {
        stored_sample: 1000,
        decisions: vec![actor(2), actor(5), actor(2)],
        before_decisions: vec![1008, 1008, 1043],
        after_decisions: vec![1046],
    }
}

#[test]
fn refreshes_bind_actual_entries_and_keep_the_later_endpoint_out_of_decisions() {
    let mut clock = clock();
    clock.advance_recorded_measurement(1000, 0, 202).unwrap();
    let timing = serde_json::to_value(clock.snapshot()).unwrap();
    let plan = timeline();
    let session = RecordedDecisionClock::new(&mut clock, &plan).unwrap();
    assert_eq!(session.sample(actor(2)).unwrap(), 1043);
    assert_eq!(session.sample(actor(5)).unwrap(), 1043);
    assert_eq!(session.sample(actor(2)).unwrap(), 1043);
    assert_eq!(session.finish().unwrap(), 1046);
    assert_eq!(clock.bios_sample(), 1046);
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), timing);
    clock.advance_recorded_measurement(1048, 2, 202).unwrap();
    assert_eq!(
        serde_json::to_value(clock.snapshot()).unwrap()["remainder"],
        2
    );
}

#[test]
fn missing_reordered_extra_and_stale_entries_reject_without_final_refresh() {
    for entries in [
        vec![actor(2)],
        vec![actor(5)],
        vec![actor(2), actor(5), actor(2), actor(2)],
        vec![ScriptActorId {
            generation: 8,
            index: 2,
        }],
    ] {
        let mut clock = clock();
        clock.rebase(1000);
        let plan = timeline();
        let session = RecordedDecisionClock::new(&mut clock, &plan).unwrap();
        for id in entries {
            let _ = session.sample(id);
        }
        assert!(session.finish().is_err());
        assert_ne!(clock.bios_sample(), 1046);
    }
}

#[test]
fn empty_or_mixed_generation_dispatch_is_not_a_supported_boundary() {
    let mut clock = clock();
    for decisions in [
        vec![],
        vec![
            actor(2),
            ScriptActorId {
                generation: 8,
                index: 5,
            },
        ],
    ] {
        let plan = RecordedClockTimeline {
            decisions,
            ..timeline()
        };
        assert!(RecordedDecisionClock::new(&mut clock, &plan).is_err());
        assert_eq!(clock.bios_sample(), 0);
    }
}

#[test]
fn samples_wrap_without_advancing_effective_measurement_or_movement() {
    let mut clock = clock();
    clock
        .advance_recorded_measurement(u32::MAX - 1, 0, 118)
        .unwrap();
    let timing = serde_json::to_value(clock.snapshot()).unwrap();
    let plan = RecordedClockTimeline {
        stored_sample: u32::MAX - 1,
        decisions: vec![actor(2)],
        before_decisions: vec![u32::MAX, 1],
        after_decisions: vec![3],
    };
    let session = RecordedDecisionClock::new(&mut clock, &plan).unwrap();
    assert_eq!(session.sample(actor(2)).unwrap(), 1);
    assert_eq!(session.finish().unwrap(), 3);
    assert_eq!(serde_json::to_value(clock.snapshot()).unwrap(), timing);
}
