//! Original mutable playback replaces a different initialized native group without running cues.
#![cfg(feature = "original-save-import")]
use cyrus_assets::animation::Animations;
use cyrus_soup::AnimationGroup;
use cyrus_world::animation::{ActorAnimation, OriginalPlaybackRestore, PlaybackField};
use rgpre::import::savegame::SaveAnimationPlayback;
use serde_json::json;

#[test]
fn original_header_restores_group_cursor_countdown_and_history_without_cues() {
    let data = Animations {
        version: 2,
        world: 17,
        metadata: json!({"actors":[{"script_name":"ACTOR","animation_groups":[
            {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":152,"anim_id":3,"anim_type":0,"commands":[{"opcode":4,"sound_param":4,"volume_shift":0},{"opcode":0,"frame_reference":66},{"opcode":1}]}
        ]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut actor = ActorAnimation::from_asset(&data, "ACTOR", AnimationGroup::PANIC).unwrap();
    let mut header = [0u8; 37];
    header[..2].copy_from_slice(&152i16.to_le_bytes());
    header[2..4].copy_from_slice(&(-1i16).to_le_bytes());
    header[5..7].copy_from_slice(&1i16.to_le_bytes());
    header[7] = 3;
    header[8] = 2;
    header[17] = 9;
    header[18] = 153;
    header[19..21].copy_from_slice(&(-7i16).to_le_bytes());
    header[31..33].copy_from_slice(&(-2i16).to_le_bytes());
    let saved = SaveAnimationPlayback::from(&header);
    assert_eq!(
        actor.restore_original(&saved).unwrap(),
        OriginalPlaybackRestore::Restored { frame: Some(66) }
    );
    assert_eq!(actor.current_group(), Some(AnimationGroup::from_index(152)));
    for (field, value) in [
        (PlaybackField::Frame, 1),
        (PlaybackField::SpeedCount, 2),
        (PlaybackField::RealFrame, 9),
        (PlaybackField::Position(0), -7),
        (PlaybackField::LastRotation(0), -2),
    ] {
        assert_eq!(actor.record_field(field).unwrap(), value);
    }
    let before = serde_json::to_value(actor.snapshot()).unwrap();
    let mut unsupported = saved.clone();
    unsupported.frame = 0;
    assert_eq!(
        actor.restore_original(&unsupported).unwrap(),
        OriginalPlaybackRestore::UnsupportedPose
    );
    assert_eq!(serde_json::to_value(actor.snapshot()).unwrap(), before);
    let mut cues = Vec::new();
    actor
        .advance_with_sounds(1, &mut |cue| {
            cues.push(cue);
            Ok(())
        })
        .unwrap();
    assert!(
        cues.is_empty(),
        "import must not replay the earlier sound command"
    );
    assert_eq!(actor.record_field(PlaybackField::SpeedCount).unwrap(), 1);
}
