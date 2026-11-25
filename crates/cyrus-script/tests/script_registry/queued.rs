use super::*;

#[test]
fn ordinary_queued_clearance_uses_strict_terrain_height_and_retained_model_order() {
    use cyrus_collision::ground::TerrainGround;
    use cyrus_script::script::OrdinaryGroundQuery;
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
        .unwrap();
    for (height, expected) in [(22, 512), (20, 256000), (19, 256000), (80, 15360)] {
        scene.terrain = Some(
            TerrainGround::new(vec![height; 256 * 256], vec![false; 256 * 256], None).unwrap(),
        );
        assert_eq!(
            world
                .query_ordinary_queued_clearance(
                    id,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    }
                )
                .unwrap(),
            expected
        );
    }
    // Water cells use the same interpolated height as dry cells.
    scene.terrain =
        Some(TerrainGround::new(vec![22; 256 * 256], vec![true; 256 * 256], None).unwrap());
    assert_eq!(
        world
            .query_ordinary_queued_clearance(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 1,
                    sidedness: false,
                }
            )
            .unwrap(),
        512
    );
}

#[test]
fn ordinary_queued_arbitration_preserves_group_and_pending_precedence() {
    use cyrus_script::script::{
        OrdinaryQueuedAction, OrdinaryQueuedInputs, ordinary_queued_action,
    };
    let input = OrdinaryQueuedInputs {
        queued: JumpMotion::NONE,
        torch_burning: 0,
        current_group: Some(AnimationGroup::PANIC),
        pending_group: None,
        has_jump_backward: true,
        has_jump_backward_torch: true,
        holding_weapon: false,
        edge_handled: false,
    };
    assert_eq!(
        ordinary_queued_action(input),
        OrdinaryQueuedAction::default()
    );
    for (queued, torch_burning, holding, group) in [
        (JumpMotion::STANDING, 0, false, 39),
        (JumpMotion::STANDING, 0, true, 112),
        (JumpMotion::from_byte(129), 0, true, 40),
        (JumpMotion::from_byte(129), 1, false, 38),
        (JumpMotion::FORWARD, 0, false, 42),
        (JumpMotion::from_byte(130), 0, false, 43),
        (JumpMotion::from_byte(130), 1, false, 41),
        (JumpMotion::BACKWARD, 0, false, 49),
        (JumpMotion::from_byte(131), 1, false, 50),
        (JumpMotion::RUNNING, 0, false, 44),
        (JumpMotion::from_byte(136), 0, false, 48),
        (JumpMotion::from_byte(136), 1, false, 45),
        (JumpMotion::ROPE, 0, false, 44),
        (JumpMotion::from_byte(152), 0, false, 48),
        (JumpMotion::from_byte(152), 1, false, 45),
        (JumpMotion::RUNNING_LEFT_LEAD, 0, false, 46),
        (JumpMotion::from_byte(137), 0, false, 48),
        (JumpMotion::from_byte(137), 1, false, 47),
    ] {
        assert_eq!(
            ordinary_queued_action(OrdinaryQueuedInputs {
                queued,
                torch_burning,
                holding_weapon: holding,
                current_group: Some(if torch_burning == 0 {
                    AnimationGroup::JUMP_BACKWARD
                } else {
                    AnimationGroup::JUMP_BACKWARD_TORCH
                }),
                ..input
            }),
            OrdinaryQueuedAction {
                clear_queued: false,
                handled: true,
                advance_vertical: true,
                request_group: Some(AnimationGroup::from_index(group))
            }
        );
    }
    for pending in [32, 33, 34, 35, 36, 37, 111] {
        assert_eq!(
            ordinary_queued_action(OrdinaryQueuedInputs {
                queued: JumpMotion::STANDING,
                pending_group: Some(AnimationGroup::from_index(pending)),
                edge_handled: true,
                ..input
            }),
            OrdinaryQueuedAction::default()
        );
    }
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: JumpMotion::BACKWARD,
            has_jump_backward: false,
            pending_group: Some(AnimationGroup::JUMP_START),
            edge_handled: true,
            ..input
        }),
        OrdinaryQueuedAction {
            clear_queued: true,
            ..Default::default()
        }
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: JumpMotion::BACKWARD,
            ..input
        }),
        OrdinaryQueuedAction::default()
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: JumpMotion::BACKWARD,
            torch_burning: 1,
            ..input
        }),
        OrdinaryQueuedAction {
            handled: true,
            ..Default::default()
        }
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: JumpMotion::STANDING,
            edge_handled: true,
            ..input
        }),
        OrdinaryQueuedAction {
            clear_queued: true,
            handled: true,
            ..Default::default()
        }
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: JumpMotion::from_byte(128),
            ..input
        }),
        OrdinaryQueuedAction {
            handled: true,
            advance_vertical: true,
            ..Default::default()
        }
    );
}

#[test]
fn ordinary_jump_entry_drives_queued_motion_to_falling_through_shared_animation() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table", "ravc_count":0,
                "animation_groups": ([0, 32, 39, 56, 58].into_iter().map(|group| serde_json::json!({
                    "group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":group},{"opcode":1}]
                })).collect::<Vec<_>>())}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.snapshot_spatial(id).unwrap();
    let origin = world.spatial(id).unwrap().local.position;
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    assert!(world.retain_ordinary_stable_position(id).unwrap());
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_local_position,
        origin
    );
    assert!(
        !world
            .begin_ordinary_jump(id, &data, 0, true, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .queued_motion,
        JumpMotion::NONE
    );
    assert!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_position_valid
    );
    assert!(
        world
            .begin_ordinary_jump(id, &data, 0, false, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert!(!world.retain_ordinary_stable_position(id).unwrap());
    assert!(
        !world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_position_valid
    );
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_local_position,
        origin
    );
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.queued_motion,
            control.fall_timer,
            control.fall_origin_y,
            control.motion_direction
        ),
        (JumpMotion::STANDING, 0, origin[1], MotionDirection::FORWARD)
    );
    assert!(
        !world
            .begin_ordinary_jump(id, &data, 0, false, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 50,
        sidedness: true,
    };
    // Replacing the current default starts the requested group immediately.
    assert_eq!(world.animation_frame(id).unwrap(), Some(32));
    for (timer, offset, queued, falling) in [
        (8, -512, JumpMotion::STANDING, JumpMotion::NONE),
        (16, -4160, JumpMotion::STANDING, JumpMotion::NONE),
        (24, -6624, JumpMotion::STANDING, JumpMotion::NONE),
        (32, -7936, JumpMotion::NONE, JumpMotion::STANDING),
    ] {
        assert!(
            world
                .resolve_ordinary_queued_motion(id, query(), environment, 8, 0, &mut |_, _| Ok(()))
                .unwrap()
        );
        let control = world.actors()[id.index].control_state().unwrap();
        assert_eq!(
            (
                control.fall_timer,
                control.queued_motion,
                control.falling_state
            ),
            (timer, queued, falling)
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [origin[0], origin[1] + offset, origin[2]]
        );
    }
    assert!(
        world
            .continue_ordinary_falling(id, &data, environment, 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(32));
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (control.fall_timer, control.falling_state),
        (24, JumpMotion::STANDING)
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position[1],
        origin[1] - 8192
    );
}

#[test]
fn ordinary_queued_clearance_truncates_fractional_terrain_toward_zero() {
    use cyrus_collision::ground::TerrainGround;
    use cyrus_script::script::OrdinaryGroundQuery;
    for (actor_y, flat_height, diagonal_height, expected) in
        [(20, 22, 24, 512), (-25, -22, -24, 768)]
    {
        let (mut world, data, models) =
            static_model_fixture_at(cache(), Some(20.0), [600, actor_y, 600]);
        let id = world.id(0).unwrap();
        world.refresh_model_presentations(&data, &[id]).unwrap();
        let mut scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
            .unwrap();
        let mut heights = vec![flat_height; 256 * 256];
        heights[(256 - 2) * 256 + 3] = diagonal_height;
        scene.terrain = Some(TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap());
        assert_eq!(
            world
                .query_ordinary_queued_clearance(
                    id,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    }
                )
                .unwrap(),
            expected
        );
    }
}

#[test]
fn ordinary_update_promotes_action_words_at_the_window_boundary_and_selects_both_jump_rows() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for (group, bit, queued, request) in [
        (18, 1, JumpMotion::RUNNING, 34),
        (18, 2, JumpMotion::RUNNING_LEFT_LEAD, 36),
        (30, 1, JumpMotion::RUNNING, 35),
        (30, 2, JumpMotion::RUNNING_LEFT_LEAD, 37),
        (18, 3, JumpMotion::RUNNING, 34),
        (30, 3, JumpMotion::RUNNING, 35),
    ] {
        for (initial_timer, first_timer, second_ticks) in [(35, 36, 1), (255, 0, 37)] {
            let mut scripts = cache();
            let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
            for (index, value) in [
                (Attribute::Collide, 4),
                (Attribute::JumpTime, 8),
                (Attribute::JumpHeight, 32),
                (Attribute::PanicGroup, group),
                (Attribute::FallHold, 1),
            ] {
                attributes[index] = value;
            }
            for placement in &mut scripts.instances {
                placement.active = 1;
                placement.object_type = PlacementType::MODEL;
                placement.map_position = [600, 20, 600];
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let (world, mut data, models) =
                static_model_fixture_at(scripts.clone(), None, [600, 20, 600]);
            let definition = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "LOGIC")
                .unwrap();
            definition["model_load_path"] = "animation_table".into();
            definition["animation_groups"] = serde_json::json!(
                [0, 18, 30, 34, 35, 36, 37, 44, 46].map(|group| serde_json::json!({
                    "group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]
                }))
            );
            let mut world = replace_health_test_animation(
                world,
                &scripts,
                &mut data,
                0,
                group as u16,
                |saved| {
                    saved["actors"][0]["control"]["directional_state"] =
                        serde_json::json!([0x100 | bit, 0x200]);
                    saved["actors"][0]["control"]["jump_window"] = initial_timer.into();
                },
            );
            let scene = ordinary_collision_scene(world.world, models);
            let mut random = cyrus_game::random::EngineRandom::default();
            for promoted in [false, true] {
                world
                    .update_ordinary(
                        &data,
                        [600, 600],
                        OrdinaryInputs {
                            scripts: ScriptInputs {
                                nominal_ticks: Some(if promoted { second_ticks } else { 1 }),
                                frame_scale: Some(256),
                                fine_ticks: Some(0),
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
                let actor = &world.actors()[0];
                assert!(actor.failure().is_none(), "{:?}", actor.failure());
                let control = actor.control_state().unwrap();
                if promoted {
                    assert_eq!(control.jump_window, 0);
                    assert_eq!(control.actions, [0, 0x300]);
                    assert_eq!(control.queued_motion, queued);
                    let animation = &world.inspect(0, 1)["actors"][0]["animation"];
                    assert!(
                        animation["current_group"] == request
                            || animation["pending_group"] == request,
                        "{animation}"
                    );
                } else {
                    assert_eq!(control.jump_window, first_timer);
                    assert_eq!(control.actions, [0x100 | bit, 0x200]);
                    assert_eq!(control.queued_motion, JumpMotion::NONE);
                }
            }
        }
    }
}

#[test]
fn ordinary_update_consumes_both_authored_jump_masks_before_the_fallback() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for (group, mask, queued, request) in [
        (18, 1, JumpMotion::RUNNING, 34),
        (18, 2, JumpMotion::RUNNING_LEFT_LEAD, 36),
        (30, 1, JumpMotion::RUNNING, 35),
        (30, 2, JumpMotion::RUNNING_LEFT_LEAD, 37),
    ] {
        let mut scripts = cache();
        let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
        for (index, value) in [
            (Attribute::Collide, 4),
            (Attribute::JumpTime, 8),
            (Attribute::JumpHeight, 32),
            (Attribute::PanicGroup, group),
            (Attribute::FallHold, 1),
        ] {
            attributes[index] = value;
        }
        for placement in &mut scripts.instances {
            placement.active = 1;
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [600, 20, 600];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let (world, mut data, models) =
            static_model_fixture_at(scripts.clone(), None, [600, 20, 600]);
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "LOGIC")
            .unwrap();
        definition["model_load_path"] = "animation_table".into();
        definition["animation_groups"] = serde_json::json!(
                [0, 18, 30, 34, 35, 36, 37, 44, 46].map(|group| serde_json::json!({
                    "group_index":group,"anim_id":1,"anim_type":0,
                    "commands": if group == 18 || group == 30 { serde_json::json!([{"opcode":15,"trigger_mask":mask,"start_frame":0,"target_group":1},{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]) } else { serde_json::json!([{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]) }
                }))
            );
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, 0, group as u16, |saved| {
                saved["actors"][0]["control"]["directional_state"] =
                    serde_json::json!([0x103, 0x200]);
                saved["actors"][0]["control"]["jump_window"] = 0.into();
            });
        let scene = ordinary_collision_scene(world.world, models);
        let mut random = cyrus_game::random::EngineRandom::default();
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(0),
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
        let actor = &world.actors()[0];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        let control = actor.control_state().unwrap();
        assert_eq!(control.jump_window, 1);
        assert_eq!(control.actions, [0x100, 0x300]);
        assert_eq!(control.queued_motion, queued);
        let animation = &world.inspect(0, 1)["actors"][0]["animation"];
        assert!(
            animation["current_group"] == request || animation["pending_group"] == request,
            "{animation}"
        );
    }
}
