use super::*;

#[test]
fn potion_counters_start_zero_keep_values_per_index_and_reject_unknown_indices() {
    assert_eq!(run_native_calls(&[(Native::GetPotion, &[2])]).unwrap(), 0);
    assert_eq!(
        run_native_calls(&[(Native::SetPotion, &[2, 7])]).unwrap(),
        1
    );
    assert_eq!(
        run_native_calls(&[(Native::SetPotion, &[2, 7]), (Native::GetPotion, &[2])]).unwrap(),
        7
    );
    assert_eq!(
        run_native_calls(&[(Native::SetPotion, &[2, 7]), (Native::GetPotion, &[3])]).unwrap(),
        0
    );
    for call in [
        (Native::GetPotion, &[4][..]),
        (Native::SetPotion, &[-1, 1][..]),
    ] {
        let error = run_native_calls(&[call]).unwrap_err();
        assert!(format!("{error:#}").contains("outside 0..4"), "{error:#}");
    }
}

fn sub_item_world(item: i32, amount: i32) -> ScriptWorld {
    item_count_world(Native::SubItem, &[item, amount])
}

pub(super) fn run_item_count_native(
    hud: &cyrus_script::script::ScriptHud,
    native: Native,
    arguments: &[i32],
) -> anyhow::Result<i32> {
    let mut world = item_count_world(native, arguments);
    world.advance(
        world.id(0).unwrap(),
        ScriptStage::Body(Thread::Main),
        cyrus_script::script::ScriptInputs {
            hud: Some(hud),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    assert_eq!(
        world.flags(),
        [13],
        "successful native continues its caller"
    );
    world.actors()[0].runtime().unwrap().variables()[0].integer()
}

#[test]
fn pushed_item_counts_restore_once_and_preserve_other_inventory_state() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::ScriptHud;
    let hud = ScriptHud::default();
    let mut asset = inventory_asset(&[(0, 1), (4, 5), (5, 3)]);
    asset.selected = Some(4);
    let item = asset.inventory.get_mut(&4).unwrap();
    item.flags = 1;
    item.remove_drop_items = vec![5];
    hud.configure(&asset, &Preferences::default());
    let mut expected = serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
    for (native, arguments, result, count, pushed, saved) in [
        (Native::PopItem, vec![4], 5, 5_u32, None, None),
        (Native::PushItem, vec![4, 0], 0, 0, Some(true), Some(5)),
        (
            Native::PushItem,
            vec![4, i32::MIN],
            i32::MIN,
            0x8000_0000,
            Some(true),
            Some(5),
        ),
        (Native::PopItem, vec![4], 5, 5, Some(false), Some(5)),
        (Native::SubItem, vec![4, 2], 0, 3, Some(false), Some(5)),
        (Native::PopItem, vec![4], 3, 3, Some(false), Some(5)),
        (
            Native::PushItem,
            vec![4, -1],
            -1,
            u32::MAX,
            Some(true),
            Some(3),
        ),
        (Native::RemoveItem, vec![4], 0, 0, Some(true), Some(3)),
        (Native::PopItem, vec![4], 3, 3, Some(false), Some(3)),
    ] {
        assert_eq!(
            run_item_count_native(&hud, native, &arguments).unwrap(),
            result
        );
        expected["items"]["4"]["count"] = serde_json::json!(count);
        if let (Some(pushed), Some(saved)) = (pushed, saved) {
            expected["saved_counts"]["4"] = serde_json::json!({"value": saved, "pushed": pushed});
        }
        assert_eq!(
            serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn pushed_item_counts_skip_closed_lookup_and_fail_before_continuation_when_open() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::ScriptHud;
    for (native, arguments) in [
        (Native::PushItem, vec![-1, -1]),
        (Native::PopItem, vec![-1]),
    ] {
        assert_eq!(
            run_item_count_native(&ScriptHud::default(), native, &arguments).unwrap(),
            0
        );
    }
    for item in [6, -1, 65536] {
        for native in [Native::PushItem, Native::PopItem] {
            let hud = ScriptHud::default();
            hud.configure(&inventory_asset(&[(4, 5)]), &Preferences::default());
            let before = serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
            let arguments = if native == Native::PushItem {
                vec![item, 0]
            } else {
                vec![item]
            };
            let mut world = item_count_world(native, &arguments);
            let error = world
                .advance(
                    world.id(0).unwrap(),
                    ScriptStage::Body(Thread::Main),
                    cyrus_script::script::ScriptInputs {
                        hud: Some(&hud),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap_err();
            assert!(format!("{error:#}").contains(&format!("{native}({})", item as u32)));
            assert_eq!(world.flags(), [12], "failed native does not continue");
            assert_eq!(
                serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn pushed_item_counts_survive_hud_restoration_and_validate_saved_identifiers() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::{Inventory, ScriptHud};
    let asset = inventory_asset(&[(4, 5)]);
    struct EmptyStore;
    impl cyrus_game::preferences::PreferenceStore for EmptyStore {
        fn load(
            &self,
            _: cyrus_game::preferences::PreferenceKey,
        ) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: cyrus_game::preferences::PreferenceKey, _: &str) -> anyhow::Result<()> {
            panic!("count operations must not write preferences")
        }
    }
    let preferences = Preferences::new(EmptyStore);
    let hud = ScriptHud::default();
    hud.configure(&asset, &preferences);
    run_item_count_native(&hud, Native::PushItem, &[4, 0]).unwrap();
    let restored = ScriptHud::from_snapshot(&asset, &preferences, hud.snapshot().unwrap()).unwrap();
    assert_eq!(
        run_item_count_native(&restored, Native::PopItem, &[4]).unwrap(),
        5
    );
    let mut snapshot = serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
    snapshot["saved_counts"]["6"] = serde_json::json!({"value": 7, "pushed": true});
    let error = Inventory::from_snapshot(&asset, serde_json::from_value(snapshot).unwrap())
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("saved count item 6"));
    let mut legacy = serde_json::to_value(restored.snapshot().unwrap()).unwrap();
    legacy["inventory"]
        .as_object_mut()
        .unwrap()
        .remove("saved_counts");
    let restored = ScriptHud::from_snapshot(
        &asset,
        &preferences,
        serde_json::from_value(legacy).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run_item_count_native(&restored, Native::PopItem, &[4]).unwrap(),
        5
    );
}

#[test]
fn pushed_item_counts_control_use_eligibility_without_changing_selection() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let make_world = || {
        player_walking_fixture_with(|scripts| {
            let id = scripts.definition.functions.len() as u16;
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
                        operation: Operation::ObjectCall {
                            call: Call {
                                function_id: id,
                                function: Native::UseItem,
                                target: Object::Player,
                                kind: CallKind::Function,
                                arguments: vec![Value::Integer { value: 4 }],
                            },
                        },
                    },
                ),
                (
                    1,
                    Step {
                        next: 2,
                        operation: Operation::Increment {
                            destination: Place::Flag { index: 0 },
                            delta: 1,
                        },
                    },
                ),
                (
                    2,
                    Step {
                        next: 3,
                        operation: Operation::End { target: 0 },
                    },
                ),
            ]);
        })
    };
    for (replacement, initially_owned) in [
        (0, false),
        (i32::MIN, false),
        (-1, false),
        (i32::MAX, true),
        (1, true),
    ] {
        let asset = inventory_asset(&[(0, 1), (4, 5)]);
        let hud = ScriptHud::default();
        hud.configure(&asset, &Preferences::default());
        assert_eq!(
            run_item_count_native(&hud, Native::PushItem, &[4, replacement]).unwrap(),
            replacement
        );
        for restored in [false, true] {
            let owned = restored || initially_owned;
            if restored {
                assert_eq!(
                    run_item_count_native(&hud, Native::PopItem, &[4]).unwrap(),
                    5
                );
            }
            let mut before = serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
            let (_, mut world, _, _) = make_world();
            let result = world.advance(
                world.id(0).unwrap(),
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
            );
            if owned {
                result.unwrap();
                assert_eq!(world.flags(), [13]);
                let used = before["items"]["4"]["used"].as_u64().unwrap() + 1;
                before["items"]["4"]["used"] = serde_json::json!(used);
            } else {
                assert!(format!("{:#}", result.unwrap_err()).contains("item not owned"));
                assert_eq!(world.flags(), [12]);
            }
            assert_eq!(hud.active_item().unwrap(), 0);
            assert_eq!(
                serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn push_all_items_replaces_consecutive_counts_without_replacing_saved_words() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::ScriptHud;
    let hud = ScriptHud::default();
    let mut asset = inventory_asset(&[(0, 1), (1, 5), (2, 3)]);
    asset.selected = Some(2);
    asset.inventory.get_mut(&1).unwrap().remove_drop_items = vec![2];
    hud.configure(&asset, &Preferences::default());
    let mut expected = serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
    for replacement in [0, i32::MIN, -1] {
        assert_eq!(
            run_item_count_native(&hud, Native::PushAllItems, &[replacement]).unwrap(),
            0
        );
        for (id, saved) in [(0, 1), (1, 5), (2, 3)] {
            let key = id.to_string();
            expected["items"][&key]["count"] = serde_json::json!(replacement as u32);
            expected["saved_counts"][&key] = serde_json::json!({"value": saved, "pushed": true});
        }
        assert_eq!(
            serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap(),
            expected
        );
    }
    for (id, saved) in [(0, 1), (1, 5), (2, 3)] {
        assert_eq!(
            run_item_count_native(&hud, Native::PopItem, &[id]).unwrap(),
            saved
        );
    }
    assert_eq!(
        hud.inspect()["inventory"]["counts"],
        serde_json::json!({"0":1,"1":5,"2":3})
    );
    assert_eq!(hud.active_item().unwrap(), 2);
}

#[test]
fn push_all_items_skips_closed_and_empty_inventories_but_retains_partial_failure() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::{ScriptHud, ScriptInputs};
    let closed = ScriptHud::default();
    assert_eq!(
        run_item_count_native(&closed, Native::PushAllItems, &[-1]).unwrap(),
        0
    );
    let empty = ScriptHud::default();
    empty.configure(&inventory_asset(&[]), &Preferences::default());
    let before = serde_json::to_value(empty.snapshot_inventory().unwrap()).unwrap();
    assert_eq!(
        run_item_count_native(&empty, Native::PushAllItems, &[-1]).unwrap(),
        0
    );
    assert_eq!(
        serde_json::to_value(empty.snapshot_inventory().unwrap()).unwrap(),
        before
    );
    let sparse = ScriptHud::default();
    sparse.configure(&inventory_asset(&[(0, 5), (2, 3)]), &Preferences::default());
    let mut expected = serde_json::to_value(sparse.snapshot_inventory().unwrap()).unwrap();
    expected["items"]["0"]["count"] = serde_json::json!(0);
    expected["saved_counts"]["0"] = serde_json::json!({"value":5,"pushed":true});
    let mut world = item_count_world(Native::PushAllItems, &[0]);
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                hud: Some(&sparse),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("PushItem(1): ITEM.INI definition absent"));
    assert_eq!(world.flags(), [12], "failed wrapper must not continue");
    assert_eq!(
        serde_json::to_value(sparse.snapshot_inventory().unwrap()).unwrap(),
        expected
    );
}

#[test]
fn pop_all_items_restores_consecutive_counts_and_keeps_an_existing_selection() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::ScriptHud;
    let hud = ScriptHud::default();
    let mut asset = inventory_asset(&[(0, 1), (1, 5), (2, 3)]);
    asset.selected = Some(2);
    hud.configure(&asset, &Preferences::default());
    // The snapshot is read because the display view repairs a selected item whose count is zero.
    let snapshot = || serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
    let before = snapshot();
    assert_eq!(
        run_item_count_native(&hud, Native::PushAllItems, &[0]).unwrap(),
        0
    );
    assert_eq!(snapshot()["items"]["1"]["count"], 0);
    assert_eq!(
        run_item_count_native(&hud, Native::PopAllItems, &[]).unwrap(),
        0
    );
    let after = snapshot();
    for id in ["0", "1", "2"] {
        assert_eq!(
            after["items"][id]["count"], before["items"][id]["count"],
            "item {id} is restored"
        );
    }
    assert_eq!(after["selected"], 2, "an existing selection is kept");
    assert_eq!(
        run_item_count_native(&hud, Native::PopAllItems, &[]).unwrap(),
        0,
        "a second pop restores nothing more"
    );
    assert_eq!(snapshot()["items"], after["items"]);
}

#[test]
fn pop_all_items_skips_closed_and_empty_inventories_but_retains_partial_failure() {
    use cyrus_game::preferences::Preferences;
    use cyrus_script::script::{ScriptHud, ScriptInputs, ScriptStage};
    let closed = ScriptHud::default();
    assert_eq!(
        run_item_count_native(&closed, Native::PopAllItems, &[]).unwrap(),
        0
    );
    let empty = ScriptHud::default();
    empty.configure(&inventory_asset(&[]), &Preferences::default());
    let before = serde_json::to_value(empty.snapshot_inventory().unwrap()).unwrap();
    assert_eq!(
        run_item_count_native(&empty, Native::PopAllItems, &[]).unwrap(),
        0
    );
    assert_eq!(
        serde_json::to_value(empty.snapshot_inventory().unwrap()).unwrap(),
        before
    );
    let sparse = ScriptHud::default();
    sparse.configure(&inventory_asset(&[(0, 5), (2, 3)]), &Preferences::default());
    run_item_count_native(&sparse, Native::PushItem, &[0, 0]).unwrap();
    assert_eq!(sparse.inventory(None).unwrap().counts[&0], 0);
    let mut world = item_count_world(Native::PopAllItems, &[]);
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                hud: Some(&sparse),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("PopItem(1): ITEM.INI definition absent"));
    assert_eq!(world.flags(), [12], "failed wrapper must not continue");
    assert_eq!(
        sparse.inventory(None).unwrap().counts[&0],
        5,
        "the earlier restoration is retained"
    );
}

#[test]
fn pop_all_items_selects_item_one_and_assigns_the_player_weapon_when_nothing_is_selected() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        // Dropping everything leaves nothing selected for the pop to repair.
        let mut steps = Vec::new();
        for name in [Native::DropAllItems, Native::PopAllItems] {
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name,
                param_count: 0,
                kind: CallKind::Function,
            });
            steps.push(assign_call(steps.len() as u32 + 1, id, name, &[]).operation);
        }
        steps.push(Operation::End { target: 0 });
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
    hud.configure(
        &inventory_asset(&[(0, 0), (1, 0), (2, 4)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            Some(&mut cyrus_game::random::EngineRandom::default()),
            100,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
    assert_eq!(
        serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap()["selected"],
        1,
        "item one is selected even though its count is zero"
    );
    let player = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    assert_eq!(world.actors()[player].attributes()[Attribute::Weapon], 1);
}

pub(super) fn item_count_world(native: Native, arguments: &[i32]) -> ScriptWorld {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: native,
        param_count: arguments.len() as u8,
        kind: CallKind::Function,
    });
    let function_id = scripts.definition.functions.len() as u16 - 1;
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (0, assign_call(1, function_id, native, arguments)),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Increment {
                    destination: Place::Flag { index: 0 },
                    delta: 1,
                },
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    ScriptWorld::prepare(&scripts, 1).unwrap()
}

#[test]
fn sub_item_wraps_signed_counts_without_drop_effects_or_selection_changes() {
    use cyrus_script::script::{ScriptHud, ScriptInputs, ScriptStage};
    for (count, amount, expected) in [
        (5, 0, 5_u32),
        (5, 2, 3),
        (5, -2, 3),
        (5, 20, 0),
        (0, -20, 0),
        (5, i32::MIN, 0x8000_0005),
        (0, i32::MIN, 0x8000_0000),
        (0x7fff_ffff, i32::MIN, 0xffff_ffff),
        (0xffff_ffff, 2, 0),
        (0x8000_0000, 0, 0),
    ] {
        let hud = ScriptHud::default();
        let mut asset = inventory_asset(&[(0, 1), (4, count), (5, 3)]);
        asset.selected = Some(4);
        let item = asset.inventory.get_mut(&4).unwrap();
        item.flags = 1;
        item.remove_drop_items = vec![5];
        hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
        let mut expected_snapshot =
            serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
        expected_snapshot["items"]["4"]["count"] = serde_json::json!(expected);
        let mut world = sub_item_world(4, amount);
        world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(cyrus_soup::Thread::Main),
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
        assert_eq!(world.flags(), [13]);
        assert_eq!(
            serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap(),
            expected_snapshot
        );
    }
}

#[test]
fn sub_item_skips_disabled_lookup_and_rejects_missing_items_before_continuation() {
    use cyrus_script::script::{ScriptHud, ScriptInputs, ScriptStage};
    let disabled = ScriptHud::default();
    let mut world = sub_item_world(-1, i32::MIN);
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(cyrus_soup::Thread::Main),
            ScriptInputs {
                hud: Some(&disabled),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
    assert_eq!(world.flags(), [13]);
    for item in [6, -1, 65536] {
        let hud = ScriptHud::default();
        hud.configure(
            &inventory_asset(&[(0, 1), (4, 5)]),
            &cyrus_game::preferences::Preferences::default(),
        );
        let before = serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap();
        let mut world = sub_item_world(item, 2);
        let error = world
            .advance(
                world.id(0).unwrap(),
                ScriptStage::Body(cyrus_soup::Thread::Main),
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(&format!("SubItem({})", item as u32)));
        assert_eq!(world.flags(), [12]);
        assert_eq!(
            serde_json::to_value(hud.snapshot_inventory().unwrap()).unwrap(),
            before
        );
    }
}

pub(super) fn inventory_asset(counts: &[(u16, u32)]) -> cyrus_assets::hud::HudAsset {
    cyrus_assets::hud::HudAsset {
        labels: Default::default(),
        images: vec![],
        candle_mode: 0,
        candle: [0; 2],
        candle_size: [0; 2],
        pickup: [0; 2],
        prompt: [0; 2],
        override_ticks: 0,
        compass: [0; 2],
        compass_size: [0; 2],
        world_compass: Default::default(),
        powerup_slots: [[0; 2]; 2],
        powerup_sizes: vec![],
        logbook: [0; 2],
        logbook_size: [0; 2],
        walk_mode: false,
        auto_defend: false,
        notices: Default::default(),
        inventory: counts
            .iter()
            .map(|&(id, count)| {
                (
                    id,
                    cyrus_assets::hud::InventoryItem {
                        count,
                        total: 100,
                        player_max: 100,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        start_items: vec![],
        items: vec![],
        selected: None,
        bindings: [0; 15],
        font: Default::default(),
        text_style: [0; 2].into(),
        player_text_style: [0; 2].into(),
        used_text_style: [0; 2].into(),
        torch_time: None,
        inventory_screen: Default::default(),
        island_map: Default::default(),
    }
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_inventory_export_retains_raw_flags_and_rejects_incomplete_snapshots() {
    use cyrus_script::script::{InventorySnapshot, ScriptHud};
    use rgpre::import::savegame::parse_save_inventory;
    let asset = inventory_asset(&[(2, 3), (7, 4)]);
    let hud = ScriptHud::default();
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    let snapshot = hud.snapshot_inventory().unwrap();
    assert!(
        snapshot
            .export_original(&asset)
            .unwrap_err()
            .to_string()
            .contains("metadata absent")
    );
    let mut value = serde_json::to_value(snapshot).unwrap();
    value["original_encoding"] = serde_json::json!({
        "flags": {"2": {"hide": 129, "pushed": 254}}, "selected": -1
    });
    value["selected"] = serde_json::Value::Null;
    value["items"]["2"]["hidden"] = true.into();
    value["items"]["2"]["used"] = u32::MAX.into();
    value["saved_counts"]["2"] = serde_json::json!({"value": 23, "pushed": true});
    let export = |value: serde_json::Value| {
        serde_json::from_value::<InventorySnapshot>(value)
            .unwrap()
            .export_original(&asset)
    };
    let bytes = export(value.clone()).unwrap();
    let decoded = parse_save_inventory(&bytes, 1).unwrap();
    let item = decoded.items[0].state();
    assert_eq!((item.hide, item.pushed, item.player_total), (129, 254, 3));
    assert_eq!((item.item_used, item.saved_player_total), (u32::MAX, 23));
    assert_eq!(decoded.selected_item, -1);
    let mut missing = value.clone();
    missing["items"]["2"]["used"] = serde_json::Value::Null;
    assert!(
        export(missing)
            .unwrap_err()
            .to_string()
            .contains("use counter absent")
    );
    let mut missing = value.clone();
    missing["saved_counts"] = serde_json::json!({});
    assert!(
        export(missing)
            .unwrap_err()
            .to_string()
            .contains("push history absent")
    );
    let mut different = value.clone();
    different["original_encoding"]["flags"] = serde_json::json!({"7": {"hide":0,"pushed":0}});
    assert!(
        export(different)
            .unwrap_err()
            .to_string()
            .contains("census differs")
    );
    let mut unsupported_count = value.clone();
    unsupported_count["items"]["2"]["count"] = u32::MAX.into();
    assert!(
        export(unsupported_count)
            .unwrap_err()
            .to_string()
            .contains("player_total exceeds supported signed range")
    );
    let mut different_source = value.clone();
    different_source["definitions"]["2"]["total"] = 101.into();
    let error = format!("{:#}", export(different_source).unwrap_err());
    assert!(
        error.contains("SAVEGAME.SAV SVIT")
            && error.contains("item 2")
            && error.contains("installed definition differs"),
        "{error}"
    );
    let mut unsupported = value.clone();
    unsupported["items"]["2"]["hidden"] = false.into();
    assert!(
        export(unsupported)
            .unwrap_err()
            .to_string()
            .contains("noncanonical hide")
    );
    let mut unsupported = value;
    unsupported["original_encoding"]["selected"] = i32::MAX.into();
    assert!(
        export(unsupported)
            .unwrap_err()
            .to_string()
            .contains("selection")
    );
}
