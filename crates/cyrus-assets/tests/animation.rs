//! Public animation contracts. The exported-model check reports a missing install.
use cyrus_assets::animation::Animations;

fn asset() -> Animations {
    serde_json::from_value(serde_json::json!({
        "version": 2, "world": 17,
        "sections": {"RAHD": [1, 2, 3]},
        "metadata": {"actors": [{"script_name": "ACTOR", "raan_entries": [{"file_path": "fxart/test.3dc", "frame_count": 2}], "animation_groups": [{"group_index": 7, "commands": [{"opcode": 4, "sound_param": 9}]}]}]},
        "models": {"TEST": {
            "base_positions": [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            "base_normals": [[0, 0, 1]], "faces": [[0, 1, 2]], "corner_normals": [[null, null, null]],
            "encoding": "animated_i16",
            "positions": [[[0, 0, 0], [1, 0, 0], [0, 1, 0]], [[0, 0, 2], [1, 0, 2], [0, 1, 2]]],
            "normals": [[], [[0, 0, 1]]]
        }},
        "unsupported": {"MISSING": "model absent"}
    })).unwrap()
}

#[test]
fn cached_animation_preserves_frames_cues_and_unresolved_references() {
    let bytes = serde_json::to_vec(&asset()).unwrap();
    let restored: Animations = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.models["TEST"].positions[1][0], [0.0, 0.0, 2.0]);
    assert_eq!(
        restored.metadata["actors"][0]["animation_groups"][0]["commands"][0]["sound_param"],
        9
    );
    assert_eq!(restored.unsupported["MISSING"], "model absent");
    assert_eq!(restored.sections["RAHD"], [1, 2, 3]);
}

#[test]
fn invalid_frame_geometry_reports_world_and_model() {
    let mut broken = asset();
    broken.models.get_mut("TEST").unwrap().positions[1].pop();
    let message = broken.validate().unwrap_err().to_string();
    assert!(
        message.contains("world 17 animation TEST frame 1"),
        "{message}"
    );
    let mut broken = asset();
    broken.models.get_mut("TEST").unwrap().faces[0][0] = 3;
    assert!(
        broken
            .validate()
            .unwrap_err()
            .to_string()
            .contains("face vertex out of range")
    );
}

#[test]
fn unsupported_encoding_is_rejected_before_loading() {
    let mut broken = asset();
    broken.models.get_mut("TEST").unwrap().encoding = "unknown".into();
    let message = broken.validate().unwrap_err().to_string();
    assert!(
        message.contains("world 17 animation TEST: unsupported frame encoding unknown"),
        "{message}"
    );
}

#[test]
fn old_frame_reference_schema_is_rejected() {
    let mut legacy = asset();
    legacy.version = 1;
    assert_eq!(
        legacy.validate().unwrap_err().to_string(),
        "world 17: animation schema 1 unsupported; re-export the world"
    );
}

#[test]
fn placed_frame_sources_belong_to_instances_and_preserve_table_bounds() {
    let mut data = asset();
    let other =
        serde_json::from_value(serde_json::to_value(&data.models["TEST"]).unwrap()).unwrap();
    data.models.insert("OTHER".into(), other);
    data.metadata["actors"][0]
        .as_object_mut()
        .unwrap()
        .remove("raan_entries");
    data.metadata["mpob_objects"] = serde_json::json!([
        {"script_name":"ACTOR", "object_type":1, "is_static":2, "model_name":"test."},
        {"script_name":"ACTOR", "object_type":1, "is_static":2, "model_name":"other.3dc"},
        {"script_name":"ACTOR", "object_type":1, "is_static":1, "model_name":"test"},
        {"script_name":"ACTOR", "object_type":1, "is_static":0, "model_name":"test"},
        {"script_name":"ACTOR", "object_type":2, "is_static":2, "model_name":"test"}
    ]);
    assert_eq!(
        data.resolve_map_frame(0, 1).unwrap(),
        cyrus_assets::animation::ModelFrame {
            model: "TEST".into(),
            frame: 1
        }
    );
    assert_eq!(
        data.resolve_map_frame(1, 1).unwrap(),
        cyrus_assets::animation::ModelFrame {
            model: "OTHER".into(),
            frame: 1
        }
    );
    assert!(
        data.resolve_map_frame(0, 2)
            .unwrap_err()
            .to_string()
            .contains("outside placed frame table")
    );
    assert!(
        data.resolve_map_frame(2, 0)
            .unwrap_err()
            .to_string()
            .contains("static placement")
    );
    assert!(
        data.resolve_map_frame(3, 0)
            .unwrap_err()
            .to_string()
            .contains("outside placed frame table")
    );
    assert!(
        data.resolve_map_frame(4, 0)
            .unwrap_err()
            .to_string()
            .contains("separate object")
    );
    assert!(
        data.resolve_map_frame(5, 0)
            .unwrap_err()
            .to_string()
            .contains("placement absent")
    );
    data.metadata["actors"][0]["raan_entries"] =
        serde_json::json!([{ "file_path":"other.3dc", "frame_count":2 }]);
    assert_eq!(data.resolve_map_frame(0, 1).unwrap().model, "OTHER");
}

#[test]
fn actor_texture_set_comes_from_raex_and_names_a_separate_frame_source() {
    use cyrus_assets::animation::{actor_texture_set, frame_source_file};
    let skin = serde_json::json!({"script_name": "A", "raex": {"texture_id": 454}});
    let plain = serde_json::json!({"script_name": "B", "raex": {"texture_id": 0}});
    let absent = serde_json::json!({"script_name": "C"});
    assert_eq!(actor_texture_set(&skin).unwrap(), Some(454));
    assert_eq!(actor_texture_set(&plain).unwrap(), None);
    assert_eq!(actor_texture_set(&absent).unwrap(), None);
    assert!(actor_texture_set(&serde_json::json!({"raex": {"texture_id": -1}})).is_err());
    assert_eq!(frame_source_file("THUGA003", Some(454)), "THUGA003-454.glb");
    assert_eq!(frame_source_file("THUGA003", None), "THUGA003.glb");
}
