use super::*;

#[test]
fn ordinary_stable_probe_keeps_inclusive_terrain_and_unranked_model_assignment() {
    use cyrus_script::script::OrdinaryStableProbe;
    let probe = OrdinaryStableProbe::new([2560, 5121, 7680], 0, 12, 4).unwrap();
    assert_eq!(probe.start, [10, 28, 54]);
    assert_eq!(probe.end, [10, 36, 54]);
    assert_eq!(probe.model_position(30), [2560, 4608, 13824]);
    assert!(probe.accepts_terrain(28.0).unwrap());
    assert!(probe.accepts_terrain(36.0).unwrap());
    assert!(!probe.accepts_terrain(27.5).unwrap());
    assert!(probe.accepts_terrain(36.5).unwrap());
    assert!(!probe.accepts_terrain(27.49).unwrap());
    assert!(probe.accepts_terrain(36.99).unwrap());
    assert!(!probe.accepts_terrain(37.0).unwrap());
    let negative = OrdinaryStableProbe::new([0, -256, 0], 0, 0, 0).unwrap();
    assert!(negative.accepts_terrain(-1.9).unwrap());
    assert!(!negative.accepts_terrain(-0.9).unwrap());
    assert!(probe.accepts_terrain(f64::NAN).is_err());
}

#[test]
fn stable_recovery_requires_own_query_and_keeps_terrain_position() {
    use cyrus_collision::ground::TerrainGround;
    let (mut world, mut data, models) =
        static_model_fixture_at(cache(), Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_stable_recovery_query(id, &scene, &data, [600, 600], &[id])
        .unwrap();
    let before = world.spatial(id).unwrap().local.position;
    // The receiver-free branch accepts even when there is no support.
    assert!(
        world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert!(
        world
            .query_stable_recovery(other, &prepared, &scene, &data)
            .is_err()
    );
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert!(
        !world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
    // The initial shape has positive extent 20, so inclusive bounds are 39..41.
    for (height, expected) in [(38, false), (39, true), (41, true), (42, false)] {
        scene.terrain = Some(
            TerrainGround::new(vec![height; 256 * 256], vec![false; 256 * 256], None).unwrap(),
        );
        assert_eq!(
            world
                .query_stable_recovery(id, &prepared, &scene, &data)
                .unwrap(),
            expected
        );
        assert_eq!(world.spatial(id).unwrap().local.position, before);
    }
    // A water cell supplies the same interpolated height as a dry one.
    scene.terrain =
        Some(TerrainGround::new(vec![40; 256 * 256], vec![true; 256 * 256], None).unwrap());
    assert!(
        world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
}

#[test]
fn stable_recovery_model_contact_assigns_probe_position_before_terrain() {
    use cyrus_collision::ground::TerrainGround;
    let mut scripts = cache();
    scripts.instances[0].angles[1] = 768;
    let (mut world, mut data, models) =
        static_model_fixture_at(scripts, Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 25;
    scene.terrain =
        Some(TerrainGround::new(vec![40; 256 * 256], vec![false; 256 * 256], None).unwrap());
    let prepared = world
        .prepare_stable_recovery_query(id, &scene, &data, [600, 600], &[id, other])
        .unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert!(
        world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [583 * 256, 0, 583 * 256]
    );
    // The supporting instance keeps its previously stored presentation transform.
    assert_eq!(
        world
            .model_presentation(other)
            .unwrap()
            .unwrap()
            .pose
            .position,
        [600 * 256, 20 * 256, 600 * 256]
    );
}

#[test]
fn ordinary_stable_response_restores_snapshot_and_consumes_one_shared_heading_draw() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::{
        OrdinaryStableRecoveryInputs, ScriptInputs, ScriptSound, ScriptStage,
    };
    use cyrus_world::sound::AnimationSoundState;
    let mut scripts = cache();
    let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
    attributes[Attribute::TurnSpeed] = 12;
    attributes[Attribute::Weight] = 64;
    let (mut world, mut data, models) =
        static_model_fixture_at(scripts, Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = "animation_table".into();
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    let mut events = Vec::new();
    let snapshot = world
        .capture_ordinary_rollback(id, &mut |actor, sound| {
            events.push((actor, sound));
            Ok(())
        })
        .unwrap();
    let mut random = EngineRandom::from_state(1);
    assert!(
        !world
            .recover_ordinary_stable(
                id,
                OrdinaryStableRecoveryInputs {
                    scene: &scene,
                    animations: &data,
                    view: [600, 600],
                    actors: &[id],
                    random: &mut random
                },
                &snapshot,
                &mut |_, _| Ok(())
            )
            .unwrap()
    );
    assert_eq!(random.state(), 1);
    assert!(world.retain_ordinary_stable_position(id).unwrap());
    let stable = world.spatial(id).unwrap().local.position;
    world.apply_ordinary_directional_motion(id, 2, 512).unwrap();
    for _ in 0..2 {
        world
            .advance(
                id,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
    }
    assert_eq!(world.animation_frame(id).unwrap(), Some(11));
    assert_ne!(world.spatial(id).unwrap().local.position, stable);
    assert!(
        world
            .recover_ordinary_stable(
                id,
                OrdinaryStableRecoveryInputs {
                    scene: &scene,
                    animations: &data,
                    view: [600, 600],
                    actors: &[id],
                    random: &mut random
                },
                &snapshot,
                &mut |actor, sound| {
                    events.push((actor, sound));
                    Ok(())
                }
            )
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, stable);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 2030);
    assert_eq!(random.state(), 1103527590);
    assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    assert_eq!(world.actors()[0].attributes()[Attribute::Weight], 0);
    let state = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            state.move_counter,
            state.falling_state,
            state.slide_flag,
            state.last_collide.bits()
        ),
        (0, JumpMotion::NONE, 0, CollisionFlags::ROLLBACK.bits())
    );
    assert_eq!(
        events,
        [
            (
                id,
                ScriptSound::AnimationState(AnimationSoundState::Capture)
            ),
            (
                id,
                ScriptSound::AnimationState(AnimationSoundState::Restore)
            )
        ]
    );
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(3),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let state = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (state.last_collide_count, state.last_collide.bits()),
        (3, 0)
    );
}

#[test]
fn ordinary_update_walking_off_an_edge_restores_the_stable_position_and_jumps() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = cache();
    let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
    attributes[Attribute::Collide] = 4;
    attributes[Attribute::JumpTime] = 8;
    attributes[Attribute::JumpHeight] = 32;
    // The default group is the run jump group, which keeps the pending jump bits alive.
    attributes[Attribute::PanicGroup] = 18;
    for placement in &mut scripts.instances {
        placement.active = 1;
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, mut data, models) = static_model_fixture_at(scripts.clone(), None, [600, 20, 600]);
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = "animation_table".into();
    definition["animation_groups"] = serde_json::json!([0, 32, 33, 34, 35, 39, 40, 38].map(
        |group| serde_json::json!({
            "group_index":group,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":10},{"opcode":1}]
        })
    ));
    // The jump window only holds its pending bits while a run or walk jump group loops.
    definition["animation_groups"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            serde_json::json!({
                "group_index":18,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]
            }),
        );
    let stable = [7 * 256, 5000, 9 * 256];
    let mut world = replace_health_test_animation(world, &scripts, &mut data, 0, 18, |saved| {
        saved["actors"][0]["control"]["directional_state"] = serde_json::json!([1, 0]);
        saved["actors"][0]["control"]["stable_local_position"] = serde_json::json!(stable);
    });
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::WOOD).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .update_ordinary(
            &data,
            [600, 600],
            OrdinaryInputs {
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    fine_ticks: Some(8),
                    step_fps: Some(12),
                    ..Default::default()
                },
                scene: Some(&scene),
            },
            &mut random,
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    let queued_before = world.actors()[id.index]
        .control_state()
        .unwrap()
        .queued_motion;
    world
        .update_ordinary(
            &data,
            [600, 600],
            OrdinaryInputs {
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    fine_ticks: Some(8),
                    step_fps: Some(12),
                    ..Default::default()
                },
                scene: Some(&scene),
            },
            &mut random,
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    let actor = &world.actors()[id.index];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    let control = actor.control_state().unwrap();
    assert_eq!(control.actions, [0, 0]);
    // The jump entry's queued state ran one arbitration and survived to the next update.
    assert_ne!(queued_before, JumpMotion::NONE);
    assert_eq!(world.spatial(id).unwrap().local.position[0], stable[0]);
    // The running jump carries the actor forward along its heading, a rate of 0x10000 / attribute 31.
    assert_ne!(world.spatial(id).unwrap().local.position[2], stable[2]);
}

#[test]
fn selected_walk_off_recovery_starts_jump_through_ordinary_update() {
    use cyrus_script::script::{
        OrdinaryInputs, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
    let (scripts, world, mut data, mut scene) = player_walking_fixture_with(|scripts| {
        let actor = scripts.actors.get_mut("CYRUS").unwrap();
        actor.attributes[Attribute::JumpTime] = 8;
        actor.attributes[Attribute::JumpHeight] = 32;
        actor.attributes[Attribute::PanicGroup] = 18;
    });
    let index = world
        .actors()
        .iter()
        .position(|a| matches!(a.source, ScriptActorSource::Player))
        .unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["script_name"] == "CYRUS")
        .unwrap();
    definition["animation_groups"]
        .as_array_mut()
        .unwrap()
        .retain(|group| group["group_index"] != AnimationGroup::RUN.index());
    for group in [18, 32, 34, 35, 38, 39, 40] {
        definition["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "group_index":group,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
            }));
    }
    let stable = [600 * 256, 20 * 256, 600 * 256];
    let mut world = replace_health_test_animation(world, &scripts, &mut data, index, 18, |saved| {
        saved["actors"][index]["control"]["directional_state"] = serde_json::json!([1, 0]);
        saved["actors"][index]["control"]["stable_local_position"] = serde_json::json!(stable);
    });
    let player = world.id(index).unwrap();
    world
        .place_selected_player([stable[0] + 256, stable[1] + 512, stable[2] + 256], 0)
        .unwrap();
    world.refresh_model_presentations(&data, &[player]).unwrap();
    scene.settings.post_collide_height = 50;
    scene.terrain = None;
    let hud = cyrus_script::script::ScriptHud::default();
    let held = cyrus_script::player::input::PlayerInput::default();
    world
        .update_ordinary(
            &data,
            [600, 600],
            OrdinaryInputs {
                scripts: ScriptInputs {
                    hud: Some(&hud),
                    player_controller: Some(PlayerControllerInput {
                        held: &held,
                        walk_mode: false,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    fine_ticks: Some(8),
                    step_fps: Some(12),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                scene: Some(&scene),
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    let actor = &world.actors()[index];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    let control = actor.control_state().unwrap();
    assert_eq!(control.actions, [0, 0]);
    assert_eq!(control.queued_motion, JumpMotion::RUNNING);
    assert_eq!(control.falling_state, JumpMotion::NONE);
    assert_eq!(control.fall_origin_y, stable[1]);
    assert_eq!(control.fall_timer, 8);
    assert!(!control.stable_position_valid);
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [stable[0], stable[1] - 512, stable[2]]
    );
    assert_eq!(
        world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
        AnimationGroup::JUMP_RUN_START.index()
    );
}
