use super::*;

#[test]
fn strength_and_armor_need_a_combat_record() {
    for name in [
        Native::SetStrength,
        Native::SetArmor,
        Native::AdjustStrength,
        Native::AdjustArmor,
    ] {
        let error = run_native_calls(&[(name, &[100][..])]).unwrap_err();
        assert!(
            format!("{error:#}").contains("receiver combat record unavailable"),
            "{error:#}"
        );
    }
}

#[test]
fn ordinary_health_value_wraps_before_signed_clamping() {
    use cyrus_script::script::ordinary_health_value as health;
    assert_eq!(health(100, -10), 90);
    assert_eq!(health(100, -256), 0);
    assert_eq!(health(250, 20), 255);
    assert_eq!(health(1, i32::MAX), 0);
    assert_eq!(health(0, i32::MIN), 0);
}

#[test]
fn health_strength_and_armor_queries_read_the_receiver() {
    assert_eq!(run_native_calls(&[(Native::Health, &[])]).unwrap(), 0);
    assert_eq!(
        run_native_calls(&[(Native::SetMyAttr, &[3, 40]), (Native::Health, &[])]).unwrap(),
        40
    );
    assert_eq!(
        run_native_calls(&[
            (Native::SetMyAttr, &[3, 40]),
            (Native::ReplenishHealth, &[])
        ])
        .unwrap(),
        1
    );
    assert_eq!(
        run_native_calls(&[
            (Native::SetMyAttr, &[3, 40]),
            (Native::ReplenishHealth, &[]),
            (Native::Health, &[])
        ])
        .unwrap(),
        100
    );
    assert_eq!(
        run_native_calls(&[
            (Native::SetMyAttr, &[3, 100]),
            (Native::ReplenishHealth, &[])
        ])
        .unwrap(),
        0
    );
    for name in [Native::Strength, Native::Armor] {
        let error = run_native_calls(&[(name, &[])]).unwrap_err();
        assert!(
            format!("{error:#}").contains("receiver combat record unavailable"),
            "{error:#}"
        );
    }
}

#[test]
fn selected_replenish_health_uses_shared_feedback_without_additive_health_services() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage};
    for (health, timer, with_hud, expected, feedback) in [
        (40, 0, true, 100, 25),
        (40, 50, true, 100, 50),
        (100, 0, false, 100, 0),
        (255, 0, false, 255, 0),
        (40, 0, false, 40, 0),
    ] {
        let mut scripts = random_scripts(Native::ReplenishHealth, vec![], Object::Player);
        scripts.definition.functions[1].param_count = 0;
        add_standalone_receivers(&mut scripts);
        let player = scripts.actors.get_mut("CYRUS").unwrap();
        player.attributes[Attribute::Health] = health;
        player.attributes[Attribute::Invulnerable] = 1;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let player_index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let caller = world.id(0).unwrap();
        let hud = ScriptHud::default();
        if timer == 50 {
            hud.advance_health_feedback(true, 0, 0, 0);
        }
        let result = world.advance(
            caller,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                hud: with_hud.then_some(&hud),
                ..Default::default()
            },
            None,
            None,
            100,
        );
        if health < 100 && !with_hud {
            assert!(format!("{:#}", result.unwrap_err()).contains("HUD owner unavailable"));
        } else {
            result.unwrap();
            assert_eq!(
                world.actors()[0].runtime().unwrap().variables()[0],
                i32::from(health < 100)
            );
        }
        assert_eq!(
            world.actors()[player_index].attributes()[Attribute::Health],
            expected
        );
        assert_eq!(hud.health_feedback_ticks(), feedback);
        // This command bypasses additive-health initialization and inhibition.
        assert!(world.globals().selected_health().is_none());
    }
}

#[test]
fn selected_native_health_routes_addition_and_retains_early_feedback() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage};
    for (function, change, mode, initialized, expected, error) in [
        (Native::AdjustHealth, 7, 0, true, 47, None),
        (Native::AdjustHealth, 300, 0, true, 100, None),
        (Native::AdjustHealth, i32::MAX, 0, true, 0, None),
        (
            Native::AdjustHealth,
            -7,
            0,
            true,
            33,
            Some("random stream unavailable"),
        ),
        (
            Native::AdjustHealth,
            7,
            0,
            false,
            40,
            Some("health initialization unavailable"),
        ),
        (Native::AdjustHealth, 7, 2, true, 47, None),
        (
            Native::AdjustHealth,
            -7,
            2,
            true,
            33,
            Some("random stream unavailable"),
        ),
        (
            Native::AdjustHealth,
            -40,
            2,
            true,
            0,
            Some("random stream unavailable"),
        ),
        (
            Native::AdjustHealth,
            -40,
            3,
            true,
            0,
            Some("native source unavailable"),
        ),
        (Native::AdjustHealth, -40, 4, true, 0, None),
        (Native::AdjustHealth, -40, 5, true, 0, None),
        (Native::AdjustHealth, -40, 6, true, 0, None),
        (Native::AdjustHealth, i32::MAX, 8, true, 0, None),
        (Native::AdjustHealth, -40, 9, true, 0, None),
        (Native::AdjustHealth, -40, 10, true, 0, None),
        (Native::AdjustHealth, -40, 11, true, 0, None),
        (Native::SetHealth, 60, 0, true, 40, None),
        (Native::SetHealth, 0, 12, true, 0, None),
        (Native::SetHealth, 60, 13, true, 60, None),
    ] {
        let (mut scripts, _, mut data, scene) = player_walking_fixture();
        for (attribute, value) in [
            (Attribute::Health, 40),
            (Attribute::Npc, if mode >= 12 { 0 } else { 1 }),
            (Attribute::Invulnerable, if mode >= 12 { 0 } else { 1 }),
        ] {
            scripts.actors.get_mut("CYRUS").unwrap().attributes[attribute] = value;
        }
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::ZapAnim] = 77;
        if matches!(mode, 5 | 6 | 12) {
            scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::PlayerType] = 1;
        }
        if mode >= 9 || mode == 6 {
            if mode >= 10 {
                scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::HoldingWeapon] = 1;
            }
            let groups = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap()["animation_groups"]
                .as_array_mut()
                .unwrap();
            for group in [121, 122, 134] {
                if mode == 11 && group == 122 {
                    continue;
                }
                groups.push(
                    serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
                );
            }
        }
        if mode == 4 {
            data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap()["animation_groups"]
                .as_array_mut()
                .unwrap()
                .push(
                    serde_json::json!({"group_index":77,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":4,"sound_param":9,"volume_shift":0},
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}),
                );
        }
        let function_id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: function,
            param_count: if function == Native::SetHealth { 1 } else { 2 },
            kind: CallKind::Function,
        });
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = Operation::ObjectCall {
            call: Call {
                function_id,
                function,
                kind: CallKind::Function,
                target: Object::Player,
                arguments: if function == Native::SetHealth {
                    vec![Value::Integer { value: change }]
                } else {
                    vec![
                        Value::Integer { value: change },
                        Value::Integer { value: mode },
                    ]
                },
            },
        };
        scripts.content_id = scripts.content_identity().unwrap();
        data.metadata["script_content_id"] = scripts.content_id.clone().into();
        let (mut world, _, _) =
            static_model_fixture_session(scripts.clone(), None, [10, 20, 30], true);
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let player = world.id(index).unwrap();
        if initialized {
            world
                .initialize_player_animation(player, &data, 0, &mut |_| Ok(()))
                .unwrap();
        }
        if mode >= 3 {
            world
                .initialize_player_spatial(player, [12, 34, 56], [0; 3])
                .unwrap();
        }
        if mode >= 4 {
            world
                .advance(
                    world.id(if mode == 4 { 0 } else { index }).unwrap(),
                    ScriptStage::Entry,
                    ScriptInputs {
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap();
            world
                .set_actor_surface(
                    player,
                    if mode == 6 {
                        Surface::DEEPWATER
                    } else {
                        Surface::UNKNOWN
                    },
                )
                .unwrap();
        }
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        let result = world.advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                death_suppressed: Some(mode == 8),
                ..Default::default()
            },
            None,
            (mode >= 3).then_some(&mut random),
            100,
        );
        match error {
            Some(message) => assert!(format!("{:#}", result.unwrap_err()).contains(message)),
            None => result.unwrap(),
        }
        assert_eq!(
            world.actors()[index].attributes()[Attribute::Health],
            expected,
            "{function} mode {mode}"
        );
        assert_eq!(hud.health_feedback_ticks(), 25);
        if (3..=12).contains(&mode) && mode != 8 {
            let state = world.globals().selected_health().unwrap();
            assert!(state.hit_inhibited());
            assert_eq!(state.death().unwrap().progress(), 0);
            assert_eq!(state.death().unwrap().local_position(), Some([12, 34, 56]));
            assert_eq!(state.death().unwrap().mode(), if mode == 6 { 2 } else { 0 });
            assert_eq!(
                world.globals().player_death_override_group(),
                Some(if mode == 4 {
                    77
                } else if mode == 7 {
                    91
                } else {
                    0
                })
            );
        }
        if (9..=11).contains(&mode) {
            assert_eq!(
                world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
                if mode == 9 {
                    134
                } else if mode == 10 {
                    122
                } else {
                    121
                }
            );
        }
        if initialized {
            assert_eq!(
                world.globals().selected_health().unwrap().displayed(),
                if error.is_some() && mode == 0 {
                    40
                } else {
                    i32::from(expected)
                }
            );
        }
        if initialized && world.globals().player_death_override_group() == Some(0) {
            assert!(
                !world
                    .select_selected_death_animation_default(player, 1, &mut |_, _| panic!(
                        "inactive override"
                    ))
                    .unwrap()
            );
        }

        if (9..=11).contains(&mode) || matches!(mode, 4 | 6) {
            // Effect mode is retained independently of later ground-surface updates.
            if mode == 6 {
                world.set_actor_surface(player, Surface::UNKNOWN).unwrap();
            }
            world.prepare_player_model_resources(player, &data).unwrap();
            let map = world.id(0).unwrap();
            world
                .refresh_model_presentations(&data, &[map, player])
                .unwrap();
            let prepared = world
                .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
                .unwrap();
            let initial_position = world.spatial(player).unwrap().local.position;
            let mut progress = 0u32;
            for ticks in if mode == 4 {
                [13, 0, 0, 0]
            } else {
                [0, 1, u32::MAX, 13]
            } {
                let state_before = random.state();
                let mut sound_calls = 0;
                let result = world.advance_player_controller(
                    player,
                    cyrus_script::script::OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    cyrus_script::clock::ClockStep {
                        frame_scale: 256,
                        nominal_ticks: ticks,
                        fine_ticks: 0,
                    },
                    ScriptInputs {
                        player_controller: Some(cyrus_script::script::PlayerControllerInput {
                            held: &cyrus_script::player::input::PlayerInput {
                                forward: true,
                                ..Default::default()
                            },
                            walk_mode: true,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        hud: (ticks == 0).then_some(&hud),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |actor, sound| {
                        assert_eq!(mode, 4);
                        assert_eq!(actor, player);
                        assert!(matches!(
                            sound,
                            cyrus_script::script::ScriptSound::Animation { .. }
                        ));
                        sound_calls += 1;
                        anyhow::bail!("death starter sound rejected")
                    },
                );
                if mode == 4 {
                    assert!(
                        format!("{:#}", result.unwrap_err())
                            .contains("death starter sound rejected")
                    );
                    assert_eq!(sound_calls, 1);
                    assert_eq!(
                        world
                            .globals()
                            .selected_health()
                            .unwrap()
                            .death()
                            .unwrap()
                            .progress(),
                        0
                    );
                    assert_eq!(world.globals().player_death_override_group(), Some(77));
                    assert_eq!(random.state(), state_before);
                    break;
                }
                if mode == 6 && ticks == 1 {
                    let error = result.unwrap_err();
                    assert!(
                        format!("{error:#}").contains("environmental effect spawning unavailable")
                    );
                    assert_eq!(
                        world
                            .globals()
                            .selected_health()
                            .unwrap()
                            .death()
                            .unwrap()
                            .progress(),
                        1
                    );
                    assert_eq!(random.state(), 3263785912);
                    break;
                }
                assert!(result.unwrap());
                progress = progress.wrapping_add(ticks);
                assert_eq!(
                    world
                        .globals()
                        .selected_health()
                        .unwrap()
                        .death()
                        .unwrap()
                        .progress(),
                    progress
                );
                assert_eq!(
                    world.globals().player_death_override_group(),
                    Some(if mode == 9 || mode == 6 { 134 } else { 122 })
                );
                assert_eq!(
                    world.spatial(player).unwrap().local.position,
                    initial_position
                );
                if mode == 6 {
                    assert_eq!(random.state(), 3397979675);
                } else {
                    assert_eq!(random.state(), state_before);
                }
                if mode == 11 {
                    assert_eq!(
                        world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
                        AnimationGroup::PANIC.index() // Missing `anim_death_fight_hard` uses the animation owner's default, not health's fallback `anim_death_fight_stab`.
                    );
                }
            }
        }
        if matches!(mode, 9 | 10) {
            let before = movement_snapshot(&world, &scripts, &data);
            let mut expected = before["actors"][index]["animation"].clone();
            let group = if mode == 9 { 134 } else { 122 };
            expected["playback"]["default"] = group.into();
            expected["playback"]["pending"] = serde_json::Value::Null;
            expected["playback"]["current"][1]["exit_reached"] = false.into();
            expected["playback"]["current"][1]["loops"] = 32.into();
            assert!(
                world
                    .select_selected_death_animation_default(player, u32::MAX, &mut |_, _| panic!(
                        "retained death group restarted"
                    ))
                    .unwrap()
            );
            let after = movement_snapshot(&world, &scripts, &data);
            assert_eq!(after["actors"][index]["animation"], expected);
            assert_eq!(
                world
                    .globals()
                    .selected_health()
                    .unwrap()
                    .death()
                    .unwrap()
                    .progress(),
                13
            );
            assert_eq!(
                world.finish_world_loop(0).unwrap(),
                cyrus_script::script::WorldLoopOutcome::Continue
            );
            data.metadata["markers"] = serde_json::json!([]);
            data.metadata["walk_node_maps"] = serde_json::json!([]);
            data.metadata["walk_node_maps"] = serde_json::json!([]);
            let mut saved = movement_snapshot(&world, &scripts, &data);
            saved["actors"][index]["animation"]["playback"]["frame_progress"] = 0.into();
            saved["actors"][index]["animation"]["playback"]["current"][1]["command"] = 1.into();
            let globals = std::mem::take(world.globals_mut());
            let collision = Default::default();
            let sources = cyrus_script::script::WorldSnapshotSources {
                identity_memo: None,
                effects: None,
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &collision,
            };
            let mut continued = ScriptWorld::from_snapshot(
                serde_json::from_value(saved).unwrap(),
                &sources,
                cyrus_script::script::WorldRestoreContext {
                    generation: 2,
                    globals,
                    samples: &BTreeMap::new(),
                },
            )
            .unwrap();
            assert_eq!(
                continued.finish_world_loop(0).unwrap(),
                cyrus_script::script::WorldLoopOutcome::Death
            );
        }
    }
}

#[test]
fn native_nonplayer_health_reports_actual_loss_without_replaying_healing() {
    let mut world = run_native_world(&[
        (Native::SetHealth, &[60]),
        (Native::AdjustHealth, &[-20, 0]),
        (Native::AdjustHealth, &[5, 0]),
        (Native::AdjustHealth, &[-10, 0]),
    ])
    .unwrap();
    let events = world.take_damage_events();
    assert_eq!(
        events.iter().map(|event| event.loss).collect::<Vec<_>>(),
        [20, 10]
    );
    assert!(world.take_damage_events().is_empty());
}

#[test]
fn set_and_adjust_health_clamp_and_leave_death_and_gates_unavailable() {
    let health = |calls: &[(Native, &[i32])]| {
        let mut calls = calls.to_vec();
        calls.push((Native::Health, &[]));
        run_native_calls(&calls)
    };
    assert_eq!(health(&[(Native::SetHealth, &[60])]).unwrap(), 60);
    assert_eq!(
        health(&[
            (Native::SetHealth, &[60]),
            (Native::AdjustHealth, &[-20, 1])
        ])
        .unwrap(),
        40
    );
    assert_eq!(health(&[(Native::AdjustHealth, &[300, 0])]).unwrap(), 255);
    assert_eq!(health(&[(Native::AdjustHealth, &[-5, 0])]).unwrap(), 0);
    // A nonpositive result of the death-owning variants first asks whether the receiver holds a
    // weapon, which this inventory-less world cannot answer.
    for calls in [
        &[(Native::AdjustHealth, &[-5, 1][..])][..],
        &[(Native::SetHealth, &[0][..])][..],
    ] {
        let error = health(calls).unwrap_err();
        assert!(
            format!("{error:#}").contains("inventory owner unavailable"),
            "{error:#}"
        );
    }
    // Attribute 120 ignores changes; attribute 97 needs the combat-record gate.
    assert_eq!(
        health(&[(Native::SetMyAttr, &[120, 1]), (Native::SetHealth, &[60])]).unwrap(),
        0
    );
    let error = health(&[(Native::SetMyAttr, &[97, 1]), (Native::SetHealth, &[60])]).unwrap_err();
    assert!(
        format!("{error:#}").contains("IsHoldingWeapon: inventory owner unavailable"),
        "{error:#}"
    );
}

#[test]
fn contact_health_keeps_addition_separate_from_caller_owned_death() {
    use cyrus_script::script::{CombatControlState, ObstacleEnvironment};
    for (health, change, immune, gated, flags, expected) in [
        (50, -7, 0, 0, 0, 43),
        (5, -7, 0, 0, 0, 0),
        (50, 300, 0, 0, 0, 255),
        (50, i32::MAX, 0, 0, 0, 0),
        (50, -7, 1, 0, 0, 50),
        (50, -7, 0, 1, 0, 50),
        (50, -7, 0, 1, 4, 43),
    ] {
        let (mut world, _, _) = movement_fixture(
            18,
            false,
            true,
            |_| {},
            |saved| {
                saved["actors"][0]["attributes"][3] = health.into();
                saved["actors"][0]["attributes"][120] = immune.into();
                saved["actors"][0]["attributes"][97] = gated.into();
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        flags: CombatFlags::from_bits(flags),
                        ..Default::default()
                    })
                    .unwrap();
            },
        );
        let id = world.id(0).unwrap();
        world
            .apply_nonplayer_contact_health(
                id,
                change,
                ObstacleEnvironment {
                    selected_player: None,
                    auto_grab: false,
                    item_system_ready: true,
                    shell_execution: false,
                    ticks: 1,
                },
            )
            .unwrap();
        assert_eq!(world.actors()[0].attributes()[Attribute::Health], expected);
        let damage = world.take_damage_events();
        if expected < health as u8 {
            assert_eq!(damage.len(), 1);
            assert_eq!(damage[0].actor, id);
            assert_eq!(damage[0].loss, health as u8 - expected);
            assert_eq!(damage[0].name, world.actors()[0].name());
            assert!(damage[0].position.is_some());
        } else {
            assert!(damage.is_empty());
        }
        assert!(world.take_damage_events().is_empty());
        assert!(
            world.take_death_events().is_empty(),
            "raw zero health is not a committed death"
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().ai_type,
            AiType::NONE
        );
        // No death groups exist in this fixture. Dispatch must still mark death,
        // without querying timing or calling animation sounds.
        world
            .dispatch_contact_death(id, 0, None, &mut |_, _| panic!("unexpected sound"))
            .unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().ai_type,
            AiType::DEAD
        );
    }
}

pub(super) fn player_health_fixture() -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    player_health_fixture_with(|_| {})
}

pub(super) fn player_health_fixture_with(
    prepare: impl FnOnce(&mut Scripts),
) -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (scripts, mut world, data, scene) = player_walking_fixture_with(prepare);
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let id = world.id(index).unwrap();
    assert_eq!(world.actors()[index].attributes()[Attribute::Health], 0);
    world
        .apply_selected_contact_health(
            id,
            50,
            ScriptInputs {
                hud: Some(&ScriptHud::default()),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::from_state(42),
            &mut |_, _| panic!("fixture healing sound"),
        )
        .unwrap();
    assert_eq!(world.globals().selected_health().unwrap().displayed(), 50);
    (scripts, world, data, scene)
}

#[test]
fn selected_health_publishes_clamped_healing_without_random_or_death() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    for (change, expected) in [(0, 50), (7, 57), (300, 100), (i32::MAX, 0)] {
        let (_, mut world, _, _) = player_health_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let id = world.id(index).unwrap();
        assert_eq!(world.actors()[index].attributes()[Attribute::Health], 50);
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        world
            .apply_selected_contact_health(
                id,
                change,
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| panic!("healing must not dispatch sound"),
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].attributes()[Attribute::Health],
            expected as u8
        );
        let state = world.globals().selected_health().unwrap();
        assert_eq!(state.maximum(), 100);
        assert_eq!(state.displayed(), expected);
        assert!(!state.hit_inhibited());
        assert_eq!(hud.health_feedback_ticks(), 25);
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn selected_health_rejects_missing_hud_before_actor_write() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs};
    let (_, mut world, _, _) = player_health_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let id = world.id(index).unwrap();
    let initial = world.actors()[index].attributes()[Attribute::Health];
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let error = world
        .apply_selected_contact_health(id, 7, ScriptInputs::default(), &mut random, &mut |_, _| {
            panic!("unexpected sound")
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("HUD owner unavailable"));
    assert_eq!(
        world.actors()[index].attributes()[Attribute::Health],
        initial
    );
    assert_eq!(
        world.globals().selected_health().unwrap().displayed(),
        i32::from(initial)
    );
    assert_eq!(random.state(), 42);
}

#[test]
fn selected_health_retains_damage_before_feedback_failure_without_publication() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (_, mut world, _, _) = player_health_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let id = world.id(index).unwrap();
    let initial = world.actors()[index].attributes()[Attribute::Health];
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let error = world
        .apply_selected_contact_health(
            id,
            -7,
            ScriptInputs {
                hud: Some(&hud),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| panic!("unexpected sound"),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("selected reaction nominal ticks unavailable"));
    assert_eq!(
        world.actors()[index].attributes()[Attribute::Health],
        initial.saturating_sub(7)
    );
    assert_eq!(
        world.globals().selected_health().unwrap().displayed(),
        i32::from(initial)
    );
    assert_eq!(hud.health_feedback_ticks(), 25);
    let mut expected = cyrus_game::random::EngineRandom::from_state(42);
    expected.draw();
    assert_eq!(random.state(), expected.state());
}

pub(super) fn replace_health_test_animation(
    mut world: ScriptWorld,
    scripts: &Scripts,
    data: &mut cyrus_assets::animation::Animations,
    index: usize,
    group: u16,
    edit: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let mut saved = movement_snapshot(&world, scripts, data);
    saved["actors"][index]["animation"] = serde_json::Value::Null;
    edit(&mut saved);
    let globals = std::mem::take(world.globals_mut());
    let mut restored = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts,
            animations: data,
            collision: &Default::default(),
        },
        WorldRestoreContext {
            generation: world.generation() + 1,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    restored
        .bind_animation(
            restored.id(index).unwrap(),
            data,
            AnimationGroup::from_index(group),
        )
        .unwrap();
    restored
}

#[test]
fn selected_health_reaction_suppression_preserves_random_and_publishes_damage() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage};
    for group in [73, 74, 117, 118, 132, 133] {
        let (scripts, world, mut data, _) = player_health_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap();
        definition["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, group, |_| {});
        let id = world.id(index).unwrap();
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
        let initial = world.actors()[index].attributes()[Attribute::Health];
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        world
            .apply_selected_contact_health(
                id,
                -7,
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| panic!("suppressed reaction must not dispatch sound"),
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].attributes()[Attribute::Health],
            initial.saturating_sub(7)
        );
        assert_eq!(
            world.globals().selected_health().unwrap().displayed(),
            i32::from(initial.saturating_sub(7))
        );
        assert_eq!(random.state(), 42);
        assert_eq!(
            world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
            group
        );
    }
}

#[test]
fn selected_health_publishes_damage_after_parity_reaction_and_sound() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    for (seed, group) in [(1, 133), (42, 132)] {
        let (scripts, world, mut data, _) = player_health_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap();
        for group in [132, 133] {
            definition["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":4,"sound_param":9,"volume_shift":0},{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
        }
        let mut world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |_| {});
        let id = world.id(index).unwrap();
        assert_eq!(world.actors()[index].attributes()[Attribute::Health], 50);
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(seed);
        let mut expected = cyrus_game::random::EngineRandom::from_state(seed);
        expected.draw();
        let mut sound_count = 0;
        world
            .apply_selected_contact_health(
                id,
                -7,
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                &mut random,
                &mut |receiver, _| {
                    assert_eq!(receiver, id);
                    sound_count += 1;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(sound_count, 1);
        assert_eq!(
            world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
            group
        );
        assert_eq!(world.globals().selected_health().unwrap().displayed(), 43);
        assert_eq!(random.state(), expected.state());
    }
}

#[test]
fn selected_health_resets_ledge_after_reaction_and_publishes_after_reset_sound() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    for fail_reset_sound in [false, true] {
        let (scripts, world, mut data, _) = player_health_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap();
        for group in [56, 133] {
            definition["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":4,"sound_param":9,"volume_shift":0},{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
        }
        let generation = world.generation();
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
                saved["actors"][index]["control"]["living_state"] = 15.into();
                saved["actors"][index]["control"]["directional_state"] =
                    serde_json::json!([128, 255]);
                saved["actors"][index]["model"]["ledge_support"] =
                    serde_json::json!([generation, index]);
            });
        let id = world.id(index).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        let mut calls = 0;
        let result = world.apply_selected_contact_health(
            id,
            -7,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                shell_execution: Some(false),
                ..Default::default()
            },
            &mut random,
            &mut |_, cue| {
                if matches!(
                    cue,
                    cyrus_script::script::ScriptSound::Animation {
                        sound: cyrus_world::animation::AnimationSound::Play { .. },
                        ..
                    }
                ) {
                    calls += 1;
                    anyhow::ensure!(!fail_reset_sound || calls != 2, "reset sound failed");
                }
                Ok(())
            },
        );
        assert_eq!(calls, 2);
        assert_eq!(result.is_err(), fail_reset_sound);
        let damage = world.take_damage_events();
        assert_eq!(damage.len(), 1);
        assert_eq!(damage[0].actor, id);
        assert_eq!(damage[0].loss, 7);
        assert_eq!(world.actors()[index].attributes()[Attribute::Health], 43);
        assert_eq!(
            world.globals().selected_health().unwrap().displayed(),
            if fail_reset_sound { 50 } else { 43 }
        );
        let control = world.actors()[index].control_state().unwrap();
        assert_eq!(
            control.living_state,
            if fail_reset_sound {
                LivingState::DROPPING
            } else {
                LivingState::ORDINARY
            }
        );
        assert_eq!(
            control.action_transition_counter,
            if fail_reset_sound { 0 } else { 6 }
        );
        assert_eq!(control.actions, [128, 255]);
        let saved = movement_snapshot(&world, &scripts, &data);
        assert!(saved["actors"][index]["model"]["ledge_support"].is_null());
    }
}

#[test]
fn selected_health_ledge_reset_uses_torch_counter_independently_of_weapon_state() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    for torch in [0u8, 1, 100, 255] {
        for weapon_drawn in [false, true] {
            let (scripts, world, mut data, _) = player_health_fixture();
            let index = world
                .actors()
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Player))
                .unwrap();
            let definition = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap();
            for group in [56, 117, 118, 132, 133] {
                definition["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
            }
            let mut world =
                replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
                    saved["actors"][index]["control"]["living_state"] = 15.into();
                    saved["actors"][index]["control"]["torch_burning"] = torch.into();
                    saved["actors"][index]["control"]["weapon_drawn"] = weapon_drawn.into();
                    saved["actors"][index]["control"]["motion_direction"] = 7.into();
                    saved["actors"][index]["control"]["directional_state"] =
                        serde_json::json!([128, 255]);
                });
            let id = world.id(index).unwrap();
            world
                .apply_selected_contact_health(
                    id,
                    -7,
                    ScriptInputs {
                        hud: Some(&ScriptHud::default()),
                        nominal_ticks: Some(1),
                        shell_execution: Some(false),
                        ..Default::default()
                    },
                    &mut cyrus_game::random::EngineRandom::from_state(1),
                    &mut |_, _| Ok(()),
                )
                .unwrap();
            let control = world.actors()[index].control_state().unwrap();
            assert_eq!(
                control.living_state,
                if torch == 0 {
                    LivingState::ORDINARY
                } else {
                    LivingState::DROPPING
                }
            );
            assert_eq!(
                control.action_transition_counter,
                if torch == 0 { 6 } else { 0 }
            );
            assert_eq!(
                control.actions,
                if torch == 0 { [128, 255] } else { [0, 0] }
            );
            assert_eq!(
                control.motion_direction,
                if torch == 0 {
                    MotionDirection::WALK_BACKWARD
                } else {
                    MotionDirection::NONE
                }
            );
            assert_eq!(control.torch_burning, torch);
            assert_eq!(control.weapon_drawn, weapon_drawn);
            assert_eq!(world.globals().selected_health().unwrap().displayed(), 43);
            assert_eq!(
                world.inspect(index, 1)["actors"][0]["animation"]["current_group"]
                    == AnimationGroup::FALL.index(),
                torch == 0,
            );
        }
    }
}

#[test]
fn selected_health_kind_one_reset_clears_support_and_directions_without_shell_or_starter() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (scripts, world, mut data, _) = player_health_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let generation = world.generation();
    let mut world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
        saved["actors"][index]["attributes"][105] = 1.into();
        saved["actors"][index]["control"]["living_state"] = 15.into();
        saved["actors"][index]["control"]["motion_direction"] = 7.into();
        saved["actors"][index]["control"]["directional_state"] = serde_json::json!([128, 255]);
        saved["actors"][index]["model"]["ledge_support"] = serde_json::json!([generation, index]);
    });
    let id = world.id(index).unwrap();
    let hud = ScriptHud::default();
    world
        .apply_selected_contact_health(
            id,
            -7,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::from_state(1),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let control = world.actors()[index].control_state().unwrap();
    assert_eq!(control.living_state, LivingState::DROPPING);
    assert_eq!(control.motion_direction, MotionDirection::NONE);
    assert_eq!(control.actions, [0, 0]);
    assert_eq!(control.action_transition_counter, 0);
    assert_eq!(world.globals().selected_health().unwrap().displayed(), 43);
    assert!(
        movement_snapshot(&world, &scripts, &data)["actors"][index]["model"]["ledge_support"]
            .is_null()
    );
}

#[test]
fn selected_health_damage_lets_an_attached_player_go_of_its_rope() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (scripts, world, mut data, _) = player_health_fixture_with(|scripts| {
        scripts.updates.rope = Some(cyrus_assets::script::RopeSettings {
            jump_add: 64,
            attach_angle: 0,
        });
    });
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    data.metadata["ropes"] = serde_json::json!([{
        "position": [0.0, 0.0, 0.0],
        "angle_y": 0,
        "rope_type": 0,
        "swing": 0,
        "speed": 0,
        "length": 8,
        "static_model": "",
        "rope_model": "ROPELINK",
        "unknown1": [0, 0, 0],
    }]);
    let mut world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
        saved["actors"][index]["control"]["rope_attached"] = true.into();
        saved["ropes"] = serde_json::json!([{
            "amplitude": 0,
            "phase": 0,
            "step": 0,
            "push": 0,
            "direction": 0,
            "previous_sum": 0,
            "attachments": [[index, 3, 0]],
        }]);
    });
    assert_eq!(world.ropes()[0].attachments.len(), 1);
    let id = world.id(index).unwrap();
    let hud = ScriptHud::default();
    world
        .apply_selected_contact_health(
            id,
            -7,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::from_state(1),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let control = world.actors()[index].control_state().unwrap();
    assert!(!control.rope_attached);
    assert_eq!(control.jump_selector, 24);
    assert!(world.ropes()[0].attachments.is_empty());
    assert_eq!(world.actors()[index].attributes()[Attribute::Health], 43);
}

#[test]
fn selected_health_refuses_an_attached_rope_without_rope_state() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs};
    let (field, value, boundary) = (
        "rope_attached",
        serde_json::json!(true),
        "rope detach settings unavailable",
    );
    let (scripts, world, mut data, _) = player_health_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let mut world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
        saved["actors"][index]["control"][field] = value;
    });
    let id = world.id(index).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    let error = world
        .apply_selected_contact_health(
            id,
            -7,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains(boundary), "{error:#}");
    assert_eq!(world.actors()[index].attributes()[Attribute::Health], 43);
    assert_eq!(world.globals().selected_health().unwrap().displayed(), 50);
    assert_eq!(hud.health_feedback_ticks(), 25);
}

#[test]
fn hurt_taunt_uses_ordered_selection_without_clock_or_target_distance() {
    use cyrus_script::script::{CombatControlState, ScriptInputs};
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.attributes[Attribute::NumTaunts] = 1;
            actor.combat_taunts = vec![CombatTaunt {
                trigger: TauntType::HURT_BY_OPPONENT,
                label: *b"HURT",
                animation: 0,
            }];
        },
        |saved| {
            saved["actors"][0]["control"]["combat"] =
                serde_json::to_value(CombatControlState::default()).unwrap();
            saved["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([1000000, 0, 0]);
        },
    );
    let id = world.id(0).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let mut expected = cyrus_game::random::EngineRandom::from_state(42);
    expected.draw();
    let error = world
        .request_hurt_combat_taunt(
            id,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| panic!("unexpected sound"),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("playback source unavailable"),
        "{error:#}"
    );
    assert_eq!(random.state(), expected.state());
}
