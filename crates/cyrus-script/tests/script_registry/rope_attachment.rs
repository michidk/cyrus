//! Early rope grabs retain their approach height until the authored grab has progressed.
use super::*;

#[test]
fn attached_rope_input_keeps_turn_acceleration_and_release_current() {
    use cyrus_script::player::input::PlayerInput;

    let (_, mut world, data, mut scene) = rope_classification_fixture();
    scene.settings.turn_speed = 12;
    scene.settings.turn_max_speed = 48;
    let player = world.selected_player().unwrap();
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    assert!(world.selected_player_on_rope());

    for expected in [12, 24, 36, 48, 48] {
        tick_rope_classification(
            &mut world,
            &data,
            &scene,
            PlayerInput {
                left: true,
                ..Default::default()
            },
            false,
        );
        assert!(world.selected_player_on_rope());
        assert!(world.actors()[player.index].failure().is_none());
        let turn = world.inspect(0, 0)["globals"]["turn"].clone();
        assert_eq!(turn["left"], expected);
        assert_eq!(turn["right"], 0);
    }

    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    let turn = world.inspect(0, 0)["globals"]["turn"].clone();
    assert_eq!(turn["left"], 0);
    assert_eq!(turn["right"], 0);
    assert!(world.selected_player_on_rope());

    tick_rope_classification(
        &mut world,
        &data,
        &scene,
        PlayerInput {
            right: true,
            jump: true,
            ..Default::default()
        },
        false,
    );
    assert!(!world.selected_player_on_rope());
    assert!(world.actors()[player.index].failure().is_none());
    let turn = world.inspect(0, 0)["globals"]["turn"].clone();
    assert_eq!(turn["left"], 0);
    assert_eq!(turn["right"], 12);
}

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
    assert_eq!(control.jump_selector, 23);
}

#[test]
fn second_jump_release_advances_regrab_countdown_on_its_ordinary_update() {
    let (_, mut world, data, scene) = rope_classification_fixture();
    let player = world.selected_player().unwrap();
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    assert!(world.selected_player_on_rope());
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        cyrus_script::player::input::PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
        [2, 16, 512],
    );
    assert!(!world.selected_player_on_rope());
    assert!(world.actors()[player.index].failure().is_none());
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .jump_selector,
        22
    );
}

#[test]
fn directional_release_carries_the_new_speed_only_on_the_next_ordinary_update() {
    use cyrus_script::player::input::PlayerInput;
    use cyrus_script::script::{OrdinaryInputs, PlayerControllerInput, ScriptHud, ScriptInputs};

    let (_, mut world, data, scene) = rope_classification_fixture_with(
        |scripts| {
            // Keep the synthetic floor from consuming the queued rope motion.
            let actor = scripts.actors.get_mut("CYRUS").unwrap();
            actor.attributes[Attribute::Collide] = 11;
            actor.attributes[Attribute::ReservedCollide] = 11;
        },
        |_| {},
    );
    let player = world.selected_player().unwrap();
    let hud = ScriptHud::default();
    hud.configure(
        &inventory_asset(&[]),
        &cyrus_game::preferences::Preferences::default(),
    );
    let tick = |world: &mut ScriptWorld, held: &PlayerInput| {
        world
            .update_ordinary(
                &data,
                [10, 30],
                OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        fine_ticks: Some(8),
                        frame_scale: Some(256),
                        step_fps: Some(12),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        magic_carpet: Some(true),
                        player_controller: Some(PlayerControllerInput {
                            held,
                            walk_mode: true,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::from_state(42),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[player.index].failure().is_none(),
            "{:?}",
            world.actors()[player.index].failure()
        );
    };
    tick(&mut world, &Default::default());
    assert!(world.selected_player_on_rope());
    tick(&mut world, &Default::default());
    let before = world.spatial(player).unwrap().local;
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_vector[2],
        0
    );
    let jump = PlayerInput {
        jump: true,
        forward: true,
        ..Default::default()
    };
    tick(&mut world, &jump);
    assert!(!world.selected_player_on_rope());
    let release = world.spatial(player).unwrap().local;
    assert_eq!(
        [release.position[0], release.position[2]],
        [before.position[0], before.position[2]]
    );
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_vector[2],
        64
    );
    tick(&mut world, &jump);
    let carried = world.spatial(player).unwrap().local;
    let expected =
        cyrus_world::transform::translate_forward(release.position, release.angles[1] as u16, 64)
            .unwrap();
    assert_eq!(
        [carried.position[0], carried.position[2]],
        [expected[0], expected[2]]
    );
}

#[test]
fn attached_rope_turn_uses_accelerated_input_at_subtick_scales() {
    use cyrus_script::player::input::PlayerInput;
    let (_, mut world, data, mut scene) = rope_classification_fixture();
    scene.settings.turn_speed = 12;
    scene.settings.turn_max_speed = 48;
    let player = world.selected_player().unwrap();
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    let before = world.inspect(player.index, 1)["actors"][0]["spatial"]["local"]["angles"][1]
        .as_i64()
        .unwrap();
    for _ in 0..40 {
        tick_rope_classification_with_timing(
            &mut world,
            &data,
            &scene,
            PlayerInput {
                walk: true,
                right: true,
                ..Default::default()
            },
            false,
            [1, 8, 90],
        );
    }
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        Default::default(),
        false,
        [1, 8, 90],
    );
    let after = world.inspect(player.index, 1)["actors"][0]["spatial"]["local"]["angles"][1]
        .as_i64()
        .unwrap();
    // BELLTOWR: heading 24 reaches 640 under 40 Right updates at scales 87..90.
    // All those scales round the accelerated 12,24,36,48 operands identically.
    assert_eq!(after - before, 640 - 24);
}

#[test]
fn downward_rope_control_searches_below_the_current_hand_link() {
    use cyrus_script::player::input::PlayerInput;
    let (scripts, mut world, mut data, scene) = rope_classification_fixture_with(
        |_| {},
        |data| {
            let actor = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|a| a["script_name"] == "CYRUS")
                .unwrap();
            actor["raex"]["grip0"] = 0.into();
            for group in [
                AnimationGroup::ROPE_PANIC,
                AnimationGroup::ROPE_SWING_FORWARD,
                AnimationGroup::ROPE_DOWN,
            ] {
                actor["animation_groups"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({
                        "group_index":group.index(),"anim_id":1,"anim_type":0,
                        "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
                    }));
            }
            data.metadata["mpsz_entries"][0]["pos_extent"][1] = 0.into();
            data.metadata["mpsz_entries"][0]["neg_extent"][1] = 48.into();
            data.metadata["mpsz_entries"][0]["center_offset"][1] = (-24).into();
            let model = data.models.get_mut("PLAYER").unwrap();
            model.base_positions = vec![[0.0, 0.0, 0.0]];
            model.positions = vec![vec![[0.0, 0.0, 0.0]]];
        },
    );
    let player = world.selected_player().unwrap();
    tick_rope_classification(&mut world, &data, &scene, Default::default(), false);
    data.metadata["ropes"][0]["position"] = serde_json::json!([-0.5, 0.3, 1.5]);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["ropes"][0]["attachments"][0][1] = 1.into();
    let position = [10 * 256, 10 * 256, 30 * 256];
    saved["spatial"]["actors"][player.index]["local"]["position"] = serde_json::json!(position);
    let globals = std::mem::take(world.globals_mut());
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        },
        cyrus_script::script::WorldRestoreContext {
            generation: 2,
            globals,
            samples: &Default::default(),
        },
    )
    .unwrap();
    world.select_player(false).unwrap();
    for _ in 0..8 {
        tick_rope_classification(
            &mut world,
            &data,
            &scene,
            PlayerInput {
                backward: true,
                ..Default::default()
            },
            false,
        );
    }
    // The synthetic animation has no hand motion; assert the public controller
    // admits the next link rather than fabricating a climb translation.
    assert_eq!(world.ropes()[0].down_limit, Some((player.index, 2)));
}
