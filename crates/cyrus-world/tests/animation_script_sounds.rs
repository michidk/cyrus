//! Script-request cues share the ordinary actor's sound handler and tick input.
use cyrus_assets::animation::Animations;
use cyrus_world::animation::AnimationSound;
use serde_json::json;

fn asset() -> Animations {
    Animations {
        version: 2,
        world: 17,
        metadata: json!({
            "actors":[{"script_name":"ACTOR","animation_groups":[
                {"group_index":0,"anim_type":0,"anim_id":1,"commands":[
                    {"opcode":0,"frame_reference":9},{"opcode":2,"value":0}]},
                {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
                    {"opcode":4,"sound_param":-17,"volume_shift":-1},
                    {"opcode":0,"frame_reference":1},{"opcode":0,"frame_reference":2},
                    {"opcode":1}]}
            ]}],
            "animation_scripts":{"ACTOR":{"version":1,"entry":0,"steps":{
                "0":{"next":14,"operation":{"type":"task","function":"PushAnimation","arguments":[7,1]}},
                "14":{"next":19,"operation":{"type":"end","target":0}}
            }}}
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    }
}

#[test]
fn script_requests_dispatch_authored_cues_before_the_immediate_pose() {
    let data = asset();
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut script = cyrus_world::animation::AnimationScript::from_asset(&data, "ACTOR").unwrap();
    actor.request(0, 0, 1).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 0, &mut sounds)
            .unwrap(),
        None
    );
    assert_eq!(actor.advance_with_sounds(0, &mut sounds).unwrap(), Some(1));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(1));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(2));
    assert_eq!(actor.advance_with_sounds(1, &mut sounds).unwrap(), Some(9));
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 1, &mut sounds)
            .unwrap(),
        None
    );
    assert_eq!(script.program_counter(), 0);
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
            AnimationSound::Stop,
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
        ]
    );
}

#[test]
fn script_sound_failures_keep_context_and_do_not_retry_the_cue() {
    let data = asset();
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut script = cyrus_world::animation::AnimationScript::from_asset(&data, "ACTOR").unwrap();
    let mut calls = 0;
    let mut sounds = |_| {
        calls += 1;
        anyhow::bail!("actor surface unavailable")
    };
    let error = script
        .advance_with_sounds(&mut actor, 0, &mut sounds)
        .unwrap_err()
        .to_string();
    assert!(error.contains("world 17 actor ACTOR script instruction 0"));
    assert!(error.contains("actor surface unavailable"));
    assert_eq!(
        script
            .advance_with_sounds(&mut actor, 1, &mut sounds)
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(calls, 1);
}

#[test]
fn forced_replacement_forgets_the_prior_sound_before_its_immediate_cue() {
    let data = asset();
    let mut actor = cyrus_world::animation::ActorAnimation::from_asset(&data, "ACTOR", 0).unwrap();
    let mut events = Vec::new();
    let mut sounds = |event| {
        events.push(event);
        Ok(())
    };
    assert_eq!(
        actor
            .force_request_with_sounds(7, 0, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        actor
            .force_request_with_sounds(7, 0, 0, &mut sounds)
            .unwrap(),
        None
    );
    assert_eq!(
        actor
            .force_request_with_sounds(0, 0, 0, &mut sounds)
            .unwrap(),
        Some(9)
    );
    assert_eq!(
        actor
            .force_request_with_sounds(7, 0, 0, &mut sounds)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        events,
        [
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
            AnimationSound::Forget,
            AnimationSound::Play {
                effect: -17,
                rate_offset_hz: -64
            },
        ]
    );
}
