//! Complete animation saves resume through public playback APIs without replaying cues.
use cyrus_assets::animation::Animations;
use cyrus_world::animation::{ActorAnimation, ActorAnimationSnapshot, AnimationSound};
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

fn sound_asset() -> Animations {
    asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":900},{"opcode":2,"value":0}]},
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":4,"sound_param":4,"volume_shift":-1},
            {"opcode":0,"frame_reference":70},
            {"opcode":4,"sound_param":8,"volume_shift":2},
            {"opcode":0,"frame_reference":71},{"opcode":1}]},
        {"group_index":8,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":800},{"opcode":1}]}
    ]))
}

fn resume(data: &Animations, actor: &ActorAnimation) -> ActorAnimation {
    let bytes = serde_json::to_vec(&actor.snapshot()).unwrap();
    let saved: ActorAnimationSnapshot = serde_json::from_slice(&bytes).unwrap();
    ActorAnimation::from_snapshot(data, "ACTOR", saved).unwrap()
}

fn pending_sound(data: &Animations) -> ActorAnimation {
    let mut actor = ActorAnimation::from_asset(data, "ACTOR", 0).unwrap();
    let mut sounds = Vec::new();
    assert_eq!(
        actor
            .request_with_sounds(7, 0, 0, &mut |sound| {
                sounds.push(sound);
                Ok(())
            })
            .unwrap(),
        Some(70)
    );
    actor.request(8, 0, 0).unwrap();
    assert_eq!(
        sounds,
        [AnimationSound::Play {
            effect: 4,
            rate_offset_hz: -64
        }]
    );
    actor
}

#[test]
fn pending_default_and_sound_state_resume_without_replaying_play() {
    let data = sound_asset();
    let mut original = pending_sound(&data);
    let mut restored = resume(&data, &original);
    assert_eq!(restored.current_group(), Some(7));
    assert_eq!(restored.pending_group(), Some(8));
    assert_eq!(restored.default_group(), 0);
    let mut original_sounds = Vec::new();
    let mut restored_sounds = Vec::new();
    let mut frames = Vec::new();
    for _ in 0..4 {
        let expected = original
            .advance_with_sounds(1, &mut |sound| {
                original_sounds.push(sound);
                Ok(())
            })
            .unwrap();
        let actual = restored
            .advance_with_sounds(1, &mut |sound| {
                restored_sounds.push(sound);
                Ok(())
            })
            .unwrap();
        assert_eq!(actual, expected);
        frames.push(actual);
    }
    assert_eq!(frames, [Some(70), Some(71), Some(800), Some(900)]);
    assert_eq!(
        restored_sounds,
        [
            AnimationSound::Play {
                effect: 8,
                rate_offset_hz: 128
            },
            AnimationSound::Stop,
        ]
    );
    assert_eq!(restored_sounds, original_sounds);
    assert_eq!(restored.current_group(), Some(0));
    assert_eq!(restored.pending_group(), None);
}

#[test]
fn replacing_a_restored_sound_receiver_forgets_without_replaying_or_stopping() {
    let data = sound_asset();
    let actor = pending_sound(&data);
    let mut restored = resume(&data, &actor);
    let mut sounds = Vec::new();
    assert_eq!(
        restored
            .force_request_with_sounds(8, 0, 0, &mut |sound| {
                sounds.push(sound);
                Ok(())
            })
            .unwrap(),
        Some(800)
    );
    assert_eq!(sounds, [AnimationSound::Forget]);
}

#[test]
fn finite_loops_and_wrapped_effective_speed_countdown_are_retained() {
    let data = asset(json!([
        {"group_index":7,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":70},{"opcode":2,"value":0},{"opcode":1}]}
    ]));
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 99).unwrap();
    actor.request(7, 0, 0).unwrap();
    actor.set_loop_count(2).unwrap();
    let mut restored = resume(&data, &actor);
    let frames: Vec<_> = (0..3).map(|_| restored.advance(1).unwrap()).collect();
    assert_eq!(frames, [Some(70), Some(70), None]);
    assert_eq!(restored.default_group(), 99);
    assert_eq!(restored.current_group(), None);

    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 99).unwrap();
    actor.request(7, 255, 0).unwrap();
    actor.set_loop_count(2).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(70));
    let mut restored = resume(&data, &actor);
    let saved = serde_json::to_value(restored.snapshot()).unwrap();
    assert_eq!(saved["current"][1]["speed"], 0);
    assert_eq!(saved["current"][1]["countdown"], 255);
    assert_eq!(saved["current"][1]["loops"], 1);
    assert_eq!(restored.advance(1).unwrap(), Some(70));
    assert_eq!(
        serde_json::to_value(restored.snapshot()).unwrap()["current"][1]["countdown"],
        254
    );
}

#[test]
fn delay_and_translation_latch_resume_at_the_same_frame_without_synthetic_ticks() {
    let data = asset(json!([
        {"group_index":7,"anim_id":2,"anim_type":0,"commands":[
            {"opcode":5},{"opcode":12,"value":3},
            {"opcode":0,"frame_reference":70},{"opcode":2,"value":2}]}
    ]));
    let mut original = ActorAnimation::from_asset(&data, "ACTOR", 99).unwrap();
    assert_eq!(original.request(7, 0, 0).unwrap(), None);
    let mut restored = resume(&data, &original);
    assert!(restored.translation_suppressed());
    let mut frames = Vec::new();
    let mut translation = Vec::new();
    for _ in 0..5 {
        let frame = restored.advance(1).unwrap();
        assert_eq!(frame, original.advance(1).unwrap());
        frames.push(frame);
        translation.push(restored.translation_suppressed());
    }
    assert_eq!(frames, [None, None, Some(70), Some(70), Some(70)]);
    assert_eq!(translation, [true, true, true, true, false]);
}

#[test]
fn type_one_exit_tail_and_pending_default_remain_distinct() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":900},{"opcode":2,"value":0}]},
        {"group_index":7,"anim_id":1,"anim_type":1,"commands":[
            {"opcode":0,"frame_reference":70},{"opcode":3,"value":3},
            {"opcode":2,"value":0},{"opcode":0,"frame_reference":71},
            {"opcode":12,"value":3},{"opcode":0,"frame_reference":72},{"opcode":1}]}
    ]));
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    actor.request(7, 0, 0).unwrap();
    actor.request(0, 0, 0).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(70));
    assert_eq!(actor.advance(1).unwrap(), Some(71));
    let mut restored = resume(&data, &actor);
    assert!(restored.exit_reached());
    assert_eq!(restored.current_group(), Some(7));
    assert_eq!(restored.pending_group(), Some(0));
    let frames: Vec<_> = (0..4).map(|_| restored.advance(1).unwrap()).collect();
    assert_eq!(frames, [None, None, None, Some(900)]);
}

#[test]
fn final_delay_and_out_of_range_sticky_failure_are_legitimate_snapshots() {
    for delay in [3, 255] {
        let data = asset(json!([
            {"group_index":7,"anim_id":0,"anim_type":0,"commands":[
                {"opcode":12,"value":delay}]}
        ]));
        let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 99).unwrap();
        assert_eq!(actor.request(7, 0, 0).unwrap(), None);
        let mut restored = resume(&data, &actor);
        if delay == 255 {
            assert_eq!(restored.advance(u32::MAX).unwrap(), None);
            assert_eq!(restored.current_group(), Some(7));
        } else {
            assert!(restored.advance(3).is_err());
            let failure = restored.failure().unwrap().to_string();
            assert!(failure.contains("command 2: outside group"));
            let mut failed = resume(&data, &restored);
            assert_eq!(failed.failure(), Some(failure.as_str()));
            assert!(failed.advance(0).is_err());
        }
    }
}

#[test]
fn sound_failure_is_sticky_across_restore_and_does_not_retry_dispatch() {
    let data = sound_asset();
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    assert!(
        actor
            .request_with_sounds(7, 0, 0, &mut |_| anyhow::bail!("channel unavailable"))
            .is_err()
    );
    let mut restored = resume(&data, &actor);
    assert_eq!(restored.failure(), actor.failure());
    let mut calls = 0;
    assert!(
        restored
            .advance_with_sounds(1, &mut |_| {
                calls += 1;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(calls, 0);
}

#[test]
fn malformed_state_and_changed_sources_reject_without_changing_live_playback() {
    let data = sound_asset();
    let actor = pending_sound(&data);
    let snapshot = serde_json::to_value(actor.snapshot()).unwrap();
    for (pointer, value) in [
        ("/version", json!(u32::MAX)),
        ("/context", json!("another world")),
        ("/current/0", json!(99)),
        ("/pending/0", json!(99)),
        ("/current/1/command", json!(999)),
        ("/current/1/ended", json!(true)),
        ("/groups/7/commands/1/frame_reference", json!(999)),
    ] {
        let mut document = snapshot.clone();
        *document.pointer_mut(pointer).unwrap() = value;
        let error = ActorAnimation::from_snapshot(
            &data,
            "ACTOR",
            serde_json::from_value(document).unwrap(),
        )
        .err()
        .unwrap();
        assert!(format!("{error:#}").contains("world 17 actor ACTOR animation restore"));
    }
    let mut changed = sound_asset();
    changed.metadata["actors"][0]["animation_groups"][1]["commands"][1]["frame_reference"] =
        json!(999);
    assert!(ActorAnimation::from_snapshot(&changed, "ACTOR", actor.snapshot()).is_err());
    let mut changed = sound_asset();
    changed.world = 18;
    assert!(ActorAnimation::from_snapshot(&changed, "ACTOR", actor.snapshot()).is_err());
    assert_eq!(serde_json::to_value(actor.snapshot()).unwrap(), snapshot);
}

#[test]
fn inactive_snapshot_does_not_initialize_or_select_a_default() {
    let data = sound_asset();
    let actor = ActorAnimation::from_asset(&data, "ACTOR", 7).unwrap();
    let restored = resume(&data, &actor);
    assert_eq!(restored.current_group(), None);
    assert_eq!(restored.pending_group(), None);
    assert_eq!(restored.default_group(), 7);
    assert_eq!(
        serde_json::to_value(restored.snapshot()).unwrap(),
        serde_json::to_value(actor.snapshot()).unwrap()
    );
}

#[test]
fn frame_progress_survives_loops_snapshot_and_collision_rollback() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":900},{"opcode":2,"value":0}]}
    ]));
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    assert_eq!(actor.advance(0).unwrap(), Some(900));
    assert_eq!(actor.frame_wait_state().unwrap(), (0, -1, 0));
    assert_eq!(actor.advance(1).unwrap(), Some(900));
    assert_eq!(actor.frame_wait_state().unwrap(), (0, -1, 0));
    for progress in 1..=255 {
        assert_eq!(actor.advance(1).unwrap(), Some(900));
        assert_eq!(actor.frame_wait_state().unwrap(), (0, -1, progress));
    }
    let checkpoint = actor.checkpoint().unwrap();
    let mut restored = resume(&data, &actor);
    for playback in [&mut actor, &mut restored] {
        assert_eq!(playback.advance(1).unwrap(), Some(900));
        assert_eq!(playback.frame_wait_state().unwrap(), (0, -1, 0));
        playback.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(playback.frame_wait_state().unwrap(), (0, -1, 255));
    }
}

#[test]
fn old_animation_snapshot_cannot_invent_frame_progress() {
    let data = sound_asset();
    let actor = pending_sound(&data);
    let mut saved = serde_json::to_value(actor.snapshot()).unwrap();
    saved["version"] = json!(1);
    assert!(
        ActorAnimation::from_snapshot(
            &data,
            "ACTOR",
            serde_json::from_value(saved.clone()).unwrap()
        )
        .is_err()
    );
    saved.as_object_mut().unwrap().remove("frame_progress");
    assert!(serde_json::from_value::<ActorAnimationSnapshot>(saved).is_err());
}

#[test]
fn combat_command_cursor_tracks_commands_through_delay_loop_and_restore() {
    let data = asset(json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":900}, {"opcode":5},
            {"opcode":12,"value":2}, {"opcode":0,"frame_reference":901},
            {"opcode":2,"value":0}]},
        {"group_index":7,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":5},{"opcode":0,"frame_reference":700},{"opcode":1}]}
    ]));
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    assert_eq!(actor.current_command_cursor().unwrap(), None);
    for (ticks, cursor, progress) in [
        (0, 0, 0),
        (1, 0, 0),
        (0, 0, 0),
        (1, 3, 1),
        (1, 3, 1),
        (1, 0, 2),
        (1, 3, 3),
        (1, 3, 3),
        (1, 0, 4),
    ] {
        actor.advance(ticks).unwrap();
        assert_eq!(actor.current_command_cursor().unwrap(), Some(cursor));
        assert_eq!(actor.frame_wait_state().unwrap(), (0, -1, progress));
        let checkpoint = actor.checkpoint().unwrap();
        let mut restored = resume(&data, &actor);
        assert_eq!(restored.current_command_cursor().unwrap(), Some(cursor));
        restored.advance(3).unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.current_command_cursor().unwrap(), Some(cursor));
        assert_eq!(actor.current_command_cursor().unwrap(), Some(cursor));
    }
    assert_eq!(actor.force_request(7, 0, 0).unwrap(), Some(700));
    assert_eq!(actor.current_command_cursor().unwrap(), Some(1));
    assert_eq!(actor.frame_wait_state().unwrap(), (7, -1, 0));
}
