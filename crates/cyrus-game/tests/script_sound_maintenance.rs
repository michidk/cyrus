//! Public maintenance sequences follow preservation's sound-reset contract.
use cyrus_game::script::{SoundMaintenance, SoundResetStage};

fn sample(maintenance: &mut SoundMaintenance, ticks: u32) -> bool {
    maintenance.advance(ticks, &mut |_| Ok(())).unwrap()
}

#[test]
fn periodic_reset_uses_strict_signed_tick_threshold_and_clears_each_update() {
    let mut maintenance = SoundMaintenance::default();
    assert!(!sample(&mut maintenance, 1400));
    assert!(sample(&mut maintenance, 1));
    assert!(maintenance.reset().unwrap());
    assert!(!sample(&mut maintenance, 0));
    assert!(!maintenance.reset().unwrap());
    assert!(!sample(&mut maintenance, 1400));
    assert!(sample(&mut maintenance, 1));
}

#[test]
fn menu_or_dialogue_inhibits_and_lockout_retains_the_previous_state() {
    let mut maintenance = SoundMaintenance::default();
    assert!(!sample(&mut maintenance, 1400));
    maintenance.ordinary_actor(1, false, 0);
    assert!(!sample(&mut maintenance, 100));
    maintenance.ordinary_actor(0, false, 1);
    assert!(!sample(&mut maintenance, 100));
    maintenance.ordinary_actor(0, false, 0);
    assert!(sample(&mut maintenance, 1));
    assert!(!sample(&mut maintenance, 1400));
    maintenance.ordinary_actor(0, true, 0);
    assert!(!sample(&mut maintenance, 100));
    maintenance.ordinary_actor(0, false, 2);
    assert!(!sample(&mut maintenance, 100));
    maintenance.ordinary_actor(0, false, 0);
    assert!(sample(&mut maintenance, 1));
}

#[test]
fn lockout_does_not_activate_inhibition_and_world_entry_restarts_the_counter() {
    let mut maintenance = SoundMaintenance::default();
    maintenance.ordinary_actor(0, false, 1);
    assert!(!sample(&mut maintenance, 1400));
    maintenance.enter_world();
    assert!(!sample(&mut maintenance, 1));
    maintenance.ordinary_actor(2, false, 0);
    maintenance.enter_world();
    assert!(!sample(&mut maintenance, 1401));
    maintenance.ordinary_actor(0, false, 0);
    assert!(sample(&mut maintenance, 1401));
}

#[test]
fn tick_accumulation_preserves_signed_wrapping() {
    let mut maintenance = SoundMaintenance::default();
    assert!(!sample(&mut maintenance, u32::MAX));
    assert!(!sample(&mut maintenance, 1401));
    assert!(sample(&mut maintenance, 1));
}

#[test]
fn reset_cleans_all_owners_in_order_and_partial_failure_is_sticky() {
    let mut maintenance = SoundMaintenance::default();
    let mut stages = Vec::new();
    assert!(
        maintenance
            .advance(1401, &mut |stage| {
                stages.push(stage);
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(
        stages,
        [
            SoundResetStage::ReleaseRtx,
            SoundResetStage::ClearDialogue,
            SoundResetStage::StopSamples,
            SoundResetStage::ClearNodeReservations
        ]
    );
    assert!(maintenance.reset().unwrap());
    stages.clear();
    let error = maintenance
        .advance(1401, &mut |stage| {
            stages.push(stage);
            if stage == SoundResetStage::ClearDialogue {
                anyhow::bail!("dialogue owner unavailable");
            }
            Ok(())
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("ClearDialogue: dialogue owner unavailable"));
    assert_eq!(
        stages,
        [SoundResetStage::ReleaseRtx, SoundResetStage::ClearDialogue]
    );
    assert!(maintenance.reset().is_err());
    stages.clear();
    maintenance.enter_world();
    assert_eq!(
        maintenance
            .advance(1, &mut |stage| {
                stages.push(stage);
                Ok(())
            })
            .unwrap_err()
            .to_string(),
        error
    );
    assert!(stages.is_empty());
}
