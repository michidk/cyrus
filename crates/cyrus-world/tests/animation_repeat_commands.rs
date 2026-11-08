//! Forward ConditionalDelay/LoopControl and the translation/rotation commands, expected from the
//! recovered command rules (shipped data never uses them, so groups are constructed).
use cyrus_assets::animation::Animations;
use cyrus_soup::AnimationGroup;
use cyrus_world::animation::{ActorAnimation, PlaybackField::*};
use serde_json::{Value, json};

fn asset(commands: Value) -> Animations {
    serde_json::from_value(json!({
        "version": 2, "world": 17, "sections": {},
        "metadata": {"actors": [{"script_name": "ACTOR", "animation_groups": [
            {"group_index": 7, "anim_type": 0, "anim_id": 1, "commands": commands}]}]},
        "models": {}, "unsupported": {}
    }))
    .unwrap()
}

fn started(data: &Animations) -> (ActorAnimation, Option<u32>) {
    let mut actor =
        ActorAnimation::from_asset(data, "ACTOR", AnimationGroup::from_index(7)).unwrap();
    let frame = actor.request(AnimationGroup::from_index(7), 0, 1).unwrap();
    (actor, frame)
}

#[test]
fn a_repeat_byte_makes_loop_control_repeat_its_block_that_many_times() {
    // Repeat byte 2: the loop command jumps back twice, so the frame after the setter shows three
    // times before the loop falls through to the next frame.
    let data = asset(json!([
        {"opcode":0,"frame_reference":0}, {"opcode":13,"value":2},
        {"opcode":0,"frame_reference":1}, {"opcode":14,"value":2},
        {"opcode":0,"frame_reference":2}, {"opcode":1}
    ]));
    let (mut actor, first) = started(&data);
    let mut shown = vec![first];
    for _ in 0..4 {
        shown.push(actor.advance(1).unwrap());
    }
    assert_eq!(shown, [Some(0), Some(1), Some(1), Some(1), Some(2)]);
    assert_eq!(actor.record_field(RepCount).unwrap(), 0);
    // EndAnimation ends the group; ordinary advancement then requests the default group, which
    // here is the same group, so it starts over.
    assert_eq!(actor.advance(1).unwrap(), Some(0));
}

#[test]
fn the_repeat_byte_is_a_property_the_loop_reads() {
    let data = asset(json!([
        {"opcode":0,"frame_reference":0}, {"opcode":14,"value":0},
        {"opcode":0,"frame_reference":1}, {"opcode":1}
    ]));
    let (mut actor, _) = started(&data);
    // A script writing the repeat byte before the loop command makes it jump once.
    actor.set_record_field(RepCount, 1).unwrap();
    assert_eq!(actor.advance(1).unwrap(), Some(0));
    assert_eq!(actor.record_field(RepCount).unwrap(), 0);
    assert_eq!(actor.advance(1).unwrap(), Some(1));
}

#[test]
fn translation_and_rotation_commands_fill_the_retained_words_until_the_next_cursor_step() {
    let data = asset(json!([
        {"opcode":6,"x":1,"y":-2,"z":3}, {"opcode":8,"x":1,"y":2,"z":3},
        {"opcode":9,"axis":2,"value":-5}, {"opcode":7,"axis":1,"value":300},
        {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1}, {"opcode":1}
    ]));
    let (mut actor, frame) = started(&data);
    assert_eq!(frame, Some(0));
    // Triple translation words take the operands shifted by eight, triple rotation words by three;
    // the axis forms store their value unshifted.
    let words = |actor: &ActorAnimation| {
        [
            Position(0),
            Position(1),
            Position(2),
            Rotation(0),
            Rotation(1),
            Rotation(2),
        ]
        .map(|field| actor.record_field(field).unwrap())
    };
    assert_eq!(words(&actor), [256, 300, 768, 8, 16, -5]);
    // Stepping to the next frame clears them.
    assert_eq!(actor.advance(1).unwrap(), Some(1));
    assert_eq!(words(&actor), [0; 6]);
    // They are plain words a script can write, and are saved with the group.
    actor.set_record_field(Position(2), -300).unwrap();
    let saved = serde_json::to_vec(&actor.snapshot()).unwrap();
    let restored =
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_slice(&saved).unwrap())
            .unwrap();
    assert_eq!(restored.record_field(Position(2)).unwrap(), -300);
}

#[test]
fn reverse_playback_steps_over_the_new_commands_without_effect() {
    let data = asset(json!([
        {"opcode":0,"frame_reference":0}, {"opcode":13,"value":9}, {"opcode":6,"x":1,"y":1,"z":1},
        {"opcode":0,"frame_reference":1}, {"opcode":0,"frame_reference":2}, {"opcode":1}
    ]));
    let (mut actor, _) = started(&data);
    actor.advance(1).unwrap();
    actor.advance(1).unwrap();
    assert_eq!(actor.record_field(RepCount).unwrap(), 9);
    actor.set_record_field(RepCount, 0).unwrap();
    actor.set_reverse(true).unwrap();
    actor.set_record_field(Frame, 3).unwrap();
    actor.set_record_field(SpeedCount, 0).unwrap();
    // Cursor 3 steps back to the translation command, over it and the setter to the first frame:
    // neither changes a byte, and the displayed frame is the one before them.
    assert_eq!(actor.advance(1).unwrap(), Some(0));
    assert_eq!(actor.record_field(RepCount).unwrap(), 0);
}

#[test]
fn change_group_switches_the_commands_in_use_and_keeps_the_other_state() {
    let data: Animations = serde_json::from_value(json!({
        "version": 2, "world": 17, "sections": {},
        "metadata": {"actors": [{"script_name": "ACTOR", "animation_groups": [
            {"group_index": 7, "anim_type": 0, "anim_id": 1, "commands": [
                {"opcode":0,"frame_reference":0},
                {"opcode":10,"target_group":9,"target_frame":1},
                {"opcode":0,"frame_reference":99}, {"opcode":1}]},
            {"group_index": 9, "anim_type": 0, "anim_id": 5, "commands": [
                {"opcode":0,"frame_reference":10}, {"opcode":0,"frame_reference":11},
                {"opcode":0,"frame_reference":12}, {"opcode":1}]},
        ]}]},
        "models": {}, "unsupported": {}
    }))
    .unwrap();
    let (mut actor, first) = started(&data);
    assert_eq!(first, Some(0));
    assert_eq!(actor.current_group(), Some(AnimationGroup::from_index(7)));
    // The next step reaches the group change in the same call: group 9's frame at index 1 is
    // shown, the current group is now 9, and the countdown/speed bytes stay group 7's.
    assert_eq!(actor.advance(1).unwrap(), Some(11));
    assert_eq!(actor.current_group(), Some(AnimationGroup::from_index(9)));
    assert_eq!(actor.record_field(Frame).unwrap(), 1);
    assert_eq!(actor.record_field(Speed).unwrap(), 1);
    // A save taken after the switch resumes in the new group.
    let saved = serde_json::to_vec(&actor.snapshot()).unwrap();
    let mut restored =
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_slice(&saved).unwrap())
            .unwrap();
    assert_eq!(
        restored.current_group(),
        Some(AnimationGroup::from_index(9))
    );
    assert_eq!(restored.advance(1).unwrap(), Some(12));
    // Playback continues through the new group's commands and ends with it.
    assert_eq!(actor.advance(1).unwrap(), Some(12));
}

#[test]
fn change_group_to_a_missing_group_or_frame_fails_playback_stickily() {
    for (group, frame) in [(40, 0), (9, 7)] {
        let data: Animations = serde_json::from_value(json!({
            "version": 2, "world": 17, "sections": {},
            "metadata": {"actors": [{"script_name": "ACTOR", "animation_groups": [
                {"group_index": 7, "anim_type": 0, "anim_id": 1, "commands": [
                    {"opcode":0,"frame_reference":0},
                    {"opcode":10,"target_group":group,"target_frame":frame},
                    {"opcode":0,"frame_reference":99}]},
                {"group_index": 9, "anim_type": 0, "anim_id": 1, "commands": [
                    {"opcode":0,"frame_reference":10}]},
            ]}]},
            "models": {}, "unsupported": {}
        }))
        .unwrap();
        let (mut actor, _) = started(&data);
        let error = actor.advance(1).unwrap_err();
        assert!(format!("{error:#}").contains("group change"), "{error:#}");
        assert!(actor.failure().is_some());
        assert!(actor.advance(1).is_err());
    }
}

#[test]
fn writing_the_current_group_switches_the_commands_in_use_without_touching_the_cursor() {
    let data: Animations = serde_json::from_value(json!({
        "version": 2, "world": 17, "sections": {},
        "metadata": {"actors": [{"script_name": "ACTOR", "animation_groups": [
            {"group_index": 7, "anim_type": 0, "anim_id": 1, "commands": [
                {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1},
                {"opcode":0,"frame_reference":2}, {"opcode":1}]},
            {"group_index": 9, "anim_type": 0, "anim_id": 5, "commands": [
                {"opcode":0,"frame_reference":10}, {"opcode":0,"frame_reference":11},
                {"opcode":0,"frame_reference":12}, {"opcode":1}]},
        ]}]},
        "models": {}, "unsupported": {}
    }))
    .unwrap();
    let (mut actor, first) = started(&data);
    assert_eq!(first, Some(0));
    actor.set_current_group(9).unwrap();
    // The word changed, the cursor and the reload byte did not.
    assert_eq!(actor.current_group(), Some(AnimationGroup::from_index(9)));
    assert_eq!(actor.record_field(Frame).unwrap(), 0);
    assert_eq!(actor.record_field(Speed).unwrap(), 1);
    // The next step reads group 9's table at the kept cursor.
    assert_eq!(actor.advance(1).unwrap(), Some(11));
    assert_eq!(actor.record_field(Frame).unwrap(), 1);
    // A group without a table stops with an explicit error and changes nothing.
    let error = actor.set_current_group(40).unwrap_err();
    assert!(format!("{error:#}").contains("absent group"), "{error:#}");
    assert_eq!(actor.current_group(), Some(AnimationGroup::from_index(9)));
    // -1 leaves no current group.
    actor.set_current_group(-1).unwrap();
    assert_eq!(actor.current_group(), None);
    let error = actor.set_current_group(7).unwrap_err();
    assert!(
        format!("{error:#}").contains("while none is current"),
        "{error:#}"
    );
}

#[test]
fn the_rotation_history_is_the_words_the_last_model_refresh_copied() {
    let data = asset(json!([
        {"opcode":8,"x":1,"y":2,"z":3},
        {"opcode":0,"frame_reference":0}, {"opcode":0,"frame_reference":1}, {"opcode":1}
    ]));
    let (mut actor, _) = started(&data);
    let history = |actor: &ActorAnimation| {
        [LastRotation(0), LastRotation(1), LastRotation(2)]
            .map(|field| actor.record_field(field).unwrap())
    };
    // Nothing is copied until a refresh runs.
    assert_eq!(history(&actor), [0; 3]);
    actor.refresh_rotation_history();
    assert_eq!(history(&actor), [8, 16, 24]);
    // The next cursor step clears the words but not the history, until the next refresh.
    actor.advance(1).unwrap();
    assert_eq!(actor.record_field(Rotation(0)).unwrap(), 0);
    assert_eq!(history(&actor), [8, 16, 24]);
    actor.refresh_rotation_history();
    assert_eq!(history(&actor), [0; 3]);
    // The history is a plain word a script can write, and is saved with the receiver.
    actor.set_record_field(LastRotation(1), -9).unwrap();
    let saved = serde_json::to_vec(&actor.snapshot()).unwrap();
    let restored =
        ActorAnimation::from_snapshot(&data, "ACTOR", serde_json::from_slice(&saved).unwrap())
            .unwrap();
    assert_eq!(restored.record_field(LastRotation(1)).unwrap(), -9);
}
