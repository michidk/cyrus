use super::*;

#[test]
fn ordinary_falling_requests_preserve_priority_and_short_distance_boundary() {
    use cyrus_script::script::{OrdinaryFallingRequest, ordinary_falling_request};
    let requested = |group| {
        Some(OrdinaryFallingRequest {
            group,
            force: false,
        })
    };
    assert_eq!(
        ordinary_falling_request(JumpMotion::NONE, 0, false, None, 0, 0),
        None
    );
    assert_eq!(
        ordinary_falling_request(
            JumpMotion::from_byte(131),
            0,
            true,
            Some(AnimationGroup::FIGHT_FALL_ATTACK),
            0,
            0
        ),
        requested(AnimationGroup::FALL_BACKWARD)
    );
    assert_eq!(
        ordinary_falling_request(
            JumpMotion::from_byte(131),
            1,
            true,
            Some(AnimationGroup::FIGHT_FALL_ATTACK),
            0,
            0
        ),
        requested(AnimationGroup::FALL_BACKWARD_TORCH)
    );
    assert_eq!(
        ordinary_falling_request(JumpMotion::from_byte(136), 0, true, None, 0, 0),
        requested(AnimationGroup::FALL_UP_ARMS)
    );
    assert_eq!(
        ordinary_falling_request(JumpMotion::RUNNING, 0, true, None, 32767, 0),
        requested(AnimationGroup::FALL_RUN)
    );
    assert_eq!(
        ordinary_falling_request(JumpMotion::RUNNING_LEFT_LEAD, 1, false, None, -32767, 0),
        requested(AnimationGroup::FALL_LEFT_TORCH)
    );
    assert_eq!(
        ordinary_falling_request(JumpMotion::RUNNING, 0, false, None, 32768, 0),
        requested(AnimationGroup::FALL)
    );
    assert_eq!(
        ordinary_falling_request(JumpMotion::RUNNING_LEFT_LEAD, 1, false, None, -32768, 0),
        requested(AnimationGroup::FALL_TORCH)
    );
    assert_eq!(
        ordinary_falling_request(
            JumpMotion::STANDING,
            0,
            true,
            Some(AnimationGroup::FIGHT_FALL_ATTACK),
            0,
            0
        ),
        None
    );
    assert_eq!(
        ordinary_falling_request(
            JumpMotion::STANDING,
            0,
            true,
            Some(AnimationGroup::FIGHT_LAND),
            0,
            0
        ),
        Some(OrdinaryFallingRequest {
            group: AnimationGroup::FIGHT_FALL,
            force: true
        })
    );
}

#[test]
fn ordinary_falling_caller_obeys_the_live_animation_vertical_marker() {
    use cyrus_script::script::ObstacleEnvironment;
    for suppressed in [false, true] {
        let (mut world, _, mut data) = movement_fixture(
            32,
            suppressed,
            true,
            |scripts| {
                scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
            },
            |saved| {
                saved["actors"][0]["control"]["falling_state"] = 1.into();
                saved["actors"][0]["control"]["fall_timer"] = 30.into();
                saved["actors"][0]["control"]["fall_origin_y"] = 10000.into();
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
        let id = world.id(0).unwrap();
        let before = world.spatial(id).unwrap().local.position;
        assert!(
            world
                .continue_ordinary_falling(
                    id,
                    &data,
                    ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 0,
                    },
                    8,
                    0,
                    50,
                    &mut |_, _| Ok(()),
                )
                .unwrap()
        );
        let control = world.actors()[id.index].control_state().unwrap();
        assert_eq!(control.falling_state, JumpMotion::STANDING);
        assert_eq!(control.fall_origin_y, 10000);
        assert_eq!(control.fall_timer, if suppressed { 30 } else { 22 });
        let after = world.spatial(id).unwrap().local.position;
        assert_eq!([after[0], after[2]], [before[0], before[2]]);
        assert_eq!(after[1], if suppressed { before[1] } else { 1808 });
    }
}

#[test]
fn ordinary_falling_reads_the_replacement_groups_vertical_marker() {
    use cyrus_script::script::{ObstacleEnvironment, ScriptInputs};
    for replacement_suppressed in [false, true] {
        let (world, scripts, mut data) = movement_fixture(
            0,
            true,
            true,
            |scripts| {
                let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
                attributes[Attribute::JumpHeight] = 32;
                attributes[Attribute::PanicGroup] = 0;
            },
            |saved| {
                saved["actors"][0]["control"]["falling_state"] = 1.into();
                saved["actors"][0]["control"]["fall_timer"] = 30.into();
                saved["actors"][0]["control"]["fall_origin_y"] = 10000.into();
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
        let mut commands = vec![];
        if replacement_suppressed {
            commands.push(serde_json::json!({"opcode":5}));
        }
        commands.push(serde_json::json!({"opcode":0,"frame_reference":56}));
        commands.push(serde_json::json!({"opcode":2,"value":0}));
        data.metadata["actors"][0]["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "group_index":56,"anim_id":1,"anim_type":0,"commands":commands
            }));
        let mut world = replace_health_test_animation(world, &scripts, &mut data, 0, 0, |_| {});
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        let before = world.spatial(id).unwrap().local.position;
        let fall = |world: &mut ScriptWorld| {
            world
                .continue_ordinary_falling(
                    id,
                    &data,
                    ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 0,
                    },
                    8,
                    0,
                    50,
                    &mut |_, _| Ok(()),
                )
                .unwrap()
        };
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
            AnimationGroup::PANIC.index()
        );
        assert!(fall(&mut world));
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
            AnimationGroup::FALL.index()
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.falling_state, JumpMotion::STANDING);
        assert_eq!(control.fall_origin_y, 10000);
        assert_eq!(
            control.fall_timer,
            if replacement_suppressed { 30 } else { 22 }
        );
        let after = world.spatial(id).unwrap().local.position;
        assert_eq!([after[0], after[2]], [before[0], before[2]]);
        assert_eq!(
            after[1],
            if replacement_suppressed {
                before[1]
            } else {
                1808
            }
        );
    }
}

#[test]
fn ordinary_static_support_miss_initializes_once_and_continues_vertical_motion() {
    use cyrus_script::script::ObstacleEnvironment;
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let y = world.spatial(id).unwrap().local.position[1];
    assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.fall_origin_y,
            control.fall_timer,
            control.falling_state
        ),
        (y.wrapping_add(8192), 30, JumpMotion::STANDING)
    );
    assert!(!world.begin_static_ordinary_fall(id, &data).unwrap());
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    assert!(
        world
            .continue_ordinary_falling(id, &data, environment, 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position[1], y);
    assert_eq!(
        world.actors()[id.index].control_state().unwrap().fall_timer,
        22
    );
    assert_eq!(world.animation_frame(id).unwrap(), None);
}

#[test]
fn falling_static_models_keep_full_support_distance_at_every_frame_rate() {
    assert_static_fall_landing(1, 97, 4, true, false);
}

#[test]
fn undamaging_static_falls_land_without_an_animation_receiver() {
    assert_static_fall_landing(1000, 100, 4, true, false);
}

#[test]
fn static_ground_admission_does_not_require_collision_bits() {
    for mask in [0, 1, 4, 8, 15] {
        assert_static_fall_landing(1000, 100, mask, true, false);
    }
}

#[test]
fn static_ground_stage_retains_motion_when_disabled_or_held() {
    for mask in [0, 4, 15] {
        assert_static_fall_landing(1000, 100, mask, false, false);
        assert_static_fall_landing(1000, 100, mask, true, true);
    }
}

fn assert_static_fall_landing(
    hurt_height: i32,
    expected_health: u8,
    collision_mask: u8,
    fall_damage: bool,
    fall_hold: bool,
) {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    for fps in [Some(9), Some(10), Some(30), None] {
        let mut scripts = cache();
        scripts.instances.truncate(1);
        scripts.updates.fall_landing = Some(cyrus_assets::script::FallLanding {
            death_height: 1000,
            hurt_height,
            hurt_zap: 3,
            bounce_height: None,
        });
        scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
            (Attribute::Collide, collision_mask),
            (Attribute::FallDamage, u8::from(fall_damage)),
            (Attribute::JumpHeight, 32),
            (Attribute::Health, 100),
        ]);
        let (mut world, data, models) =
            static_model_fixture_at(scripts, Some(20.0), [600, 20, 600]);
        let id = world.id(0).unwrap();
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.settings.post_collide_height = 500;
        scene.collision.texture_surfaces = Some(Default::default());
        scene.terrain = Some(
            cyrus_collision::ground::TerrainGround::new(
                vec![450; 256 * 256],
                vec![false; 256 * 256],
                Some(vec![1; 256 * 256]),
            )
            .unwrap(),
        );
        assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
        world.set_attribute_for_test(id, Attribute::FallHold, u8::from(fall_hold));
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(1),
                        step_fps: fps,
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| panic!("static landing emitted animation audio"),
                100,
            )
            .unwrap();
        let actor = &world.actors()[0];
        eprintln!(
            "static landing: mask={collision_mask} enabled={fall_damage} held={fall_hold} hurt_height={hurt_height} fps={fps:?} y_q8={} falling={:?} health={} failure={:?}",
            world.spatial(id).unwrap().local.position[1],
            actor.control_state().unwrap().falling_state,
            actor.attributes()[Attribute::Health],
            actor.failure(),
        );
        assert!(
            actor.failure().is_none(),
            "fps {fps:?}: {:?}",
            actor.failure()
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position[1],
            if fall_damage && !fall_hold {
                428 * 256
            } else {
                20 * 256
            },
            "fps {fps:?}"
        );
        assert_eq!(
            actor.control_state().unwrap().falling_state,
            if fall_damage && !fall_hold {
                JumpMotion::NONE
            } else {
                JumpMotion::STANDING
            },
            "fps {fps:?}"
        );
        assert_eq!(
            actor.attributes()[Attribute::Health],
            expected_health,
            "fps {fps:?}"
        );
        assert_eq!(world.animation_frame(id).unwrap(), None);
    }
}

#[test]
fn ordinary_animated_fall_entry_distinguishes_rollback_from_start() {
    use cyrus_script::script::{OrdinaryFallEntry, ordinary_fall_entry};
    for group in [AnimationGroup::RUN, AnimationGroup::RUN_TORCH] {
        assert_eq!(
            ordinary_fall_entry(group, AnimationGroup::IDLE_LEDGE, true, 1, true),
            OrdinaryFallEntry::RestoreStable {
                force_group: AnimationGroup::IDLE_LEDGE
            }
        );
        assert_eq!(
            ordinary_fall_entry(group, AnimationGroup::IDLE_LEDGE, false, 0, true),
            OrdinaryFallEntry::Start {
                force_group: Some(AnimationGroup::FALL_RUN)
            }
        );
        assert_eq!(
            ordinary_fall_entry(group, AnimationGroup::IDLE_LEDGE, false, 1, false),
            OrdinaryFallEntry::Start {
                force_group: Some(AnimationGroup::FALL_RIGHT_TORCH)
            }
        );
    }
    assert_eq!(
        ordinary_fall_entry(AnimationGroup::PANIC, AnimationGroup::PANIC, true, 0, false),
        OrdinaryFallEntry::Start {
            force_group: Some(AnimationGroup::FALL)
        }
    );
    assert_eq!(
        ordinary_fall_entry(AnimationGroup::PANIC, AnimationGroup::PANIC, true, 1, false),
        OrdinaryFallEntry::Start {
            force_group: Some(AnimationGroup::FALL_TORCH)
        }
    );
    assert_eq!(
        ordinary_fall_entry(
            AnimationGroup::FIGHT_LAND,
            AnimationGroup::PANIC,
            false,
            1,
            true
        ),
        OrdinaryFallEntry::Start {
            force_group: Some(AnimationGroup::FIGHT_FALL)
        }
    );
    assert_eq!(
        ordinary_fall_entry(
            AnimationGroup::FIGHT_FALL_ATTACK,
            AnimationGroup::PANIC,
            false,
            0,
            true
        ),
        OrdinaryFallEntry::Start { force_group: None }
    );
}

#[test]
fn ordinary_animated_fall_entry_commits_before_forced_request() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::ObstacleEnvironment;
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Health] = 100;
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "animation_groups":([0,56,65,73,74,134].into_iter().map(|group| serde_json::json!({
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
    assert_eq!(
        world
            .apply_ordinary_directional_motion(id, 1, 4096)
            .unwrap(),
        None
    );
    world.set_actor_surface(id, Surface::DEEPWATER).unwrap();
    let before = world.spatial(id).unwrap().local;
    assert_eq!(
        world
            .apply_ordinary_directional_motion(id, 2, 4096)
            .unwrap(),
        Some(MotionDirection::FORWARD)
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        cyrus_world::transform::translate_forward(
            before.position,
            before.wrapped_angles()[1],
            2048
        )
        .unwrap()
    );
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (control.motion_direction, control.move_counter),
        (MotionDirection::FORWARD, 48)
    );
    let y = world.spatial(id).unwrap().local.position[1];
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    assert!(
        world
            .complete_ordinary_animated_fall_entry(id, &data, environment, &mut |_, _| Ok(()))
            .unwrap()
    );
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.falling_state,
            control.fall_timer,
            control.fall_origin_y
        ),
        (JumpMotion::STANDING, 30, y.wrapping_add(8192))
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(56));
    assert!(
        world
            .complete_ordinary_animated_fall_entry(id, &data, environment, &mut |_, _| Ok(()))
            .unwrap_err()
            .to_string()
            .contains("already falling")
    );
    assert!(
        world
            .change_ordinary_health(id, -10, &data, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.actors()[id.index].attributes()[Attribute::Health], 90);
    let damage = world.take_damage_events();
    assert_eq!(damage.len(), 1);
    assert_eq!((damage[0].actor, damage[0].loss), (id, 10));
    assert_eq!(damage[0].source_position, None);
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
    let decision = world
        .resolve_ordinary_landing_damage(
            id,
            &data,
            environment,
            cyrus_script::script::OrdinaryLandingSettings {
                death_height: 100,
                hurt_height: 1,
                hurt_zap: 10,
            },
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        decision,
        cyrus_script::script::OrdinaryLandingDecision::Hurt { health_change: -10 }
    );
    assert_eq!(world.actors()[id.index].attributes()[Attribute::Health], 80);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .falling_state,
        JumpMotion::NONE
    );
    world
        .complete_ordinary_landing(id, &data, environment, None, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .falling_state,
        JumpMotion::NONE
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(56));
    assert!(
        world
            .change_ordinary_health(id, -256, &data, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.actors()[id.index].attributes()[Attribute::Health], 0);
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(control.ai_type, AiType::DEAD);
    assert_eq!(control.ai_mode, 0);
    assert_eq!(world.animation_frame(id).unwrap(), Some(134));
}

#[test]
fn static_directional_motion_does_not_require_an_animation_receiver() {
    use cyrus_script::script::MotionDirection;
    for (surface, distance, expected) in [
        (Surface::UNKNOWN, 4097, 4097),
        (Surface::DEEPWATER, 4097, 2048),
        (Surface::SCAPEDEEPWATER, 4097, 2048),
        (Surface::SCAPEWATER, 4097, 4097),
        (Surface::UNKNOWN, 0, 0),
        (Surface::UNKNOWN, -1, 0),
    ] {
        let (mut world, _, _) = static_model_fixture(cache());
        let id = world.id(0).unwrap();
        world.set_actor_surface(id, surface).unwrap();
        let before = world.spatial(id).unwrap().local;
        assert_eq!(world.animation_frame(id).unwrap(), None);
        assert_eq!(
            world
                .apply_ordinary_directional_motion(id, 1, distance)
                .unwrap(),
            None
        );
        assert_eq!(world.spatial(id).unwrap().local.position, before.position);
        assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 0);
        assert_eq!(
            world
                .apply_ordinary_directional_motion(id, 2, distance)
                .unwrap(),
            Some(MotionDirection::FORWARD)
        );
        let after = world.spatial(id).unwrap().local.position;
        assert_eq!(
            after,
            cyrus_world::transform::translate_forward(
                before.position,
                before.wrapped_angles()[1],
                expected
            )
            .unwrap()
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.motion_direction, control.move_counter),
            (MotionDirection::FORWARD, 48)
        );
        assert_eq!(
            serde_json::to_value(world.globals()).unwrap()["locomotion_called"],
            4
        );
        eprintln!(
            "static directional: surface={surface:?} distance={distance} before={:?} after={after:?} counter={}",
            before.position, control.move_counter
        );
        assert_eq!(world.animation_frame(id).unwrap(), None);
    }
}

#[test]
fn ordinary_directional_motion_retains_flags_when_animation_suppresses_translation() {
    use cyrus_script::script::{OrdinaryDirectionalMotion, ordinary_directional_motion};
    for (motion, direction, step, heading) in [
        (2, MotionDirection::FORWARD, StepDirection::Forward, 2000),
        (6, MotionDirection::FORWARD, StepDirection::Forward, 2000),
        (8, MotionDirection::FORWARD, StepDirection::Forward, 2000),
        (9, MotionDirection::FORWARD, StepDirection::Forward, 2000),
        (24, MotionDirection::FORWARD, StepDirection::Forward, 2000),
        (3, MotionDirection::BACKWARD, StepDirection::Backward, 976),
        (7, MotionDirection::BACKWARD, StepDirection::Backward, 976),
        (4, MotionDirection::LEFT, StepDirection::Left, 464),
        (5, MotionDirection::RIGHT, StepDirection::Right, 1488),
    ] {
        assert_eq!(
            ordinary_directional_motion(motion | 128, 2000, 4096, Surface::DEEPWATER, false),
            Some(OrdinaryDirectionalMotion {
                direction,
                step,
                heading,
                distance: Some(2048)
            })
        );
        assert_eq!(
            ordinary_directional_motion(motion, 2000, 4096, Surface::UNKNOWN, true),
            Some(OrdinaryDirectionalMotion {
                direction,
                step,
                heading,
                distance: None
            })
        );
    }
    for motion in [0, 1, 10, 23, 25, 127] {
        assert_eq!(
            ordinary_directional_motion(motion, 0, 4096, Surface::UNKNOWN, false),
            None
        );
    }
    assert_eq!(
        ordinary_directional_motion(2, 0, -1, Surface::SCAPEDEEPWATER, false)
            .unwrap()
            .distance,
        None
    );
    assert_eq!(
        ordinary_directional_motion(2, 0, 4097, Surface::SCAPEDEEPWATER, false)
            .unwrap()
            .distance,
        Some(2048)
    );
    assert_eq!(
        ordinary_directional_motion(2, 0, 4097, Surface::SCAPEWATER, false)
            .unwrap()
            .distance,
        Some(4097)
    );
}

#[test]
fn ordinary_update_starts_and_continues_an_animated_fall_after_a_support_miss() {
    ordinary_animated_fall_landing(false, false, false);
}

#[test]
fn ordinary_update_resolves_landing_surface_before_fatal_damage() {
    ordinary_animated_fall_landing(true, true, false);
    ordinary_animated_fall_landing(false, true, false);
}

#[test]
fn ordinary_update_defers_falling_to_the_active_model_vertical_marker() {
    ordinary_animated_fall_landing(false, false, true);
}

fn ordinary_animated_fall_landing(water: bool, fatal_distance: bool, model_vertical: bool) {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = cache();
    scripts.updates.fall_landing = Some(cyrus_assets::script::FallLanding {
        death_height: if fatal_distance { 1 } else { 568 },
        hurt_height: if fatal_distance { 0 } else { 300 },
        hurt_zap: 10,
        bounce_height: None,
    });
    let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
    attributes[Attribute::Health] = 100;
    attributes[Attribute::Collide] = 4;
    attributes[Attribute::JumpHeight] = 32;
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (mut world, mut data, models) = static_model_fixture_at(scripts, None, [600, 20, 600]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = "animation_table".into();
    definition["animation_groups"] =
        serde_json::json!([0, 56, 57, 65, 74].map(|group| serde_json::json!({
            "group_index":group,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":10},{"opcode":1}]
        })));
    if model_vertical {
        for group in definition["animation_groups"].as_array_mut().unwrap() {
            group["commands"] = serde_json::json!([
                {"opcode":5},
                {"opcode":0,"frame_reference":10},
                {"opcode":2,"value":0}
            ]);
        }
    }
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let start_y = world.spatial(id).unwrap().local.position[1];
    let mut random = cyrus_game::random::EngineRandom::default();
    let mut step = |world: &mut ScriptWorld, scene: &cyrus_script::player::data::PlayerScene| {
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
                    scene: Some(scene),
                },
                &mut random,
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[id.index].failure().is_none(),
            "{:?}",
            world.actors()[id.index].failure()
        );
    };
    step(&mut world, &scene);
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(control.falling_state, JumpMotion::STANDING);
    assert_eq!(control.fall_origin_y, start_y + 32 * 256);
    if model_vertical {
        // Both entry and subsequent host updates must retain the model's vertical ownership.
        for _ in 0..3 {
            let control = world.actors()[id.index].control_state().unwrap();
            assert_eq!(control.falling_state, JumpMotion::STANDING);
            assert_eq!(control.fall_timer, 30);
            assert_eq!(control.fall_origin_y, start_y + 32 * 256);
            assert_eq!(world.spatial(id).unwrap().local.position[1], start_y);
            step(&mut world, &scene);
        }
        return;
    }
    // Entry runs the writer once: factor[30] is 256, so Y is unchanged and the byte timer drops by eight.
    assert_eq!(
        (
            control.fall_timer,
            world.spatial(id).unwrap().local.position[1]
        ),
        (22, start_y)
    );
    step(&mut world, &scene);
    let factor = 256 * (900 - 7 * 7) / 900;
    assert_eq!(
        world.spatial(id).unwrap().local.position[1],
        start_y + 32 * 256 - 32 * factor
    );
    assert_eq!(
        world.actors()[id.index].control_state().unwrap().fall_timer,
        14
    );
    // Water suppresses damage even beyond the fatal threshold; dry support still applies it.
    let y_map = i16::try_from(world.spatial(id).unwrap().local.position[1] >> 8).unwrap();
    scene.terrain = Some(
        cyrus_collision::ground::TerrainGround::new(
            vec![y_map + 3; 256 * 256],
            vec![false; 256 * 256],
            Some(vec![if water { 0 } else { 1 }; 256 * 256]),
        )
        .unwrap(),
    );
    scene.collision.texture_surfaces = Some(Default::default());
    assert_eq!(world.actor_surface(id).unwrap(), Surface::UNKNOWN);
    step(&mut world, &scene);
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(control.falling_state, JumpMotion::NONE);
    assert_eq!(
        world.actor_surface(id).unwrap(),
        if water {
            Surface::SCAPEDEEPWATER
        } else {
            Surface::UNKNOWN
        }
    );
    assert_eq!(
        world.actors()[id.index].attributes()[Attribute::Health],
        if water || !fatal_distance { 100 } else { 0 }
    );
    let deaths = world.take_death_events();
    assert_eq!(deaths.len(), usize::from(!water && fatal_distance));
    if let Some(death) = deaths.first() {
        assert_eq!(death.snapshot.actor, id);
        assert!(death.source_position.is_none());
    }
}

#[test]
fn a_support_miss_in_a_jump_group_carries_the_last_displacement_into_the_fall() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = cache();
    let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
    attributes[Attribute::Collide] = 4;
    attributes[Attribute::JumpTime] = 8;
    attributes[Attribute::JumpHeight] = 32;
    // The default group is the run jump group, which keeps the pending jump bits alive.
    attributes[Attribute::PanicGroup] = 41;
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
                "group_index":41,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]
            }),
        );
    let stable = [7 * 256, 5000, 9 * 256];
    let mut world = replace_health_test_animation(world, &scripts, &mut data, 0, 41, |saved| {
        saved["actors"][0]["control"]["directional_state"] = serde_json::json!([0, 0]);
        saved["actors"][0]["control"]["motion_direction"] = 2.into();
        saved["actors"][0]["control"]["stable_local_position"] = serde_json::json!(stable);
    });
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::WOOD).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let start = world.spatial(id).unwrap().local.position;
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
    let actor = &world.actors()[id.index];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    // Direction 2 at distance 4096 translates the actor forward by 16 map units before it falls.
    assert_ne!(world.spatial(id).unwrap().local.position[2], start[2]);
}
