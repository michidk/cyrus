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
