use super::*;

#[test]
fn the_action_key_draws_the_selected_sabre_once_and_defers_to_an_activation_owner() {
    use cyrus_script::script::{QuickKeys, ScriptInputs};
    let draw = |owner: bool| {
        let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
        let player = world
            .id(world
                .actors()
                .iter()
                .position(|actor| {
                    matches!(
                        actor.source,
                        cyrus_script::script::ScriptActorSource::Player
                    )
                })
                .unwrap())
            .unwrap();
        let hud = cyrus_script::script::ScriptHud::default();
        let mut asset = inventory_asset(&[(1, 1)]);
        asset.inventory.get_mut(&1).unwrap().item_type = cyrus_assets::hud::ItemKind::WEAPON;
        asset.inventory.get_mut(&1).unwrap().weapon = true;
        hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
        if owner {
            world.set_activation_owner_for_test(Some(world.id(0).unwrap()));
        }
        let inputs = ScriptInputs {
            hud: Some(&hud),
            shell_execution: Some(false),
            quick_keys: Some(QuickKeys {
                sword: false,
                health: false,
                use_selected: true,
            }),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::default();
        world
            .run_quick_keys(inputs, &mut random, &mut |_, _| Ok(()))
            .unwrap();
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .living_state
    };
    assert_eq!(
        draw(false),
        LivingState::DRAWING_SWORD,
        "the selected sabre is drawn"
    );
    assert_eq!(
        draw(true),
        LivingState::ORDINARY,
        "an activation owner keeps the key"
    );
}

#[test]
fn ending_activation_keeps_the_action_press_consumed_until_release() {
    use cyrus_script::script::{
        ActivationActor, ActivationAnimation, ActivationInputs, QuickKeys, ScriptInputs,
    };
    for deactivate in [false, true] {
        for item in [1, 4] {
            let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
                let activate = scripts.definition.functions.len() as u16;
                scripts.definition.functions.push(Function {
                    name: Native::Activate,
                    param_count: 1,
                    kind: CallKind::Function,
                });
                scripts.definition.functions.push(Function {
                    name: Native::Deactivate,
                    param_count: 0,
                    kind: CallKind::Function,
                });
                let mut operations = vec![
                    assign_call(
                        1,
                        activate,
                        Native::Activate,
                        &[i32::from_le_bytes(*b"TEST")],
                    )
                    .operation,
                ];
                if deactivate {
                    operations.push(Operation::Call {
                        call: Call {
                            function_id: activate + 1,
                            function: Native::Deactivate,
                            target: Object::Me,
                            kind: CallKind::Function,
                            arguments: vec![],
                        },
                    });
                }
                operations.push(Operation::End { target: 0 });
                scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
                    .into_iter()
                    .enumerate()
                    .map(|(i, operation)| {
                        (
                            i as u32,
                            Step {
                                next: i as u32 + 1,
                                operation,
                            },
                        )
                    })
                    .collect();
            });
            let player = world
                .id(world
                    .actors()
                    .iter()
                    .position(|a| {
                        matches!(a.source, cyrus_script::script::ScriptActorSource::Player)
                    })
                    .unwrap())
                .unwrap();
            let owner = world.id(0).unwrap();
            let hud = cyrus_script::script::ScriptHud::default();
            let mut asset = inventory_asset(&[(item, 1)]);
            let definition = asset.inventory.get_mut(&item).unwrap();
            definition.flags = 4;
            if item == 1 {
                definition.item_type = cyrus_assets::hud::ItemKind::WEAPON;
                definition.weapon = true;
            }
            hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
            hud.select_item(item).unwrap();
            let state = ActivationActor {
                state: LivingState::ORDINARY,
                on_rope: false,
                water_state: Surface::UNKNOWN,
                torch_burning: 0,
                weapon_assigned: true,
                weapon_drawn: false,
                animation: Some(ActivationAnimation {
                    group: Some(AnimationGroup::PANIC),
                }),
            };
            let activation = ActivationInputs {
                player,
                actors: BTreeMap::from([(owner.index, state), (player.index, state)]),
                labels: BTreeMap::from([("test".into(), "OPEN".into())]),
                lockout: 0,
                dialogue_open: false,
                lineup_result: 0,
                use_held: true,
                item_system_ready: true,
                shell_execution: false,
                render_disabled: false,
            };
            world
                .advance(
                    owner,
                    ScriptStage::OrdinaryMain,
                    ScriptInputs {
                        activation: Some(&activation),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap();
            assert_eq!(
                world.actors()[owner.index].runtime().unwrap().variables()[0],
                1
            );
            assert_eq!(
                world.activation().owner(),
                None,
                "the script released its owner"
            );
            let mut random = cyrus_game::random::EngineRandom::default();
            for (held, use_selected) in [(true, true), (true, true), (false, false)] {
                world.globals_mut().end_item_use_attempt();
                world
                    .run_quick_keys(
                        ScriptInputs {
                            hud: Some(&hud),
                            shell_execution: Some(false),
                            activation_use_held: Some(held),
                            quick_keys: Some(QuickKeys {
                                use_selected,
                                ..Default::default()
                            }),
                            nominal_ticks: Some(1),
                            frame_scale: Some(256),
                            ..Default::default()
                        },
                        &mut random,
                        &mut |_, _| Ok(()),
                    )
                    .unwrap();
                assert_eq!(
                    world.actors()[player.index]
                        .control_state()
                        .unwrap()
                        .living_state,
                    LivingState::ORDINARY,
                    "activation must not draw the selected sword"
                );
                assert_eq!(
                    hud.inventory(None).unwrap().counts[&item],
                    1,
                    "activation must not consume the selected item"
                );
            }
            world
                .run_quick_keys(
                    ScriptInputs {
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        activation_use_held: Some(true),
                        quick_keys: Some(QuickKeys {
                            use_selected: true,
                            ..Default::default()
                        }),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |_, _| Ok(()),
                )
                .unwrap();
            assert_eq!(
                hud.inventory(None).unwrap().counts[&item],
                0,
                "a new press may use the selected item"
            );
            assert_eq!(
                world.actors()[player.index]
                    .control_state()
                    .unwrap()
                    .living_state,
                if item == 1 {
                    LivingState::DRAWING_SWORD
                } else {
                    LivingState::ORDINARY
                }
            );
        }
    }
}

#[test]
fn compass_item_use_is_not_repeated_by_the_hud_for_the_same_press() {
    use cyrus_script::script::{CompassUse, QuickKeys, ScriptInputs};
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(0, 1)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    hud.select_item(0).unwrap();
    let inputs = ScriptInputs {
        hud: Some(&hud),
        shell_execution: Some(false),
        quick_keys: Some(QuickKeys {
            sword: false,
            health: false,
            use_selected: true,
        }),
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let mut random = cyrus_game::random::EngineRandom::default();
    for expected in [true, false, true] {
        world.globals_mut().end_item_use_attempt();
        world
            .run_quick_keys(inputs, &mut random, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            hud.inspect()["compass_on"],
            expected,
            "item use toggles once"
        );
        for _ in 0..3 {
            assert_eq!(
                hud.compass_input(true, CompassUse::Accepted),
                expected,
                "the HUD must not repeat an accepted item use while held"
            );
        }
        assert_eq!(hud.compass_input(false, CompassUse::Blocked), expected);
    }
    assert_eq!(hud.inventory(None).unwrap().counts[&0], 1);
}

#[test]
fn the_action_key_does_not_use_the_selected_item_on_a_rope() {
    use cyrus_script::script::{QuickKeys, ScriptInputs};
    let use_item = |rope: bool| {
        let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
        let player = world
            .id(world
                .actors()
                .iter()
                .position(|actor| {
                    matches!(
                        actor.source,
                        cyrus_script::script::ScriptActorSource::Player
                    )
                })
                .unwrap())
            .unwrap();
        let hud = cyrus_script::script::ScriptHud::default();
        // Flag bit four empties the item once it is used, which makes a use observable.
        let mut asset = inventory_asset(&[(4, 1)]);
        asset.inventory.get_mut(&4).unwrap().flags = 4;
        hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
        hud.select_item(4).unwrap();
        world.set_attribute_for_test(player, Attribute::Health, 60);
        world.globals_mut().initialize_selected_health_for_test(60);
        world.set_rope_attached_for_test(player, rope);
        let inputs = ScriptInputs {
            hud: Some(&hud),
            shell_execution: Some(false),
            quick_keys: Some(QuickKeys {
                sword: false,
                health: false,
                use_selected: true,
            }),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::default();
        world
            .run_quick_keys(inputs, &mut random, &mut |_, _| Ok(()))
            .unwrap();
        hud.inventory(None).unwrap().counts[&4]
    };
    assert_eq!(use_item(false), 0, "the selected potion is used up");
    assert_eq!(use_item(true), 1, "a player on a rope does not use it");
}

#[test]
fn removed_pickup_revisit_skips_ordinary_calls_without_repeating_inventory_awards() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::{ScriptActors, ScriptHud, ScriptInputs, WorldSnapshotSources};
    use cyrus_world::lifecycle::PreparedWorld;
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: Native::AddItem,
            param_count: 2,
            kind: CallKind::Function,
        },
        Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        },
    ]);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.steps = [
        wait(
            1,
            Native::AddItem,
            CallKind::Function,
            vec![Value::Integer { value: 4 }, Value::Integer { value: 1 }],
        ),
        wait(2, Native::KillMe, CallKind::Function, vec![]),
        Operation::End { target: 0 },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, operation)| {
        (
            i as u32,
            Step {
                next: i as u32 + 1,
                operation,
            },
        )
    })
    .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut app, prepared, animations) = revisit_test_app(&scripts);
    let hud = ScriptHud::default();
    hud.configure(&inventory_asset(&[(4, 0)]), &Preferences::default());
    app.insert_resource(prepared.clone());
    app.update();
    let call = |world: &mut ScriptWorld, index| {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
    };
    let old = {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        call(world, 0);
        assert!(world.actors()[0].control_state().unwrap().killed);
        world.id(0).unwrap()
    };
    assert_eq!(hud.inventory(None).unwrap().counts[&4], 1);
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    let mut other_scripts: Scripts =
        serde_json::from_value(serde_json::to_value(&scripts).unwrap()).unwrap();
    other_scripts.world = 8;
    other_scripts.content_id = other_scripts.content_identity().unwrap();
    let mut other_animations: cyrus_assets::animation::Animations =
        serde_json::from_value(serde_json::to_value(&animations).unwrap()).unwrap();
    other_animations.world = 8;
    other_animations.metadata["script_content_id"] = other_scripts.content_id.clone().into();
    let other_scripts = app
        .world_mut()
        .resource_mut::<bevy::prelude::Assets<Scripts>>()
        .add(other_scripts);
    let other_animations = app
        .world_mut()
        .resource_mut::<bevy::prelude::Assets<cyrus_assets::animation::Animations>>()
        .add(other_animations);
    let mut other_prepared = prepared.clone();
    other_prepared.world = Some(8);
    other_prepared.requested_name = "OTHER".into();
    other_prepared.scripts = other_scripts;
    other_prepared.animations = other_animations;
    app.insert_resource(other_prepared);
    app.update();
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .world,
        8
    );
    assert_eq!(hud.inventory(None).unwrap().counts[&4], 1);
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    app.insert_resource(prepared);
    app.update();
    let collision = cyrus_assets::collision::CollisionScene::default();
    let mut actors = app.world_mut().resource_mut::<ScriptActors>();
    assert!(actors.has_pending_revisit());
    assert!(
        actors
            .restore_visited(
                &WorldSnapshotSources {
                    identity_memo: None,
                    effects: None,
                    player: None,
                    scripts: &scripts,
                    animations: &animations,
                    collision: &collision
                },
                &mut |_, _| Ok(())
            )
            .unwrap()
    );
    let world = actors.current.as_mut().unwrap();
    assert!(
        world
            .advance(
                old,
                ScriptStage::OrdinaryMain,
                ScriptInputs::default(),
                None,
                None,
                100
            )
            .is_err()
    );
    for _ in 0..3 {
        call(world, 0);
    }
    assert_eq!(hud.inventory(None).unwrap().counts[&4], 1);
    call(world, 1);
    assert_eq!(hud.inventory(None).unwrap().counts[&4], 2);
    assert!(world.actors()[1].control_state().unwrap().killed);
    for index in 0..2 {
        call(world, index);
    }
    assert_eq!(hud.inventory(None).unwrap().counts[&4], 2);
}

#[test]
fn removed_pickup_ordinary_hook_finishes_admitted_iteration_but_not_the_next() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::KillMe,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
        function_id: 1,
        function: Native::KillMe,
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![],
    });
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (mut world, data, _) = static_model_fixture(scripts);
    for expected in [5, 5] {
        world
            .update_ordinary(
                &data,
                [0; 2],
                cyrus_script::script::OrdinaryInputs {
                    scene: None,
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [expected],
            "{}",
            world.inspect(0, 1)
        );
        assert!(world.actors()[0].control_state().unwrap().killed);
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
    }
}
