use super::*;

#[test]
fn neighbor_spacing_uses_normalized_quantized_local_vectors_and_signed_half_turns() {
    for (target, receiver, neighbor, expected) in [
        ([0; 3], [256, 256000000, 256], [256, 256, 256], -256),
        ([0; 3], [255, 0, 256], [256, 0, 256], -256),
        ([0; 3], [-1, 0, 256], [0, 0, 256], -256),
        ([0; 3], [0, 0, 256], [0, 0, -256], 1024),
        ([0; 3], [0, 0, -256], [0, 0, 256], -1024),
        (
            [-8388608, 0, 0],
            [i32::MAX, 0, 0],
            [i32::MIN + 256, 0, 0],
            -1024,
        ),
        ([0; 3], [0; 3], [0; 3], 0),
    ] {
        let mut scripts = cache();
        scripts.instances[0].map_position = target;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        let target = world.id(0).unwrap();
        let id = world.id(2).unwrap();
        let other = world.id(3).unwrap();
        world
            .initialize_player_spatial(id, receiver, [0; 3])
            .unwrap();
        world
            .initialize_player_spatial(other, neighbor, [0; 3])
            .unwrap();
        let before = serde_json::to_value(world.spatial(id).unwrap()).unwrap();
        assert_eq!(
            world
                .ordinary_neighbor_separation(id, target, Some(other))
                .unwrap(),
            expected,
            "receiver {receiver:?}, neighbor {neighbor:?}"
        );
        assert_eq!(
            serde_json::to_value(world.spatial(id).unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn absent_spacing_neighbor_bypasses_spatial_queries_and_errors_name_all_receivers() {
    let world = ScriptWorld::prepare(&cache(), 7).unwrap();
    let id = world.id(0).unwrap();
    let target = world.id(1).unwrap();
    assert_eq!(
        world
            .ordinary_neighbor_separation(id, target, None)
            .unwrap(),
        1024
    );
    let error = format!(
        "{:#}",
        world
            .ordinary_neighbor_separation(id, target, Some(target))
            .unwrap_err()
    );
    assert!(
        error.contains("world 7 instance 0 neighbor 1 target 1 spacing separation"),
        "{error}"
    );
    assert!(error.contains("spatial"), "{error}");
}

pub(super) fn spacing_fixture(
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    spacing_fixture_group(18, edit_saved)
}

pub(super) fn spacing_fixture_group(
    initial_group: u16,
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    movement_fixture(
        initial_group,
        false,
        true,
        |scripts| {
            scripts.non_engaged_spacing = Some(128);
            for index in 2..5 {
                let mut instance = scripts.instances[0].clone();
                instance.object_id = index;
                scripts.instances.push(instance);
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 5;
            actor.instance_variables = vec![vec![4]; 5];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            scripts.instances[0].angles[1] = 1536;
            scripts.instances[1].map_position = [80, 0, 0];
        },
        |saved| {
            for index in [0, 2, 3] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        action: -1,
                        flags: CombatFlags::from_bits(4),
                        attacker_selection_state: 1,
                        spacing_period: 5,
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            edit_saved(saved);
        },
    )
}

pub(super) fn movement_snapshot(
    world: &ScriptWorld,
    scripts: &Scripts,
    data: &cyrus_assets::animation::Animations,
) -> serde_json::Value {
    let collision = Default::default();
    serde_json::to_value(
        world
            .snapshot(&cyrus_script::script::WorldSnapshotSources {
                identity_memo: None,
                effects: None,
                player: None,
                scripts,
                animations: data,
                collision: &collision,
            })
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn live_spacing_neighbors_keep_first_unmeasured_strict_order_and_previous_primary() {
    for (positions, expected) in [
        ([100000, 200, 100], [Some(0), Some(2)]),
        ([100, 200, 200], [Some(2), Some(3)]),
        ([100, 65535, 65535], [Some(3), None]),
    ] {
        let (mut world, _, _) = spacing_fixture(|saved| {
            // Actor 4 is the receiver so actor 0 is an unmeasured first attacker.
            for (index, x) in [3, 2, 0].into_iter().zip(positions) {
                saved["spatial"]["actors"][index]["local"]["position"] =
                    serde_json::json!([x, 999999, 0]);
            }
            saved["spatial"]["actors"][4]["local"]["position"] = serde_json::json!([0, 0, 0]);
        });
        let ids = world
            .ordinary_actors([0, 0])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>();
        assert_eq!(ids, [4, 3, 2, 1, 0]);
        let pair = world
            .ordinary_spacing_neighbors(world.id(4).unwrap(), world.id(1).unwrap(), None)
            .unwrap()
            .unwrap();
        assert_eq!(
            [
                pair.primary.map(|id| id.index),
                pair.secondary.map(|id| id.index)
            ],
            expected
        );
    }
}

#[test]
fn live_spacing_neighbor_filters_short_circuit_death_and_use_linked_distance() {
    let (mut world, _, _) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
        saved["actors"][2]["control"]["death_selector"] = 8.into();
        saved["actors"][3]["control"]["combat"]["target"] = serde_json::Value::Null;
    });
    world.ordinary_actors([0, 0]).unwrap();
    assert_eq!(
        world
            .ordinary_spacing_neighbors(world.id(0).unwrap(), world.id(1).unwrap(), None)
            .unwrap(),
        None
    );
    let (mut world, _, _) = spacing_fixture(|saved| {
        saved["spatial"]["actors"][2]["local"]["position"] = serde_json::json!([100, 0, 0]);
        saved["spatial"]["actors"][2]["linked_position"] = serde_json::json!([70000, 0, 0]);
        saved["spatial"]["actors"][3]["local"]["position"] = serde_json::json!([200, 999999, 0]);
    });
    world.ordinary_actors([0, 0]).unwrap();
    let pair = world
        .ordinary_spacing_neighbors(world.id(0).unwrap(), world.id(1).unwrap(), None)
        .unwrap()
        .unwrap();
    assert_eq!(pair.primary, Some(world.id(3).unwrap()));
    assert_eq!(pair.secondary, None);
}

#[test]
fn live_neighbor_sidestep_translates_synchronously_and_ignores_completion() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for disabled in [false, true] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 25600]);
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][2]["local"]["position"] = serde_json::json!([256, 0, 25600]);
            saved["actors"][3]["control"]["combat"] = serde_json::Value::Null;
            if disabled {
                saved["actors"][0]["control"]["rope_attached"] = true.into();
            }
        });
        world.ordinary_actors([0, 0]).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert!(
            world
                .space_ordinary_neighbors(
                    id,
                    world.id(1).unwrap(),
                    ScriptInputs {
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap()
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if disabled {
                [0, 0, 25600]
            } else {
                [256, 0, 25600]
            }
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn live_spacing_root_registers_idle_passes_once_and_keeps_rng_and_tasks_untouched() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = spacing_fixture(|_| {});
    world.ordinary_actors([0, 0]).unwrap();
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::default();
    for tick_counter in [0, 2, 5] {
        world
            .space_ordinary(
                id,
                OrdinarySpacingInput {
                    distance: 20480,
                    tick_counter,
                },
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([0])
        );
        assert_eq!(random.state(), 1);
        assert_eq!(world.spatial(id).unwrap().local.position, [0; 3]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
    world.flush_map_updates([0; 3], true).unwrap();
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["map_updates"],
        serde_json::json!([])
    );
}

#[test]
fn live_spacing_root_preserves_action_turn_and_clock_admission_and_nonstance_draws() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let sample = OrdinarySpacingInput {
        distance: 20480,
        tick_counter: 0,
    };
    let (mut world, scripts, data) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["action"] = 4.into();
    });
    let id = world.id(0).unwrap();
    let before = movement_snapshot(&world, &scripts, &data);
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .space_ordinary(
            id,
            sample,
            ScriptInputs::default(),
            &mut random,
            &mut |_, _| panic!("inactive action dispatched sound"),
        )
        .unwrap();
    assert_eq!(movement_snapshot(&world, &scripts, &data), before);
    for (stance, heading, expected_state) in [(1, 1408, 1), (0, 0, 2524885223)] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = stance.into();
            saved["actors"][0]["animation"] = serde_json::Value::Null;
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, heading, 0]);
        });
        let id = world.id(0).unwrap();
        let before = world.inspect(0, 1);
        let mut random = cyrus_game::random::EngineRandom::default();
        let error = world
            .space_ordinary(
                id,
                sample,
                ScriptInputs::default(),
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("frame_scale"));
        assert_eq!(world.inspect(0, 1), before);
        assert_eq!(random.state(), 1);
        let hud = ScriptHud::default();
        world
            .space_ordinary(
                id,
                sample,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(random.state(), expected_state);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn live_spacing_root_reports_signed_division_faults_before_rng_and_registration() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (period, tick_counter) in [(0, 0), (-1, i32::MIN)] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["spacing_period"] = period.into();
        });
        world.ordinary_actors([0, 0]).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::default();
        let error = world
            .space_ordinary(
                world.id(0).unwrap(),
                OrdinarySpacingInput {
                    distance: 20480,
                    tick_counter,
                },
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(
            error.contains("world 7 instance 0 fight spacing") && error.contains("division fault"),
            "{error}"
        );
        assert_eq!(random.state(), 1);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn live_spacing_root_queries_amount_then_moves_only_when_midpoint_is_retained() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (step, expected) in [(10, [2560, 0, 0]), (40, [0; 3])] {
        let (mut world, scripts, data) = spacing_fixture_group(92, |saved| {
            saved["actors"][0]["attributes"][17] = step.into();
            saved["actors"][0]["control"]["movement_flags"] = 0x30.into();
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([25600, 0, 0]);
            saved["spatial"]["actors"][1]["linked_position"] = serde_json::json!([-1000000, 0, 0]);
        });
        let id = world.id(0).unwrap();
        world.refresh_transform(id).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::default();
        world
            .space_ordinary(
                id,
                OrdinarySpacingInput {
                    distance: 25600,
                    tick_counter: 1,
                },
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    forward_step_clear: Some(true),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(random.state(), 1);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([0])
        );
    }
}

#[test]
fn live_spacing_root_registers_far_pass_without_available_walk_animation() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = spacing_fixture(|saved| {
        saved["actors"][0]["animation"] = serde_json::Value::Null;
    });
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .space_ordinary(
            world.id(0).unwrap(),
            OrdinarySpacingInput {
                distance: 100000,
                tick_counter: 1,
            },
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                hud: Some(&hud),
                shell_execution: Some(false),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["map_updates"],
        serde_json::json!([0])
    );
    assert_eq!(random.state(), 1);
}
