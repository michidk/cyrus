use super::*;

#[test]
fn the_world_owns_its_exported_ropes() {
    use cyrus_assets::animation::Animations;
    let scripts = cache();
    let animations = |ropes: serde_json::Value| Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"actors":[], "markers":[], "walk_node_maps":[], "script_content_id":scripts.content_id, "ropes": ropes}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let rope = |model: &str| {
        serde_json::json!({
            "angle_y": 512, "length": 3, "position": [1.0, 2.0, 3.0], "rope_model": model,
            "rope_type": 0, "speed": 100, "static_model": "", "swing": 9,
            "unknown1": [62, 62, 63, 0, 0, 0, 0]
        })
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world
        .prepare_ropes(&animations(serde_json::json!([rope("ROPELINK")])))
        .unwrap();
    assert_eq!(world.ropes().len(), 1);
    assert_eq!(world.ropes()[0].links, 3);
    assert_eq!(world.ropes()[0].chain().len(), 3);
    let mut disabled_scripts = scripts.clone();
    disabled_scripts.updates.ropes_load = Some(false);
    disabled_scripts.content_id = disabled_scripts.content_identity().unwrap();
    let mut disabled = ScriptWorld::prepare(&disabled_scripts, 1).unwrap();
    disabled
        .prepare_ropes(&animations(serde_json::json!([rope("ROPELINK")])))
        .unwrap();
    assert!(
        disabled.ropes().is_empty(),
        "ropes_load=no suppresses construction"
    );
    let mut missing_scripts = scripts.clone();
    missing_scripts.updates.ropes_load = None;
    missing_scripts.content_id = missing_scripts.content_identity().unwrap();
    let mut missing = ScriptWorld::prepare(&missing_scripts, 1).unwrap();
    let error = missing
        .prepare_ropes(&animations(serde_json::json!([rope("ROPELINK")])))
        .unwrap_err()
        .to_string();
    assert!(error.contains("ropes_load"), "{error}");
    let mut none = ScriptWorld::prepare(&scripts, 1).unwrap();
    none.prepare_ropes(&animations(serde_json::Value::Null))
        .unwrap();
    assert!(
        none.ropes().is_empty(),
        "a cache without the records has no ropes"
    );
    let mut unknown = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = unknown
        .prepare_ropes(&animations(serde_json::json!([rope("STRANGE")])))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("rope 0") && error.contains("STRANGE"),
        "{error}"
    );
}

fn sample_rope_jump_controller(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    held: &cyrus_script::player::input::PlayerInput,
    fail_sound: bool,
) -> anyhow::Result<()> {
    use cyrus_script::script::{OrdinaryGroundQuery, PlayerControllerInput, ScriptInputs};
    let player = world.selected_player().unwrap();
    let map = world.id(0).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, scene, data, [10, 30], &[player, map])
        .unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world.begin_player_movement_update(player, 0)?;
    let result = world.advance_player_controller(
        player,
        OrdinaryGroundQuery {
            scene,
            animations: data,
            prepared: &prepared,
            distance: 4,
            sidedness: false,
        },
        cyrus_script::clock::ClockStep {
            frame_scale: 256,
            nominal_ticks: 0,
            fine_ticks: 0,
        },
        ScriptInputs {
            player_controller: Some(PlayerControllerInput {
                held,
                walk_mode: false,
                auto_defend: false,
                steer: None,
                claim: None,
                probes: None,
            }),
            hud: Some(&hud),
            fine_ticks: Some(0),
            shell_execution: Some(false),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            forward_step_clear: Some(true),
            locomotion_geometry: Some(cyrus_script::script::LocomotionGeometry {
                ground: OrdinaryGroundQuery {
                    scene,
                    animations: data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                environment: cyrus_script::script::ObstacleEnvironment {
                    selected_player: Some(player),
                    auto_grab: false,
                    item_system_ready: false,
                    shell_execution: false,
                    ticks: 1,
                },
            }),
            ..Default::default()
        },
        &mut cyrus_game::random::EngineRandom::default(),
        &mut |id, cue| {
            let _ = (id, cue);
            anyhow::ensure!(!fail_sound, "forced countdown sound rejected");
            Ok(())
        },
    );
    let finished = world.finish_player_movement_update(player);
    result?;
    finished
}

pub(super) fn rope_classification_fixture() -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    rope_classification_fixture_with(|_| {}, |_| {})
}

pub(super) fn rope_classification_fixture_with(
    prepare_scripts: impl FnOnce(&mut Scripts),
    prepare_animation: impl FnOnce(&mut cyrus_assets::animation::Animations),
) -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with_animation(
        |scripts| {
            scripts.updates.ropes_load = Some(true);
            scripts.updates.rope = Some(cyrus_assets::script::RopeSettings {
                jump_add: 64,
                attach_angle: 0,
            });
            prepare_scripts(scripts);
        },
        |data| {
            let groups = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap()["animation_groups"]
                .as_array_mut()
                .unwrap();
            for group in [6, 32, 84, 85] {
                let mut commands = vec![];
                if group == 85 {
                    commands.push(serde_json::json!({"opcode":4,"sound_param":9,"volume_shift":0}));
                }
                commands.extend([
                    serde_json::json!({"opcode":0,"frame_reference":0}),
                    serde_json::json!({"opcode":2,"value":0}),
                ]);
                groups.push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":commands}));
            }
            prepare_animation(data);
        },
    );
    let player = world.selected_player().unwrap();
    let index = player.index;
    sample_rope_jump_controller(
        &mut world,
        &data,
        &scene,
        &cyrus_script::player::input::PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .queued_motion
            .byte(),
        1
    );
    data.metadata["ropes"] = serde_json::json!([{
        "position": [-0.5, 0.3, (30.0 - 16777215.0 / 256.0) / 20.0],
        "angle_y": 0, "rope_type": 0, "swing": 0, "speed": 0,
        "length": 8, "static_model": "", "rope_model": "ROPELINK",
        "unknown1": [0, 0, 0],
    }]);
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    world.prepare_ropes(&data).unwrap();
    assert!(
        !world
            .ordinary_overlap_spheres(player, &data)
            .unwrap()
            .is_empty()
    );

    (scripts, world, data, scene)
}

pub(super) fn tick_rope_classification(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    held: cyrus_script::player::input::PlayerInput,
    fail_sound: bool,
) {
    tick_rope_classification_with_timing(world, data, scene, held, fail_sound, [1, 8, 256]);
}

pub(super) fn tick_rope_classification_with_timing(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    held: cyrus_script::player::input::PlayerInput,
    fail_sound: bool,
    [nominal_ticks, fine_ticks, frame_scale]: [u32; 3],
) {
    use cyrus_script::script::{OrdinaryInputs, PlayerControllerInput, ScriptInputs};
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .update_ordinary(
            data,
            [10, 30],
            OrdinaryInputs {
                scene: Some(scene),
                scripts: ScriptInputs {
                    nominal_ticks: Some(nominal_ticks),
                    hud: Some(&hud),
                    step_fps: Some(12),
                    fine_ticks: Some(fine_ticks),
                    frame_scale: Some(frame_scale),
                    shell_execution: Some(false),
                    player_controller: Some(PlayerControllerInput {
                        held: &held,
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
            &mut |_, cue| {
                anyhow::ensure!(
                    !fail_sound
                        || matches!(cue, cyrus_script::script::ScriptSound::AnimationState(_)),
                    "rope release sound rejected"
                );
                Ok(())
            },
            100,
        )
        .unwrap();
}

#[test]
fn rope_release_retains_signed_speed_and_defers_its_carried_classification() {
    use cyrus_script::player::input::PlayerInput;
    for (forward, backward, speed) in [(false, false, 0), (true, false, 64), (false, true, -64)] {
        let (_, mut world, data, scene) = rope_classification_fixture();
        let player = world.selected_player().unwrap();
        tick_rope_classification(
            &mut world,
            &data,
            &scene,
            PlayerInput {
                jump: true,
                ..Default::default()
            },
            false,
        );
        assert!(
            world.selected_player_on_rope(),
            "player={:?}, control={:?}",
            world.actors()[player.index].failure(),
            world.actors()[player.index].control_state()
        );
        let attached_class = world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_classification;
        tick_rope_classification(
            &mut world,
            &data,
            &scene,
            PlayerInput {
                jump: true,
                ..Default::default()
            },
            false,
        );
        assert!(
            world.selected_player_on_rope(),
            "held jump must not release the latch"
        );
        assert_eq!(
            world.actors()[player.index]
                .control_state()
                .unwrap()
                .motion_classification,
            attached_class
        );
        tick_rope_classification(&mut world, &data, &scene, PlayerInput::default(), false);
        tick_rope_classification(
            &mut world,
            &data,
            &scene,
            PlayerInput {
                jump: true,
                forward,
                backward,
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
        let control = world.actors()[player.index].control_state().unwrap();
        assert_eq!(
            (control.queued_motion.byte(), control.jump_selector),
            (0, 23)
        );
        assert_eq!(control.motion_vector[2], speed);
        assert_eq!(control.motion_classification, MotionDirection::FORWARD);
        // The ordinary wrapper clears directions without a locomotion call.
        assert_eq!(control.move_counter, 0);
    }
}

#[test]
fn rope_release_sound_failure_keeps_prior_classification_after_detaching_and_queuing() {
    use cyrus_script::player::input::PlayerInput;
    let (_, mut world, data, scene) = rope_classification_fixture();
    let player = world.selected_player().unwrap();
    tick_rope_classification(
        &mut world,
        &data,
        &scene,
        PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
    );
    assert!(
        world.selected_player_on_rope(),
        "player={:?}, control={:?}",
        world.actors()[player.index].failure(),
        world.actors()[player.index].control_state()
    );
    tick_rope_classification(&mut world, &data, &scene, PlayerInput::default(), false);
    let before = world.actors()[player.index]
        .control_state()
        .unwrap()
        .motion_classification;
    assert_eq!(before, MotionDirection::FORWARD);
    tick_rope_classification(
        &mut world,
        &data,
        &scene,
        PlayerInput {
            jump: true,
            ..Default::default()
        },
        true,
    );
    assert!(!world.selected_player_on_rope());
    assert!(
        world.actors()[player.index]
            .failure()
            .unwrap()
            .contains("rope release sound rejected")
    );
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (control.queued_motion.byte(), control.jump_selector),
        (24, 24)
    );
    assert_eq!(control.motion_classification, MotionDirection::FORWARD);
    let failure = world.actors()[player.index].failure().unwrap().to_owned();
    tick_rope_classification(&mut world, &data, &scene, PlayerInput::default(), false);
    assert_eq!(
        world.actors()[player.index].failure(),
        Some(failure.as_str())
    );
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (control.queued_motion.byte(), control.motion_classification),
        (24, MotionDirection::FORWARD)
    );
}

fn detached_rope_countdown_fixture() -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    detached_rope_countdown_fixture_with(|_| {})
}

fn detached_rope_countdown_fixture_with(
    prepare_animation: impl FnOnce(&mut cyrus_assets::animation::Animations),
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    use cyrus_script::{player::input::PlayerInput, script::ScriptInputs};
    let (_, mut world, data, scene) = rope_classification_fixture_with(
        |scripts| {
            scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::Health] = 40;
        },
        |data| {
            let groups = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap()["animation_groups"]
                .as_array_mut()
                .unwrap();
            for group in [132, 133] {
                groups.push(
                    serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
                );
            }
            prepare_animation(data);
        },
    );
    let player = world.selected_player().unwrap();
    tick_rope_classification(
        &mut world,
        &data,
        &scene,
        PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
    );
    assert!(
        world.selected_player_on_rope(),
        "player={:?}, control={:?}",
        world.actors()[player.index].failure(),
        world.actors()[player.index].control_state()
    );
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .apply_selected_contact_health(
            player,
            -1,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                shell_execution: Some(false),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::from_state(42),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(!world.selected_player_on_rope());
    assert!(world.ropes()[0].attachments.is_empty());
    assert_eq!(
        world.actors()[player.index].attributes()[Attribute::Health],
        39
    );
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.queued_motion.byte(),
            control.falling_state.byte(),
            control.jump_selector
        ),
        (0, 0, 24)
    );
    assert_eq!(control.motion_classification, MotionDirection::FORWARD);
    (world, data, scene)
}

#[test]
fn controller_jump_during_rope_countdown_completes_and_refuses_a_second_initialization() {
    use cyrus_script::player::input::PlayerInput;
    let (mut world, data, scene) = detached_rope_countdown_fixture();
    let player = world.selected_player().unwrap();
    let jump = PlayerInput {
        jump: true,
        ..Default::default()
    };
    sample_rope_jump_controller(&mut world, &data, &scene, &jump, false).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.queued_motion.byte(),
            control.jump_selector,
            control.motion_classification
        ),
        (24, 24, MotionDirection::FORWARD)
    );
    // A second live sample must not request the sounding initialization again while queued.
    sample_rope_jump_controller(&mut world, &data, &scene, &jump, true).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.queued_motion.byte(),
            control.jump_selector,
            control.motion_classification
        ),
        (24, 24, MotionDirection::FORWARD)
    );
    assert!(world.actors()[player.index].failure().is_none());
}

#[test]
fn forced_countdown_jump_sound_failure_retains_queue_before_classification_and_blocks_retry() {
    use cyrus_script::player::input::PlayerInput;
    let (mut world, data, scene) = detached_rope_countdown_fixture();
    let player = world.selected_player().unwrap();
    let jump = PlayerInput {
        jump: true,
        ..Default::default()
    };
    let error = sample_rope_jump_controller(&mut world, &data, &scene, &jump, true).unwrap_err();
    assert!(
        format!("{error:#}").contains("forced countdown sound rejected"),
        "{error:#}"
    );
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.queued_motion.byte(),
            control.jump_selector,
            control.motion_classification
        ),
        (24, 24, MotionDirection::FORWARD)
    );
    let failure = world.actors()[player.index].failure().unwrap().to_owned();
    assert!(failure.contains("forced countdown sound rejected"));
    assert!(sample_rope_jump_controller(&mut world, &data, &scene, &jump, false).is_err());
    assert_eq!(
        world.actors()[player.index].failure(),
        Some(failure.as_str())
    );
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
}

fn rope_regrab_fixture() -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let (mut world, data, scene) = detached_rope_countdown_fixture_with(|data| {
        let groups = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap()["animation_groups"]
            .as_array_mut()
            .unwrap();
        groups.push(
            serde_json::json!({"group_index":44,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
        );
        for group in groups.iter_mut().filter(|group| group["group_index"] != 0) {
            for command in group["commands"].as_array_mut().unwrap() {
                if command["opcode"] == 2 {
                    *command = serde_json::json!({"opcode":1});
                }
            }
        }
        let grab = groups
            .iter_mut()
            .find(|group| group["group_index"] == 84)
            .unwrap();
        grab["commands"].as_array_mut().unwrap().insert(
            0,
            serde_json::json!({"opcode":4,"sound_param":9,"volume_shift":0}),
        );
    });
    sample_rope_jump_controller(
        &mut world,
        &data,
        &scene,
        &cyrus_script::player::input::PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
    )
    .unwrap();
    let player = world.selected_player().unwrap();
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .jump_selector,
        24
    );
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
    // Complete the authored finite animation through the public stage, without ticking controls.
    for _ in 0..4 {
        world
            .advance(
                player,
                cyrus_script::script::ScriptStage::Animation,
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
    assert_eq!(
        world.inspect(player.index, 1)["actors"][0]["animation"]["current_group"],
        0
    );
    (world, data, scene)
}

#[test]
fn rope_regrab_is_refused_until_the_exact_countdown_expiry_update() {
    let (mut world, data, scene) = rope_regrab_fixture();
    let player = world.selected_player().unwrap();
    // The countdown refuses regrab while the detached player retains airborne motion.
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        cyrus_script::player::input::PlayerInput::default(),
        false,
        [23, 0, 0],
    );
    assert!(
        world.actors()[player.index].failure().is_none(),
        "{:?}",
        world.actors()[player.index].failure()
    );
    assert!(!world.selected_player_on_rope());
    assert!(world.ropes()[0].attachments.is_empty());
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.jump_selector,
            control.queued_motion.byte(),
            control.falling_state.byte(),
            control.motion_classification
        ),
        (1, 24, 0, MotionDirection::FORWARD)
    );
    // Repeat the jump request while one exclusion tick remains.
    sample_rope_jump_controller(
        &mut world,
        &data,
        &scene,
        &cyrus_script::player::input::PlayerInput {
            jump: true,
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .queued_motion
            .byte(),
        24
    );
    tick_rope_classification_with_timing(
        &mut world,
        &data,
        &scene,
        cyrus_script::player::input::PlayerInput::default(),
        false,
        [1, 0, 0],
    );
    assert!(
        world.actors()[player.index].failure().is_none(),
        "{:?}",
        world.actors()[player.index].failure()
    );
    assert!(
        world.selected_player_on_rope(),
        "player={:?}, control={:?}",
        world.actors()[player.index].failure(),
        world.actors()[player.index].control_state()
    );
    assert_eq!(world.ropes()[0].attachments.len(), 1);
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.jump_selector,
            control.queued_motion.byte(),
            control.falling_state.byte(),
            control.motion_classification
        ),
        (0, 0, 0, MotionDirection::FORWARD)
    );
}

#[test]
fn rope_expiry_uses_full_nominal_ticks_and_retains_queued_motion_on_grab_failure() {
    for ticks in [24, 256] {
        let (mut admitted, data, scene) = rope_regrab_fixture();
        tick_rope_classification_with_timing(
            &mut admitted,
            &data,
            &scene,
            cyrus_script::player::input::PlayerInput::default(),
            false,
            [ticks, 0, 0],
        );
        assert!(admitted.selected_player_on_rope());
        assert_eq!(
            admitted.actors()[admitted.selected_player().unwrap().index]
                .control_state()
                .unwrap()
                .jump_selector,
            0
        );
        let (mut world, data, scene) = rope_regrab_fixture();
        let player = world.selected_player().unwrap();
        tick_rope_classification_with_timing(
            &mut world,
            &data,
            &scene,
            cyrus_script::player::input::PlayerInput::default(),
            true,
            [ticks, 0, 0],
        );
        assert!(!world.selected_player_on_rope());
        assert!(world.ropes()[0].attachments.is_empty());
        assert!(
            world.actors()[player.index]
                .failure()
                .unwrap()
                .contains("rope release sound rejected")
        );
        let control = world.actors()[player.index].control_state().unwrap();
        assert_eq!(
            (
                control.jump_selector,
                control.queued_motion.byte(),
                control.motion_classification
            ),
            (0, 24, MotionDirection::FORWARD)
        );
        let failure = world.actors()[player.index].failure().unwrap().to_owned();
        tick_rope_classification_with_timing(
            &mut world,
            &data,
            &scene,
            cyrus_script::player::input::PlayerInput::default(),
            false,
            [1, 0, 0],
        );
        assert!(!world.selected_player_on_rope());
        assert_eq!(
            world.actors()[player.index].failure(),
            Some(failure.as_str())
        );
    }
}

#[test]
fn logical_session_preserves_the_selected_rope_jump_latch() {
    use cyrus_game::{
        preferences::{PreferenceKey, PreferenceStore, Preferences},
        random::EngineRandom,
    };
    use cyrus_script::{
        clock::SimulationClock,
        player::input::{PlayerInput, PlayerToggles},
        script::{
            LogicalSessionRestoreInputs, LogicalSessionSnapshot, OrdinaryInputs,
            PlayerControllerInput, ScriptActors, ScriptHud, ScriptInputs, WorldRestoreContext,
            WorldSnapshotSources,
        },
    };
    struct Store;
    impl PreferenceStore for Store {
        fn load(&self, _: PreferenceKey) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        scripts.updates.ropes_load = Some(true);
        scripts.updates.rope = Some(cyrus_assets::script::RopeSettings {
            jump_add: 64,
            attach_angle: 0,
        });
    });
    let player = world.selected_player().unwrap();
    let index = player.index;
    let groups = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "CYRUS")
        .unwrap()["animation_groups"]
        .as_array_mut()
        .unwrap();
    for group in [6, 84, 85] {
        groups.push(
            serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
        );
    }
    // A stationary rope intersects the airborne fixture player's ordinary collision sphere.
    data.metadata["ropes"] = serde_json::json!([{
        "position": [-0.5, 0.3, (30.0 - 16777215.0 / 256.0) / 20.0],
        "angle_y": 0, "rope_type": 0, "swing": 0, "speed": 0,
        "length": 8, "static_model": "", "rope_model": "ROPELINK",
        "unknown1": [0, 0, 0],
    }]);
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    world.prepare_ropes(&data).unwrap();
    world
        .prepare_ordinary_collision_shape(player, &data, &scene)
        .unwrap();
    assert!(
        !world
            .ordinary_overlap_spheres(player, &data)
            .unwrap()
            .is_empty()
    );
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = BTreeMap::from([(
        scripts.world,
        WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        },
    )]);
    // Establish an airborne starting pose only. Attachment and its input latch must be
    // created by the ordinary player update, rather than injected into the save.
    let mut airborne =
        serde_json::to_value(world.snapshot(&sources[&scripts.world]).unwrap()).unwrap();
    airborne["actors"][index]["control"]["falling_state"] = 1.into();
    airborne["actors"][index]["animation"] = serde_json::Value::Null;
    world = ScriptWorld::from_snapshot(
        serde_json::from_value(airborne).unwrap(),
        &sources[&scripts.world],
        WorldRestoreContext {
            generation: 2,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    world
        .bind_animation(world.id(index).unwrap(), &data, AnimationGroup::PANIC)
        .unwrap();
    let tick = |world: &mut ScriptWorld, jump| {
        let held = PlayerInput {
            jump,
            ..Default::default()
        };
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
                        shell_execution: Some(false),
                        player_controller: Some(PlayerControllerInput {
                            held: &held,
                            walk_mode: true,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        ..Default::default()
                    },
                },
                &mut EngineRandom::from_state(42),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
    };
    tick(&mut world, true);
    assert!(
        world.selected_player_on_rope(),
        "ordinary airborne update must grab the rope"
    );
    assert_eq!(world.ropes()[0].attachments.len(), 1);
    let asset = inventory_asset(&[]);
    let preferences = Preferences::new(Store);
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    let clock = SimulationClock::new(scripts.clock).unwrap();
    let random = EngineRandom::from_state(42);
    let inputs = || LogicalSessionRestoreInputs {
        sources: &sources,
        last_generation: 10,
        hud: &asset,
        preferences: &preferences,
        bios_tick: 100,
    };
    let samples: BTreeMap<u8, u8> = (0..16).map(|slot| (slot, slot)).collect();
    let restore = |document: serde_json::Value| {
        serde_json::from_value::<LogicalSessionSnapshot>(document)
            .unwrap()
            .plan(inputs())
            .unwrap()
            .finish(&samples)
    };
    for released_at_capture in [false, true] {
        let mut actors = ScriptActors::default();
        actors.current = Some(world);
        if released_at_capture {
            tick(actors.current.as_mut().unwrap(), false);
        }
        let document = serde_json::to_value(
            actors
                .snapshot_session(
                    &sources,
                    &hud,
                    &clock,
                    &random,
                    PlayerToggles {
                        walk_mode: true,
                        auto_defend: false,
                    },
                )
                .unwrap(),
        )
        .unwrap();
        for (edit, expected) in [
            (0, "attachment count"),
            (1, "attachment count"),
            (2, "invalid session attachment node"),
            (3, "invalid session attachment node"),
        ] {
            let mut invalid = document.clone();
            let world = &mut invalid["registry"]["worlds"][0];
            match edit {
                0 => world["actors"][index]["control"]["rope_attached"] = false.into(),
                1 => {
                    let duplicate = world["ropes"][0]["attachments"][0].clone();
                    world["ropes"][0]["attachments"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                2 => world["ropes"][0]["attachments"][0][1] = 8.into(),
                3 => world["ropes"][0]["attachments"][0][0] = 999.into(),
                _ => unreachable!(),
            }
            let error = restore(invalid)
                .err()
                .expect("invalid owner must reject off-live");
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
        let mut legacy = document.clone();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("selected_rope_jump_latch");
        let error = restore(legacy)
            .err()
            .expect("active legacy owner must reject");
        assert!(format!("{error:#}").contains("legacy selected rope jump latch unavailable"));
        assert!(!actors.current_is_restored());
        let restored_session = restore(document.clone()).unwrap();
        assert!(restored_session.actors.current_is_restored());
        assert_eq!(
            restored_session
                .actors
                .current
                .as_ref()
                .unwrap()
                .ordinary_updates(),
            0
        );
        let mut restored = restored_session.actors.current.unwrap();
        let mut uninterrupted = actors.current.take().unwrap();
        let first_jump = !released_at_capture;
        for live in [&mut uninterrupted, &mut restored] {
            tick(live, first_jump);
            assert!(live.selected_player_on_rope());
            let control = live.actors()[index].control_state().unwrap();
            assert_eq!(
                (control.queued_motion, control.jump_selector),
                (JumpMotion::NONE, 0)
            );
            assert_ne!(
                movement_snapshot(live, &scripts, &data)["actors"][index]["animation"]["playback"]
                    ["current"][0]
                    .as_u64(),
                Some(85)
            );
            tick(live, false);
            tick(live, true);
            assert!(!live.selected_player_on_rope());
            assert!(live.ropes()[0].attachments.is_empty());
            let control = live.actors()[index].control_state().unwrap();
            assert_eq!(control.jump_selector, 23);
            assert_eq!(
                movement_snapshot(live, &scripts, &data)["actors"][index]["animation"]["playback"]
                    ["current"][0]
                    .as_u64(),
                Some(85)
            );
            for _ in 0..2 {
                tick(live, false);
                assert!(
                    !live.selected_player_on_rope(),
                    "jump cooldown must prevent regrabbing the released rope after restore"
                );
                assert!(live.ropes()[0].attachments.is_empty());
                assert!(live.actors()[index].control_state().unwrap().jump_selector > 0);
            }
        }
        let mut detached = ScriptActors::default();
        detached.current = Some(restored);
        let mut legacy_detached = serde_json::to_value(
            detached
                .snapshot_session(
                    &sources,
                    &hud,
                    &clock,
                    &random,
                    PlayerToggles {
                        walk_mode: true,
                        auto_defend: false,
                    },
                )
                .unwrap(),
        )
        .unwrap();
        legacy_detached
            .as_object_mut()
            .unwrap()
            .remove("selected_rope_jump_latch");
        assert!(
            !restore(legacy_detached)
                .unwrap()
                .actors
                .current
                .unwrap()
                .selected_player_on_rope()
        );
        // Return to a naturally attached fixture for the released-at-capture branch.
        // The original held session still owns the grab, independently of the tested continuations.
        world = restore(document).unwrap().actors.current.unwrap();
    }
}
