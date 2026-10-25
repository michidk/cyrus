//! Reverse playback, retained direction and original command boundaries through the actor API.
use cyrus_world::animation::{ActorAnimation, ActorAnimationSnapshot, AnimationSound, Animations};
use serde_json::json;
fn data() -> Animations {
    Animations {
        version: 2,
        world: 17,
        metadata: json!({"actors":[{"script_name":"ACTOR","animation_groups":[
            {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":900},{"opcode":2,"value":0}]},
            {"group_index":7,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":70},{"opcode":0,"frame_reference":71},{"opcode":0,"frame_reference":72},{"opcode":0,"frame_reference":73},{"opcode":0,"frame_reference":74},{"opcode":1}]}
        ]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    }
}
#[test]
fn reverse_retains_direction_in_snapshots_and_checkpoints() {
    let data = data();
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["current"][1]["command"] = 3.into();
    saved["current"][1]["countdown"] = 0.into();
    saved["frame_progress"] = 250.into();
    actor = ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
        .unwrap();
    actor.set_reverse(true).unwrap();
    let checkpoint = actor.checkpoint().unwrap();
    let bytes = serde_json::to_vec(&actor.snapshot()).unwrap();
    let saved: ActorAnimationSnapshot = serde_json::from_slice(&bytes).unwrap();
    let mut restored = ActorAnimation::from_snapshot(&data, "ACTOR", saved).unwrap();
    assert!(restored.is_reversed());
    for actor in [&mut actor, &mut restored] {
        assert_eq!(actor.advance(1).unwrap(), Some(72));
        assert_eq!(actor.current_command_cursor().unwrap(), Some(2));
        assert_eq!(actor.frame_wait_state().unwrap().2, 249);
        assert_eq!(actor.advance(2).unwrap(), Some(900));
        assert!(!actor.is_reversed());
        actor.restore_checkpoint(&checkpoint).unwrap();
        assert!(actor.is_reversed());
        assert_eq!(actor.advance(1).unwrap(), Some(72));
    }
}

#[test]
fn reverse_controls_match_original_literal_scenarios() {
    for (opcode, operand, loops, cursor, remaining, delay) in [
        (2, 1, 0, 2, 0, 0),
        (2, 1, 1, 4, 0, 0),
        (2, 1, 2, 1, 1, 0),
        (3, 1, 0, 2, 0, 0),
        (3, 1, 1, 4, 1, 0),
        (3, 1, 2, 4, 2, 0),
        (5, 1, 0, 2, 0, 0),
        (6, 1, 0, 2, 0, 0),
        (7, 1, 0, 2, 0, 0),
        (8, 1, 0, 2, 0, 0),
        (9, 1, 0, 2, 0, 0),
        (10, 1, 0, 2, 0, 0),
        (13, 1, 0, 2, 0, 0),
        (14, 1, 0, 2, 0, 0),
        (15, 1, 0, 2, 0, 0),
        (12, 0, 0, 2, 0, 0),
        (12, 2, 0, 2, 0, 2),
        (12, 255, 0, 2, 0, 255),
    ] {
        let mut data = data();
        data.metadata["actors"][0]["animation_groups"][1]["commands"][3] =
            json!({"opcode":opcode,"value":operand});
        let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
        actor.request(7, 0, 0).unwrap();
        let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
        saved["current"][1]["command"] = 3.into();
        saved["current"][1]["loops"] = loops.into();
        saved["reverse"] = true.into();
        let mut actor =
            ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
                .unwrap();
        assert_eq!(
            actor.advance(0).unwrap(),
            if delay == 0 { Some(70 + cursor) } else { None },
            "opcode {opcode} loops {loops}"
        );
        let saved = serde_json::to_value(actor.snapshot()).unwrap();
        assert_eq!(saved["current"][1]["command"], cursor);
        assert_eq!(saved["current"][1]["loops"], remaining);
        assert_eq!(saved["current"][1]["delay"], delay);
        assert!(!actor.translation_suppressed());
        assert!(!actor.exit_reached());
    }
}

#[test]
fn reverse_sound_dispatches_and_actor_events_remain_explicit() {
    let mut data = data();
    data.metadata["actors"][0]["animation_groups"][1]["commands"][3] =
        json!({"opcode":4,"sound_param":1,"volume_shift":2});
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["current"][1]["command"] = 3.into();
    saved["reverse"] = true.into();
    actor = ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
        .unwrap();
    let mut sounds = vec![];
    assert_eq!(
        actor
            .advance_with_sounds(0, &mut |s| {
                sounds.push(s);
                Ok(())
            })
            .unwrap(),
        Some(72)
    );
    assert_eq!(
        sounds,
        [AnimationSound::Play {
            effect: 1,
            rate_offset_hz: 128
        }]
    );
    data.metadata["actors"][0]["animation_groups"][1]["commands"][3] =
        json!({"opcode":11,"value":1});
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["current"][1]["command"] = 3.into();
    saved["reverse"] = true.into();
    actor = ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
        .unwrap();
    assert!(
        format!("{:#}", actor.advance(0).unwrap_err()).contains("opcode 11 playback unsupported")
    );
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["current"][1]["command"] = 3.into();
    saved["reverse"] = true.into();
    let mut actor =
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
            .unwrap();
    let mut events = vec![];
    assert_eq!(
        actor
            .advance_with_sounds(0, &mut |event| {
                events.push(event);
                Ok(())
            })
            .unwrap(),
        Some(72)
    );
    assert_eq!(events, [AnimationSound::Event { flags: 1 }]);
}

#[test]
fn forward_actor_events_dispatch_in_command_order_before_the_next_frame() {
    let mut data = data();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = json!([
        {"opcode":11,"value":17},
        {"opcode":4,"sound_param":1,"volume_shift":2},
        {"opcode":0,"frame_reference":70},
        {"opcode":11,"value":8},
        {"opcode":0,"frame_reference":71},
        {"opcode":1}
    ]);
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut seen = vec![];
    let frame = actor
        .request_with_sounds(7, 0, 0, &mut |event| {
            seen.push(event);
            Ok(())
        })
        .unwrap();
    assert_eq!(frame, Some(70));
    assert_eq!(
        seen,
        [
            AnimationSound::Event { flags: 17 },
            AnimationSound::Play {
                effect: 1,
                rate_offset_hz: 128
            }
        ]
    );
    let mut bare = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    assert!(
        format!("{:#}", bare.request(7, 0, 0).unwrap_err())
            .contains("opcode 11 playback unsupported")
    );
}

#[test]
fn reverse_frame_countdown_and_delay_match_original_scenarios() {
    for (cursor, countdown, ticks, delay, expected, frame, ended) in [
        (3, 2, 0, 0, (3, 2, 250, 0), Some(73), false),
        (3, 2, 1, 0, (3, 1, 250, 0), Some(73), false),
        (3, 0, 1, 0, (2, 2, 249, 0), Some(72), false),
        (3, 0, 2, 0, (1, 2, 248, 0), Some(71), false),
        (3, 0, 3, 0, (0, 2, 247, 0), Some(900), true),
        (1, 0, 1, 0, (0, 2, 249, 0), Some(900), true),
        (0, 2, 0, 0, (0, 2, 250, 0), Some(900), true),
        (3, 0, 1, 255, (3, 0, 250, 255), None, false),
        (3, 0, 1, 2, (3, 0, 250, 1), None, false),
        (3, 0, 2, 2, (1, 2, 248, 0), Some(71), false),
        (3, 0, 0, 2, (3, 0, 250, 2), None, false),
    ] {
        let mut data = data();
        data.metadata["actors"][0]["animation_groups"][1]["anim_id"] = 3.into();
        let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
        actor.request(7, 0, 0).unwrap();
        let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
        saved["current"][1]["command"] = cursor.into();
        saved["current"][1]["countdown"] = countdown.into();
        saved["current"][1]["delay"] = delay.into();
        saved["frame_progress"] = 250.into();
        saved["reverse"] = true.into();
        actor =
            ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
                .unwrap();
        assert_eq!(
            actor.advance(ticks).unwrap(),
            frame,
            "cursor {cursor} countdown {countdown} ticks {ticks} delay {delay}"
        );
        let saved = serde_json::to_value(actor.snapshot()).unwrap();
        if ended {
            assert_eq!(actor.current_group(), Some(0));
            assert!(!actor.is_reversed());
            assert_eq!(actor.frame_wait_state().unwrap().2, 0);
        } else {
            assert_eq!(actor.current_command_cursor().unwrap(), Some(expected.0));
            assert_eq!(saved["current"][1]["countdown"], expected.1);
            assert_eq!(actor.frame_wait_state().unwrap().2, expected.2);
            assert_eq!(saved["current"][1]["delay"], expected.3);
        }
    }
}

#[test]
fn direction_changes_do_not_advance_and_new_groups_reset_direction() {
    let data = data();
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.set_reverse(true).unwrap();
    let saved = actor.snapshot();
    actor = ActorAnimation::from_snapshot(&data, "ACTOR", saved).unwrap();
    assert!(actor.is_reversed());
    assert_eq!(actor.current_group(), None);
    actor.request(7, 0, 0).unwrap();
    assert!(!actor.is_reversed());
    let before = actor.frame_wait_state().unwrap();
    actor.set_reverse(true).unwrap();
    assert_eq!(actor.frame_wait_state().unwrap(), before);
    actor.force_request(7, 0, 0).unwrap();
    assert!(actor.is_reversed());
    assert_eq!(actor.force_request(0, 0, 0).unwrap(), Some(900));
    assert!(!actor.is_reversed());
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["version"] = 2.into();
    assert!(
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
            .is_err()
    );
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved.as_object_mut().unwrap().remove("reverse");
    assert!(serde_json::from_value::<ActorAnimationSnapshot>(saved).is_err());
}

#[test]
fn reverse_boundary_sound_failure_restores_its_signed_cursor_and_sticky_error() {
    let mut data = data();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = json!([
        {"opcode":0,"frame_reference":70}, {"opcode":4,"sound_param":1,"volume_shift":0},
        {"opcode":0,"frame_reference":72}, {"opcode":2,"value":0}
    ]);
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut accept = |_| Ok(());
    actor.request_with_sounds(7, 0, 0, &mut accept).unwrap();
    for _ in 0..3 {
        actor.advance_with_sounds(1, &mut accept).unwrap();
    }
    assert_eq!(actor.current_command_cursor().unwrap(), Some(0));
    actor.set_reverse(true).unwrap();
    let error = actor
        .advance_with_sounds(1, &mut |sound| {
            assert_eq!(sound, AnimationSound::Stop);
            anyhow::bail!("sound backend unavailable")
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("sound backend unavailable"));
    let saved = serde_json::to_value(actor.snapshot()).unwrap();
    assert_eq!(saved["current"][1]["command"], -1);
    let mut actor =
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
            .unwrap();
    assert!(actor.is_reversed());
    assert!(
        actor
            .advance(0)
            .unwrap_err()
            .to_string()
            .contains("sound backend unavailable")
    );
}

#[test]
fn a_mask_four_transition_follows_the_falling_byte_without_asking_the_host() {
    let mut data = data();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = json!([
        {"opcode":15,"trigger_mask":4,"start_frame":0,"target_group":3},
        {"opcode":0,"frame_reference":70},
        {"opcode":1},
        {"opcode":0,"frame_reference":80},
        {"opcode":1}
    ]);
    for (falling, frame) in [(false, 70), (true, 80)] {
        let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
        actor.set_motion_active(falling);
        let mut seen = vec![];
        let shown = actor
            .request_with_sounds(7, 0, 0, &mut |event| {
                seen.push(event);
                Ok(())
            })
            .unwrap();
        assert_eq!(shown, Some(frame), "falling {falling}");
        assert!(seen.is_empty(), "mask 4 is decided without the host");
    }
}

#[test]
fn forward_transitions_ask_the_host_and_reverse_steps_past_them() {
    let transition = json!({"opcode":15,"trigger_mask":2,"start_frame":0,"target_group":36});
    let mut data = data();
    data.metadata["actors"][0]["animation_groups"][1]["commands"] = json!([
        transition.clone(),
        {"opcode":0,"frame_reference":70},
        {"opcode":1}
    ]);
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut seen = vec![];
    let frame = actor
        .request_with_sounds(7, 0, 0, &mut |event| {
            seen.push(event);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        frame,
        Some(70),
        "the transition continues to the next command"
    );
    assert_eq!(seen, [AnimationSound::Transition { mask: 2, target: 36 }]);
    let mut refused = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let error = refused
        .request_with_sounds(7, 0, 0, &mut |_| anyhow::bail!("combat state unmodelled"))
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("combat state unmodelled"),
        "{error:#}"
    );
    let mut bare = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    assert!(
        format!("{:#}", bare.request(7, 0, 0).unwrap_err())
            .contains("opcode 15 playback unsupported")
    );
    // Reverse playback steps back over the command without asking the host.
    let mut data = self::data();
    data.metadata["actors"][0]["animation_groups"][1]["commands"][3] = transition;
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.request(7, 0, 0).unwrap();
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["current"][1]["command"] = 3.into();
    saved["reverse"] = true.into();
    let mut actor =
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_value(saved).unwrap())
            .unwrap();
    assert_eq!(actor.advance(0).unwrap(), Some(72));
}

#[test]
fn a_skipping_request_starts_the_group_at_once_two_frames_in() {
    let data = data();
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    assert_eq!(actor.request_skipping_frames(7, 0, 2).unwrap(), Some(72));
    assert_eq!(actor.current_group(), Some(7));
    assert_eq!(actor.frame_wait_state().unwrap().2, 2);
    assert_eq!(actor.current_command_cursor().unwrap(), Some(2));
    // Already playing with enough frames shown: only a pending request would clear.
    assert_eq!(actor.request_skipping_frames(7, 0, 2).unwrap(), None);
    assert_eq!(actor.frame_wait_state().unwrap().2, 2);
}
