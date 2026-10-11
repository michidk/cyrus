//! Authored task scheduling through public APIs, with synthetic program descriptors.
use cyrus_game::animation::Animations;

fn asset() -> Animations {
    serde_json::from_value(serde_json::json!({
        "version":2, "world":17, "sections":{}, "unsupported":{},
        "models":{"TEST":{
            "base_positions":[[0,0,0],[1,0,0],[0,1,0]],
            "base_normals":[[0,0,1]], "faces":[[0,1,2]], "encoding":"animated_i32",
            "positions":[[[0,0,0],[1,0,0],[0,1,0]],[[0,0,2],[1,0,2],[0,1,2]]],
            "normals":[[],[[0,0,1]]]
        }},
        "metadata":{
            "actors":[{"script_name":"ACTOR",
                "raan_entries":[{"file_path":"test.3dc","frame_count":2}],
                "animation_groups":[
                    {"group_index":1,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":1},{"opcode":2,"value":0},{"opcode":1}]},
                    {"group_index":10,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0},{"opcode":1}]}
                ]}],
            "animation_scripts":{"ACTOR":{"version":1,"entry":0,"steps":{
                "0":{"next":14,"operation":{"type":"task","function":"PushAnimation","arguments":[7,1]}},
                "14":{"next":28,"operation":{"type":"task","function":"PushAnimation","arguments":[10,257]}},
                "28":{"next":33,"operation":{"type":"end","target":0}}
            }}}
        }
    })).unwrap()
}

#[test]
fn request_count_wait_and_end_restart_are_separate_script_stages() {
    let data = asset();
    let mut actor = data.actor_animation("ACTOR", 1).unwrap();
    let mut script = data.animation_script("actor").unwrap();
    actor.request(1, 0, 0).unwrap();
    assert_eq!(script.advance(&mut actor, 0).unwrap(), Some(1));
    assert_eq!(script.program_counter(), 14);
    assert_eq!(script.waiting_for_group(), Some(7));

    assert_eq!(actor.advance(1).unwrap(), Some(1));
    assert_eq!(
        actor.advance(1).unwrap(),
        Some(1),
        "the request invocation must not set the finite count"
    );
    assert_eq!(actor.current_group(), Some(7));
    assert_eq!(script.advance(&mut actor, 0).unwrap(), None);
    assert_eq!(actor.advance(1).unwrap(), Some(0));
    assert_eq!(actor.current_group(), Some(1));

    assert_eq!(script.advance(&mut actor, 1).unwrap(), Some(0));
    assert_eq!(script.waiting_for_group(), Some(10));
    assert_eq!(script.program_counter(), 28);
    script.advance(&mut actor, 0).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(0));
    assert_eq!(
        actor.current_group(),
        Some(1),
        "257 wraps to a byte loop count of one"
    );
    assert_eq!(script.advance(&mut actor, 0).unwrap(), None);
    assert_eq!(script.waiting_for_group(), None);
    assert_eq!(
        script.program_counter(),
        0,
        "End saves its target and yields without restarting in this stage"
    );
    assert_eq!(actor.current_group(), Some(1));
    assert_eq!(
        script.advance(&mut actor, 0).unwrap(),
        Some(1),
        "a later eligible stage resumes the authored sequence"
    );
    assert_eq!(script.waiting_for_group(), Some(7));
}

#[test]
fn a_task_already_current_starts_its_count_without_restarting_the_frame() {
    let data = asset();
    let mut actor = data.actor_animation("ACTOR", 1).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut script = data.animation_script("ACTOR").unwrap();
    assert_eq!(script.advance(&mut actor, 0).unwrap(), None);
    assert_eq!(actor.advance(1).unwrap(), Some(1));
    assert_eq!(actor.advance(1).unwrap(), Some(0));
    assert_eq!(actor.current_group(), Some(1));
    script.advance(&mut actor, 0).unwrap();
    assert_eq!(script.waiting_for_group(), Some(10));
}

#[test]
fn a_default_group_task_does_not_finish_when_the_same_group_restarts() {
    let mut data = asset();
    data.metadata["actors"][0]["animation_groups"][0]["commands"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"opcode":1}));
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["0"]["operation"]["arguments"] =
        serde_json::json!([1, 1]);
    let mut actor = data.actor_animation("ACTOR", 1).unwrap();
    actor.request(1, 0, 0).unwrap();
    let mut script = data.animation_script("ACTOR").unwrap();
    script.advance(&mut actor, 0).unwrap();
    actor.advance(1).unwrap();
    actor.advance(1).unwrap();
    assert_eq!(actor.current_group(), Some(1));
    assert_eq!(script.advance(&mut actor, 0).unwrap(), None);
    assert_eq!(script.waiting_for_group(), Some(1));
    assert_eq!(
        script.program_counter(),
        14,
        "task completion depends on group identity, not a finite duration"
    );
}

#[test]
fn unsupported_operations_stop_with_a_sticky_world_actor_and_instruction_error() {
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["0"]["operation"]["function"] =
        "Sound".into();
    let mut actor = data.actor_animation("ACTOR", 1).unwrap();
    let mut script = data.animation_script("ACTOR").unwrap();
    let error = script.advance(&mut actor, 0).unwrap_err().to_string();
    assert_eq!(
        error,
        "world 17 actor ACTOR script instruction 0: task Sound unsupported"
    );
    assert_eq!(
        script.advance(&mut actor, 1).unwrap_err().to_string(),
        error
    );
}

#[test]
fn unsupported_entry_channels_and_invalid_cache_continuations_are_rejected() {
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["version"] = 99.into();
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("program version 99")
    );
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["entry"] = 14.into();
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("interrupt entry")
    );
    let mut data = asset();
    data.metadata["actors"][0]["attributes"] = serde_json::json!({"attr_29":1});
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("synchronization")
    );
    data.metadata["actors"][0]["attributes"]["attr_29"] = (-1).into();
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("invalid attribute 29")
    );
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["28"]["operation"]["target"] = 1.into();
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("End target 1")
    );
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["0"]["next"] = 0.into();
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("task continuation")
    );
    let mut data = asset();
    data.metadata
        .as_object_mut()
        .unwrap()
        .remove("animation_scripts");
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("re-export")
    );
    let mut data = asset();
    data.version = 1;
    assert!(
        data.animation_script("ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("schema 1 unsupported")
    );
}

#[test]
fn scripted_playback_deforms_only_its_mesh_and_world_change_restores_it() {
    use bevy::{
        asset::RenderAssetUsages,
        gltf::GltfExtras,
        mesh::{PrimitiveTopology, VertexAttributeValues},
        prelude::*,
    };
    use cyrus_game::animation::{
        AnimationPlayback, AnimationPlugin, AnimationPose, AnimationRequest, AnimationTick,
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin))
        .init_asset::<Mesh>();
    let data = asset();
    let source = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let playback = AnimationPlayback::new_scripted(
        source.clone(),
        app.world()
            .resource::<Assets<Animations>>()
            .get(&source)
            .unwrap(),
        "ACTOR",
        1,
    )
    .unwrap();
    app.world_mut().resource_mut::<AnimationRequest>().0 = source;
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
        .spawn((Name::new("B_001_ACTOR"), playback))
        .id();
    let primitive = app
        .world_mut()
        .spawn((
            Mesh3d(original.clone()),
            ChildOf(node),
            GltfExtras {
                value: r#"{"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}"#
                    .into(),
            },
        ))
        .id();
    let other = app.world_mut().spawn(Mesh3d(original.clone())).id();
    app.update();
    assert!(
        app.world().get::<AnimationPose>(node).is_none(),
        "render updates supply no nominal ticks"
    );
    app.world_mut().write_message(AnimationTick { ticks: 0 });
    app.update();
    assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 1);
    let private = app.world().get::<Mesh3d>(primitive).unwrap().0.clone();
    assert_ne!(private, original);
    assert_eq!(app.world().get::<Mesh3d>(other).unwrap().0, original);
    let meshes = app.world().resource::<Assets<Mesh>>();
    let VertexAttributeValues::Float32x3(positions) = meshes
        .get(&private)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
    else {
        panic!("positions");
    };
    assert_eq!(
        positions,
        &[[0.0, 0.0, 0.1], [-0.05, 0.0, 0.1], [0.0, -0.05, 0.1]]
    );
    app.world_mut().resource_mut::<AnimationRequest>().0 = Handle::default();
    app.update();
    assert!(app.world().get::<AnimationPlayback>(node).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());
    assert_eq!(app.world().get::<Mesh3d>(primitive).unwrap().0, original);
}

#[test]
fn an_animation_task_keeps_waiting_while_its_current_group_is_delayed() {
    let mut data = asset();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = serde_json::json!([
        {"opcode":12,"value":2}, {"opcode":0,"frame_reference":1}, {"opcode":1}
    ]);
    let mut actor = data.actor_animation("ACTOR", 1).unwrap();
    let mut script = data.animation_script("ACTOR").unwrap();
    actor.advance(0).unwrap();
    assert_eq!(script.advance(&mut actor, 0).unwrap(), None);
    for ticks in [0, 1] {
        assert_eq!(actor.advance(ticks).unwrap(), None);
        assert_eq!(script.advance(&mut actor, ticks).unwrap(), None);
        assert_eq!(actor.current_group(), Some(7));
        assert_eq!(script.waiting_for_group(), Some(7));
        assert_eq!(script.program_counter(), 14);
    }
    assert_eq!(actor.advance(1).unwrap(), Some(1));
    assert_eq!(script.advance(&mut actor, 1).unwrap(), None);
    assert_eq!(script.waiting_for_group(), Some(7));
    actor.advance(1).unwrap();
    assert_eq!(script.advance(&mut actor, 1).unwrap(), Some(0));
    assert_eq!(script.waiting_for_group(), Some(10));
    assert_eq!(script.program_counter(), 28);
}
