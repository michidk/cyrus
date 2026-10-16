//! Public off-live explicit-player continuation and identity contracts.
use bevy::prelude::*;
use cyrus_game::player::{
    placement::MarkerEntry,
    runtime::{PlayerRuntime, PlayerRuntimeBinding, PlayerRuntimeSnapshot},
};

#[test]
fn runtime_replacement_retains_failure_and_pending_entry_with_explicit_bindings()
-> anyhow::Result<()> {
    let mut world = World::new();
    let player = world.spawn_empty().id();
    let replacement = world.spawn_empty().id();
    let mut document = serde_json::to_value(
        PlayerRuntime::default().snapshot(&Handle::default(), &Handle::default())?,
    )?;
    document["source"] = serde_json::json!({"player":player.to_bits(),"generation":7,"marker":{"marker":3,"heading":-1}});
    document["entry_pending"] = true.into();
    document["explicit_updated"] = true.into();
    document["error"] = "retained collision failure".into();
    let snapshot: PlayerRuntimeSnapshot = serde_json::from_value(document)?;
    assert_eq!(snapshot.source_identity(), Some((player.to_bits(), 7)));
    assert!(PlayerRuntime::from_snapshot(snapshot.clone(), None).is_err());
    let bind = |saved_generation| PlayerRuntimeBinding {
        saved_player: player.to_bits(),
        saved_generation,
        player: replacement,
        generation: 19,
        scene: Handle::default(),
        animation: Handle::default(),
        marker: MarkerEntry {
            marker: 3,
            heading: -1,
        },
    };
    assert!(PlayerRuntime::from_snapshot(snapshot.clone(), Some(bind(8))).is_err());
    let restored = PlayerRuntime::from_snapshot(snapshot, Some(bind(7)))?;
    assert_eq!(
        restored.error.as_deref(),
        Some("retained collision failure")
    );
    let resumed = restored.snapshot(&Handle::default(), &Handle::default())?;
    assert_eq!(resumed.source_identity(), Some((replacement.to_bits(), 19)));
    let value = serde_json::to_value(resumed)?;
    assert_eq!(value["entry_pending"], true);
    assert_eq!(value["explicit_updated"], true);
    Ok(())
}

#[test]
fn runtime_snapshot_rejects_version_and_orphaned_update_state() -> anyhow::Result<()> {
    let live = PlayerRuntime::default();
    let saved = serde_json::to_value(live.snapshot(&Handle::default(), &Handle::default())?)?;
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("entry_pending", true.into()),
        ("explicit_updated", true.into()),
    ] {
        let mut invalid = saved.clone();
        invalid[field] = value;
        assert!(PlayerRuntime::from_snapshot(serde_json::from_value(invalid)?, None).is_err());
        assert_eq!(
            serde_json::to_value(live.snapshot(&Handle::default(), &Handle::default())?)?,
            saved
        );
    }
    Ok(())
}
