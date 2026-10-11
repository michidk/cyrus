use cyrus_game::animation::{AnimationSound, Animations};
use serde_json::{Value, json};

fn asset(groups: Value) -> Animations {
    Animations {
        version: 2,
        world: 17,
        metadata: json!({"actors":[{"script_name":"ACTOR","animation_groups":groups}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    }
}

#[test]
fn type_one_finishes_its_exit_tail_before_selecting_the_pending_default() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":9},{"opcode":2,"value":0}]},
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":0,"frame_reference":0},{"opcode":3,"value":3},
            {"opcode":0,"frame_reference":1},{"opcode":5},
            {"opcode":0,"frame_reference":2},{"opcode":0,"frame_reference":3},
            {"opcode":1}]}
    ]));
    let mut actor = data.actor_animation("ACTOR", 0).unwrap();
    assert_eq!(actor.request(7, 0, 1).unwrap(), Some(0));
    assert_eq!(actor.request(0, 0, 1).unwrap(), None);
    for (ticks, frame, group, marker) in [
        (1, 2, 7, true),
        (0, 2, 7, true),
        (1, 3, 7, false),
        (1, 9, 0, false),
    ] {
        assert_eq!(actor.advance(ticks).unwrap(), Some(frame));
        assert_eq!(actor.current_group(), Some(group));
        assert_eq!(actor.translation_suppressed(), marker);
    }
}

#[test]
fn type_one_delay_precedes_replacement_by_a_pending_non_default() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":9}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":8}]},
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":0,"frame_reference":0},{"opcode":3,"value":3},
            {"opcode":0,"frame_reference":1},{"opcode":0,"frame_reference":2},
            {"opcode":12,"value":2},{"opcode":0,"frame_reference":3}]}
    ]));
    let mut actor = data.actor_animation("ACTOR", 0).unwrap();
    assert_eq!(actor.request(7, 0, 1).unwrap(), Some(0));
    actor.request(0, 0, 0).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(2));
    assert_eq!(actor.advance(1).unwrap(), None);
    actor.request(8, 0, 0).unwrap();
    assert_eq!(actor.advance(1).unwrap(), None);
    assert_eq!(actor.current_group(), Some(7));
    assert_eq!(actor.advance(1).unwrap(), Some(8));
    assert_eq!(actor.current_group(), Some(8));
}

#[test]
fn authored_sounds_dispatch_once_per_command_with_rate_offsets_and_end_stop() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":9}]},
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":0,"frame_reference":0},
            {"opcode":4,"sound_param":4,"volume_shift":-1},
            {"opcode":0,"frame_reference":1},
            {"opcode":4,"sound_param":7,"volume_shift":2},{"opcode":1}]}
    ]));
    let mut actor = data.actor_animation("ACTOR", 0).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    assert_eq!(
        actor.request_with_sounds(7, 0, 1, &mut sounds).unwrap(),
        Some(0)
    );
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(1));
    assert_eq!(actor.advance_with_sounds(0, &mut sounds).unwrap(), Some(1));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(9));
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: 4,
                rate_offset_hz: -64
            },
            AnimationSound::Play {
                effect: 7,
                rate_offset_hz: 128
            },
            AnimationSound::Stop,
        ]
    );
}

#[test]
fn replacing_the_current_default_forgets_its_sound_without_stopping_it() {
    let data = asset(json!([
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":4,"sound_param":4,"volume_shift":0},{"opcode":0,"frame_reference":1}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":8}]}
    ]));
    let mut actor = data.actor_animation("ACTOR", 7).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    assert_eq!(actor.advance_with_sounds(0, &mut sounds).unwrap(), Some(1));
    assert_eq!(
        actor.request_with_sounds(8, 0, 1, &mut sounds).unwrap(),
        Some(8)
    );
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: 4,
                rate_offset_hz: 0
            },
            AnimationSound::Forget
        ]
    );
}

#[test]
fn invalid_sound_fields_and_handler_failures_stop_with_context() {
    for (effect, shift) in [(512, 0), (4, -513)] {
        let data = asset(
            json!([{"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":4,"sound_param":effect,"volume_shift":shift},{"opcode":0,"frame_reference":0}]}]),
        );
        let mut actor = data.actor_animation("ACTOR", 7).unwrap();
        let error = actor
            .advance_with_sounds(0, &mut |_| Ok(()))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("world 17 actor ACTOR group 7 command 0: sound"),
            "{error}"
        );
        assert_eq!(actor.advance(0).unwrap_err().to_string(), error);
    }
    let data = asset(
        json!([{"group_index":7,"anim_id":1,"anim_type":1,"commands":[
        {"opcode":4,"sound_param":4,"volume_shift":0},{"opcode":0,"frame_reference":0}]}]),
    );
    let mut actor = data.actor_animation("ACTOR", 7).unwrap();
    let error = actor
        .advance_with_sounds(0, &mut |_| anyhow::bail!("channel unavailable"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("channel unavailable"));
    assert!(error.starts_with("world 17 actor ACTOR group 7 command 0:"));
    assert!(actor.advance_with_sounds(0, &mut |_| Ok(())).is_err());
}
