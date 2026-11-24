use bevy::{asset::AssetPlugin, audio::AudioSource, prelude::*};
use cyrus_audio::{
    persistence::AudioRestoreBindings,
    sound::{
        RestoredDialogueAssets, RuntimeDialogue, RuntimeDialogueSnapshot, dialogue::DialogueClip,
    },
};
use cyrus_script::script::ScriptActorId;
use std::collections::BTreeMap;

#[test]
fn restored_torch_retains_its_shared_channel_instead_of_losing_the_owner() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<AudioSource>()
        .init_asset::<DialogueClip>();
    let server = app.world().resource::<AssetServer>();
    let sources = app.world().resource::<Assets<AudioSource>>();
    let loaded = app.world().resource::<Assets<DialogueClip>>();
    let mut document = serde_json::to_value(
        RuntimeDialogue::default()
            .snapshot(server, sources)
            .unwrap(),
    )
    .unwrap();
    document["generation"] = 4.into();
    document["torch_channel"] = 6.into();
    let bindings = AudioRestoreBindings::new(
        &[(
            ScriptActorId {
                generation: 4,
                index: 2,
            },
            ScriptActorId {
                generation: 9,
                index: 3,
            },
        )],
        &[],
    )
    .unwrap();
    let clips = BTreeMap::new();
    let requests = BTreeMap::new();
    let saved: RuntimeDialogueSnapshot = serde_json::from_value(document.clone()).unwrap();
    let restored = RuntimeDialogue::from_snapshot(
        saved,
        RestoredDialogueAssets {
            server,
            audio_sources: sources,
            loaded,
            clips: &clips,
            requests: &requests,
            hud: None,
        },
        &BTreeMap::from([(6, 8)]),
        &bindings,
    )
    .unwrap();
    for (field, value) in [
        ("torch_channel", serde_json::json!(16)),
        ("generation", serde_json::Value::Null),
    ] {
        let mut invalid = document.clone();
        invalid[field] = value;
        let invalid = serde_json::from_value(invalid).unwrap();
        assert!(
            RuntimeDialogue::from_snapshot(
                invalid,
                RestoredDialogueAssets {
                    server,
                    audio_sources: sources,
                    loaded,
                    clips: &clips,
                    requests: &requests,
                    hud: None
                },
                &BTreeMap::from([(6, 8)]),
                &bindings,
            )
            .is_err(),
            "invalid {field} must reject before live replacement"
        );
    }
    for mapping in [BTreeMap::new(), BTreeMap::from([(6, 16)])] {
        assert!(
            RuntimeDialogue::from_snapshot(
                serde_json::from_value(document.clone()).unwrap(),
                RestoredDialogueAssets {
                    server,
                    audio_sources: sources,
                    loaded,
                    clips: &clips,
                    requests: &requests,
                    hud: None
                },
                &mapping,
                &bindings,
            )
            .is_err()
        );
    }
    let mut legacy = document;
    legacy.as_object_mut().unwrap().remove("torch_channel");
    assert!(serde_json::from_value::<RuntimeDialogueSnapshot>(legacy).is_ok());
    let continued = serde_json::to_value(restored.snapshot(server, sources).unwrap()).unwrap();
    assert_eq!(continued["generation"], 9);
    assert_eq!(
        continued["torch_channel"], 8,
        "restoration must retain and rebind the shared torch slot"
    );
}

#[test]
fn original_load_releases_cache_hints_and_keeps_retained_service_owners() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<AudioSource>()
        .init_asset::<DialogueClip>();
    let server = app.world().resource::<AssetServer>();
    let sources = app.world().resource::<Assets<AudioSource>>();
    let mut document = serde_json::to_value(
        RuntimeDialogue::default()
            .snapshot(server, sources)
            .unwrap(),
    )
    .unwrap();
    let clip = serde_json::json!({
        "entry": { "version": 1, "label": [84, 69, 83, 84], "text": "line", "text_lifetime": 60, "voice": null },
        "audio_hash": null
    });
    document["cache"]["slots"][0] = serde_json::json!({
        "label": [84, 69, 83, 84], "payload": clip, "sample": 6, "used": true, "age": 0
    });
    document["generation"] = 4.into();
    document["torch_channel"] = 6.into();
    document["ordinary_completed"] = true.into();
    let saved: RuntimeDialogueSnapshot = serde_json::from_value(document.clone()).unwrap();
    assert_eq!(
        saved.required_labels(),
        std::collections::BTreeSet::from([*b"TEST"])
    );
    let reset = saved.for_saved_world().unwrap();
    assert!(
        reset.required_labels().is_empty(),
        "released cache must require no installed clip"
    );
    let after = serde_json::to_value(reset).unwrap();
    for field in [
        "generation",
        "torch_channel",
        "ordinary_completed",
        "hud_bound",
        "preparation",
        "taunts",
    ] {
        assert_eq!(after[field], document[field], "retained {field} changed");
    }
    assert!(
        after["cache"]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .all(|slot| slot["payload"].is_null())
    );
}
