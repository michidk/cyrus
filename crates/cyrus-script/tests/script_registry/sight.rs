use super::*;

#[test]
fn visibility_calls_return_zero_without_disabling_the_receivers_script() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    for (function, hidden) in [
        (Native::HideMe, true),
        (Native::ShowMe, false),
        (Native::IsDead, false),
    ] {
        let mut scripts = random_scripts(function, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        for _ in 0..2 {
            world
                .advance(
                    id,
                    ScriptStage::OrdinaryMain,
                    ScriptInputs {
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!(control.hidden, hidden);
            assert!(!control.object_update_excluded);
            assert_eq!(control.script_control, 0);
            assert!(!world.actors()[1].control_state().unwrap().hidden);
        }
    }
}

#[test]
fn actor_in_sight_is_blocked_by_candidate_models_and_gated_by_distance() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::OrdinaryObstacleQuery;
    // Actors sit 262144 Q8 units apart along X; the optional wall stands between them.
    for (wall, limit, expected) in [
        (true, i32::MAX, false),
        (false, i32::MAX, true),
        (false, 262_144, true),
        (false, 262_143, false),
    ] {
        let (mut world, data) = overlap_response_fixture(128, 0, 1024);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        world.snapshot_spatial(ids[0]).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: wall
                .then(|| StaticPlacement {
                    id: 1,
                    model: "WALL".into(),
                    position: [604, 20, 30],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .into_iter()
                .collect(),
            models: BTreeMap::from([("WALL".into(), ordinary_forward_wall_model())]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        assert!(
            world
                .player_in_sight_retained(ids[0], ids[1], &scene, &data)
                .unwrap(),
            "before the first gather, nearby geometry alone must not block sight"
        );
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &[ids[0], ids[1]])
            .unwrap();
        let query = OrdinaryObstacleQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            vertical_offset: 0,
            respond: false,
        };
        assert_eq!(
            world.actor_in_sight(ids[0], ids[1], limit, query).unwrap(),
            expected,
            "wall {wall}, limit {limit}"
        );
        // PlayerInSight uses the fixed limit 262144 (inclusive), which these actors sit at exactly.
        assert_eq!(
            world.player_in_sight(ids[0], ids[1], query).unwrap(),
            !wall,
            "wall {wall}"
        );
        // A receiver that gathered nothing reads the list the latest gather left behind.
        assert_eq!(
            world
                .player_in_sight_retained(ids[0], ids[1], &scene, &data)
                .unwrap(),
            !wall,
            "retained list, wall {wall}"
        );
        let mut empty_scene =
            ordinary_collision_scene(world.world, static_model_fixture(cache()).2);
        empty_scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        };
        empty_scene.static_collision = StaticCollision::new(&empty_scene.collision).unwrap();
        world
            .prepare_ordinary_collision_query(ids[1], &empty_scene, &data, [10, 30], &[])
            .unwrap();
        assert!(
            world
                .player_in_sight_retained(ids[0], ids[1], &scene, &data)
                .unwrap(),
            "another actor's empty gather replaces the wall list rather than accumulating it"
        );
    }
}

#[test]
fn ai_clearance_collects_at_local_position_before_resolving_linked_or_attached_origin() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::{OrdinaryObstacleQuery, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.map_position = [0; 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, models) = static_model_fixture_at(scripts.clone(), None, [0; 3]);
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.collision = CollisionScene {
        sphere_object_scale: Some(256),
        placements: vec![StaticPlacement {
            id: 1,
            model: "WALL".into(),
            position: [10256, 0, 0],
            rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
        }],
        models: BTreeMap::from([("WALL".into(), ordinary_forward_wall_model())]),
        ..Default::default()
    };
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for (local_x, attached, expected_clear) in
        [(0, false, true), (0, true, true), (10000, false, false)]
    {
        let mut snapshot = saved.clone();
        snapshot["spatial"]["actors"][0]["local"]["position"] =
            serde_json::json!([local_x * 256, 0, 0]);
        snapshot["spatial"]["actors"][0]["linked_position"] = serde_json::json!([2560000, 0, 0]);
        if attached {
            snapshot["actors"][0]["attributes"][Attribute::Slave as usize] = 1.into();
            let mut pose = snapshot["spatial"]["actors"][0]["local"].clone();
            pose["position"] = serde_json::json!([2560000, 0, 0]);
            snapshot["spatial"]["actors"][0]["effective"] = pose;
        }
        let mut world = ScriptWorld::from_snapshot(
            serde_json::from_value(snapshot).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let before = world.inspect(0, 2);
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10000, 0], &[])
            .unwrap();
        let clear = world
            .ai_destination_clear(
                ids[0],
                ids[1],
                [2691072, 0, 0],
                OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 0,
                    respond: false,
                },
            )
            .unwrap();
        assert_eq!(
            clear, expected_clear,
            "local X {local_x}, attached {attached}"
        );
        assert_eq!(world.inspect(0, 2), before);
    }
}

#[test]
fn blocked_ahead_is_false_without_a_combat_record_and_fighting_group() {
    use cyrus_assets::collision::CollisionScene;
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_script::script::OrdinaryObstacleQuery;
    let (mut world, data) = overlap_response_fixture(128, 0, 1024);
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    world.snapshot_spatial(ids[0]).unwrap();
    let (_, _, models) = static_model_fixture(cache());
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.collision = CollisionScene {
        sphere_object_scale: Some(256),
        ..Default::default()
    };
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &[ids[0], ids[1]])
        .unwrap();
    let query = OrdinaryObstacleQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        vertical_offset: 0,
        respond: false,
    };
    // Neither fixture actor has a combat record, so the probe never runs.
    assert!(!world.blocked_ahead(ids[0], 100, query).unwrap());
}

#[test]
fn sight_uses_attribute_selected_attached_then_linked_then_local_positions() {
    use cyrus_script::script::{OrdinaryObstacleQuery, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
        instance.map_position = [0; 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, models) = static_model_fixture_at(scripts.clone(), None, [0; 3]);
    let scene = ordinary_collision_scene(world.world, models);
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for (attached, linked, effective, expected) in [
        (0, None, None, true),
        (0, Some([100000, 0, 0]), None, false),
        (1, Some([100000, 0, 0]), Some([0, 0, 0]), true),
        (1, None, Some([100000, 0, 0]), false),
    ] {
        let mut snapshot = saved.clone();
        snapshot["actors"][0]["attributes"][26] = attached.into();
        snapshot["spatial"]["actors"][0]["linked_position"] = serde_json::json!(linked);
        snapshot["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([20480, 0, 0]);
        if let Some(position) = effective {
            let mut pose = snapshot["spatial"]["actors"][0]["local"].clone();
            pose["position"] = serde_json::json!(position);
            snapshot["spatial"]["actors"][0]["effective"] = pose;
        }
        let mut world = ScriptWorld::from_snapshot(
            serde_json::from_value(snapshot).unwrap(),
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
        let query = OrdinaryObstacleQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            vertical_offset: 0,
            respond: false,
        };
        assert_eq!(
            world.actor_in_sight(ids[0], ids[1], 20480, query).unwrap(),
            expected,
            "attached {attached}, linked {linked:?}, effective {effective:?}"
        );
    }
}
