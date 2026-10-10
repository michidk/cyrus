//! Public animation contracts. The exported-model check reports a missing install.
use cyrus_game::animation::Animations;

fn asset() -> Animations {
    serde_json::from_value(serde_json::json!({
        "version": 2, "world": 17,
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
fn frame_projection_keeps_coincident_source_vertices_distinct() {
    use cyrus_game::animation::MeshFrameBinding;
    let mut input = asset();
    let model = input.models.get_mut("TEST").unwrap();
    model.encoding = "animated_i32".into();
    model.base_positions[2] = model.base_positions[0];
    model.positions[0][2] = model.positions[0][0];
    model.positions[1][2] = [20.0, 40.0, 60.0];
    model.normals[1][0] = [0.0, 1.0, 0.0];
    let binding = MeshFrameBinding {
        version: 1,
        source_vertices: vec![[2, 0], [0, 0], [1, 0], [2, 0]],
    };
    let frame = model.frame_geometry(1, &binding).unwrap();
    assert_eq!(
        frame.positions,
        [
            [-1.0, -2.0, 3.0],
            [0.0, 0.0, 0.1],
            [-0.05, 0.0, 0.1],
            [-1.0, -2.0, 3.0]
        ]
    );
    assert_eq!(frame.normals, [[0.0, -1.0, 0.0]; 4]);
}

#[test]
fn frame_projection_rejects_unknown_mapping_and_unproven_encoding() {
    use cyrus_game::animation::MeshFrameBinding;
    let input = asset();
    let model = &input.models["TEST"];
    let mut binding = MeshFrameBinding {
        version: 2,
        source_vertices: vec![[0, 0]],
    };
    assert_eq!(
        model.frame_geometry(0, &binding).err().unwrap().to_string(),
        "mesh frame mapping version 2 unsupported"
    );
    binding.version = 1;
    assert_eq!(
        model.frame_geometry(1, &binding).err().unwrap().to_string(),
        "frame 1: playback for animated_i16 is unsupported"
    );
    binding.source_vertices[0][0] = 3;
    assert_eq!(
        model.frame_geometry(0, &binding).err().unwrap().to_string(),
        "frame 0: vertex 3 does not belong to face 0"
    );
}

#[test]
fn explicit_pose_deforms_a_private_mesh_and_scene_change_restores_it() {
    use bevy::{
        asset::RenderAssetUsages,
        gltf::GltfExtras,
        mesh::{PrimitiveTopology, VertexAttributeValues},
        prelude::*,
    };
    use cyrus_game::animation::{AnimationPlugin, AnimationPose, AnimationRequest};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin))
        .init_asset::<Mesh>();
    let mut data = asset();
    data.models.get_mut("TEST").unwrap().encoding = "animated_i32".into();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.world_mut().resource_mut::<AnimationRequest>().0 = handle;
    let original = app.world_mut().resource_mut::<Assets<Mesh>>().add(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [-0.05, 0.0, 0.0], [0.0, -0.05, 0.0]],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3]),
    );
    let node = app
        .world_mut()
        .spawn((
            Name::new("B_000_ACTOR"),
            AnimationPose {
                model: "TEST".into(),
                frame: 1,
            },
        ))
        .id();
    let primitive = app
        .world_mut()
        .spawn((
            ChildOf(node),
            Mesh3d(original.clone()),
            GltfExtras {
                value: r#"{"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}"#
                    .into(),
            },
        ))
        .id();
    let incompatible = app.world_mut().resource_mut::<Assets<Mesh>>().add(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[9.0, 0.0, 0.0]; 3]),
    );
    let rejected = app
        .world_mut()
        .spawn((
            ChildOf(node),
            Mesh3d(incompatible.clone()),
            GltfExtras {
                value: r#"{"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}"#
                    .into(),
            },
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Mesh3d>(rejected).unwrap().0,
        incompatible,
        "a stale or different model must not be deformed using this mapping"
    );
    let private = &app.world().get::<Mesh3d>(primitive).unwrap().0;
    assert_ne!(private, &original);
    let meshes = app.world().resource::<Assets<Mesh>>();
    let VertexAttributeValues::Float32x3(positions) = meshes
        .get(private)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
    else {
        panic!("missing positions");
    };
    assert_eq!(
        positions,
        &[[0.0, 0.0, 0.1], [-0.05, 0.0, 0.1], [0.0, -0.05, 0.1]]
    );
    app.world_mut().resource_mut::<AnimationRequest>().0 = Handle::default();
    app.update();
    assert_eq!(app.world().get::<Mesh3d>(primitive).unwrap().0, original);
}

fn sequence_asset(speed: u16, commands: serde_json::Value) -> Animations {
    let mut data = asset();
    data.metadata["actors"][0]["animation_groups"] = serde_json::json!([{
        "group_index": 7, "anim_type": 0, "anim_id": speed, "commands": commands
    }]);
    data
}

fn request_asset() -> Animations {
    let mut data = asset();
    data.metadata["actors"][0]["animation_groups"] = serde_json::json!([
        {"group_index":1,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1}, {"opcode":2,"value":0}
        ]},
        {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":1}, {"opcode":3,"value":3},
            {"opcode":2,"value":0}, {"opcode":1}
        ]},
        {"group_index":10,"anim_type":0,"anim_id":255,"commands":[
            {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1}, {"opcode":1}
        ]},
        {"group_index":20,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":5}, {"opcode":0,"frame_reference":0}, {"opcode":2,"value":1}
        ]}
    ]);
    data
}

#[test]
fn ordinary_default_requests_preserve_cursor_but_other_groups_start_immediately() {
    let mut state = request_asset().actor_animation("ACTOR", 1).unwrap();
    assert_eq!(state.current_group(), None);
    assert_eq!(state.request(1, 0, 0).unwrap(), Some(0));
    assert_eq!(state.advance(1).unwrap(), Some(0));
    assert_eq!(state.request(1, 0, 1).unwrap(), None);
    assert_eq!(
        state.advance(1).unwrap(),
        Some(1),
        "requesting the default must not restart it"
    );
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(1));
    assert_eq!(state.current_group(), Some(7));
    assert_eq!(state.pending_group(), None);
}

#[test]
fn pending_priority_and_exit_select_replacement_in_the_same_update() {
    let mut state = request_asset().actor_animation("ACTOR", 1).unwrap();
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(1));
    assert_eq!(state.request(20, 0, 0).unwrap(), None);
    assert_eq!(state.request(10, 0, 0).unwrap(), None);
    assert_eq!(
        state.pending_group(),
        Some(20),
        "lower requests cannot replace higher pending groups"
    );
    assert_eq!(state.advance(1).unwrap(), Some(0));
    assert_eq!(state.current_group(), Some(20));
    assert_eq!(state.pending_group(), None);
    assert!(state.translation_suppressed());
}

#[test]
fn absent_requests_fall_back_to_default_or_clear_pending() {
    let data = request_asset();
    let mut state = data.actor_animation("ACTOR", 1).unwrap();
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(1));
    state.request(20, 0, 0).unwrap();
    state.request(21, 0, 0).unwrap();
    assert_eq!(state.pending_group(), Some(1));
    assert_eq!(state.advance(1).unwrap(), Some(0));
    assert_eq!(state.current_group(), Some(1));
    let mut state = data.actor_animation("ACTOR", 9).unwrap();
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(1));
    state.request(20, 0, 0).unwrap();
    state.request(21, 0, 0).unwrap();
    assert_eq!(state.pending_group(), None);
    assert_eq!(state.current_group(), Some(7));
    assert_eq!(state.advance(1).unwrap(), Some(1));
}

#[test]
fn finite_task_loops_defer_pending_transition_without_restarting_the_group() {
    let mut state = request_asset().actor_animation("ACTOR", 1).unwrap();
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(1));
    state.set_loop_count(2).unwrap();
    state.request(20, 0, 0).unwrap();
    assert_eq!(state.advance(1).unwrap(), Some(1));
    assert_eq!(state.current_group(), Some(7));
    assert_eq!(state.advance(1).unwrap(), Some(0));
    assert_eq!(state.current_group(), Some(20));
}

#[test]
fn end_restarts_existing_default_using_the_same_ticks_and_byte_modifier_wraps() {
    let mut state = request_asset().actor_animation("ACTOR", 1).unwrap();
    assert_eq!(state.request(10, 2, 1).unwrap(), Some(0));
    assert_eq!(
        state.advance(1).unwrap(),
        Some(1),
        "255 plus modifier 2 wraps to countdown 1"
    );
    assert_eq!(state.advance(1).unwrap(), Some(0));
    assert_eq!(state.current_group(), Some(1));
    assert_eq!(
        state.advance(1).unwrap(),
        Some(1),
        "the replacement already consumed the preceding tick input"
    );
}

#[test]
fn forced_current_request_cancels_pending_without_restarting_and_other_force_replaces() {
    let mut state = request_asset().actor_animation("ACTOR", 9).unwrap();
    assert_eq!(state.request(1, 0, 1).unwrap(), Some(0));
    state.request(20, 0, 0).unwrap();
    assert_eq!(state.force_request(1, 99, 1).unwrap(), None);
    assert_eq!(state.pending_group(), None);
    assert_eq!(state.advance(1).unwrap(), Some(1));
    state.request(20, 0, 0).unwrap();
    assert_eq!(state.force_request(7, 0, 1).unwrap(), Some(1));
    assert_eq!(state.current_group(), Some(7));
    assert_eq!(state.pending_group(), None);
}

#[test]
fn unsupported_replacement_is_sticky_and_empty_default_cycles_fail() {
    let mut data = request_asset();
    data.metadata["actors"][0]["animation_groups"][3]["anim_type"] = 1.into();
    let mut state = data.actor_animation("ACTOR", 1).unwrap();
    state.request(7, 0, 1).unwrap();
    state.request(20, 0, 0).unwrap();
    let message = state.advance(1).unwrap_err().to_string();
    assert_eq!(
        message,
        "world 17 actor ACTOR group 20: group type unsupported"
    );
    assert_eq!(
        state.force_request(1, 0, 1).unwrap_err().to_string(),
        message
    );
    let data = sequence_asset(1, serde_json::json!([{"opcode":1}]));
    let mut state = data.actor_animation("ACTOR", 7).unwrap();
    assert_eq!(
        state.advance(0).unwrap_err().to_string(),
        "world 17 actor ACTOR group 7: replacement cycle has no displayed frame"
    );
}

#[test]
fn only_the_low_group_type_byte_is_interpreted() {
    let mut data = request_asset();
    data.metadata["actors"][0]["animation_groups"][0]["anim_type"] = 256.into();
    assert_eq!(
        data.sequence("ACTOR", 1, 0)
            .unwrap()
            .advance(0, false)
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        data.actor_animation("ACTOR", 1)
            .unwrap()
            .request(1, 0, 0)
            .unwrap(),
        Some(0)
    );
}

#[test]
fn returning_to_the_same_default_keeps_group_identity_for_waiting_tasks() {
    let data = sequence_asset(
        1,
        serde_json::json!([
            {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1},
            {"opcode":3,"value":4}, {"opcode":2,"value":0}, {"opcode":1}
        ]),
    );
    let mut state = data.actor_animation("ACTOR", 7).unwrap();
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(0));
    state.set_loop_count(2).unwrap();
    let frames = [1, 1, 1, 1].map(|ticks| state.advance(ticks).unwrap());
    assert_eq!(frames, [Some(1), Some(0), Some(1), Some(0)]);
    assert_eq!(
        state.current_group(),
        Some(7),
        "finite loops alone do not complete a task waiting for the group to change"
    );
    assert_eq!(state.pending_group(), None);
}

#[test]
fn force_after_an_exit_restarts_the_matching_group() {
    let mut data = request_asset();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = serde_json::json!([
        {"opcode":0,"frame_reference":0}, {"opcode":3,"value":2},
        {"opcode":0,"frame_reference":1}, {"opcode":1}
    ]);
    let mut state = data.actor_animation("ACTOR", 1).unwrap();
    assert_eq!(state.request(7, 0, 1).unwrap(), Some(0));
    state.request(20, 0, 0).unwrap();
    assert_eq!(state.advance(1).unwrap(), Some(1));
    assert_eq!(state.current_group(), Some(7));
    assert_eq!(state.force_request(7, 0, 1).unwrap(), Some(0));
    assert_eq!(state.pending_group(), None);
}

#[test]
fn malformed_and_old_request_assets_report_the_source_context() {
    let mut data = request_asset();
    data.version = 1;
    assert_eq!(
        data.actor_animation("ACTOR", 1).err().unwrap().to_string(),
        "world 17: animation schema 1 unsupported; re-export the world"
    );
    let mut data = request_asset();
    data.metadata["actors"][0]["animation_groups"][1]["group_index"] = 1.into();
    assert_eq!(
        data.actor_animation("ACTOR", 1).err().unwrap().to_string(),
        "world 17 actor ACTOR group 1: duplicate group"
    );
}

#[test]
fn finite_loop_count_and_multi_tick_skip_follow_command_stream() {
    let data = sequence_asset(
        1,
        serde_json::json!([
            {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1},
            {"opcode":3,"value":4}, {"opcode":2,"value":0}, {"opcode":1}
        ]),
    );
    let mut sequence = data.sequence("ACTOR", 7, 2).unwrap();
    let mut selected = Vec::new();
    for _ in 0..5 {
        selected.push(sequence.advance(1, false).unwrap());
    }
    assert_eq!(selected, [Some(0), Some(1), Some(0), Some(1), None]);
    assert_eq!(sequence.advance(0, false).unwrap(), None);
    let mut skipped = data.sequence("ACTOR", 7, 2).unwrap();
    assert_eq!(skipped.advance(1, false).unwrap(), Some(0));
    assert_eq!(skipped.advance(3, false).unwrap(), Some(1));
    assert_eq!(skipped.advance(1, false).unwrap(), None);
}

#[test]
fn frame_countdown_and_zero_tick_updates_do_not_become_render_frame_playback() {
    let data = sequence_asset(
        3,
        serde_json::json!([
            {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1},
            {"opcode":0,"frame_reference":2}, {"opcode":1}
        ]),
    );
    let mut sequence = data.sequence("ACTOR", 7, 1).unwrap();
    let observed = [1, 5, 0, 1, 2].map(|ticks| sequence.advance(ticks, false).unwrap());
    assert_eq!(observed, [Some(0), Some(0), Some(0), Some(0), Some(2)]);
}

#[test]
fn unsupported_sound_cue_is_reported_without_skipping_to_a_later_frame() {
    let data = sequence_asset(
        1,
        serde_json::json!([
            {"opcode":0,"frame_reference":0}, {"opcode":4,"sound_param":9},
            {"opcode":0,"frame_reference":1}, {"opcode":1}
        ]),
    );
    let mut sequence = data.sequence("ACTOR", 7, 1).unwrap();
    assert_eq!(sequence.advance(1, false).unwrap(), Some(0));
    assert_eq!(
        sequence.advance(1, false).unwrap_err().to_string(),
        "world 17 actor ACTOR group 7 command 1: opcode 4 playback unsupported"
    );
    assert_eq!(
        sequence.advance(3, true).unwrap_err().to_string(),
        "world 17 actor ACTOR group 7 command 1: opcode 4 playback unsupported"
    );
}

#[test]
fn file_frame_reference_crosses_raan_model_boundaries() {
    let mut data = asset();
    data.models.insert(
        "SECOND".into(),
        serde_json::from_value(serde_json::to_value(&data.models["TEST"]).unwrap()).unwrap(),
    );
    data.metadata["actors"][0]["raan_entries"] = serde_json::json!([
        {"file_path":"3dart/test.3d", "frame_count":2},
        {"file_path":"fxart/second.3dc", "frame_count":2}
    ]);
    assert_eq!(
        data.resolve_frame("actor", 3).unwrap(),
        cyrus_game::animation::ModelFrame {
            model: "SECOND".into(),
            frame: 1
        }
    );
    assert_eq!(
        data.resolve_frame("ACTOR", 4).unwrap_err().to_string(),
        "world 17 actor ACTOR frame reference 4: outside RAAN frame table"
    );
}

#[test]
fn activated_group_uses_supplied_ticks_and_stops_at_end() {
    use bevy::prelude::*;
    use cyrus_game::animation::{
        AnimationPlayback, AnimationPlugin, AnimationPose, AnimationRequest, AnimationTick,
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin));
    let mut data = sequence_asset(
        1,
        serde_json::json!([
            {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1}, {"opcode":1}
        ]),
    );
    data.models.get_mut("TEST").unwrap().encoding = "animated_i32".into();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let playback = AnimationPlayback::new(
        handle.clone(),
        app.world()
            .resource::<Assets<Animations>>()
            .get(&handle)
            .unwrap(),
        "ACTOR",
        7,
        1,
    )
    .unwrap();
    app.world_mut().resource_mut::<AnimationRequest>().0 = handle;
    let node = app
        .world_mut()
        .spawn((Name::new("B_000_ACTOR"), playback))
        .id();
    app.update();
    assert!(
        app.world().get::<AnimationPose>(node).is_none(),
        "render updates must not invent simulation ticks"
    );
    for (ticks, expected) in [(0, 0), (1, 0), (0, 0), (1, 1)] {
        app.world_mut().write_message(AnimationTick { ticks });
        app.update();
        let pose = app.world().get::<AnimationPose>(node).unwrap();
        assert_eq!((&*pose.model, pose.frame), ("TEST", expected));
    }
    app.world_mut().write_message(AnimationTick { ticks: 1 });
    app.update();
    assert!(app.world().get::<AnimationPlayback>(node).is_none());
    assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 1);
}

#[test]
fn changing_world_clears_playback_and_pose_without_a_simulation_tick() {
    use bevy::prelude::*;
    use cyrus_game::animation::{
        AnimationPlayback, AnimationPlugin, AnimationPose, AnimationRequest, AnimationTick,
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin));
    let data = sequence_asset(1, serde_json::json!([{"opcode":0,"frame_reference":0}]));
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let playback = AnimationPlayback::new(
        handle.clone(),
        app.world()
            .resource::<Assets<Animations>>()
            .get(&handle)
            .unwrap(),
        "ACTOR",
        7,
        0,
    )
    .unwrap();
    app.world_mut().resource_mut::<AnimationRequest>().0 = handle;
    let node = app
        .world_mut()
        .spawn((Name::new("B_000_ACTOR"), playback))
        .id();
    app.world_mut().write_message(AnimationTick { ticks: 0 });
    app.update();
    assert!(app.world().get::<AnimationPose>(node).is_some());
    app.world_mut().resource_mut::<AnimationRequest>().0 = Handle::default();
    app.update();
    assert!(app.world().get::<AnimationPlayback>(node).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());
}

#[test]
fn a_control_cycle_fails_instead_of_hanging_the_update() {
    let data = sequence_asset(1, serde_json::json!([{"opcode":2,"value":0}]));
    let mut sequence = data.sequence("ACTOR", 7, 0).unwrap();
    assert_eq!(
        sequence.advance(0, false).unwrap_err().to_string(),
        "world 17 actor ACTOR group 7 command 0: control cycle has no displayed frame"
    );
    assert!(sequence.advance(1, false).is_err());
}

#[test]
fn breakpoint_blocks_walking_until_the_frame_cursor_advances() {
    let data = sequence_asset(
        2,
        serde_json::json!([
            {"opcode":5}, {"opcode":0,"frame_reference":0},
            {"opcode":0,"frame_reference":1}, {"opcode":1}
        ]),
    );
    let mut sequence = data.sequence("ACTOR", 7, 1).unwrap();
    assert!(!sequence.translation_suppressed());
    for (ticks, frame, suppressed) in [
        (0, 0, true),
        (1, 0, true),
        (0, 0, true),
        (1, 0, true),
        (1, 1, false),
    ] {
        assert_eq!(sequence.advance(ticks, false).unwrap(), Some(frame));
        assert_eq!(sequence.translation_suppressed(), suppressed);
    }
    let data = sequence_asset(
        1,
        serde_json::json!([
            {"opcode":0,"frame_reference":0}, {"opcode":5},
            {"opcode":0,"frame_reference":1}, {"opcode":2,"value":0}, {"opcode":1}
        ]),
    );
    let mut sequence = data.sequence("ACTOR", 7, 2).unwrap();
    for (frame, suppressed) in [(0, false), (1, true), (0, false)] {
        assert_eq!(sequence.advance(1, false).unwrap(), Some(frame));
        assert_eq!(sequence.translation_suppressed(), suppressed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn exported_island_flag_mapping_binds_a_real_frame_and_restores_the_mesh() {
    use bevy::{
        asset::RenderAssetUsages,
        gltf::{GltfExtras, gltf::Gltf},
        mesh::{PrimitiveTopology, VertexAttributeValues},
        prelude::*,
    };
    use cyrus_game::{
        animation::{AnimationPlugin, AnimationPose, AnimationRequest},
        install::GameInstall,
    };
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let install = match GameInstall::open(&root) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("skipped exported animation contract: no install ({error})");
            return;
        }
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("animation-export-test-{}", std::process::id()));
    let path = directory.join("ISLAND.glb");
    cyrus_game::scene::export_scene(&install, "ISLAND", &path).unwrap();
    let data: Animations =
        serde_json::from_slice(&std::fs::read(path.with_extension("animations")).unwrap()).unwrap();
    data.validate().unwrap();
    assert!(data.sections.contains_key("RASC"));
    let mut script = data.animation_script("FLAG2").unwrap();
    let mut actor = data.actor_animation("FLAG2", 0).unwrap();
    let mut transitions = Vec::new();
    let mut ended = false;
    for _ in 0..10_000 {
        actor.advance(1).unwrap();
        actor.request(0, 0, 1).unwrap();
        let before = script.program_counter();
        script.advance(&mut actor, 1).unwrap();
        if script.program_counter() != before {
            if script.waiting_for_group().is_none() {
                ended = true;
                break;
            }
            transitions.push(script.waiting_for_group().unwrap());
        }
    }
    assert!(ended, "the installed finite tasks must reach End");
    assert_eq!(transitions, [1, 10, 1, 10]);
    assert_eq!(script.program_counter(), 0);
    script.advance(&mut actor, 1).unwrap();
    assert_eq!(script.waiting_for_group(), Some(1));
    let resolved = data.resolve_frame("FLAG2", 2).unwrap();
    assert_eq!((&*resolved.model, resolved.frame), ("FLAGA01", 2));
    let bytes = std::fs::read(&path).unwrap();
    let glb = Gltf::from_slice(&bytes).unwrap();
    let node = glb
        .nodes()
        .find(|node| node.name() == Some("B_346_FLAG2"))
        .unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin))
        .init_asset::<Mesh>();
    let source = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.world_mut().resource_mut::<AnimationRequest>().0 = source;
    let actor = app
        .world_mut()
        .spawn((
            Name::new("B_346_FLAG2"),
            AnimationPose {
                model: resolved.model.clone(),
                frame: resolved.frame,
            },
        ))
        .id();
    let mut primitives = Vec::new();
    for primitive in node.mesh().unwrap().primitives() {
        let reader = primitive.reader(|_| glb.blob.as_deref());
        let base: Vec<_> = reader.read_positions().unwrap().collect();
        let extras = primitive.extras().as_ref().unwrap().get();
        let original = app.world_mut().resource_mut::<Assets<Mesh>>().add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, base.clone()),
        );
        let entity = app
            .world_mut()
            .spawn((
                ChildOf(actor),
                Mesh3d(original.clone()),
                GltfExtras {
                    value: extras.into(),
                },
            ))
            .id();
        primitives.push((entity, original, base));
    }
    assert!(!primitives.is_empty());
    app.update();
    let mut moved = false;
    for (entity, original, base) in &primitives {
        let private = &app.world().get::<Mesh3d>(*entity).unwrap().0;
        assert_ne!(
            private, original,
            "the real export must bind to a private mesh"
        );
        let meshes = app.world().resource::<Assets<Mesh>>();
        let VertexAttributeValues::Float32x3(selected) = meshes
            .get(private)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
        else {
            panic!("positions absent")
        };
        moved |= selected != base;
    }
    assert!(
        moved,
        "the selected authored flag frame must change visible geometry"
    );
    app.world_mut().entity_mut(actor).remove::<AnimationPose>();
    app.update();
    for (entity, original, _) in &primitives {
        assert_eq!(&app.world().get::<Mesh3d>(*entity).unwrap().0, original);
    }
    drop(app);
    std::fs::remove_dir_all(directory).unwrap();
}
