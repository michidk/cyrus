use super::*;

pub(super) fn retained_point_fixture(
    group: u16,
    speed: u8,
    height: i32,
    edit: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let (mut world, scripts, mut data) = movement_fixture(
        group,
        false,
        true,
        |scripts| {
            for instance in &mut scripts.instances {
                instance.object_type = PlacementType::MODEL;
            }
            for attribute in [
                Attribute::ForwardSpeed,
                Attribute::BackwardSpeed,
                Attribute::SideSpeed,
                Attribute::ForwardWalk,
                Attribute::BackwardWalk,
                Attribute::FightForwardSpeed,
                Attribute::FightForwardWalk,
            ] {
                scripts.actors.get_mut("LOGIC").unwrap().attributes[attribute] = speed;
            }
        },
        |saved| {
            saved["actors"][0]["control"]["combat"] =
                serde_json::to_value(cyrus_script::script::CombatControlState {
                    retained_target_position: [0, 0, 10000],
                    ..Default::default()
                })
                .unwrap();
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([255, -257, -1]);
            edit(saved);
        },
    );
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    data.metadata["actors"][0]["mpsz_bounds_primary"] = 0.into();
    data.metadata["actors"][0]["raex"] = serde_json::json!({"unknown_12":256});
    data.metadata["mpsz_entries"] = serde_json::json!([{
        "total_extent":[24,height+12,24],"pos_extent":[12,height,12],
        "neg_extent":[12,12,12],"center_offset":[0,0,0],"flags":0
    }]);
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    world
        .prepare_ordinary_collision_shape(world.id(0).unwrap(), &data, &scene)
        .unwrap();
    (world, scripts, data)
}

#[test]
fn retained_point_arrival_uses_positive_y_extent_strict_speed_and_all_axis_quantization() {
    for (group, speed, height, distance, expected) in [
        (18, 3, 17, 3, true),
        (18, 3, 17, 4, false),
        (18, 3, 17, 5, false),
        (20, 1, 0, 0, true),
        (20, 1, 0, 1, false),
        (0, 255, 17, 0, false),
        (18, 0, 17, 0, false),
        (143, 255, 17, 381, true),
    ] {
        let (mut world, _, _) = retained_point_fixture(group, speed, height, |_| {});
        let id = world.id(0).unwrap();
        let point = [distance * 256 + 3, (height - 2) * 256 + 5, -249];
        let heading = world.spatial(id).unwrap().local.angles;
        assert_eq!(
            world.arrive_ordinary_point(id, point).unwrap(),
            expected,
            "group {group}, speed {speed}, height {height}, distance {distance}"
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if expected {
                [distance * 256, (height - 2) * 256, -256]
            } else {
                [255, -257, -1]
            }
        );
        assert_eq!(world.spatial(id).unwrap().local.angles, heading);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
    let (mut world, _, _) = retained_point_fixture(18, 3, 17, |saved| {
        saved["actors"][0]["animation"] = serde_json::Value::Null;
    });
    let id = world.id(0).unwrap();
    assert!(
        !world
            .arrive_ordinary_point(id, [3, 15 * 256, -256])
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [255, -257, -1]);
}

#[test]
fn retained_point_facing_ignores_live_target_and_preserves_y_on_horizontal_snap() {
    use cyrus_script::script::OrdinarySlopeInputs;
    let (mut world, id, target) = facing_world(400, [900, -3, 800], Some(18));
    let target_before = world.spatial(target).unwrap().local.position;
    assert_eq!(
        world
            .face_ordinary_point(
                id,
                [1024, -999999, 0],
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0
                },
                &mut |_, _| panic!("arrival requested sound")
            )
            .unwrap(),
        0
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [1024, 1280, 0]);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 400);
    assert_eq!(world.spatial(target).unwrap().local.position, target_before);
}

#[test]
fn disengaged_combat_uses_retained_point_and_real_forward_or_walk_movement() {
    use cyrus_script::script::{OrdinarySlopeInputs, ScriptHud, ScriptInputs};
    for (distance, position, group, direction) in [
        (500000, [0, 0, 2048], 18, MotionDirection::FORWARD),
        (500001, [0, 0, 1024], 20, MotionDirection::WALK_FORWARD),
    ] {
        let (mut world, _, _) = retained_point_fixture(group, 8, 12, |saved| {
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([2000000, 0, 0]);
            saved["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            saved["actors"][0]["control"]["movement_flags"] = 0x30.into();
            // The original lifecycle-seven/clear-collision gate admits WalkForward without a probe.
            saved["actors"][0]["control"]["death_selector"] = 7.into();
            saved["actors"][0]["attributes"][1] = 0.into();
            saved["actors"][0]["attributes"][12] = 4.into();
        });
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        world
            .move_disengaged_combat(
                id,
                distance,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 1,
                },
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, position);
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
            group
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_direction,
            direction
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn direct_point_steps_turn_then_walk_complete_on_arrival_and_restart_from_unknown_phases() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let hud = ScriptHud::default();
    let run = |phase: u8, destination: [i32; 3], falling: bool| {
        let (mut world, _, _) = retained_point_fixture(18, 8, 12, |saved| {
            saved["actors"][0]["control"]["point_command"] =
                serde_json::json!({"value":0,"word":0,"phase":phase,"counter":0});
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
            if falling {
                saved["actors"][0]["control"]["falling_state"] = 1.into();
            }
        });
        let id = world.id(0).unwrap();
        let result = world
            .move_ordinary_direct(
                id,
                destination,
                0,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                None,
                Some(&mut |_, _| Ok(())),
            )
            .unwrap();
        let control = world.actors()[0].control_state().unwrap();
        (
            result,
            world.globals().native_command().point_phase,
            control.ai_type.byte(),
        )
    };
    let far_ahead = [255, -257, 100_000];
    let here = [255, -257, -1];
    // Phase zero starts a turn; phase 22 finishes it and starts walking.
    assert_eq!(run(0, far_ahead, false), (1, PointPhase::DIRECT_TURN, 7));
    assert_eq!(run(22, far_ahead, false), (1, PointPhase::DIRECT_WALK, 7));
    // Arrival completes from phases that test it, and phase 21 completes unconditionally.
    assert_eq!(run(0, here, false).0, 0);
    assert_eq!(run(9, here, false).0, 0);
    assert_eq!(run(21, far_ahead, false), (0, PointPhase::DIRECT_ARRIVE, 0));
    // Unknown phases restart from zero; falling or queued motion interrupts any phase.
    assert_eq!(run(5, far_ahead, false), (1, PointPhase::START, 7));
    assert_eq!(run(11, far_ahead, true), (1, PointPhase::START, 7));
}

#[test]
fn disengaged_turn_bypasses_arrival_and_movement_services() {
    use cyrus_script::script::OrdinarySlopeInputs;
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |_| {},
        |saved| {
            saved["actors"][0]["control"]["combat"] =
                serde_json::to_value(cyrus_script::script::CombatControlState {
                    retained_target_position: [10000, 0, 0],
                    ..Default::default()
                })
                .unwrap();
        },
    );
    let id = world.id(0).unwrap();
    assert!(world.ordinary_collision_shape(id).is_err());
    let before = world.spatial(id).unwrap().local.position;
    world
        .move_disengaged_combat(
            id,
            500001,
            OrdinarySlopeInputs {
                selected_player: None,
                holding_weapon: false,
                combat_active: false,
                frame_scale: 256,
                ticks: 1,
            },
            Default::default(),
            &mut |_, _| panic!("turn emitted sound"),
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 2040);
    assert_eq!(
        world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
        AnimationGroup::RUN.index()
    );
    assert_eq!(
        world.inspect(0, 1)["actors"][0]["animation"]["pending_group"],
        AnimationGroup::TURN_RIGHT.index()
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn disengaged_arrival_is_lazy_and_preserves_facing_snap_on_missing_bounds() {
    use cyrus_script::script::OrdinarySlopeInputs;
    for prepared in [false, true] {
        let edit = |saved: &mut serde_json::Value| {
            saved["actors"][0]["control"]["combat"] =
                serde_json::to_value(cyrus_script::script::CombatControlState {
                    retained_target_position: [3, 2565, -249],
                    ..Default::default()
                })
                .unwrap();
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([255, -257, -1]);
        };
        let (mut world, _, _) = if prepared {
            retained_point_fixture(18, 8, 12, edit)
        } else {
            movement_fixture(18, false, true, |_| {}, edit)
        };
        let id = world.id(0).unwrap();
        let result = world.move_disengaged_combat(
            id,
            500001,
            OrdinarySlopeInputs {
                selected_player: None,
                holding_weapon: false,
                combat_active: false,
                frame_scale: 256,
                ticks: 1,
            },
            Default::default(),
            &mut |_, _| panic!("arrival emitted sound"),
        );
        if prepared {
            result.unwrap();
            assert_eq!(world.spatial(id).unwrap().local.position, [0, 2560, -256]);
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("world 7 instance 0 disengaged combat movement"),
                "{error}"
            );
            assert!(
                error.contains("ordinary collision shape not prepared"),
                "{error}"
            );
            assert_eq!(world.spatial(id).unwrap().local.position, [3, -257, -249]);
        }
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn direct_point_hanging_nonselected_receiver_retains_pending_without_services() {
    let (mut world, _, _) = retained_point_fixture(18, 8, 12, |saved| {
        saved["actors"][0]["control"]["living_state"] = 15.into();
        saved["actors"][0]["control"]["point_command"] =
            serde_json::json!({"value":0,"word":0,"phase":21,"counter":0});
    });
    let id = world.id(0).unwrap();
    let before = world.spatial(id).unwrap().local.position;
    for _ in 0..2 {
        assert_eq!(
            world
                .move_ordinary_direct(id, before, 0, Default::default(), None, None)
                .unwrap(),
            1
        );
        let control = world.actors()[id.index].control_state().unwrap();
        assert_eq!(
            (control.living_state.byte(), control.ai_type.byte()),
            (15, 7)
        );
        assert_eq!(
            world.globals().native_command().point_phase,
            PointPhase::START
        );
        assert_eq!(
            world.actors()[id.index].attributes()[cyrus_soup::Attribute::Collide],
            13
        );
        assert_eq!(world.spatial(id).unwrap().local.position, before);
    }
}

#[test]
fn direct_point_selected_hang_release_uses_receiver_collision_and_preserves_sound_failure() {
    use cyrus_script::script::{PlayerCollisionSource, ScriptInputs};
    for fail in [false, true] {
        let (mut world, data, scene) = selected_hang_classification_fixture();
        let player = world.selected_player().unwrap();
        let actors = [player, world.id(0).unwrap()];
        let before = world.spatial(player).unwrap().local.position;
        let mut sounds = 0;
        let result = world.move_ordinary_direct(
            player,
            before,
            0,
            ScriptInputs {
                player_collision: Some(PlayerCollisionSource {
                    scene: &scene,
                    animations: &data,
                    view: [10, 30],
                    actors: &actors,
                }),
                nominal_ticks: Some(0),
                fine_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            Some(&mut |_, _| {
                sounds += 1;
                anyhow::ensure!(!fail, "direct hang sound rejected");
                Ok(())
            }),
        );
        let control = world.actors()[player.index].control_state().unwrap();
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("direct hang sound rejected"));
            assert_eq!(control.living_state.byte(), 17);
        } else {
            assert_eq!(result.unwrap(), 1);
            assert_eq!(
                (
                    control.living_state.byte(),
                    control.action_transition_counter,
                    control.ai_type.byte()
                ),
                (0, 6, 7)
            );
            assert_eq!(
                world.globals().native_command().point_phase,
                PointPhase::START
            );
        }
        assert_eq!(sounds, 1);
        assert_eq!(world.spatial(player).unwrap().local.position, before);
    }
}

#[test]
fn direct_point_native_task_releases_hanging_player_then_resumes_turn_phase() {
    let (mut world, data, scene) = selected_hang_fixture_with_point_task(true);
    let player = world.selected_player().unwrap();
    let before = world.spatial(player).unwrap().local.position;
    advance_selected_hang_script(&mut world, &data, &scene, false).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.living_state.byte(),
            control.action_transition_counter,
            control.ai_type.byte()
        ),
        (0, 6, 7)
    );
    assert_eq!(
        world.globals().native_command().point_phase,
        PointPhase::START
    );
    assert_eq!(world.spatial(player).unwrap().local.position, before);
    advance_selected_hang_stage(&mut world, &data, &scene, false, ScriptStage::Tasks).unwrap();
    assert_eq!(
        world.globals().native_command().point_phase,
        PointPhase::DIRECT_TURN
    );
    assert_eq!(world.spatial(player).unwrap().local.position, before);
}
