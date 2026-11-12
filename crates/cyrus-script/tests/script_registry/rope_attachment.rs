//! Early rope grabs retain their approach height until the authored grab has progressed.
use super::*;

#[test]
fn a_positive_rope_grab_retains_height_before_three_animation_progress_values() {
    let (_, mut world, data, scene) = rope_classification_fixture_with(
        |_| {},
        |data| {
            let actor = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap();
            let jump = actor["animation_groups"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|group| group["group_index"] == AnimationGroup::JUMP_START.index())
                .unwrap();
            *jump["commands"].as_array_mut().unwrap().last_mut().unwrap() =
                serde_json::json!({"opcode":1});
        },
    );
    let player = world.selected_player().unwrap();
    for _ in 0..4 {
        world
            .advance(
                player,
                ScriptStage::Animation,
                cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(256),
                    frame_scale: Some(0),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
    }
    let before = world.inspect(player.index, 1)["actors"][0]["spatial"]["local"]["position"][1]
        .as_i64()
        .unwrap();
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        Default::default(),
        false,
        [0, 0, 0],
    );
    assert!(
        world.actors()[player.index].failure().is_none(),
        "{:?}",
        world.actors()[player.index].failure()
    );
    assert!(world.selected_player_on_rope());
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        Default::default(),
        false,
        [0, 0, 0],
    );
    let attached = world.inspect(player.index, 1);
    assert_eq!(
        attached["actors"][0]["animation"]["current_group"],
        AnimationGroup::ROPE_GRAB.index()
    );
    assert_eq!(
        attached["actors"][0]["spatial"]["local"]["position"][1],
        before
    );
}

#[test]
fn admitting_a_rope_attachment_does_not_place_a_pending_grab() {
    let (_, mut world, data, scene) = rope_classification_fixture();
    let player = world.selected_player().unwrap();
    let before =
        world.inspect(player.index, 1)["actors"][0]["spatial"]["local"]["position"].clone();
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        Default::default(),
        false,
        [0, 0, 0],
    );
    assert!(world.selected_player_on_rope());
    let attached = world.inspect(player.index, 1);
    assert_eq!(
        attached["actors"][0]["animation"]["pending_group"],
        AnimationGroup::ROPE_GRAB.index()
    );
    assert_eq!(
        attached["actors"][0]["spatial"]["local"]["position"],
        before
    );
}

#[test]
fn rope_admission_selects_the_link_at_the_actors_hand_without_a_link_length_bias() {
    let (_, mut world, data, scene) = rope_classification_fixture_with(
        |_| {},
        |data| {
            let actor = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap();
            actor["raex"]["grip0"] = 0.into();
            actor["animation_groups"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "group_index":AnimationGroup::ROPE_PANIC.index(), "anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
                }));
            data.metadata["mpsz_entries"][0]["pos_extent"][1] = 0.into();
            data.metadata["mpsz_entries"][0]["neg_extent"][1] = 48.into();
            data.metadata["mpsz_entries"][0]["center_offset"][1] = (-24).into();
            let model = data.models.get_mut("PLAYER").unwrap();
            model.base_positions = vec![[0.0, 0.0, 0.0]];
            model.positions = vec![vec![[0.0, 0.0, 0.0]]];
        },
    );
    let player = world.selected_player().unwrap();
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        Default::default(),
        false,
        [0, 0, 0],
    );
    assert!(
        world.actors()[player.index].failure().is_none(),
        "{:?}",
        world.actors()[player.index].failure()
    );
    assert!(world.selected_player_on_rope());
    assert_eq!(world.ropes()[0].attachments[0].node, 0);
}

#[test]
fn second_jump_release_resolves_queued_motion_in_the_same_ordinary_update() {
    let (_, mut world, data, scene) = rope_classification_fixture();
    let player = world.selected_player().unwrap();
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    assert!(world.selected_player_on_rope());
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    tick_rope_classification(
        &mut world,
        &data,
        &scene,
        cyrus_script::player::input::PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
    );
    assert!(!world.selected_player_on_rope());
    assert!(
        world.actors()[player.index].failure().is_none(),
        "{:?}",
        world.actors()[player.index].failure()
    );
    // This supported fixture resolves its release immediately. Keeping the attached bypass
    // would leave ROPE queued until the following ordinary update.
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(control.queued_motion, JumpMotion::NONE);
    assert_eq!(control.jump_selector, 24);
}
