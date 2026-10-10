//! Contracts for cached animation inputs, independent of an installed game.
use cyrus_game::animation::Animations;

fn asset() -> Animations {
    serde_json::from_value(serde_json::json!({
        "world": 17,
        "sections": {"RAHD": [1, 2, 3]},
        "metadata": {"actors": [{"script_name": "ACTOR", "raan_entries": [{"file_path": "fxart/test.3dc", "frame_count": 2}], "animation_groups": [{"group_index": 7, "commands": [{"opcode": 4, "sound_param": 9}]}]}]},
        "models": {"TEST": {
            "base_positions": [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            "base_normals": [[0, 0, 1]], "faces": [[0, 1, 2]],
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
fn plugin_loads_animation_sidecar_through_asset_server() {
    use bevy::prelude::*;
    use cyrus_game::animation::AnimationPlugin;
    let directory = std::env::temp_dir().join(format!("cyrus-animation-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("test.animations"),
        serde_json::to_vec(&asset()).unwrap(),
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: directory.to_string_lossy().into_owned(),
            ..default()
        },
        AnimationPlugin,
    ));
    let handle: Handle<Animations> = app
        .world()
        .resource::<AssetServer>()
        .load("test.animations");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        if let Some(loaded) = app.world().resource::<Assets<Animations>>().get(&handle) {
            assert_eq!(loaded.world, 17);
            assert_eq!(loaded.models["TEST"].positions[1][2], [0.0, 1.0, 2.0]);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "animation asset never loaded: {:?}",
            app.world()
                .resource::<AssetServer>()
                .load_state(handle.id())
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(app);
    std::fs::remove_dir_all(directory).unwrap();
}
