use super::*;

#[test]
fn burning_held_torch_publishes_tip_light_and_dousing_removes_it() {
    use cyrus_script::script::{WeaponInputs, WorldRestoreContext, WorldSnapshotSources};
    let (mut world, data, scripts) = display_model_fixture("hand", "hilt", [0; 3], false, vec![]);
    let mut random = cyrus_game::random::EngineRandom::default();
    for (held, burning, living, ready, shell, emits) in [
        (Some(45), 89, 0, true, false, true),
        (None, 89, 0, true, false, false),
        (Some(45), 0, 0, true, false, false),
        (Some(45), 89, 33, true, false, false),
        (Some(45), 89, 34, true, false, false),
        (Some(45), 89, 0, false, false, false),
        (Some(45), 89, 0, true, true, false),
    ] {
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        saved["actors"][0]["control"]["held_item"] = serde_json::json!(held);
        saved["actors"][0]["control"]["torch_burning"] = burning.into();
        saved["actors"][0]["control"]["living_state"] = living.into();
        let generation = world.generation() + 1;
        world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation,
                globals: std::mem::take(world.globals_mut()),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let id = world.id(0).unwrap();
        world.refresh_model_presentations(&data, &[id]).unwrap();
        world
            .refresh_actor_torch_attachment(
                id,
                &data,
                WeaponInputs {
                    selected_player: id,
                    item_system_ready: ready,
                    shell_execution: shell,
                },
                &mut random,
            )
            .unwrap();
        let lights = world.held_torch_lights();
        assert_eq!(lights.len(), usize::from(emits));
        if emits {
            let light = &lights[0];
            assert_eq!(light.radius, 2816);
            let attachment = world.display_attachments().last().unwrap();
            assert_eq!(
                light.position,
                attachment.geometry.combat_endpoints(17).unwrap()[1]
            );
            assert!(light.rgb.into_iter().all(|channel| channel > 0));
        }
    }
}

#[test]
fn a_torch_is_lit_burns_down_is_doused_and_relit_from_its_saved_counter() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::UseItem,
            param_count: 1,
            kind: CallKind::Function,
        });
        let call = Operation::ObjectCall {
            call: Call {
                function_id: id,
                function: Native::UseItem,
                target: Object::Player,
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value: 45 }],
            },
        };
        scripts.actors.get_mut("LOGIC").unwrap().program.steps =
            [call, Operation::End { target: 0 }]
                .into_iter()
                .enumerate()
                .map(|(index, operation)| {
                    (
                        index as u32,
                        Step {
                            next: index as u32 + 1,
                            operation,
                        },
                    )
                })
                .collect();
    });
    let hud = cyrus_script::script::ScriptHud::default();
    let mut asset = inventory_asset(&[(45, 1)]);
    asset.inventory.get_mut(&45).unwrap().item_type = cyrus_assets::hud::ItemKind::TORCH;
    asset.torch_time = Some(60);
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    let map = world.id(0).unwrap();
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
    let inputs = || ScriptInputs {
        hud: Some(&hud),
        shell_execution: Some(false),
        nominal_ticks: Some(2),
        frame_scale: Some(256),
        ..Default::default()
    };
    let torch = |world: &ScriptWorld| {
        let control = world.actors()[player.index].control_state().unwrap();
        (
            control.living_state,
            control.torch_burning,
            control.torch_saved,
        )
    };
    let use_item = |world: &mut ScriptWorld| {
        world.globals_mut().end_item_use_attempt();
        world
            .advance(
                map,
                ScriptStage::OrdinaryMain,
                inputs(),
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
    };
    let progress = |world: &mut ScriptWorld| {
        for _ in 0..4 {
            world.run_held_torch(player, inputs(), 2, &mut |_, _| Ok(()));
            world
                .advance(
                    player,
                    ScriptStage::Animation,
                    inputs(),
                    Some(&mut |_, _| Ok(())),
                    None,
                    100,
                )
                .unwrap();
        }
        assert!(
            world.actors()[player.index].failure().is_none(),
            "{:?}",
            world.actors()[player.index].failure()
        );
    };
    use_item(&mut world);
    assert_eq!(
        torch(&world),
        (LivingState::LIGHTING_TORCH, 0, 0),
        "lighting begins"
    );
    progress(&mut world);
    assert_eq!(
        torch(&world),
        (LivingState::ORDINARY, 90, 90),
        "sixty seconds burn for ninety counts"
    );
    world
        .advance_torch_burn(player, inputs(), 2, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(torch(&world).1, 88, "the countdown loses the nominal ticks");
    use_item(&mut world);
    assert_eq!(
        torch(&world).0,
        LivingState::DOUSING_TORCH,
        "any use while burning puts the torch out"
    );
    progress(&mut world);
    assert_eq!(
        torch(&world),
        (LivingState::ORDINARY, 0, 88),
        "the counter is saved when doused"
    );
    use_item(&mut world);
    progress(&mut world);
    assert_eq!(
        torch(&world),
        (LivingState::ORDINARY, 88, 88),
        "relighting restores the saved counter"
    );
}

#[test]
fn goldfinger_holds_the_selected_players_burning_torch_at_a_hundred_instead_of_counting_down() {
    use cyrus_script::script::ScriptInputs;
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let player = world.selected_player().unwrap();
    let counter = |world: &ScriptWorld| {
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .torch_burning
    };
    let retained = || ScriptInputs {
        torch_retained: Some(true),
        ..Default::default()
    };
    // Without the cheat the burn update loses the nominal ticks.
    world.set_selected_torch(40).unwrap();
    world
        .advance_torch_burn(player, ScriptInputs::default(), 2, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(counter(&world), 38);
    // With it the counter becomes exactly 100 and no countdown runs in that update.
    world
        .advance_torch_burn(player, retained(), 2, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(counter(&world), 100);
    // Only a burning torch is refreshed: a zero counter stays zero.
    world.set_selected_torch(0).unwrap();
    world
        .advance_torch_burn(player, retained(), 2, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(counter(&world), 0);
}

#[test]
fn goldfinger_replaces_the_water_branches_and_the_last_tick_of_the_torch_burn() {
    use cyrus_script::script::ScriptInputs;
    let (_, mut world, _, _) = player_walking_fixture_with(|_| {});
    let player = world.selected_player().unwrap();
    let state = |world: &ScriptWorld| {
        let control = world.actors()[player.index].control_state().unwrap();
        (control.living_state, control.torch_burning)
    };
    let burn = |world: &mut ScriptWorld, surface: u8, counter: u8, retained: bool| {
        world
            .set_actor_surface(player, Surface::from_byte(surface))
            .unwrap();
        world.set_selected_torch(counter).unwrap();
        world
            .advance_torch_burn(
                player,
                ScriptInputs {
                    torch_retained: Some(retained),
                    ..Default::default()
                },
                2,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        state(world)
    };
    // Deep water puts an ordinary torch out at once; goldfinger refreshes it instead.
    for surface in [2, 4] {
        assert_eq!(
            burn(&mut world, surface, 40, false).1,
            0,
            "surface {surface}"
        );
        assert_eq!(
            burn(&mut world, surface, 40, true).1,
            100,
            "surface {surface}"
        );
    }
    // Shallow water would start the dousing; goldfinger leaves the torch burning and the
    // living state ordinary.
    for surface in [1, 3] {
        assert_eq!(
            burn(&mut world, surface, 40, true),
            (LivingState::ORDINARY, 100),
            "surface {surface}"
        );
    }
    // On dry ground the last tick of an ordinary torch reaches zero; goldfinger refreshes it.
    assert_eq!(burn(&mut world, 0, 2, false).1, 0);
    assert_eq!(burn(&mut world, 0, 2, true).1, 100);
}

#[test]
fn light_torch_starts_the_lighting_state_and_unlight_torch_waits_for_a_burning_torch() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let mut call = |name: Native| {
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name,
                param_count: 0,
                kind: CallKind::Function,
            });
            Operation::ObjectCall {
                call: Call {
                    function_id: id,
                    function: name,
                    target: Object::Player,
                    kind: CallKind::Function,
                    arguments: vec![],
                },
            }
        };
        // The first update lights the torch and parks the continuation on the dousing call.
        let steps = [
            call(Native::LightTorch),
            Operation::End { target: 2 },
            call(Native::UnlightTorch),
            Operation::End { target: 2 },
        ];
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps
            .into_iter()
            .enumerate()
            .map(|(index, operation)| {
                (
                    index as u32,
                    Step {
                        next: index as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
    });
    let hud = cyrus_script::script::ScriptHud::default();
    let mut asset = inventory_asset(&[(45, 1)]);
    asset.inventory.get_mut(&45).unwrap().item_type = cyrus_assets::hud::ItemKind::TORCH;
    asset.torch_time = Some(60);
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    let map = world.id(0).unwrap();
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
    let inputs = || ScriptInputs {
        hud: Some(&hud),
        shell_execution: Some(false),
        nominal_ticks: Some(2),
        frame_scale: Some(256),
        ..Default::default()
    };
    let torch = |world: &ScriptWorld| {
        let control = world.actors()[player.index].control_state().unwrap();
        (control.living_state, control.torch_burning)
    };
    let run = |world: &mut ScriptWorld| {
        world
            .advance(
                map,
                ScriptStage::OrdinaryMain,
                inputs(),
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        assert!(world.actors()[map.index].failure().is_none());
    };
    run(&mut world);
    assert_eq!(
        torch(&world),
        (LivingState::LIGHTING_TORCH, 0),
        "LightTorch begins the lighting state"
    );
    // The torch is still lighting: dousing is not admitted, so UnlightTorch completes and changes nothing.
    run(&mut world);
    assert_eq!(
        torch(&world),
        (LivingState::LIGHTING_TORCH, 0),
        "UnlightTorch leaves a lighting torch alone"
    );
}
