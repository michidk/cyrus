use super::*;

#[test]
fn change_group_and_user_store_the_low_byte_and_reject_divergent_membership() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // The attribute after one call by an actor created with `creation` and, optionally, a direct
    // attribute write between creation and the call.
    let run = |function: Native,
               argument: i32,
               attribute: Attribute,
               creation: u8,
               direct_write: Option<u8>| {
        let mut scripts = random_scripts(
            function,
            vec![Value::Integer { value: argument }],
            Object::Me,
        );
        scripts.actors.get_mut("LOGIC").unwrap().attributes[attribute] = creation;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        if let Some(value) = direct_write {
            world.set_attribute_for_test(id, attribute, value);
        }
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .map(|_| world.actors()[0].attributes()[attribute])
    };
    for (function, attribute) in [
        (Native::ChangeGroup, Attribute::Group),
        (Native::ChangeUser, Attribute::User),
    ] {
        let context = format!("{function}");
        assert_eq!(
            run(function, 7, attribute, 5, None).unwrap(),
            7,
            "{context}"
        );
        assert_eq!(
            run(function, 0x105, attribute, 0, None).unwrap(),
            5,
            "{context}: only the low eight bits are stored"
        );
        assert_eq!(
            run(function, 0, attribute, 5, None).unwrap(),
            0,
            "{context}"
        );
        assert_eq!(
            run(function, 5, attribute, 5, None).unwrap(),
            5,
            "{context}: the same group stays"
        );
        let error = run(function, 7, attribute, 5, Some(9)).unwrap_err();
        assert!(
            format!("{error:#}").contains("differs from its recorded membership 5"),
            "{context}: {error:#}"
        );
    }
}

#[test]
fn group_and_user_channels_allow_end_and_group_tasks_wait_for_release() {
    let build = |channel: u8, user: u8, task: bool| {
        let mut ops = vec![];
        if task {
            ops.push(wait(
                4,
                Native::SyncWithGroup,
                CallKind::Task,
                vec![Value::Integer { value: 3 }],
            ));
        }
        ops.extend([set_flag(7), Operation::End { target: 0 }]);
        let mut scripts = wait_cache(ops);
        scripts.definition.functions.push(Function {
            name: Native::SyncWithGroup,
            param_count: 1,
            kind: CallKind::Task,
        });
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.attributes[Attribute::Group] = channel;
        logic.attributes[Attribute::User] = user;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let result = world.step(id, Thread::Main, 20);
        (
            result,
            world.flags()[0],
            world.actors()[0].runtime().unwrap().active_tasks(),
        )
    };
    // End used to fail for any actor with a channel; with a group channel it now ends normally.
    let (result, flag, tasks) = build(5, 0, false);
    result.unwrap();
    assert_eq!((flag, tasks), (7, 0));
    let (result, flag, tasks) = build(0, 5, false);
    result.unwrap();
    assert_eq!((flag, tasks), (7, 0));
    // The barrier wait registers a task that keeps waiting while the channel has not released group 3
    // (nothing has run the channel update), and the thread does not reach its flag write.
    let (result, flag, tasks) = build(5, 0, true);
    result.unwrap();
    assert_eq!((flag, tasks), (12, 1));
    // An actor without a channel never waits: the task completes on its first poll.
    let mut scripts = wait_cache(vec![
        wait(
            4,
            Native::SyncWithGroup,
            CallKind::Task,
            vec![Value::Integer { value: 3 }],
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::SyncWithGroup,
        param_count: 1,
        kind: CallKind::Task,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn group_sync_completes_when_attribute_writes_select_an_unregistered_channel() {
    for initial_channel in [0, 5] {
        let mut scripts = wait_cache(vec![
            wait(
                4,
                Native::SetMyAttr,
                CallKind::Function,
                vec![Value::Integer { value: 29 }, Value::Integer { value: 6 }],
            ),
            wait(
                5,
                Native::SyncWithGroup,
                CallKind::Task,
                vec![Value::Integer { value: 3 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.extend([
            Function {
                name: Native::SetMyAttr,
                param_count: 2,
                kind: CallKind::Function,
            },
            Function {
                name: Native::SyncWithGroup,
                param_count: 1,
                kind: CallKind::Task,
            },
        ]);
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Group] = initial_channel;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].attributes()[Attribute::Group], 6);
        world.step_tasks(id, Some(256)).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags()[0], 7);
    }
}

#[test]
fn live_world_entry_and_reload_register_group_members_before_task_polling() {
    use cyrus_script::script::ScriptActors;
    let mut scripts = wait_cache(vec![
        wait(
            4,
            Native::SyncWithGroup,
            CallKind::Task,
            vec![Value::Integer { value: 3 }],
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::SyncWithGroup,
        param_count: 1,
        kind: CallKind::Task,
    });
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Group] = 5;
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut app, prepared, _) = revisit_test_app(&scripts);
    for _ in 0..2 {
        app.insert_resource(prepared.clone());
        app.update();
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().expect("live script world prepared");
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        world.step_tasks(id, Some(256)).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().active_tasks(),
            1,
            "the fresh member must wait for the channel's first release"
        );
        assert_eq!(world.flags()[0], 12);
    }
}

#[test]
fn user_sync_checks_channel_members_and_the_full_operand_before_continuation() {
    for (channel, ignored, value, expected_tasks, expected_flag) in [
        (0, 0, 3, 0, 7),
        (5, 0, 0, 1, 12),
        (5, 1, 0, 0, 7),
        (5, 1, 256, 1, 12),
        (5, 1, -256, 1, 12),
    ] {
        let mut scripts = wait_cache(vec![
            wait(
                4,
                Native::SyncWithUser,
                CallKind::Task,
                vec![Value::Integer { value }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.push(Function {
            name: Native::SyncWithUser,
            param_count: 1,
            kind: CallKind::Task,
        });
        let logic = scripts.actors.get_mut("LOGIC").unwrap();
        logic.attributes[Attribute::User] = channel;
        logic.attributes[Attribute::AlwaysSync] = ignored;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        world.step_tasks(id, Some(256)).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().active_tasks(),
            expected_tasks
        );
        assert_eq!(world.flags()[0], expected_flag);
    }
}

#[test]
fn reset_user_sync_native_validates_arguments() {
    run_native_world(&[(Native::ResetUserSync, &[])]).unwrap();
    let error = run_native_world(&[(Native::ResetUserSync, &[1])])
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("ResetUserSync requires no arguments"));
}

#[test]
fn reset_group_sync_native_accepts_no_arguments_and_rejects_arguments() {
    // The channel state itself is covered by GroupSync's algorithm tests; this exercises the
    // registry's installed function entry, including its zero-argument contract.
    run_native_world(&[(Native::ResetGroupSync, &[])]).unwrap();
    let error = match run_native_world(&[(Native::ResetGroupSync, &[1])]) {
        Ok(_) => panic!("ResetGroupSync accepted an argument"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("ResetGroupSync requires no arguments"));
}

#[test]
fn combat_coordination_reuses_attacker_state_and_preserves_the_entry_branch() {
    use cyrus_script::script::{OrdinaryCombatBranch, OrdinaryCombatCoordinationInput};
    for (
        engaged,
        state,
        budget,
        close,
        candidate,
        distance,
        expected_branch,
        expected_state,
        expected_counter,
        message,
    ) in [
        (
            true,
            0,
            50,
            11,
            false,
            20480,
            OrdinaryCombatBranch::Waiting,
            1,
            0,
            None,
        ),
        (
            true,
            0,
            50,
            10,
            true,
            20480,
            OrdinaryCombatBranch::Waiting,
            0,
            10,
            None,
        ),
        (
            true,
            0,
            50,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Waiting,
            0,
            11,
            Some(CombatMessageCode::SENDER_COORDINATES),
        ),
        (
            true,
            1,
            0,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Active,
            1,
            11,
            None,
        ),
        (
            true,
            1,
            -1,
            11,
            true,
            29999,
            OrdinaryCombatBranch::Active,
            0,
            0,
            Some(CombatMessageCode::RECEIVER_COORDINATES),
        ),
        (
            true,
            1,
            -1,
            11,
            true,
            30000,
            OrdinaryCombatBranch::Active,
            1,
            11,
            None,
        ),
        (
            false,
            1,
            50,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Disengaged,
            0,
            0,
            Some(CombatMessageCode::RECEIVER_COORDINATES),
        ),
        (
            false,
            0,
            50,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Disengaged,
            0,
            11,
            None,
        ),
    ] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            let own = &mut saved["actors"][0]["control"]["combat"];
            own["attacker_selection_state"] = state.into();
            own["close_distance_passes"] = close.into();
            own["decision"]["aggression"] = budget.into();
            saved["actors"][2]["control"]["combat"] = serde_json::Value::Null;
            let other = &mut saved["actors"][3]["control"]["combat"];
            other["attacker_selection_state"] = i32::from(candidate).into();
            other["spacing_ranges"] = serde_json::json!([1, 10000, 5000]);
            if !candidate {
                other["target"] = serde_json::Value::Null;
            }
            saved["actors"][3]["attributes"][110] = 1.into();
            saved["spatial"]["actors"][3]["local"]["position"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["position"] =
                serde_json::json!([0, distance, 0]);
        });
        world.ordinary_actors([0, 0]).unwrap();
        let id = world.id(0).unwrap();
        let other = world.id(3).unwrap();
        let player = world.id(1).unwrap();
        let before = movement_snapshot(&world, &scripts, &data);
        let mut random = cyrus_game::random::EngineRandom::default();
        let branch = world
            .coordinate_ordinary_combat(
                id,
                OrdinaryCombatCoordinationInput {
                    engaged,
                    selected_player: player,
                    frame_value: 0,
                },
                Default::default(),
                &mut random,
                &mut |_, _| panic!("empty taunt list dispatched sound"),
            )
            .unwrap();
        assert_eq!(branch, expected_branch);
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                combat.attacker_selection_state,
                combat.close_distance_passes
            ),
            (expected_state, expected_counter)
        );
        assert_eq!(combat.decision.aggression, budget);
        let received = world.consume_combat_message(other).unwrap();
        assert_eq!(
            received.map(|m| (m.sender, m.receiver, m.code)),
            message.map(|code| (id, Some(other), code))
        );
        assert!(world.consume_combat_message(other).unwrap().is_none());
        assert_eq!(random.state(), 1);
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(after["actors"][0]["tasks"], before["actors"][0]["tasks"]);
        assert_eq!(after["spatial"], before["spatial"]);
    }
}

#[test]
fn combat_coordinator_queries_preserve_head_order_cycling_and_distinct_filters() {
    let (mut world, _, _) = spacing_fixture(|saved| {
        // Legacy duplicate fields cannot override the live coordination owner on restore.
        saved["actors"][3]["control"]["combat"]["decision"]["near_state"] = 0.into();
        saved["actors"][3]["control"]["combat"]["decision"]["far_ticks"] = 999.into();
        // Waiting lookup admits a different nonnull target and does not query weapon state.
        saved["actors"][3]["control"]["combat"]["target"] = serde_json::json!([1, 4]);
        saved["actors"][3]["attributes"][110] = 1.into();
        saved["actors"][2]["attributes"][110] = 1.into();
        saved["actors"][0]["attributes"][110] = 1.into();
    });
    let own = world.id(0).unwrap();
    let player = world.id(1).unwrap();
    assert!(
        format!(
            "{:#}",
            world.ordinary_waiting_coordinator(player).unwrap_err()
        )
        .contains("traversal unavailable")
    );
    world.ordinary_actors([0, 0]).unwrap();
    assert_eq!(
        world.ordinary_waiting_coordinator(player).unwrap(),
        Some(world.id(3).unwrap())
    );
    assert_eq!(
        world
            .ordinary_active_coordinator(own, player, Default::default())
            .unwrap(),
        Some(world.id(2).unwrap())
    );
    assert_eq!(
        world
            .ordinary_active_coordinator(world.id(2).unwrap(), player, Default::default())
            .unwrap(),
        Some(own)
    );
    world
        .set_combat_coordination(world.id(3).unwrap(), 0)
        .unwrap();
    assert_eq!(
        world.ordinary_waiting_coordinator(player).unwrap(),
        Some(world.id(2).unwrap())
    );
    world
        .set_combat_coordination(world.id(2).unwrap(), 0)
        .unwrap();
    world.set_combat_coordination(own, 0).unwrap();
    assert_eq!(world.ordinary_waiting_coordinator(player).unwrap(), None);
    // Active lookup does not filter candidates by coordination state.
    assert_eq!(
        world
            .ordinary_active_coordinator(own, player, Default::default())
            .unwrap(),
        Some(world.id(2).unwrap())
    );
}

#[test]
fn coordination_keeps_queued_message_and_prior_state_when_taunt_service_fails() {
    use cyrus_script::script::OrdinaryCombatCoordinationInput;
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let mut third = scripts.instances[0].clone();
            third.object_id = 2;
            scripts.instances.push(third);
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 3;
            actor.instance_variables = vec![vec![4]; 3];
            actor.attributes[Attribute::NumTaunts] = 1;
            actor.combat_taunts = vec![CombatTaunt {
                trigger: TauntType::WANT_SWITCH_IN,
                label: *b"TEST",
                animation: 0,
            }];
        },
        |saved| {
            for index in [0, 2] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        flags: CombatFlags::from_bits(4),
                        attacker_selection_state: 1,
                        close_distance_passes: 11,
                        taunt_period: 1,
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            saved["actors"][0]["control"]["combat"]["decision"]["aggression"] = (-1).into();
            saved["actors"][2]["attributes"][110] = 1.into();
        },
    );
    world.ordinary_actors([0, 0]).unwrap();
    let id = world.id(0).unwrap();
    let player = world.id(1).unwrap();
    let other = world.id(2).unwrap();
    let mut random = cyrus_game::random::EngineRandom::default();
    let error = world
        .coordinate_ordinary_combat(
            id,
            OrdinaryCombatCoordinationInput {
                engaged: true,
                selected_player: player,
                frame_value: 0,
            },
            Default::default(),
            &mut random,
            &mut |_, _| panic!("missing timing dispatched sound"),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("nominal ticks unavailable"),
        "{error:#}"
    );
    assert_eq!(
        world.consume_combat_message(other).unwrap().unwrap().code,
        CombatMessageCode::RECEIVER_COORDINATES
    );
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            combat.attacker_selection_state,
            combat.close_distance_passes
        ),
        (1, 11)
    );
    let mut expected = cyrus_game::random::EngineRandom::default();
    for _ in 0..15 {
        expected.draw();
    }
    assert_eq!(random.state(), expected.state());
}

pub(super) fn coordination_message_fixture(
    edit: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    movement_fixture(
        18,
        false,
        true,
        |scripts| {
            for index in 2..6 {
                let mut instance = scripts.instances[0].clone();
                instance.object_id = index;
                scripts.instances.push(instance);
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 6;
            actor.instance_variables = vec![vec![4]; 6];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
        },
        |saved| {
            for index in 0..6 {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        attacker_selection_state: 1,
                        close_distance_passes: 11,
                        decision: cyrus_script::script::CombatDecision {
                            aggression: 7,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 5]);
            }
            saved["actors"][4]["control"]["death_selector"] = 8.into();
            edit(saved);
        },
    )
}

#[test]
fn coordination_messages_reset_only_living_exact_active_attackers_then_assign_budget() {
    for code in [
        CombatMessageCode::RECEIVER_COORDINATES,
        CombatMessageCode::SENDER_COORDINATES,
    ] {
        let (mut world, scripts, data) = coordination_message_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
            saved["actors"][1]["control"]["combat"]["attacker_selection_state"] = 2.into();
            saved["actors"][3]["control"]["combat"]["target"] = serde_json::json!([1, 0]);
        });
        world.ordinary_actors([0, 0]).unwrap();
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world.send_combat_message(sender, receiver, code).unwrap();
        let before = movement_snapshot(&world, &scripts, &data);
        world.process_combat_message(receiver, None).unwrap();
        let states: Vec<_> = world
            .actors()
            .iter()
            .map(|actor| {
                let c = actor.control_state().unwrap().combat.as_ref().unwrap();
                (
                    c.attacker_selection_state,
                    c.close_distance_passes,
                    c.decision.aggression,
                )
            })
            .collect();
        assert_eq!(
            states,
            if code == CombatMessageCode::RECEIVER_COORDINATES {
                vec![
                    (1, 0, 50),
                    (2, 11, 7),
                    (0, 0, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                ]
            } else {
                vec![
                    (0, 11, 7),
                    (1, 0, 50),
                    (0, 0, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                ]
            }
        );
        assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(before["spatial"], after["spatial"]);
        for index in 0..6 {
            assert_eq!(
                before["actors"][index]["tasks"],
                after["actors"][index]["tasks"]
            );
            assert_eq!(
                before["actors"][index]["control"]["combat"]["target"],
                after["actors"][index]["control"]["combat"]["target"]
            );
        }
    }
}

#[test]
fn coordination_messages_use_receiver_or_sender_target_and_skip_null_target_traversal() {
    for code in [
        CombatMessageCode::RECEIVER_COORDINATES,
        CombatMessageCode::SENDER_COORDINATES,
    ] {
        let (mut world, _, _) = coordination_message_fixture(|saved| {
            saved["actors"][if code == CombatMessageCode::RECEIVER_COORDINATES {
                0
            } else {
                1
            }]["control"]["combat"]["target"] = serde_json::Value::Null;
        });
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world.send_combat_message(sender, receiver, code).unwrap();
        // A null target must not require the unprepared ordinary list.
        world.process_combat_message(receiver, None).unwrap();
        for index in 0..6 {
            let c = world.actors()[index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap();
            assert_eq!(
                (
                    c.attacker_selection_state,
                    c.close_distance_passes,
                    c.decision.aggression
                ),
                if index
                    == if code == CombatMessageCode::RECEIVER_COORDINATES {
                        0
                    } else {
                        1
                    }
                {
                    (1, 0, 50)
                } else {
                    (1, 11, 7)
                }
            );
        }
    }
    let (mut world, _, _) = coordination_message_fixture(|saved| {
        saved["actors"][1]["control"]["combat"]["target"] = serde_json::json!([1, 3]);
        saved["actors"][2]["control"]["combat"]["target"] = serde_json::json!([1, 3]);
    });
    world.ordinary_actors([0, 0]).unwrap();
    let receiver = world.id(0).unwrap();
    world
        .send_combat_message(
            world.id(1).unwrap(),
            receiver,
            CombatMessageCode::SENDER_COORDINATES,
        )
        .unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .attacker_selection_state,
        1
    );
    assert_eq!(
        world.actors()[2]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .attacker_selection_state,
        0
    );
}

#[test]
fn coordination_message_consumption_precedes_errors_and_processes_one_code_per_call() {
    let (mut world, _, _) = coordination_message_fixture(|_| {});
    let receiver = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    world
        .send_combat_message(sender, receiver, CombatMessageCode::RECEIVER_COORDINATES)
        .unwrap();
    let error = format!(
        "{:#}",
        world.process_combat_message(receiver, None).unwrap_err()
    );
    assert!(
        error.contains("world 7 instance 0 combat message"),
        "{error}"
    );
    assert!(error.contains("ordinary traversal unavailable"), "{error}");
    assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .decision
            .aggression,
        7
    );
    world
        .send_combat_message(sender, receiver, CombatMessageCode::from(0xd004))
        .unwrap();
    world
        .send_combat_message(sender, receiver, CombatMessageCode::SENDER_COORDINATES)
        .unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!(
        world
            .consume_combat_message(receiver)
            .unwrap()
            .unwrap()
            .code,
        CombatMessageCode::SENDER_COORDINATES
    );
    for code in [CombatMessageCode::HIT, CombatMessageCode::DIRECT_DAMAGE] {
        let before_flags = [receiver, sender].map(|actor| {
            world.actors()[actor.index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits()
        });
        world.send_combat_message(sender, receiver, code).unwrap();
        let error = format!(
            "{:#}",
            world.process_combat_message(receiver, None).unwrap_err()
        );
        assert!(error.contains("unsupported"), "{error}");
        assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
        let flags = [receiver, sender].map(|actor| {
            world.actors()[actor.index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits()
        });
        assert_eq!(
            flags,
            if code == CombatMessageCode::HIT {
                before_flags.map(|flags| flags & !3)
            } else {
                before_flags
            }
        );
    }
}

/// Six combat actors in a line along Z (receiver first, defender second), all with combat flags 7
/// and coordination state one except where `edit` changes them.
fn deflected_attack_fixture(
    edit: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    movement_fixture(
        18,
        false,
        true,
        |scripts| {
            for index in 2..6 {
                let mut instance = scripts.instances[0].clone();
                instance.object_id = index;
                scripts.instances.push(instance);
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 6;
            actor.instance_variables = vec![vec![4]; 6];
            for (index, instance) in scripts.instances.iter_mut().enumerate() {
                instance.active = 1;
                instance.map_position = [1000, 0, 1000 + 500 * index as i64];
            }
        },
        |saved| {
            for index in 0..6 {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        attacker_selection_state: 1,
                        flags: CombatFlags::from_bits(7),
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 5]);
            }
            saved["actors"][4]["control"]["death_selector"] = 8.into();
            edit(saved);
        },
    )
}

#[test]
fn deflected_attack_pushes_the_defender_back_and_joins_coordination() {
    let flags = |world: &ScriptWorld, index: usize| {
        world.actors()[index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .flags
            .bits()
    };
    let state = |world: &ScriptWorld, index: usize| {
        world.actors()[index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .attacker_selection_state
    };
    // No coordination state: the receiver asks the first waiting coordinator other than the
    // defender (actor 5; the dead actor 4 is skipped) to take over.
    let (mut world, _, _) = deflected_attack_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
    });
    world.ordinary_actors([0, 0]).unwrap();
    let (receiver, sender) = (world.id(0).unwrap(), world.id(1).unwrap());
    let before = world.spatial(sender).unwrap().local.position;
    world
        .send_combat_message(sender, receiver, CombatMessageCode::BLOCK)
        .unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!((flags(&world, 0), flags(&world, 1)), (4, 4));
    let after = world.spatial(sender).unwrap().local.position;
    assert_eq!(
        [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2]
        ],
        [0, 0, 3000],
        "the defender moves 3000 Q8 units directly away from the attacker"
    );
    let message = world
        .consume_combat_message(world.id(5).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        (message.code, message.sender),
        (CombatMessageCode::SENDER_COORDINATES, receiver)
    );
    assert_eq!(state(&world, 0), 0, "the receiver waits for the transfer");

    // No other waiting coordinator: the receiver becomes the active one itself.
    let (mut world, _, _) = deflected_attack_fixture(|saved| {
        for index in 0..6 {
            let value: i32 = if index == 0 { 0 } else { 2 };
            saved["actors"][index]["control"]["combat"]["attacker_selection_state"] = value.into();
        }
    });
    world.ordinary_actors([0, 0]).unwrap();
    let (receiver, sender) = (world.id(0).unwrap(), world.id(1).unwrap());
    world
        .send_combat_message(sender, receiver, CombatMessageCode::BLOCK)
        .unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!(state(&world, 0), 1);
    assert_eq!(
        world.consume_combat_message(world.id(5).unwrap()).unwrap(),
        None
    );

    // A receiver that already has a coordination state keeps it and sends nothing; the flags and
    // the push still apply.
    let (mut world, _, _) = deflected_attack_fixture(|_| {});
    world.ordinary_actors([0, 0]).unwrap();
    let (receiver, sender) = (world.id(0).unwrap(), world.id(1).unwrap());
    let before = world.spatial(sender).unwrap().local.position;
    world
        .send_combat_message(sender, receiver, CombatMessageCode::BLOCK)
        .unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!(state(&world, 0), 1);
    assert_eq!((flags(&world, 0), flags(&world, 1)), (4, 4));
    assert_ne!(world.spatial(sender).unwrap().local.position, before);
    assert_eq!(
        world.consume_combat_message(world.id(5).unwrap()).unwrap(),
        None
    );
}

#[test]
fn coordination_message_keeps_prior_resets_when_selected_death_state_is_unknown() {
    let (mut world, _, _) = coordination_message_fixture(|_| {});
    let order = world.ordinary_actors([0, 0]).unwrap();
    assert_eq!(
        order.iter().map(|id| id.index).collect::<Vec<_>>(),
        [5, 4, 3, 2, 1, 0]
    );
    let receiver = world.id(0).unwrap();
    let selected_player = world.id(2).unwrap();
    world
        .send_combat_message(
            world.id(1).unwrap(),
            receiver,
            CombatMessageCode::RECEIVER_COORDINATES,
        )
        .unwrap();
    let error = format!(
        "{:#}",
        world
            .process_combat_message(receiver, Some(selected_player))
            .unwrap_err()
    );
    assert!(
        error.contains("selected-player hit inhibition unavailable"),
        "{error}"
    );
    let state = |index: usize| {
        let c = world.actors()[index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        (
            c.attacker_selection_state,
            c.close_distance_passes,
            c.decision.aggression,
        )
    };
    assert_eq!(state(3), (0, 0, 7));
    assert_eq!(state(2), (1, 11, 7));
    assert_eq!(state(0), (1, 11, 7));
    assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
}
