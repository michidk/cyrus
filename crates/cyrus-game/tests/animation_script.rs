//! Authored task scheduling through public APIs, with synthetic program descriptors.
use cyrus_assets::animation::Animations;

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
            "animation_scripts":{"ACTOR":{"version":2,"entry":0,"steps":{
                "0":{"next":14,"operation":{"type":"call","call":{"function_id":0,"kind":"task","target":{"type":"me"},"function":"PushAnimation","arguments":[{"type":"integer","value":7},{"type":"integer","value":1}]}}},
                "14":{"next":28,"operation":{"type":"call","call":{"function_id":0,"kind":"task","target":{"type":"me"},"function":"PushAnimation","arguments":[{"type":"integer","value":10},{"type":"integer","value":257}]}}},
                "28":{"next":33,"operation":{"type":"end","target":0}}
            }}}
        }
    })).unwrap()
}

#[test]
fn request_count_wait_and_end_restart_are_separate_script_stages() {
    let data = asset();
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 1).unwrap();
    let mut script = cyrus_game::script::AnimationScript::from_asset(&data, "actor").unwrap();
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
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 1).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut script = cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR").unwrap();
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
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["0"]["operation"]["call"]["arguments"] =
        serde_json::json!([{"type":"integer","value":1},{"type":"integer","value":1}]);
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 1).unwrap();
    actor.request(1, 0, 0).unwrap();
    let mut script = cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR").unwrap();
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
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["0"]["operation"]["call"]["function"] =
        "Sound".into();
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 1).unwrap();
    let mut script = cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR").unwrap();
    let error = script.advance(&mut actor, 0).unwrap_err().to_string();
    assert_eq!(
        error,
        "world 17 actor ACTOR script instruction 0 thread Main: task Sound unsupported"
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
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("program version 99")
    );
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["entry"] = 14.into();
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("interrupt entry")
    );
    let mut data = asset();
    data.metadata["actors"][0]["attributes"] = serde_json::json!({"attr_29":1});
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("synchronization")
    );
    data.metadata["actors"][0]["attributes"]["attr_29"] = (-1).into();
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("invalid attribute 29")
    );
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["28"]["operation"]["target"] = 1.into();
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("target 1")
    );
    let mut data = asset();
    data.metadata["animation_scripts"]["ACTOR"]["steps"]["0"]["next"] = 0.into();
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("invalid continuation")
    );
    let mut data = asset();
    data.metadata
        .as_object_mut()
        .unwrap()
        .remove("animation_scripts");
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("re-export")
    );
    let mut data = asset();
    data.version = 1;
    assert!(
        cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR")
            .err()
            .unwrap()
            .to_string()
            .contains("schema 1 unsupported")
    );
}

#[test]
fn an_animation_task_keeps_waiting_while_its_current_group_is_delayed() {
    let mut data = asset();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = serde_json::json!([
        {"opcode":12,"value":2}, {"opcode":0,"frame_reference":1}, {"opcode":1}
    ]);
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 1).unwrap();
    let mut script = cyrus_game::script::AnimationScript::from_asset(&data, "ACTOR").unwrap();
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
