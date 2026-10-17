use cyrus_assets::animation::Animations;
use cyrus_world::animation::AnimationSound;
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
fn model_initialization_preserves_the_selected_frame_and_shared_ticks() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},
            {"opcode":0,"frame_reference":12},{"opcode":2,"value":0}]}
    ]));
    for (ticks, next_frame) in [(0, 10), (1, 11), (2, 11)] {
        let (mut animation, frame) =
            cyrus_world::animation::ActorAnimation::initialize(&data, "ACTOR", ticks, &mut |_| {
                Ok(())
            })
            .unwrap();
        assert_eq!(frame, Some(10));
        assert_eq!(animation.default_group(), 0);
        assert_eq!(animation.current_group(), Some(0));
        assert_eq!(animation.pending_group(), None);
        assert_eq!(animation.advance(1).unwrap(), Some(next_frame));
    }
}

#[test]
fn changed_current_default_replaces_immediately_and_preserves_sound_order() {
    let data = asset(json!([
        {"group_index":7,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":4,"sound_param":4,"volume_shift":0},{"opcode":0,"frame_reference":70}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":4,"sound_param":8,"volume_shift":0},{"opcode":0,"frame_reference":80},
            {"opcode":0,"frame_reference":81},{"opcode":2,"value":1}]}
    ]));
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    actor.advance_with_sounds(1, &mut sounds).unwrap();
    assert_eq!(
        actor.select_default_with_sounds(8, 1, &mut sounds).unwrap(),
        Some(80)
    );
    assert_eq!(actor.current_group(), Some(8));
    assert_eq!(actor.default_group(), 8);
    assert_eq!(
        actor.select_default_with_sounds(8, 1, &mut sounds).unwrap(),
        None
    );
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(81));
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: 4,
                rate_offset_hz: 0
            },
            AnimationSound::Forget,
            AnimationSound::Play {
                effect: 8,
                rate_offset_hz: 0
            },
        ]
    );
}

#[test]
fn changed_default_retargets_pending_request_using_ordinary_priority() {
    let data = asset(json!([
        {"group_index":1,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10}]},
        {"group_index":7,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":70}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":80}]},
        {"group_index":9,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":90}]}
    ]));
    for (new_default, pending) in [(8, 8), (1, 7)] {
        let mut actor =
            cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
        actor.request(9, 0, 0).unwrap();
        actor.request(7, 0, 0).unwrap();
        assert_eq!(
            actor
                .select_default_with_sounds(new_default, 0, &mut |_| Ok(()))
                .unwrap(),
            None
        );
        assert_eq!(actor.current_group(), Some(9));
        assert_eq!(actor.default_group(), new_default);
        assert_eq!(actor.pending_group(), Some(pending));
    }
}

#[test]
fn changed_default_does_not_force_an_old_default_past_its_exit_marker() {
    let data = asset(json!([
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":0,"frame_reference":70},{"opcode":3,"value":3},
            {"opcode":2,"value":0},{"opcode":0,"frame_reference":71},{"opcode":1}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":80}]},
        {"group_index":9,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":90}]}
    ]));
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 9).unwrap();
    actor.request(7, 0, 1).unwrap();
    actor.request(9, 0, 1).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(71));
    actor
        .select_default_with_sounds(7, 0, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(actor.current_group(), Some(7));
    assert_eq!(actor.pending_group(), Some(9));
    assert_eq!(
        actor
            .select_default_with_sounds(8, 0, &mut |_| Ok(()))
            .unwrap(),
        None
    );
    assert_eq!(actor.current_group(), Some(7));
    assert_eq!(actor.pending_group(), Some(9));
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
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
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
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
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
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
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
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
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
        let mut actor =
            cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
        let error = actor
            .advance_with_sounds(0, &mut |_| Ok(()))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("world 17 actor ACTOR group 7 command 0: sound"),
            "{error}"
        );
        assert_eq!(actor.failure(), Some(error.as_str()));
        assert_eq!(actor.advance(0).unwrap_err().to_string(), error);
    }
    let data = asset(
        json!([{"group_index":7,"anim_id":1,"anim_type":1,"commands":[
        {"opcode":4,"sound_param":4,"volume_shift":0},{"opcode":0,"frame_reference":0}]}]),
    );
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
    let error = actor
        .advance_with_sounds(0, &mut |_| anyhow::bail!("channel unavailable"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("channel unavailable"));
    assert!(error.starts_with("world 17 actor ACTOR group 7 command 0:"));
    assert!(actor.advance_with_sounds(0, &mut |_| Ok(())).is_err());
}

#[test]
fn collision_checkpoint_restores_cursor_pending_and_sound_state_but_keeps_default() {
    use cyrus_world::animation::ActorAnimation;
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":4,"sound_param":4,"volume_shift":0},
            {"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":1}]},
        {"group_index":7,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":70},{"opcode":2,"value":0}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":80}]}
    ]));
    let mut events = Vec::new();
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 8).unwrap();
    actor
        .request_with_sounds(0, 0, 0, &mut |sound| {
            events.push(sound);
            Ok(())
        })
        .unwrap();
    actor
        .request_with_sounds(7, 0, 0, &mut |sound| {
            events.push(sound);
            Ok(())
        })
        .unwrap();
    let checkpoint = actor.checkpoint().unwrap();
    actor
        .select_default_with_sounds(7, 0, &mut |sound| {
            events.push(sound);
            Ok(())
        })
        .unwrap();
    actor
        .force_request_with_sounds(8, 0, 0, &mut |sound| {
            events.push(sound);
            Ok(())
        })
        .unwrap();
    assert_eq!(actor.current_group(), Some(8));
    actor.restore_checkpoint(&checkpoint).unwrap();
    assert_eq!(actor.default_group(), 7);
    assert_eq!(actor.current_group(), Some(0));
    assert_eq!(actor.pending_group(), Some(7));
    let count = events.len();
    assert_eq!(
        actor
            .advance_with_sounds(1, &mut |sound| {
                events.push(sound);
                Ok(())
            })
            .unwrap(),
        Some(10)
    );
    assert_eq!(
        actor
            .advance_with_sounds(1, &mut |sound| {
                events.push(sound);
                Ok(())
            })
            .unwrap(),
        Some(11)
    );
    assert_eq!(
        events.len(),
        count,
        "restoring does not replay the earlier sound command"
    );
    actor
        .advance_with_sounds(1, &mut |sound| {
            events.push(sound);
            Ok(())
        })
        .unwrap();
    actor
        .advance_with_sounds(1, &mut |sound| {
            events.push(sound);
            Ok(())
        })
        .unwrap();
    assert!(events[count..].contains(&AnimationSound::Stop));
}

fn type_two_asset() -> Animations {
    asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":9}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":8}]},
        {"group_index":7,"anim_id":1,"anim_type":2,"commands":[
            {"opcode":0,"frame_reference":0},{"opcode":3,"value":3},
            {"opcode":0,"frame_reference":1},{"opcode":5},
            {"opcode":0,"frame_reference":2},{"opcode":1}]}
    ]))
}

#[test]
fn type_two_ignores_a_pending_default_at_go_to_future() {
    let mut actor =
        cyrus_world::animation::ActorAnimation::from_asset(&type_two_asset(), "ACTOR", 0).unwrap();
    assert_eq!(actor.request(7, 0, 1).unwrap(), Some(0));
    actor.request(0, 0, 1).unwrap();
    // Type one would jump to the exit tail (frame 2); type two plays frame 1 first.
    assert_eq!(actor.advance(1).unwrap(), Some(1));
    assert!(!actor.exit_reached());
    assert_eq!(actor.current_group(), Some(7));
}

#[test]
fn type_two_exits_for_a_pending_non_default_group() {
    let mut actor =
        cyrus_world::animation::ActorAnimation::from_asset(&type_two_asset(), "ACTOR", 0).unwrap();
    assert_eq!(actor.request(7, 0, 1).unwrap(), Some(0));
    actor.request(8, 0, 0).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(2));
    assert!(actor.exit_reached());
    assert_eq!(actor.advance(1).unwrap(), Some(8));
    assert_eq!(actor.current_group(), Some(8));
}
