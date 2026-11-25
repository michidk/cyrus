use super::*;

#[test]
fn use_item_starts_the_use_script_once_per_instance_limit_and_consumes_the_drop_list() {
    check_scripted_item_use(false);
}

#[test]
fn scripted_item_use_preserves_the_rope_association() {
    check_scripted_item_use(true);
}

fn check_scripted_item_use(on_rope: bool) {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::UseItem,
            param_count: 1,
            kind: CallKind::Function,
        });
        let mut shell = scripts.actors["LOGIC"].clone();
        shell.code_origin += 1000;
        shell.header_index += 100;
        scripts.actors.insert("ITEMSCR".into(), shell);
        let call = Operation::ObjectCall {
            call: Call {
                function_id: id,
                function: Native::UseItem,
                target: Object::Player,
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value: 4 }],
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
    let mut asset = inventory_asset(&[(0, 1), (4, 3), (5, 2)]);
    {
        let item = asset.inventory.get_mut(&4).unwrap();
        item.use_script = Some("itemscr".into());
        item.script_instances = 1;
        item.drop_use_items = vec![5];
    }
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    let player = world.selected_player().unwrap();
    world.set_rope_attached_for_test(player, on_rope);
    let map = world.id(0).unwrap();
    let inputs = |hud| ScriptInputs {
        hud: Some(hud),
        shell_execution: Some(false),
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let before = world.actors().len();
    for _ in 0..2 {
        world.globals_mut().end_item_use_attempt();
        world
            .advance(
                map,
                ScriptStage::OrdinaryMain,
                inputs(&hud),
                None,
                None,
                100,
            )
            .unwrap();
    }
    // The instance limit of one stopped the second start; each use dropped one unit of item 5.
    assert_eq!(world.actors().len(), before + 1);
    assert_eq!(world.running_shells("ITEMSCR"), 1);
    assert_eq!(hud.inventory(None).unwrap().counts[&5], 0);
    world
        .update_shells(
            inputs(&hud),
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(world.actors().last().unwrap().failure().is_none());
    assert_eq!(world.selected_player_on_rope(), on_rope);
}

#[test]
fn item_use_torch_transition_requires_an_animation_owner() {
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    for animated in [false, true] {
        for state in [0, 33, 34] {
            let (scripts, animated_world, _, _) = player_walking_fixture_with(|scripts| {
                let reference = scripts.definition.references.len() as u16;
                scripts
                    .definition
                    .references
                    .push(cyrus_soup::Property::ExtraFlag);
                let function_id = scripts.definition.functions.len() as u16;
                scripts.definition.functions.push(Function {
                    name: Native::UseItem,
                    param_count: 1,
                    kind: CallKind::Function,
                });
                scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
                    (
                        0,
                        Step {
                            next: 1,
                            operation: Operation::Assign {
                                destination: Place::Property {
                                    object: Object::Player,
                                    reference,
                                },
                                formula: Formula {
                                    first: Value::Integer { value: state },
                                    rest: vec![],
                                },
                            },
                        },
                    ),
                    (
                        1,
                        Step {
                            next: 2,
                            operation: Operation::ObjectCall {
                                call: Call {
                                    function_id,
                                    function: Native::UseItem,
                                    target: Object::Player,
                                    kind: CallKind::Function,
                                    arguments: vec![Value::Integer { value: 4 }],
                                },
                            },
                        },
                    ),
                    (
                        2,
                        Step {
                            next: 3,
                            operation: Operation::Assign {
                                destination: Place::Flag { index: 0 },
                                formula: Formula {
                                    first: Value::Integer { value: 13 },
                                    rest: vec![],
                                },
                            },
                        },
                    ),
                    (
                        3,
                        Step {
                            next: 4,
                            operation: Operation::End { target: 0 },
                        },
                    ),
                ]);
            });
            let mut world = if animated {
                animated_world
            } else {
                static_model_fixture_session(scripts, None, [10, 20, 30], true).0
            };
            let player = world.selected_player().unwrap();
            assert_eq!(world.animation_frame(player).unwrap().is_some(), animated);
            world.set_rope_attached_for_test(player, true);
            let hud = ScriptHud::default();
            let mut asset = inventory_asset(&[(4, 1), (5, 2)]);
            asset.inventory.get_mut(&4).unwrap().drop_use_items = vec![5];
            hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
            let inputs = ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            };
            let map = world.id(0).unwrap();
            world
                .advance(
                    map,
                    ScriptStage::Body(Thread::Main),
                    inputs,
                    None,
                    None,
                    100,
                )
                .unwrap();
            assert_eq!(world.flags(), [13]);
            let refused = animated && state != 0;
            assert_eq!(
                hud.inventory(None).unwrap().counts[&5],
                if refused { 2 } else { 1 },
                "animation {animated}, action {state}"
            );
            assert_eq!(hud.inventory(None).unwrap().counts[&4], 1);
            assert!(world.selected_player_on_rope());
            assert_eq!(
                world.actors()[player.index]
                    .control_state()
                    .unwrap()
                    .living_state,
                cyrus_script::script::LivingState::from_byte(state as u8)
            );
            // The accepted or refused attempt consumes the latch until input release.
            world
                .advance(
                    map,
                    ScriptStage::Body(Thread::Main),
                    inputs,
                    None,
                    None,
                    100,
                )
                .unwrap();
            assert_eq!(
                hud.inventory(None).unwrap().counts[&5],
                if refused { 2 } else { 1 }
            );
        }
    }
}

#[test]
fn using_the_island_map_item_raises_the_island_map_request_once() {
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
                arguments: vec![Value::Integer { value: 83 }],
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
    hud.configure(
        &inventory_asset(&[(0, 1), (83, 1)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    let map = world.id(0).unwrap();
    assert!(!world.globals_mut().take_island_map_request());
    world
        .advance(
            map,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    assert!(world.globals_mut().take_island_map_request());
    assert!(!world.globals_mut().take_island_map_request());
}

#[test]
fn item_used_tracks_completed_use_through_public_callers_and_snapshots() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    struct Store;
    impl cyrus_game::preferences::PreferenceStore for Store {
        fn load(
            &self,
            _: cyrus_game::preferences::PreferenceKey,
        ) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: cyrus_game::preferences::PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let preferences = Preferences::new(Store);
    let asset = inventory_asset(&[(0, 1), (4, 3), (5, 2)]);
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    assert_eq!(
        run_item_count_native(&ScriptHud::default(), Native::ItemUsed, &[-1]).unwrap(),
        0
    );
    for id in [0, 4, 5] {
        assert_eq!(
            run_item_count_native(&hud, Native::ItemUsed, &[id]).unwrap(),
            0
        );
    }
    for id in [6, -1, 65536] {
        let mut world = item_count_world(Native::ItemUsed, &[id]);
        let error = world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(&format!("ItemUsed({})", id as u32)));
        assert_eq!(world.flags(), [12]);
    }
    for (id, initial) in [(0, 0), (4, 0x7fff_ffff_u32), (4, u32::MAX)] {
        let mut snapshot = serde_json::to_value(hud.snapshot().unwrap()).unwrap();
        snapshot["inventory"]["items"][id.to_string()]["used"] = serde_json::json!(initial);
        let restored = ScriptHud::from_snapshot(
            &asset,
            &preferences,
            serde_json::from_value(snapshot).unwrap(),
        )
        .unwrap();
        assert_eq!(
            run_item_count_native(&restored, Native::ItemUsed, &[id]).unwrap(),
            initial as i32
        );
        let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
            let function_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: Native::UseItem,
                param_count: 1,
                kind: CallKind::Function,
            });
            scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
                Operation::ObjectCall {
                    call: Call {
                        function_id,
                        function: Native::UseItem,
                        target: Object::Player,
                        kind: CallKind::Function,
                        arguments: vec![Value::Integer { value: id }],
                    },
                },
                Operation::Increment {
                    destination: Place::Flag { index: 0 },
                    delta: 1,
                },
                Operation::End { target: 0 },
            ]
            .into_iter()
            .enumerate()
            .map(|(n, operation)| {
                (
                    n as u32,
                    Step {
                        next: n as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
        });
        for _ in 0..2 {
            world
                .advance(
                    world.id(0).unwrap(),
                    ScriptStage::OrdinaryMain,
                    ScriptInputs {
                        hud: Some(&restored),
                        shell_execution: Some(false),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap();
        }
        assert_eq!(
            run_item_count_native(&restored, Native::ItemUsed, &[id]).unwrap(),
            initial.wrapping_add(1) as i32
        );
        let roundtrip =
            ScriptHud::from_snapshot(&asset, &preferences, restored.snapshot().unwrap()).unwrap();
        assert_eq!(
            run_item_count_native(&roundtrip, Native::ItemUsed, &[id]).unwrap(),
            initial.wrapping_add(1) as i32
        );
        run_item_count_native(&roundtrip, Native::RemoveItem, &[id]).unwrap();
        assert_eq!(
            run_item_count_native(&roundtrip, Native::ItemUsed, &[id]).unwrap(),
            initial.wrapping_add(1) as i32
        );
    }
    let mut legacy = serde_json::to_value(hud.snapshot().unwrap()).unwrap();
    legacy["inventory"]["items"]["4"]
        .as_object_mut()
        .unwrap()
        .remove("used");
    let legacy = ScriptHud::from_snapshot(
        &asset,
        &preferences,
        serde_json::from_value(legacy).unwrap(),
    )
    .unwrap();
    assert!(
        format!(
            "{:#}",
            run_item_count_native(&legacy, Native::ItemUsed, &[4]).unwrap_err()
        )
        .contains("no use history")
    );
}
