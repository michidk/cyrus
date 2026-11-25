use super::*;

#[test]
fn navigation_reservations_use_identifier_bytes_and_maintenance_clears_all_maps() {
    let mut world = wander_world(
        1024,
        0,
        serde_json::json!([
            {"node_count":2,"walk_nodes":[{"reserved":255,"node_pos_x":0,"node_pos_y":0,"node_pos_z":0,"route_count":0,"routes":[]},{"reserved":1,"node_pos_x":0,"node_pos_y":0,"node_pos_z":0,"route_count":0,"routes":[]}]},
            {"node_count":1,"walk_nodes":[{"reserved":2,"node_pos_x":0,"node_pos_y":0,"node_pos_z":0,"route_count":0,"routes":[]}]}
        ]),
    );
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    assert!(world.reserve_walk_node(first, 0, 0).unwrap());
    assert!(world.reserve_walk_node(first, 0, 0).unwrap());
    assert!(!world.reserve_walk_node(second, 0, 0).unwrap());
    world.release_walk_node(second, 0, 0).unwrap();
    assert!(!world.reserve_walk_node(second, 0, 0).unwrap());
    world.release_walk_node(first, 0, 0).unwrap();
    assert!(world.reserve_walk_node(second, 0, 0).unwrap());
    assert!(world.reserve_walk_node(first, 0, 1).unwrap());
    assert!(world.reserve_walk_node(first, 1, 0).unwrap());
    world.clear_walk_node_reservations().unwrap();
    for (map, node) in [(0, 0), (0, 1), (1, 0)] {
        assert!(world.reserve_walk_node(second, map, node).unwrap());
    }
    assert!(
        world
            .reserve_walk_node(first, 1, 1)
            .unwrap_err()
            .to_string()
            .contains("world 7 actor 0 reserve navigation")
    );
    assert!(
        world
            .reserve_walk_node(
                cyrus_script::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                0,
                0
            )
            .is_err()
    );
}

pub(super) fn free_node_fixture(
    maps: serde_json::Value,
    height: i32,
    edit: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let (mut world, scripts, mut data) = retained_point_fixture(18, 3, height, edit);
    data.metadata["walk_node_maps"] = maps;
    world.prepare_navigation(&data).unwrap();
    (world, scripts, data)
}

pub(super) fn walk_node(position: [i32; 3]) -> serde_json::Value {
    serde_json::json!({"node_pos_x":position[0],"node_pos_y":position[1],"node_pos_z":position[2],"reserved":255,"route_count":0,"routes":[]})
}

#[test]
fn free_node_search_reverses_file_maps_and_preserves_early_return_ties_and_live_reservations() {
    let (mut world, scripts, data) = free_node_fixture(
        serde_json::json!([
            {"node_count":1,"walk_nodes":[walk_node([0,0,0])]},
            {"node_count":3,"walk_nodes":[walk_node([20,0,0]),walk_node([10,0,0]),walk_node([0,0,0])]}
        ]),
        0,
        |saved| saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]),
    );
    let actor = world.id(0).unwrap();
    let before = world.spatial(actor).unwrap().local;
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), Some((1, 1)));
    assert_eq!(
        serde_json::to_value(world.spatial(actor).unwrap().local).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert!(world.reserve_walk_node(actor, 1, 1).unwrap());
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), Some((1, 2)));
    assert!(world.reserve_walk_node(actor, 1, 2).unwrap());
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), Some((0, 0)));
    assert!(world.reserve_walk_node(actor, 0, 0).unwrap());
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), Some((1, 0)));
    assert!(world.reserve_walk_node(actor, 1, 0).unwrap());
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), None);
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["walk_nodes"]["selected_map"],
        serde_json::Value::Null
    );
    world.clear_walk_node_reservations().unwrap();
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), Some((1, 1)));

    let (mut world, _, _) = free_node_fixture(
        serde_json::json!([
            {"node_count":1,"walk_nodes":[walk_node([20,0,0])]},
            {"node_count":1,"walk_nodes":[walk_node([20,0,0])]}
        ]),
        0,
        |saved| saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]),
    );
    assert_eq!(
        world.nearest_free_walk_node(world.id(0).unwrap()).unwrap(),
        Some((1, 0))
    );
    for (distance, expected) in [(11, Some((1, 0))), (12, Some((0, 0)))] {
        let (mut world, _, _) = free_node_fixture(
            serde_json::json!([
                {"node_count":1,"walk_nodes":[walk_node([0,0,0])]},
                {"node_count":1,"walk_nodes":[walk_node([distance,0,0])]}
            ]),
            0,
            |saved| {
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0])
            },
        );
        assert_eq!(
            world.nearest_free_walk_node(world.id(0).unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn free_node_search_uses_query_pose_shifted_axes_height_and_exclusive_bound() {
    for (x, expected) in [(4095, Some((0, 0))), (4096, None)] {
        let (mut world, _, _) = free_node_fixture(
            serde_json::json!([{"node_count":1,"walk_nodes":[walk_node([x,15,0])]}]),
            17,
            |saved| {
                saved["spatial"]["actors"][0]["local"]["position"] =
                    serde_json::json!([255, -257, 255])
            },
        );
        assert_eq!(
            world.nearest_free_walk_node(world.id(0).unwrap()).unwrap(),
            expected
        );
    }
    for attached in [false, true] {
        let (mut world, _, _) = free_node_fixture(
            serde_json::json!([{"node_count":2,"walk_nodes":[walk_node([0,15,0]),walk_node([100,15,0])]}]),
            17,
            |saved| {
                saved["spatial"]["actors"][0]["local"]["position"] =
                    serde_json::json!([255, -257, 255]);
                saved["spatial"]["actors"][0]["linked_position"] =
                    serde_json::json!([25600, -257, 0]);
                if attached {
                    saved["actors"][0]["attributes"][26] = 1.into();
                    let mut pose = saved["spatial"]["actors"][0]["local"].clone();
                    pose["position"] = serde_json::json!([0, -257, 0]);
                    saved["spatial"]["actors"][0]["effective"] = pose;
                }
            },
        );
        assert_eq!(
            world.nearest_free_walk_node(world.id(0).unwrap()).unwrap(),
            Some((0, if attached { 0 } else { 1 }))
        );
    }
}

#[test]
fn free_node_winning_map_and_reservations_survive_rebinding_and_reject_invalid_saved_map() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let (mut world, scripts, data) = free_node_fixture(
        serde_json::json!([
            {"node_count":1,"walk_nodes":[walk_node([20,0,0])]},
            {"node_count":1,"walk_nodes":[walk_node([30,0,0])]}
        ]),
        0,
        |saved| saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]),
    );
    let actor = world.id(0).unwrap();
    world.reserve_walk_node(actor, 0, 0).unwrap();
    assert_eq!(world.nearest_free_walk_node(actor).unwrap(), Some((1, 0)));
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(saved["walk_nodes"]["selected_map"], 1);
    let restore = |saved| {
        ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 3,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
    };
    let mut restored = restore(saved.clone()).unwrap();
    assert_eq!(
        movement_snapshot(&restored, &scripts, &data)["walk_nodes"],
        saved["walk_nodes"]
    );
    assert_eq!(
        restored
            .nearest_free_walk_node(restored.id(0).unwrap())
            .unwrap(),
        Some((1, 0))
    );
    let mut invalid = saved;
    invalid["walk_nodes"]["selected_map"] = 2.into();
    let error = restore(invalid).err().unwrap();
    assert!(format!("{error:#}").contains("saved selected walk-node map unavailable"));
}

#[test]
fn free_node_preparation_rejects_missing_or_out_of_range_coordinates_with_context() {
    for (field, value) in [
        ("node_pos_x", Some(-1)),
        ("node_pos_x", Some(65536)),
        ("node_pos_y", Some(32768)),
        ("node_pos_y", Some(-32769)),
        ("node_pos_z", Some(65536)),
        ("node_pos_z", None),
    ] {
        let mut node = walk_node([0, 0, 0]);
        node[field] = serde_json::json!(value);
        let scripts = cache();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,"walk_node_maps":[{"node_count":1,"walk_nodes":[node]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let error = world.prepare_navigation(&data).unwrap_err();
        assert!(format!("{error:#}").contains(&format!(
            "world 7 walk_node_maps map 0 node 0: {field} absent or invalid"
        )));
    }
}

pub(super) fn navigation_owner_fixture()
-> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [20, -2, 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[],"walk_node_maps":[
            {"node_count":2,"walk_nodes":[walk_node([20,-2,3]),walk_node([40,-2,3])]},
            {"node_count":2,"walk_nodes":[walk_node([0,-2,3]),walk_node([20,-2,3])]}
        ]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    (world, scripts, data)
}

pub(super) fn restore_navigation_owner(
    saved: serde_json::Value,
    scripts: &Scripts,
    data: &cyrus_assets::animation::Animations,
) -> anyhow::Result<ScriptWorld> {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let generation = saved["generation"].as_u64().unwrap() + 1;
    ScriptWorld::from_snapshot(
        serde_json::from_value(saved)?,
        &WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts,
            animations: data,
            collision: &Default::default(),
        },
        WorldRestoreContext {
            generation,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
}

#[test]
fn actor_navigation_initializes_unset_then_associates_authored_local_map_units() {
    use cyrus_script::script::ActorNavigation;
    let (mut world, scripts, data) = navigation_owner_fixture();
    assert_eq!(
        world.actors()[0].control_state().unwrap().navigation,
        ActorNavigation {
            map: None,
            current: -1,
            next: -1,
            previous_next: 0,
            destination: -1
        }
    );
    world.prepare_navigation(&data).unwrap();
    assert_eq!(
        world.actors()[0].control_state().unwrap().navigation,
        ActorNavigation {
            map: Some(1),
            current: 1,
            next: -1,
            previous_next: 0,
            destination: -1
        }
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["walk_nodes"]["maps"],
        serde_json::json!([[0, 0], [0, 0]])
    );
    assert_eq!(
        saved["walk_nodes"]["disabled"],
        serde_json::json!([false, false])
    );
    let restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    assert_eq!(
        restored.actors()[0].control_state().unwrap().navigation,
        world.actors()[0].control_state().unwrap().navigation
    );
}

#[test]
fn actor_navigation_release_preserves_foreign_owner_map_and_destination() {
    let (mut world, scripts, data) = navigation_owner_fixture();
    world.prepare_navigation(&data).unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    let identifier = world.actors()[0].runtime_identifier.unwrap() as u8;
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":0,"destination":1});
    saved["walk_nodes"]["maps"][0] = serde_json::json!([identifier, 99]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    world.release_held_walk_nodes(world.id(0).unwrap()).unwrap();
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][0]["control"]["navigation"],
        serde_json::json!({"map":0,"current":-1,"next":-1,"previous_next":0,"destination":1})
    );
    assert_eq!(saved["walk_nodes"]["maps"][0], serde_json::json!([0, 99]));
}

#[test]
fn actor_navigation_disabled_map_precedes_invalid_indices_and_retains_destination_once() {
    for disabled in [false, true] {
        let (mut world, scripts, data) = navigation_owner_fixture();
        world.prepare_navigation(&data).unwrap();
        let mut saved = movement_snapshot(&world, &scripts, &data);
        let owner = world.actors()[0].runtime_identifier.unwrap() as u8;
        saved["actors"][0]["control"]["navigation"] =
            serde_json::json!({"map":0,"current":0,"next":2,"previous_next":0,"destination":2});
        saved["walk_nodes"]["maps"][0] = serde_json::json!([owner, 99]);
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        world.set_walk_map_enabled(0, !disabled).unwrap();
        let id = world.id(0).unwrap();
        assert!(!world.validate_walk_navigation(id).unwrap());
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            saved["actors"][0]["control"]["navigation"],
            serde_json::json!({"map":null,"current":-1,"next":-1,"previous_next":0,"destination":if disabled {2} else {-1}})
        );
        assert_eq!(saved["walk_nodes"]["maps"][0], serde_json::json!([0, 99]));
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        assert!(
            !world
                .validate_walk_navigation(world.id(0).unwrap())
                .unwrap()
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .navigation
                .destination,
            -1
        );
    }
}

#[test]
fn actor_navigation_restore_rejects_unknown_maps_and_unsupported_negative_indices() {
    let (mut world, scripts, data) = navigation_owner_fixture();
    world.prepare_navigation(&data).unwrap();
    let saved = movement_snapshot(&world, &scripts, &data);
    for (field, value, expected) in [
        ("map", 2, "walk-node map 2: unavailable"),
        ("current", -2, "index below unset sentinel"),
    ] {
        let mut invalid = saved.clone();
        invalid["actors"][0]["control"]["navigation"][field] = value.into();
        let error = restore_navigation_owner(invalid, &scripts, &data)
            .err()
            .unwrap();
        let error = format!("{error:#}");
        assert!(
            error.contains("world 7 actor 0 saved navigation"),
            "{error}"
        );
        assert!(error.contains(expected), "{error}");
    }
    let mut invalid = saved;
    invalid["walk_nodes"]["disabled"] = serde_json::json!([true]);
    assert!(
        format!(
            "{:#}",
            restore_navigation_owner(invalid, &scripts, &data)
                .err()
                .unwrap()
        )
        .contains("saved walk-map disabled count differs")
    );
}

#[test]
fn resurrect_clears_the_killed_flag_after_kill_me_and_releases_held_nodes() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.definition.functions.push(Function {
        name: Native::Resurrect,
        param_count: 0,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let first = actor.program.steps[&0].clone();
    let mut second = Step {
        next: 2,
        operation: first.operation.clone(),
    };
    if let Operation::Assign { formula, .. } = &mut second.operation
        && let Value::Call { call } = &mut formula.first
    {
        call.function_id = 2;
        call.function = Native::Resurrect;
    }
    actor.program.steps = BTreeMap::from([
        (0, Step { next: 1, ..first }),
        (1, second),
        (
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, mut data) = navigation_owner_fixture();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    // Resurrect's combat branch consumes one queued class message. Give the
    // receiver a minimal combat record and enable combat construction in the
    // same saved state used by the navigation lifecycle test.
    saved["actors"][0]["attributes"][14] = 1.into();
    saved["actors"][0]["control"]["combat"] =
        serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
    let owner = world.actors()[0].runtime_identifier.unwrap() as u8;
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":0,"destination":1});
    saved["walk_nodes"]["maps"][0] = serde_json::json!([owner, owner]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    world
        .send_combat_message(sender, id, CombatMessageCode::HIT)
        .unwrap();
    assert!(world.peek_combat_message(id).unwrap().is_some());
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )
        .unwrap();
    let control = world.actors()[0].control_state().unwrap();
    // KillMe set the flag and selector 8; Resurrect cleared only the flag.
    assert!(!control.killed);
    assert_eq!(control.ai_type, AiType::DEAD);
    assert!(world.peek_combat_message(id).unwrap().is_none());
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(saved["walk_nodes"]["maps"][0], serde_json::json!([0, 0]));
}

#[test]
fn resurrect_runs_the_end_of_combat_reset_for_a_killed_selector_six_receiver() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::Resurrect, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, mut data) = navigation_owner_fixture();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["attributes"][14] = 1.into();
    saved["actors"][0]["control"]["combat"] =
        serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
    saved["actors"][0]["control"]["killed"] = true.into();
    saved["actors"][0]["control"]["death_selector"] = 6.into();
    saved["actors"][0]["control"]["death_response"] = 3.into();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs::default(),
            None,
            None,
            100,
        )
        .unwrap();
    assert!(world.actors()[0].control_state().unwrap().killed);
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )
        .unwrap();
    let control = world.actors()[0].control_state().unwrap();
    assert!(!control.killed);
    assert_eq!((control.ai_type.byte(), control.ai_mode.byte()), (1, 0));
}

#[test]
fn actor_navigation_kill_me_releases_held_nodes_before_owned_sound_cleanup() {
    use cyrus_script::script::{ScriptInputs, ScriptSound, ScriptStage};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, mut data) = navigation_owner_fixture();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    let owner = world.actors()[0].runtime_identifier.unwrap() as u8;
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":1,"previous_next":0,"destination":1});
    saved["walk_nodes"]["maps"][0] = serde_json::json!([owner, owner]);
    let initial = saved.clone();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let id = world.id(0).unwrap();
    let mut stopped = false;
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |actor, sound| {
                assert_eq!(actor, id);
                assert!(matches!(sound, ScriptSound::StopOwned));
                stopped = true;
                Ok(())
            }),
            None,
            100,
        )
        .unwrap();
    assert!(stopped);
    assert!(world.actors()[0].control_state().unwrap().killed);
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(saved["walk_nodes"]["maps"][0], serde_json::json!([0, 0]));
    assert_eq!(
        saved["actors"][0]["control"]["navigation"],
        serde_json::json!({"map":0,"current":-1,"next":-1,"previous_next":0,"destination":1})
    );
    let mut world = restore_navigation_owner(initial, &scripts, &data).unwrap();
    let error = world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| anyhow::bail!("sound cleanup unavailable")),
            None,
            100,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("sound cleanup unavailable"));
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(saved["walk_nodes"]["maps"][0], serde_json::json!([0, 0]));
    assert_eq!(saved["actors"][0]["control"]["navigation"]["current"], -1);
    assert_eq!(saved["actors"][0]["control"]["navigation"]["next"], -1);
}

fn perception_case(
    delay: i32,
    timer: i32,
    ticks: u32,
    visible: bool,
    lost: i32,
) -> (i32, i32, i32, bool) {
    use cyrus_script::script::{
        CombatControlState, LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery,
        ScriptInputs, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = cache();
    scripts.node_timer = Some(delay);
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.map_position = [0; 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, models) = static_model_fixture_at(scripts.clone(), None, [0; 3]);
    data.metadata["walk_node_maps"] = serde_json::json!([
        {"node_count":1,"walk_nodes":[walk_node([0,0,0])]}
    ]);
    world.prepare_navigation(&data).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    let combat = CombatControlState {
        flags: CombatFlags::from_bits(4),
        lost_sight_ticks: timer,
        lost_sight_routing: lost,
        spacing_ranges: [0, 123, 0],
        ..Default::default()
    };
    saved["actors"][0]["control"]["combat"] = serde_json::to_value(combat).unwrap();
    saved["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
    saved["actors"][0]["control"]["death_selector"] = 7.into();
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":0,"previous_next":0,"destination":0});
    saved["walk_nodes"]["maps"][0] = serde_json::json!([99]);
    saved["spatial"]["actors"][1]["local"]["position"] =
        serde_json::json!([if visible { 100 } else { 1000 }, 0, 0]);
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    let prepared = world
        .prepare_ordinary_collision_query(ids[0], &scene, &data, [0, 0], &ids)
        .unwrap();
    let result = world.maintain_combat_perception(
        ids[0],
        ids[1],
        ticks,
        ScriptInputs {
            frame_scale: Some(256),
            locomotion_geometry: Some(LocomotionGeometry {
                ground: OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 0,
                    sidedness: false,
                },
                environment: ObstacleEnvironment {
                    selected_player: Some(ids[1]),
                    auto_grab: false,
                    item_system_ready: false,
                    shell_execution: false,
                    ticks,
                },
            }),
            ..Default::default()
        },
        None,
        &mut |_, _| Ok(()),
    );
    if let Err(error) = &result {
        assert!(
            format!("{error:#}").contains("routed point navigation unsupported"),
            "{error:#}"
        );
    }
    if visible && lost != 0 {
        let restored = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        assert_eq!(restored["walk_nodes"]["maps"][0], serde_json::json!([0]));
        assert_eq!(
            restored["actors"][0]["control"]["navigation"],
            serde_json::json!({"map":0,"current":-1,"next":-1,"previous_next":0,"destination":0})
        );
        assert_eq!(
            restored["actors"][0]["control"]["combat"]["attacker_selection_state"],
            1
        );
    }
    let control = world.actors()[0].control_state().unwrap();
    let combat = control.combat.as_ref().unwrap();
    (
        combat.lost_sight_ticks,
        combat.lost_sight_routing,
        i32::from(control.ai_type.byte()),
        result.is_ok(),
    )
}

#[test]
fn perception_waits_at_equality_wraps_ticks_and_reacquires_through_live_owners() {
    assert_eq!(perception_case(3, 3, 5, false, 7), (8, 7, 7, true));
    assert_eq!(
        perception_case(i32::MAX, i32::MAX, 1, false, 7),
        (i32::MIN, 7, 7, true)
    );
    assert_eq!(perception_case(3, 4, 5, false, 7), (4, 1, 0, true));
    assert_eq!(perception_case(3, 4, 5, true, 7), (0, 0, 6, true));
    assert_eq!(perception_case(3, 4, 5, true, 0), (0, 0, 7, true));
}

#[test]
fn native_command_slots_restore_and_pass_shared_state_between_actors() {
    use cyrus_script::script::NativeCommandState;
    let scripts = wait_cache(vec![
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 4 }],
        ),
        Operation::End { target: 0 },
    ]);
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,"actors":[]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
    let mut saved = movement_snapshot(&world, &scripts, &data);
    let retained = NativeCommandState {
        continuation: 305419896,
        continuation_direction: 2271560481,
        pause_ticks: u32::MAX,
        point_value: 7654321,
        point_word: 4321,
        point_phase: PointPhase::from_byte(67),
        point_counter: 89,
    };
    saved["actors"][0]["native_commands"][0] = serde_json::to_value(retained).unwrap();
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    world.step_tasks(world.id(0).unwrap(), Some(256)).unwrap();
    assert_eq!(
        *world.globals().native_command(),
        NativeCommandState {
            point_value: 1,
            ..retained
        }
    );
    let mut slot = serde_json::to_value(retained).unwrap();
    slot.as_object_mut().unwrap().remove("point_value");
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][0]["native_commands"],
        serde_json::to_value(vec![slot.clone(); 6]).unwrap()
    );
    world.step_tasks(world.id(1).unwrap(), Some(256)).unwrap();
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][1]["native_commands"],
        serde_json::to_value(vec![slot.clone(); 6]).unwrap()
    );
    let restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    assert_eq!(
        movement_snapshot(&restored, &scripts, &data)["actors"][1]["native_commands"],
        serde_json::to_value(vec![slot.clone(); 6]).unwrap()
    );
}

#[test]
fn game_host_native_setup_distinguishes_hook_statement_and_expression_encoding() {
    use cyrus_script::script::{NativeCommandState, ScriptInputs, ScriptStage};
    let retained = NativeCommandState {
        continuation: 123,
        continuation_direction: 0,
        pause_ticks: 456,
        point_value: 789,
        point_word: 12,
        point_phase: PointPhase::from_byte(34),
        point_counter: 56,
    };
    for encoding in 0..3 {
        let health = Call {
            function_id: 4,
            target: Object::Me,
            kind: CallKind::Function,
            function: Native::Health,
            arguments: vec![],
        };
        let operation = if encoding == 0 {
            Operation::Call {
                call: health.clone(),
            }
        } else {
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: if encoding == 1 {
                        Value::Call {
                            call: Box::new(health.clone()),
                        }
                    } else {
                        Value::ObjectCall {
                            call: Box::new(health.clone()),
                        }
                    },
                    rest: vec![],
                },
            }
        };
        let mut scripts = wait_cache(vec![
            wait(
                1,
                Native::Wait,
                CallKind::Multitask,
                vec![Value::Integer { value: 100 }],
            ),
            Operation::Yield,
            operation,
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.push(Function {
            name: Native::Health,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(health);
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        for index in 0..2 {
            world
                .step(world.id(index).unwrap(), Thread::Main, 20)
                .unwrap();
        }
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["native_commands"][0] = serde_json::to_value(retained).unwrap();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        world.step_tasks(world.id(0).unwrap(), Some(256)).unwrap();
        world
            .advance(
                world.id(1).unwrap(),
                ScriptStage::NativeHook,
                ScriptInputs::default(),
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            *world.globals().native_command(),
            NativeCommandState {
                point_value: 1,
                ..retained
            }
        );
        world.step(world.id(1).unwrap(), Thread::Main, 20).unwrap();
        assert_eq!(
            *world.globals().native_command(),
            if encoding == 2 {
                NativeCommandState {
                    point_value: 1,
                    ..Default::default()
                }
            } else {
                NativeCommandState::default()
            }
        );
    }
}

fn route_fixture(
    links: &[Vec<(usize, u16)>],
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    let nodes: Vec<_> = links
        .iter()
        .enumerate()
        .map(|(index, links)| {
            let mut node = walk_node([index as i32 * 100, 0, 0]);
            node["route_count"] = links.len().into();
            node["routes"] = serde_json::json!(
                links
                    .iter()
                    .map(|&(target, cost)| serde_json::json!({"target_node_id":target,"cost":cost}))
                    .collect::<Vec<_>>()
            );
            node
        })
        .collect();
    let (world, scripts, data) = free_node_fixture(
        serde_json::json!([{"node_count":nodes.len(),"walk_nodes":nodes}]),
        0,
        |_| {},
    );
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][0]["control"]["navigation"] =
        serde_json::json!({"map":0,"current":0,"next":-1,"previous_next":0,"destination":-1});
    (
        restore_navigation_owner(saved, &scripts, &data).unwrap(),
        scripts,
        data,
    )
}

#[test]
fn walk_route_reverse_predecessor_ties_override_the_discovering_parent() {
    let (mut world, scripts, data) =
        route_fixture(&[vec![(1, 1), (2, 1)], vec![(3, 5)], vec![(3, 5)], vec![]]);
    let actor = world.id(0).unwrap();
    let before = movement_snapshot(&world, &scripts, &data);
    assert_eq!(world.next_walk_node(actor, 3, None).unwrap(), 2);
    let after = movement_snapshot(&world, &scripts, &data);
    assert_eq!(after["actors"], before["actors"]);
    assert_eq!(after["spatial"], before["spatial"]);
    assert_eq!(after["walk_nodes"], before["walk_nodes"]);
    assert_eq!(world.globals().walk_route().link_flags, [0, 0]);
    let mut restored = restore_navigation_owner(after, &scripts, &data).unwrap();
    assert_eq!(
        restored
            .next_walk_node(restored.id(0).unwrap(), 3, None)
            .unwrap(),
        2
    );
}

#[test]
fn walk_route_cost_cutoff_and_first_duplicate_control_selection_and_flags() {
    for (links, expected, flags) in [
        (vec![(1, 2048)], 1, [0, 0]),
        (vec![(1, 2049)], -1, [0, 0]),
        (vec![(1, 0)], -1, [0, 0]),
        (vec![(1, 0), (1, 1)], -1, [0, 0]),
        (vec![(1, 16385), (1, 1)], 1, [1, 0]),
        (vec![(1, 32769), (1, 1)], 1, [0, 1]),
    ] {
        let (mut world, _, _) = route_fixture(&[links, vec![]]);
        let actor = world.id(0).unwrap();
        assert_eq!(world.next_walk_node(actor, 1, None).unwrap(), expected);
        assert_eq!(world.globals().walk_route().link_flags, flags);
        assert_eq!(world.next_walk_node(actor, 0, None).unwrap(), 0);
        assert_eq!(world.globals().walk_route().link_flags, flags);
    }
}

#[test]
fn walk_route_reads_live_source_reservations_and_preserves_them_on_restore() {
    let (mut world, scripts, data) = route_fixture(&[vec![(1, 1)], vec![]]);
    let actor = world.id(0).unwrap();
    let peer = world.id(1).unwrap();
    world.reserve_walk_node(peer, 0, 1).unwrap();
    assert_eq!(world.next_walk_node(actor, 1, None).unwrap(), 1);
    world.reserve_walk_node(peer, 0, 0).unwrap();
    assert_eq!(world.next_walk_node(actor, 1, None).unwrap(), -1);
    let saved = movement_snapshot(&world, &scripts, &data);
    let mut restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let actor = restored.id(0).unwrap();
    assert_eq!(restored.next_walk_node(actor, 1, None).unwrap(), -1);
    restored.clear_walk_node_reservations().unwrap();
    restored.reserve_walk_node(actor, 0, 0).unwrap();
    assert_eq!(restored.next_walk_node(actor, 1, None).unwrap(), 1);
    restored.set_walk_map_enabled(0, false).unwrap();
    assert_eq!(restored.next_walk_node(actor, 1, None).unwrap(), -1);
}

#[test]
fn disable_and_enable_node_maps_set_every_map_flag_and_need_prepared_navigation() {
    let (mut world, _, _) = route_fixture(&[vec![(1, 1)], vec![]]);
    let actor = world.id(0).unwrap();
    world.reserve_walk_node(actor, 0, 0).unwrap();
    assert_eq!(world.next_walk_node(actor, 1, None).unwrap(), 1);
    world.set_all_walk_maps_enabled(false).unwrap();
    assert_eq!(world.next_walk_node(actor, 1, None).unwrap(), -1);
    world.set_all_walk_maps_enabled(true).unwrap();
    assert_eq!(world.next_walk_node(actor, 1, None).unwrap(), 1);
    for name in [Native::DisableNodeMaps, Native::EnableNodeMaps] {
        assert!(run_native_world(&[(name, &[])]).is_err());
        assert!(run_native_world(&[(name, &[1])]).is_err());
    }
}

#[test]
fn walk_route_player_exclusion_uses_local_map_units_and_admits_equality() {
    use cyrus_script::script::{ScriptActorId, WalkRouteExclusion};
    let (world, scripts, data) = route_fixture(&[vec![(1, 1)], vec![]]);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([25600, 0, 0]);
    saved["spatial"]["actors"][1]["linked_position"] = serde_json::json!([-1000000, 0, 0]);
    let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
    let actor = world.id(0).unwrap();
    let player = world.id(1).unwrap();
    assert_eq!(
        world
            .next_walk_node(
                actor,
                1,
                Some(WalkRouteExclusion {
                    selected_player: player,
                    minimum_distance: 1
                })
            )
            .unwrap(),
        -1
    );
    assert_eq!(
        world
            .next_walk_node(
                actor,
                1,
                Some(WalkRouteExclusion {
                    selected_player: player,
                    minimum_distance: 0
                })
            )
            .unwrap(),
        1
    );
    let stale = ScriptActorId {
        generation: 999,
        index: 1,
    };
    let error = world
        .next_walk_node(
            actor,
            1,
            Some(WalkRouteExclusion {
                selected_player: stale,
                minimum_distance: 0,
            }),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("route selected player"));
    assert_eq!(
        world
            .next_walk_node(
                actor,
                0,
                Some(WalkRouteExclusion {
                    selected_player: stale,
                    minimum_distance: 0
                })
            )
            .unwrap(),
        0
    );
}

#[test]
fn walk_route_metadata_rejects_missing_counts_and_invalid_links_in_context() {
    for (field, value) in [
        ("routes", serde_json::Value::Null),
        ("route_count", serde_json::json!(1)),
        ("routes", serde_json::json!([{"target_node_id":1,"cost":1}])),
        (
            "routes",
            serde_json::json!([{"target_node_id":0,"cost":65536}]),
        ),
    ] {
        let (mut world, scripts, mut data) = navigation_owner_fixture();
        let mut node = walk_node([0, 0, 0]);
        node[field] = value;
        if field == "routes" && node[field].is_array() {
            node["route_count"] = 1.into();
        }
        data.metadata["walk_node_maps"] = serde_json::json!([{"node_count":1,"walk_nodes":[node]}]);
        let error = format!("{:#}", world.prepare_navigation(&data).unwrap_err());
        assert!(
            error.contains(&format!(
                "world {} walk_node_maps map 0 node 0",
                scripts.world
            )),
            "{error}"
        );
    }
}
