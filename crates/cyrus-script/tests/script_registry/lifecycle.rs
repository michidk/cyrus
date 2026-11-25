use super::*;

#[test]
fn killed_map_actor_keeps_its_named_pose() -> anyhow::Result<()> {
    use cyrus_script::script::ScriptInputs;
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.definition.references = vec![Property::AngleVy];
    scripts.instances[0].angles[1] = 111;
    scripts.instances[1].angles[1] = 222;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["logic".into()];
    actor.program.entry = 3;
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(1, Native::KillMe, CallKind::Function, vec![]),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Endint,
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Read {
                            place: Place::Property {
                                object: Object::Named { string: 0 },
                                reference: 0,
                            },
                        },
                        rest: vec![],
                    },
                },
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: Operation::End { target: 3 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity()?;
    let mut world = ScriptWorld::prepare(&scripts, 1)?;
    world.prepare_spatial()?;
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    world.advance(
        world.id(0)?,
        ScriptStage::Body(Thread::Interrupt),
        inputs,
        Some(&mut |_, _| Ok(())),
        None,
        20,
    )?;
    world.update_created(
        inputs,
        &mut cyrus_game::random::EngineRandom::from_state(42),
        &mut |_, _| Ok(()),
        20,
    )?;
    world.advance(
        world.id(1)?,
        ScriptStage::Body(Thread::Main),
        inputs,
        None,
        None,
        20,
    )?;
    assert!(world.actors()[0].control_state().unwrap().killed);
    let killed = world.id(0)?;
    assert_eq!(world.spatial(killed)?.query_pose()?.angles[1], 111);
    world.set_actor_surface(killed, Surface::WOOD)?;
    assert_eq!(world.actor_surface(killed)?, Surface::WOOD);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), &[111]);
    Ok(())
}

#[test]
fn retired_created_references_are_unsupported_without_promoting_duplicates() -> anyhow::Result<()> {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    for (restore, target) in [false, true].into_iter().flat_map(|restore| {
        [Object::Named { string: 0 }, Object::Local { index: 1 }]
            .into_iter()
            .map(move |target| (restore, target))
    }) {
        let mut scripts = cache();
        scripts.actors.get_mut("LOGIC").unwrap().namespace = None;
        scripts.definition.references = vec![Property::AngleVy, Property::Address];
        let kill = scripts.definition.functions.len() as u16;
        scripts.definition.functions.push(Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        });
        let mut actor = scripts.actors["LOGIC"].clone();
        actor.instances = 1;
        actor.code_origin += 1000;
        actor.header_index += 100;
        actor.namespace = Some("projectile".into());
        actor.instance_variables = vec![vec![0, 0]];
        actor.program.variables = vec![0, 0];
        actor.program.strings = vec!["projectile".into()];
        actor.program.entry = 3;
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(kill, Native::KillMe, CallKind::Function, vec![]),
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Assign {
                        destination: Place::Local { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Property {
                                    object: Object::Named { string: 0 },
                                    reference: 0,
                                },
                            },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::Endint,
                },
            ),
            (
                4,
                Step {
                    next: 5,
                    operation: Operation::Assign {
                        destination: Place::Local { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Property {
                                    object: target.clone(),
                                    reference: 0,
                                },
                            },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                5,
                Step {
                    next: 6,
                    operation: Operation::End { target: 4 },
                },
            ),
        ]);
        actor.program.steps.insert(
            3,
            Step {
                next: 4,
                operation: Operation::Assign {
                    destination: Place::Local { index: 1 },
                    formula: Formula {
                        first: Value::Read {
                            place: Place::Property {
                                object: Object::Named { string: 0 },
                                reference: 1,
                            },
                        },
                        rest: vec![],
                    },
                },
            },
        );
        scripts.actors.insert("PROJECTILE".into(), actor);
        for placement in &mut scripts.instances {
            placement.object_type = PlacementType::MODEL;
            placement.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity()?;
        let (mut world, data, _) =
            static_model_fixture_session(scripts.clone(), None, [10, 20, 30], false);
        let creator = world.id(0)?;
        let first = world.create_object(creator, "PROJECTILE", [0; 3], [0, 111, 0])?;
        let reader = world.create_object(creator, "PROJECTILE", [0; 3], [0, 222, 0])?;
        let inputs = ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            ..Default::default()
        };
        world.advance(
            first,
            ScriptStage::Body(Thread::Interrupt),
            inputs,
            Some(&mut |_, _| Ok(())),
            None,
            20,
        )?;
        assert_eq!(
            world.actors()[first.index].runtime().unwrap().variables(),
            &[111, 0]
        );
        // The killed record remains allocated until the created-object pass retires it.
        world.advance(
            reader,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )?;
        assert_eq!(
            world.actors()[reader.index].runtime().unwrap().variables(),
            &[
                cyrus_soup::ScriptValue::Integer(111),
                cyrus_soup::ScriptValue::Actor(cyrus_soup::ActorReference {
                    generation: first.generation,
                    index: first.index
                })
            ]
        );
        assert!(world.actors()[first.index].control_state().unwrap().killed);
        assert_eq!(world.spatial(first)?.query_pose()?.angles[1], 111);
        world.set_actor_surface(first, Surface::WOOD)?;
        assert_eq!(world.actor_surface(first)?, Surface::WOOD);
        world.update_created(
            inputs,
            &mut cyrus_game::random::EngineRandom::from_state(42),
            &mut |_, _| Ok(()),
            20,
        )?;
        assert!(world.actors()[first.index].runtime().is_none());
        world.update_created(
            inputs,
            &mut cyrus_game::random::EngineRandom::from_state(42),
            &mut |_, _| Ok(()),
            20,
        )?;
        let fresh = world.create_object(creator, "PROJECTILE", [0; 3], [0, 333, 0])?;
        if restore {
            let collision = Default::default();
            let sources = WorldSnapshotSources {
                identity_memo: None,
                effects: None,
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &collision,
            };
            let saved = world.snapshot(&sources)?;
            let mut invalid = serde_json::to_value(&saved)?;
            invalid["actors"][first.index]["control"]["killed"] = false.into();
            let before = world.inspect(0, 64);
            let error = ScriptWorld::from_snapshot(
                serde_json::from_value(invalid)?,
                &sources,
                WorldRestoreContext {
                    generation: 101,
                    globals: Default::default(),
                    samples: &BTreeMap::new(),
                },
            )
            .err()
            .expect("an executable created actor cannot lose runtime before killed cleanup");
            assert!(
                format!("{error:#}")
                    .contains("created actor executable absent without killed cleanup"),
                "{error:#}"
            );
            assert_eq!(world.inspect(0, 64), before);
            let globals = std::mem::take(world.globals_mut());
            world = ScriptWorld::from_snapshot(
                saved,
                &sources,
                WorldRestoreContext {
                    generation: 101,
                    globals,
                    samples: &BTreeMap::new(),
                },
            )?;
        }
        let first = world.id(first.index)?;
        let fresh = world.id(fresh.index)?;
        assert_eq!(world.spatial(fresh)?.query_pose()?.angles[1], 333);
        let before = world.inspect(0, 64);
        for result in [
            world.spatial(first).map(|_| ()),
            world.set_actor_surface(first, Surface::SCAPE),
            world.snapshot_spatial(first),
            world.advance(
                first,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            ),
            world
                .create_object(first, "PROJECTILE", [0; 3], [0; 3])
                .map(|_| ()),
        ] {
            let error =
                result.expect_err("direct access to a retired created receiver is unsupported");
            let message = format!("{error:#}");
            assert!(
                message.contains("world 7")
                    && message.contains(&format!(
                        "retired created receiver {} PROJECTILE",
                        first.index
                    ))
                    && message.contains("unsupported"),
                "{message}"
            );
        }
        assert_eq!(world.inspect(0, 64), before);

        let error = world
            .advance(
                fresh,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            )
            .expect_err("retired target must not return its old pose or a duplicate's pose");
        let captured = world.actors()[fresh.index].runtime().unwrap().variables()[1].actor()?;
        assert_eq!(
            captured.index, first.index,
            "address itself does not dereference the retired record"
        );
        assert_eq!(captured.generation, first.generation);
        let message = format!("{error:#}");
        assert!(
            message.contains("world 7")
                && message.contains("object target")
                && message.contains(&format!(
                    "retired created receiver {} PROJECTILE",
                    first.index
                ))
                && message.contains("unsupported"),
            "{message}"
        );
        assert_eq!(
            world.actors()[fresh.index].runtime().unwrap().variables(),
            &[
                cyrus_soup::ScriptValue::Integer(0),
                cyrus_soup::ScriptValue::Actor(captured)
            ]
        );
        assert_eq!(world.flags(), &[12]);
    }
    Ok(())
}

#[test]
fn created_objects_use_identifier_zero_and_first_namespace_registration_wins() {
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().namespace = None;
    let mut created = scripts.actors["LOGIC"].clone();
    created.instances = 1;
    created.namespace = Some("projectile".into());
    created.instance_variables = vec![vec![0]];
    created.program.strings = vec!["projectile".into()];
    scripts.definition.references = vec![Property::AngleVy];
    created.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Read {
                            place: Place::Property {
                                object: Object::Named { string: 0 },
                                reference: 0,
                            },
                        },
                        rest: vec![],
                    },
                },
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
    scripts.actors.insert("PROJECTILE".into(), created);
    scripts.content_id = scripts.content_identity().unwrap();

    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let creator = world.id(0).unwrap();
    let first = world
        .create_object(creator, "PROJECTILE", [0, 0, 0], [0, 111, 0])
        .unwrap();
    let second = world
        .create_object(creator, "PROJECTILE", [0, 0, 0], [0, 222, 0])
        .unwrap();

    for id in [first, second] {
        let actor = &world.actors()[id.index];
        assert_eq!(actor.runtime_identifier, Some(0));
        assert_eq!(actor.namespace.as_deref(), Some("projectile"));
    }

    world
        .advance(
            second,
            ScriptStage::Body(Thread::Main),
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
    assert_eq!(
        world.actors()[second.index].runtime().unwrap().variables(),
        [111]
    );
}

#[test]
fn map_animation_initialization_retains_independent_frames_and_rejects_reinitialization() {
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},
                        {"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]}]}]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    for object_type in [0, 2, 3, 4, 5, 6] {
        let mut unsupported = scripts.clone();
        unsupported.instances[0].object_type = PlacementType::from_byte(object_type);
        unsupported.content_id = unsupported.content_identity().unwrap();
        data.metadata["script_content_id"] = unsupported.content_id.clone().into();
        let mut world = ScriptWorld::prepare(&unsupported, 1).unwrap();
        let id = world.id(0).unwrap();
        let error = world
            .initialize_map_animation(id, &data, 1, &mut |_, _| {
                panic!("unsupported map type dispatched an animation sound")
            })
            .unwrap_err();
        assert!(error.to_string().contains("requires map object type 1"));
        assert_eq!(world.animation_frame(id).unwrap(), None);
    }
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    for path in ["none", "static", ""] {
        data.metadata["actors"][0]["model_load_path"] = path.into();
        assert!(
            world
                .initialize_map_animation(first, &data, 1, &mut |_, _| Ok(()))
                .is_err()
        );
        assert_eq!(world.animation_frame(first).unwrap(), None);
    }
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    for id in [first, second] {
        world
            .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    }
    world
        .advance(
            first,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            0,
        )
        .unwrap();
    assert_eq!(world.animation_frame(first).unwrap(), Some(11));
    assert_eq!(world.animation_frame(second).unwrap(), Some(10));
    assert!(
        world
            .initialize_map_animation(first, &data, 1, &mut |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(world.animation_frame(first).unwrap(), Some(11));
}

#[test]
fn animation_initialization_does_not_repeat_a_failed_sound_effect() {
    use cyrus_assets::animation::Animations;
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":4,"sound_param":-17,"volume_shift":-1},
                        {"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut calls = 0;
    for _ in 0..2 {
        let error = world
            .initialize_map_animation(id, &data, 1, &mut |receiver, _| {
                assert_eq!(receiver, id);
                calls += 1;
                anyhow::bail!("sound unavailable")
            })
            .unwrap_err();
        assert!(error.to_string().contains("sound unavailable"));
    }
    assert_eq!(calls, 1);
    assert!(world.actors()[0].failure().is_some());
    assert!(
        world.actors()[0]
            .animation_failure()
            .unwrap()
            .contains("sound unavailable")
    );
    assert!(!world.actors()[0].animation_initialized());
    assert!(world.actors()[0].runtime().unwrap().failure().is_none());
    assert_eq!(world.animation_frame(id).unwrap(), None);
}

#[test]
fn invisible_instances_have_separate_authored_locals_and_shared_globals() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors().len(), 2);
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    world
        .invoke(first, Thread::Main, &mut NoServices, 20)
        .unwrap();
    assert_eq!(world.flags(), [5]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [9]);
    world
        .invoke(second, Thread::Main, &mut NoServices, 20)
        .unwrap();
    assert_eq!(world.flags(), [10]);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
}

#[test]
fn world_replacement_rejects_old_actor_ids_before_mutating_globals() {
    let scripts = cache();
    let old = ScriptWorld::prepare(&scripts, 1).unwrap().id(0).unwrap();
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    let error = replacement
        .invoke(old, Thread::Main, &mut NoServices, 20)
        .unwrap_err();
    assert!(error.to_string().contains("stale script actor generation"));
    assert_eq!(replacement.flags(), [12]);
}

#[test]
fn inspection_steps_logical_state_and_rejects_stale_generations() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 3).unwrap();
    let id = world.id(1).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    let report = world.inspect(1, 1);
    assert_eq!(report["actors"][0]["locals"][0], 10);
    assert_eq!(report["actors"][0]["id"]["generation"], 3);
    assert_eq!(world.flags(), [10]);
    let old = cyrus_script::script::ScriptActorId {
        generation: 2,
        index: 1,
    };
    assert!(
        world
            .step(old, Thread::Main, 20)
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
    assert!(
        world
            .step_tasks(old, Some(256))
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
}

pub(super) fn revisit_test_app(
    scripts: &Scripts,
) -> (
    bevy::prelude::App,
    cyrus_world::lifecycle::PreparedWorld,
    cyrus_assets::animation::Animations,
) {
    use bevy::{asset::AssetApp, prelude::*};
    use cyrus_assets::animation::Animations;
    use cyrus_script::script::ScriptPlugin;
    use cyrus_world::lifecycle::PreparedWorld;
    let make_animations = || Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"actors":[], "markers":[], "walk_node_maps":[], "script_content_id":scripts.content_id}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let animations = make_animations();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .insert_resource(State::new(cyrus_game::AppState::Menu))
        .add_plugins(ScriptPlugin);
    let script_handle = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(serde_json::from_value::<Scripts>(serde_json::to_value(scripts).unwrap()).unwrap());
    let animation_handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(make_animations());
    let prepared = PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(scripts.world),
        scripts: script_handle.clone(),
        animations: animation_handle.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    };
    (app, prepared, animations)
}

#[test]
fn retained_world_rebinds_actors_after_service_cleanup_without_restarting_script_state() {
    use cyrus_script::script::{ScriptActors, SoundResetStage, WorldSnapshotSources};
    use cyrus_world::lifecycle::PreparedWorld;
    let scripts = cache();
    let (mut app, prepared, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    app.insert_resource(prepared.clone());
    app.update();
    let old = {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
        id
    };
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    app.insert_resource(prepared.clone());
    app.update();
    assert!(app.world().resource::<ScriptActors>().has_pending_revisit());
    // Cancel the prepared entry before service restoration, then enter it again.
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    app.insert_resource(prepared.clone());
    app.update();
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        assert!(actors.has_pending_revisit());
        assert_eq!(
            actors.current.as_ref().unwrap().actors()[0]
                .runtime()
                .unwrap()
                .variables(),
            [4]
        );
        let mut wrong_scripts = cache();
        wrong_scripts.world += 1;
        wrong_scripts.content_id = wrong_scripts.content_identity().unwrap();
        assert!(
            actors
                .restore_visited(
                    &WorldSnapshotSources {
                        identity_memo: None,
                        effects: None,
                        scripts: &wrong_scripts,
                        player: None,
                        animations: &animations,
                        collision: &collision,
                    },
                    &mut |_, _| panic!("invalid source reached cleanup")
                )
                .is_err()
        );
        assert!(actors.has_pending_revisit());
        let mut cleanup = vec![];
        assert!(
            actors
                .restore_visited(&sources, &mut |stage, globals| {
                    cleanup.push(stage);
                    if stage == SoundResetStage::ClearDialogue {
                        globals.dialogue_mut().unwrap().reset_active_lines();
                    }
                    Ok(())
                })
                .unwrap()
        );
        assert_eq!(
            cleanup,
            [
                SoundResetStage::ReleaseRtx,
                SoundResetStage::ClearDialogue,
                SoundResetStage::StopSamples
            ]
        );
        assert!(!actors.has_pending_revisit());
        let world = actors.current.as_mut().unwrap();
        assert!(world.step(old, Thread::Main, 20).is_err());
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
        let fresh = world.id(0).unwrap();
        world.step(fresh, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [6]);
        assert!(
            !actors
                .restore_visited(&sources, &mut |_, _| panic!("cleanup repeated"))
                .unwrap()
        );
    }
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    app.insert_resource(prepared.clone());
    app.update();
    let mut actors = app.world_mut().resource_mut::<ScriptActors>();
    let before = actors.current.as_ref().unwrap().id(0).unwrap();
    let failure = actors
        .restore_visited(&sources, &mut |stage, _| {
            if stage == SoundResetStage::ClearDialogue {
                anyhow::bail!("dialogue cleanup failed");
            }
            Ok(())
        })
        .unwrap_err()
        .to_string();
    assert!(failure.contains("dialogue cleanup failed"));
    assert!(actors.has_pending_revisit());
    assert_eq!(actors.current.as_ref().unwrap().id(0).unwrap(), before);
    assert_eq!(
        actors.current.as_ref().unwrap().actors()[0]
            .runtime()
            .unwrap()
            .variables(),
        [4]
    );
    assert!(
        actors
            .restore_visited(&sources, &mut |_, _| panic!("failed cleanup retried"))
            .is_err()
    );
}

#[test]
fn retained_world_wait_task_continues_its_saved_invocation_counter() {
    use cyrus_script::script::{ScriptActors, WorldSnapshotSources};
    use cyrus_world::lifecycle::PreparedWorld;
    let scripts = wait_cache(vec![
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer { value: 4 }],
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let (mut app, prepared, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    app.insert_resource(prepared.clone());
    app.update();
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        world.step_tasks(id, Some(512)).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(world.flags(), [12]);
    }
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    app.insert_resource(prepared);
    app.update();
    let mut actors = app.world_mut().resource_mut::<ScriptActors>();
    actors
        .restore_visited(&sources, &mut |_, _| Ok(()))
        .unwrap();
    let world = actors.current.as_mut().unwrap();
    let id = world.id(0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(id, Some(0)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    world.step_tasks(id, Some(256)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn bevy_world_removal_and_same_handle_reload_expire_actor_ids() {
    use bevy::{asset::AssetApp, prelude::*};
    use cyrus_assets::animation::Animations;
    use cyrus_game::AppState;
    use cyrus_script::clock::SimulationClock;
    use cyrus_script::script::{ScriptActors, ScriptPlugin, ScriptStatus};
    use cyrus_world::lifecycle::PreparedWorld;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .insert_resource(State::new(AppState::InGame))
        .add_plugins(ScriptPlugin);
    let data = cache();
    let content_id = data.content_id.clone();
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(Animations {
            version: 2,
            world: 7,
            metadata: serde_json::json!({"actors": [], "markers": [], "walk_node_maps": [], "script_content_id": content_id}),
            sections: BTreeMap::new(),
            models: BTreeMap::new(),
            unsupported: BTreeMap::new(),
        });
    let scripts = app.world_mut().resource_mut::<Assets<Scripts>>().add(data);
    let prepared = || PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(7),
        scripts: scripts.clone(),
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    };
    let animation_data = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .remove(animations.id())
        .unwrap();
    app.insert_resource(prepared());
    app.update();
    assert!(matches!(
        app.world().resource::<ScriptStatus>(),
        ScriptStatus::Loading
    ));
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .insert(animations.id(), animation_data)
        .unwrap();
    app.update();
    let old = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .id(0)
        .unwrap();
    {
        let mut clock = app.world_mut().resource_mut::<SimulationClock>();
        for _ in 0..12 {
            clock.advance(0);
        }
        assert_eq!(clock.advance(18).frame_scale, 236);
    }
    app.insert_resource(State::new(AppState::Menu));
    app.update();
    app.insert_resource(State::new(AppState::InGame));
    app.update();
    assert_eq!(
        app.world_mut()
            .resource_mut::<SimulationClock>()
            .advance(18)
            .frame_scale,
        236
    );
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    assert!(app.world().resource::<ScriptActors>().current.is_none());
    assert!(matches!(
        app.world().resource::<ScriptStatus>(),
        ScriptStatus::NotRequested
    ));
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .begin_new_game();
    app.insert_resource(prepared());
    app.update();
    {
        let mut clock = app.world_mut().resource_mut::<SimulationClock>();
        for _ in 0..12 {
            assert_eq!(clock.advance(18).frame_scale, 256);
        }
        assert_eq!(clock.advance(18).frame_scale, 236);
    }
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        assert!(
            world
                .step(old, Thread::Main, 20)
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
        let fresh = world.id(0).unwrap();
        world.step(fresh, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [5]);
    }
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&animations)
        .unwrap()
        .metadata["script_content_id"] = serde_json::json!("old-export");
    app.insert_resource(prepared());
    app.update();
    assert!(app.world().resource::<ScriptActors>().current.is_none());
    assert!(
        serde_json::to_string(app.world().resource::<ScriptStatus>())
            .unwrap()
            .contains("generation mismatch")
    );
}

#[test]
fn kill_me_marks_the_receiver_dead_hides_its_model_and_stops_its_sounds() {
    use cyrus_script::script::{ScriptInputs, ScriptSound, ScriptStage};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut sounds = Vec::new();
    let mut record = |actor, sound| {
        sounds.push((actor, sound));
        Ok(())
    };
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut record),
            None,
            100,
        )
        .unwrap();
    let control = world.actors()[0].control_state().unwrap();
    assert!(control.killed);
    assert_eq!(control.ai_type, AiType::DEAD);
    assert!(!world.actors()[1].control_state().unwrap().killed);
    assert!(matches!(sounds.as_slice(), [(actor, ScriptSound::StopOwned)] if *actor == id));
}

#[test]
fn kill_me_releases_the_receivers_light_and_flat_records() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions.push(Function {
        name: Native::Light,
        param_count: 2,
        kind: CallKind::Function,
    });
    let light_id = scripts.definition.functions.len() as u16 - 1;
    scripts.definition.functions.push(Function {
        name: Native::FlatOffset,
        param_count: 3,
        kind: CallKind::Function,
    });
    let flat_id = scripts.definition.functions.len() as u16 - 1;
    let steps = &mut scripts.actors.get_mut("LOGIC").unwrap().program.steps;
    let kill = steps.remove(&0).unwrap();
    let end = steps.remove(&1).unwrap();
    steps.insert(0, assign_call(1, light_id, Native::Light, &[50, 200]));
    steps.insert(1, assign_call(2, flat_id, Native::FlatOffset, &[1, 2, 3]));
    steps.insert(2, Step { next: 3, ..kill });
    steps.insert(3, Step { next: 4, ..end });
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut record = |_, _| Ok(());
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut record),
            None,
            100,
        )
        .unwrap();
    assert!(world.actors()[0].control_state().unwrap().killed);
    assert!(world.actors()[0].light_state().is_none());
    assert!(world.actors()[0].flat_state().is_none());
}

#[test]
fn fresh_preparation_after_kill_me_leaks_no_killed_state_records_or_tasks() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts(Native::KillMe, vec![], Object::Me);
    scripts.definition.functions.push(Function {
        name: Native::Light,
        param_count: 2,
        kind: CallKind::Function,
    });
    let light_id = scripts.definition.functions.len() as u16 - 1;
    let steps = &mut scripts.actors.get_mut("LOGIC").unwrap().program.steps;
    let kill = steps.remove(&0).unwrap();
    let end = steps.remove(&1).unwrap();
    steps.insert(0, assign_call(1, light_id, Native::Light, &[50, 200]));
    steps.insert(1, Step { next: 2, ..kill });
    steps.insert(2, Step { next: 3, ..end });
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let old = world.id(0).unwrap();
    let mut sounds = Vec::new();
    let mut record = |actor, sound| {
        sounds.push((actor, sound));
        Ok(())
    };
    world
        .advance(
            old,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut record),
            None,
            100,
        )
        .unwrap();
    assert!(world.actors()[0].control_state().unwrap().killed);
    assert!(world.actors()[0].light_state().is_none());

    // Revisit: a new preparation of the same exported scripts starts from authored state.
    // Stale-identity rejection belongs to the Bevy actor resource and has its own test.
    let mut revisit = ScriptWorld::prepare(&scripts, 1).unwrap();
    let fresh = revisit.id(0).unwrap();
    let control = revisit.actors()[0].control_state().unwrap();
    assert!(!control.killed);
    assert_ne!(control.ai_type, AiType::DEAD);
    assert!(revisit.actors()[0].light_state().is_none());
    assert!(revisit.actors()[0].flat_state().is_none());
    assert_eq!(revisit.actors()[0].runtime().unwrap().active_tasks(), 0);
    // The fresh actor runs its own script from the start and is killed only by that run.
    let mut fresh_sounds = Vec::new();
    let mut fresh_record = |actor, sound| {
        fresh_sounds.push((actor, sound));
        Ok(())
    };
    revisit
        .advance(
            fresh,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut fresh_record),
            None,
            100,
        )
        .unwrap();
    assert!(revisit.actors()[0].control_state().unwrap().killed);
    assert!(matches!(fresh_sounds.as_slice(), [(actor, _)] if *actor == fresh));
}

#[test]
fn lifecycle_byte_queries_read_the_fresh_receiver_state() {
    for name in [
        Native::MyAiType,
        Native::MyAiMode,
        Native::IsOnEdge,
        Native::IsInAir,
        Native::IsCombatCapable,
    ] {
        assert_eq!(run_native_calls(&[(name, &[])]).unwrap(), 0, "{name}");
        let error = run_native_calls(&[(name, &[1])]).unwrap_err();
        assert!(format!("{error:#}").contains("no arguments"), "{name}");
    }
}

#[test]
fn kill_my_tasks_clears_every_task_slot_at_once_and_later_tasks_survive() {
    let build = |kill: bool| {
        let mut ops = vec![wait(
            4,
            Native::Wait,
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        )];
        if kill {
            ops.push(wait(5, Native::KillMyTasks, CallKind::Function, vec![]));
        }
        ops.extend([
            wait(
                4,
                Native::Wait,
                CallKind::Multitask,
                vec![Value::Integer { value: 100 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut scripts = wait_cache(ops);
        scripts.definition.functions.extend([
            Function {
                name: Native::Wait,
                param_count: 1,
                kind: CallKind::Multitask,
            },
            Function {
                name: Native::KillMyTasks,
                param_count: 0,
                kind: CallKind::Function,
            },
        ]);
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        (
            world.flags()[0],
            world.actors()[0].runtime().unwrap().active_tasks(),
        )
    };
    // Without the call both multitasks remain; with it only the one started afterwards does.
    assert_eq!(build(false), (7, 2));
    assert_eq!(build(true), (7, 1));
}

#[test]
fn kill_me_clears_receiver_tasks_before_marking_the_actor_dead() {
    use cyrus_script::script::{ScriptInputs, ScriptSound, ScriptStage};
    let mut scripts = wait_cache(vec![
        wait(
            4,
            Native::Wait,
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(5, Native::KillMe, CallKind::Function, vec![]),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.extend([
        Function {
            name: Native::Wait,
            param_count: 1,
            kind: CallKind::Multitask,
        },
        Function {
            name: Native::KillMe,
            param_count: 0,
            kind: CallKind::Function,
        },
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut sounds = Vec::new();
    let mut record = |actor, sound| {
        sounds.push((actor, sound));
        Ok(())
    };
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut record),
            None,
            20,
        )
        .unwrap();
    assert!(world.actors()[0].control_state().unwrap().killed);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert!(matches!(sounds.as_slice(), [(actor, ScriptSound::StopOwned)] if *actor == id));
}

#[test]
fn a_created_object_takes_its_creators_frame_runs_each_update_and_survives_a_snapshot() {
    use cyrus_script::script::{
        ScriptActorSource, ScriptInputs, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = cache();
    let mut projectile = scripts.actors["LOGIC"].clone();
    projectile.code_origin += 1000;
    projectile.header_index += 100;
    projectile.attributes[Attribute::ForwardSpeed] = 50;
    scripts.actors.insert("BALL".into(), projectile);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, _) = static_model_fixture(scripts.clone());
    let census = world.actors().len();
    let creator = world.id(0).unwrap();
    let created = world
        .create_object(creator, "ball", [0, 0, 256], [0, 512, 0])
        .unwrap();
    assert_eq!(world.actors().len(), census + 1);
    assert!(matches!(
        &world.actors()[created.index].source,
        ScriptActorSource::Created(name) if name == "BALL"
    ));
    assert_eq!(
        world.actors()[created.index].attributes()[Attribute::Active],
        0
    );
    assert!(
        world
            .create_object(creator, "absent", [0; 3], [0; 3])
            .is_err()
    );
    let at = world.spatial(created).unwrap().local.position;
    let origin = world.spatial(creator).unwrap().local.position;
    assert_ne!(at, origin, "the offset moved it off its creator");
    world
        .update_created(
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(world.actors()[created.index].failure().is_none());
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
    let restored = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 9,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(restored.actors().len(), census + 1);
    let id = restored.id(created.index).unwrap();
    assert_eq!(restored.spatial(id).unwrap().local.position, at);
}

#[test]
fn killed_created_lineup_receiver_never_resumes_its_waiting_main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use cyrus_script::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, PlayerCollisionSource,
        ScriptInputs, WorldRestoreContext, WorldSnapshotSources,
    };
    for restore_before_cleanup in [false, true] {
        let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
            scripts.lineup_distance = Some(i32::MAX);
            scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::ForwardWalk] = 3;
            let lineup_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: Native::PlayerLineUp,
                param_count: 3,
                kind: CallKind::Task,
            });
            let kill_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: Native::KillMe,
                param_count: 0,
                kind: CallKind::Function,
            });
            let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
            program.entry = 3;
            program.steps = BTreeMap::from([
                (
                    0,
                    Step {
                        next: 1,
                        operation: wait(kill_id, Native::KillMe, CallKind::Function, vec![]),
                    },
                ),
                (
                    1,
                    Step {
                        next: 2,
                        operation: Operation::Endint,
                    },
                ),
                (
                    3,
                    Step {
                        next: 4,
                        operation: wait(
                            lineup_id,
                            Native::PlayerLineUp,
                            CallKind::Task,
                            vec![
                                Value::Integer { value: 1024 },
                                Value::Integer { value: 0 },
                                Value::Integer { value: -2048 },
                            ],
                        ),
                    },
                ),
                (
                    4,
                    Step {
                        next: 5,
                        operation: set_flag(7),
                    },
                ),
                (
                    5,
                    Step {
                        next: 6,
                        operation: Operation::End { target: 3 },
                    },
                ),
            ]);
        });
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        let player = world.selected_player().context("selected player")?;
        let creator = world.id(0)?;
        let receiver = world.create_object(creator, "LOGIC", [0; 3], [0; 3])?;
        let actors = [player, creator, receiver];
        let hud = cyrus_script::script::ScriptHud::default();
        let prepared =
            world.prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &actors)?;
        let inputs = ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            shell_execution: Some(false),
            hud: Some(&hud),
            player_collision: Some(PlayerCollisionSource {
                scene: &scene,
                animations: &data,
                view: [10, 30],
                actors: &actors,
            }),
            locomotion_geometry: Some(LocomotionGeometry {
                ground: OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                environment: ObstacleEnvironment {
                    selected_player: Some(player),
                    auto_grab: false,
                    item_system_ready: false,
                    shell_execution: false,
                    ticks: 1,
                },
            }),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        world.begin_player_movement_update(player, 0)?;
        world.advance(
            receiver,
            ScriptStage::Body(Thread::Main),
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )?;
        ensure!(
            world.actors()[receiver.index]
                .runtime()
                .context("receiver runtime")?
                .active_tasks()
                == 1,
            "alignment must be active before kill"
        );
        let before = world.inspect(receiver.index, 1);
        let flags = world.flags().to_vec();
        let position = world.spatial(player)?.local.position;
        let result = before["globals"]["lineup_result"].clone();
        let collision = Default::default();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let active = serde_json::to_value(world.snapshot(&sources)?)?;
        let task = &active["actors"][receiver.index]["tasks"]["tasks"][0]["state"]["LineUp"];
        ensure!(
            task["initialized"] == true && task["reference"] == receiver.index,
            "created owner must retain its own initialized reference"
        );
        world.advance(
            receiver,
            ScriptStage::Body(Thread::Interrupt),
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            20,
        )?;
        ensure!(
            world.actors()[receiver.index]
                .control_state()
                .context("control")?
                .killed,
            "independent interrupt did not kill receiver"
        );
        ensure!(
            world.actors()[receiver.index]
                .runtime()
                .context("runtime before cleanup")?
                .active_tasks()
                == 0,
            "kill retained task"
        );

        if restore_before_cleanup {
            let globals = std::mem::take(world.globals_mut());
            let saved = world.snapshot(&sources)?;
            world = ScriptWorld::from_snapshot(
                saved,
                &sources,
                WorldRestoreContext {
                    generation: 101,
                    globals,
                    samples: &BTreeMap::new(),
                },
            )?;
        }
        let player = world
            .selected_player()
            .context("selected player after replacement")?;
        let receiver = world.id(receiver.index)?;
        for _ in 0..3 {
            world.update_created(
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| panic!("killed receiver must not emit sounds"),
                100,
            )?;
            ensure!(
                world.actors()[receiver.index].runtime().is_none(),
                "cleanup retained executable runtime"
            );
            ensure!(
                world.actors()[receiver.index].failure().is_none(),
                "killed receiver attempted a failing continuation"
            );
            ensure!(
                world.spatial(player)?.local.position == position,
                "killed receiver moved player"
            );
            ensure!(
                world.inspect(receiver.index, 1)["globals"]["lineup_result"] == result,
                "killed receiver published lineup completion"
            );
            ensure!(
                world.flags() == flags,
                "waiting main continuation executed after kill"
            );
        }
    }
    Ok(())
}

#[test]
fn created_object_killed_by_its_hook_does_not_run_main() {
    use cyrus_script::script::ScriptInputs;
    let mut scripts = cache();
    let kill_id = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: Native::KillMe,
        param_count: 0,
        kind: CallKind::Function,
    });
    let template = scripts.actors.get_mut("LOGIC").unwrap();
    template.native_hook = Some(Call {
        function_id: kill_id,
        function: Native::KillMe,
        target: Object::Me,
        kind: CallKind::Function,
        arguments: vec![],
    });
    template.program.steps.get_mut(&0).unwrap().operation = set_flag(7);
    let (mut world, _, _) = static_model_fixture(scripts);
    let creator = world.id(0).unwrap();
    let receiver = world
        .create_object(creator, "LOGIC", [0; 3], [0; 3])
        .unwrap();
    let flags = world.flags().to_vec();
    world
        .update_created(
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            20,
        )
        .unwrap();
    assert!(
        world.actors()[receiver.index]
            .control_state()
            .unwrap()
            .killed
    );
    assert!(world.actors()[receiver.index].runtime().is_none());
    assert!(world.actors()[receiver.index].failure().is_none());
    assert_eq!(
        world.flags(),
        flags,
        "main ran after the hook killed its receiver"
    );
}

#[test]
fn a_created_object_snapshot_rejects_an_out_of_range_creator_reference() {
    use cyrus_script::script::{ScriptActorSource, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    let mut projectile = scripts.actors["LOGIC"].clone();
    projectile.code_origin += 1000;
    projectile.header_index += 100;
    scripts.actors.insert("BALL".into(), projectile);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, data, _) = static_model_fixture(scripts.clone());
    let creator = world.id(0).unwrap();
    let created = world
        .create_object(creator, "ball", [0, 0, 256], [0, 0, 0])
        .unwrap();
    assert!(matches!(
        &world.actors()[created.index].source,
        ScriptActorSource::Created(name) if name == "BALL"
    ));
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
    saved["actors"][created.index]["control"]["creator"] = serde_json::json!(9999);
    let result = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 9,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    );
    let error = match result {
        Ok(_) => panic!("restore accepted an out-of-range creator"),
        Err(error) => error,
    };
    assert!(
        format!("{error:#}").contains("saved creator actor 9999 is stale or absent"),
        "{error:#}"
    );
}

#[test]
fn retained_world_places_player_roles_at_the_new_destination() {
    use cyrus_script::script::{ScriptActorSource, ScriptActors, WorldSnapshotSources};
    use cyrus_world::lifecycle::PreparedWorld;
    let mut scripts = cache();
    add_standalone_receivers(&mut scripts);
    let (mut app, prepared, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    app.insert_resource(prepared.clone());
    app.update();
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        world.prepare_spatial().unwrap();
        let player = world.selected_player().unwrap();
        world
            .initialize_player_spatial(player, [100, 200, 300], [0, 128, 0])
            .unwrap();
        let logic = world.id(0).unwrap();
        world
            .advance(
                logic,
                ScriptStage::Body(Thread::Main),
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
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
    }
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    app.insert_resource(prepared);
    app.update();
    let mut actors = app.world_mut().resource_mut::<ScriptActors>();
    let world = actors.current.as_mut().unwrap();
    world.prepare_spatial().unwrap();
    let player = world.selected_player().unwrap();
    world
        .initialize_player_spatial(player, [400, 500, 600], [0, 768, 0])
        .unwrap();
    assert!(
        actors
            .restore_visited(&sources, &mut |_, _| Ok(()))
            .unwrap()
    );
    let world = actors.current.as_mut().unwrap();
    for (index, actor) in world.actors().iter().enumerate() {
        if matches!(
            actor.source,
            ScriptActorSource::Player | ScriptActorSource::AlternatePlayer
        ) {
            let spatial = world.spatial(world.id(index).unwrap()).unwrap();
            assert_eq!(spatial.local.position, [400, 500, 600]);
            assert_eq!(spatial.local.angles, [0, 768, 0]);
        }
    }
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [5]);
    let logic = world.id(0).unwrap();
    world
        .advance(
            logic,
            ScriptStage::Body(Thread::Main),
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
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [6]);
}

#[test]
fn lifecycle_observations_distinguish_same_name_actors_and_ignore_raw_health() {
    use cyrus_script::script::ActorPresence;
    let mut world = ScriptWorld::prepare(&cache(), 901).unwrap();
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    assert_eq!(world.actors()[0].name(), world.actors()[1].name());
    world.set_attribute_for_test(first, Attribute::Health, 0);
    assert!(!world.actor_snapshot(first).unwrap().dead);
    assert!(world.take_death_events().is_empty());
    world.mark_actor_dead(0).unwrap();
    world.mark_actor_dead(1).unwrap();
    world.mark_actor_dead(0).unwrap();
    let events = world.take_death_events();
    assert_eq!(
        events.iter().map(|e| e.snapshot.actor).collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(
        world.actor_snapshot(first).unwrap().presence,
        ActorPresence::Visible
    );
    assert!(world.actor_snapshot(first).unwrap().dead);
    assert!(world.take_death_events().is_empty());
    assert!(
        world
            .actor_snapshot(cyrus_script::script::ScriptActorId {
                generation: 900,
                index: 0
            })
            .is_err()
    );
}

#[test]
fn restored_dead_snapshot_does_not_replay_a_death() {
    let (mut world, scripts, data) = movement_fixture(18, false, true, |_| {}, |_| {});
    world.mark_actor_dead(0).unwrap();
    assert_eq!(world.take_death_events().len(), 1);
    let saved = movement_snapshot(&world, &scripts, &data);
    let mut restored = restore_navigation_owner(saved, &scripts, &data).unwrap();
    assert!(
        restored
            .actor_snapshot(restored.id(0).unwrap())
            .unwrap()
            .dead
    );
    assert!(restored.take_death_events().is_empty());
    restored.mark_actor_dead(0).unwrap();
    assert!(restored.take_death_events().is_empty());
}
