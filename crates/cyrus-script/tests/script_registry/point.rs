use super::*;

fn point_wrapper_fixture(
    pause: u32,
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    use cyrus_script::script::NativeCommandState;
    let (_, _, mut data) = navigation_owner_fixture();
    let mut scripts = wait_cache(vec![
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 4 }],
        ),
        Operation::End { target: 0 },
    ]);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [20, -2, 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
    world.prepare_spatial().unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["native_commands"][0] = serde_json::to_value(NativeCommandState {
        continuation: 1234567,
        continuation_direction: 0,
        pause_ticks: pause,
        point_value: 987654,
        point_word: 321,
        point_phase: PointPhase::from_byte(99),
        point_counter: 88,
    })
    .unwrap();
    saved["actors"][0]["control"]["point_command"] =
        serde_json::json!({"value":1001,"word":50001,"phase":11,"counter":21});
    saved["actors"][1]["control"]["point_command"] =
        serde_json::json!({"value":1002,"word":50002,"phase":12,"counter":22});
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    world
        .advance(
            world.id(0).unwrap(),
            cyrus_script::script::ScriptStage::Tasks,
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            None,
            0,
        )
        .unwrap();
    (world, scripts, data)
}

#[test]
fn point_wrapper_retains_actor_fields_separately_and_consumes_shared_unsigned_pause() {
    use cyrus_script::script::{NativeCommandState, PointCommandState, ScriptInputs};
    let (world, _, _) = navigation_owner_fixture();
    assert_eq!(
        world.actors()[0].control_state().unwrap().point_command,
        PointCommandState {
            value: 0,
            word: 65535,
            phase: PointPhase::START,
            counter: 0
        }
    );
    let (mut world, scripts, data) = point_wrapper_fixture(7);
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(3),
        frame_scale: Some(256),
        ..Default::default()
    };
    assert_eq!(
        world
            .move_ordinary_point(first, [111, 222, 333], 0, inputs, None, None)
            .unwrap(),
        1
    );
    assert_eq!(
        *world.globals().native_command(),
        NativeCommandState {
            continuation: 1234567,
            continuation_direction: 0,
            pause_ticks: 4,
            point_value: 1001,
            point_word: 50001,
            point_phase: PointPhase::DIRECT_WALK,
            point_counter: 21,
        }
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    let restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    assert_eq!(
        restored.actors()[1].control_state().unwrap().point_command,
        PointCommandState {
            value: 1002,
            word: 50002,
            phase: PointPhase::LEAP_LINK,
            counter: 22
        }
    );
    assert_eq!(
        world
            .move_ordinary_point(second, [0; 3], 17, inputs, None, None)
            .unwrap(),
        1
    );
    assert_eq!(
        *world.globals().native_command(),
        NativeCommandState {
            continuation: 1234567,
            continuation_direction: 0,
            pause_ticks: 1,
            point_value: 1002,
            point_word: 50002,
            point_phase: PointPhase::LEAP_LINK,
            point_counter: 22,
        }
    );
    assert_eq!(
        world
            .move_ordinary_point(first, [0; 3], 0, inputs, None, None)
            .unwrap(),
        1
    );
    assert_eq!(world.globals().native_command().pause_ticks, 0);
    let error = world
        .move_ordinary_point(first, [0; 3], 0, inputs, None, None)
        .unwrap_err();
    assert!(format!("{error:#}").contains("point visibility geometry unavailable"));
    let (mut world, _, _) = point_wrapper_fixture(u32::MAX);
    let id = world.id(0).unwrap();
    assert_eq!(
        world
            .move_ordinary_point(id, [0; 3], 0, inputs, None, None)
            .unwrap(),
        1
    );
    assert_eq!(world.globals().native_command().pause_ticks, u32::MAX - 3);
}

#[test]
fn point_wrapper_validates_before_pause_and_restores_all_coordinates_above_horizontal_bound() {
    use cyrus_script::script::ScriptInputs;
    for (scale, distance, expected) in [
        (256, 8192, [9892, -1991, 3300]),
        (256, 8193, [1700, -2000, 3300]),
        (262144, 0, [1700, -2000, 3300]),
    ] {
        let (world, scripts, data) = point_wrapper_fixture(7);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["spatial"]["actors"][0]["local"]["position"] =
            serde_json::json!([1700 + distance, -1991, 3300]);
        saved["spatial"]["actors"][0]["previous_local_position"] =
            serde_json::json!([1700, -2000, 3300]);
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":null,"current":0,"next":1,"previous_next":0,"destination":1});
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        world
            .advance(
                world.id(0).unwrap(),
                cyrus_script::script::ScriptStage::Tasks,
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [0; 3],
                    0,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(scale),
                        ..Default::default()
                    },
                    None,
                    None
                )
                .unwrap(),
            1
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().navigation,
            Default::default()
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
    }
    let (mut world, _, _) = point_wrapper_fixture(7);
    let id = world.id(0).unwrap();
    world.set_walk_map_enabled(1, false).unwrap();
    let error = world
        .move_ordinary_point(id, [0; 3], 0, ScriptInputs::default(), None, None)
        .unwrap_err();
    assert!(format!("{error:#}").contains("point pause requires nominal_ticks"));
    assert_eq!(world.globals().native_command().pause_ticks, 7);
    assert_eq!(
        world.actors()[0].control_state().unwrap().navigation.map,
        None
    );
}

#[test]
fn point_entry_completes_horizontal_arrival_and_retains_pending_motion() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // The last column is the motion direction: 15 (a ledge update only the selected player has)
    // takes the same pending branch for any other actor.
    for (
        falling,
        queued,
        phase,
        counter,
        blocked,
        expected_phase,
        expected_counter,
        result,
        direction,
    ) in [
        (0, 0, 0, 0, false, 0, 0, 0, 0),
        (0, 0, 8, 8, true, 24, 9, 0, 0),
        (0, 0, 8, 255, true, 8, 0, 0, 0),
        (1, 0, 11, 8, true, 1, 9, 1, 0),
        (0, 1, 22, 255, true, 1, 0, 1, 0),
        (0, 0, 11, 8, true, 1, 9, 1, 15),
    ] {
        let (world, scripts, data) = point_wrapper_fixture(0);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        let control = &mut saved["actors"][0]["control"];
        control["point_command"] =
            serde_json::json!({"value":7654321,"word":65535,"phase":phase,"counter":counter});
        control["navigation"] =
            serde_json::json!({"map":1,"current":0,"next":1,"previous_next":0,"destination":1});
        control["death_selector"] = 7.into();
        control["falling_state"] = falling.into();
        control["queued_motion"] = queued.into();
        control["motion_direction"] = direction.into();
        control["collision_response_auxiliary"] =
            serde_json::json!([if blocked { 128 } else { 0 }, 0]);
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([100, 200, 300]);
        saved["spatial"]["actors"][0]["previous_local_position"] =
            serde_json::json!([100, 200, 300]);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Tasks,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [101, 999, 303],
                    17,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None
                )
                .unwrap(),
            result
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.point_command.phase, control.point_command.counter),
            (PointPhase::from_byte(expected_phase), expected_counter)
        );
        assert_eq!(
            (control.point_command.value, control.point_command.word),
            (7654321, 65535)
        );
        assert_eq!(control.ai_type.byte(), if result == 0 { 0 } else { 7 });
        assert_eq!(world.inspect(0, 1)["actors"][0]["attributes"][1], 13);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if result == 0 {
                [101, 200, 303]
            } else {
                [100, 200, 300]
            }
        );
        assert_eq!(
            (control.navigation.current, control.navigation.next),
            if result == 0 { (-1, -1) } else { (0, 1) }
        );
    }
}

#[test]
fn point_immediate_animation_uses_forced_default_and_live_guards() {
    use cyrus_script::script::ScriptInputs;
    for (enabled, excluded, current, expected) in [
        (false, false, 18, 18),
        (true, true, 18, 18),
        (true, false, 0, 0),
        (true, false, 18, 0),
    ] {
        let mut world = point_movement_fixture(
            current,
            false,
            enabled,
            |_| {},
            |saved| {
                saved["actors"][0]["control"]["object_update_excluded"] = excluded.into();
                saved["actors"][0]["control"]["point_command"]["phase"] = 0.into();
            },
        )
        .0;
        let id = world.id(0).unwrap();
        let destination = world.spatial(id).unwrap().local.position;
        let mut sounds = |_, _| Ok(());
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    destination,
                    0,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    Some(&mut sounds)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
            expected
        );
    }
}

#[test]
fn point_arrival_accepts_horizontal_equality_and_uses_current_or_fallback_speed() {
    use cyrus_script::script::ScriptInputs;
    for (group, distance, tolerance, arrived) in [
        (18, 2560, 0, true),
        (18, 2561, 0, false),
        (18, 2561, 1, true),
        (0, 1280, 0, true),
        (0, 1281, 0, false),
    ] {
        let mut world = point_movement_fixture(group, false, false, |_| {}, |_| {}).0;
        let id = world.id(0).unwrap();
        let mut destination = world.spatial(id).unwrap().local.position;
        destination[0] += distance;
        destination[1] += 100000;
        let result = world.move_ordinary_point(
            id,
            destination,
            tolerance,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
        );
        assert_eq!(
            result.is_ok(),
            arrived,
            "group {group} distance {distance} tolerance {tolerance}: {result:?}"
        );
        if arrived {
            assert_eq!(result.unwrap(), 0);
        }
    }
}

#[test]
fn point_entry_rejects_unavailable_branches_before_pending_motion() {
    use cyrus_script::script::ScriptInputs;
    let mut world = point_movement_fixture(
        0,
        false,
        false,
        |_| {},
        |saved| {
            saved["actors"][0]["control"]["body_response_flags"] = 4.into();
        },
    )
    .0;
    let id = world.id(0).unwrap();
    let position = world.spatial(id).unwrap().local.position;
    let message = format!(
        "{:#}",
        world
            .move_ordinary_point(
                id,
                position,
                0,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None
            )
            .unwrap_err()
    );
    assert!(
        message.contains("hit reaction requires the shared engine random stream"),
        "{message}"
    );
}

fn point_movement_fixture(
    initial_group: u16,
    suppressed: bool,
    animation_system: bool,
    edit_scripts: impl FnOnce(&mut Scripts),
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let (mut world, scripts, mut data) = movement_fixture(
        initial_group,
        suppressed,
        animation_system,
        edit_scripts,
        edit_saved,
    );
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    world.prepare_navigation(&data).unwrap();
    (world, scripts, data)
}

#[test]
fn point_default_animation_dispatches_sound_before_completion_and_propagates_failure() {
    use cyrus_script::script::{ScriptInputs, ScriptSound};
    use cyrus_world::animation::AnimationSound;
    for fail in [false, true] {
        let (_, scripts, mut data) = point_movement_fixture(18, false, true, |_| {}, |_| {});
        data.metadata["actors"][0]["animation_groups"][0]["commands"] = serde_json::json!([
            {"opcode":4,"sound_param":17,"volume_shift":0},
            {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.prepare_navigation(&data).unwrap();
        let id = world.id(0).unwrap();
        world
            .bind_animation(id, &data, AnimationGroup::PANIC)
            .unwrap();
        let position = world.spatial(id).unwrap().local.position;
        let mut cues = 0;
        let result=world.move_ordinary_point(id,position,0,ScriptInputs {nominal_ticks:Some(1),frame_scale:Some(256),..Default::default()}, None,Some(&mut |actor,sound| {
            assert_eq!(actor,id);
            assert!(matches!(sound,ScriptSound::Animation {sound:AnimationSound::Play {effect:17,..},position:Some(p),..} if p==position));
            cues+=1;
            if fail {anyhow::bail!("point sound rejected");}
            Ok(())
        }));
        assert_eq!(cues, 1);
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("point sound rejected"));
        } else {
            assert_eq!(result.unwrap(), 0);
        }
    }
}

fn node_assignment_fixture(
    identifier: u32,
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let (_, mut scripts, mut data) = navigation_owner_fixture();
    scripts.actor_pool_capacity = identifier - 4;
    scripts.content_id = scripts.content_identity().unwrap();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.prepare_spatial().unwrap();
    assert_eq!(
        world.actors()[0].runtime_identifier,
        Some(identifier as i16)
    );
    (world, scripts, data)
}

fn point_node_fixture(speed: u8) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let (mut world, scripts, mut data) = movement_fixture(
        18,
        false,
        false,
        |scripts| {
            scripts.actor_pool_capacity = 253;
            scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::ForwardSpeed] = speed;
        },
        |_| {},
    );
    data.metadata["walk_node_maps"] = serde_json::json!([{"node_count":2,"walk_nodes":[walk_node([0,-2,3]),walk_node([1,-2,3])]}]);
    world.prepare_navigation(&data).unwrap();
    assert_eq!(world.actors()[0].runtime_identifier, Some(257));
    (world, scripts, data)
}

#[test]
fn navigation_history_survives_validation_release_and_snapshot_replacement() {
    let (world, scripts, data) = node_assignment_fixture(257);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":1});
    for mode in ["absent", "disabled", "invalid", "release"] {
        let mut saved = saved.clone();
        if mode == "absent" {
            saved["actors"][0]["control"]["navigation"]["map"] = serde_json::Value::Null;
        }
        if mode == "invalid" {
            saved["actors"][0]["control"]["navigation"]["next"] = 2.into();
        }
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        if mode == "disabled" {
            world.set_walk_map_enabled(0, false).unwrap();
        }
        if mode == "release" {
            world.release_held_walk_nodes(id).unwrap();
        } else {
            assert!(!world.validate_walk_navigation(id).unwrap());
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .navigation
                .previous_next,
            99
        );
        let saved = movement_snapshot(&world, &scripts, &data);
        let restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
        assert_eq!(
            restored.actors()[0]
                .control_state()
                .unwrap()
                .navigation
                .previous_next,
            99
        );
    }
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"]
        .as_object_mut()
        .unwrap()
        .remove("previous_next");
    let restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    assert_eq!(
        restored.actors()[0]
            .control_state()
            .unwrap()
            .navigation
            .previous_next,
        0
    );
}

#[test]
fn node_assignment_releases_before_claim_and_only_current_updates_history() {
    use cyrus_script::script::WalkNodeSlot;
    for (slot, owners, requested, accepted, indices, previous, final_owners) in [
        (WalkNodeSlot::Current, [1, 0], 1, true, [1, -1], 1, [0, 1]),
        (WalkNodeSlot::Current, [1, 2], 1, false, [-1, -1], 1, [0, 2]),
        (WalkNodeSlot::Next, [1, 0], 1, true, [-1, 1], 99, [0, 1]),
        (WalkNodeSlot::Next, [1, 2], 1, false, [-1, -1], 99, [0, 2]),
        (WalkNodeSlot::Current, [1, 1], 2, false, [-1, -1], 1, [0, 0]),
    ] {
        let (world, scripts, data) = node_assignment_fixture(257);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":1});
        saved["walk_nodes"]["maps"][0] = serde_json::json!(owners);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(
            world.assign_walk_node(id, requested, slot).unwrap(),
            accepted
        );
        let nav = world.actors()[0].control_state().unwrap().navigation;
        assert_eq!([nav.current, nav.next], indices);
        assert_eq!(
            (nav.previous_next, nav.map, nav.destination),
            (previous, Some(0), 1)
        );
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["walk_nodes"]["maps"][0],
            serde_json::json!(final_owners)
        );
    }
}

#[test]
fn point_node_arrival_advances_even_when_claim_is_rejected_and_registers_once() {
    use cyrus_script::script::ScriptInputs;
    for (phase, next, foreign, speed, expected_phase) in [
        (1, 1, false, 8, 8),
        (27, 1, true, 8, 8),
        (1, -1, false, 8, 0),
        (27, -1, true, 8, 0),
        (1, 1, false, 255, 8),
    ] {
        let (world, scripts, data) = point_node_fixture(speed);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":next,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"] =
            serde_json::json!({"value":7654321,"word":65535,"phase":phase,"counter":0});
        saved["walk_nodes"]["maps"][0] = serde_json::json!([1, if foreign { 2 } else { 0 }]);
        let origin = [256 + (i32::from(speed) + 1) * 256, 200, 768];
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(origin);
        saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(origin);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 999, 1000000],
                    17,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None
                )
                .unwrap(),
            1
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.point_command.phase, control.ai_type.byte()),
            (PointPhase::from_byte(expected_phase), 7)
        );
        assert_eq!(
            (
                control.navigation.current,
                control.navigation.next,
                control.navigation.previous_next
            ),
            (
                if next != -1 && !foreign { 1 } else { -1 },
                -1,
                if next != -1 { 1 } else { 99 }
            )
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if next == -1 || speed == 255 {
                origin
            } else {
                [256, 200, 768]
            }
        );
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(saved["actors"][0]["attributes"][1], 12);
        assert_eq!(saved["map_updates"], serde_json::json!([0]));
        assert_eq!(
            saved["walk_nodes"]["maps"][0],
            serde_json::json!([0, if foreign { 2 } else { u8::from(next != -1) }])
        );
    }
}

#[test]
fn point_leap_without_animation_receiver_keeps_logical_start_and_wrapper_save() {
    use cyrus_script::script::{JumpMotion, ScriptHud, ScriptInputs};
    for (enabled, deep_water) in [(false, false), (true, false), (true, true)] {
        let (_, mut scripts, mut data) = point_node_fixture(8);
        scripts.updates.animation_system = enabled;
        scripts.content_id = scripts.content_identity().unwrap();
        data.metadata["script_content_id"] = scripts.content_id.clone().into();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_navigation(&data).unwrap();
        world.prepare_spatial().unwrap();
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"] =
            serde_json::json!({"value":7654321,"word":65535,"phase":12,"counter":0});
        saved["actors"][0]["surface"] = if deep_water { 2 } else { 0 }.into();
        saved["actors"][0]["control"]["stable_position_valid"] = true.into();
        saved["actors"][0]["control"]["fall_timer"] = 7.into();
        saved["actors"][0]["control"]["sliding_distance"] = 999.into();
        let origin = [256, 200, -24832];
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(origin);
        saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(origin);
        saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 999, 1000000],
                    0,
                    ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    Some(&mut |_, _| panic!("absent animation receiver emitted a cue"))
                )
                .unwrap(),
            1
        );
        let control = world.actors()[0].control_state().unwrap();
        if deep_water {
            assert_eq!(control.queued_motion, JumpMotion::NONE);
            assert_eq!(control.fall_timer, 7);
            assert_eq!(control.sliding_distance, 999);
            assert!(control.stable_position_valid);
        } else {
            assert_eq!(control.queued_motion, JumpMotion::FORWARD);
            assert_eq!(control.fall_origin_y, origin[1]);
            assert_eq!(control.fall_timer, 0);
            assert_eq!(control.sliding_distance, 0);
            assert!(!control.stable_position_valid);
        }
        assert_eq!(control.point_command.phase, PointPhase::LEAP_LINK);
        assert_eq!(world.spatial(id).unwrap().local.position, origin);
        let saved = movement_snapshot(&world, &scripts, &data);
        assert!(saved["actors"][0]["animation"].is_null());
        assert_eq!(saved["actors"][0]["attributes"][1], 12);
        assert_eq!(saved["map_updates"], serde_json::json!([0]));
        eprintln!(
            "static leap enabled={enabled} deep_water={deep_water}: queued={:?} timer={} sliding={} phase=12 attribute1=12 map=[0] cues=0",
            control.queued_motion, control.fall_timer, control.sliding_distance
        );
    }
}

#[test]
fn point_phase_twelve_claims_the_node_on_arrival_like_a_walked_link() {
    use cyrus_script::script::ScriptInputs;
    let (world, scripts, data) = point_node_fixture(8);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
    saved["actors"][0]["control"]["point_command"] =
        serde_json::json!({"value":7654321,"word":65535,"phase":12,"counter":0});
    saved["walk_nodes"]["maps"][0] = serde_json::json!([1, 0]);
    // The receiver stands on node one, so the arrival test passes before any leap starts.
    let origin = [256, 200, 768];
    saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(origin);
    saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(origin);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    world
        .move_ordinary_point(
            id,
            [1000000, 999, 1000000],
            17,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.point_command.phase, PointPhase::MONITOR_ROUTE);
    assert_eq!(
        (control.navigation.current, control.navigation.next),
        (1, -1)
    );
    assert_eq!(world.spatial(id).unwrap().local.position, origin);
}

#[test]
fn point_node_rejects_missing_weapon_owner_without_claiming_or_registering() {
    use cyrus_script::script::ScriptInputs;
    let (world, scripts, data) = point_node_fixture(8);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
    saved["actors"][0]["control"]["point_command"]["phase"] = 1.into();
    saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([2561, 200, 768]);
    saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!([2561, 200, 768]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    let result = world
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap_err();
    assert!(format!("{result:#}").contains("IsHoldingWeapon: inventory owner unavailable"));
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(saved["map_updates"], serde_json::json!([]));
    assert_eq!(
        saved["actors"][0]["control"]["navigation"],
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0})
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [2561, 200, 768]);
}

#[test]
fn point_node_animation_sound_failure_precedes_reservation_assignment() {
    use cyrus_script::script::ScriptInputs;
    for fail in [false, true] {
        let (_, mut scripts, mut data) = point_node_fixture(8);
        scripts.updates.animation_system = true;
        scripts.content_id = scripts.content_identity().unwrap();
        data.metadata["script_content_id"] = scripts.content_id.clone().into();
        data.metadata["actors"][0]["animation_groups"][0]["commands"] = serde_json::json!([
            {"opcode":4,"sound_param":17,"volume_shift":0},{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.prepare_navigation(&data).unwrap();
        let id = world.id(0).unwrap();
        world
            .bind_animation(id, &data, AnimationGroup::PANIC)
            .unwrap();
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"]["phase"] = 1.into();
        saved["walk_nodes"]["maps"][0] = serde_json::json!([1, 0]);
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([257, 200, 768]);
        saved["spatial"]["actors"][0]["previous_local_position"] =
            serde_json::json!([257, 200, 768]);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let mut cues = 0;
        let result = world.move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            Some(&mut |actor, sound| {
                assert_eq!(actor, id);
                assert!(matches!(
                    sound,
                    cyrus_script::script::ScriptSound::Animation {
                        sound: cyrus_world::animation::AnimationSound::Play { effect: 17, .. },
                        position: Some([256, 200, 768]),
                        ..
                    }
                ));
                cues += 1;
                if fail {
                    anyhow::bail!("node arrival sound rejected");
                }
                Ok(())
            }),
        );
        assert_eq!(cues, 1);
        let nav = world.actors()[0].control_state().unwrap().navigation;
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("node arrival sound rejected"));
            assert_eq!((nav.current, nav.next, nav.previous_next), (0, 1, 99));
        } else {
            assert_eq!(result.unwrap(), 1);
            assert_eq!((nav.current, nav.next, nav.previous_next), (1, -1, 1));
        }
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([0])
        );
    }
}

#[test]
fn point_phase_zero_claims_the_nearest_free_node_or_stays_in_phase_zero_without_a_map() {
    use cyrus_script::script::ScriptInputs;
    for (position, hit) in [([0, 0, 0], true), ([90_000_000, 0, 90_000_000], false)] {
        let (mut world, scripts, data) = free_node_fixture(
            serde_json::json!([{"node_count":2,"walk_nodes":[walk_node([0,0,0]),walk_node([5,0,0])]}]),
            0,
            |saved| {
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(position);
                saved["actors"][0]["control"]["point_command"]["phase"] = 0.into();
            },
        );
        let animation_before =
            movement_snapshot(&world, &scripts, &data)["actors"][0]["animation"].clone();
        let id = world.id(0).unwrap();
        let result = world
            .move_ordinary_point(
                id,
                [1000000, 0, 1000000],
                0,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                Some(&mut |_, _| Ok(())),
            )
            .unwrap();
        assert_eq!(result, 1, "the wrapper stays pending");
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["actors"][0]["animation"],
            animation_before,
            "route initialization preserves current and pending playback"
        );
        let control = world.actors()[0].control_state().unwrap();
        if hit {
            assert_eq!(control.navigation.map, Some(0));
            assert_eq!(control.navigation.next, 0);
            assert_eq!(control.point_command.phase, PointPhase::FACE_NEXT_NODE);
        } else {
            assert_eq!(control.navigation.map, None);
            assert_eq!(control.navigation.next, -1);
            assert_eq!(control.point_command.phase, PointPhase::START);
        }
    }
}

#[test]
fn point_node_facing_turns_then_selects_walk_or_shared_random_delay() {
    use cyrus_script::script::{CombatControlState, ScriptHud, ScriptInputs};
    for phase in [1, 27] {
        for (
            heading,
            weapon,
            flags,
            delay,
            seed,
            expected_heading,
            expected_phase,
            expected_word,
            expected_random,
        ) in [
            (64, false, 0, 1, 1, 56, phase, 65535, 1),
            (64, false, 4, 1, 1, 32, phase, 65535, 1),
            (64, true, 0, 1, 1, 32, phase, 65535, 1),
            (0, false, 0, 0, 1, 0, 2, 65535, 1),
            (0, false, 0, 1, 1, 0, 9, 142, 2524885223),
            (0, false, 0, 7, 1, 0, 2, 65535, 1103527590),
        ] {
            let (world, scripts, data) = point_node_fixture(8);
            let mut saved = movement_snapshot(&world, &scripts, &data);
            saved["actors"][0]["attributes"][15] = 8.into();
            saved["actors"][0]["attributes"][21] = 32.into();
            saved["actors"][0]["attributes"][90] = delay.into();
            saved["actors"][0]["attributes"][110] = u8::from(weapon).into();
            saved["actors"][0]["animation"] = serde_json::Value::Null;
            saved["actors"][0]["control"]["combat"] = serde_json::to_value(CombatControlState {
                flags: CombatFlags::from_bits(flags),
                ..Default::default()
            })
            .unwrap();
            saved["actors"][0]["control"]["navigation"] = serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
            saved["actors"][0]["control"]["point_command"] =
                serde_json::json!({"value":7654321,"word":65535,"phase":phase,"counter":0});
            let origin = [256, 200, -24832];
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(origin);
            saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(origin);
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, heading, 0]);
            let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
            let id = world.id(0).unwrap();
            let hud = ScriptHud::default();
            let mut random = cyrus_game::random::EngineRandom::from_state(seed);
            assert_eq!(
                world
                    .move_ordinary_point(
                        id,
                        [1000000, 0, 1000000],
                        0,
                        ScriptInputs {
                            hud: Some(&hud),
                            nominal_ticks: Some(1),
                            frame_scale: Some(256),
                            ..Default::default()
                        },
                        Some(&mut random),
                        Some(&mut |_, _| panic!("unbound animation emitted sound"))
                    )
                    .unwrap(),
                1
            );
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!(
                (
                    control.point_command.phase,
                    control.point_command.word,
                    control.ai_type.byte()
                ),
                (PointPhase::from_byte(expected_phase), expected_word, 7)
            );
            assert_eq!(
                (
                    control.navigation.current,
                    control.navigation.next,
                    control.navigation.previous_next
                ),
                (0, 1, 99)
            );
            assert_eq!(world.spatial(id).unwrap().local.position, origin);
            assert_eq!(world.spatial(id).unwrap().local.angles[1], expected_heading);
            assert_eq!(random.state(), expected_random);
            assert_eq!(
                movement_snapshot(&world, &scripts, &data)["map_updates"],
                serde_json::json!([0])
            );
        }
    }
}

#[test]
fn point_node_facing_sound_failure_precedes_heading_phase_and_random_changes() {
    use cyrus_script::script::ScriptInputs;
    let (_, mut scripts, mut data) = point_node_fixture(8);
    scripts.updates.animation_system = true;
    scripts.content_id = scripts.content_identity().unwrap();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let groups = data.metadata["actors"][0]["animation_groups"]
        .as_array_mut()
        .unwrap();
    let group = groups
        .iter_mut()
        .find(|group| group["group_index"] == AnimationGroup::FIGHT_TURN_RIGHT.index())
        .unwrap();
    group["commands"] = serde_json::json!([
        {"opcode":4,"sound_param":17,"volume_shift":0},{"opcode":0,"frame_reference":97},{"opcode":2,"value":0}
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_navigation(&data).unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, AnimationGroup::RUN)
        .unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["attributes"][110] = 1.into();
    saved["actors"][0]["attributes"][90] = 1.into();
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
    saved["actors"][0]["control"]["point_command"] =
        serde_json::json!({"value":7654321,"word":65535,"phase":27,"counter":0});
    let origin = [256, 200, -24832];
    saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(origin);
    saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(origin);
    saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 64, 0]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let mut cues = 0;
    let error = world
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut random),
            Some(&mut |actor, sound| {
                assert_eq!(actor, id);
                assert!(matches!(
                    sound,
                    cyrus_script::script::ScriptSound::Animation {
                        sound: cyrus_world::animation::AnimationSound::Play { effect: 17, .. },
                        position: Some([256, 200, -24832]),
                        ..
                    }
                ));
                cues += 1;
                anyhow::bail!("facing sound rejected")
            }),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("facing sound rejected"));
    assert_eq!(cues, 1);
    assert_eq!(random.state(), 42);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 64);
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (control.point_command.phase, control.point_command.word),
        (PointPhase::DETOUR, 65535)
    );
    assert_eq!(
        (
            control.navigation.current,
            control.navigation.next,
            control.navigation.previous_next
        ),
        (0, 1, 99)
    );
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["map_updates"],
        serde_json::json!([])
    );
}

#[test]
fn point_node_facing_requires_the_shared_random_owner_only_after_alignment() {
    use cyrus_script::script::ScriptInputs;
    let (world, scripts, data) = point_node_fixture(8);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["attributes"][110] = 1.into();
    saved["actors"][0]["attributes"][90] = 1.into();
    saved["actors"][0]["animation"] = serde_json::Value::Null;
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
    saved["actors"][0]["control"]["point_command"] =
        serde_json::json!({"value":7654321,"word":65535,"phase":1,"counter":0});
    let origin = [256, 200, -24832];
    saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(origin);
    saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(origin);
    saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    let error = world
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            Some(&mut |_, _| panic!("unbound animation emitted sound")),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("point facing requires shared engine random stream"));
    assert_eq!(
        world.globals().native_command().point_phase,
        PointPhase::WALK_TO_NEXT_NODE
    );
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (control.point_command.phase, control.point_command.word),
        (PointPhase::FACE_NEXT_NODE, 65535)
    );
    assert_eq!(
        (
            control.navigation.current,
            control.navigation.next,
            control.navigation.previous_next
        ),
        (0, 1, 99)
    );
    assert_eq!(world.spatial(id).unwrap().local.position, origin);
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["map_updates"],
        serde_json::json!([])
    );
}

fn point_delay_fixture(
    word: u16,
    weapon: bool,
    matching: bool,
    animation_system: bool,
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let (_, mut scripts, mut data) = point_node_fixture(8);
    scripts.updates.animation_system = animation_system;
    scripts.content_id = scripts.content_identity().unwrap();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    for group in 1..=3 {
        let definition = serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":4,"sound_param":100+group,"volume_shift":0},
            {"opcode":0,"frame_reference":group},{"opcode":2,"value":0}
        ]});
        data.metadata["actors"][0]["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(definition);
    }
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, AnimationGroup::RUN)
        .unwrap();
    world
        .advance(
            id,
            cyrus_script::script::ScriptStage::Animation,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["animation"]["frame"] = serde_json::Value::Null;
    saved["actors"][0]["attributes"][110] = u8::from(weapon).into();
    saved["actors"][0]["animation"]["playback"]["default"] = if matching { 18 } else { 0 }.into();
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
    saved["actors"][0]["control"]["point_command"] =
        serde_json::json!({"value":7654321,"word":word,"phase":9,"counter":0});
    saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([513, 200, 768]);
    saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!([513, 200, 768]);
    (
        restore_navigation_owner(saved, &scripts, &data).unwrap(),
        scripts,
        data,
    )
}

#[test]
fn point_delay_countdown_preserves_signed_word_comparison_and_weapon_exit() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for (word, ticks, weapon, expected_word, expected_phase) in [
        (0, 0, false, 0, 2),
        (1, 1, false, 0, 2),
        (16, 17, false, 0, 2),
        (16, 1, false, 15, 9),
        (16, 1, true, 15, 2),
        (32768, 65535, false, 32769, 9),
        (65535, 1, false, 65534, 9),
        (32768, u32::MAX, false, 0, 2),
        (65535, u32::MAX, false, 0, 2),
    ] {
        let (mut world, scripts, data) = point_delay_fixture(word, weapon, false, false);
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 999, 1000000],
                    17,
                    ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(ticks),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    Some(&mut random),
                    None
                )
                .unwrap(),
            1
        );
        let c = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                c.point_command.word,
                c.point_command.phase,
                c.ai_type.byte()
            ),
            (expected_word, PointPhase::from_byte(expected_phase), 7)
        );
        assert_eq!(
            (
                c.navigation.current,
                c.navigation.next,
                c.navigation.previous_next
            ),
            (0, 1, 99)
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [513, 200, 768]);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["actors"][0]["animation"]["playback"]["current"]
                [0],
            serde_json::json!(18)
        );
        assert_eq!(random.state(), 42);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([0])
        );
    }
}

#[test]
fn point_delay_idle_uses_shared_draws_rejects_three_and_dispatches_before_return() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for (seed, group, expected_state) in [
        (71, 1, 3472157085),
        (5, 2, 2701728395),
        (10, 3, 1849040536),
        (350, 1, 1391208277),
        (284, 2, 847347883),
        (416, 3, 1935068671),
    ] {
        let (mut world, scripts, data) = point_delay_fixture(16, false, true, true);
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(seed);
        let mut effects = vec![];
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 0, 1000000],
                    0,
                    ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    Some(&mut random),
                    Some(&mut |actor, sound| {
                        assert_eq!(actor, id);
                        if let cyrus_script::script::ScriptSound::Animation {
                            sound: cyrus_world::animation::AnimationSound::Play { effect, .. },
                            position,
                            ..
                        } = sound
                        {
                            assert_eq!(position, Some([513, 200, 768]));
                            effects.push(effect);
                        }
                        Ok(())
                    })
                )
                .unwrap(),
            1
        );
        assert_eq!(effects, vec![100 + group as i16]);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["actors"][0]["animation"]["playback"]["current"]
                [0],
            serde_json::json!(group)
        );
        assert_eq!(random.state(), expected_state);
        let c = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                c.point_command.word,
                c.point_command.phase,
                c.ai_type.byte()
            ),
            (15, PointPhase::IDLE_PAUSE, 7)
        );
        assert_eq!(
            (
                c.navigation.current,
                c.navigation.next,
                c.navigation.previous_next
            ),
            (0, 1, 99)
        );
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(saved["actors"][0]["animation"]["playback"]["default"], 18);
        assert_eq!(saved["map_updates"], serde_json::json!([0]));
        world
            .move_ordinary_point(
                id,
                [1000000, 0, 1000000],
                0,
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .point_command
                .word,
            14
        );
        assert_eq!(random.state(), expected_state);
    }
}

#[test]
fn point_delay_disabled_animation_retains_group_without_idle_sound_and_continues() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = point_delay_fixture(16, false, true, false);
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(350);
    assert_eq!(
        world
            .move_ordinary_point(
                id,
                [1000000, 0, 1000000],
                0,
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut random),
                Some(&mut |_, _| panic!("disabled animation dispatched idle sound")),
            )
            .unwrap(),
        1
    );
    assert_eq!(random.state(), 1391208277);
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(control.point_command.word, 15);
    assert_eq!(control.point_command.phase, PointPhase::IDLE_PAUSE);
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][0]["animation"]["playback"]["current"][0],
        18
    );
    assert_eq!(saved["map_updates"], serde_json::json!([0]));
}

#[test]
fn point_delay_excluded_receiver_retains_animation_while_command_continues() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for excluded in [true, false] {
        let (world, scripts, data) = point_delay_fixture(16, false, true, true);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["control"]["object_update_excluded"] = excluded.into();
        let before_animation = saved["actors"][0]["animation"].clone();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(350);
        let mut effects = vec![];
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 0, 1000000],
                    0,
                    ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    Some(&mut random),
                    Some(&mut |actor, sound| {
                        assert_eq!(actor, id);
                        if let cyrus_script::script::ScriptSound::Animation {
                            sound: cyrus_world::animation::AnimationSound::Play { effect, .. },
                            ..
                        } = sound
                        {
                            effects.push(effect);
                        }
                        Ok(())
                    }),
                )
                .unwrap(),
            1
        );
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            saved["actors"][0]["animation"]["playback"]["current"][0],
            if excluded { 18 } else { 1 },
            "excluded={excluded}: ordinary request must retain excluded receiver ownership"
        );
        assert_eq!(effects, if excluded { vec![] } else { vec![101] });
        if excluded {
            assert_eq!(saved["actors"][0]["animation"], before_animation);
        }
        assert_eq!(random.state(), 1391208277);
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.point_command.word, 15);
        assert_eq!(control.point_command.phase, PointPhase::IDLE_PAUSE);
        assert_eq!(world.spatial(id).unwrap().local.position, [513, 200, 768]);
        assert_eq!(saved["map_updates"], serde_json::json!([0]));
        eprintln!(
            "point idle excluded={excluded}: group={}, cues={effects:?}, word={}, random={}",
            saved["actors"][0]["animation"]["playback"]["current"][0],
            control.point_command.word,
            random.state()
        );
    }
}

#[test]
fn point_delay_sound_failure_retains_consumed_random_and_stops_before_wrapper_save() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = point_delay_fixture(16, false, true, true);
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(350);
    let mut effects = vec![];
    let error = world
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut random),
            Some(&mut |_, sound| {
                if let cyrus_script::script::ScriptSound::Animation {
                    sound: cyrus_world::animation::AnimationSound::Play { effect, .. },
                    ..
                } = sound
                {
                    effects.push(effect);
                    anyhow::bail!("idle sound rejected");
                }
                Ok(())
            }),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("idle sound rejected"));
    assert_eq!(effects, vec![101]);
    assert_eq!(random.state(), 1391208277);
    assert_eq!(world.globals().native_command().point_word, 15);
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            c.point_command.word,
            c.point_command.phase,
            c.ai_type.byte()
        ),
        (16, PointPhase::IDLE_PAUSE, 0)
    );
    assert_eq!(
        (
            c.navigation.current,
            c.navigation.next,
            c.navigation.previous_next
        ),
        (0, 1, 99)
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [513, 200, 768]);
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(saved["map_updates"], serde_json::json!([]));
    assert!(
        saved["actors"][0]["animation_failure"]
            .as_str()
            .unwrap()
            .contains("idle sound rejected")
    );
}

#[test]
fn point_delay_snapshot_resumes_countdown_and_zero_exit_needs_no_optional_owners() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = point_delay_fixture(2, false, true, false);
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    world
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut random),
            None,
        )
        .unwrap();
    assert_eq!(random.state(), 1103527590);
    let saved = movement_snapshot(&world, &scripts, &data);
    let mut restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = restored.id(0).unwrap();
    assert_eq!(
        restored.actors()[0]
            .control_state()
            .unwrap()
            .point_command
            .word,
        1
    );
    restored
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    let c = restored.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            c.point_command.word,
            c.point_command.phase,
            c.ai_type.byte()
        ),
        (0, PointPhase::WALK_TO_NEXT_NODE, 7)
    );
    assert_eq!(
        (
            c.navigation.current,
            c.navigation.next,
            c.navigation.previous_next
        ),
        (0, 1, 99)
    );
    assert_eq!(
        movement_snapshot(&restored, &scripts, &data)["map_updates"],
        serde_json::json!([0])
    );
}

#[test]
fn point_delay_missing_random_or_sound_owner_preserves_failure_order() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for missing_random in [false, true] {
        let (mut world, scripts, data) = point_delay_fixture(16, false, true, true);
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(71);
        let error = world
            .move_ordinary_point(
                id,
                [1000000, 0, 1000000],
                0,
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                if missing_random {
                    None
                } else {
                    Some(&mut random)
                },
                None,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(if missing_random {
            "point delay requires shared engine random stream"
        } else {
            "point delay requires synchronous sounds"
        }));
        assert_eq!(random.state(), if missing_random { 71 } else { 3472157085 });
        assert_eq!(world.globals().native_command().point_word, 15);
        let c = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                c.point_command.word,
                c.point_command.phase,
                c.ai_type.byte()
            ),
            (16, PointPhase::IDLE_PAUSE, 0)
        );
        assert_eq!(
            (
                c.navigation.current,
                c.navigation.next,
                c.navigation.previous_next
            ),
            (0, 1, 99)
        );
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn coordinate_walking_restores_only_overshoot_owned_state_at_inclusive_boundaries() {
    use cyrus_script::script::{OrdinaryMovement, ScriptHud, ScriptInputs};
    for (heading, snapped) in [
        (0, false),
        (255, false),
        (256, true),
        (512, true),
        (1792, true),
        (1793, false),
        (2047, false),
    ] {
        let (mut world, scripts, data) = movement_fixture(
            30,
            false,
            false,
            |scripts| {
                scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::NodeRun] = 1;
            },
            |saved| {
                saved["actors"][0]["control"]["movement_flags"] = 0x85.into();
                saved["spatial"]["actors"][0]["local"]["position"] =
                    serde_json::json!([100, 200, 300]);
                saved["spatial"]["actors"][0]["local"]["angles"][1] = heading.into();
                saved["spatial"]["actors"][0]["linked_position"] = serde_json::json!([0, 0, 0]);
            },
        );
        let id = world.id(0).unwrap();
        let before = movement_snapshot(&world, &scripts, &data);
        let mut moved = restore_navigation_owner(before.clone(), &scripts, &data).unwrap();
        let moved_id = moved.id(0).unwrap();
        let hud = ScriptHud::default();
        let inputs = ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            hud: Some(&hud),
            shell_execution: Some(false),
            forward_step_clear: Some(true),
            ..Default::default()
        };
        moved
            .move_ordinary(
                moved_id,
                OrdinaryMovement::Forward,
                1,
                0,
                inputs,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        let result = world
            .walk_ordinary_point(
                id,
                [0, -999, 20000],
                0,
                inputs,
                Some(&mut |_, _| panic!("fixture has no authored sound")),
            )
            .unwrap();
        assert_eq!(result, i32::from(!snapped), "heading {heading}");
        let after = movement_snapshot(&world, &scripts, &data);
        if snapped {
            assert_eq!(world.spatial(id).unwrap().local.position, [0, 200, 20000]);
            assert_eq!(
                world.actors()[0].control_state().unwrap().move_counter,
                0x85
            );
            assert_eq!(
                after["actors"][0]["animation"],
                before["actors"][0]["animation"]
            );
            assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
        } else {
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                moved.spatial(moved_id).unwrap().local.position
            );
            assert_eq!(
                after["actors"][0]["animation"],
                movement_snapshot(&moved, &scripts, &data)["actors"][0]["animation"]
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().move_counter,
                0x95
            );
            assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
        }
        assert_eq!(
            world.actors()[0].control_state().unwrap().motion_direction,
            MotionDirection::FORWARD
        );
        assert_eq!(after["map_updates"], serde_json::json!([]));
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn coordinate_walking_initial_arrival_needs_no_movement_or_sound_owner() {
    use cyrus_script::script::ScriptInputs;
    for (destination, tolerance) in [([0, i32::MAX, 2560], 0), ([0, i32::MIN, 2577], 17)] {
        let (mut world, scripts, data) = movement_fixture(18, false, false, |_| {}, |_| {});
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .walk_ordinary_point(
                    id,
                    destination,
                    tolerance,
                    ScriptInputs {
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None
                )
                .unwrap(),
            0
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [0, 0, destination[2]]
        );
        assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 0);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn coordinate_walking_reports_live_walk_admission_instead_of_using_run() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    // Collision attribute bits make the step check need live geometry.
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 1;
        },
        |_| {},
    );
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let error = world
        .walk_ordinary_point(
            id,
            [0, 0, 20000],
            0,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                hud: Some(&hud),
                shell_execution: Some(false),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
        )
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("world 7 actor 0 coordinate walking"),
        "{error}"
    );
    assert!(error.contains(Native::WalkForward.name()), "{error}");
    assert!(
        error.contains("forward step check input unavailable"),
        "{error}"
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
}

#[test]
fn point_walking_continues_then_claims_or_releases_nodes_with_one_registration() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for (next, foreign, heading, distance, phase, z) in [
        (-1, false, 0, 4096, 0, -3328),
        (1, false, 0, 4096, 2, -2816),
        (1, false, 1024, 4096, 8, 768),
        (1, false, 0, 2560, 8, 768),
        (1, true, 0, 2560, 8, 768),
    ] {
        let (world, scripts, data) = point_node_fixture(8);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["attributes"][93] = 1.into();
        saved["actors"][0]["animation"]["playback"]["current"][1]["exit_reached"] = true.into();
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":next,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"] =
            serde_json::json!({"value":7654321,"word":1234,"phase":2,"counter":0});
        saved["walk_nodes"]["maps"][0] = serde_json::json!([1, if foreign { 2 } else { 0 }]);
        let position = [256, 200, 768 - distance];
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!(position);
        saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!(position);
        saved["spatial"]["actors"][0]["local"]["angles"][1] = heading.into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 0, 1000000],
                    0,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        forward_step_clear: Some(true),
                        ..Default::default()
                    },
                    None,
                    Some(&mut |_, _| panic!("fixture has no authored sound"))
                )
                .unwrap(),
            1
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.point_command.phase,
                control.point_command.word,
                control.ai_type.byte()
            ),
            (PointPhase::from_byte(phase), 1234, 7)
        );
        assert_eq!(
            (
                control.navigation.current,
                control.navigation.next,
                control.navigation.previous_next
            ),
            match phase {
                0 => (-1, -1, 99),
                2 => (0, 1, 99),
                _ => (if foreign { -1 } else { 1 }, -1, 1),
            }
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [256, 200, z]);
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(after["map_updates"], serde_json::json!([0]));
        assert_eq!(after["actors"][0]["attributes"][1], 12);
    }
}

#[test]
fn point_walking_faces_only_before_the_exit_marker_and_ignores_pending_turn_result() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for (exited, heading, distance, phase, movement_flags) in [
        (false, 56, 4096, 2, 0x10),
        (true, 64, 4096, 2, 0x10),
        (false, 56, 128, 8, 0),
        (true, 64, 128, 8, 0),
    ] {
        let (world, scripts, data) = point_node_fixture(8);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["attributes"][15] = 8.into();
        saved["actors"][0]["attributes"][93] = 1.into();
        saved["actors"][0]["attributes"][110] = 0.into();
        saved["actors"][0]["animation"]["playback"]["current"][1]["exit_reached"] = exited.into();
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"]["phase"] = 2.into();
        saved["spatial"]["actors"][0]["local"]["position"] =
            serde_json::json!([256, 200, 768 - distance]);
        saved["spatial"]["actors"][0]["previous_local_position"] =
            serde_json::json!([256, 200, 768 - distance]);
        saved["spatial"]["actors"][0]["local"]["angles"][1] = 64.into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        world
            .move_ordinary_point(
                id,
                [1000000, 0, 1000000],
                0,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    forward_step_clear: Some(true),
                    ..Default::default()
                },
                None,
                Some(&mut |_, _| Ok(())),
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.angles[1], heading);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .point_command
                .phase,
            PointPhase::from_byte(phase)
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().move_counter,
            movement_flags
        );
        assert!(world.spatial(id).unwrap().local.position[2] > 768 - distance);
    }
}

#[test]
fn coordinate_walking_reads_local_bearing_after_movement() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        false,
        |scripts| {
            scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::NodeRun] = 1;
        },
        |saved| {
            saved["spatial"]["actors"][0]["local"]["angles"][1] = 255.into();
        },
    );
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    assert_eq!(
        world
            .walk_ordinary_point(
                id,
                [0, 999, 2561],
                0,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    forward_step_clear: Some(true),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(()))
            )
            .unwrap(),
        0
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 2561]);
    assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 0);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 255);
}

#[test]
fn point_walking_failed_post_snap_arrival_retains_held_nodes_and_history() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let (world, scripts, data) = point_node_fixture(8);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["attributes"][93] = 1.into();
    saved["actors"][0]["animation"]["playback"]["current"][1]["exit_reached"] = true.into();
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
    saved["actors"][0]["control"]["point_command"]["phase"] = 2.into();
    saved["walk_nodes"]["maps"][0] = serde_json::json!([1, 0]);
    saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([256, 200, -1792]);
    saved["spatial"]["actors"][0]["previous_local_position"] = serde_json::json!([256, 200, -1792]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    world
        .move_ordinary_point(
            id,
            [1000000, 0, 1000000],
            0,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(1 << 20),
                hud: Some(&hud),
                shell_execution: Some(false),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            None,
            Some(&mut |_, _| Ok(())),
        )
        .unwrap();
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            control.point_command.phase,
            control.navigation.current,
            control.navigation.next,
            control.navigation.previous_next
        ),
        (PointPhase::START, 0, 1, 99)
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [256, 200, -1792]);
    let after = movement_snapshot(&world, &scripts, &data);
    assert_eq!(after["walk_nodes"]["maps"][0], serde_json::json!([1, 0]));
    assert_eq!(after["map_updates"], serde_json::json!([0]));
}

#[test]
fn point_destination_search_stays_on_current_map_and_ignores_reservations() {
    let (world, scripts, data) = free_node_fixture(
        serde_json::json!([
            {"node_count":3,"walk_nodes":[walk_node([12,-2,3]),walk_node([11,-2,3]),walk_node([0,-2,3])]},
            {"node_count":1,"walk_nodes":[walk_node([0,-2,3])]}
        ]),
        0,
        |_| {},
    );
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"]["map"] = 0.into();
    saved["walk_nodes"]["maps"][0] = serde_json::json!([2, 2, 2]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    assert_eq!(
        world.nearest_current_walk_node(id, [0, -257, 768]).unwrap(),
        Some(1)
    );
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["walk_nodes"]["selected_map"],
        0
    );
    world.set_walk_map_enabled(0, false).unwrap();
    assert_eq!(
        world.nearest_current_walk_node(id, [0, -257, 768]).unwrap(),
        None
    );
    assert!(movement_snapshot(&world, &scripts, &data)["walk_nodes"]["selected_map"].is_null());
}

#[test]
fn point_route_completion_releases_claims_and_retains_phase_and_history() {
    use cyrus_script::script::ScriptInputs;
    for (current, link, next_owner, expected_result, expected_phase, completion) in [
        (-1, false, 0, 1, 0, None),
        (0, false, 0, 0, 8, Some(3)),
        (0, false, 0, 0, 8, Some(2)),
        (0, true, 0, 1, 1, None),
        (0, true, 0, 1, 12, None),
        (0, true, 2, 0, 8, Some(7)),
        (1, false, 0, 1, 21, None),
    ] {
        let (_, scripts, mut data) = point_node_fixture(8);
        data.metadata["walk_node_maps"][0]["walk_nodes"][0] = walk_node([100, 0, 100]);
        if link {
            data.metadata["walk_node_maps"][0]["walk_nodes"][0]["route_count"] = 1.into();
            data.metadata["walk_node_maps"][0]["walk_nodes"][0]["routes"] =
                serde_json::json!([{"target_node_id":1,"cost":1}]);
            if expected_phase == 12 {
                data.metadata["walk_node_maps"][0]["walk_nodes"][0]["route_count"] = 2.into();
                data.metadata["walk_node_maps"][0]["walk_nodes"][0]["routes"] = serde_json::json!([{"target_node_id":1,"cost":16384},{"target_node_id":1,"cost":1}]);
            }
        }
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.prepare_navigation(&data).unwrap();
        world
            .bind_animation(world.id(0).unwrap(), &data, AnimationGroup::RUN)
            .unwrap();
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["attributes"][95] = 1.into();
        saved["actors"][0]["control"]["death_selector"] = 7.into();
        saved["actors"][0]["control"]["navigation"] = serde_json::json!({"map":0,"current":current,"next":1,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"]["phase"] = 8.into();
        saved["walk_nodes"]["maps"][0] = serde_json::json!([1, next_owner]);
        for field in ["local", "previous_local_position"] {
            if field == "local" {
                saved["spatial"]["actors"][0][field]["position"] =
                    serde_json::json!([1000000, 200, 1000000]);
            } else {
                saved["spatial"]["actors"][0][field] = serde_json::json!([1000000, 200, 1000000]);
            }
        }
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    if completion == Some(2) {
                        [2000000, 0, 2000000]
                    } else {
                        [256, -512, 768]
                    },
                    17,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None
                )
                .unwrap(),
            expected_result
        );
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            after["actors"][0]["control"]["point_command"]["phase"],
            expected_phase
        );
        assert_eq!(
            after["actors"][0]["control"]["navigation"]["previous_next"],
            99
        );
        assert_eq!(
            after["actors"][0]["attributes"][1],
            if completion.is_some() { 13 } else { 12 }
        );
        assert_eq!(
            after["actors"][0]["control"]["death_selector"],
            if completion.is_some() { 0 } else { 7 }
        );
        if let Some(result) = completion {
            assert_eq!(
                world.inspect(0, 1)["globals"]["native_command_return"],
                result
            );
        }
    }
}

#[test]
fn point_and_ai_destination_queries_preserve_their_polygon_sidedness() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::OrdinaryObstacleQuery;
    for (wall, facing, distance, point_visible, ai_clear) in [
        (false, true, 131072, true, true),
        (false, true, 131073, false, false),
        (true, true, 131072, false, false),
        (true, false, 131072, true, false),
        (true, false, 131073, false, false),
    ] {
        let (mut world, data) = overlap_response_fixture(128, 0, 1024);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let from = world.spatial(ids[0]).unwrap().local.position;
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        let mut model = ordinary_forward_wall_model();
        if !facing {
            model.geometry.normals[0][0] = 1.0;
            model.geometry.faces[0].reverse();
        }
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: wall
                .then(|| StaticPlacement {
                    id: 1,
                    model: "WALL".into(),
                    position: [(from[0] >> 8) + 504, from[1] >> 8, from[2] >> 8],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .into_iter()
                .collect(),
            models: BTreeMap::from([("WALL".into(), model)]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
            .unwrap();
        let query = OrdinaryObstacleQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            vertical_offset: 0,
            respond: false,
        };
        let before = world.inspect(0, ids.len());
        assert_eq!(
            world
                .point_direct_visible(
                    ids[0],
                    ids[1],
                    [from[0] + distance, from[1], from[2]],
                    query
                )
                .unwrap(),
            point_visible,
            "wall {wall}, facing {facing}, distance {distance}"
        );
        assert_eq!(
            world
                .ai_destination_clear(
                    ids[0],
                    ids[1],
                    [from[0] + distance, from[1], from[2]],
                    query
                )
                .unwrap(),
            ai_clear,
            "AI wall {wall}, facing {facing}, distance {distance}"
        );
        assert_eq!(world.inspect(0, ids.len()), before);
    }
}

#[test]
fn point_direct_attribute_gate_releases_before_pending_and_default_completion() {
    use cyrus_script::script::ScriptInputs;
    for (phase, expected_result, expected_phase) in [(11, 1, 21), (22, 1, 21), (21, 0, 21)] {
        let (world, scripts, data) = point_node_fixture(8);
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["attributes"][95] = 1.into();
        saved["actors"][0]["control"]["object_update_excluded"] = true.into();
        saved["actors"][0]["control"]["death_selector"] = 7.into();
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":1,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["point_command"]["phase"] = phase.into();
        saved["walk_nodes"]["maps"][0] = serde_json::json!([1, 1]);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    [1000000, 0, 1000000],
                    0,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None
                )
                .unwrap(),
            expected_result
        );
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            after["actors"][0]["control"]["point_command"]["phase"],
            expected_phase
        );
        assert_eq!(
            after["actors"][0]["attributes"][1],
            if expected_result == 0 { 13 } else { 12 }
        );
        assert_eq!(after["actors"][0]["control"]["navigation"]["current"], -1);
        assert_eq!(after["actors"][0]["control"]["navigation"]["next"], -1);
        assert_eq!(
            after["actors"][0]["control"]["navigation"]["previous_next"],
            99
        );
        assert_eq!(after["walk_nodes"]["maps"][0], serde_json::json!([0, 0]));
    }
}

#[test]
fn point_direct_visible_completion_and_rejection_use_live_geometry_owner() {
    use cyrus_script::script::{LocomotionGeometry, OrdinaryGroundQuery, ScriptHud, ScriptInputs};
    for (phase, far, tolerance, expected_result, expected_phase, expected_attribute, sliding) in [
        (8, false, 0, 1, 22, 12, false),
        (11, true, 0, 1, 0, 12, false),
        (11, false, 100000, 0, 11, 13, false),
        (22, false, 0, 1, 11, 12, false),
        (11, false, 0, 1, 11, 13, false),
        (11, false, 0, 1, 0, 13, true),
    ] {
        let (_, mut scripts, point_data) = point_node_fixture(8);
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let (mut world, mut data, models) = static_model_fixture(scripts.clone());
        data.metadata["actors"][0]["animation_groups"] =
            point_data.metadata["actors"][0]["animation_groups"].clone();
        data.metadata["walk_node_maps"] = point_data.metadata["walk_node_maps"].clone();
        world.prepare_navigation(&data).unwrap();
        world
            .bind_animation(world.id(0).unwrap(), &data, AnimationGroup::RUN)
            .unwrap();
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["attributes"][93] = 1.into();
        saved["actors"][0]["control"]["sliding_state"] = u8::from(sliding).into();
        saved["actors"][0]["control"]["point_command"]["phase"] = phase.into();
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":-1,"previous_next":99,"destination":0});
        saved["actors"][0]["control"]["object_update_excluded"] = true.into();
        // Keep facing already aligned toward the destination while arrival stays false.
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([256, 0, -20000]);
        saved["spatial"]["actors"][0]["previous_local_position"] =
            serde_json::json!([256, 0, -20000]);
        saved["spatial"]["actors"][0]["local"]["angles"][1] = 0.into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [0, 0], &[])
            .unwrap();
        let hud = ScriptHud::default();
        let destination = if far {
            [256, 0, 200000]
        } else {
            [256, -512, 768]
        };
        let inputs = ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            hud: Some(&hud),
            shell_execution: Some(false),
            locomotion_geometry: Some(LocomotionGeometry {
                ground: OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 1,
                    sidedness: false,
                },
                environment: cyrus_script::script::ObstacleEnvironment {
                    selected_player: Some(id),
                    auto_grab: false,
                    item_system_ready: false,
                    shell_execution: false,
                    ticks: 1,
                },
            }),
            ..Default::default()
        };
        assert_eq!(
            world
                .move_ordinary_point(
                    id,
                    destination,
                    tolerance,
                    inputs,
                    None,
                    Some(&mut |_, _| Ok(()))
                )
                .unwrap(),
            expected_result,
            "phase {phase}"
        );
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            after["actors"][0]["control"]["point_command"]["phase"],
            expected_phase
        );
        assert_eq!(after["actors"][0]["attributes"][1], expected_attribute);
        assert_eq!(after["actors"][0]["control"]["navigation"]["current"], -1);
    }
}

#[test]
fn point_destination_cutoff_is_exclusive_and_a_miss_clears_selected_map() {
    let (world, scripts, data) = free_node_fixture(
        serde_json::json!([
            {"node_count":1,"walk_nodes":[walk_node([4096,0,0])]}
        ]),
        0,
        |_| {},
    );
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"]["map"] = 0.into();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    assert_eq!(
        world.nearest_current_walk_node(id, [256, 0, 0]).unwrap(),
        Some(0)
    );
    assert_eq!(
        world.nearest_current_walk_node(id, [0, 0, 0]).unwrap(),
        None
    );
    assert!(movement_snapshot(&world, &scripts, &data)["walk_nodes"]["selected_map"].is_null());
}

fn node_point_ledge_fixture(
    player_type: u8,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with_animation(
        |scripts| {
            let function = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: Native::MoveNodePosition,
                param_count: 4,
                kind: CallKind::Task,
            });
            let actor = scripts.actors.get_mut("CYRUS").unwrap();
            actor.attributes[Attribute::PlayerType] = player_type;
            actor.program.steps = BTreeMap::from([
                (
                    0,
                    Step {
                        next: 1,
                        operation: wait(
                            function,
                            Native::MoveNodePosition,
                            CallKind::Task,
                            [2560, -1536, 7680, 0]
                                .map(|value| Value::Integer { value })
                                .to_vec(),
                        ),
                    },
                ),
                (
                    1,
                    Step {
                        next: 2,
                        operation: Operation::End { target: 0 },
                    },
                ),
            ]);
        },
        |data| {
            let actor = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap();
            actor["animation_groups"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "group_index":56,"anim_id":1,"anim_type":0,"commands":[
                        {"opcode":4,"sound_param":9,"volume_shift":0},
                        {"opcode":0,"frame_reference":0},{"opcode":1}]
                }));
        },
    );
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let player = world.selected_player().unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    let control = &mut saved["actors"][player.index]["control"];
    control["living_state"] = 17.into();
    control["motion_direction"] = 15.into();
    world = ScriptWorld::from_snapshot(
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
            generation: world.generation() + 1,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    (world, data, scene)
}

#[test]
fn node_point_ledge_update_precedes_pending_exit_and_preserves_sound_failure() {
    use cyrus_script::script::{PlayerCollisionSource, ScriptInputs};
    for fail in [false, true] {
        let (mut world, data, scene) = node_point_ledge_fixture(0);
        let player = world.selected_player().unwrap();
        let actors = [player, world.id(0).unwrap()];
        let before = world.spatial(player).unwrap().local.position;
        let mut sounds = 0;
        let result = world.move_ordinary_point(
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
                anyhow::ensure!(!fail, "node ledge sound rejected");
                Ok(())
            }),
        );
        let control = world.actors()[player.index].control_state().unwrap();
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("node ledge sound rejected"));
            assert_eq!(
                (
                    control.living_state.byte(),
                    control.action_transition_counter,
                    control.ai_type.byte()
                ),
                (17, 0, 0)
            );
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
fn node_point_ledge_excluded_player_needs_no_ledge_services() {
    let (mut world, _, _) = node_point_ledge_fixture(1);
    let player = world.selected_player().unwrap();
    let before = world.spatial(player).unwrap().local.position;
    assert_eq!(
        world
            .move_ordinary_point(
                player,
                before,
                0,
                cyrus_script::script::ScriptInputs {
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None
            )
            .unwrap(),
        1
    );
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (control.living_state.byte(), control.ai_type.byte()),
        (17, 7)
    );
    assert_eq!(
        world.globals().native_command().point_phase,
        PointPhase::START
    );
    assert_eq!(world.spatial(player).unwrap().local.position, before);
}

#[test]
fn node_point_ledge_native_task_runs_transition_before_retaining_its_slot() {
    let (mut world, data, scene) = node_point_ledge_fixture(0);
    let player = world.selected_player().unwrap();
    let before = world.spatial(player).unwrap().local.position;
    advance_selected_hang_stage(
        &mut world,
        &data,
        &scene,
        false,
        ScriptStage::Body(Thread::Main),
    )
    .unwrap();
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
        world.actors()[player.index]
            .runtime()
            .unwrap()
            .active_tasks(),
        1
    );
    assert_eq!(world.spatial(player).unwrap().local.position, before);
    advance_selected_hang_stage(&mut world, &data, &scene, false, ScriptStage::Tasks).unwrap();
    assert_eq!(
        world.actors()[player.index]
            .runtime()
            .unwrap()
            .active_tasks(),
        1
    );
    assert_eq!(
        world.globals().native_command().point_phase,
        PointPhase::START
    );
}
