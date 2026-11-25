use super::*;

#[test]
fn ordinary_body_uses_vertex_bound_shape_instead_of_initial_spheres() {
    use cyrus_assets::animation::AnimationModel;
    use cyrus_script::script::{OrdinaryBodyQuery, OrdinaryResidualQuery};
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 1;
    let (mut world, mut data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world.refresh_model_presentations(&data, &[other]).unwrap();
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    data.metadata["actors"][0]["ravc_count"] = 1.into();
    data.metadata["actors"][0]["collision_volumes"] = serde_json::json!([
        {"vertex":0,"offset_x":0,"offset_y":0,"offset_z":0,"radius":24}
    ]);
    data.metadata["actors"][0]["raan_entries"] = serde_json::json!([
        {"file_path":"VOLUME.3D","frame_count":1}
    ]);
    data.metadata["actors"][0]["animation_groups"] = serde_json::json!([
        {"group_index":0,"anim_id":1,"anim_type":0,
         "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}
    ]);
    data.metadata["mpob_objects"][0]["script_name"] = "LOGIC".into();
    data.metadata["mpob_objects"][0]["object_type"] = 1.into();
    data.models.insert(
        "VOLUME".into(),
        AnimationModel {
            radius: Some(100.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![[100.0, 0.0, 0.0]]],
            normals: vec![vec![]],
        },
    );
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        world
            .query_ordinary_body(
                id,
                OrdinaryBodyQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                    excluded_candidate_type: None,
                    terrain: None,
                }
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    // Move the selected vertex back onto the support face without changing initial bounds.
    data.models.get_mut("VOLUME").unwrap().positions[0][0] = [0.0, -1.0, 0.0];
    assert!(
        !world
            .query_ordinary_body(
                id,
                OrdinaryBodyQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                    excluded_candidate_type: None,
                    terrain: None,
                }
            )
            .unwrap()
            .is_empty()
    );

    // The residual record was copied before that vertex changed. Primary and destination
    // read the live selection, but residual must keep the distant copied sphere.
    assert!(
        world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        !world
            .ordinary_destination_body_clear(
                id,
                world.spatial(id).unwrap().local.position,
                cyrus_script::script::OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 0,
                    respond: false,
                }
            )
            .unwrap()
    );
    let rebound = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        !world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &rebound,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );

    data.models.get_mut("VOLUME").unwrap().positions[0][0] = [100.0, 0.0, 0.0];
    let position = world.spatial(id).unwrap().local.position;
    for expected in [5, 6, 7] {
        world
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_script::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
        assert_eq!(world.spatial(id).unwrap().local.position, position);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            0
        );
    }
}

#[test]
fn ordinary_base_model_ravc_uses_scene_vertices_and_rebinds_working_records() {
    use cyrus_script::script::{OrdinaryBodyQuery, OrdinaryObstacleQuery, OrdinaryResidualQuery};
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.active = 1;
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Collide] = 1;
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, models) = static_model_fixture(scripts.clone());
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    data.metadata["actors"][0]["ravc_count"] = 1.into();
    // Base vertex zero is (-20, 0, -20). Signed offsets put its sphere at
    // (100, 0, 0), away from the face, despite the initial sphere intersecting it.
    data.metadata["actors"][0]["collision_volumes"] = serde_json::json!([
        {"vertex":0,"offset_x":120,"offset_y":0,"offset_z":20,"radius":24}
    ]);
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        world
            .query_ordinary_body(
                id,
                OrdinaryBodyQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                    excluded_candidate_type: None,
                    terrain: None,
                }
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    let position = world.spatial(id).unwrap().local.position;
    assert!(
        world
            .ordinary_destination_body_clear(
                id,
                position,
                OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 0,
                    respond: false,
                }
            )
            .unwrap()
    );
    // Rebind the current selection to (0, -1, 0). Only a newly prepared working
    // record may see that change; candidates retain their original admission.
    data.metadata["actors"][0]["collision_volumes"][0]["offset_x"] = 20.into();
    data.metadata["actors"][0]["collision_volumes"][0]["offset_y"] = (-1).into();
    assert!(
        !world
            .query_ordinary_body(
                id,
                OrdinaryBodyQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                    excluded_candidate_type: None,
                    terrain: None,
                }
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        !world
            .ordinary_destination_body_clear(
                id,
                position,
                OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 0,
                    respond: false,
                }
            )
            .unwrap()
    );
    assert!(
        world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    let rebound = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        !world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &rebound,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    // Unsupported vertex indices fail contextually, without poisoning the retained
    // working copy; repairing the source admits the next ordinary update.
    data.metadata["actors"][0]["collision_volumes"][0]["vertex"] = 999.into();
    let error = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("vertex 999"));
    assert!(
        !world
            .query_ordinary_residual_body(
                id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &rebound,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    data.metadata["actors"][0]["collision_volumes"][0]["vertex"] = 0.into();
    data.metadata["actors"][0]["collision_volumes"][0]["offset_x"] = 120.into();
    data.metadata["actors"][0]["collision_volumes"][0]["offset_y"] = 0.into();
    for expected in [5, 6, 7] {
        world
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_script::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
        assert_eq!(world.spatial(id).unwrap().local.position, position);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits(),
            0
        );
    }

    // Restoration persists actor state, then binds new working records to the
    // restored generation and existing scene geometry. Old query IDs are rejected.
    let sources = cyrus_script::script::WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let mut restored = ScriptWorld::from_snapshot(
        world.snapshot(&sources).unwrap(),
        &sources,
        cyrus_script::script::WorldRestoreContext {
            generation: 2,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let restored_id = restored.id(0).unwrap();
    assert!(
        restored
            .query_ordinary_residual_body(
                restored_id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    sidedness: false,
                }
            )
            .is_err()
    );
    let fresh = restored
        .prepare_ordinary_collision_query(
            restored_id,
            &scene,
            &data,
            [10, 30],
            &[restored_id, restored.id(1).unwrap()],
        )
        .unwrap();
    assert!(
        restored
            .query_ordinary_residual_body(
                restored_id,
                OrdinaryResidualQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &fresh,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(restored.actors()[0].runtime().unwrap().variables(), [7]);
    assert_eq!(
        restored.spatial(restored_id).unwrap().local.position,
        position
    );
}
