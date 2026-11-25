use super::*;

#[test]
fn living_map_default_writes_do_not_change_the_combat_death_default() {
    use cyrus_assets::animation::Animations;
    for combat_attribute in [0, 1] {
        for (function, name) in [(1, Native::SetAttribute), (2, Native::SetMyAttr)] {
            let mut scripts = attribute_cache(vec![
                Operation::Call {
                    call: attribute_call(function, name, 44, Some(-2)),
                },
                Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(attribute_call(3, Native::GetAttribute, 44, None)),
                        },
                        rest: vec![],
                    },
                },
                Operation::End { target: 0 },
            ]);
            scripts.instances[0].object_type = PlacementType::MODEL;
            scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Combat] =
                combat_attribute;
            scripts.content_id = scripts.content_identity().unwrap();
            let data = Animations {
                version: 2,
                world: scripts.world,
                metadata: serde_json::json!({
                    "script_content_id": scripts.content_id,
                    "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                            "commands":[{"opcode":0,"frame_reference":10},
                                {"opcode":2,"value":0}]}]}]
                }),
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            };
            let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
            let id = world.id(0).unwrap();
            world
                .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
                .unwrap();
            world
                .advance(
                    id,
                    cyrus_script::script::ScriptStage::Body(Thread::Main),
                    cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            let actor = &world.actors()[0];
            assert_eq!(actor.attributes()[Attribute::PanicGroup], 254);
            assert_eq!(actor.runtime().unwrap().variables(), [254]);
            assert_eq!(
                actor.control_state().unwrap().combat_death_default,
                if combat_attribute == 0 { None } else { Some(0) }
            );
            assert_eq!(world.animation_frame(id).unwrap(), Some(10));
            assert_eq!(world.actors()[1].attributes()[Attribute::PanicGroup], 0);
        }
    }
}

#[test]
fn standalone_default_writes_use_initialized_player_control() {
    for (function, name) in [(1, Native::SetAttribute), (2, Native::SetMyAttr)] {
        let mut write = attribute_call(function, name, 44, Some(7));
        write.target = Object::Player;
        let mut scripts = attribute_cache(vec![
            Operation::ObjectCall { call: write },
            Operation::End { target: 0 },
        ]);
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.globals_mut().initialize_selected_health_for_test(40);
        world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
        let player = world
            .actors()
            .iter()
            .find(|actor| actor.name() == "CYRUS")
            .unwrap();
        assert_eq!(
            player.control_state().unwrap().living_state,
            LivingState::ORDINARY
        );
        assert_eq!(player.attributes()[Attribute::PanicGroup], 7);
        let (_, state) = world.player_activation_state().unwrap();
        assert!(state.compass_eligible(player.attributes()[Attribute::HoldingWeapon], true, false));
    }
}

#[test]
fn map_default_selection_observes_script_writes_and_updates_the_shared_frame() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = attribute_cache(vec![
        Operation::Call {
            call: attribute_call(1, Native::SetAttribute, 44, Some(262)),
        },
        Operation::Yield,
        Operation::Call {
            call: attribute_call(2, Native::SetMyAttr, 110, Some(1)),
        },
        Operation::End { target: 0 },
    ]);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups": ([0, 6, 8, 91, 129].into_iter().map(|group| serde_json::json!({
                    "group_index": group, "anim_id": 1, "anim_type": 0,
                    "commands": [{"opcode": 0, "frame_reference": group * 10},
                        {"opcode": 2, "value": 0}]
                })).collect::<Vec<_>>()) }]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(0));
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    world
        .select_map_animation_default(id, false, 1, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(60));
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    world
        .select_map_animation_default(id, false, 1, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(80));
    assert_eq!(world.actors()[0].attributes()[Attribute::PanicGroup], 6);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat_death_default,
        None
    );
    for (initial_group, items_ready, expected_frame) in [
        (129, true, 60),
        (129, false, 60),
        (91, true, 80),
        (91, false, 60),
    ] {
        let mut selected = ScriptWorld::prepare(&scripts, 1).unwrap();
        selected
            .bind_animation(id, &data, AnimationGroup::from_index(initial_group))
            .unwrap();
        selected
            .advance(id, ScriptStage::Animation, inputs, None, None, 20)
            .unwrap();
        selected
            .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
            .unwrap();
        selected
            .select_map_animation_default(id, items_ready, 1, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(selected.animation_frame(id).unwrap(), Some(expected_frame));
    }
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    assert!(
        replacement
            .select_map_animation_default(id, false, 1, &mut |_, _| Ok(()))
            .is_err()
    );
}

#[test]
fn animation_hold_pause_and_release_take_the_receiver_and_reject_wrong_argument_counts() {
    // The wait byte itself is covered in cyrus-world; an actor without playback has no record.
    for calls in [
        &[(Native::HoldAnimation, &[][..])][..],
        &[(Native::PauseAnimation, &[7][..])][..],
        &[(Native::ReleaseAnimation, &[][..])][..],
    ] {
        run_native_world(calls).unwrap();
    }
    assert!(run_native_world(&[(Native::HoldAnimation, &[1])]).is_err());
    assert!(run_native_world(&[(Native::PauseAnimation, &[])]).is_err());
    assert!(run_native_world(&[(Native::ReleaseAnimation, &[1])]).is_err());
}

pub(super) fn static_model_fixture(
    scripts: Scripts,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_collision::actor_models::ActorModelCollision,
) {
    static_model_fixture_at(scripts, None, [10, 20, 30])
}

pub(super) fn static_model_fixture_at(
    scripts: Scripts,
    extent_height: Option<f32>,
    map_position: [i64; 3],
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_collision::actor_models::ActorModelCollision,
) {
    static_model_fixture_session(scripts, extent_height, map_position, false)
}

pub(super) fn static_model_fixture_session(
    mut scripts: Scripts,
    extent_height: Option<f32>,
    map_position: [i64; 3],
    fresh_session: bool,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_collision::actor_models::ActorModelCollision,
) {
    use cyrus_assets::collision::{ActorCollisionModels, CollisionGeometry};
    use cyrus_collision::actor_models::ActorModelCollision;
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = map_position;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "mpob_objects": scripts.instances.iter().map(|_| serde_json::json!({"model_id":-1})).collect::<Vec<_>>(),
            "actors": scripts.actors.keys().map(|name| serde_json::json!({"script_name":name,"model_load_path":"static","mpsz_bounds_primary":-1,"raex":{"unknown_12":0,"v_vertex":0,"v_size":0}})).collect::<Vec<_>>()}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let statics = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 2,
        models: BTreeMap::from([(
            "STATIC".into(),
            CollisionGeometry {
                version: Some(*b"v5.0"),
                radius: 42.0,
                vertices: {
                    let mut vertices =
                        vec![[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [0.0, 0.0, 20.0]];
                    if let Some(height) = extent_height {
                        vertices.push([0.0, height, 0.0]);
                    }
                    vertices
                },
                normals: vec![[0.0, -1.0, 0.0]],
                faces: vec![vec![0, 1, 2]],
                textures: Some(vec![None]),
                subobjects: vec![cyrus_assets::collision::CollisionSubobject {
                    center: [0.0; 3],
                    radius: 42.0,
                    extents: Some([1.0; 3]),
                    faces: vec![0],
                }],
            },
        )]),
        instances: BTreeMap::from([(0, "STATIC".into()), (1, "STATIC".into())]),
        collision_flags: BTreeMap::from([(0, 6), (1, 7)]),
        unsupported: Default::default(),
        ..Default::default()
    })
    .unwrap();
    let mut world = if fresh_session {
        use bevy::{asset::AssetApp, prelude::*};
        use cyrus_assets::animation::Animations;
        use cyrus_script::script::{ScriptActors, ScriptPlugin};
        use cyrus_world::lifecycle::PreparedWorld;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Scripts>()
            .init_asset::<Animations>()
            .insert_resource(State::new(cyrus_game::AppState::InGame))
            .add_plugins(ScriptPlugin);
        let mut metadata = data.metadata.clone();
        metadata["markers"] = serde_json::json!([]);
        metadata["walk_node_maps"] = serde_json::json!([]);
        let animations = app
            .world_mut()
            .resource_mut::<Assets<Animations>>()
            .add(Animations {
                version: data.version,
                world: data.world,
                metadata,
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            });
        let scripts = app
            .world_mut()
            .resource_mut::<Assets<Scripts>>()
            .add(scripts);
        app.insert_resource(PreparedWorld {
            requested_name: "TEST".into(),
            world: Some(data.world),
            scripts,
            animations,
            player_path: "test.player".into(),
            scene_path: "test.glb".into(),
            environment_path: "test.environment".into(),
        });
        app.update();
        app.world_mut()
            .resource_mut::<ScriptActors>()
            .current
            .take()
            .unwrap()
    } else {
        ScriptWorld::prepare(&scripts, 1).unwrap()
    };
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &statics).unwrap();
    (world, data, statics)
}

#[test]
fn model_records_retain_presentation_pose_until_admitted_refresh() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: Native::MoveByAxis,
        param_count: 3,
        kind: CallKind::Task,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    1,
                    Native::MoveByAxis,
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 1 },
                        Value::Integer { value: 256 },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
    let (mut world, data, statics) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let initial = world.model_presentation(id).unwrap().unwrap();
    assert_eq!(initial.pose.position, [2560, 5120, 7680]);
    assert_eq!(initial.radius, 42.0);
    assert!(initial.animation.is_none());
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    world
        .advance(id, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 5376, 7680]
    );
    world.refresh_model_presentations(&data, &[]).unwrap();
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position[0],
        2560
    );
    let support_height = |world: &ScriptWorld| {
        let pose = world
            .model_presentation(id)
            .unwrap()
            .unwrap()
            .collision_pose()
            .unwrap();
        statics
            .segment_contacts(0, &pose, [10.0, 10.0, 30.0], [10.0, 30.0, 30.0], false)
            .unwrap()[0]
            .point[1]
    };
    assert_eq!(support_height(&world), 20.0);
    let snapshot = world.actor_snapshot(id).unwrap();
    let (captured, position) = snapshot.presentation.unwrap();
    assert_eq!(captured.position, [2560, 5120, 7680]);
    assert_eq!(position[1], 20.0);
    assert_ne!(captured.position, world.spatial(id).unwrap().local.position);
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert_eq!(support_height(&world), 21.0);
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        world.spatial(id).unwrap().local.position
    );
    assert!(
        world
            .model_presentation(cyrus_script::script::ScriptActorId {
                generation: 2,
                index: 0
            })
            .is_err()
    );
}

#[test]
fn hide_clears_model_and_show_restores_retained_static_resource_immediately() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::HideMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.definition.functions.push(Function {
        name: Native::ShowMe,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(1, Native::HideMe, CallKind::Function, vec![]),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 2 },
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: wait(2, Native::ShowMe, CallKind::Function, vec![]),
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
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    assert_eq!(world.model_position(id).unwrap(), Some([0.0; 3]));
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert!(world.model_presentation(id).unwrap().is_some());
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    assert!(world.actors()[0].control_state().unwrap().hidden);
    assert!(world.model_presentation(id).unwrap().is_none());
    assert_eq!(world.model_position(id).unwrap(), Some([10.0, 20.0, 30.0]));
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert!(world.model_presentation(id).unwrap().is_none());
    assert_eq!(world.model_position(id).unwrap(), Some([10.0, 20.0, 30.0]));
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    assert!(!world.actors()[0].control_state().unwrap().hidden);
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        [2560, 5120, 7680]
    );
    let mut wrong = data;
    wrong.world += 1;
    assert!(world.refresh_model_presentations(&wrong, &[id]).is_err());
    assert!(world.model_presentation(id).unwrap().is_some());
}

#[test]
fn show_refreshes_static_coordinates_even_without_a_retained_resource() {
    use cyrus_assets::collision::ActorCollisionModels;
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_script::script::{AnimationData, ScriptInputs, ScriptStage};
    for (path, expected) in [
        ("static", [10.0, 20.0, 30.0]),
        ("none", [0.0; 3]),
        ("animation_table", [0.0; 3]),
    ] {
        let mut scripts = random_scripts(Native::ShowMe, vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let (_, mut data, _) = static_model_fixture(scripts.clone());
        data.metadata["actors"][0]["model_load_path"] = path.into();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let empty = ActorModelCollision::new(&ActorCollisionModels {
            object_count: 2,
            collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
            ..Default::default()
        })
        .unwrap();
        world.prepare_model_resources(&data, &empty).unwrap();
        let actor = world.id(0).unwrap();
        assert_eq!(world.model_position(actor).unwrap(), Some([0.0; 3]));
        world
            .advance(
                actor,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    animations: Some(AnimationData(&data)),
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            world.model_position(actor).unwrap(),
            Some(expected),
            "{path}"
        );
        assert!(world.model_presentation(actor).unwrap().is_none(), "{path}");
        assert!(!world.actors()[0].control_state().unwrap().hidden);
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    }
}

#[test]
fn animated_model_selection_retains_radius_when_hidden_and_clears_missing_geometry() {
    use cyrus_assets::{animation::AnimationModel, collision::ActorCollisionModels};
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::HideMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, mut data, _) = static_model_fixture(scripts.clone());
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    data.metadata["actors"][0] = serde_json::json!({"script_name":"LOGIC", "model_load_path":"animation_table",
        "raan_entries":[{"file_path":"MOVING.3D", "frame_count":1}],
        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]});
    data.metadata["mpob_objects"] = serde_json::json!([{"script_name":"LOGIC", "object_type":1}]);
    data.models.insert(
        "MOVING".into(),
        AnimationModel {
            radius: Some(17.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![]],
            normals: vec![vec![]],
        },
    );
    let empty = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 2,
        collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
        ..Default::default()
    })
    .unwrap();
    world.prepare_model_resources(&data, &empty).unwrap();
    let groups: Vec<u8> = world
        .actors()
        .iter()
        .filter(|actor| actor.placement().is_some())
        .map(|actor| actor.attributes()[Attribute::MessGroup])
        .collect();
    assert!(!groups.is_empty());
    assert_eq!(groups, (0..groups.len() as u8).collect::<Vec<_>>());
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert!(world.model_presentation(id).unwrap().is_none());
    assert_eq!(world.model_radius(id).unwrap(), None);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert_eq!(world.model_radius(id).unwrap(), Some(17.0));
    assert_eq!(
        world
            .model_presentation(id)
            .unwrap()
            .unwrap()
            .animation
            .as_ref()
            .unwrap()
            .model,
        "MOVING"
    );
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert!(world.model_presentation(id).unwrap().is_none());
    assert_eq!(world.model_radius(id).unwrap(), Some(17.0));

    // A fresh visible generation selects the same frame but cannot retain missing geometry.
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut visible = ScriptWorld::prepare(&scripts, 2).unwrap();
    visible.prepare_spatial().unwrap();
    visible.prepare_model_resources(&data, &empty).unwrap();
    let id = visible.id(0).unwrap();
    visible
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    visible.refresh_model_presentations(&data, &[id]).unwrap();
    data.models.clear();
    assert!(visible.refresh_model_presentations(&data, &[id]).is_err());
    assert!(visible.model_presentation(id).unwrap().is_none());
    assert_eq!(visible.model_radius(id).unwrap(), None);
}

#[test]
fn live_collision_candidates_use_actor_metadata_and_captured_bounds_in_supplied_order() {
    use cyrus_collision::candidates::{
        CandidateOwner, CandidateQuery, CollisionCandidates, ModelBounds, ModelInstance,
    };
    let (mut world, data, _) = static_model_fixture(cache());
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[first, second])
        .unwrap();
    let live = world.live_collision_inputs(&[second, first]).unwrap();
    assert_eq!(
        live.iter()
            .map(|actor| (actor.actor, actor.collision_flags))
            .collect::<Vec<_>>(),
        [(1, 7), (0, 6)]
    );
    let query = CandidateQuery::new(
        [2560, 5120, 7680],
        Some(ModelBounds {
            instance: ModelInstance::Runtime(0),
            position: [10.0, 20.0, 30.0],
            radius: 42.0,
        }),
    )
    .unwrap();
    let candidates = CollisionCandidates::build(&query, &[], &live, &[]).unwrap();
    assert_eq!(candidates.faces.len(), 1);
    assert_eq!(candidates.faces[0].owner, CandidateOwner::Actor(1));
    assert_eq!(candidates.faces[0].collision_flags, 7);
    assert!(candidates.actors.is_empty());
    assert!(
        world
            .live_collision_inputs(&[cyrus_script::script::ScriptActorId {
                generation: 2,
                index: 0
            }])
            .is_err()
    );
}

#[test]
fn live_collision_candidates_reject_absent_models_before_object_path_metadata() {
    let mut scripts = cache();
    add_standalone_receivers(&mut scripts);
    let (mut world, data, _) = static_model_fixture(scripts);
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    let roles = (2..5)
        .map(|index| world.id(index).unwrap())
        .collect::<Vec<_>>();
    assert!(world.live_collision_inputs(&roles).unwrap().is_empty());
    world.refresh_model_presentations(&data, &[first]).unwrap();
    let order = [roles[2], second, first, roles[1], roles[0]];
    let inputs = world.live_collision_inputs(&order).unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].actor, first.index);
    assert_eq!(inputs[0].collision_flags, 6);
    assert!(world.model_presentation(second).unwrap().is_none());
    assert!(
        world
            .live_collision_inputs(&[cyrus_script::script::ScriptActorId {
                generation: world.generation() + 1,
                index: roles[2].index,
            }])
            .is_err()
    );
}

#[test]
fn script_plugin_prepares_model_resources_only_from_current_scene_asset() {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    use cyrus_assets::animation::Animations;
    use cyrus_game::AppState;
    use cyrus_script::{
        player::data::{MovementSettings, PlayerScene, PlayerSceneRequest},
        script::{ScriptActors, ScriptPlugin},
    };
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, mut data, statics) = static_model_fixture(scripts.clone());
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_asset::<PlayerScene>()
        .add_message::<cyrus_world::animation::AnimationTick>()
        .add_plugins(ScriptPlugin);
    let scripts_handle = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animation_handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.insert_resource(cyrus_world::lifecycle::PreparedWorld {
        requested_name: "test".into(),
        world: Some(7),
        scripts: scripts_handle,
        animations: animation_handle,
        player_path: "current-model.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    let stale = app
        .world()
        .resource::<AssetServer>()
        .load::<PlayerScene>("previous-model.player");
    app.insert_resource(PlayerSceneRequest(stale.clone()));
    let scene = PlayerScene {
        source_identity: None,
        world: 7,
        model_radii: Default::default(),
        control_codes: None,
        settings: MovementSettings {
            standing_height: -2,
            post_collide_height: 1,
            slide_range: 1,
            slide_speed: None,
            slide_hurt: None,
            tap_time: None,
            sheath_sword_delay: None,
            defend_interval: None,
            swim_depth: None,
            max_remap_objects: None,
            turn_speed: 1,
            turn_max_speed: 1,
            poly_push_units: 1,
            joystick: None,
        },
        definition: None,
        terrain: None,
        collision: cyrus_assets::collision::CollisionScene {
            model_visibility: Some(Default::default()),
            sphere_object_scale: Some(256),
            ..Default::default()
        },
        static_collision: cyrus_collision::collision_world::StaticCollision::new(
            &Default::default(),
        )
        .unwrap(),
        actor_collision: Some(statics),
        unavailable: None,
    };
    app.world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .insert(stale.id(), scene)
        .unwrap();
    app.update();
    {
        let actors = app.world().resource::<ScriptActors>();
        let world = actors
            .current
            .as_ref()
            .expect("current logical world was not prepared");
        assert!(world.model_radius(world.id(0).unwrap()).is_err());
    }
    let current = app
        .world()
        .resource::<AssetServer>()
        .load::<PlayerScene>("current-model.player");
    let scene = app
        .world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .remove(stale.id())
        .unwrap();
    app.world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .insert(current.id(), scene)
        .unwrap();
    app.insert_resource(PlayerSceneRequest(current));
    app.update();
    let actors = app.world().resource::<ScriptActors>();
    let world = actors
        .current
        .as_ref()
        .expect("current logical world was not prepared");
    assert_eq!(
        world.model_radius(world.id(0).unwrap()).unwrap(),
        Some(42.0)
    );
    assert!(
        world
            .model_presentation(world.id(0).unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn pvo_secondary_ids_use_load_time_z_x_order_and_preserve_cell_chains() {
    let mut scripts = cache();
    let template = scripts.instances[0].clone();
    scripts.instances = [(1, 2, 0), (2, 1, 0), (2, 1, 0), (0, 0, 1)]
        .into_iter()
        .enumerate()
        .map(|(index, (x, z, active))| Instance {
            object_id: index as u32,
            object_type: PlacementType::MODEL,
            map_position: [x * 512, 0, z * 512],
            active,
            ..template.clone()
        })
        .collect();
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 4;
    actor.instance_variables = vec![vec![4]; 4];
    actor.attributes[Attribute::NonMoving] = 1;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    let indices = |world: &ScriptWorld| {
        world
            .initial_pvo_secondary_actors()
            .unwrap()
            .into_iter()
            .map(|id| id.index)
            .collect::<Vec<_>>()
    };
    assert_eq!(indices(&world), [2, 1, 0, 3]);
    world.ordinary_actors([65535, 65535]).unwrap();
    assert_eq!(indices(&world), [2, 1, 0, 3]);
}

#[test]
fn model_initialization_with_the_animation_system_off_selects_no_group_or_frame() {
    // The request routine returns before touching the record, so the two initial panic
    // requests select nothing: the actor has no frame and the original does not draw it.
    for (animation_system, frame) in [(true, Some(0)), (false, None)] {
        let mut scripts = cache();
        scripts.updates.animation_system = animation_system;
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.instances = 1;
        actor.instance_variables = vec![vec![4]];
        scripts.instances.truncate(1);
        scripts.instances[0].object_type = PlacementType::MODEL;
        scripts.instances[0].active = 1;
        scripts.instances[0].map_position = [10, 20, 30];
        scripts.content_id = scripts.content_identity().unwrap();
        let (_, mut data, _) = static_model_fixture(scripts.clone());
        data.metadata["actors"] = serde_json::json!([
            {"script_name":"LOGIC", "model_load_path":"animation_table",
             "raan_entries":[{"file_path":"MOVING.3D","frame_count":1}],
             "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                 "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}
        ]);
        data.metadata["mpob_objects"] =
            serde_json::json!([{"script_name":"LOGIC", "object_type":1}]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            world.animation_frame(id).unwrap(),
            frame,
            "animation system {animation_system}"
        );
        assert!(world.actors()[0].failure().is_none());
    }
}

#[test]
fn automatic_model_admission_reads_retained_support_before_ground_processing() {
    use cyrus_assets::{
        animation::AnimationModel,
        collision::{ModelVisibility, VisibilityNode},
    };
    use cyrus_script::script::ModelSupportOwners;
    let mut scripts = cache();
    let mut support = scripts.actors["LOGIC"].clone();
    support.instances = 1;
    support.instance_variables = vec![vec![4]];
    support.attributes[Attribute::NonMoving] = 1;
    scripts.actors.insert("SUPPORT".into(), support);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables = vec![vec![4]];
    scripts.instances[1].actor = "SUPPORT".into();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.active = 1;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, mut data, statics) = static_model_fixture(scripts.clone());
    data.metadata["actors"] = serde_json::json!([
        {"script_name":"LOGIC", "model_load_path":"animation_table",
         "raan_entries":[{"file_path":"MOVING.3D","frame_count":1}],
         "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
             "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]},
        {"script_name":"SUPPORT","model_load_path":"static"}
    ]);
    data.metadata["mpob_objects"] = serde_json::json!([
        {"script_name":"LOGIC", "object_type":1},
        {"script_name":"SUPPORT", "object_type":1}
    ]);
    data.models.insert(
        "MOVING".into(),
        AnimationModel {
            radius: Some(17.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![]],
            normals: vec![vec![]],
        },
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &statics).unwrap();
    let actor = world.id(0).unwrap();
    let support_actor = world.id(1).unwrap();
    world
        .initialize_map_animation(actor, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    let mut root = VisibilityNode {
        children: [None; 8],
        objects: vec![0],
    };
    root.children[7] = Some(1);
    world
        .prepare_model_admission(
            &ModelVisibility {
                center: [0; 3],
                half_extent: 1024,
                nodes: vec![
                    root,
                    VisibilityNode {
                        children: [None; 8],
                        objects: vec![1],
                    },
                ],
            },
            1,
        )
        .unwrap();
    assert_eq!(
        world.model_support(actor).unwrap(),
        ModelSupportOwners::default()
    );
    // Rejected support still positions an actor with a selected frame, without loading geometry.
    assert_eq!(world.model_position(actor).unwrap(), Some([0.0; 3]));
    world
        .set_model_support(
            actor,
            ModelSupportOwners {
                actor: Some(support_actor),
                static_placement: None,
            },
        )
        .unwrap();
    let resource = data.models.remove("MOVING").unwrap();
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(
        world.actors()[0].failure().is_none(),
        "{:?}",
        world.actors()[0].failure()
    );
    assert!(world.model_presentation(actor).unwrap().is_none());
    assert_eq!(
        world.model_position(actor).unwrap(),
        Some([10.0, 20.0, 30.0])
    );
    data.models.insert("MOVING".into(), resource);
    world
        .set_model_support(actor, ModelSupportOwners::default())
        .unwrap();
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(world.model_presentation(actor).unwrap().is_some());
    // The refresh stores the heading change since the update-start history in `yangle_change`.
    assert_eq!(
        world.actors()[actor.index]
            .control_state()
            .unwrap()
            .yangle_change,
        0
    );
    world.set_local_angles_for_test(actor, [0, 300, 0]);
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.actors()[actor.index]
            .control_state()
            .unwrap()
            .yangle_change,
        300
    );
    world.set_local_angles_for_test(actor, [0, 0, 0]);
    assert!(world.model_presentation(support_actor).unwrap().is_none());
    world
        .set_model_support(
            actor,
            ModelSupportOwners {
                actor: Some(support_actor),
                static_placement: None,
            },
        )
        .unwrap();
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(world.model_presentation(actor).unwrap().is_none());
    assert_eq!(world.model_radius(actor).unwrap(), None);
    world
        .refresh_automatic_models(
            &data,
            [256; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(world.model_presentation(actor).unwrap().is_some());
    world
        .set_model_support(
            actor,
            ModelSupportOwners {
                actor: None,
                static_placement: Some(0),
            },
        )
        .unwrap();
    world
        .refresh_automatic_models(
            &data,
            [256; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(world.model_presentation(actor).unwrap().is_none());
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(world.model_presentation(actor).unwrap().is_some());
    assert_eq!(
        world.model_support(actor).unwrap().static_placement,
        Some(0)
    );
    assert!(
        world
            .set_model_support(
                actor,
                ModelSupportOwners {
                    actor: Some(cyrus_script::script::ScriptActorId {
                        generation: 2,
                        index: 1
                    }),
                    static_placement: None
                }
            )
            .is_err()
    );
    assert_eq!(
        world.model_support(actor).unwrap().static_placement,
        Some(0)
    );
    world
        .update_ordinary(
            &data,
            [0; 2],
            cyrus_script::script::OrdinaryInputs {
                scene: None,
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
    assert_eq!(
        world.model_support(actor).unwrap(),
        ModelSupportOwners::default()
    );
    assert!(world.model_presentation(actor).unwrap().is_some());
    world
        .refresh_model_presentations(&data, &[support_actor])
        .unwrap();
    assert!(world.model_presentation(support_actor).unwrap().is_some());
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            cyrus_script::script::ScriptInputs::default(),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert!(world.model_presentation(support_actor).unwrap().is_some());
    assert_eq!(
        world.model_position(support_actor).unwrap(),
        Some([10.0, 20.0, 30.0])
    );
}

#[test]
fn stored_model_position_roundtrips_independently_of_logical_coordinates() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, statics) = static_model_fixture(scripts.clone());
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    let restore = |saved| {
        ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 9,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
    };
    let mut displaced = saved.clone();
    displaced["actors"][0]["model"]["current"]["position"] =
        serde_json::json!([10.0_f32.to_bits(), 200.5_f32.to_bits(), 30.0_f32.to_bits()]);
    displaced["actors"][0]["model"]["position"] =
        displaced["actors"][0]["model"]["current"]["position"].clone();
    let restored = restore(displaced).unwrap();
    let id = restored.id(0).unwrap();
    assert_eq!(
        restored.spatial(id).unwrap().local.position,
        [2560, 5120, 7680]
    );
    let model = restored.model_presentation(id).unwrap().unwrap();
    assert_eq!(model.position, [10.0, 200.5, 30.0]);
    assert_eq!(model.pose.position, [2560, 5120, 7680]);
    assert_eq!(
        restored.live_collision_inputs(&[id]).unwrap()[0]
            .model
            .as_ref()
            .unwrap()
            .position,
        [10.0, 200.5, 30.0]
    );
    let hits = statics
        .segment_contacts(
            0,
            &model.collision_pose().unwrap(),
            [10.0, 190.0, 30.0],
            [10.0, 210.0, 30.0],
            false,
        )
        .unwrap();
    assert_eq!(hits[0].point[1], 200.5);
    assert_eq!(
        serde_json::to_value(restored.snapshot(&sources).unwrap()).unwrap()["actors"][0]["model"]["current"]
            ["position"],
        serde_json::json!([10.0_f32.to_bits(), 200.5_f32.to_bits(), 30.0_f32.to_bits()])
    );
    let mut old = saved.clone();
    old["actors"][0]["model"]
        .as_object_mut()
        .unwrap()
        .remove("position");
    old["actors"][0]["model"]["current"]
        .as_object_mut()
        .unwrap()
        .remove("position");
    let old = restore(old).unwrap();
    assert_eq!(
        old.model_presentation(old.id(0).unwrap())
            .unwrap()
            .unwrap()
            .position,
        [10.0, 20.0, 30.0]
    );
    assert_eq!(
        old.model_position(old.id(0).unwrap()).unwrap(),
        Some([10.0, 20.0, 30.0])
    );
    let mut hidden = saved.clone();
    hidden["actors"][0]["model"]["current"] = serde_json::Value::Null;
    let retained = restore(hidden.clone()).unwrap();
    assert_eq!(
        retained.model_position(retained.id(0).unwrap()).unwrap(),
        Some([10.0, 20.0, 30.0])
    );
    hidden["actors"][0]["model"]
        .as_object_mut()
        .unwrap()
        .remove("position");
    let unknown = restore(hidden).unwrap();
    assert_eq!(
        unknown.model_position(unknown.id(0).unwrap()).unwrap(),
        None
    );
    let mut no_resource = saved.clone();
    no_resource["actors"][0]["model"]["current"] = serde_json::Value::Null;
    no_resource["actors"][0]["model"]["static_radius"] = serde_json::Value::Null;
    no_resource["actors"][0]["model"]["position"] = serde_json::json!([0, 0, 0]);
    let mut no_resource = restore(no_resource).unwrap();
    let actor = no_resource.id(0).unwrap();
    no_resource
        .refresh_model_presentations(&data, &[actor])
        .unwrap();
    assert!(no_resource.model_presentation(actor).unwrap().is_none());
    assert_eq!(
        no_resource.model_position(actor).unwrap(),
        Some([10.0, 20.0, 30.0])
    );
    let mut malformed_retained = saved.clone();
    malformed_retained["actors"][0]["model"]["position"][1] = f32::INFINITY.to_bits().into();
    assert!(
        format!("{:#}", restore(malformed_retained).err().unwrap())
            .contains("saved retained model position invalid")
    );
    let mut inconsistent = saved.clone();
    inconsistent["actors"][0]["model"]["position"][1] = 21.0_f32.to_bits().into();
    assert!(
        format!("{:#}", restore(inconsistent).err().unwrap())
            .contains("saved current and retained model position disagree")
    );
    let mut malformed = saved;
    malformed["actors"][0]["model"]["current"]["position"][1] = f32::NAN.to_bits().into();
    assert!(
        format!("{:#}", restore(malformed).err().unwrap()).contains("saved model position invalid")
    );
}

#[test]
fn model_positioning_allows_known_dry_surfaces_with_collision_enabled() {
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .extend([(Attribute::Collide, 4), (Attribute::JumpTime, 8)]);
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    for surface in [0, 6, 7, 8, 9, 10] {
        world
            .set_actor_surface(id, Surface::from_byte(surface))
            .unwrap();
        world.refresh_model_presentations(&data, &[id]).unwrap();
        assert_eq!(
            world.model_presentation(id).unwrap().unwrap().pose.position,
            [2560, 5120, 7680]
        );
    }
    world.set_actor_surface(id, Surface::DEEPWATER).unwrap();
    assert!(
        world
            .refresh_model_presentations(&data, &[id])
            .unwrap_err()
            .to_string()
            .contains("water-dependent model position")
    );
    assert!(world.model_presentation(id).unwrap().is_none());
}

fn family_model_fixture(collision: u8) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = heading_property_scripts(294, Object::Me);
    scripts.definition.references[294] = Property::PosVx;
    let mut child = scripts.actors["LOGIC"].clone();
    child.attributes[Attribute::Slave] = 1;
    child.attributes[Attribute::Collide] = collision;
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Master] = 1;
    scripts.actors.insert("CHILD".into(), child);
    scripts.instances[1].actor = "CHILD".into();
    scripts.instances[0].angles = [0; 3];
    scripts.instances[1].angles = [0; 3];
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 294,
        },
        formula: Formula {
            first: Value::Integer { value: -257 },
            rest: vec![],
        },
    };
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (world, data, _) = static_model_fixture(scripts);
    (world, data)
}

#[test]
fn family_refresh_updates_current_model_without_resampling_geometry_or_root() {
    let (mut world, data) = family_model_fixture(4);
    let root = world.id(0).unwrap();
    let child = world.id(1).unwrap();
    world.set_actor_surface(child, Surface::DEEPWATER).unwrap();
    world.refresh_families().unwrap();
    world
        .refresh_model_presentations(&data, &[root, child])
        .unwrap();
    let before = world.model_presentation(child).unwrap().unwrap();
    let radius = before.radius;
    assert!(before.animation.is_none());
    world
        .advance(
            root,
            cyrus_script::script::ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world.refresh_families().unwrap();
    let current = world.model_presentation(child).unwrap().unwrap();
    assert_eq!(current.position, [-2.0, 20.0, 30.0]);
    assert_eq!(current.pose.position, [-257, 5120, 7680]);
    assert_eq!(
        current.pose.matrix,
        world.spatial(child).unwrap().effective.unwrap().matrix
    );
    assert_eq!(current.radius, radius);
    assert!(current.animation.is_none());
    assert_eq!(world.model_position(child).unwrap(), Some(current.position));
    assert_eq!(
        world.model_presentation(root).unwrap().unwrap().position,
        [10.0, 20.0, 30.0]
    );
}

#[test]
fn ordinary_heading_assignment_waits_for_the_next_model_matrix_refresh() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = heading_property_scripts(294, Object::Me);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 294,
        },
        formula: Formula {
            first: Value::Integer { value: 512 },
            rest: vec![],
        },
    };
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    world
        .prepare_model_admission(&Default::default(), 0)
        .unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            inputs,
            Some(&mut random),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let before = world.spatial(id).unwrap().local;
    assert_eq!(before.angles[1], 1536);
    world
        .update_ordinary(
            &data,
            [0; 2],
            OrdinaryInputs {
                scripts: inputs,
                scene: None,
            },
            &mut random,
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(
        world.actors()[id.index].failure().is_none(),
        "{:?}",
        world.actors()[id.index].failure()
    );
    let after = world.spatial(id).unwrap();
    assert_eq!(after.local.angles[1], 512);
    assert_eq!(after.local.matrix, before.matrix);
    assert_eq!(after.angle_history[1], 1536);
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            inputs,
            Some(&mut random),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    let published = world.spatial(id).unwrap();
    assert_eq!(published.local.angles[1], 512);
    assert_ne!(published.local.matrix, before.matrix);
    assert_eq!(published.effective.unwrap().angles[1], 512);
}

#[test]
fn automatic_family_models_capture_master_before_ordinary_script_movement() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let (mut world, data) = family_model_fixture(0);
    let root = world.id(0).unwrap();
    let child = world.id(1).unwrap();
    world
        .prepare_model_admission(&Default::default(), 0)
        .unwrap();
    world.refresh_families().unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            inputs,
            Some(&mut random),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.model_position(child).unwrap(),
        Some([10.0, 20.0, 30.0])
    );
    assert_eq!(
        world.model_presentation(child).unwrap().unwrap().position,
        [10.0, 20.0, 30.0]
    );
    world
        .update_ordinary(
            &data,
            [0; 2],
            OrdinaryInputs {
                scripts: inputs,
                scene: None,
            },
            &mut random,
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(
        world.actors()[root.index].failure().is_none(),
        "{:?}",
        world.actors()[root.index].failure()
    );
    assert_eq!(
        world.spatial(root).unwrap().local.position,
        [-257, 5120, 7680]
    );
    assert_eq!(
        world.model_position(child).unwrap(),
        Some([10.0, 20.0, 30.0]),
        "ordinary scripts must not rewrite this frame's attached presentation"
    );
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position,
        [2560, 5120, 7680]
    );
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            inputs,
            Some(&mut random),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.model_position(child).unwrap(),
        Some([-2.0, 20.0, 30.0])
    );
    assert_eq!(
        world
            .model_presentation(child)
            .unwrap()
            .unwrap()
            .pose
            .position,
        [-257, 5120, 7680]
    );
}

#[test]
fn automatic_group_release_waits_for_the_next_model_phase() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
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
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (mut world, data, _) = static_model_fixture(scripts);
    world
        .prepare_model_admission(&Default::default(), 0)
        .unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            inputs,
            Some(&mut random),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    world
        .update_ordinary(
            &data,
            [0; 2],
            OrdinaryInputs {
                scripts: inputs,
                scene: None,
            },
            &mut random,
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    for index in 0..2 {
        assert_eq!(
            world.actors()[index].runtime().unwrap().active_tasks(),
            1,
            "each member must be waiting before the next family refresh"
        );
        let id = world.id(index).unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
        world
            .advance(
                id,
                cyrus_script::script::ScriptStage::Tasks,
                inputs,
                None,
                None,
                100,
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].runtime().unwrap().active_tasks(),
            1,
            "newly posted groups must remain unreleased during this simulation pass"
        );
    }
    world
        .refresh_automatic_models(
            &data,
            [0; 3],
            None,
            inputs,
            Some(&mut random),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    for index in 0..2 {
        let id = world.id(index).unwrap();
        world
            .advance(
                id,
                cyrus_script::script::ScriptStage::Tasks,
                inputs,
                None,
                None,
                100,
            )
            .unwrap();
        assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn ordinary_working_orientation_copies_current_angles_before_script_rotation() {
    use cyrus_script::script::{
        OrdinaryBodyQuery, OrdinaryResidualQuery, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
        placement.active = 1;
    }
    let function = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::SetRotation,
        kind: CallKind::Function,
        param_count: 3,
    });
    let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
    program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    function,
                    Native::SetRotation,
                    CallKind::Function,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer { value: 512 },
                        Value::Integer { value: 0 },
                    ],
                ),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Increment {
                    destination: Place::Local { index: 0 },
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
    let (mut world, mut data, models) = static_model_fixture(scripts.clone());
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    world.refresh_model_presentations(&data, &ids).unwrap();
    // Base vertex (-20,0,-20) plus (120,-1,20) yields (100,-1,0).
    data.metadata["actors"][0]["ravc_count"] = 1.into();
    data.metadata["actors"][0]["collision_volumes"] = serde_json::json!([
        {"vertex":0,"offset_x":120,"offset_y":-1,"offset_z":20,"radius":24}
    ]);
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
    // A completed prior heading differs from the current selected shape orientation.
    // Current yaw zero puts the sphere over the other actor's face at (10,20,30).
    // Prior yaw 512 puts it 100 units away on Z instead.
    saved["spatial"]["actors"][0]["local"]["position"] =
        serde_json::json!([-90 * 256, 20 * 256, 30 * 256]);
    saved["spatial"]["actors"][0]["angle_history"] = serde_json::json!([0, 512, 0]);
    world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 0, 0]);
    assert_eq!(world.spatial(id).unwrap().angle_history, [0, 512, 0]);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let primary = |prepared| OrdinaryBodyQuery {
        scene: &scene,
        animations: &data,
        prepared,
        sidedness: false,
        excluded_candidate_type: None,
        terrain: None,
    };
    let residual = |prepared| OrdinaryResidualQuery {
        scene: &scene,
        animations: &data,
        prepared,
        sidedness: false,
    };
    assert!(
        !world
            .query_ordinary_body(id, primary(&prepared))
            .unwrap()
            .is_empty()
    );
    assert!(
        !world
            .query_ordinary_residual_body(id, residual(&prepared))
            .unwrap()
            .is_empty()
    );
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.angles, [0, 512, 0]);
    assert!(
        world
            .query_ordinary_body(id, primary(&prepared))
            .unwrap()
            .is_empty()
    );
    assert!(
        !world
            .query_ordinary_residual_body(id, residual(&prepared))
            .unwrap()
            .is_empty()
    );
    let next = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        world
            .query_ordinary_residual_body(id, residual(&next))
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
    for expected in [6, 7] {
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
    }
}
