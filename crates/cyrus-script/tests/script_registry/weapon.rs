use super::*;

/// An animated actor with a grip pair (vertices 0, 1) and a scabbard pair (vertices 1, 0) whose
/// script calls the hand then the hilt display native with the given names and offsets.
pub(super) fn display_model_fixture(
    hand: &str,
    hilt: &str,
    offsets: [i32; 3],
    equipped: bool,
    after: Vec<Operation>,
) -> (ScriptWorld, cyrus_assets::animation::Animations, Scripts) {
    use cyrus_assets::animation::AnimationModel;
    let mut scripts = cache();
    scripts.torches.insert(
        45,
        weapon::Torch {
            model: "HAND".into(),
            length: 17,
        },
    );
    // The two item natives follow the two model natives in the function table.
    let [hand_id, hilt_id, _, _] = [
        Native::DisplayHandModel,
        Native::DisplayHiltModel,
        Native::DisplayHandItem,
        Native::DisplayHiltItem,
    ]
    .map(|name| {
        let id = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name,
            param_count: 4,
            kind: CallKind::Function,
        });
        id
    });
    if equipped {
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .extend([(Attribute::Combat, 1), (Attribute::Weapon, 1)]);
        scripts.weapons.insert(
            1,
            weapon::Weapon {
                hand_model: "HAND".into(),
                hilt_model: "HILT".into(),
                length: 17,
            },
        );
    }
    let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
    program.strings = vec![hand.into(), hilt.into()];
    let call = |id, name, string| Operation::Call {
        call: Call {
            function_id: id,
            function: name,
            kind: CallKind::Function,
            target: Object::Me,
            arguments: vec![
                Value::String { index: string },
                Value::Integer { value: offsets[0] },
                Value::Integer { value: offsets[1] },
                Value::Integer { value: offsets[2] },
            ],
        },
    };
    let operations: Vec<Operation> = [
        call(hand_id, Native::DisplayHandModel, 0),
        call(hilt_id, Native::DisplayHiltModel, 1),
    ]
    .into_iter()
    .chain(after)
    .chain([Operation::End { target: 0 }])
    .collect();
    program.steps = operations
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
    program.variables = vec![0; 8];
    scripts.definition.references = vec![
        Property::HandPosVx,
        Property::HandPosVy,
        Property::HandPosVz,
        Property::HandAngleVx,
        Property::HandAngleVy,
        Property::HandAngleVz,
    ];
    for instance in &mut scripts.actors.get_mut("LOGIC").unwrap().instance_variables {
        *instance = vec![0; 8];
    }
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, _) = static_model_fixture(scripts.clone());
    data.metadata["actors"][0] = serde_json::json!({"script_name":"LOGIC","model_load_path":"animation_table",
        "raex":{"grip0":0,"grip1":1,"scabbard0":1,"scabbard1":0,"v_vertex":0,"v_size":0,"unknown_12":0},
        "raan_entries":[{"file_path":"BODY.3D","frame_count":1}],
        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]});
    data.metadata["mpob_objects"][0]["script_name"] = "LOGIC".into();
    let model = || AnimationModel {
        radius: Some(1.0),
        base_positions: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        base_normals: vec![],
        faces: vec![],
        corner_normals: vec![],
        encoding: "animated_i16".into(),
        positions: vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        normals: vec![vec![]],
    };
    for name in ["BODY", "HAND", "HILT"] {
        data.models.insert(name.into(), model());
    }
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    (world, data, scripts)
}

fn run_display_script(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
) -> anyhow::Result<()> {
    use cyrus_script::script::{AnimationData, ScriptInputs, ScriptStage};
    let id = world.id(0)?;
    world.advance(
        id,
        ScriptStage::OrdinaryMain,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            animations: Some(AnimationData(data)),
            ..Default::default()
        },
        Some(&mut |_, _| Ok(())),
        None,
        20,
    )
}

#[test]
fn display_hand_and_hilt_models_place_frame_transients_on_the_attachment_pairs() {
    // Names resolve by base file name, upper case: "hand.3d" and a directory-qualified name.
    let (mut world, data, _) = display_model_fixture(
        "hand.3d",
        "system\\hilt.3d",
        [512, 1024, 768],
        false,
        vec![],
    );
    run_display_script(&mut world, &data).unwrap();
    let shown = world.display_attachments();
    assert_eq!(shown.len(), 2);
    let actor = world.id(0).unwrap();
    assert_eq!(
        (shown[0].actor, shown[0].model.as_str(), shown[0].hilt),
        (actor, "HAND", false)
    );
    assert_eq!(
        (shown[1].actor, shown[1].model.as_str(), shown[1].hilt),
        (actor, "HILT", true)
    );
    // Grip pair (0, 256, 0 apart): the hand model sits at the midpoint of the offset pair; the
    // scabbard pair (reversed) puts the hilt at its first transformed vertex.
    assert_eq!(shown[0].geometry.position, [12.0, 24.0, 33.0]);
    assert_eq!(shown[1].geometry.position, [12.0, 25.0, 33.0]);
    assert_eq!(shown[1].geometry.retained_point, [3072, 6400, 8448]);
    // The helpers' retained attachment words: the last write (the hilt's) stands.
    let retained = world.retained_attachment(actor).unwrap().unwrap();
    assert_eq!(retained.0, [3072, 6400, 8448]);
    assert_eq!(retained.1, shown[1].geometry.retained_angles.map(i32::from));
}

#[test]
fn display_models_replace_the_equipped_placements_retained_words_and_expire_with_the_frame() {
    use cyrus_script::script::{OrdinaryInputs, WeaponInputs};
    let (mut world, data, _) =
        display_model_fixture("hand", "hilt", [512, 1024, 768], true, vec![]);
    let id = world.id(0).unwrap();
    world
        .refresh_actor_weapon_attachment(
            id,
            &data,
            WeaponInputs {
                selected_player: world.id(1).unwrap(),
                item_system_ready: true,
                shell_execution: false,
            },
        )
        .unwrap();
    let before = world.equipped_weapon(id).unwrap().unwrap().clone();
    assert_eq!(before.geometry.retained_point, [2560, 5376, 7680]);
    // The equipped placement stored its words in the actor; the display native replaces them with
    // its own, leaving the weapon's published geometry alone.
    assert_eq!(
        world.retained_attachment(id).unwrap().unwrap(),
        (
            before.geometry.retained_point,
            before.geometry.retained_angles.map(i32::from)
        )
    );
    run_display_script(&mut world, &data).unwrap();
    let after = world.equipped_weapon(id).unwrap().unwrap();
    assert_eq!(after.geometry, before.geometry);
    assert_eq!(
        world.retained_attachment(id).unwrap().unwrap().0,
        [3072, 6400, 8448]
    );
    assert_eq!(world.display_attachments().len(), 2);
    // The next world frame empties the transient pool.
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .update_ordinary(
            &data,
            [10, 30],
            OrdinaryInputs {
                scripts: cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                scene: None,
            },
            &mut random,
            &mut |_, _| Ok(()),
            20,
        )
        .unwrap();
    assert!(world.display_attachments().is_empty());
}

#[test]
fn hand_pos_and_hand_angle_are_the_retained_attachment_words_scripts_read_and_write() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let property = |reference: u16| Place::Property {
        object: Object::Me,
        reference,
    };
    let read = |local: u8, reference: u16| Operation::Assign {
        destination: Place::Local { index: local },
        formula: Formula {
            first: Value::Read {
                place: property(reference),
            },
            rest: vec![],
        },
    };
    let write = |reference: u16, value: i32| Operation::Assign {
        destination: property(reference),
        formula: Formula {
            first: Value::Integer { value },
            rest: vec![],
        },
    };
    // The script's two display calls store the words; it reads all six, then writes one point and
    // one angle word beyond any angle range and reads those back.
    let after = (0..6)
        .map(|i| read(i as u8, i))
        .chain([write(0, 999), write(4, -77), read(6, 0), read(7, 4)])
        .collect();
    let (mut world, data, scripts) =
        display_model_fixture("hand", "hilt", [512, 1024, 768], false, after);
    run_display_script(&mut world, &data).unwrap();
    let id = world.id(0).unwrap();
    let hilt = world.display_attachments()[1].geometry;
    let locals: Vec<_> = world.actors()[0]
        .runtime()
        .unwrap()
        .variables()
        .iter()
        .map(|value| value.integer().unwrap())
        .collect();
    let angles = hilt.retained_angles.map(i32::from);
    assert_eq!(
        locals,
        [3072, 6400, 8448, angles[0], angles[1], angles[2], 999, -77],
        "the display's words are visible to the script and its writes read back unmasked"
    );
    // The writes reached the actor's own words, the owner every producer shares.
    let words = world.retained_attachment(id).unwrap().unwrap();
    assert_eq!(words.0, [999, 6400, 8448]);
    assert_eq!(words.1, [angles[0], -77, angles[2]]);
    // They survive a snapshot and a restore.
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = world.snapshot(&sources).unwrap();
    let generation = world.generation() + 1;
    let restored = ScriptWorld::from_snapshot(
        saved,
        &sources,
        WorldRestoreContext {
            generation,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let id = restored.id(0).unwrap();
    assert_eq!(restored.retained_attachment(id).unwrap().unwrap(), words);
}

fn display_item_call(item: i32, native: Native) -> Operation {
    // The item natives follow the two model natives, which follow the cache script's functions.
    let base = cache().definition.functions.len() as u16;
    let function_id = base
        + match native {
            Native::DisplayHandItem => 2,
            Native::DisplayHiltItem => 3,
            other => panic!("{other} is not an item display native"),
        };
    Operation::Call {
        call: Call {
            function_id,
            function: native,
            kind: CallKind::Function,
            target: Object::Me,
            arguments: [item, 512, 1024, 768]
                .map(|value| Value::Integer { value })
                .to_vec(),
        },
    }
}

fn run_display_items(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    hud: &cyrus_script::script::ScriptHud,
) -> anyhow::Result<()> {
    use cyrus_script::script::{AnimationData, ScriptInputs, ScriptStage};
    let id = world.id(0)?;
    world.advance(
        id,
        ScriptStage::OrdinaryMain,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            animations: Some(AnimationData(data)),
            hud: Some(hud),
            ..Default::default()
        },
        Some(&mut |_, _| Ok(())),
        None,
        20,
    )
}

/// The item list: item 4 has the hand model `HAND`, item 5 has no hand model.
fn display_item_hud() -> cyrus_script::script::ScriptHud {
    let mut asset = inventory_asset(&[(4, 1), (5, 1), (7, 1)]);
    asset.inventory.get_mut(&4).unwrap().hand_model =
        cyrus_assets::hud::ItemHandModel::Model("HAND".into());
    asset.inventory.get_mut(&5).unwrap().hand_model = cyrus_assets::hud::ItemHandModel::Absent;
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
    hud
}

#[test]
fn display_items_place_the_item_list_hand_model_for_the_frame() {
    use cyrus_script::script::OrdinaryInputs;
    let after = vec![
        display_item_call(4, Native::DisplayHandItem),
        display_item_call(4, Native::DisplayHiltItem),
        display_item_call(5, Native::DisplayHandItem),
    ];
    let (mut world, data, _) = display_model_fixture("hand", "hilt", [0; 3], false, after);
    let hud = display_item_hud();
    run_display_items(&mut world, &data, &hud).unwrap();
    let shown = world.display_attachments();
    // Two model natives, then the two item natives that matched; item 5 has no hand model, so
    // its call returns without allocating a record.
    assert_eq!(shown.len(), 4);
    let actor = world.id(0).unwrap();
    assert_eq!(
        shown[2..]
            .iter()
            .map(|display| (display.actor, display.model.as_str(), display.hilt))
            .collect::<Vec<_>>(),
        [(actor, "HAND", false), (actor, "HAND", true)],
        "the hilt item native places the item's hand model with the hilt helper"
    );
    assert_eq!(shown[2].geometry.position, [12.0, 24.0, 33.0]);
    assert_eq!(shown[3].geometry.position, [12.0, 25.0, 33.0]);
    // The attachment words hold the last helper's result, and the frame ends with the next update.
    assert_eq!(
        world.retained_attachment(actor).unwrap().unwrap().0,
        shown[3].geometry.retained_point
    );
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .update_ordinary(
            &data,
            [10, 30],
            OrdinaryInputs {
                scripts: cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                scene: None,
            },
            &mut random,
            &mut |_, _| Ok(()),
            20,
        )
        .unwrap();
    assert!(world.display_attachments().is_empty());
}

#[test]
fn display_items_from_an_export_without_hand_models_refuse_instead_of_reading_as_none() {
    // Item 7 stands for an export written before hand models were recorded: its record has the
    // default (unexported) hand model, which must not read as an item without a hand model.
    let hud = display_item_hud();
    let (mut world, data, _) = display_model_fixture(
        "hand",
        "hilt",
        [0; 3],
        false,
        vec![display_item_call(7, Native::DisplayHandItem)],
    );
    let error = run_display_items(&mut world, &data, &hud).unwrap_err();
    assert!(
        format!("{error:#}").contains("not in this HUD export"),
        "{error:#}"
    );
    // An old item record without the field deserializes as unexported.
    let old: cyrus_assets::hud::InventoryItem = serde_json::from_value(serde_json::json!({
        "weapon": false, "count": 0, "total": 1, "player_max": 1, "hidden": false, "flags": 0,
        "required_items": [], "drop_add_items": [], "add_items": [], "remove_drop_items": []
    }))
    .unwrap();
    assert_eq!(old.hand_model, cyrus_assets::hud::ItemHandModel::Unexported);
}

#[test]
fn display_items_without_a_list_entry_or_item_system_are_the_originals_no_match() {
    let hud = display_item_hud();
    // An id the list does not hold, including a negative one read as unsigned.
    for item in [6, -1] {
        let (mut world, data, _) = display_model_fixture(
            "hand",
            "hilt",
            [0; 3],
            false,
            vec![display_item_call(item, Native::DisplayHandItem)],
        );
        let error = run_display_items(&mut world, &data, &hud).unwrap_err();
        assert!(
            format!("{error:#}").contains("not in the item list"),
            "{error:#}"
        );
    }
    // Without an item system the lookup is disabled: the same no match.
    let (mut world, data, _) = display_model_fixture(
        "hand",
        "hilt",
        [0; 3],
        false,
        vec![display_item_call(4, Native::DisplayHiltItem)],
    );
    let off = cyrus_script::script::ScriptHud::default();
    let error = run_display_items(&mut world, &data, &off).unwrap_err();
    assert!(
        format!("{error:#}").contains("not in the item list"),
        "{error:#}"
    );
}

#[test]
fn display_models_reject_an_unknown_name_and_a_full_pool() {
    let (mut world, data, _) = display_model_fixture("hand", "nothere", [0; 3], false, vec![]);
    let error = run_display_script(&mut world, &data).unwrap_err();
    assert!(format!("{error:#}").contains("NOTHERE"), "{error:#}");
    // The call before the failing one still placed its record.
    assert_eq!(world.display_attachments().len(), 1);
    // The pool holds 900 records; the next allocation is the original's runtime error.
    let (mut world, data, _) = display_model_fixture("hand", "hilt", [0; 3], false, vec![]);
    for _ in 0..450 {
        run_display_script(&mut world, &data).unwrap();
    }
    assert_eq!(world.display_attachments().len(), 900);
    let error = run_display_script(&mut world, &data).unwrap_err();
    assert!(format!("{error:#}").contains("pool is full"), "{error:#}");
    assert_eq!(world.display_attachments().len(), 900);
}

#[test]
fn equipped_weapon_retains_stowed_geometry_and_snapshot_bits() {
    use cyrus_assets::animation::AnimationModel;
    use cyrus_script::script::{WeaponInputs, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .extend([(Attribute::Combat, 1), (Attribute::Weapon, 1)]);
    scripts.weapons.insert(
        1,
        weapon::Weapon {
            hand_model: "HAND".into(),
            hilt_model: "HILT".into(),
            length: 17,
        },
    );
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, _) = static_model_fixture(scripts.clone());
    data.metadata["actors"][0] = serde_json::json!({"script_name":"LOGIC","model_load_path":"animation_table",
        "raex":{"grip0":0,"grip1":1,"scabbard0":1,"scabbard1":0,"v_vertex":0,"v_size":0,"unknown_12":0},
        "raan_entries":[{"file_path":"BODY.3D","frame_count":1}],
        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]});
    data.metadata["mpob_objects"][0]["script_name"] = "LOGIC".into();
    let group = data.metadata["actors"][0]["animation_groups"][0].clone();
    data.metadata["actors"][0]["animation_groups"]
        .as_array_mut()
        .unwrap()
        .push(group);
    data.metadata["actors"][0]["animation_groups"][1]["group_index"] = 129.into();
    let model = || AnimationModel {
        radius: Some(1.0),
        base_positions: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        base_normals: vec![],
        faces: vec![],
        corner_normals: vec![],
        encoding: "animated_i16".into(),
        positions: vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        normals: vec![vec![]],
    };
    for name in ["BODY", "HAND", "HILT"] {
        data.models.insert(name.into(), model());
    }
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    let inputs = WeaponInputs {
        selected_player: world.id(1).unwrap(),
        item_system_ready: true,
        shell_execution: false,
    };
    world
        .refresh_actor_weapon_attachment(id, &data, inputs)
        .unwrap();
    let retained = world.equipped_weapon(id).unwrap().unwrap().clone();
    assert_eq!(retained.model, "HILT");
    assert_eq!(retained.geometry.position, [10.0, 21.0, 30.0]);
    assert_eq!(retained.geometry.retained_point, [2560, 5376, 7680]);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = world.snapshot(&sources).unwrap();
    let mut malformed = serde_json::to_value(&saved).unwrap();
    let restored = ScriptWorld::from_snapshot(
        saved,
        &sources,
        WorldRestoreContext {
            generation: 8,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        restored.equipped_weapon(restored.id(0).unwrap()).unwrap(),
        Some(&retained)
    );
    malformed["actors"][0]["model"]["weapon"]["matrix_bits"][0][0] = f32::NAN.to_bits().into();
    let invalid = ScriptWorld::from_snapshot(
        serde_json::from_value(malformed).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 9,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    );
    assert!(format!("{:#}", invalid.err().unwrap()).contains("nonfinite"));
    let mut sheathed = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    sheathed["actors"][0]["animation"]["playback"]["current"][0] = 129.into();
    world = ScriptWorld::from_snapshot(
        serde_json::from_value(sheathed).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 10,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let id = world.id(0).unwrap();
    let inputs = WeaponInputs {
        selected_player: world.id(1).unwrap(),
        ..inputs
    };
    world
        .refresh_actor_weapon_attachment(id, &data, inputs)
        .unwrap();
    let retained = world.equipped_weapon(id).unwrap().unwrap().clone();
    assert_eq!(retained.model, "HILT");
    assert_eq!(retained.geometry.position, [10.0, 20.0, 30.0]);
    data.metadata["script_content_id"] = "stale".into();
    assert!(
        world
            .refresh_actor_weapon_attachment(id, &data, inputs)
            .is_err()
    );
    assert_eq!(world.equipped_weapon(id).unwrap(), Some(&retained));
}

#[test]
fn static_weapon_contact_uses_retained_tip_first_and_keeps_attack_intent() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{
        CombatControlState, LocomotionGeometry, OrdinaryGroundQuery, ScriptInputs,
    };
    for (tip_x, base_x, expected_cues) in [(-10, 10, 2), (10, -10, 0), (-10, -5, 0)] {
        let (world, scripts, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                }
                scripts.combat_clank_effects = [Some(31), None];
                scripts.combat_attacks =
                    vec![combat::Attack {
                        is_combo: false,
                        motions: vec![serde_json::from_value(serde_json::json!({
                    "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                    "first_collide_frame":0,"collide_duration":1
                })).unwrap()],
                        min_range: 0,
                        max_range: 100,
                    }];
            },
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        action: 0,
                        flags: CombatFlags::from_bits(1),
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "none".into();
        data.metadata["actors"][0]["ravc_count"] = 0.into();
        data.models.insert(
            "WEAPON".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        );
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([262144, 0, 262144]);
        saved["actors"][0]["model"]["weapon"] = serde_json::json!({
            "item":1,"model":"WEAPON","position_bits":[0,0,0],
            "matrix_bits":[[1065353216_u32,0,0],[0,1065353216_u32,0],[0,0,1065353216_u32]],
            "retained_point":[0,0,0],"retained_angles":[0,0,0],
            "endpoints":[[1024+base_x,0,1024],[1024+tip_x,0,1024]]
        });
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: (0..2)
                .map(|id| StaticPlacement {
                    id,
                    model: "WALL".into(),
                    position: [1024, 0, 1024],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .collect(),
            models: BTreeMap::from([("WALL".into(), ordinary_forward_wall_model())]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [1024, 1024], &[])
            .unwrap();
        let mut random = cyrus_game::random::EngineRandom::default();
        let mut cues = Vec::new();
        world
            .maintain_map_combat(
                id,
                1,
                false,
                ScriptInputs {
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: 0,
                            sidedness: false,
                        },
                        environment: cyrus_script::script::ObstacleEnvironment {
                            selected_player: Some(id),
                            auto_grab: false,
                            item_system_ready: true,
                            shell_execution: false,
                            ticks: 1,
                        },
                    }),
                    ..Default::default()
                },
                Some(&mut random),
                &mut |actor, sound| {
                    cues.push((actor, sound));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(cues.len(), expected_cues, "tip {tip_x} base {base_x}");
        assert_eq!(
            random.state(),
            if expected_cues == 0 { 1 } else { 2524885223 }
        );
        for ((actor, cue), expected_pitch) in cues.into_iter().zip([96, 992]) {
            assert_eq!(actor, id);
            assert!(matches!(
                cue,
                cyrus_script::script::ScriptSound::Positional {
                    effect: 31,
                    volume: 0,
                    rate_offset_hz,
                    ..
                } if rate_offset_hz == expected_pitch
            ));
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            0x21
        );
    }
}

#[test]
fn draw_sword_enters_the_draw_state_and_a_weapon_item_use_draws_it() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for item_use in [false, true] {
        let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
            let name = if item_use {
                Native::UseItem
            } else {
                Native::DrawSword
            };
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name,
                param_count: u8::from(item_use),
                kind: CallKind::Function,
            });
            let call = Operation::ObjectCall {
                call: Call {
                    function_id: id,
                    function: name,
                    target: Object::Player,
                    kind: CallKind::Function,
                    arguments: if item_use {
                        vec![Value::Integer { value: 1 }]
                    } else {
                        vec![]
                    },
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
        let mut asset = inventory_asset(&[(1, 1)]);
        asset.inventory.get_mut(&1).unwrap().item_type = cyrus_assets::hud::ItemKind::WEAPON;
        asset.inventory.get_mut(&1).unwrap().weapon = true;
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
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        let actor = &world.actors()[player.index];
        let control = actor.control_state().unwrap();
        assert_eq!(
            control.living_state,
            LivingState::DRAWING_SWORD,
            "item use: {item_use}"
        );
        assert_eq!(control.combat.as_ref().unwrap().flags.bits() & 4, 4);
        if item_use {
            assert_eq!(
                actor.attributes()[Attribute::Weapon],
                1,
                "the item becomes the weapon"
            );
        }
    }
}

#[test]
fn sheath_sword_and_end_combat_dispatch_to_the_combat_services() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // An unarmed player cannot sheathe, so SheathSword completes without a state change; endCombat
    // still runs its unconditional reset of the lifecycle selector, AI mode and combat flag.
    for function in [Native::SheathSword, Native::EndCombat] {
        let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name: function,
                param_count: 0,
                kind: CallKind::Function,
            });
            let call = Operation::ObjectCall {
                call: Call {
                    function_id: id,
                    function,
                    target: Object::Player,
                    kind: CallKind::Function,
                    arguments: vec![],
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
            &inventory_asset(&[]),
            &cyrus_game::preferences::Preferences::default(),
        );
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
        let before = world.actors()[player.index]
            .control_state()
            .unwrap()
            .ai_type
            .byte();
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
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        assert!(world.actors()[map.index].failure().is_none(), "{function}");
        let control = world.actors()[player.index].control_state().unwrap();
        assert_eq!(
            control.living_state,
            LivingState::ORDINARY,
            "{function} leaves an unarmed player idle"
        );
        if function == Native::EndCombat {
            assert_eq!(control.ai_type, AiType::FIND_PLAYER);
            assert_eq!(control.ai_mode, 0);
            assert_eq!(control.combat.as_ref().unwrap().flags.bits() & 4, 0);
        } else {
            assert_eq!(
                control.ai_type.byte(),
                before,
                "SheathSword leaves the selector"
            );
        }
    }
}

#[test]
fn kill_me_releases_equipped_weapon_presentation_before_resurrection() {
    use cyrus_assets::animation::AnimationModel;
    use cyrus_script::script::{ScriptInputs, WeaponInputs};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.definition.functions.push(Function {
        name: Native::Resurrect,
        param_count: 0,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor
        .attributes
        .extend([(Attribute::Combat, 1), (Attribute::Weapon, 1)]);
    actor.program.steps = BTreeMap::from([
        (0, assign_call(1, 1, Native::KillMe, &[])),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 2 },
            },
        ),
        (2, assign_call(3, 2, Native::Resurrect, &[])),
        (
            3,
            Step {
                next: 4,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.weapons.insert(
        1,
        weapon::Weapon {
            hand_model: "HAND".into(),
            hilt_model: "HILT".into(),
            length: 17,
        },
    );
    let (mut world, mut data, _) = static_model_fixture(scripts);
    data.metadata["actors"][0] = serde_json::json!({
        "script_name":"LOGIC", "model_load_path":"animation_table",
        "raex":{"grip0":0,"grip1":1,"scabbard0":1,"scabbard1":0,"v_vertex":0,"v_size":0,"unknown_12":0},
        "raan_entries":[{"file_path":"BODY.3D","frame_count":1}],
        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]
    });
    data.metadata["mpob_objects"][0]["script_name"] = "LOGIC".into();
    for name in ["BODY", "HAND", "HILT"] {
        data.models.insert(
            name.into(),
            AnimationModel {
                radius: Some(1.0),
                base_positions: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
                normals: vec![vec![]],
            },
        );
    }
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    world
        .refresh_actor_weapon_attachment(
            id,
            &data,
            WeaponInputs {
                selected_player: world.id(1).unwrap(),
                item_system_ready: true,
                shell_execution: false,
            },
        )
        .unwrap();
    assert!(world.model_presentation(id).unwrap().is_some());
    assert_eq!(world.equipped_weapon(id).unwrap().unwrap().model, "HILT");
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            inputs,
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )
        .unwrap();
    assert!(world.actors()[0].control_state().unwrap().killed);
    let death = world.take_death_events();
    assert_eq!(death.len(), 1);
    assert_eq!(death[0].snapshot.actor, id);
    let revision = world.actor_snapshot(id).unwrap().revision;
    assert_eq!(
        world.actor_snapshot(id).unwrap().presence,
        cyrus_script::script::ActorPresence::Removed
    );

    assert!(world.model_presentation(id).unwrap().is_none());
    assert!(
        world.equipped_weapon(id).unwrap().is_none(),
        "KillMe must release the independently rendered weapon"
    );
    world.refresh_model_presentations(&data, &[id]).unwrap();
    world
        .refresh_actor_weapon_attachment(
            id,
            &data,
            WeaponInputs {
                selected_player: world.id(1).unwrap(),
                item_system_ready: true,
                shell_execution: false,
            },
        )
        .unwrap();
    assert!(
        world.model_presentation(id).unwrap().is_none(),
        "later publication must not recreate a killed receiver's body"
    );
    assert!(
        world.equipped_weapon(id).unwrap().is_none(),
        "later publication must not recreate a killed receiver's weapon"
    );
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            inputs,
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )
        .unwrap();
    assert!(!world.actors()[0].control_state().unwrap().killed);
    assert!(world.actor_snapshot(id).unwrap().revision > revision);
    assert!(world.take_death_events().is_empty());

    assert!(world.model_presentation(id).unwrap().is_none());
    assert!(
        world.equipped_weapon(id).unwrap().is_none(),
        "Resurrect must not republish stale attachment geometry"
    );
}
