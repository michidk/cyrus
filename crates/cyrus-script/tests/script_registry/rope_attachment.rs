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
