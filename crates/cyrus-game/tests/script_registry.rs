//! Logical actors execute independently of geometry through explicit public stages.
use cyrus_assets::script::*;
use cyrus_game::script::ScriptWorld;
use cyrus_script::program::*;
use cyrus_script::{Argument, CallResult, Host, Target, Thread};
use std::collections::BTreeMap;

struct NoServices;
impl Host for NoServices {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: &str,
        _: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        anyhow::bail!("no native services")
    }
    fn poll(
        &mut self,
        _: u64,
        _: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        anyhow::bail!("no tasks")
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}

fn cache() -> Scripts {
    let program = Program {
        version: 3,
        entry: 0,
        variables: vec![4],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Increment {
                        destination: Place::Local { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Assign {
                        destination: Place::Flag { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Local { index: 0 },
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
                    operation: Operation::End { target: 0 },
                },
            ),
        ]),
    };
    let mut scripts = Scripts {
        version: 17,
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        non_engaged_spacing: None,
        dialogue: cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        },
        player_distance_override: false,
        updates: cyrus_assets::script::UpdateSettings {
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 32,
        content_id: String::new(),
        world: 7,
        definition: Definition {
            functions: vec![Function {
                name: "NullFunction".into(),
                param_count: 0,
                kind: CallKind::Function,
            }],
            flags: vec![Flag {
                name: "Counter".into(),
                flag_type: "NUMBER".into(),
                initial_value: 12,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([(
            "LOGIC".into(),
            Actor {
                header_index: 0,
                code_origin: 0,
                instances: 2,
                namespace: Some("logic".into()),
                instance_variables: vec![vec![4], vec![9]],
                location_offsets: vec![],
                program,
                attributes: BTreeMap::new(),
                combat_taunts: vec![],
                native_hook: None,
            },
        )]),
        instances: (0..2)
            .map(|id| Instance {
                object_id: id,
                actor: "LOGIC".into(),
                object_type: 0,
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
#[test]
fn map_animation_initialization_retains_independent_frames_and_rejects_reinitialization() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
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
        unsupported.instances[0].object_type = object_type;
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
        instance.object_type = 1;
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
    let old = cyrus_game::script::ScriptActorId {
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

#[test]
fn researched_attribute_query_reads_unsigned_bytes_and_checks_bounds() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "GetMyAttr".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    scripts.definition.attributes = vec!["first".into()];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes.insert("first".into(), 255);
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: "GetMyAttr".into(),
                    arguments: vec![Value::Integer { value: 0 }],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [255]);
    for index in [-1, 256] {
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        let Operation::Assign { formula, .. } =
            &mut actor.program.steps.get_mut(&0).unwrap().operation
        else {
            unreachable!()
        };
        let Value::Call { call } = &mut formula.first else {
            unreachable!()
        };
        call.arguments = vec![Value::Integer { value: index }];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [0]);
    }
}

fn revisit_test_app(
    scripts: &Scripts,
) -> (
    bevy::prelude::App,
    cyrus_world::lifecycle::PreparedWorld,
    cyrus_assets::animation::Animations,
) {
    use bevy::{asset::AssetApp, prelude::*};
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::ScriptPlugin;
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
    use cyrus_game::script::{ScriptActors, SoundResetStage, WorldSnapshotSources};
    use cyrus_world::lifecycle::PreparedWorld;
    let scripts = cache();
    let (mut app, prepared, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
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
    use cyrus_game::script::{ScriptActors, WorldSnapshotSources};
    use cyrus_world::lifecycle::PreparedWorld;
    let scripts = wait_cache(vec![
        wait(1, "Wait", CallKind::Task, vec![Value::Integer { value: 4 }]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let (mut app, prepared, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
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
    use cyrus_game::script::{ScriptActors, ScriptPlugin, ScriptStatus};
    use cyrus_game::{AppState, clock::SimulationClock};
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
fn native_flag_helpers_keep_type_behavior_numeric_values_and_latches_separate() {
    let native = |id, name: &str| Value::Call {
        call: Box::new(Call {
            function_id: id,
            kind: CallKind::Function,
            target: Object::Me,
            function: name.into(),
            arguments: vec![Value::Integer { value: 0 }],
        }),
    };
    let assign = |index, value| Operation::Assign {
        destination: Place::Local { index },
        formula: Formula {
            first: value,
            rest: vec![],
        },
    };
    for (kind, initial, after_two, after_third, final_latch) in [
        ("BOOL", 0, 1, 0, 1),
        ("FLIPFLOP", 0, 0, 1, 1),
        ("COUNTER", 2, 0, 0, 0),
        ("NUMBER", 7, 7, 7, 1),
    ] {
        let mut scripts = cache();
        scripts.definition.flags[0].flag_type = kind.into();
        scripts.definition.flags[0].initial_value = initial;
        scripts.definition.functions.extend(
            ["SetGlobalFlag", "ResetGlobalFlag", "TestGlobalFlag"].map(|name| Function {
                name: name.into(),
                param_count: 1,
                kind: CallKind::Function,
            }),
        );
        let ops = vec![
            assign(0, native(1, "SetGlobalFlag")),
            assign(0, native(1, "SetGlobalFlag")),
            assign(1, native(3, "TestGlobalFlag")),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::Yield,
            assign(0, native(2, "ResetGlobalFlag")),
            assign(1, native(3, "TestGlobalFlag")),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::Yield,
            assign(0, native(1, "SetGlobalFlag")),
            assign(1, native(3, "TestGlobalFlag")),
            assign(
                2,
                Value::Read {
                    place: Place::Flag { index: 0 },
                },
            ),
            Operation::End { target: 0 },
        ];
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.program.variables = vec![0; 3];
        actor.instance_variables = vec![vec![0; 3]; 2];
        actor.program.steps = ops
            .into_iter()
            .enumerate()
            .map(|(offset, operation)| {
                (
                    offset as u32,
                    Step {
                        next: offset as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, 1, after_two],
            "{kind}"
        );
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, 0, after_two],
            "{kind}"
        );
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [0, final_latch, after_third],
            "{kind}"
        );
    }
}

fn wait_cache(ops: Vec<Operation>) -> Scripts {
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "Wait".into(),
            param_count: 1,
            kind: CallKind::Task,
        },
        Function {
            name: "WaitOnTasks".into(),
            param_count: 0,
            kind: CallKind::Task,
        },
        Function {
            name: "WaitNonMulti".into(),
            param_count: 0,
            kind: CallKind::Task,
        },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = ops
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
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
fn wait(id: u16, name: &str, kind: CallKind, args: Vec<Value>) -> Operation {
    Operation::Call {
        call: Call {
            function_id: id,
            function: name.into(),
            target: Object::Me,
            kind,
            arguments: args,
        },
    }
}
fn set_flag(value: i32) -> Operation {
    Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Integer { value },
            rest: vec![],
        },
    }
}
#[test]
fn wait_continues_to_flag_change_and_missing_scale_does_not_poison_actor() {
    let scripts = wait_cache(vec![
        wait(1, "Wait", CallKind::Task, vec![Value::Integer { value: 4 }]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert!(
        world
            .step_tasks(id, None)
            .unwrap_err()
            .to_string()
            .contains("requires frame_scale")
    );
    assert!(world.actors()[0].runtime().unwrap().failure().is_none());
    world.step_tasks(id, Some(512)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    world.step_tasks(id, Some(0)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    world.step_tasks(id, Some(256)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(
        world.actors()[1]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
}
#[test]
fn wait_signed_duration_wrap_and_initial_count_excludes_unallocated_slot() {
    for duration in [i32::MIN, -1, 0, 1, 2] {
        let scripts = wait_cache(vec![
            wait(
                1,
                "Wait",
                CallKind::Task,
                vec![Value::Integer { value: duration }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [12]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world.step_tasks(id, Some(256)).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags()[0], if duration == i32::MIN { 12 } else { 7 });
    }
    for (id, name) in [(2, "WaitOnTasks"), (3, "WaitNonMulti")] {
        let scripts = wait_cache(vec![
            wait(id, name, CallKind::Task, vec![]),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let actor = world.id(0).unwrap();
        world.step(actor, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [12]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world.step_tasks(actor, Some(256)).unwrap();
        world.step(actor, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [7]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}
#[test]
fn wait_on_tasks_ignores_paused_slots_and_stop_clears_native_bookkeeping() {
    let scripts = wait_cache(vec![
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(2, "WaitOnTasks", CallKind::Multitask, vec![]),
        Operation::Yield,
        Operation::TaskPause { label: 0 },
        Operation::TaskPause { label: 1 },
        Operation::Yield,
        Operation::TaskResume { label: 0 },
        Operation::TaskStop { label: 1 },
        Operation::Yield,
        Operation::TaskStop { label: 0 },
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.flags(), [7]);
    // A later authored pass allocates fresh host tokens after cancellation.
    world.step(id, Thread::Main, 20).unwrap();
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
}

#[test]
fn wait_non_multi_counts_blocking_slots_and_observes_same_pass_completion() {
    let mut scripts = wait_cache(vec![
        Operation::TaskPause { label: 6 },
        wait(
            1,
            "Wait",
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::TaskResume { label: 6 },
        wait(3, "WaitNonMulti", CallKind::Task, vec![]),
        set_flag(7),
        Operation::Endint,
        wait(
            1,
            "Wait",
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::End { target: 6 },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.entry = 6;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    world.step(id, Thread::Interrupt, 20).unwrap();
    world.step(id, Thread::Interrupt, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(256)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(12800)).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    world.step(id, Thread::Interrupt, 20).unwrap();
    assert_eq!(world.flags(), [7]);
}

#[test]
fn wait_non_multi_ignores_multiple_unpaused_multitasks() {
    let scripts = wait_cache(vec![
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(
            1,
            "Wait",
            CallKind::Multitask,
            vec![Value::Integer { value: 100 }],
        ),
        wait(3, "WaitNonMulti", CallKind::Task, vec![]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 3);
    world.step_tasks(id, Some(256)).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 2);
}

fn routed_cache(caller: Vec<Operation>, receiver: Vec<Operation>) -> Scripts {
    let mut scripts = wait_cache(caller);
    let program_steps = |ops: Vec<Operation>| {
        ops.into_iter()
            .enumerate()
            .map(|(index, operation)| {
                (
                    index as u32 * 16,
                    Step {
                        next: (index as u32 + 1) * 16,
                        operation,
                    },
                )
            })
            .collect()
    };
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let ops = actor
        .program
        .steps
        .values()
        .map(|step| step.operation.clone())
        .collect();
    actor.program.steps = program_steps(ops);
    actor.namespace = Some("caller".into());
    actor.instances = 1;
    actor.instance_variables = vec![vec![4]];
    actor.program.strings = vec!["receiver".into()];
    let mut recipient = actor.clone();
    recipient.header_index = 1;
    recipient.code_origin = 100;
    recipient.namespace = Some("receiver".into());
    recipient.program.steps = program_steps(receiver);
    recipient.program.variables = vec![9];
    recipient.instance_variables = vec![vec![9]];
    scripts.actors.insert("RECEIVER".into(), recipient);
    scripts.instances[1].actor = "RECEIVER".into();
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
fn object_wait(target: Object, kind: CallKind, duration: Value) -> Operation {
    let Operation::Call { mut call } = wait(1, "Wait", kind, vec![duration]) else {
        unreachable!()
    };
    call.target = target;
    Operation::ObjectCall { call }
}
#[test]
fn routed_task_uses_receiver_locals_and_slots_while_the_caller_continues() {
    let scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Task,
                Value::Read {
                    place: Place::Local { index: 0 },
                },
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ],
        vec![set_flag(8), Operation::End { target: 0 }],
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step(receiver, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(receiver, Some(768)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(receiver, Some(1024)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
    world.step(receiver, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [8]);
}
#[test]
fn source_operand_identity_keeps_equal_cursors_distinct_and_receiver_labels_local() {
    let scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Multitask,
                Value::Integer { value: 100 },
            ),
            Operation::TaskStop { label: 3 },
            Operation::End { target: 0 },
        ],
        vec![
            wait(
                1,
                "Wait",
                CallKind::Multitask,
                vec![Value::Integer { value: 100 }],
            ),
            Operation::TaskStop { label: 0 },
            Operation::End { target: 0 },
        ],
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step(receiver, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(receiver, Some(256)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
}
#[test]
fn explicit_me_object_task_continues_and_matches_the_embedded_dispatch_label() {
    let scripts = routed_cache(
        vec![
            object_wait(Object::Me, CallKind::Task, Value::Integer { value: 100 }),
            set_flag(7),
            Operation::TaskPause { label: 3 },
            Operation::Yield,
            Operation::TaskResume { label: 3 },
            Operation::TaskStop { label: 3 },
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert!(!world.actors()[0].runtime().unwrap().has_blocking_task());
    world.step_tasks(caller, Some(256)).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}
#[test]
fn named_functions_resolve_namespace_and_receiver_arguments_case_insensitively() {
    let call = Call {
        function_id: 4,
        target: Object::Named { string: 0 },
        kind: CallKind::Function,
        function: "GetMyAttr".into(),
        arguments: vec![Value::Read {
            place: Place::Local { index: 0 },
        }],
    };
    let mut scripts = routed_cache(
        vec![
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(call),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.definition.functions.push(Function {
        name: "GetMyAttr".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    scripts.definition.attributes = (0..10).map(|index| format!("attribute_{index}")).collect();
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "ReCeIvEr".into();
    scripts
        .actors
        .get_mut("RECEIVER")
        .unwrap()
        .attributes
        .insert("attribute_9".into(), 255);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attribute_4".into(), 11);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [255]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [9]);
}
#[test]
fn failed_receiver_calls_restore_registry_and_keep_failures_sticky() {
    let mut scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Task,
                Value::Integer { value: 100 },
            ),
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.actors.get_mut("RECEIVER").unwrap().namespace = Some("receiver".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "receiver".into();
    let Operation::ObjectCall { call } = &mut scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation
    else {
        unreachable!()
    };
    call.arguments = vec![Value::Read {
        place: Place::Local { index: 0 },
    }];
    scripts
        .actors
        .get_mut("RECEIVER")
        .unwrap()
        .program
        .variables = vec![];
    scripts
        .actors
        .get_mut("RECEIVER")
        .unwrap()
        .instance_variables = vec![vec![]];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let caller = world.id(0).unwrap();
    let receiver = world.id(1).unwrap();
    let error = world
        .step(caller, Thread::Main, 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("local variable absent"), "{error}");
    assert!(world.actors().iter().all(|actor| actor.runtime().is_some()));
    assert!(world.actors()[0].runtime().unwrap().failure().is_some());
    assert!(world.actors()[1].runtime().unwrap().failure().is_some());
    assert_eq!(
        world
            .step(caller, Thread::Main, 20)
            .unwrap_err()
            .to_string(),
        error
    );
    assert!(world.step(receiver, Thread::Main, 20).is_err());
}

fn add_standalone_receivers(scripts: &mut Scripts) {
    for (index, (name, namespace, local)) in [
        ("CYRUS", None, 9),
        ("GREMLIN", Some("gremlin"), 11),
        ("CAMERA", None, 5),
    ]
    .into_iter()
    .enumerate()
    {
        let mut actor = scripts.actors["LOGIC"].clone();
        actor.instances = 1;
        actor.code_origin = 200 + index as u32 * 100;
        actor.namespace = namespace.map(str::to_owned);
        actor.instance_variables = vec![vec![local]];
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Yield,
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
        scripts.actors.insert(name.into(), actor);
    }
    scripts.content_id = scripts.content_identity().unwrap();
}
#[test]
fn standalone_player_and_camera_receive_tasks_without_map_placements() {
    use cyrus_game::script::ScriptActorSource;
    let mut scripts = wait_cache(vec![
        object_wait(
            Object::Player,
            CallKind::Task,
            Value::Read {
                place: Place::Local { index: 0 },
            },
        ),
        object_wait(
            Object::Camera,
            CallKind::Task,
            Value::Read {
                place: Place::Local { index: 0 },
            },
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    add_standalone_receivers(&mut scripts);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.actors().len(), 5);
    assert_eq!(
        world
            .actors()
            .iter()
            .map(|actor| actor.runtime_identifier)
            .collect::<Vec<_>>(),
        [Some(36), Some(37), Some(1), Some(1), Some(2)]
    );
    assert!(matches!(
        world.actors()[2].source,
        ScriptActorSource::Player
    ));
    assert!(matches!(
        world.actors()[3].source,
        ScriptActorSource::AlternatePlayer
    ));
    assert!(matches!(
        world.actors()[4].source,
        ScriptActorSource::Camera
    ));
    assert!(
        world.actors()[2..]
            .iter()
            .all(|actor| actor.placement().is_none())
    );
    assert_eq!(
        world.inspect(2, 1)["actors"][0]["object_id"],
        serde_json::Value::Null
    );
    let caller = world.id(0).unwrap();
    let player = world.id(2).unwrap();
    let camera = world.id(4).unwrap();
    world.step(caller, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[2].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.actors()[4].runtime().unwrap().active_tasks(), 1);
    assert_eq!(
        world.actors()[2]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    world.step_tasks(player, Some(7 * 256)).unwrap();
    assert_eq!(world.actors()[2].runtime().unwrap().active_tasks(), 1);
    world.step_tasks(camera, Some(5 * 256)).unwrap();
    assert_eq!(world.actors()[4].runtime().unwrap().active_tasks(), 0);
    world.step_tasks(player, Some(5 * 256)).unwrap();
    assert_eq!(world.actors()[2].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[2].runtime().unwrap().variables(), [9]);
    assert_eq!(world.actors()[4].runtime().unwrap().variables(), [5]);
}
#[test]
fn signed_identifier_order_resolves_duplicate_names_across_the_low_word_wrap() {
    let mut scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Task,
                Value::Integer { value: 100 },
            ),
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.actor_pool_capacity = 32763;
    scripts.actors.get_mut("RECEIVER").unwrap().namespace = Some("caller".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "CaLlEr".into();
    scripts.instances[0].object_id = 0;
    scripts.instances[1].object_id = u32::MAX;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.actors()[0].runtime_identifier, Some(32767));
    assert_eq!(world.actors()[1].runtime_identifier, Some(-32768));
    world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
}
#[test]
fn normal_duplicate_names_choose_the_first_registered_runtime_identifier() {
    let mut scripts = routed_cache(
        vec![
            object_wait(
                Object::Named { string: 0 },
                CallKind::Multitask,
                Value::Integer { value: 100 },
            ),
            Operation::End { target: 0 },
        ],
        vec![Operation::End { target: 0 }],
    );
    scripts.actors.get_mut("RECEIVER").unwrap().namespace = Some("caller".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings[0] = "caller".into();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 0);
}
#[test]
fn activation_and_separate_map_paths_preserve_identifier_and_local_allocation_order() {
    let mut scripts = cache();
    scripts.actor_pool_capacity = 10;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 5;
    actor
        .instance_variables
        .extend([vec![13], vec![17], vec![21]]);
    let first = scripts.instances[0].clone();
    scripts.instances = [2, 6, 0, 1, 0]
        .into_iter()
        .enumerate()
        .map(|(index, object_type)| Instance {
            object_type,
            active: u8::from(index != 2),
            ..first.clone()
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world
            .actors()
            .iter()
            .map(|actor| actor.runtime_identifier)
            .collect::<Vec<_>>(),
        [None, None, Some(14), Some(16), Some(18)]
    );
    for actor in &world.actors()[..2] {
        assert!(actor.runtime().is_some());
        assert!(actor.unavailable().is_none());
        assert!(actor.control_state().is_none());
    }
    for (index, value) in [4, 9, 13, 17, 21].into_iter().enumerate() {
        assert_eq!(
            world.actors()[index].runtime().unwrap().variables(),
            [value]
        );
    }
}

#[test]
fn registering_an_existing_identifier_keeps_the_first_namespace_only() {
    let mut scripts = wait_cache(vec![
        object_wait(
            Object::Named { string: 0 },
            CallKind::Task,
            Value::Integer { value: 10 },
        ),
        Operation::End { target: 0 },
    ]);
    add_standalone_receivers(&mut scripts);
    scripts.actors.get_mut("CYRUS").unwrap().namespace = Some("first_player".into());
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["gremlin".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let error = world
        .step(world.id(0).unwrap(), Thread::Main, 20)
        .unwrap_err()
        .to_string();
    assert!(error.contains("receiver absent"), "{error}");
    assert!(world.actors()[3].runtime().is_some());
    assert_eq!(world.actors()[3].runtime().unwrap().active_tasks(), 0);
}

fn attribute_call(id: u16, name: &str, index: i32, value: Option<i32>) -> Call {
    let mut arguments = vec![Value::Integer { value: index }];
    if let Some(value) = value {
        arguments.push(Value::Integer { value });
    }
    Call {
        function_id: id,
        target: Object::Me,
        kind: CallKind::Function,
        function: name.into(),
        arguments,
    }
}
fn attribute_cache(operations: Vec<Operation>) -> Scripts {
    let mut scripts = wait_cache(operations);
    scripts.definition.functions.truncate(1);
    for (name, param_count) in [
        ("SetAttribute", 2),
        ("SetMyAttr", 2),
        ("GetAttribute", 1),
        ("GetMyAttr", 1),
    ] {
        scripts.definition.functions.push(Function {
            name: name.into(),
            kind: CallKind::Function,
            param_count,
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
#[test]
fn attribute_writes_truncate_and_only_set_attribute_updates_backup_bytes() {
    let scripts = attribute_cache(vec![
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(attribute_call(1, "SetAttribute", 1, Some(-2))),
                },
                rest: vec![],
            },
        },
        Operation::Yield,
        Operation::Call {
            call: attribute_call(1, "SetAttribute", 6, Some(0x123)),
        },
        Operation::Call {
            call: attribute_call(2, "SetMyAttr", 1, Some(511)),
        },
        Operation::Call {
            call: attribute_call(2, "SetMyAttr", 6, Some(-1)),
        },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(attribute_call(3, "GetAttribute", 115, None)),
                },
                rest: vec![(
                    1,
                    Value::Call {
                        call: Box::new(attribute_call(4, "GetMyAttr", 119, None)),
                    },
                )],
            },
        },
        Operation::End { target: 0 },
    ]);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    world.step(id, Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [289]);
    let mut expected = [0; 256];
    expected[1] = 255;
    expected[6] = 255;
    expected[115] = 35;
    expected[119] = 254;
    assert_eq!(world.actors()[0].attributes(), &expected);
    assert_eq!(world.actors()[1].attributes(), &[0; 256]);
}
#[test]
fn set_my_attribute_ignores_out_of_range_indices_without_mutation() {
    for index in [i32::MIN, -1, 256, i32::MAX] {
        let scripts = attribute_cache(vec![
            Operation::Assign {
                destination: Place::Local { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(attribute_call(2, "SetMyAttr", index, Some(5))),
                    },
                    rest: vec![],
                },
            },
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
        assert_eq!(world.actors()[0].attributes(), &[0; 256]);
    }
}
#[test]
fn routed_attribute_writes_use_receiver_locals_and_are_visible_in_the_same_stage() {
    let mut write = attribute_call(1, "SetAttribute", 1, Some(0));
    write.target = Object::Named { string: 0 };
    write.arguments[1] = Value::Read {
        place: Place::Local { index: 0 },
    };
    let mut read = attribute_call(3, "GetAttribute", 119, None);
    read.target = Object::Named { string: 0 };
    let mut scripts = attribute_cache(vec![
        Operation::ObjectCall { call: write },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(read),
                },
                rest: vec![],
            },
        },
        Operation::End { target: 0 },
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().program.strings = vec!["LoGiC".into()];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.step(world.id(1).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[0].attributes()[1], 4);
    assert_eq!(world.actors()[0].attributes()[119], 4);
    assert_eq!(world.actors()[1].attributes(), &[0; 256]);
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [4]);
}
#[test]
fn unsupported_attribute_access_fails_before_mutation_and_remains_sticky() {
    for (id, name, index, value, reason) in [
        (1, "SetAttribute", -1, Some(9), "index outside"),
        (1, "SetAttribute", 256, Some(9), "index outside"),
        (3, "GetAttribute", 256, None, "index outside"),
    ] {
        let scripts = attribute_cache(vec![
            Operation::Call {
                call: attribute_call(id, name, index, value),
            },
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let actor = world.id(0).unwrap();
        let error = world.step(actor, Thread::Main, 20).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
        assert!(
            error.contains("world 7") && error.contains("instruction 0"),
            "{error}"
        );
        assert_eq!(world.actors()[0].attributes(), &[0; 256]);
        assert_eq!(world.flags(), [12]);
        assert_eq!(
            world.step(actor, Thread::Main, 20).unwrap_err().to_string(),
            error
        );
    }
}

#[test]
fn nested_families_compose_original_positions_and_keep_generation_identity() {
    let mut scripts = cache();
    let template = scripts.actors["LOGIC"].clone();
    scripts.actors.clear();
    scripts.instances.clear();
    for (index, (name, parent, master, position, angles)) in [
        ("ROOT", 0, 20, [100, 200, 300], [0, 0, 0]),
        ("PLATFORM", 20, 24, [100, 500, 300], [0, 0, 0]),
        ("LEVER", 24, 0, [110, 480, 330], [0, 512, 0]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut actor = template.clone();
        actor.instances = 1;
        actor.instance_variables.truncate(1);
        actor.attributes = BTreeMap::from([("attr_25".into(), master), ("attr_26".into(), parent)]);
        scripts.actors.insert(name.into(), actor);
        scripts.instances.push(Instance {
            object_id: index as u32,
            actor: name.into(),
            object_type: 0,
            active: 0,
            model_name: String::new(),
            model_mode: 0,
            light_intensity: 0,
            light_radius: 0,
            light_rgb: [0; 3],
            map_position: position,
            angles,
        });
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 12).unwrap();
    world.prepare_spatial().unwrap();
    let lever = world.id(2).unwrap();
    assert_eq!(
        world.spatial(lever).unwrap().local.position,
        [2560, -5120, 7680]
    );
    assert!(world.spatial(lever).unwrap().effective.is_none());
    world.refresh_families().unwrap();
    let pose = world.spatial(lever).unwrap();
    assert_eq!(pose.effective.unwrap().position, [28160, 122880, 84480]);
    assert_eq!(pose.effective.unwrap().angles, [0, 512, 0]);
    assert_eq!(pose.previous_effective_position, Some([2560, -5120, 7680]));
    assert!(world.prepare_spatial().is_err());
    let replacement = ScriptWorld::prepare(&scripts, 13).unwrap();
    assert!(
        replacement
            .spatial(lever)
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
}

#[test]
fn spatial_distance_requires_explicit_override_and_fallback_state() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let actor = world.spatial(world.id(0).unwrap()).unwrap();
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [256, -512, 768],
            heading: 2047,
        }),
        distance_override: None,
        fallback_position: None,
        linked_position: Some(None),
    };
    assert!(actor.query("PlayerDistance", &[], inputs).is_err());
    inputs.distance_override = Some(false);
    assert_eq!(
        actor.query("PlayerDistance", &[], inputs).unwrap(),
        Some(1536)
    );
    assert_eq!(
        actor
            .query(
                "PlayerFacing",
                &[Argument::Integer(0), Argument::Integer(1)],
                inputs
            )
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        actor
            .query(
                "PlayerFacing",
                &[Argument::Integer(0), Argument::Integer(-1)],
                inputs
            )
            .unwrap(),
        Some(0)
    );
    inputs.player = None;
    assert!(
        actor
            .query(
                "PlayerFacing",
                &[Argument::Integer(0), Argument::Integer(1)],
                inputs
            )
            .is_err()
    );
    assert!(actor.query("PlayerDistance", &[], inputs).is_err());
    inputs.fallback_position = Some([1, 2, 3]);
    assert_eq!(actor.query("PlayerDistance", &[], inputs).unwrap(), Some(6));
    inputs.distance_override = Some(true);
    assert_eq!(
        actor.query("PlayerDistance", &[], inputs).unwrap(),
        Some(9_999_999)
    );
}

#[test]
fn ambiguous_attachment_preparation_does_not_install_partial_state() {
    let mut data = cache();
    data.instances[1].active = 1;
    data.actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_25".into(), 1);
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    let error = world.prepare_spatial().unwrap_err();
    assert!(format!("{error:#}").contains("duplicate masters"));
    assert!(
        world
            .spatial(world.id(0).unwrap())
            .unwrap_err()
            .to_string()
            .contains("not prepared")
    );
    assert_eq!(world.flags(), [12]);
}

fn ordered_world() -> ScriptWorld {
    let mut data = cache();
    let placements = [
        (1, 1, 0, 0),
        (1, 1, 0, 0),
        (2, 1, 0, 0),
        (1, 2, 0, 0),
        (0, 1, 0, 0),
        (0, 1, 0, 0),
        (0, 0, 0, 0),
        (120, 120, 1, 0),
        (120, 120, 1, 0),
        (1, 1, 0, 2),
        (20, 20, 0, 0),
        (1, 0, 0, 0),
        (1, 17, 0, 0),
        (17, 17, 0, 0),
    ];
    let template = data.instances[0].clone();
    data.instances = placements
        .into_iter()
        .enumerate()
        .map(|(index, (x, z, active, object_type))| Instance {
            object_id: index as u32,
            actor: if index == 5 {
                "BORDER".into()
            } else {
                "LOGIC".into()
            },
            map_position: [x * 512, 0, z * 512],
            active,
            object_type,
            ..template.clone()
        })
        .collect();
    data.definition.functions.push(Function {
        name: "SetMyAttr".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    let actor = data.actors.get_mut("LOGIC").unwrap();
    actor.instances = 14;
    actor.instance_variables = vec![vec![4]; 14];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Call {
                    call: Call {
                        function_id: 1,
                        kind: CallKind::Function,
                        target: Object::Me,
                        function: "SetMyAttr".into(),
                        arguments: vec![Value::Integer { value: 113 }, Value::Integer { value: 0 }],
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
    let mut role = actor.clone();
    role.instances = 1;
    role.instance_variables = vec![vec![4]];
    for name in ["CYRUS", "GREMLIN", "CAMERA", "BORDER"] {
        let mut actor = role.clone();
        actor.namespace = Some(name.to_ascii_lowercase());
        if name == "BORDER" {
            actor.attributes.insert("attr_113".into(), 1);
        }
        data.actors.insert(name.into(), actor);
    }
    data.content_id = data.content_identity().unwrap();
    ScriptWorld::prepare(&data, 19).unwrap()
}

#[test]
fn ordinary_actor_lists_preserve_cells_borders_and_persistent_creation_order() {
    let mut world = ordered_world();
    let ids = world.ordinary_actors([4608, 4608]).unwrap();
    assert_eq!(
        ids.iter().map(|id| id.index).collect::<Vec<_>>(),
        [11, 4, 1, 0, 2, 3, 12, 16, 15, 14, 8, 7]
    );
    assert!(ids.iter().all(|id| id.generation == 19));
    assert!(world.ordinary_actors([-1, 0]).is_err());
    assert!(world.ordinary_actors([65536, 0]).is_err());
    assert_eq!(
        world
            .ordinary_actors([65535, 65535])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [16, 15, 14, 8, 7]
    );
}

#[test]
fn ordinary_border_selection_changes_only_when_the_view_starting_cell_changes() {
    let mut world = ordered_world();
    assert!(
        !world
            .ordinary_actors([4608, 4608])
            .unwrap()
            .iter()
            .any(|id| id.index == 5)
    );
    world.step(world.id(5).unwrap(), Thread::Main, 20).unwrap();
    assert_eq!(world.actors()[5].attributes()[113], 0);
    assert!(
        !world
            .ordinary_actors([5119, 4608])
            .unwrap()
            .iter()
            .any(|id| id.index == 5)
    );
    world.ordinary_actors([5120, 4608]).unwrap();
    let ids = world.ordinary_actors([4608, 4608]).unwrap();
    assert_eq!(
        ids.iter().map(|id| id.index).collect::<Vec<_>>(),
        [11, 5, 4, 1, 0, 2, 3, 12, 16, 15, 14, 8, 7]
    );
}

#[test]
fn ordinary_actor_window_uses_the_original_upper_edge_origin() {
    let mut data = cache();
    data.instances[0].map_position = [109 * 512, 0, 110 * 512];
    data.instances[1].map_position = [126 * 512, 0, 126 * 512];
    data.content_id = data.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&data, 1).unwrap();
    assert_eq!(
        world
            .ordinary_actors([65535, 65535])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [0]
    );
}

#[test]
fn living_map_default_writes_do_not_change_the_combat_death_default() {
    use cyrus_assets::animation::Animations;
    for combat_attribute in [0, 1] {
        for (function, name) in [(1, "SetAttribute"), (2, "SetMyAttr")] {
            let mut scripts = attribute_cache(vec![
                Operation::Call {
                    call: attribute_call(function, name, 44, Some(-2)),
                },
                Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(attribute_call(3, "GetAttribute", 44, None)),
                        },
                        rest: vec![],
                    },
                },
                Operation::End { target: 0 },
            ]);
            scripts.instances[0].object_type = 1;
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .attributes
                .insert("attr_14".into(), combat_attribute);
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
                    cyrus_game::script::ScriptStage::Body(Thread::Main),
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            let actor = &world.actors()[0];
            assert_eq!(actor.attributes()[44], 254);
            assert_eq!(actor.runtime().unwrap().variables(), [254]);
            assert_eq!(
                actor.control_state().unwrap().combat_death_default,
                if combat_attribute == 0 { None } else { Some(0) }
            );
            assert_eq!(world.animation_frame(id).unwrap(), Some(10));
            assert_eq!(world.actors()[1].attributes()[44], 0);
        }
    }
}

#[test]
fn standalone_default_writes_use_initialized_player_control() {
    for (function, name) in [(1, "SetAttribute"), (2, "SetMyAttr")] {
        let mut write = attribute_call(function, name, 44, Some(7));
        write.target = Object::Player;
        let mut scripts = attribute_cache(vec![
            Operation::ObjectCall { call: write },
            Operation::End { target: 0 },
        ]);
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
        let player = world
            .actors()
            .iter()
            .find(|actor| actor.name() == "CYRUS")
            .unwrap();
        assert_eq!(player.control_state().unwrap().living_state, 0);
        assert_eq!(player.attributes()[44], 7);
        let (_, state) = world.player_activation_state().unwrap();
        assert!(state.compass_eligible(player.attributes()[110], true, false));
    }
}

#[test]
fn map_default_selection_observes_script_writes_and_updates_the_shared_frame() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = attribute_cache(vec![
        Operation::Call {
            call: attribute_call(1, "SetAttribute", 44, Some(262)),
        },
        Operation::Yield,
        Operation::Call {
            call: attribute_call(2, "SetMyAttr", 110, Some(1)),
        },
        Operation::End { target: 0 },
    ]);
    scripts.instances[0].object_type = 1;
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
    assert_eq!(world.actors()[0].attributes()[44], 6);
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
        selected.bind_animation(id, &data, initial_group).unwrap();
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
fn spatial_axis_locks_restore_independent_saved_axes_and_leave_the_matrix_unchanged() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let template = world.spatial(world.id(0).unwrap()).unwrap();
    for (attribute, position, angles) in [
        (40, [100, 500, 600], [400, 500, 600]),
        (41, [400, 200, 600], [400, 500, 600]),
        (42, [400, 500, 300], [400, 500, 600]),
        (38, [400, 500, 600], [100, 500, 600]),
        (37, [400, 500, 600], [400, 200, 600]),
        (39, [400, 500, 600], [400, 500, 300]),
    ] {
        for slave in [0, 1] {
            let mut pose = template.clone();
            pose.local.position = [100, 200, 300];
            pose.local.angles = [100, 200, 300];
            pose.snapshot();
            pose.local.position = [400, 500, 600];
            pose.local.angles = [400, 500, 600];
            let matrix = pose.local.matrix;
            let mut attributes = [0; 256];
            attributes[attribute] = 255;
            attributes[26] = slave;
            pose.restore_axis_locks(&attributes).unwrap();
            assert_eq!(
                pose.local.position,
                if slave == 0 {
                    position
                } else {
                    [400, 500, 600]
                }
            );
            assert_eq!(
                pose.local.angles,
                if slave == 0 { angles } else { [400, 500, 600] }
            );
            assert_eq!(pose.local.matrix, matrix);
        }
    }
    let mut pose = template.clone();
    assert!(pose.restore_axis_locks(&[0; 255]).is_err());
}

#[test]
fn native_hook_updates_only_its_receiver_without_advancing_the_script() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "SetMyAttr".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
        function_id: 1,
        function: "SetMyAttr".into(),
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![Value::Integer { value: 6 }, Value::Integer { value: 19 }],
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 17).unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::NativeHook,
            ScriptInputs::default(),
            None,
            None,
            0,
        )
        .unwrap();
    assert_eq!(world.actors()[0].attributes()[6], 19);
    assert_eq!(world.actors()[1].attributes()[6], 0);
    assert_eq!(
        world.actors()[0]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
}

#[test]
fn disabling_a_script_finishes_its_invocation_and_freezes_later_main_task_passes() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    for (name, param_count, kind) in [
        ("DisableScript", 0, CallKind::Function),
        ("EnableScript", 0, CallKind::Function),
        ("Wait", 1, CallKind::Task),
    ] {
        scripts.definition.functions.push(Function {
            name: name.into(),
            param_count,
            kind,
        });
    }
    let control_call = |enabled| Call {
        function_id: if enabled { 2 } else { 1 },
        function: if enabled {
            "EnableScript"
        } else {
            "DisableScript"
        }
        .into(),
        kind: CallKind::Function,
        target: Object::Me,
        arguments: vec![],
    };
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.native_hook = Some(control_call(true));
    actor.program.steps = [
        Operation::Call {
            call: Call {
                function_id: 3,
                function: "Wait".into(),
                kind: CallKind::Multitask,
                target: Object::Me,
                arguments: vec![Value::Integer { value: 100 }],
            },
        },
        Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(control_call(false)),
                },
                rest: vec![],
            },
        },
        Operation::Increment {
            destination: Place::Flag { index: 0 },
            delta: 1,
        },
        Operation::End { target: 2 },
    ]
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
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 31).unwrap();
    let id = world.id(0).unwrap();
    let inputs = ScriptInputs {
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.flags(), [13]);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(world.actors()[0].control_state().unwrap().script_control, 1);
    assert_eq!(world.actors()[1].control_state().unwrap().script_control, 0);
    for _ in 0..200 {
        world
            .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
            .unwrap();
    }
    assert_eq!(world.flags(), [13]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .advance(id, ScriptStage::NativeHook, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].control_state().unwrap().script_control, 0);
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.flags(), [14]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
}

#[test]
fn ordinary_hook_observes_unpaused_multitasks_and_installed_task_switch() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for task_system in [false, true] {
        for paused in [false, true] {
            let mut operations = vec![wait(
                1,
                "Wait",
                CallKind::Multitask,
                vec![Value::Integer { value: 100 }],
            )];
            if paused {
                operations.push(Operation::TaskPause { label: 0 });
            }
            operations.push(Operation::End { target: 0 });
            let mut scripts = wait_cache(operations);
            scripts.updates.task_system = task_system;
            scripts.definition.functions.push(Function {
                name: "SetMyAttr".into(),
                param_count: 2,
                kind: CallKind::Function,
            });
            scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
                function_id: 4,
                function: "SetMyAttr".into(),
                kind: CallKind::Function,
                target: Object::Me,
                arguments: vec![Value::Integer { value: 6 }, Value::Integer { value: 19 }],
            });
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
            let id = world.id(0).unwrap();
            let inputs = ScriptInputs {
                frame_scale: Some(256),
                ..Default::default()
            };
            world
                .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
            assert!(!world.actors()[0].runtime().unwrap().has_blocking_task());
            world
                .advance(id, ScriptStage::OrdinaryHook, inputs, None, None, 20)
                .unwrap();
            assert_eq!(
                world.actors()[0].attributes()[6],
                if !task_system || paused { 19 } else { 0 }
            );
            assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        }
    }
}

#[test]
fn disabled_task_system_preserves_main_continuation_and_task_state() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = wait_cache(vec![
        wait(1, "Wait", CallKind::Task, vec![Value::Integer { value: 3 }]),
        set_flag(99),
        Operation::End { target: 0 },
    ]);
    scripts.updates.task_system = false;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let inputs = ScriptInputs {
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert_eq!(
        world.actors()[0]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
    world
        .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 20)
        .unwrap();
    for _ in 0..10 {
        world
            .advance(id, ScriptStage::OrdinaryMain, inputs, None, None, 20)
            .unwrap();
    }
    assert_eq!(world.flags(), [12]);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .advance(id, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .advance(id, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

fn marker_world(position: [i32; 3], marker: i32, distance: i32) -> ScriptWorld {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "MoveByAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        },
        Function {
            name: "AtNodeMarker".into(),
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    let mut operations: Vec<_> = position
        .into_iter()
        .enumerate()
        .map(|(axis, value)| {
            wait(
                1,
                "MoveByAxis",
                CallKind::Multitask,
                vec![
                    Value::Integer { value: axis as i32 },
                    Value::Integer { value },
                    Value::Integer { value: 1 },
                ],
            )
        })
        .collect();
    operations.push(Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 2,
                    function: "AtNodeMarker".into(),
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![
                        Value::Integer { value: marker },
                        Value::Integer { value: distance },
                    ],
                }),
            },
            rest: vec![],
        },
    });
    operations.push(Operation::End { target: 0 });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":[{"map_position":[0,0,0],"reserved":1},{"map_position":[0,0,0],"reserved":0}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let result = world.advance(
        id,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            ..Default::default()
        },
        None,
        None,
        20,
    );
    if matches!(marker, 0) {
        result.unwrap();
    } else {
        assert!(result.is_err());
    }
    world
}

#[test]
fn node_marker_query_uses_euclidean_distance_including_the_boundary_and_vertical_axis() {
    for (position, distance, expected) in [
        ([768, 1024, 0], 1279, 0),
        ([768, 1024, 0], 1280, 1),
        ([0, 0, 0], -1, 0),
        ([0, 0, 0], 0, 1),
        ([40265340, 53687120, 1], 67108900, 0),
        ([40265340, 53687120, 1], 67108904, 1),
    ] {
        assert_eq!(
            marker_world(position, 0, distance).flags(),
            [expected],
            "position {position:?} distance {distance}"
        );
    }
}

#[test]
fn unavailable_marker_query_is_contextual_and_sticky() {
    for (marker, message) in [(-1, "outside"), (1, "unavailable"), (128, "outside")] {
        let mut world = marker_world([0; 3], marker, 0);
        let error = world.actors()[0].failure().unwrap().to_owned();
        assert!(
            error.contains("world 7")
                && error.contains(&format!("marker {marker}"))
                && error.contains(message),
            "{error}"
        );
        assert_eq!(world.flags(), [12]);
        let id = world.id(0).unwrap();
        assert!(world.step(id, Thread::Main, 20).is_err());
        assert_eq!(world.actors()[0].failure(), Some(error.as_str()));
    }
}

#[test]
fn marker_preparation_rejects_malformed_or_mismatched_data_before_installing_state() {
    use cyrus_assets::animation::Animations;
    let scripts = cache();
    let data = |markers| Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":markers}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let valid = serde_json::json!([{"map_position":[0,0,0],"reserved":1}]);
    for malformed in [
        serde_json::Value::Null,
        serde_json::json!([{"map_position":[0,0],"reserved":1}]),
        serde_json::json!([{"map_position":[8388608,0,0],"reserved":1}]),
        serde_json::json!(vec![
            serde_json::json!({"map_position":[0,0,0],"reserved":1});
            129
        ]),
    ] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let error = world.prepare_markers(&data(malformed)).unwrap_err();
        assert!(format!("{error:#}").contains("world 7 marker"));
        world.prepare_markers(&data(valid.clone())).unwrap();
        assert!(world.prepare_markers(&data(valid.clone())).is_err());
    }
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut mismatch = data(valid.clone());
    mismatch.metadata["script_content_id"] = "old".into();
    assert!(
        world
            .prepare_markers(&mismatch)
            .unwrap_err()
            .to_string()
            .contains("cache generation")
    );
    mismatch.metadata["script_content_id"] = scripts.content_id.clone().into();
    mismatch.world += 1;
    assert!(world.prepare_markers(&mismatch).is_err());
    world.prepare_markers(&data(valid)).unwrap();
}

#[test]
fn routed_node_marker_query_reads_the_receivers_local_attached_position() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "AtNodeMarker".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["child".into()];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Flag { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id: 1,
                                function: "AtNodeMarker".into(),
                                kind: CallKind::Function,
                                target: Object::Named { string: 0 },
                                arguments: vec![
                                    Value::Integer { value: 0 },
                                    Value::Integer { value: 1280 },
                                ],
                            }),
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
    let mut child = actor.clone();
    child.namespace = Some("child".into());
    child.attributes.insert("attr_26".into(), 1);
    actor.attributes.insert("attr_25".into(), 1);
    scripts.actors.insert("CHILD".into(), child);
    scripts.instances[0].map_position = [100, 0, 0];
    scripts.instances[1].map_position = [103, 4, 0];
    scripts.instances[1].actor = "CHILD".into();
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"markers":[{"map_position":[0,0,0],"reserved":2}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_markers(&data).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let parent = world.id(0).unwrap();
    let child = world.id(1).unwrap();
    assert_eq!(world.spatial(child).unwrap().local.position, [768, 1024, 0]);
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position,
        [26368, 1024, 0]
    );
    world
        .advance(
            parent,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(0),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.flags(), [1]);
    assert_eq!(
        world.actors()[1]
            .runtime()
            .unwrap()
            .program_counter(Thread::Main),
        Some(0)
    );
}

fn random_scripts(function: &str, arguments: Vec<Value>, target: Object) -> Scripts {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: function.into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.program.strings = vec!["logic".into()];
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(Call {
                                function_id: 1,
                                function: function.into(),
                                kind: CallKind::Function,
                                target,
                                arguments,
                            }),
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
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

fn assign_call(next: u32, function_id: u16, function: &str, arguments: &[i32]) -> Step {
    Step {
        next,
        operation: Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id,
                        function: function.into(),
                        kind: CallKind::Function,
                        target: Object::Me,
                        arguments: arguments
                            .iter()
                            .map(|&value| Value::Integer { value })
                            .collect(),
                    }),
                },
                rest: vec![],
            },
        },
    }
}

/// Runs the given native calls in order on one logical actor and returns its first local.
fn run_native_calls(calls: &[(&str, &[i32])]) -> anyhow::Result<i32> {
    Ok(run_native_world(calls)?.actors()[0]
        .runtime()
        .unwrap()
        .variables()[0])
}

fn run_native_world(calls: &[(&str, &[i32])]) -> anyhow::Result<ScriptWorld> {
    run_native_world_with(calls, false)
}

fn run_native_world_with(calls: &[(&str, &[i32])], spatial: bool) -> anyhow::Result<ScriptWorld> {
    let mut scripts = random_scripts("Rnd", vec![], Object::Me);
    scripts.definition.functions.clear();
    scripts.definition.functions.push(Function {
        name: "NullFunction".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    let mut ids = BTreeMap::new();
    for (name, arguments) in calls {
        ids.entry(*name).or_insert_with(|| {
            scripts.definition.functions.push(Function {
                name: (*name).into(),
                param_count: arguments.len() as u8,
                kind: CallKind::Function,
            });
            scripts.definition.functions.len() as u16 - 1
        });
    }
    let mut steps = BTreeMap::new();
    for (index, (name, arguments)) in calls.iter().enumerate() {
        steps.insert(
            index as u32,
            assign_call(index as u32 + 1, ids[name], name, arguments),
        );
    }
    steps.insert(
        calls.len() as u32,
        Step {
            next: calls.len() as u32 + 1,
            operation: Operation::End { target: 0 },
        },
    );
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    if spatial {
        world.prepare_spatial().unwrap();
    }
    let id = world.id(0).unwrap();
    world.advance(
        id,
        cyrus_game::script::ScriptStage::OrdinaryMain,
        cyrus_game::script::ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(3),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    Ok(world)
}

#[test]
fn potion_counters_start_zero_keep_values_per_index_and_reject_unknown_indices() {
    assert_eq!(run_native_calls(&[("GetPotion", &[2])]).unwrap(), 0);
    assert_eq!(run_native_calls(&[("SetPotion", &[2, 7])]).unwrap(), 1);
    assert_eq!(
        run_native_calls(&[("SetPotion", &[2, 7]), ("GetPotion", &[2])]).unwrap(),
        7
    );
    assert_eq!(
        run_native_calls(&[("SetPotion", &[2, 7]), ("GetPotion", &[3])]).unwrap(),
        0
    );
    for call in [("GetPotion", &[4][..]), ("SetPotion", &[-1, 1][..])] {
        let error = run_native_calls(&[call]).unwrap_err();
        assert!(format!("{error:#}").contains("outside 0..4"), "{error:#}");
    }
}

#[test]
fn in_rectangle_tests_the_receiver_position_with_normalized_inclusive_corners() {
    let origin = {
        let world = run_native_world_with(&[], true).unwrap();
        world.spatial(world.id(0).unwrap()).unwrap().local.position
    };
    let (x, z) = (origin[0], origin[2]);
    let call = |x1: i32, z1: i32, x2: i32, z2: i32| {
        let world = run_native_world_with(&[("InRectangle", &[x1, z1, x2, z2])], true).unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    assert_eq!(call(x - 1, z - 1, x + 1, z + 1), 1);
    assert_eq!(
        call(x + 1, z + 1, x - 1, z - 1),
        1,
        "corners are normalized"
    );
    assert_eq!(call(x, z, x, z), 1, "edges are inclusive");
    assert_eq!(call(x + 1, z - 1, x + 5, z + 1), 0);
    assert_eq!(call(x - 1, z + 1, x + 1, z + 5), 0);
    assert!(run_native_world(&[("InRectangle", &[0, 0, 1, 1])]).is_err());
}

#[test]
fn light_controls_create_a_disabled_record_then_update_it_with_low_word_stores() {
    let light = |calls: &[(&str, &[i32])]| {
        run_native_world(calls).unwrap().actors()[0]
            .light_state()
            .unwrap()
    };
    let fresh = light(&[("LightOffset", &[1, 2, 3])]);
    assert_eq!(fresh["enabled"], false);
    assert_eq!(
        (fresh["intensity"].clone(), fresh["radius"].clone()),
        (256.into(), 100.into())
    );
    assert_eq!(fresh["offset"], serde_json::json!([1, 2, 3]));
    let lit = light(&[("Light", &[50, 200])]);
    assert_eq!(lit["enabled"], true);
    assert_eq!(
        (lit["radius"].clone(), lit["intensity"].clone()),
        (50.into(), 200.into())
    );
    assert_eq!(lit["starting_radius"], 50);
    assert_eq!(
        light(&[("LightIntensity", &[65537])])["intensity"],
        1,
        "low word kept"
    );
    assert_eq!(light(&[("LightRadius", &[70])])["enabled"], true);
    assert_eq!(
        light(&[("LightFlicker", &[7, 9])])["flicker"],
        serde_json::json!([7, 0, 9])
    );
    assert_eq!(
        light(&[("FlickerLight", &[7])])["flicker"],
        serde_json::json!([7, 0, 1])
    );
    assert_eq!(
        light(&[("LightFlicker", &[7, 9]), ("LightFlickerOff", &[])])["flicker"],
        serde_json::json!([0, 0, 9])
    );
    assert!(
        run_native_world(&[("Light", &[1])]).is_err(),
        "wrong argument count"
    );
}

#[test]
fn frame_passed_reports_the_shared_nominal_tick_increment() {
    assert_eq!(run_native_calls(&[("GetFramePassed", &[])]).unwrap(), 3);
}

#[test]
fn hook_toggles_set_and_clear_only_the_native_hook_suppression_bit() {
    let hook_bit = |calls: &[(&str, &[i32])]| {
        let world = run_native_world(calls).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        (
            control.script_control,
            world.actors()[1].control_state().unwrap().script_control,
        )
    };
    assert_eq!(hook_bit(&[("DisableHook", &[])]), (2, 0));
    assert_eq!(
        hook_bit(&[("DisableHook", &[]), ("EnableHook", &[])]),
        (0, 0)
    );
    assert_eq!(hook_bit(&[("EnableHook", &[])]), (0, 0));
}

#[test]
fn strength_and_armor_need_a_combat_record() {
    for name in ["setStrength", "setArmor", "AdjustStrength", "AdjustArmor"] {
        let error = run_native_calls(&[(name, &[100][..])]).unwrap_err();
        assert!(
            format!("{error:#}").contains("receiver combat record unavailable"),
            "{error:#}"
        );
    }
}

#[test]
fn random_stream_is_shared_by_routed_actors_and_survives_world_replacement() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    let scripts = random_scripts(
        "QuickRnd",
        vec![Value::Integer { value: -1 }],
        Object::Named { string: 0 },
    );
    let mut random = EngineRandom::default();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (index, expected) in [(1, 16838), (0, 5758)] {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].runtime().unwrap().variables(),
            [expected]
        );
    }
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [16838]);
    let mut replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    replacement
        .advance(
            replacement.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(
        replacement.actors()[0].runtime().unwrap().variables(),
        [10113]
    );
    assert_eq!(random.state(), 662824084);
}

#[test]
fn random_handler_requires_shared_state_and_invalid_bounds_fail_stickily() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    for (function, bound, supplied, message) in [
        (
            "QuickRnd",
            127,
            false,
            "shared engine random stream unavailable",
        ),
        ("Rnd", 0, true, "positive bound"),
        ("Rnd", -1, true, "positive bound"),
    ] {
        let scripts = random_scripts(function, vec![Value::Integer { value: bound }], Object::Me);
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let mut random = EngineRandom::default();
        let id = world.id(0).unwrap();
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                Default::default(),
                None,
                supplied.then_some(&mut random),
                20,
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains(message), "{error}");
        assert!(world.actors()[0].failure().is_some());
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
        assert_eq!(random.state(), 1);
        assert!(
            world
                .advance(
                    id,
                    ScriptStage::Body(Thread::Main),
                    Default::default(),
                    None,
                    Some(&mut random),
                    20
                )
                .is_err()
        );
        assert_eq!(random.state(), 1);
    }
}

#[test]
fn rnd_script_masks_before_reducing_and_consumes_one_draw() {
    use cyrus_game::{random::EngineRandom, script::ScriptStage};
    let scripts = random_scripts("Rnd", vec![Value::Integer { value: 3 }], Object::Me);
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let mut random = EngineRandom::default();
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            Default::default(),
            None,
            Some(&mut random),
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [2]);
    assert_eq!(random.state(), 1103527590);
}

fn wander_world(radius: i32, movement: i32, maps: serde_json::Value) -> ScriptWorld {
    use cyrus_assets::animation::Animations;
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "MoveByAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        },
        Function {
            name: "Wander".into(),
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    scripts.instances[0].map_position = [1000, 2, 2000];
    let mut operations = Vec::new();
    for (axis, delta) in [(0, movement), (1, 256)] {
        operations.push(wait(
            1,
            "MoveByAxis",
            CallKind::Multitask,
            vec![
                Value::Integer { value: axis },
                Value::Integer { value: delta },
                Value::Integer { value: 1 },
            ],
        ));
    }
    operations.push(Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 2,
                    function: "Wander".into(),
                    kind: CallKind::Function,
                    target: Object::Me,
                    arguments: vec![
                        Value::Integer { value: radius },
                        Value::Integer { value: 32768 },
                    ],
                }),
            },
            rest: vec![],
        },
    });
    operations.push(Operation::End { target: 0 });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id, "walk_node_maps": maps}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_navigation(&data).unwrap();
    world.prepare_spatial().unwrap();
    world
}

#[test]
fn empty_map_wander_consumes_original_draws_and_restores_excess_displacement() {
    use cyrus_game::{
        random::EngineRandom,
        script::{ScriptInputs, ScriptStage},
    };
    for (radius, movement, scale, state, restore) in [
        (4, 0, 256, 2524885223, false),
        (5, 0, 256, 3295386429, false),
        (1204, 0, 256, 2524885223, false),
        (1204, 8448, 256, 2524885223, true),
        (1204, 8192, 256, 2524885223, false),
        (1204, 256, 0, 2524885223, true),
    ] {
        let mut world = wander_world(radius * 256, movement, serde_json::json!([]));
        let id = world.id(0).unwrap();
        let mut random = EngineRandom::default();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(scale),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [movement]);
        assert_eq!(random.state(), state, "radius {radius}");
        assert_eq!(world.actors()[0].attributes()[94], 1);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if restore {
                [256000, 512, 512000]
            } else {
                [256000 + movement, 768, 512000]
            }
        );
    }
}

#[test]
fn wander_rejects_nonempty_maps_and_zero_radius_without_consuming_random() {
    use cyrus_game::{
        random::EngineRandom,
        script::{ScriptInputs, ScriptStage},
    };
    for (maps, radius, expected) in [
        (
            serde_json::json!([{"node_count":0,"walk_nodes":[]}]),
            1024,
            "walk-node maps unsupported",
        ),
        (serde_json::json!([]), 255, "positive map-unit radius"),
    ] {
        let mut world = wander_world(radius, 0, maps);
        let id = world.id(0).unwrap();
        let mut random = EngineRandom::default();
        let error = world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                Some(&mut random),
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(random.state(), 1);
        assert!(world.actors()[0].failure().is_some());
    }
}

#[test]
fn visibility_calls_return_zero_without_disabling_the_receivers_script() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (function, hidden) in [("HideMe", true), ("ShowMe", false), ("isDead", false)] {
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
fn kill_me_marks_the_receiver_dead_hides_its_model_and_stops_its_sounds() {
    use cyrus_game::script::{ScriptInputs, ScriptSound, ScriptStage};
    let mut scripts = random_scripts("KillMe", vec![], Object::Me);
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
    assert_eq!(control.death_selector, 8);
    assert!(!world.actors()[1].control_state().unwrap().killed);
    assert!(matches!(sounds.as_slice(), [(actor, ScriptSound::StopOwned)] if *actor == id));
}

#[test]
fn automatic_world_updates_advance_authored_actors_and_pause_in_menu() {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    use cyrus_assets::animation::Animations;
    use cyrus_game::{
        AppState,
        script::{ScriptActors, ScriptPlugin, ScriptView},
    };
    use cyrus_world::{
        animation::{AnimationRequest, AnimationTick},
        lifecycle::PreparedWorld,
    };
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = 1;
        placement.active = 1;
    }
    scripts.definition.functions.push(Function {
        name: "PlayerFacing".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&1)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Flag { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    function: "PlayerFacing".into(),
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![Value::Integer { value: 0 }, Value::Integer { value: 0 }],
                }),
            },
            rest: vec![],
        },
    };
    scripts.definition.functions.push(Function {
        name: "PlayerDistance".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.definition.flags.push(Flag {
        name: "Distance".into(),
        flag_type: "NUMBER".into(),
        initial_value: -1,
    });
    let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
    program.steps.insert(
        3,
        Step {
            next: 4,
            operation: Operation::End { target: 0 },
        },
    );
    program.steps.insert(
        2,
        Step {
            next: 3,
            operation: Operation::Assign {
                destination: Place::Flag { index: 1 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(Call {
                            function_id: 2,
                            function: "PlayerDistance".into(),
                            target: Object::Me,
                            kind: CallKind::Function,
                            arguments: vec![],
                        }),
                    },
                    rest: vec![],
                },
            },
        },
    );
    for (name, origin) in [("CYRUS", 1000), ("GREMLIN", 2000)] {
        let mut role = scripts.actors["LOGIC"].clone();
        role.namespace = Some(name.into());
        role.code_origin = origin;
        role.instances = 1;
        role.instance_variables.truncate(1);
        role.attributes.insert("attr_14".into(), 1);
        role.attributes.insert("attr_23".into(), 1);
        scripts.actors.insert(name.into(), role);
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({
            "script_content_id": scripts.content_id, "markers": [], "walk_node_maps": [],
            "mpob_objects": [{"script_name":"LOGIC", "object_type":1},{"script_name":"LOGIC", "object_type":1}],
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "raan_entries":[{"file_path":"LOGIC.3D", "frame_count":14}],
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},
                        {"opcode":0,"frame_reference":11}, {"opcode":0,"frame_reference":12},
                        {"opcode":0,"frame_reference":13}, {"opcode":2,"value":0}]}]}]
        }),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    data.models.insert(
        "LOGIC".into(),
        cyrus_assets::animation::AnimationModel {
            radius: Some(17.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![]; 14],
            normals: vec![vec![]; 14],
        },
    );
    data.metadata["markers"] = serde_json::json!([
        {"map_position":[10,20,30],"position":[0,0,0],"reserved":1}
    ]);
    for (name, effect) in [("CYRUS", 9), ("GREMLIN", 10)] {
        data.metadata["actors"].as_array_mut().unwrap().push(serde_json::json!({
            "script_name":name,"raan_entries":[{"file_path":"LOGIC.3D","frame_count":14}],
            "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":4,"sound_param":effect,"volume_shift":0},
                    {"opcode":0,"frame_reference":1},{"opcode":0,"frame_reference":2},{"opcode":2,"value":0}]}]
        }));
    }
    let mut hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: [(
            1,
            cyrus_assets::hud::InventoryItem {
                weapon: true,
                count: 1,
                total: 1,
                player_max: 1,
                ..Default::default()
            },
        )]
        .into(),
        start_items: vec![],
        items: vec![],
        selected: Some(1),
        bindings: [0; 15],
        font: Default::default(),
        text_alpha: [0; 2],
        player_text_alpha: [0; 2],
    };
    let hud = cyrus_game::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    let mut app = App::new();
    app.insert_resource(hud);
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_asset::<cyrus_game::player::data::PlayerScene>()
        .add_message::<AnimationTick>()
        .add_message::<cyrus_world::sound::SoundCue>()
        .init_resource::<cyrus_game::player::placement::MarkerEntry>()
        .add_plugins(ScriptPlugin);
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.insert_resource(PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(7),
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    let scene = cyrus_game::player::data::PlayerScene::decode(
        &serde_json::to_vec(&serde_json::json!({
            "version":2, "world":7,
            "settings":{"standing_height":-2,"post_collide_height":1,"slide_range":1,
                "turn_speed":1,"turn_max_speed":1,"poly_push_units":1},
            "definition":{}, "collision":{
                "placements":[],"models":{},"unsupported":{},
                "model_visibility":{"center":[0,0,0],"half_extent":0,"nodes":[]},
                "actor_models":{"object_count":2,"models":{},"instances":{},
                    "collision_flags":{"0":0,"1":0},"unsupported":{}}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let scene_handle = app
        .world()
        .resource::<AssetServer>()
        .load::<cyrus_game::player::data::PlayerScene>("test.player");
    app.world_mut()
        .resource_mut::<Assets<cyrus_game::player::data::PlayerScene>>()
        .insert(scene_handle.id(), scene)
        .unwrap();
    app.insert_resource(cyrus_game::player::data::PlayerSceneRequest(scene_handle));
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: [256000, 0, 256000],
    });
    let player = app
        .world_mut()
        .spawn((
            cyrus_game::player::Player,
            Transform::default(),
            cyrus_game::player::placement::PlayerPose {
                position: [256, 512, 768],
                heading: 0,
            },
        ))
        .id();
    app.world_mut()
        .spawn(cyrus_game::player::placement::PlayerPose {
            position: [999, 999, 999],
            heading: 1024,
        });
    app.update();
    let snapshot = |app: &App| {
        let actors = app.world().resource::<ScriptActors>();
        actors
            .current
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "script preparation failed: {:?}",
                    app.world().resource::<cyrus_game::script::ScriptStatus>()
                )
            })
            .inspect(0, 2)
    };
    assert_eq!(
        snapshot(&app)["actors"][0]["animation"],
        serde_json::Value::Null
    );
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let first = snapshot(&app);
    let role_snapshot = |app: &App| {
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .inspect(2, 2)
    };
    let mut initial_roles = role_snapshot(&app);
    assert_eq!(initial_roles["actors"][0]["attributes"][23], 1);
    assert_eq!(initial_roles["actors"][1]["attributes"][23], 1);
    assert_eq!(initial_roles["actors"][0]["animation"]["frame"], 1);
    assert_eq!(initial_roles["actors"][1]["animation"]["frame"], 1);
    let presented = app
        .world()
        .get::<cyrus_world::animation::AnimationPose>(player)
        .unwrap();
    assert_eq!((presented.model.as_str(), presented.frame), ("LOGIC", 1));
    let world = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap();
    assert!(world.actors()[2].control_state().unwrap().combat.is_some());
    let logical_pose = world
        .spatial(world.id(2).unwrap())
        .unwrap()
        .query_pose()
        .unwrap();
    assert_eq!(logical_pose.position, [256, 512, 768]);
    assert_eq!(logical_pose.angles, [0, 0, 0]);
    assert_eq!(world.globals().lockout(), Some(0));
    assert_eq!(world.globals().player_idle_counter(), Some(0));
    assert!(
        world.actors()[3]
            .control_state()
            .unwrap()
            .object_update_excluded
    );
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<cyrus_world::sound::SoundCue>>()
        .drain()
        .collect();
    let effects: Vec<_> = cues
        .iter()
        .filter(|cue| matches!(cue, cyrus_world::sound::SoundCue::Animation { .. }))
        .map(|cue| match cue {
            cyrus_world::sound::SoundCue::Animation {
                actor,
                sound: cyrus_world::animation::AnimationSound::Play { effect, .. },
                surface: Some(0),
                position: Some([2560, 5120, 7680]),
            } => (*actor, *effect),
            _ => panic!("unexpected initialization cue: {cue:?}"),
        })
        .collect();
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0], (player, 9));
    assert_eq!(effects[1].1, 10);
    assert_ne!(effects[1].0, player);
    let alternate = effects[1].0;
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [1, 1536],
        "automatic actor failure: {:?}",
        first["actors"][0]["failure"]
    );
    app.world_mut()
        .get_mut::<cyrus_game::player::placement::PlayerPose>(player)
        .unwrap()
        .heading = 1024;
    assert_eq!(first["actors"][0]["failure"], serde_json::Value::Null);
    assert_eq!(first["actors"][0]["locals"], serde_json::json!([5]));
    assert!(first["actors"][0]["animation"]["frame"].is_number());
    hud_asset.inventory.get_mut(&1).unwrap().count = 0;
    let unowned_hud = cyrus_game::script::ScriptHud::default();
    unowned_hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    app.insert_resource(unowned_hud);
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let globals = actors.current.as_mut().unwrap().globals_mut();
        globals
            .prepare_player_idle(
                false,
                Some(20),
                100,
                &mut cyrus_game::random::EngineRandom::from_state(5),
            )
            .unwrap();
    }
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(1),
    ));
    app.update();
    app.insert_resource(bevy::time::TimeUpdateStrategy::Automatic);
    let next = snapshot(&app);
    let logical_world = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap();
    assert_eq!(logical_world.globals().player_idle_counter(), Some(1));
    assert_eq!(
        logical_world
            .spatial(logical_world.id(2).unwrap())
            .unwrap()
            .local
            .angles,
        [0, 0, 0],
        "presentation changes must not reinitialize the logical player pose"
    );
    assert_eq!(
        app.world()
            .get::<cyrus_game::player::placement::PlayerPose>(player)
            .unwrap()
            .heading,
        0,
        "presentation must recover the authoritative logical heading"
    );
    initial_roles["actors"][0]["attributes"][23] = serde_json::json!(0);
    assert_eq!(role_snapshot(&app)["actors"], initial_roles["actors"]);
    assert!(
        app.world_mut()
            .resource_mut::<Messages<cyrus_world::sound::SoundCue>>()
            .drain()
            .all(|cue| !matches!(cue, cyrus_world::sound::SoundCue::Animation { .. }))
    );
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [1, 1536]
    );
    assert_eq!(next["actors"][0]["locals"], serde_json::json!([6]));
    assert_ne!(
        next["actors"][0]["animation"]["frame"],
        first["actors"][0]["animation"]["frame"]
    );
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    app.update();
    app.update();
    assert_eq!(snapshot(&app), next);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    assert_eq!(
        snapshot(&app)["actors"][0]["locals"],
        serde_json::json!([7])
    );
    app.init_resource::<cyrus_console::Console>();
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = true;
    let before_console = snapshot(&app);
    let before_idle = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .globals()
        .player_idle_counter();
    let before_clock = app
        .world()
        .resource::<cyrus_game::clock::SimulationClock>()
        .step();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(60),
    ));
    for _ in 0..3 {
        app.update();
        assert_eq!(snapshot(&app), before_console);
        assert_eq!(
            app.world()
                .resource::<cyrus_game::clock::SimulationClock>()
                .step(),
            before_clock
        );
    }
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = false;
    app.update();
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .globals()
            .player_idle_counter(),
        before_idle,
        "suspended BIOS time must not advance the idle counter"
    );
    assert_eq!(
        snapshot(&app)["actors"][0]["locals"],
        serde_json::json!([8])
    );
    assert_eq!(
        app.world()
            .resource::<cyrus_game::clock::SimulationClock>()
            .step()
            .frame_scale,
        before_clock.frame_scale
    );
    app.insert_resource(bevy::time::TimeUpdateStrategy::Automatic);
    let prepared = app.world().resource::<PreparedWorld>();
    let replacement = PreparedWorld {
        requested_name: prepared.requested_name.clone(),
        world: prepared.world,
        scripts: prepared.scripts.clone(),
        animations: prepared.animations.clone(),
        player_path: prepared.player_path.clone(),
        scene_path: prepared.scene_path.clone(),
        environment_path: prepared.environment_path.clone(),
    };
    let mut changed_scripts = app
        .world()
        .resource::<Assets<Scripts>>()
        .get(&replacement.scripts)
        .unwrap()
        .clone();
    changed_scripts.player_distance_override = true;
    changed_scripts.content_id = changed_scripts.content_identity().unwrap();
    let mut changed_animations = Animations {
        version: 2,
        world: changed_scripts.world,
        metadata: app
            .world()
            .resource::<Assets<Animations>>()
            .get(&replacement.animations)
            .unwrap()
            .metadata
            .clone(),
        sections: Default::default(),
        models: serde_json::from_value(
            serde_json::to_value(
                &app.world()
                    .resource::<Assets<Animations>>()
                    .get(&replacement.animations)
                    .unwrap()
                    .models,
            )
            .unwrap(),
        )
        .unwrap(),
        unsupported: Default::default(),
    };
    changed_animations.metadata["script_content_id"] = changed_scripts.content_id.clone().into();
    let mut replacement = replacement;
    replacement.scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(changed_scripts);
    replacement.animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(changed_animations);
    app.insert_resource(AnimationRequest(replacement.animations.clone()));
    app.insert_resource(ScriptView {
        source: replacement.animations.clone(),
        position: [256000, 0, 256000],
    });
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .begin_new_game();
    app.insert_resource(replacement);
    app.update();
    let replaced = snapshot(&app);
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .globals()
            .lockout(),
        Some(0),
        "world replacement must retain session lockout"
    );
    assert!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .globals()
            .player_idle_counter()
            .is_some_and(|counter| counter >= 1),
        "world replacement must not reset the session idle counter"
    );
    assert!(app.world().get_entity(alternate).is_err());
    assert_eq!(
        app.world_mut()
            .query::<&cyrus_game::script::ScriptBinding>()
            .iter(app.world())
            .count(),
        2
    );
    assert_ne!(replaced["generation"], first["generation"]);
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .flags(),
        [1, 9_999_999]
    );
    assert_eq!(replaced["actors"][0]["locals"], serde_json::json!([5]));
    struct EmptyCheatStore;
    impl cyrus_game::cheats::CheatStore for EmptyCheatStore {
        fn load(&self) -> anyhow::Result<cyrus_assets::cheats::CheatFile> {
            Ok(cyrus_assets::cheats::empty())
        }
        fn save(&self, _: &cyrus_assets::cheats::CheatFile) -> anyhow::Result<()> {
            Ok(())
        }
    }
    app.insert_resource(cyrus_game::cheats::Cheats::new(EmptyCheatStore));
    for (value, expected) in [(0, 1536), (1, 9_999_999), (0, 1536)] {
        app.world_mut()
            .resource_mut::<cyrus_game::cheats::Cheats>()
            .execute("neversaydie", Some(value))
            .unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .flags()[1],
            expected
        );
    }
    app.world_mut().remove_resource::<PreparedWorld>();
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&cyrus_world::sound::PlayerAnimationSource>()
            .iter(app.world())
            .count(),
        0
    );
    assert!(
        app.world()
            .get::<cyrus_game::script::ScriptBinding>(player)
            .is_none()
    );
}

#[test]
fn placed_distance_queries_use_receiver_owned_initial_link_state() {
    use cyrus_game::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.instances[1].map_position = [10, 0, 0];
    scripts.definition.functions.push(Function {
        name: "PlayerDistance".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    function: "PlayerDistance".into(),
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    for (index, expected) in [(0, 256), (1, 2304)] {
        world
            .advance(
                world.id(index).unwrap(),
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [256, 0, 0],
                            heading: 0,
                        }),
                        distance_override: Some(false),
                        fallback_position: None,
                        linked_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [expected]);
    }
}

#[test]
fn surface_class_queries_read_receivers_stored_surface_without_a_support_query() {
    let cases = [
        (
            "IsInDeepWater",
            (|surface| surface == 2 || surface == 4) as fn(u8) -> bool,
        ),
        ("IsInWater", |surface| (1..=4).contains(&surface)),
        ("IsInLava", |surface| surface == 5),
    ];
    for (name, hit) in cases {
        let mut scripts = cache();
        scripts.definition.functions.push(Function {
            name: name.into(),
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id: 1,
                        target: Object::Me,
                        kind: CallKind::Function,
                        function: name.into(),
                        arguments: vec![],
                    }),
                },
                rest: vec![],
            },
        };
        scripts.content_id = scripts.content_identity().unwrap();
        for surface in 0..=u8::MAX {
            let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
            let first = world.id(0).unwrap();
            let second = world.id(1).unwrap();
            world.set_actor_surface(first, surface).unwrap();
            world.step(first, Thread::Main, 20).unwrap();
            assert_eq!(world.flags(), [i32::from(hit(surface))]);
            world.step(second, Thread::Main, 20).unwrap();
            assert_eq!(
                world.flags(),
                [0],
                "another actor retains its allocation surface"
            );
            assert!(
                world
                    .set_actor_surface(
                        cyrus_game::script::ScriptActorId {
                            generation: 2,
                            index: 0
                        },
                        surface
                    )
                    .is_err()
            );
        }
    }
}

#[test]
fn fresh_player_deep_water_query_has_zero_surface_before_grounded_updates() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "IsInDeepWater".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    let mut actor = scripts.actors.remove("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.namespace = Some("cyrus".into());
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: "IsInDeepWater".into(),
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.actors.insert("CYRUS".into(), actor);
    scripts.instances.clear();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let player = world.id(0).unwrap();
    world.step(player, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [0]);
    assert!(world.actors()[0].failure().is_none());
}

#[test]
fn object_axis_task_moves_in_receiver_orientation_and_does_not_snap_at_target() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (axis, angles, delta, expected) in [
        (0, [0, 0, 0], 512, [256, 0, 0]),
        (1, [0, 0, 0], -512, [0, -256, 0]),
        (2, [0, 512, 0], 512, [-256, 0, 0]),
        (2, [512, 0, 0], 512, [0, 256, 0]),
        (0, [0, 0, 512], 512, [0, -256, 0]),
        (9, [0, 0, 0], 512, [0, 0, 0]),
    ] {
        let mut scripts = cache();
        scripts.definition.functions.push(Function {
            name: "MoveObjectAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts.instances[0].angles = angles;
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            1,
            "MoveObjectAxis",
            CallKind::Task,
            vec![
                Value::Integer { value: axis },
                Value::Integer { value: delta },
                Value::Integer { value: 2 },
            ],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world
            .advance(
                id,
                ScriptStage::Tasks,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            world.spatial(world.id(1).unwrap()).unwrap().local.position,
            [0; 3]
        );
    }
}

#[test]
fn deferred_movement_relocates_cells_and_retains_nearby_cached_order() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (excluded, distant_view) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut scripts = cache();
        scripts.instances[0].map_position = [1024, 0, 512];
        scripts.instances[1].map_position = [512, 0, 512];
        scripts.definition.functions.push(Function {
            name: "MoveObjectAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        });
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap()
            .operation = wait(
            1,
            "MoveObjectAxis",
            CallKind::Task,
            vec![
                Value::Integer { value: 0 },
                Value::Integer { value: -262144 },
                Value::Integer { value: 2 },
            ],
        );
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let original = world.ordinary_actors([0, 0]).unwrap().to_vec();
        assert_eq!(
            original.iter().map(|id| id.index).collect::<Vec<_>>(),
            [1, 0]
        );
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
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
            world.spatial(id).unwrap().local.position,
            [131072, 0, 131072]
        );
        let view = if distant_view {
            [6000 * 256, 0, 0]
        } else {
            [0; 3]
        };
        world.flush_map_updates(view, excluded).unwrap();
        let cached: Vec<_> = world
            .ordinary_actors([0, 0])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect();
        assert_eq!(
            cached,
            if distant_view && !excluded {
                vec![0, 1]
            } else {
                vec![1, 0]
            }
        );
        // The excluded pass consumes its registration; a later admitted pass cannot replay it.
        world.flush_map_updates([0; 3], false).unwrap();
        world.ordinary_actors([6000, 6000]).unwrap();
        let rebuilt: Vec<_> = world
            .ordinary_actors([0, 0])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect();
        assert_eq!(rebuilt, if excluded { vec![1, 0] } else { vec![0, 1] });
    }
}

#[test]
fn positional_sound_uses_the_routed_receivers_pose_and_returns_zero() {
    use cyrus_game::script::{ScriptSound, ScriptStage};
    let mut scripts = random_scripts(
        "Sound",
        vec![
            Value::Integer { value: 7 },
            Value::Integer { value: -40 },
            Value::Integer { value: 2000 },
        ],
        Object::Named { string: 0 },
    );
    scripts.definition.functions.last_mut().unwrap().param_count = 3;
    scripts.instances[0].map_position = [3, 4, 5];
    scripts.instances[1].map_position = [10, 20, 30];
    scripts.content_id = scripts.content_identity().unwrap();
    for surface in [0, 1, 6, 7, 12, 255] {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.refresh_families().unwrap();
        let caller = world.id(1).unwrap();
        let receiver = world.id(0).unwrap();
        world.set_actor_surface(receiver, surface).unwrap();
        let mut cues = Vec::new();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                cyrus_game::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                Some(&mut |actor, sound| {
                    cues.push((actor, sound));
                    Ok(())
                }),
                None,
                20,
            )
            .unwrap();
        assert_eq!(
            cues,
            [(
                receiver,
                ScriptSound::Positional {
                    effect: 7,
                    volume: -40,
                    rate_offset_hz: 2000,
                    position: [768, 1024, 1280],
                }
            )]
        );
        assert_eq!(world.actors()[1].runtime().unwrap().variables(), [0]);
    }
}

fn static_model_fixture(
    scripts: Scripts,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_collision::actor_models::ActorModelCollision,
) {
    static_model_fixture_at(scripts, None, [10, 20, 30])
}

fn static_model_fixture_at(
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

fn static_model_fixture_session(
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
        placement.object_type = 1;
        placement.map_position = map_position;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "mpob_objects": scripts.instances.iter().map(|_| serde_json::json!({"model_id":-1})).collect::<Vec<_>>(),
            "actors": scripts.actors.keys().map(|name| serde_json::json!({"script_name":name,"model_load_path":"static","mpsz_bounds_primary":-1,"raex":{"unknown_12":0}})).collect::<Vec<_>>()}),
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
    })
    .unwrap();
    let mut world = if fresh_session {
        use bevy::{asset::AssetApp, prelude::*};
        use cyrus_assets::animation::Animations;
        use cyrus_game::script::{ScriptActors, ScriptPlugin};
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
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "MoveByAxis".into(),
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
                    "MoveByAxis",
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
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert_eq!(support_height(&world), 21.0);
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        world.spatial(id).unwrap().local.position
    );
    assert!(
        world
            .model_presentation(cyrus_game::script::ScriptActorId {
                generation: 2,
                index: 0
            })
            .is_err()
    );
}

#[test]
fn hide_clears_model_and_show_restores_retained_static_resource_immediately() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts("HideMe", vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.definition.functions.push(Function {
        name: "ShowMe".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(1, "HideMe", CallKind::Function, vec![]),
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
                operation: wait(2, "ShowMe", CallKind::Function, vec![]),
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
    world.refresh_model_presentations(&data, &[id]).unwrap();
    assert!(world.model_presentation(id).unwrap().is_none());
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
fn animated_model_selection_retains_radius_when_hidden_and_clears_missing_geometry() {
    use cyrus_assets::{animation::AnimationModel, collision::ActorCollisionModels};
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts("HideMe", vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    for placement in &mut scripts.instances {
        placement.object_type = 1;
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
        placement.object_type = 1;
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
            .live_collision_inputs(&[cyrus_game::script::ScriptActorId {
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
            .live_collision_inputs(&[cyrus_game::script::ScriptActorId {
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
    use cyrus_game::{
        AppState,
        player::data::{MovementSettings, PlayerScene, PlayerSceneRequest},
        script::{ScriptActors, ScriptPlugin},
    };
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = 1;
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
        keyboard_codes: None,
        settings: MovementSettings {
            standing_height: -2,
            post_collide_height: 1,
            slide_range: 1,
            slide_speed: None,
            turn_speed: 1,
            turn_max_speed: 1,
            poly_push_units: 1,
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
            object_type: 1,
            map_position: [x * 512, 0, z * 512],
            active,
            ..template.clone()
        })
        .collect();
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 4;
    actor.instance_variables = vec![vec![4]; 4];
    actor.attributes.insert("attr_122".into(), 1);
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
fn automatic_model_admission_reads_retained_support_before_ground_processing() {
    use cyrus_assets::{
        animation::AnimationModel,
        collision::{ModelVisibility, VisibilityNode},
    };
    use cyrus_game::script::ModelSupportOwners;
    let mut scripts = cache();
    let mut support = scripts.actors["LOGIC"].clone();
    support.instances = 1;
    support.instance_variables = vec![vec![4]];
    support.attributes.insert("attr_122".into(), 1);
    scripts.actors.insert("SUPPORT".into(), support);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables = vec![vec![4]];
    scripts.instances[1].actor = "SUPPORT".into();
    for placement in &mut scripts.instances {
        placement.object_type = 1;
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
    world.refresh_automatic_models(&data, [0; 3]).unwrap();
    assert!(world.model_presentation(actor).unwrap().is_some());
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
    world.refresh_automatic_models(&data, [0; 3]).unwrap();
    assert!(world.model_presentation(actor).unwrap().is_none());
    assert_eq!(world.model_radius(actor).unwrap(), None);
    world.refresh_automatic_models(&data, [256; 3]).unwrap();
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
    world.refresh_automatic_models(&data, [256; 3]).unwrap();
    assert!(world.model_presentation(actor).unwrap().is_none());
    world.refresh_automatic_models(&data, [0; 3]).unwrap();
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
                    actor: Some(cyrus_game::script::ScriptActorId {
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
            cyrus_game::script::OrdinaryInputs {
                scene: None,
                scripts: cyrus_game::script::ScriptInputs {
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
}

#[test]
fn model_positioning_allows_known_dry_surfaces_with_collision_enabled() {
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_1".into(), 4);
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    for surface in [0, 6, 7, 8, 9, 10] {
        world.set_actor_surface(id, surface).unwrap();
        world.refresh_model_presentations(&data, &[id]).unwrap();
        assert_eq!(
            world.model_presentation(id).unwrap().unwrap().pose.position,
            [2560, 5120, 7680]
        );
    }
    world.set_actor_surface(id, 2).unwrap();
    assert!(
        world
            .refresh_model_presentations(&data, &[id])
            .unwrap_err()
            .to_string()
            .contains("water-dependent model position")
    );
    assert!(world.model_presentation(id).unwrap().is_none());
}

fn ordinary_collision_scene(
    world: u32,
    models: cyrus_collision::actor_models::ActorModelCollision,
) -> cyrus_game::player::data::PlayerScene {
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::player::data::{MovementSettings, PlayerScene};
    PlayerScene {
        source_identity: None,
        world,
        model_radii: Default::default(),
        keyboard_codes: None,
        settings: MovementSettings {
            standing_height: -2,
            post_collide_height: 1,
            slide_range: 1,
            slide_speed: None,
            turn_speed: 1,
            turn_max_speed: 1,
            poly_push_units: 1,
        },
        definition: None,
        terrain: None,
        collision: cyrus_assets::collision::CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        },
        static_collision: StaticCollision::new(&Default::default()).unwrap(),
        actor_collision: Some(models),
        unavailable: None,
    }
}

#[test]
fn ordinary_ground_retains_live_owner_excludes_self_and_has_no_player_fallback() {
    use cyrus_collision::ground::SupportSource;
    use cyrus_game::script::{ModelSupportOwners, OrdinaryGroundQuery, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "MoveByAxis".into(),
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
                    "MoveByAxis",
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
    let (mut world, data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let support_actor = world.id(1).unwrap();
    let order = [id, support_actor];
    world.refresh_model_presentations(&data, &order).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &order)
        .unwrap();
    let other_prepared = world
        .prepare_ordinary_collision_query(support_actor, &scene, &data, [10, 30], &order)
        .unwrap();
    let query = |prepared| OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared,
        distance: 50,
        sidedness: false,
    };
    let support = world
        .query_ordinary_ground(id, query(&prepared))
        .unwrap()
        .unwrap();
    assert_eq!(support.source(), SupportSource::Actor { actor: 1, face: 0 });
    assert_eq!(support.actor_height(0, -2), 18 * 256);
    assert_eq!(
        world.model_support(id).unwrap(),
        ModelSupportOwners {
            actor: Some(support_actor),
            static_placement: None,
        }
    );
    assert_eq!(world.actor_surface(id).unwrap(), 0);
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            support_actor,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    world
        .advance(support_actor, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(
        world.spatial(support_actor).unwrap().local.position[1],
        21 * 256
    );
    assert_eq!(
        world
            .query_ordinary_ground(id, query(&prepared))
            .unwrap()
            .unwrap()
            .actor_height(0, -2),
        18 * 256
    );
    world
        .refresh_model_presentations(&data, &[support_actor])
        .unwrap();
    assert_eq!(
        world
            .query_ordinary_ground(id, query(&prepared))
            .unwrap()
            .unwrap()
            .actor_height(0, -2),
        19 * 256
    );
    assert!(
        world
            .query_ordinary_ground(id, query(&other_prepared))
            .is_err()
    );
    let self_prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
        .unwrap();
    let empty_prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
        .unwrap();
    let body_query = |prepared| cyrus_game::script::OrdinaryBodyQuery {
        scene: &scene,
        animations: &data,
        prepared,
        sidedness: false,
        excluded_candidate_type: None,
        terrain: None,
    };
    let contacts = world
        .query_ordinary_body(id, body_query(&prepared))
        .unwrap();
    assert!(!contacts.is_empty());
    let residual = world
        .query_ordinary_residual_body(
            id,
            cyrus_game::script::OrdinaryResidualQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
            },
        )
        .unwrap();
    assert!(!residual.is_empty());
    assert!(
        residual
            .iter()
            .all(|contact| contact.source == SupportSource::Actor { actor: 1, face: 0 })
    );
    assert!(
        contacts
            .iter()
            .all(|contact| contact.source == SupportSource::Actor { actor: 1, face: 0 })
    );
    assert!(
        world
            .query_ordinary_body(id, body_query(&self_prepared))
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.model_support(id).unwrap().actor, Some(support_actor));
    assert!(
        world
            .query_ordinary_body(
                cyrus_game::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                body_query(&prepared)
            )
            .is_err()
    );
    // Only the queried actor remains. Its current model must not support itself.
    assert!(
        world
            .query_ordinary_ground(id, query(&self_prepared))
            .unwrap()
            .is_none()
    );
    // Query misses do not clear owners; the admitted update boundary does.
    assert_eq!(world.model_support(id).unwrap().actor, Some(support_actor));
    world.set_model_support(id, Default::default()).unwrap();
    assert!(
        world
            .query_ordinary_ground(id, query(&empty_prepared))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        world.model_support(id).unwrap(),
        ModelSupportOwners::default()
    );
    assert!(
        world
            .query_ordinary_ground(
                cyrus_game::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                query(&prepared)
            )
            .is_err()
    );
}

#[test]
fn ordinary_collision_admission_is_retained_across_script_movement() {
    use cyrus_game::script::{OrdinaryGroundQuery, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "MoveByAxis".into(),
        param_count: 3,
        kind: CallKind::Task,
    });
    let mut steps = BTreeMap::new();
    for (offset, delta, target) in [(0, 256000, 2), (2, -256000, 0)] {
        steps.insert(
            offset,
            Step {
                next: offset + 1,
                operation: wait(
                    1,
                    "MoveByAxis",
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer { value: delta },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        );
        steps.insert(
            offset + 1,
            Step {
                next: offset + 2,
                operation: Operation::End { target },
            },
        );
    }
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    let (mut world, data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            other,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.spatial(other).unwrap().local.position[0], 1010 * 256);
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let actors = [id, other];
    let retained = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &actors)
        .unwrap();
    world
        .advance(
            other,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.spatial(other).unwrap().local.position[0], 10 * 256);
    world.refresh_model_presentations(&data, &[other]).unwrap();
    let fresh = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &actors)
        .unwrap();
    let query = |prepared| OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared,
        distance: 50,
        sidedness: false,
    };
    assert!(
        world
            .query_ordinary_ground(id, query(&retained))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        world
            .query_ordinary_ground(id, query(&fresh))
            .unwrap()
            .unwrap()
            .source(),
        cyrus_collision::ground::SupportSource::Actor { actor: 1, face: 0 }
    );
}

#[test]
fn ordinary_collision_preparation_precedes_script_and_admitted_late_response() {
    use cyrus_game::script::{OrdinaryInputs, ScriptInputs};
    for (available, terrain) in [(true, false), (true, true), (false, false)] {
        let mut scripts = cache();
        scripts.definition.attributes = vec!["attr_0".into(), "attr_1".into()];
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert("attr_1".into(), 1);
        for placement in &mut scripts.instances {
            placement.active = 1;
        }
        let (mut world, data, models) = static_model_fixture_at(scripts, None, [600, 20, 600]);
        let mut scene = ordinary_collision_scene(world.world, models);
        if terrain {
            scene.terrain = Some(
                cyrus_collision::ground::TerrainGround::new(
                    vec![20; 256 * 256],
                    vec![false; 256 * 256],
                    None,
                )
                .unwrap(),
            );
        }
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: available.then_some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [if available { 5 } else { 4 }],
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables(),
            [if available { 10 } else { 9 }]
        );
        for actor in world.actors().iter().take(2) {
            if available {
                assert!(actor.failure().is_none());
            } else {
                assert!(
                    actor
                        .failure()
                        .unwrap()
                        .contains("collision scene unavailable")
                );
            }
        }
    }
}

#[test]
fn ordinary_combat_respects_scheduler_shell_without_activation_inputs() {
    use cyrus_game::script::{OrdinaryInputs, ScriptInputs};
    for shell in [true, false] {
        let mut scripts = cache();
        scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
        scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
            ("attr_1".into(), 1),
            ("attr_14".into(), 1),
            ("attr_119".into(), 7),
        ]);
        for placement in &mut scripts.instances {
            placement.active = 1;
        }
        let (mut world, mut data, models) = static_model_fixture_at(scripts, None, [600, 20, 600]);
        for actor in data.metadata["actors"].as_array_mut().unwrap() {
            actor["model_load_path"] = serde_json::json!("animation_table");
            actor["animation_groups"] = serde_json::json!([
                {"group_index":0,"anim_id":1,"anim_type":0,
                 "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}
            ]);
        }
        let scene = ordinary_collision_scene(world.world, models);
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world.send_combat_message(sender, receiver, 0xd001).unwrap();
        world
            .update_ordinary(
                &data,
                [600, 600],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        shell_execution: Some(shell),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        if shell {
            assert!(
                world.actors()[0].failure().is_none(),
                "{:?}",
                world.actors()[0].failure()
            );
            assert_eq!(world.actors()[0].attributes()[1], 1);
            assert_eq!(
                world
                    .consume_combat_message(receiver)
                    .unwrap()
                    .unwrap()
                    .code,
                0xd001
            );
        } else {
            assert_eq!(
                world.actors()[0].attributes()[1],
                7,
                "{:?}",
                world.actors()[0].failure()
            );
            assert!(
                world.actors()[0]
                    .failure()
                    .unwrap()
                    .contains("combat message 53249")
            );
            assert!(world.consume_combat_message(receiver).unwrap().is_none());
        }
    }
}

#[test]
fn ordinary_primary_response_owns_grounded_pose_heading_and_marks() {
    use cyrus_collision::{collision_world::RuntimeBodyContact, ground::SupportSource};
    use cyrus_game::{
        player::data::MovementSettings,
        script::{ScriptInputs, ScriptStage},
    };
    for (normals, slide_range, expected, heading, flags) in [
        (vec![([256, 0, 0], 1)], -60000, [0, 0, 2019], Some(313), 1),
        (
            vec![([256, 0, 0], 1), ([0, 0, -256], 1)],
            -60000,
            [-3564, 0, 0],
            None,
            1,
        ),
        (vec![([256, 0, 0], 1)], 0, [0; 3], Some(344), 2),
        (
            vec![([256, 0, 0], 4)],
            -60000,
            [-3341, 0, 1893],
            Some(344),
            0,
        ),
    ] {
        let mut scripts = cache();
        for placement in &mut scripts.instances {
            placement.object_type = 1;
            placement.angles = [0, 344, 0];
        }
        scripts.definition.functions.push(Function {
            name: "MoveByAxis".into(),
            param_count: 3,
            kind: CallKind::Task,
        });
        let mut steps = BTreeMap::new();
        for (offset, axis, delta) in [(0, 0, -3341), (1, 2, 1893)] {
            steps.insert(
                offset,
                Step {
                    next: offset + 1,
                    operation: wait(
                        1,
                        "MoveByAxis",
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: axis },
                            Value::Integer { value: delta },
                            Value::Integer { value: 1 },
                        ],
                    ),
                },
            );
        }
        steps.insert(
            2,
            Step {
                next: 3,
                operation: Operation::End { target: 0 },
            },
        );
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (
                control.sliding_state,
                control.falling_state,
                control.queued_motion,
                control.body_response_flags,
                control.collision_transition_flags
            ),
            (0, 0, 0, 0, 0)
        );
        world.prepare_spatial().unwrap();
        world.snapshot_spatial(id).unwrap();
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
        assert_eq!(world.spatial(id).unwrap().local.position, [-3341, 0, 1893]);
        let contacts = normals
            .into_iter()
            .map(|(normal, classification)| RuntimeBodyContact {
                source: SupportSource::Model { model: 0, face: 0 },
                point: [0; 3],
                normal,
                classification,
            })
            .collect::<Vec<_>>();
        let settings = MovementSettings {
            standing_height: -2,
            post_collide_height: 50,
            slide_range,
            slide_speed: None,
            turn_speed: 12,
            turn_max_speed: 48,
            poly_push_units: 16,
        };
        world
            .resolve_ordinary_primary_contacts(id, &contacts, &settings)
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        if let Some(heading) = heading {
            assert_eq!(world.spatial(id).unwrap().local.angles[1], heading);
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .body_response_flags,
            flags
        );
        assert_eq!(world.actor_surface(id).unwrap(), 0);
        let residue = |normal| RuntimeBodyContact {
            source: SupportSource::Model { model: 0, face: 0 },
            point: [0; 3],
            normal,
            classification: 1,
        };
        world
            .resolve_ordinary_residual_contacts(
                id,
                &[residue([256, 0, 0]), residue([-256, 0, 0])],
                &settings,
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .body_response_flags,
            flags | 4
        );
        world
            .resolve_ordinary_residual_contacts(id, &[residue([256, 0, 0])], &settings)
            .unwrap();
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [expected[0] + 4096, expected[1], expected[2]]
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .body_response_flags,
            flags | 4
        );
    }
}

#[test]
fn ordinary_collision_loader_distinguishes_authored_static_and_model_less_shapes() {
    use cyrus_collision::{actor_shape::BodySphere, collision_world::StaticCollision};
    use cyrus_game::player::data::{MovementSettings, PlayerScene};
    for (path, primary, selector, expected_y, generated) in [
        ("static", -1, -1, 0, true),
        ("static", 0, 1, 62, true),
        ("animation_table", 0, -1, 62, true),
        ("animation_table", -1, -1, 12, false),
        ("none", 0, -1, 12, false),
    ] {
        let (mut world, mut data, models) = static_model_fixture(cache());
        let id = world.id(0).unwrap();
        data.metadata["actors"][0]["model_load_path"] = path.into();
        data.metadata["actors"][0]["mpsz_bounds_primary"] = primary.into();
        data.metadata["mpob_objects"][0]["model_id"] = selector.into();
        data.metadata["mpsz_entries"] = serde_json::json!([{
            "total_extent":[48,128,49], "pos_extent":[24,62,24],
            "neg_extent":[24,66,25], "center_offset":[0,0,0], "flags":2,
        }]);
        let mut scene = PlayerScene {
            source_identity: None,
            world: world.world,
            model_radii: Default::default(),
            keyboard_codes: None,
            settings: MovementSettings {
                standing_height: -2,
                post_collide_height: 50,
                slide_range: 1,
                slide_speed: None,
                turn_speed: 1,
                turn_max_speed: 1,
                poly_push_units: 1,
            },
            definition: None,
            terrain: None,
            collision: cyrus_assets::collision::CollisionScene {
                sphere_object_scale: Some(256),
                ..Default::default()
            },
            static_collision: StaticCollision::new(&Default::default()).unwrap(),
            actor_collision: Some(models),
            unavailable: None,
        };
        if path == "static" && selector == -1 {
            scene.collision.sphere_object_scale = None;
            assert!(
                world
                    .prepare_ordinary_collision_shape(id, &data, &scene)
                    .unwrap_err()
                    .to_string()
                    .contains("sphere_object_scale")
            );
            assert!(world.ordinary_collision_shape(id).is_err());
            scene.collision.sphere_object_scale = Some(256);
        }
        world
            .prepare_ordinary_collision_shape(id, &data, &scene)
            .unwrap();
        let shape = world.ordinary_collision_shape(id).unwrap();
        assert_eq!(
            shape.positive_extent[1], expected_y,
            "{path}/{primary}/{selector}"
        );
        assert_eq!(shape.additional.is_some(), generated);
        if !generated {
            assert_eq!(
                shape.body,
                [BodySphere {
                    center: [0; 3],
                    radius: 12
                }]
            );
        } else if expected_y == 62 {
            assert_eq!(
                shape.body,
                [
                    BodySphere {
                        center: [0, -42, 0],
                        radius: 24
                    },
                    BodySphere {
                        center: [0, -12, 0],
                        radius: 24
                    }
                ]
            );
        }
        assert!(
            world
                .prepare_ordinary_collision_shape(id, &data, &scene)
                .is_err()
        );
    }
}

#[test]
fn ordinary_actor_overlap_reads_current_local_pose_and_excludes_self() {
    use cyrus_game::script::{ScriptActorId, ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "MoveByAxis".into(),
        param_count: 3,
        kind: CallKind::Task,
    });
    let mut steps = BTreeMap::new();
    for (offset, delta, target) in [(0, 83 * 256, 2), (2, 256, 2)] {
        steps.insert(
            offset,
            Step {
                next: offset + 1,
                operation: wait(
                    1,
                    "MoveByAxis",
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer { value: delta },
                        Value::Integer { value: 1 },
                    ],
                ),
            },
        );
        steps.insert(
            offset + 1,
            Step {
                next: offset + 2,
                operation: Operation::End { target },
            },
        );
    }
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    assert_eq!(
        world.query_ordinary_actor_overlap(id, id, &data).unwrap(),
        0
    );
    assert_eq!(
        world
            .query_ordinary_actor_overlap(id, other, &data)
            .unwrap(),
        84
    );
    let inputs = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    for expected in [1, 0] {
        world
            .advance(
                other,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            )
            .unwrap();
        // The model presentation still has its initial pose. Overlap follows
        // current logical coordinates independently of stored face geometry.
        assert_eq!(
            world
                .model_presentation(other)
                .unwrap()
                .unwrap()
                .pose
                .position[0],
            10 * 256
        );
        assert_eq!(
            world
                .query_ordinary_actor_overlap(id, other, &data)
                .unwrap(),
            expected
        );
    }
    assert!(
        world
            .query_ordinary_actor_overlap(
                id,
                ScriptActorId {
                    generation: 2,
                    index: 0
                },
                &data
            )
            .is_err()
    );
}

#[test]
fn ordinary_initial_overlap_shape_preserves_fractional_rotated_centers() {
    use cyrus_assets::{animation::Animations, collision::ActorCollisionModels};
    use cyrus_collision::actor_models::ActorModelCollision;
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = 1;
        placement.map_position = [10, 20, 30];
        placement.angles = [256, 0, 0];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "ravc_count":0,"mpsz_bounds_primary":0,"raex":{"unknown_12":0}}],
            "mpsz_entries":[{"total_extent":[48,128,49],"pos_extent":[24,62,24],
                "neg_extent":[24,66,25],"center_offset":[0,0,0]}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 2,
        collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
        ..Default::default()
    })
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let id = world.id(0).unwrap();
    world
        .prepare_ordinary_collision_shape(id, &data, &scene)
        .unwrap();
    let spheres = world.ordinary_overlap_spheres(id, &data).unwrap();
    assert_eq!(spheres.len(), 2);
    assert_eq!(spheres[0].radius, 24.0);
    assert_eq!(spheres[0].center[0], 10.0);
    assert!((spheres[0].center[1] - (-9.698486)).abs() < 0.00001);
    assert!((spheres[0].center[2] - 59.698486).abs() < 0.00001);
    assert_ne!(spheres[0].center[1], spheres[0].center[1].round());
    data.metadata["actors"][0]["ravc_count"] = 1.into();
    assert!(
        world
            .ordinary_overlap_spheres(id, &data)
            .unwrap_err()
            .to_string()
            .contains("vertex-bound")
    );
}

#[test]
fn ordinary_overlap_weights_preserve_static_pair_and_small_contact_rules() {
    use cyrus_game::script::{OrdinaryOverlapMotion as Motion, ordinary_overlap_motion};
    for (weight, other, distance, expected) in [
        (128, Some(128), 0, Motion::NoContact),
        (0, Some(0), 2, Motion::Touch),
        (0, None, 2, Motion::Rollback),
        (
            128,
            None,
            2,
            Motion::Separate {
                actor: 767,
                other: 0,
            },
        ),
        (0, Some(0), 3, Motion::Rollback),
        (
            128,
            Some(128),
            10,
            Motion::Separate {
                actor: 1535,
                other: 1535,
            },
        ),
        (
            128,
            None,
            10,
            Motion::Separate {
                actor: 2815,
                other: 0,
            },
        ),
        (
            0,
            Some(128),
            10,
            Motion::Separate {
                actor: 0,
                other: 1107,
            },
        ),
        (
            128,
            Some(0),
            10,
            Motion::Separate {
                actor: 1107,
                other: 0,
            },
        ),
        (
            255,
            Some(255),
            10,
            Motion::Separate {
                actor: 1535,
                other: 1535,
            },
        ),
        (
            128,
            None,
            16384,
            Motion::Separate {
                actor: -4194049,
                other: 0,
            },
        ),
    ] {
        assert_eq!(
            ordinary_overlap_motion(weight, other, distance).unwrap(),
            expected,
            "{weight}/{other:?}/{distance}"
        );
    }
    assert!(ordinary_overlap_motion(128, Some(128), -1).is_err());
}

fn overlap_response_fixture(
    weight: u8,
    other_weight: u8,
    offset: i32,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    overlap_response_fixture_with(weight, other_weight, offset, 8)
}

fn overlap_response_fixture_with(
    weight: u8,
    other_weight: u8,
    offset: i32,
    attr_1: u8,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    scripts.definition.attributes = (0..27).map(|index| format!("attr_{index}")).collect();
    scripts.definition.functions.push(Function {
        name: "MoveByAxis".into(),
        param_count: 3,
        kind: CallKind::Task,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables = vec![vec![4]];
    actor.attributes.insert("attr_6".into(), weight);
    actor.attributes.insert("attr_1".into(), attr_1);
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    1,
                    "MoveByAxis",
                    CallKind::Multitask,
                    vec![
                        Value::Integer { value: 0 },
                        Value::Integer {
                            value: offset * 256,
                        },
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
    let mut other = actor.clone();
    other.instance_variables = vec![vec![9]];
    other.attributes.insert("attr_6".into(), other_weight);
    scripts.actors.insert("OTHER".into(), other);
    scripts.instances[1].actor = "OTHER".into();
    let (mut world, data, _) = static_model_fixture(scripts);
    let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
    world.refresh_model_presentations(&data, &ids).unwrap();
    world
        .advance(
            ids[1],
            cyrus_game::script::ScriptStage::Body(Thread::Main),
            cyrus_game::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    (world, data)
}

#[test]
fn ordinary_overlap_response_moves_logical_actors_and_preserves_model_records() {
    use cyrus_game::script::OrdinaryOverlapMotion as Motion;
    for (weight, other_weight, expected_actor, expected_other, actor_distance, other_distance) in [
        (128, 128, 1025, 23039, 1535, 1535),
        (128, 0, 1453, 21504, 1107, 0),
        (0, 128, 2560, 22611, 0, 1107),
    ] {
        let (mut world, data) = overlap_response_fixture(weight, other_weight, 74);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        assert_eq!(
            world
                .resolve_ordinary_actor_overlap(id, other, &data)
                .unwrap(),
            Motion::Separate {
                actor: actor_distance,
                other: other_distance
            }
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [expected_actor, 5120, 7680]
        );
        assert_eq!(
            world.spatial(other).unwrap().local.position,
            [expected_other, 5120, 7680]
        );
        for actor in [id, other] {
            assert_eq!(
                world
                    .model_presentation(actor)
                    .unwrap()
                    .unwrap()
                    .pose
                    .position,
                [2560, 5120, 7680]
            );
            assert_eq!(
                world.actors()[actor.index]
                    .control_state()
                    .unwrap()
                    .body_response_flags,
                4
            );
        }
        assert_eq!(
            world.actors()[id.index]
                .control_state()
                .unwrap()
                .overlap_contact_identifier,
            37
        );
        assert_eq!(
            world.actors()[other.index]
                .control_state()
                .unwrap()
                .overlap_contact_identifier,
            0
        );
    }
}

#[test]
fn ordinary_overlap_touch_ends_pass_and_immovable_pair_rolls_back_without_new_actor_mark() {
    use cyrus_game::script::{OrdinaryOverlapMotion as Motion, ScriptInputs, ScriptStage};
    let (mut world, data) = overlap_response_fixture(0, 0, 83);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    let before = [
        world.spatial(id).unwrap().local.position,
        world.spatial(other).unwrap().local.position,
    ];
    assert_eq!(
        world
            .resolve_ordinary_actor_overlap(id, other, &data)
            .unwrap(),
        Motion::Touch
    );
    assert_eq!(
        [
            world.spatial(id).unwrap().local.position,
            world.spatial(other).unwrap().local.position
        ],
        before
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .body_response_flags,
        4
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .body_response_flags,
        4
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .overlap_contact_identifier,
        37
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .overlap_contact_identifier,
        36
    );
    let (mut world, data) = overlap_response_fixture(0, 0, 74);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world.snapshot_spatial(id).unwrap();
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
    assert_eq!(world.spatial(id).unwrap().local.position[0], 21504);
    assert_eq!(
        world
            .resolve_ordinary_actor_overlap(id, other, &data)
            .unwrap(),
        Motion::Rollback
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 5120, 7680]
    );
    assert_eq!(
        world.spatial(other).unwrap().local.position,
        [21504, 5120, 7680]
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .body_response_flags,
        0
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .body_response_flags,
        4
    );
}

fn static_overlap_scene(world: u32) -> cyrus_game::player::data::PlayerScene {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionScene, StaticPlacement,
    };
    use cyrus_collision::collision_world::StaticCollision;
    let (_, _, models) = static_model_fixture(cache());
    let mut scene = ordinary_collision_scene(world, models);
    scene.collision = CollisionScene {
        sphere_object_scale: Some(256),
        placements: [604, 605]
            .into_iter()
            .enumerate()
            .map(|(id, x)| StaticPlacement {
                id: id as u32,
                model: "SPHERE".into(),
                position: [x, 20, 30],
                rotation: [0; 9],
            })
            .collect(),
        models: BTreeMap::from([(
            "SPHERE".into(),
            CollisionModel {
                flags: 1,
                geometry: CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 42.0,
                    vertices: vec![],
                    normals: vec![],
                    faces: vec![],
                    textures: None,
                    subobjects: vec![],
                },
            },
        )]),
        ..Default::default()
    };
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    scene
}

#[test]
fn ordinary_static_overlap_keeps_reverse_placement_order_and_tiny_response() {
    use cyrus_collision::candidates::{ActorCandidate, CandidateOwner, ModelInstance};
    use cyrus_game::script::{
        OrdinaryOverlapMotion as Motion, OrdinaryOverlapPass as Pass, ScriptInputs, ScriptStage,
    };
    for weight in [128, 0] {
        let (mut world, data) = overlap_response_fixture(weight, 0, 512);
        let id = world.id(0).unwrap();
        world.snapshot_spatial(id).unwrap();
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
        assert_eq!(world.spatial(id).unwrap().local.position[0], 133632);
        let scene = static_overlap_scene(world.world);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
            .unwrap();
        let candidate = ActorCandidate {
            owner: CandidateOwner::Static(1),
            model: Some(ModelInstance::StaticPlacement(1)),
            collision_flags: 1,
        };
        if weight == 128 {
            assert_eq!(
                world
                    .resolve_ordinary_overlaps(id, &scene, &data, &prepared)
                    .unwrap(),
                Pass::Complete {
                    needs_polygon_repair: true
                }
            );
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                [133121, 5120, 7680]
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .body_response_flags,
                4
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .overlap_contact_identifier,
                0
            );
        } else {
            assert_eq!(
                world
                    .resolve_ordinary_static_overlap(id, &scene, &data, &prepared, &candidate)
                    .unwrap(),
                Motion::Rollback
            );
            assert_eq!(
                world.spatial(id).unwrap().local.position,
                [2560, 5120, 7680]
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .body_response_flags,
                0
            );
        }
        let foreign = ActorCandidate {
            owner: CandidateOwner::Static(2),
            model: Some(ModelInstance::StaticPlacement(2)),
            collision_flags: 1,
        };
        assert!(
            world
                .resolve_ordinary_static_overlap(id, &scene, &data, &prepared, &foreign)
                .is_err()
        );
    }
}

#[test]
fn ordinary_overlap_pass_stops_at_first_tiny_pair_before_unsupported_later_shape() {
    use cyrus_assets::{
        animation::{AnimationModel, Animations},
        collision::ActorCollisionModels,
    };
    use cyrus_collision::actor_models::ActorModelCollision;
    use cyrus_game::script::OrdinaryOverlapPass as Pass;
    let mut scripts = cache();
    scripts.definition.attributes = (0..27).map(|index| format!("attr_{index}")).collect();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_6".into(), 128);
    let mut later = scripts.actors["LOGIC"].clone();
    later.instances = 1;
    later.instance_variables = vec![vec![4]];
    scripts.actors.insert("LATER".into(), later);
    let mut placement = scripts.instances[0].clone();
    placement.object_id = 2;
    placement.actor = "LATER".into();
    scripts.instances.push(placement);
    for (placement, x) in scripts.instances.iter_mut().zip([10, 33, 20]) {
        placement.object_type = 1;
        placement.map_position = [x, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": (["LOGIC", "LATER"].map(|name| serde_json::json!({
                "script_name":name, "model_load_path":"animation_table",
                "ravc_count": if name == "LATER" {1} else {0},
                "raan_entries":[{"file_path":"MOVING.3D", "frame_count":1}],
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}))),
            "mpob_objects": scripts.instances.iter().map(|p| serde_json::json!({"script_name":p.actor,"object_type":1})).collect::<Vec<_>>()}),
        models: BTreeMap::from([(
            "MOVING".into(),
            AnimationModel {
                radius: Some(42.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        )]),
        sections: Default::default(),
        unsupported: Default::default(),
    };
    let models = ActorModelCollision::new(&ActorCollisionModels {
        object_count: 3,
        collision_flags: BTreeMap::from([(0, 0), (1, 0), (2, 0)]),
        ..Default::default()
    })
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let ids = [
        world.id(0).unwrap(),
        world.id(1).unwrap(),
        world.id(2).unwrap(),
    ];
    for id in ids {
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        world
            .prepare_ordinary_collision_shape(id, &data, &scene)
            .unwrap();
    }
    world.refresh_model_presentations(&data, &ids).unwrap();
    assert!(world.ordinary_overlap_spheres(ids[2], &data).is_err());
    let first = world
        .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &ids)
        .unwrap();
    assert_eq!(
        world
            .resolve_ordinary_overlaps(ids[0], &scene, &data, &first)
            .unwrap(),
        Pass::Touch
    );
    let reversed = world
        .prepare_ordinary_collision_query(
            ids[0],
            &scene,
            &data,
            [10, 30],
            &[ids[0], ids[2], ids[1]],
        )
        .unwrap();
    assert!(
        world
            .resolve_ordinary_overlaps(ids[0], &scene, &data, &reversed)
            .unwrap_err()
            .to_string()
            .contains("vertex-bound")
    );
}

#[test]
fn ordinary_forward_hit_keeps_rank_ties_and_combines_later_normals_in_order() {
    use cyrus_collision::collision_mesh::IntegerSegmentContact;
    use cyrus_game::script::{OrdinaryForwardHit, ordinary_forward_hit};
    let contacts = [
        IntegerSegmentContact {
            face: 0,
            rank: 0x8002,
            point: [10, 20, 30],
            normal: [256, 0, 0],
        },
        IntegerSegmentContact {
            face: 1,
            rank: 2,
            point: [40, 50, 60],
            normal: [256, 0, 256],
        },
        IntegerSegmentContact {
            face: 2,
            rank: 5,
            point: [70, 80, 90],
            normal: [256, 0, -256],
        },
        IntegerSegmentContact {
            face: 3,
            rank: 0,
            point: [100, 110, 120],
            normal: [-256, 0, 0],
        },
    ];
    assert_eq!(
        ordinary_forward_hit(512, &contacts),
        Some(OrdinaryForwardHit {
            point: [10, 20, 30],
            normal: [245, 0, -73],
        })
    );
    let reordered = [
        contacts[1].clone(),
        contacts[2].clone(),
        contacts[0].clone(),
    ];
    assert_eq!(
        ordinary_forward_hit(512, &reordered),
        Some(OrdinaryForwardHit {
            point: [40, 50, 60],
            normal: [256, 0, 0],
        })
    );
}

#[test]
fn ordinary_forward_facing_retains_strict_boundary_and_2047_wrap() {
    use cyrus_collision::collision_mesh::IntegerSegmentContact;
    use cyrus_game::script::ordinary_forward_hit;
    let contact = IntegerSegmentContact {
        face: 0,
        rank: 1,
        point: [0; 3],
        normal: [256, 0, 0],
    };
    for (heading, hit) in [(0, false), (1, true), (1023, true), (1024, false)] {
        assert_eq!(
            ordinary_forward_hit(heading, std::slice::from_ref(&contact)).is_some(),
            hit
        );
    }
    assert!(
        ordinary_forward_hit(
            2047,
            &[IntegerSegmentContact {
                normal: [0, 0, -256],
                ..contact
            }]
        )
        .is_some()
    );
}

fn ordinary_forward_wall_model() -> cyrus_assets::collision::CollisionModel {
    use cyrus_assets::collision::{CollisionGeometry, CollisionModel, CollisionSubobject};
    CollisionModel {
        flags: 0,
        geometry: CollisionGeometry {
            version: Some(*b"v5.0"),
            radius: 1000.0,
            vertices: vec![
                [0.0, -100.0, -100.0],
                [0.0, -100.0, 100.0],
                [0.0, 100.0, 100.0],
                [0.0, 100.0, -100.0],
            ],
            normals: vec![[-1.0, 0.0, 0.0]],
            faces: vec![vec![0, 1, 2, 3]],
            textures: None,
            subobjects: vec![CollisionSubobject {
                center: [0.0; 3],
                radius: 1000.0,
                extents: Some([1.0; 3]),
                faces: vec![0],
            }],
        },
    }
}

#[test]
fn ordinary_forward_obstacle_uses_effective_motion_and_preserves_y_and_model_pose() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::script::{OrdinaryObstacleQuery, ScriptInputs, ScriptStage};
    for (offset, respond, expected_hit, expected_x, wrapper) in [
        (0, false, true, 264704, false),
        (0, true, true, 143872, false),
        (500, true, false, 264704, false),
        (0, true, true, 143872, true),
    ] {
        let (mut world, data) = overlap_response_fixture(128, 0, 1024);
        let id = world.id(0).unwrap();
        world.snapshot_spatial(id).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: vec![StaticPlacement {
                id: 1,
                model: "WALL".into(),
                position: [604, 20, 30],
                rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
            }],
            models: BTreeMap::from([("WALL".into(), ordinary_forward_wall_model())]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
            .unwrap();
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
        if wrapper {
            world
                .resolve_ordinary_obstacle_wrapper(
                    id,
                    OrdinaryObstacleQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        vertical_offset: offset,
                        respond,
                    },
                    cyrus_game::script::ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 0,
                    },
                    &mut |_, _| Ok(()),
                )
                .unwrap();
        } else {
            assert_eq!(
                world
                    .resolve_ordinary_forward_obstacle(
                        id,
                        OrdinaryObstacleQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            vertical_offset: offset,
                            respond,
                        }
                    )
                    .unwrap(),
                expected_hit
            );
        }
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [expected_x, 5120, 7680]
        );
        assert_eq!(
            world.model_presentation(id).unwrap().unwrap().pose.position,
            [2560, 5120, 7680]
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .body_response_flags,
            if wrapper { 8 } else { 0 }
        );
    }
}

#[test]
fn ordinary_bounds_endpoints_keep_stored_negative_magnitudes_and_positive_y() {
    use cyrus_game::script::{OrdinaryCollisionShape, ordinary_bounds_segments};
    let shape = OrdinaryCollisionShape {
        positive_extent: [10, 20, 30],
        negative_extent: [40, 50, 60],
        body: vec![],
        additional: None,
    };
    assert_eq!(
        ordinary_bounds_segments(
            &shape,
            cyrus_world::transform::Matrix::orientation([0; 3]),
            [256, 512, 768]
        )
        .unwrap(),
        [[[11, 24, 33], [41, 20, 63]], [[41, 24, 33], [11, 20, 63]],]
    );
}

#[test]
fn ordinary_bounds_separation_keeps_independent_rank_limits_and_earlier_contributions() {
    use cyrus_collision::collision_mesh::IntegerSegmentContact;
    use cyrus_game::script::{
        BoundsSegment::{First, Second},
        ordinary_bounds_separation,
    };
    let contact = |rank: i32, normal| IntegerSegmentContact {
        face: 0,
        rank: rank as u32,
        point: [0; 3],
        normal,
    };
    assert_eq!(
        ordinary_bounds_separation(&[
            (First, contact(10, [100, -41, 0])),
            (First, contact(5, [0, 160, 100])),
            (First, contact(7, [999, 0, 0])),
            (First, contact(5, [999, 0, 0])),
            (Second, contact(5, [200, 0, 0])),
            (First, contact(0, [999, 0, 0])),
            (First, contact(-999999, [999, 0, 0])),
            (First, contact(-10, [0, 0, 200])),
            (First, contact(-20, [999, 0, 0])),
            (First, contact(-5, [5, 0, 0])),
            (First, contact(1, [999, -42, 0])),
            (First, contact(1, [999, 161, 0])),
        ]),
        [305, 119, 300]
    );
}

#[test]
fn ordinary_bounds_query_commits_raw_normals_to_all_axes_and_marks_only_nonzero_sum() {
    use cyrus_assets::collision::{
        CollisionGeometry, CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement,
    };
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::script::{OrdinaryObstacleQuery, ScriptInputs, ScriptStage};
    for (normal_scale, expected_position, expected_flags) in [
        (0.25, [133632, 5248, 7936], 0x40),
        (0.75, [133632, 5120, 7680], 0),
    ] {
        let (mut world, data) = overlap_response_fixture(128, 0, 512);
        let id = world.id(0).unwrap();
        world.snapshot_spatial(id).unwrap();
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
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: (0..2)
                .map(|id| StaticPlacement {
                    id,
                    model: "SLOPE".into(),
                    position: [542, 20, 50],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .collect(),
            models: BTreeMap::from([(
                "SLOPE".into(),
                CollisionModel {
                    flags: 0,
                    geometry: CollisionGeometry {
                        version: Some(*b"v5.0"),
                        radius: 1000.0,
                        vertices: vec![
                            [-100.0, -200.0, 100.0],
                            [100.0, -200.0, 100.0],
                            [100.0, 200.0, -100.0],
                            [-100.0, 200.0, -100.0],
                        ],
                        normals: vec![[0.0, normal_scale, normal_scale * 2.0]],
                        faces: vec![vec![0, 1, 2, 3]],
                        textures: None,
                        subobjects: vec![CollisionSubobject {
                            center: [0.0; 3],
                            radius: 1000.0,
                            extents: Some([1.0; 3]),
                            faces: vec![0],
                        }],
                    },
                },
            )]),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
            .unwrap();
        world
            .resolve_ordinary_bounds_obstacle(
                id,
                OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 9999,
                    respond: false,
                },
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected_position);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .body_response_flags,
            expected_flags
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .collision_transition_flags,
            0
        );
        assert_eq!(
            world.model_presentation(id).unwrap().unwrap().pose.position,
            [2560, 5120, 7680]
        );
    }
}

#[test]
fn ordinary_obstacle_transition_preserves_grounded_and_state_fifteen_and_restarts_falls() {
    use cyrus_game::script::{
        ObstacleFallStart, ObstacleTransition, ObstacleTransitionInput,
        ordinary_obstacle_transition,
    };
    let base = ObstacleTransitionInput {
        living_state: 0,
        falling_state: 0,
        queued_motion: 0,
        logical_y: 2560,
        fall_attribute: 7,
        holding_weapon: false,
        current_group: None,
    };
    assert_eq!(
        ordinary_obstacle_transition(base),
        Some(ObstacleTransition {
            living_state: 0,
            falling_state: 0,
            queued_motion: 0,
            body_response_flags: 0,
            clear_motion_vector: false,
            fall_start: None,
        })
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: 15,
            falling_state: 0x88,
            queued_motion: 24,
            ..base
        }),
        None
    );
    for living in 27..=32 {
        assert_eq!(
            ordinary_obstacle_transition(ObstacleTransitionInput {
                living_state: living,
                ..base
            }),
            Some(ObstacleTransition {
                living_state: 0,
                falling_state: 1,
                queued_motion: 0,
                body_response_flags: 0x10,
                clear_motion_vector: true,
                fall_start: Some(ObstacleFallStart {
                    origin_y: 4352,
                    timer: 30,
                    force_group: Some(56)
                })
            })
        );
    }
    for (queued, holding, current, falling, group, clear) in [
        (24, false, None, 1, Some(56), true),
        (5, false, None, 1, Some(56), false),
        (0x85, false, None, 0x81, Some(58), false),
        (5, true, None, 1, Some(113), false),
        (5, true, Some(113), 1, None, false),
    ] {
        assert_eq!(
            ordinary_obstacle_transition(ObstacleTransitionInput {
                queued_motion: queued,
                holding_weapon: holding,
                current_group: current,
                ..base
            }),
            Some(ObstacleTransition {
                living_state: 0,
                falling_state: falling,
                queued_motion: 0,
                body_response_flags: 0x10,
                clear_motion_vector: clear,
                fall_start: Some(ObstacleFallStart {
                    origin_y: 4352,
                    timer: 30,
                    force_group: group
                }),
            })
        );
    }
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            falling_state: 0x8f,
            ..base
        }),
        Some(ObstacleTransition {
            living_state: 0,
            falling_state: 0x81,
            queued_motion: 0,
            body_response_flags: 0x20,
            clear_motion_vector: false,
            fall_start: None
        })
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: 26,
            ..base
        })
        .unwrap()
        .living_state,
        26
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            living_state: 33,
            ..base
        })
        .unwrap()
        .living_state,
        33
    );
    assert_eq!(
        ordinary_obstacle_transition(ObstacleTransitionInput {
            queued_motion: 1,
            logical_y: i32::MAX,
            fall_attribute: 255,
            ..base
        })
        .unwrap()
        .fall_start
        .unwrap()
        .origin_y,
        -2147418369
    );
}

#[test]
fn ordinary_overlap_stage_repeats_obstacle_wrapper_after_weighted_static_separation() {
    use cyrus_assets::collision::StaticPlacement;
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::script::{
        ObstacleEnvironment, OrdinaryObstacleQuery, ScriptInputs, ScriptStage,
    };
    let (mut world, data) = overlap_response_fixture(128, 0, 512);
    let id = world.id(0).unwrap();
    world.snapshot_spatial(id).unwrap();
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
    let mut scene = static_overlap_scene(world.world);
    scene
        .collision
        .models
        .insert("WALL".into(), ordinary_forward_wall_model());
    scene.collision.placements.push(StaticPlacement {
        id: 2,
        model: "WALL".into(),
        position: [512, 20, 30],
        rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
    });
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [522, 30], &[id])
        .unwrap();
    world
        .resolve_ordinary_overlap_stage(
            id,
            OrdinaryObstacleQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                vertical_offset: 9999,
                respond: false,
            },
            ObstacleEnvironment {
                selected_player: None,
                auto_grab: false,
                item_system_ready: false,
                shell_execution: false,
                ticks: 0,
            },
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [120320, 5120, 7680]
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .body_response_flags,
        12
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .collision_transition_flags,
        0
    );
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        [2560, 5120, 7680]
    );
}

#[test]
fn ordinary_ground_prevalidation_preserves_slope_direction_and_animation_exclusions() {
    use cyrus_collision::ground::GroundSupport;
    use cyrus_game::script::{OrdinaryGroundPrevalidation, ordinary_ground_prevalidation};
    let flat = GroundSupport {
        height: 0.0,
        normal: [0, -256, 0],
    };
    let steep = GroundSupport {
        height: 0.0,
        normal: [0, -100, 256],
    };
    let clear = OrdinaryGroundPrevalidation {
        rollback: false,
        sliding: false,
    };
    let slide = OrdinaryGroundPrevalidation {
        rollback: false,
        sliding: true,
    };
    let veto = OrdinaryGroundPrevalidation {
        rollback: true,
        sliding: false,
    };
    assert_eq!(
        ordinary_ground_prevalidation(None, None, [0; 3], 0, false).unwrap(),
        clear
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(flat), Some(flat), [0; 3], 0, false).unwrap(),
        clear
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(steep), [0; 3], 1, true).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [-256, 0, 0], 0, false).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [256, 0, 0], 0, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, 256], 0, false).unwrap(),
        slide
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, -256], 0, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, 256], 1, false).unwrap(),
        veto
    );
    assert_eq!(
        ordinary_ground_prevalidation(Some(steep), Some(flat), [0, 0, 256], 0, true).unwrap(),
        veto
    );
    assert!(ordinary_ground_prevalidation(Some(steep), None, [0; 3], 0, false).is_err());
    assert!(ordinary_ground_prevalidation(None, Some(flat), [0; 3], 0, false).is_err());
}

#[test]
fn ordinary_ground_prevalidation_queries_retained_support_and_restores_local_motion() {
    use cyrus_game::script::{
        ObstacleEnvironment, OrdinaryGroundPrevalidation, OrdinaryGroundQuery, ScriptInputs,
        ScriptStage,
    };
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_1".into(), 2);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_32".into(), 1);
    let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    world.snapshot_spatial(id).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 1,
        sidedness: false,
    };
    assert_eq!(
        world
            .query_ordinary_ground_prevalidation(id, query(), false)
            .unwrap(),
        OrdinaryGroundPrevalidation::default()
    );
    assert_eq!(
        world
            .query_ordinary_ground_prevalidation(id, query(), true)
            .unwrap(),
        OrdinaryGroundPrevalidation::default()
    );

    world
        .apply_ordinary_falling_vertical(
            other,
            cyrus_game::script::OrdinaryVerticalInputs::for_falling(0, 14, 0, 50, false).unwrap(),
        )
        .unwrap();
    world.refresh_model_presentations(&data, &[other]).unwrap();
    assert_eq!(
        world.query_ordinary_queued_clearance(id, query()).unwrap(),
        256
    );
    let empty = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
        .unwrap();
    assert_eq!(
        world
            .query_ordinary_queued_clearance(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &empty,
                    distance: 1,
                    sidedness: false,
                }
            )
            .unwrap(),
        256000
    );

    let (mut world, data) = overlap_response_fixture(128, 0, 1024);
    let id = world.id(0).unwrap();
    world.snapshot_spatial(id).unwrap();
    let before = world.spatial(id).unwrap().local.position;
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
    assert_ne!(world.spatial(id).unwrap().local.position, before);
    world
        .apply_ordinary_ground_prevalidation(
            id,
            OrdinaryGroundPrevalidation {
                rollback: true,
                sliding: false,
            },
            &data,
            ObstacleEnvironment {
                selected_player: None,
                auto_grab: false,
                item_system_ready: false,
                shell_execution: false,
                ticks: 0,
            },
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .motion_direction,
        0
    );
}

#[test]
fn ordinary_vertical_motion_retains_byte_timer_signed_limit_and_q8_origin() {
    use cyrus_game::script::{
        OrdinaryVerticalInputs, OrdinaryVerticalState, ordinary_falling_vertical,
        ordinary_queued_vertical,
    };
    let input = OrdinaryVerticalInputs {
        factor: 256,
        advanced_factor: 256,
        fine_ticks: 8,
        additional_ticks: 0,
        post_collide_height: 50,
        model_controls_vertical: false,
    };
    let state = OrdinaryVerticalState {
        y: 9000,
        origin_y: 10000,
        timer: 30,
        queued: 129,
        falling: 0,
    };
    assert_eq!(
        ordinary_queued_vertical(state, 32, input),
        OrdinaryVerticalState {
            y: 1808,
            timer: 38,
            queued: 0,
            falling: 129,
            ..state
        }
    );
    assert_eq!(
        ordinary_falling_vertical(state, 32, input),
        OrdinaryVerticalState {
            y: 1808,
            timer: 22,
            ..state
        }
    );
    assert_eq!(
        ordinary_falling_vertical(
            OrdinaryVerticalState { timer: 0, ..state },
            32,
            OrdinaryVerticalInputs {
                factor: 16,
                ..input
            }
        ),
        OrdinaryVerticalState {
            y: 14120,
            timer: 0,
            ..state
        }
    );
    assert_eq!(
        ordinary_falling_vertical(
            OrdinaryVerticalState {
                timer: 128,
                ..state
            },
            255,
            OrdinaryVerticalInputs {
                factor: 16,
                ..input
            }
        )
        .y,
        34344
    );
    assert_eq!(
        ordinary_falling_vertical(
            state,
            32,
            OrdinaryVerticalInputs {
                model_controls_vertical: true,
                ..input
            }
        ),
        state
    );
    assert_eq!(
        ordinary_queued_vertical(
            state,
            32,
            OrdinaryVerticalInputs {
                model_controls_vertical: true,
                ..input
            }
        ),
        state
    );
    assert_eq!(
        ordinary_queued_vertical(
            OrdinaryVerticalState {
                timer: 250,
                ..state
            },
            32,
            OrdinaryVerticalInputs {
                advanced_factor: 255,
                ..input
            }
        )
        .timer,
        2
    );
    assert_eq!(
        ordinary_queued_vertical(
            state,
            32,
            OrdinaryVerticalInputs {
                advanced_factor: 255,
                ..input
            }
        )
        .queued,
        129
    );
    assert_eq!(
        ordinary_falling_vertical(OrdinaryVerticalState { timer: 1, ..state }, 32, input).timer,
        249
    );
    assert_eq!(
        ordinary_falling_vertical(
            OrdinaryVerticalState {
                timer: 0,
                y: i32::MAX,
                ..state
            },
            1,
            OrdinaryVerticalInputs {
                factor: 16,
                fine_ticks: 0,
                ..input
            }
        )
        .y,
        i32::MIN + 31
    );
}

#[test]
fn ordinary_vertical_writers_commit_logical_y_without_moving_model_record() {
    use cyrus_game::script::OrdinaryVerticalInputs;
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_32".into(), 32);
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let before = world.spatial(id).unwrap().local.position;
    let record = world.model_presentation(id).unwrap().unwrap().pose.position;
    let input = OrdinaryVerticalInputs {
        factor: 16,
        advanced_factor: 32,
        fine_ticks: 8,
        additional_ticks: 0,
        post_collide_height: 50,
        model_controls_vertical: false,
    };
    world.apply_ordinary_falling_vertical(id, input).unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [before[0], before[1] + 5120, before[2]]
    );
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        record
    );
    world
        .apply_ordinary_queued_vertical(id, input, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [before[0], -512, before[2]]
    );
    assert_eq!(
        world.actors()[id.index].control_state().unwrap().fall_timer,
        8
    );
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        record
    );
}

#[test]
fn ordinary_vertical_factor_resolution_obeys_signed_timer_and_read_order() {
    use cyrus_game::script::{OrdinaryVerticalInputs, ordinary_vertical_factor};
    assert_eq!(ordinary_vertical_factor(0).unwrap(), 16);
    assert_eq!(ordinary_vertical_factor(3).unwrap(), 63);
    assert_eq!(ordinary_vertical_factor(14).unwrap(), 192);
    assert_eq!(ordinary_vertical_factor(28).unwrap(), 255);
    for index in 29..70 {
        assert_eq!(ordinary_vertical_factor(index).unwrap(), 256);
    }
    assert!(ordinary_vertical_factor(-1).is_err());
    assert!(ordinary_vertical_factor(70).is_err());
    let queued = OrdinaryVerticalInputs::for_queued(24, 8, 0, 50, false).unwrap();
    assert_eq!((queued.factor, queued.advanced_factor), (248, 256));
    let falling = OrdinaryVerticalInputs::for_falling(128, 8, 0, 50, false).unwrap();
    assert_eq!(falling.factor, 16);
    assert_eq!(
        OrdinaryVerticalInputs::for_falling(30, 8, 0, 50, false)
            .unwrap()
            .factor,
        256
    );
    assert!(OrdinaryVerticalInputs::for_queued(128, 8, 0, 50, false).is_err());
    assert!(OrdinaryVerticalInputs::for_queued(64, 8, 0, 50, false).is_err());
    assert!(OrdinaryVerticalInputs::for_queued(128, 8, 0, 50, true).is_ok());
    assert!(OrdinaryVerticalInputs::for_falling(70, 8, 0, 50, true).is_ok());
}

#[test]
fn ordinary_queued_clearance_uses_strict_terrain_height_and_retained_model_order() {
    use cyrus_collision::ground::TerrainGround;
    use cyrus_game::script::OrdinaryGroundQuery;
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
        .unwrap();
    for (height, expected) in [(22, 512), (20, 256000), (19, 256000), (80, 15360)] {
        scene.terrain = Some(
            TerrainGround::new(vec![height; 256 * 256], vec![false; 256 * 256], None).unwrap(),
        );
        assert_eq!(
            world
                .query_ordinary_queued_clearance(
                    id,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    }
                )
                .unwrap(),
            expected
        );
    }
    scene.terrain =
        Some(TerrainGround::new(vec![22; 256 * 256], vec![true; 256 * 256], None).unwrap());
    assert!(
        world
            .query_ordinary_queued_clearance(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 1,
                    sidedness: false,
                }
            )
            .unwrap_err()
            .to_string()
            .contains("water/grid boundary unsupported")
    );
}

#[test]
fn ordinary_queued_arbitration_preserves_group_and_pending_precedence() {
    use cyrus_game::script::{OrdinaryQueuedAction, OrdinaryQueuedInputs, ordinary_queued_action};
    let input = OrdinaryQueuedInputs {
        queued: 0,
        torch_burning: 0,
        current_group: Some(0),
        pending_group: None,
        has_group_49: true,
        has_group_50: true,
        holding_weapon: false,
        edge_handled: false,
    };
    assert_eq!(
        ordinary_queued_action(input),
        OrdinaryQueuedAction::default()
    );
    for (queued, torch_burning, holding, group) in [
        (1, 0, false, 39),
        (1, 0, true, 112),
        (129, 0, true, 40),
        (129, 1, false, 38),
        (2, 0, false, 42),
        (130, 0, false, 43),
        (130, 1, false, 41),
        (3, 0, false, 49),
        (131, 1, false, 50),
        (8, 0, false, 44),
        (136, 0, false, 48),
        (136, 1, false, 45),
        (24, 0, false, 44),
        (152, 0, false, 48),
        (152, 1, false, 45),
        (9, 0, false, 46),
        (137, 0, false, 48),
        (137, 1, false, 47),
    ] {
        assert_eq!(
            ordinary_queued_action(OrdinaryQueuedInputs {
                queued,
                torch_burning,
                holding_weapon: holding,
                current_group: Some(if torch_burning == 0 { 49 } else { 50 }),
                ..input
            }),
            OrdinaryQueuedAction {
                clear_queued: false,
                handled: true,
                advance_vertical: true,
                request_group: Some(group)
            }
        );
    }
    for pending in [32, 33, 34, 35, 36, 37, 111] {
        assert_eq!(
            ordinary_queued_action(OrdinaryQueuedInputs {
                queued: 1,
                pending_group: Some(pending),
                edge_handled: true,
                ..input
            }),
            OrdinaryQueuedAction::default()
        );
    }
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: 3,
            has_group_49: false,
            pending_group: Some(32),
            edge_handled: true,
            ..input
        }),
        OrdinaryQueuedAction {
            clear_queued: true,
            ..Default::default()
        }
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs { queued: 3, ..input }),
        OrdinaryQueuedAction::default()
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: 3,
            torch_burning: 1,
            ..input
        }),
        OrdinaryQueuedAction {
            handled: true,
            ..Default::default()
        }
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: 1,
            edge_handled: true,
            ..input
        }),
        OrdinaryQueuedAction {
            clear_queued: true,
            handled: true,
            ..Default::default()
        }
    );
    assert_eq!(
        ordinary_queued_action(OrdinaryQueuedInputs {
            queued: 128,
            ..input
        }),
        OrdinaryQueuedAction {
            handled: true,
            advance_vertical: true,
            ..Default::default()
        }
    );
}

#[test]
fn ordinary_jump_entry_drives_queued_motion_to_falling_through_shared_animation() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_32".into(), 32);
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups": ([0, 32, 39, 56, 58].into_iter().map(|group| serde_json::json!({
                    "group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":group},{"opcode":1}]
                })).collect::<Vec<_>>())}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.snapshot_spatial(id).unwrap();
    let origin = world.spatial(id).unwrap().local.position;
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    assert!(world.retain_ordinary_stable_position(id).unwrap());
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_local_position,
        origin
    );
    assert!(
        !world
            .begin_ordinary_jump(id, &data, 0, true, environment, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .queued_motion,
        0
    );
    assert!(
        world
            .begin_ordinary_jump(id, &data, 0, false, environment, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert!(!world.retain_ordinary_stable_position(id).unwrap());
    assert!(
        !world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_position_valid
    );
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .stable_local_position,
        origin
    );
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.queued_motion,
            control.fall_timer,
            control.fall_origin_y,
            control.motion_direction
        ),
        (1, 0, origin[1], 2)
    );
    assert!(
        !world
            .begin_ordinary_jump(id, &data, 0, false, environment, &mut |_, _| Ok(()))
            .unwrap()
    );
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 50,
        sidedness: true,
    };
    // Replacing the current default starts the requested group immediately.
    assert_eq!(world.animation_frame(id).unwrap(), Some(32));
    for (timer, offset, queued, falling) in [
        (8, -512, 1, 0),
        (16, -4160, 1, 0),
        (24, -6624, 1, 0),
        (32, -7936, 0, 1),
    ] {
        assert!(
            world
                .resolve_ordinary_queued_motion(id, query(), environment, 8, 0, &mut |_, _| Ok(()))
                .unwrap()
        );
        let control = world.actors()[id.index].control_state().unwrap();
        assert_eq!(
            (
                control.fall_timer,
                control.queued_motion,
                control.falling_state
            ),
            (timer, queued, falling)
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [origin[0], origin[1] + offset, origin[2]]
        );
    }
    assert!(
        world
            .continue_ordinary_falling(id, &data, environment, 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(32));
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!((control.fall_timer, control.falling_state), (24, 1));
    assert_eq!(
        world.spatial(id).unwrap().local.position[1],
        origin[1] - 8192
    );
}

#[test]
fn ordinary_falling_requests_preserve_priority_and_short_distance_boundary() {
    use cyrus_game::script::{OrdinaryFallingRequest, ordinary_falling_request};
    let requested = |group| {
        Some(OrdinaryFallingRequest {
            group,
            force: false,
        })
    };
    assert_eq!(ordinary_falling_request(0, 0, false, None, 0, 0), None);
    assert_eq!(
        ordinary_falling_request(131, 0, true, Some(115), 0, 0),
        requested(63)
    );
    assert_eq!(
        ordinary_falling_request(131, 1, true, Some(115), 0, 0),
        requested(64)
    );
    assert_eq!(
        ordinary_falling_request(136, 0, true, None, 0, 0),
        requested(58)
    );
    assert_eq!(
        ordinary_falling_request(8, 0, true, None, 32767, 0),
        requested(59)
    );
    assert_eq!(
        ordinary_falling_request(9, 1, false, None, -32767, 0),
        requested(62)
    );
    assert_eq!(
        ordinary_falling_request(8, 0, false, None, 32768, 0),
        requested(56)
    );
    assert_eq!(
        ordinary_falling_request(9, 1, false, None, -32768, 0),
        requested(57)
    );
    assert_eq!(ordinary_falling_request(1, 0, true, Some(115), 0, 0), None);
    assert_eq!(
        ordinary_falling_request(1, 0, true, Some(114), 0, 0),
        Some(OrdinaryFallingRequest {
            group: 113,
            force: true
        })
    );
}

#[test]
fn ordinary_static_support_miss_initializes_once_and_continues_vertical_motion() {
    use cyrus_game::script::ObstacleEnvironment;
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_32".into(), 32);
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let y = world.spatial(id).unwrap().local.position[1];
    assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.fall_origin_y,
            control.fall_timer,
            control.falling_state
        ),
        (y.wrapping_add(8192), 30, 1)
    );
    assert!(!world.begin_static_ordinary_fall(id, &data).unwrap());
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    assert!(
        world
            .continue_ordinary_falling(id, &data, environment, 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position[1], y);
    assert_eq!(
        world.actors()[id.index].control_state().unwrap().fall_timer,
        22
    );
    assert_eq!(world.animation_frame(id).unwrap(), None);
}

#[test]
fn ordinary_animated_fall_entry_distinguishes_rollback_from_start() {
    use cyrus_game::script::{OrdinaryFallEntry, ordinary_fall_entry};
    for group in [18, 30] {
        assert_eq!(
            ordinary_fall_entry(group, 7, true, 1, true),
            OrdinaryFallEntry::RestorePrevious { force_group: 7 }
        );
        assert_eq!(
            ordinary_fall_entry(group, 7, false, 0, true),
            OrdinaryFallEntry::Start {
                force_group: Some(59)
            }
        );
        assert_eq!(
            ordinary_fall_entry(group, 7, false, 1, false),
            OrdinaryFallEntry::Start {
                force_group: Some(60)
            }
        );
    }
    assert_eq!(
        ordinary_fall_entry(0, 0, true, 0, false),
        OrdinaryFallEntry::Start {
            force_group: Some(56)
        }
    );
    assert_eq!(
        ordinary_fall_entry(0, 0, true, 1, false),
        OrdinaryFallEntry::Start {
            force_group: Some(57)
        }
    );
    assert_eq!(
        ordinary_fall_entry(114, 0, false, 1, true),
        OrdinaryFallEntry::Start {
            force_group: Some(113)
        }
    );
    assert_eq!(
        ordinary_fall_entry(115, 0, false, 0, true),
        OrdinaryFallEntry::Start { force_group: None }
    );
}

#[test]
fn ordinary_animated_fall_entry_commits_before_forced_request() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::ObstacleEnvironment;
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_32".into(), 32);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_3".into(), 100);
    scripts.content_id = scripts.content_identity().unwrap();
    let data = Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "animation_groups":([0,56,65,73,74,134].into_iter().map(|group| serde_json::json!({
                    "group_index":group,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":group},{"opcode":1}]
                })).collect::<Vec<_>>())}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world
            .apply_ordinary_directional_motion(id, 1, 4096)
            .unwrap(),
        None
    );
    world.set_actor_surface(id, 2).unwrap();
    let before = world.spatial(id).unwrap().local;
    assert_eq!(
        world
            .apply_ordinary_directional_motion(id, 2, 4096)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        cyrus_world::transform::translate_forward(before.position, before.angles[1], 2048).unwrap()
    );
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!((control.motion_direction, control.movement_flags), (2, 48));
    let y = world.spatial(id).unwrap().local.position[1];
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    assert!(
        world
            .complete_ordinary_animated_fall_entry(id, &data, environment, &mut |_, _| Ok(()))
            .unwrap()
    );
    let control = world.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.falling_state,
            control.fall_timer,
            control.fall_origin_y
        ),
        (1, 30, y.wrapping_add(8192))
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(56));
    assert!(
        world
            .complete_ordinary_animated_fall_entry(id, &data, environment, &mut |_, _| Ok(()))
            .unwrap_err()
            .to_string()
            .contains("already falling")
    );
    assert!(
        world
            .change_ordinary_health(id, -10, &data, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.actors()[id.index].attributes()[3], 90);
    world.set_actor_surface(id, 0).unwrap();
    let decision = world
        .resolve_ordinary_landing_damage(
            id,
            &data,
            environment,
            cyrus_game::script::OrdinaryLandingSettings {
                death_height: 100,
                hurt_height: 1,
                hurt_zap: 10,
            },
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        decision,
        cyrus_game::script::OrdinaryLandingDecision::Hurt { health_change: -10 }
    );
    assert_eq!(world.actors()[id.index].attributes()[3], 80);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .falling_state,
        0
    );
    world
        .complete_ordinary_landing(id, &data, environment, None, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .falling_state,
        0
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(56));
    assert!(
        world
            .change_ordinary_health(id, -256, &data, environment, None, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.actors()[id.index].attributes()[3], 0);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .death_response,
        8
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(134));
}

#[test]
fn ordinary_directional_motion_retains_flags_when_animation_suppresses_translation() {
    use cyrus_game::script::{OrdinaryDirectionalMotion, ordinary_directional_motion};
    for (motion, direction, bits, heading) in [
        (2, 2, 48, 2000),
        (6, 2, 48, 2000),
        (8, 2, 48, 2000),
        (9, 2, 48, 2000),
        (24, 2, 48, 2000),
        (3, 3, 192, 976),
        (7, 3, 192, 976),
        (4, 4, 3, 464),
        (5, 5, 12, 1488),
    ] {
        assert_eq!(
            ordinary_directional_motion(motion | 128, 2000, 4096, 2, false),
            Some(OrdinaryDirectionalMotion {
                direction,
                movement_bits: bits,
                heading,
                distance: Some(2048)
            })
        );
        assert_eq!(
            ordinary_directional_motion(motion, 2000, 4096, 0, true),
            Some(OrdinaryDirectionalMotion {
                direction,
                movement_bits: bits,
                heading,
                distance: None
            })
        );
    }
    for motion in [0, 1, 10, 23, 25, 127] {
        assert_eq!(ordinary_directional_motion(motion, 0, 4096, 0, false), None);
    }
    assert_eq!(
        ordinary_directional_motion(2, 0, -1, 4, false)
            .unwrap()
            .distance,
        None
    );
    assert_eq!(
        ordinary_directional_motion(2, 0, 4097, 4, false)
            .unwrap()
            .distance,
        Some(2048)
    );
    assert_eq!(
        ordinary_directional_motion(2, 0, 4097, 3, false)
            .unwrap()
            .distance,
        Some(4097)
    );
}

#[test]
fn ordinary_landing_decision_uses_strict_installed_thresholds_and_water() {
    use cyrus_game::script::{
        OrdinaryLandingDecision as D, OrdinaryLandingSettings, ordinary_landing_decision,
    };
    let settings = OrdinaryLandingSettings {
        death_height: 600,
        hurt_height: 300,
        hurt_zap: 10,
    };
    let decide = |distance, surface| ordinary_landing_decision(distance, 0, surface, settings);
    assert_eq!(decide(300 * 256, 0), D::Ordinary);
    assert_eq!(decide(300 * 256 + 1, 0), D::Hurt { health_change: -10 });
    assert_eq!(decide(-(600 * 256), 0), D::Hurt { health_change: -10 });
    assert_eq!(decide(-(600 * 256 + 1), 0), D::Fatal);
    assert_eq!(decide(300 * 256 + 1, 11), D::HurtExcluded);
    assert_eq!(decide(300 * 256 + 1, 5), D::HurtExcluded);
    for surface in 1..=4 {
        assert_eq!(decide(600 * 256 + 1, surface), D::Ordinary);
    }
    assert_eq!(decide(600 * 256 + 1, 5), D::Fatal);
    assert_eq!(
        ordinary_landing_decision(i32::MIN, i32::MAX, 0, settings),
        D::Ordinary
    );
}

#[test]
fn ordinary_landing_requests_keep_selected_player_weapon_state_separate() {
    use cyrus_game::script::ordinary_landing_request as request;
    assert_eq!(request(0, 0, false, None, None).unwrap(), None);
    for (motion, dry, torch, armed) in [(8, 67, 68, 130), (9, 69, 70, 131)] {
        assert_eq!(
            request(motion | 128, 0, true, Some(false), None).unwrap(),
            Some(dry)
        );
        assert_eq!(
            request(motion, 1, false, Some(false), None).unwrap(),
            Some(torch)
        );
        assert_eq!(
            request(motion, 0, false, Some(true), None).unwrap(),
            Some(armed)
        );
        assert!(
            request(motion, 0, false, None, None)
                .unwrap_err()
                .to_string()
                .contains("selected-player")
        );
    }
    assert_eq!(request(1, 0, false, None, None).unwrap(), Some(65));
    assert_eq!(request(1, 1, false, None, None).unwrap(), Some(66));
    assert_eq!(request(1, 0, true, None, Some(114)).unwrap(), Some(114));
    assert_eq!(request(1, 0, true, None, Some(115)).unwrap(), None);
    assert_eq!(request(1, 0, true, None, Some(116)).unwrap(), None);
}

#[test]
fn ordinary_health_value_wraps_before_signed_clamping() {
    use cyrus_game::script::ordinary_health_value as health;
    assert_eq!(health(100, -10), 90);
    assert_eq!(health(100, -256), 0);
    assert_eq!(health(250, 20), 255);
    assert_eq!(health(1, i32::MAX), 0);
    assert_eq!(health(0, i32::MIN), 0);
}

#[test]
fn ordinary_stable_probe_keeps_inclusive_terrain_and_unranked_model_assignment() {
    use cyrus_game::script::OrdinaryStableProbe;
    let probe = OrdinaryStableProbe::new([2560, 5121, 7680], 0, 12, 4).unwrap();
    assert_eq!(probe.start, [10, 28, 54]);
    assert_eq!(probe.end, [10, 36, 54]);
    assert_eq!(probe.model_position(30), [2560, 4608, 13824]);
    assert!(probe.accepts_terrain(28.0).unwrap());
    assert!(probe.accepts_terrain(36.0).unwrap());
    assert!(!probe.accepts_terrain(27.5).unwrap());
    assert!(probe.accepts_terrain(36.5).unwrap());
    assert!(!probe.accepts_terrain(27.49).unwrap());
    assert!(probe.accepts_terrain(36.99).unwrap());
    assert!(!probe.accepts_terrain(37.0).unwrap());
    let negative = OrdinaryStableProbe::new([0, -256, 0], 0, 0, 0).unwrap();
    assert!(negative.accepts_terrain(-1.9).unwrap());
    assert!(!negative.accepts_terrain(-0.9).unwrap());
    assert!(probe.accepts_terrain(f64::NAN).is_err());
}

#[test]
fn stable_recovery_requires_own_query_and_keeps_terrain_position() {
    use cyrus_collision::ground::TerrainGround;
    let (mut world, mut data, models) =
        static_model_fixture_at(cache(), Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_stable_recovery_query(id, &scene, &data, [600, 600], &[id])
        .unwrap();
    let before = world.spatial(id).unwrap().local.position;
    // The receiver-free branch accepts even when there is no support.
    assert!(
        world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert!(
        world
            .query_stable_recovery(other, &prepared, &scene, &data)
            .is_err()
    );
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert!(
        !world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
    // The initial shape has positive extent 20, so inclusive bounds are 39..41.
    for (height, expected) in [(38, false), (39, true), (41, true), (42, false)] {
        scene.terrain = Some(
            TerrainGround::new(vec![height; 256 * 256], vec![false; 256 * 256], None).unwrap(),
        );
        assert_eq!(
            world
                .query_stable_recovery(id, &prepared, &scene, &data)
                .unwrap(),
            expected
        );
        assert_eq!(world.spatial(id).unwrap().local.position, before);
    }
    scene.terrain =
        Some(TerrainGround::new(vec![40; 256 * 256], vec![true; 256 * 256], None).unwrap());
    assert!(
        world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap_err()
            .to_string()
            .contains("water or grid edge unsupported")
    );
}

#[test]
fn stable_recovery_model_contact_assigns_probe_position_before_terrain() {
    use cyrus_collision::ground::TerrainGround;
    let mut scripts = cache();
    scripts.instances[0].angles[1] = 768;
    let (mut world, mut data, models) =
        static_model_fixture_at(scripts, Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 25;
    scene.terrain =
        Some(TerrainGround::new(vec![40; 256 * 256], vec![false; 256 * 256], None).unwrap());
    let prepared = world
        .prepare_stable_recovery_query(id, &scene, &data, [600, 600], &[id, other])
        .unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert!(
        world
            .query_stable_recovery(id, &prepared, &scene, &data)
            .unwrap()
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [583 * 256, 0, 583 * 256]
    );
    // The supporting instance keeps its previously stored presentation transform.
    assert_eq!(
        world
            .model_presentation(other)
            .unwrap()
            .unwrap()
            .pose
            .position,
        [600 * 256, 20 * 256, 600 * 256]
    );
}

#[test]
fn ordinary_water_rollback_restores_retained_position_frame_and_flags() {
    use cyrus_game::script::{ScriptInputs, ScriptSound, ScriptStage};
    use cyrus_world::sound::AnimationSoundState;
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_6".into(), 7);
    let (mut world, mut data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = serde_json::json!("animation_table");
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    let before = world.spatial(id).unwrap().local.position;
    world.set_actor_surface(id, 0).unwrap();
    assert!(world.retain_ordinary_water_position(id).unwrap());
    world.apply_ordinary_directional_motion(id, 2, 512).unwrap();
    world.snapshot_spatial(id).unwrap();
    assert_ne!(world.spatial(id).unwrap().previous_local_position, before);
    let mut events = Vec::new();
    let snapshot = world
        .capture_ordinary_rollback(id, &mut |actor, sound| {
            events.push((actor, sound));
            Ok(())
        })
        .unwrap();
    assert!(
        !world
            .rollback_ordinary_water(id, Some(id), &snapshot, &mut |_, _| Ok(()))
            .unwrap()
    );
    world.set_actor_surface(id, 1).unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(11));
    world.apply_ordinary_directional_motion(id, 4, 512).unwrap();
    assert!(!world.retain_ordinary_water_position(id).unwrap());
    assert_ne!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(
        world.actors()[0].control_state().unwrap().movement_flags,
        51
    );
    let other_snapshot = world
        .capture_ordinary_rollback(other, &mut |_, _| Ok(()))
        .unwrap();
    assert!(
        world
            .rollback_ordinary_water(id, None, &other_snapshot, &mut |_, _| Ok(()))
            .is_err()
    );
    assert!(
        world
            .rollback_ordinary_water(id, None, &snapshot, &mut |actor, sound| {
                events.push((actor, sound));
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    assert_eq!(world.actors()[0].attributes()[6], 0);
    assert_eq!(
        world.actors()[0].control_state().unwrap().movement_flags,
        48
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .collision_transition_flags
            & 1,
        1
    );
    assert_eq!(
        events,
        vec![
            (
                id,
                ScriptSound::AnimationState(AnimationSoundState::Capture)
            ),
            (
                id,
                ScriptSound::AnimationState(AnimationSoundState::Restore)
            )
        ]
    );
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.animation_frame(id).unwrap(), Some(11));
}

#[test]
fn ordinary_queued_clearance_truncates_fractional_terrain_toward_zero() {
    use cyrus_collision::ground::TerrainGround;
    use cyrus_game::script::OrdinaryGroundQuery;
    for (actor_y, flat_height, diagonal_height, expected) in
        [(20, 22, 24, 512), (-25, -22, -24, 768)]
    {
        let (mut world, data, models) =
            static_model_fixture_at(cache(), Some(20.0), [600, actor_y, 600]);
        let id = world.id(0).unwrap();
        world.refresh_model_presentations(&data, &[id]).unwrap();
        let mut scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
            .unwrap();
        let mut heights = vec![flat_height; 256 * 256];
        heights[(256 - 2) * 256 + 3] = diagonal_height;
        scene.terrain = Some(TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap());
        assert_eq!(
            world
                .query_ordinary_queued_clearance(
                    id,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    }
                )
                .unwrap(),
            expected
        );
    }
}

#[test]
fn ordinary_slide_direction_boundary_and_scaled_turn_rate() {
    use cyrus_game::script::{
        OrdinarySlide, OrdinarySlopeTurn, ordinary_slide, ordinary_slope_turn,
    };
    assert_eq!(
        ordinary_slide([0, -100, 200], 511, false, 36, 256),
        OrdinarySlide {
            movement_heading: 0,
            target_heading: 0,
            group: 75,
            movement_bits: 0x30,
            distance: 4608,
        }
    );
    assert_eq!(
        ordinary_slide([0, -100, 200], 512, true, 36, 128),
        OrdinarySlide {
            movement_heading: 0,
            target_heading: 1024,
            group: 78,
            movement_bits: 0xc0,
            distance: 2304,
        }
    );
    assert_eq!(
        ordinary_slide([0, -100, 200], 2047, true, -1, 256).distance,
        1024
    );
    assert_eq!(ordinary_slide([0, -100, 200], 0, false, 36, 0).distance, 0);
    assert_eq!(
        ordinary_slope_turn(0, 100, 12, 128, false, false),
        OrdinarySlopeTurn {
            heading: 8,
            group: Some(14)
        }
    );
    assert_eq!(
        ordinary_slope_turn(2044, 4, 12, 128, false, false),
        OrdinarySlopeTurn {
            heading: 4,
            group: None
        }
    );
    assert_eq!(
        ordinary_slope_turn(0, 1024, 12, 256, true, false),
        OrdinarySlopeTurn {
            heading: 12,
            group: Some(96)
        }
    );
    assert_eq!(
        ordinary_slope_turn(1024, 0, 12, 256, false, true),
        OrdinarySlopeTurn {
            heading: 1012,
            group: Some(17)
        }
    );
}

#[test]
fn ordinary_slope_updates_shared_animation_then_requeries_retained_support() {
    use cyrus_collision::ground::{GroundSupport, SelectedSupport, SupportSource};
    use cyrus_game::script::{OrdinaryGroundQuery, OrdinarySlopeInputs};
    let mut scripts = cache();
    scripts
        .actors
        .insert("SUPPORT".into(), scripts.actors["LOGIC"].clone());
    scripts.instances[1].actor = "SUPPORT".into();
    let (mut world, mut data, models) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    data.metadata["actors"][0]["animation_groups"] = serde_json::json!(
        [0, 14, 15, 16, 17, 75, 76, 77, 78].map(|group| serde_json::json!({
            "group_index":group,"anim_id":group+1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":group},{"opcode":2,"value":0}]
        }))
    );
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.refresh_model_presentations(&data, &[other]).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.slide_speed = Some(8);
    world
        .prepare_ordinary_collision_shape(id, &data, &scene)
        .unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 50,
        sidedness: false,
    };
    let inputs = |selected_player| OrdinarySlopeInputs {
        selected_player,
        holding_weapon: false,
        combat_active: false,
        frame_scale: 64,
        ticks: 0,
    };
    let support = SelectedSupport::Primary {
        source: SupportSource::Terrain,
        support: GroundSupport {
            height: 20.0,
            normal: [0, -100, 200],
        },
    };
    assert!(
        world
            .resolve_ordinary_slope(
                id,
                Some(&support),
                query(),
                inputs(Some(id)),
                &mut |_, _| Ok(())
            )
            .unwrap_err()
            .to_string()
            .contains("selected-player slope camera")
    );
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 5120, 7680]
    );
    world
        .resolve_ordinary_slope(
            id,
            Some(&support),
            query(),
            inputs(None),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 1536, 7936]
    );
    assert_eq!(world.animation_frame(id).unwrap(), Some(75));
    assert_eq!(world.model_support(id).unwrap().actor, Some(other));
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            control.sliding_state,
            control.sliding_distance,
            control.movement_flags
        ),
        (1, 256, 0x30)
    );
    world
        .resolve_ordinary_slope(
            id,
            Some(&support),
            query(),
            inputs(None),
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 1536, 8192]
    );
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        512
    );
    world
        .resolve_ordinary_slope(id, None, query(), inputs(None), &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [2560, 1536, 8192]
    );
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        512
    );
}

#[test]
fn ordinary_support_follow_carries_position_and_heading_before_axis_locks() {
    use cyrus_game::script::{ModelSupportOwners, ScriptInputs, ScriptStage};
    for enabled in [false, true] {
        let mut scripts = cache();
        scripts.definition.attributes = (0..25).map(|index| format!("attr_{index}")).collect();
        for (name, kind) in [
            ("MoveByAxis", CallKind::Multitask),
            ("RotateByAxis", CallKind::Multitask),
        ] {
            scripts.definition.functions.push(Function {
                name: name.into(),
                param_count: 3,
                kind,
            });
        }
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes.insert("attr_24".into(), u8::from(enabled));
        actor.program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: wait(
                        1,
                        "MoveByAxis",
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: 0 },
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
                    operation: wait(
                        2,
                        "RotateByAxis",
                        CallKind::Multitask,
                        vec![
                            Value::Integer { value: 1 },
                            Value::Integer { value: 512 },
                            Value::Integer { value: 1 },
                        ],
                    ),
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
        for instance in &mut scripts.instances {
            instance.object_type = 1;
        }
        scripts.instances[0].map_position = [20, 20, 30];
        scripts.instances[0].angles = [128, 2000, 64];
        scripts.instances[1].map_position = [10, 20, 30];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let rider = world.id(0).unwrap();
        let owner = world.id(1).unwrap();
        assert!(!world.follow_ordinary_support(rider, None).unwrap());
        world.snapshot_spatial(owner).unwrap();
        let inputs = ScriptInputs {
            nominal_ticks: Some(0),
            frame_scale: Some(256),
            ..Default::default()
        };
        world
            .advance(
                owner,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            )
            .unwrap();
        world
            .advance(owner, ScriptStage::Tasks, inputs, None, None, 20)
            .unwrap();
        assert_eq!(
            world.spatial(owner).unwrap().local.position,
            [2816, 5120, 7680]
        );
        assert_eq!(world.spatial(owner).unwrap().local.angles, [0, 512, 0]);
        world
            .set_model_support(
                rider,
                ModelSupportOwners {
                    actor: Some(owner),
                    static_placement: None,
                },
            )
            .unwrap();
        let initial = world.spatial(rider).unwrap().local;
        if enabled {
            assert!(world.follow_ordinary_support(rider, Some(rider)).is_err());
            assert_eq!(
                world.spatial(rider).unwrap().local.position,
                initial.position
            );
        }
        assert_eq!(world.follow_ordinary_support(rider, None).unwrap(), enabled);
        assert_eq!(
            world.spatial(rider).unwrap().local.position,
            if enabled {
                [2816, 5120, 10240]
            } else {
                [5120, 5120, 7680]
            }
        );
        assert_eq!(
            world.spatial(rider).unwrap().local.angles,
            if enabled {
                [128, 464, 64]
            } else {
                [128, 2000, 64]
            }
        );
    }
}

#[test]
fn ordinary_automatic_ground_response_retains_support_and_sustained_script_updates() {
    use cyrus_game::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = cache();
    scripts.definition.attributes = (0..116).map(|index| format!("attr_{index}")).collect();
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes.insert("attr_1".into(), 4);
    actor.attributes.insert("attr_6".into(), 128);
    actor.attributes.insert("attr_115".into(), 64);
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (mut world, data, models) = static_model_fixture(scripts);
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[first, second])
        .unwrap();
    world.set_actor_surface(first, 7).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 50;
    let mut random = cyrus_game::random::EngineRandom::default();
    for update in 1..=3 {
        world
            .update_ordinary(
                &data,
                [10, 30],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut random,
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors().iter().all(|actor| actor.failure().is_none()),
            "{:?}",
            world
                .actors()
                .iter()
                .map(|actor| actor.failure())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [4 + update]
        );
        assert_eq!(
            world.actors()[1].runtime().unwrap().variables(),
            [9 + update]
        );
        assert_eq!(
            world.spatial(first).unwrap().local.position,
            [2560, 4608, 7680]
        );
        assert_eq!(world.model_support(first).unwrap().actor, Some(second));
        assert_eq!(world.actor_surface(first).unwrap(), 0);
        let control = world.actors()[0].control_state().unwrap();
        assert!(control.stable_position_valid);
        assert_eq!(control.stable_local_position, [2560, 4608, 7680]);
        assert_eq!(world.actors()[0].attributes()[6], 64);
    }
}

#[test]
fn ordinary_script_wrapper_counts_and_clears_prior_response_marks() {
    let (mut world, _, _) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, 0).unwrap();
    world.retain_ordinary_water_position(id).unwrap();
    let snapshot = world
        .capture_ordinary_rollback(id, &mut |_, _| Ok(()))
        .unwrap();
    world.set_actor_surface(id, 1).unwrap();
    let mark = |world: &mut ScriptWorld| {
        assert!(
            world
                .rollback_ordinary_water(id, None, &snapshot, &mut |_, _| Ok(()))
                .unwrap()
        );
    };
    mark(&mut world);
    assert!(
        world
            .finish_ordinary_script_response(id, None)
            .unwrap_err()
            .to_string()
            .contains("nominal_ticks")
    );
    let state = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            state.collision_response_age,
            state.collision_transition_flags
        ),
        (0, 1)
    );
    for (ticks, expected) in [(1, 1), (255, 0), (200, 200), (7, 200)] {
        mark(&mut world);
        world
            .finish_ordinary_script_response(id, Some(ticks))
            .unwrap();
        let state = world.actors()[0].control_state().unwrap();
        assert_eq!(state.collision_response_age, expected);
        assert_eq!(
            (
                state.body_response_flags,
                state.collision_transition_flags,
                state.overlap_contact_identifier
            ),
            (0, 0, 0)
        );
        assert_eq!(state.collision_response_auxiliary, [0; 2]);
    }
    world.finish_ordinary_script_response(id, None).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .collision_response_age,
        0
    );
}

#[test]
fn ordinary_stable_response_restores_snapshot_and_consumes_one_shared_heading_draw() {
    use cyrus_game::{
        random::EngineRandom,
        script::{OrdinaryStableRecoveryInputs, ScriptInputs, ScriptSound, ScriptStage},
    };
    use cyrus_world::sound::AnimationSoundState;
    let mut scripts = cache();
    let attributes = &mut scripts.actors.get_mut("LOGIC").unwrap().attributes;
    attributes.insert("attr_15".into(), 12);
    attributes.insert("attr_6".into(), 64);
    let (mut world, mut data, models) =
        static_model_fixture_at(scripts, Some(20.0), [600, 20, 600]);
    let id = world.id(0).unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "LOGIC")
        .unwrap();
    definition["model_load_path"] = "animation_table".into();
    definition["animation_groups"] = serde_json::json!([{
        "group_index":0,"anim_id":1,"anim_type":0,
        "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":1}]
    }]);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    let mut events = Vec::new();
    let snapshot = world
        .capture_ordinary_rollback(id, &mut |actor, sound| {
            events.push((actor, sound));
            Ok(())
        })
        .unwrap();
    let mut random = EngineRandom::from_state(1);
    assert!(
        !world
            .recover_ordinary_stable(
                id,
                OrdinaryStableRecoveryInputs {
                    scene: &scene,
                    animations: &data,
                    view: [600, 600],
                    actors: &[id],
                    random: &mut random
                },
                &snapshot,
                &mut |_, _| Ok(())
            )
            .unwrap()
    );
    assert_eq!(random.state(), 1);
    assert!(world.retain_ordinary_stable_position(id).unwrap());
    let stable = world.spatial(id).unwrap().local.position;
    world.apply_ordinary_directional_motion(id, 2, 512).unwrap();
    for _ in 0..2 {
        world
            .advance(
                id,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
    }
    assert_eq!(world.animation_frame(id).unwrap(), Some(11));
    assert_ne!(world.spatial(id).unwrap().local.position, stable);
    assert!(
        world
            .recover_ordinary_stable(
                id,
                OrdinaryStableRecoveryInputs {
                    scene: &scene,
                    animations: &data,
                    view: [600, 600],
                    actors: &[id],
                    random: &mut random
                },
                &snapshot,
                &mut |actor, sound| {
                    events.push((actor, sound));
                    Ok(())
                }
            )
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, stable);
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 2030);
    assert_eq!(random.state(), 1103527590);
    assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    assert_eq!(world.actors()[0].attributes()[6], 0);
    let state = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            state.movement_flags,
            state.falling_state,
            state.sliding_state,
            state.collision_transition_flags
        ),
        (0, 0, 0, 1)
    );
    assert_eq!(
        events,
        [
            (
                id,
                ScriptSound::AnimationState(AnimationSoundState::Capture)
            ),
            (
                id,
                ScriptSound::AnimationState(AnimationSoundState::Restore)
            )
        ]
    );
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(3),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let state = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (
            state.collision_response_age,
            state.collision_transition_flags
        ),
        (3, 0)
    );
}

#[test]
fn fresh_combat_maintenance_restores_collision_and_rejects_addressed_effects() {
    for (capable, defenses, expected_defenses) in [
        (1, [1, 0, 3, 0], [Some(1), None, None, None]),
        (1, [1, 2, 3, 4], [Some(1), Some(2), Some(3), Some(4)]),
        (1, [0, 2, 3, 4], [None; 4]),
        (0, [1, 2, 3, 4], [None; 4]),
    ] {
        let mut scripts = cache();
        scripts.combat_attacks = (0..65)
            .map(|index| cyrus_assets::script::combat::Attack {
                is_combo: true,
                motions: vec![
                    serde_json::from_value(serde_json::json!({
                        "section":index,"kind":0,"animation_group":0,
                        "min_range":0,"max_range":0,"first_collide_frame":0,"collide_duration":0
                    }))
                    .unwrap(),
                ],
                min_range: 65535,
                max_range: index,
            })
            .collect();
        scripts.combat_attacks[54].motions = vec![
            serde_json::from_value(serde_json::json!({
                "section":54,"kind":0,"animation_group":97,"min_range":10,"max_range":100,
                "first_collide_frame":7,"collide_duration":3,"defense":2
            }))
            .unwrap(),
        ];
        scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes.extend([
            ("attr_1".into(), 1),
            ("attr_14".into(), capable),
            ("attr_119".into(), 7),
            ("attr_70".into(), 54),
            ("attr_72".into(), 56),
            ("attr_78".into(), 64),
            ("attr_79".into(), defenses[0]),
            ("attr_80".into(), defenses[1]),
            ("attr_81".into(), defenses[2]),
            ("attr_82".into(), defenses[3]),
        ]);
        for instance in &mut scripts.instances {
            instance.object_type = 1;
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let first = world.id(0).unwrap();
        let second = world.id(1).unwrap();
        world
            .initialize_map_animation(first, &data, 1, &mut |_, _| Ok(()))
            .unwrap();
        if capable == 0 {
            assert!(world.actors()[0].control_state().unwrap().combat.is_none());
            continue;
        }
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (combat.target, combat.flags, combat.action, combat.cooldown),
            (None, 0, -1, 0)
        );
        assert_eq!(combat.attacker_selection_state, 0);
        assert_eq!(
            combat.attack_indices,
            [
                Some(54),
                None,
                Some(56),
                None,
                None,
                None,
                None,
                None,
                Some(64)
            ]
        );
        assert_eq!(combat.defense_indices, expected_defenses);
        assert_eq!(
            world.combat_attack(first, 0).unwrap().unwrap().max_range,
            54
        );
        assert!(world.combat_attack(first, 1).unwrap().is_none());
        assert_eq!(
            world.combat_defence_candidate(first, 0).unwrap(),
            Some(cyrus_game::script::decision::DefenceCandidate {
                min_range: 10,
                max_range: 100,
                near_timing: 7,
                counter_attack: 2,
            })
        );
        assert!(world.combat_defence_candidate(first, 1).unwrap().is_none());
        assert!(
            world
                .combat_defence_candidate(first, 2)
                .unwrap_err()
                .to_string()
                .contains("counter-attack reference unavailable")
        );
        assert!(world.combat_defence_candidate(second, 0).is_err());

        assert_eq!(
            world.combat_attack(first, 2).unwrap().unwrap().max_range,
            56
        );
        assert_eq!(
            world.combat_attack(first, 8).unwrap().unwrap().max_range,
            64
        );
        assert!(world.combat_attack(first, 9).is_err());
        assert!(world.combat_attack(second, 0).is_err());
        world.maintain_map_combat(first, 1, false, true).unwrap();
        assert_eq!(world.actors()[0].attributes()[1], 1);
        world.maintain_map_combat(first, 1, false, false).unwrap();
        assert_eq!(world.actors()[0].attributes()[1], 7);
        world.send_combat_message(second, first, 0xd001).unwrap();
        let error = world
            .maintain_map_combat(first, 1, false, false)
            .unwrap_err();
        assert!(format!("{error:#}").contains("combat message 53249 from actor 1 unsupported"));
        assert_eq!(world.consume_combat_message(first).unwrap(), None);
        let stale = cyrus_game::script::ScriptActorId {
            generation: 2,
            index: 0,
        };
        assert!(world.send_combat_message(second, stale, 1).is_err());
        world.send_combat_message(stale, stale, 0).unwrap();
    }
}

#[test]
fn navigation_reservations_use_identifier_bytes_and_maintenance_clears_all_maps() {
    let mut world = wander_world(
        1024,
        0,
        serde_json::json!([
            {"node_count":2,"walk_nodes":[{"reserved":255},{"reserved":1}]},
            {"node_count":1,"walk_nodes":[{"reserved":2}]}
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
                cyrus_game::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                0,
                0
            )
            .is_err()
    );
}

#[test]
fn separate_updates_retain_locals_and_tasks_without_ordinary_control_or_attributes() {
    use cyrus_game::script::ScriptInputs;
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 2;
        instance.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (first, second) in [(5, 10), (6, 11)] {
        world
            .update_separate(
                [600, 600],
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
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [first]);
        assert_eq!(world.actors()[1].runtime().unwrap().variables(), [second]);
        for actor in world.actors() {
            assert!(actor.control_state().is_none());
            assert!(actor.attributes().iter().all(|value| *value == 0));
            assert!(actor.failure().is_none());
        }
    }
    let mut scripts = wait_cache(vec![
        wait(
            1,
            "Wait",
            CallKind::Task,
            vec![Value::Integer { value: 100 }],
        ),
        Operation::End { target: 0 },
    ]);
    for instance in &mut scripts.instances {
        instance.object_type = 2;
        instance.map_position = [600, 20, 600];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    for _ in 0..3 {
        world
            .update_separate(
                [600, 600],
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
        for actor in world.actors() {
            assert_eq!(actor.runtime().unwrap().active_tasks(), 1);
        }
    }
}

#[test]
fn separate_attribute_writes_are_temporary_and_activation_does_not_bypass_view_selection() {
    use cyrus_game::script::ScriptInputs;
    let mut scripts = cache();
    scripts.definition.functions.extend([
        Function {
            name: "GetAttribute".into(),
            param_count: 1,
            kind: CallKind::Function,
        },
        Function {
            name: "SetAttribute".into(),
            param_count: 2,
            kind: CallKind::Function,
        },
    ]);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes.insert("attr_0".into(), 99);
    let get = attribute_call(1, "GetAttribute", 0, None);
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Flag { index: 0 },
                    formula: Formula {
                        first: Value::Call {
                            call: Box::new(get),
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
                operation: Operation::Call {
                    call: attribute_call(2, "SetAttribute", 0, Some(7)),
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
    for instance in &mut scripts.instances {
        instance.object_type = 2;
        instance.map_position = [600, 20, 600];
        instance.active = 1;
    }
    scripts.instances[1].map_position = [60000, 20, 60000];
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world
            .separate_actors([600, 600])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>(),
        [0]
    );
    for _ in 0..2 {
        world
            .update_separate(
                [600, 600],
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
        assert_eq!(world.flags()[0], 0);
        assert_eq!(world.actors()[0].attributes()[0], 0);
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
    }
}

#[test]
fn separate_disabled_tasks_preserve_locals_and_maintain_authored_lights() {
    use cyrus_game::script::ScriptInputs;
    for object_type in [2, 6] {
        let mut scripts = cache();
        scripts.updates.task_system = false;
        for instance in &mut scripts.instances {
            instance.object_type = object_type;
            instance.map_position = [600, 20, 600];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world
            .update_separate(
                [600, 600],
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
        for (index, actor) in world.actors().iter().enumerate() {
            assert_eq!(
                actor.runtime().unwrap().variables(),
                [if index == 0 { 4 } else { 9 }]
            );
            assert!(actor.control_state().is_none());
            assert!(actor.failure().is_none());
        }
    }
}

#[test]
fn wait_on_dialog_uses_initialized_shared_records_and_yields_before_completion() {
    let mut scripts = wait_cache(vec![
        wait(4, "WaitOnDialog", CallKind::Task, vec![]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: "WaitOnDialog".into(),
        param_count: 0,
        kind: CallKind::Task,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world.globals().dialogue().unwrap().settings(),
        &scripts.dialogue
    );
    assert!(!world.globals().dialogue().unwrap().waiting());
    for index in 0..2 {
        let id = world.id(index).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 1);
        world.step_tasks(id, Some(256)).unwrap();
        assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 0);
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [7]);
    }
}

#[test]
fn dialogue_lines_block_other_receivers_and_reuse_only_cleared_records() {
    let mut scripts = wait_cache(vec![
        wait(4, "WaitOnDialog", CallKind::Task, vec![]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: "WaitOnDialog".into(),
        param_count: 0,
        kind: CallKind::Task,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let speaker = world.id(0).unwrap();
    let listener = world.id(1).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    for index in 1..scripts.dialogue.max_dialog_lines as usize {
        assert_eq!(
            dialogue.begin_line(speaker, "line".into(), Some(3), 100),
            Some(index)
        );
    }
    assert_eq!(
        dialogue.begin_line(speaker, "overflow".into(), None, 100),
        None
    );
    dialogue.clear_line(None);
    assert!(dialogue.waiting());
    world.step(listener, Thread::Main, 20).unwrap();
    world.step_tasks(listener, Some(256)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.clear_line(Some(2));
    assert!(!dialogue.line_active(Some(2)));
    assert_eq!(
        dialogue.begin_line(speaker, "replacement".into(), None, 100),
        Some(2)
    );
    for index in 1..scripts.dialogue.max_dialog_lines as usize {
        dialogue.clear_line(Some(index));
    }
    assert!(!dialogue.waiting());
    world.step_tasks(listener, Some(256)).unwrap();
    world.step(listener, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
}

#[test]
fn dialogue_sample_cleanup_excludes_reserved_player_line_and_other_slots() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.begin_player_line(first, "player".into(), Some(2), 100);
    assert_eq!(
        dialogue.begin_line(first, "first".into(), Some(2), 100),
        Some(1)
    );
    assert_eq!(
        dialogue.begin_line(second, "second".into(), Some(2), 100),
        Some(2)
    );
    assert_eq!(
        dialogue.begin_line(second, "other".into(), Some(3), 100),
        Some(3)
    );
    dialogue.clear_sample_lines(Some(2));
    assert!(dialogue.line_active(Some(0)));
    assert!(!dialogue.line_active(Some(1)));
    assert!(!dialogue.line_active(Some(2)));
    assert!(dialogue.line_active(Some(3)));
    dialogue.begin_player_line(second, "replacement".into(), None, 100);
    dialogue.clear_sample_lines(Some(3));
    dialogue.clear_sample_lines(None);
    assert!(dialogue.waiting());
    dialogue.clear_line(Some(0));
    assert!(!dialogue.waiting());
}

#[test]
fn dialogue_sound_reset_clears_player_and_ordinary_records() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let speaker = world.id(0).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.begin_player_line(speaker, "player".into(), Some(2), 100);
    let line = dialogue.begin_line(speaker, "ordinary".into(), Some(3), 200);
    assert_eq!(line, Some(1));
    dialogue.reset_active_lines();
    assert!(!dialogue.waiting());
    assert!(!dialogue.line_active(Some(0)));
    assert!(!dialogue.line_active(line));
    let state = serde_json::to_value(&*dialogue).unwrap();
    for index in [0, 1] {
        assert_eq!(state["records"][index]["text"], "");
        assert!(state["records"][index]["sample"].is_null());
    }
    assert_eq!(state["records"][0]["remaining"], 100);
    assert_eq!(state["records"][1]["remaining"], 200);
    assert_eq!(state["records"][2]["sample"], 0);
    assert_eq!(
        dialogue.begin_line(speaker, "reused".into(), None, 300),
        Some(1)
    );
}

#[test]
fn dialogue_record_updates_preserve_order_and_speech_specific_completion() {
    use cyrus_game::script::{DialogueEffects, DialoguePlayback, ScriptActorId};
    #[derive(Default)]
    struct Effects {
        events: Vec<String>,
        playing: bool,
    }
    impl DialogueEffects for Effects {
        fn inhibit_speaker(&mut self, speaker: ScriptActorId) -> anyhow::Result<()> {
            self.events.push(format!("inhibit {}", speaker.index));
            Ok(())
        }
    }
    impl DialoguePlayback for Effects {
        fn sample_playing(&mut self, sample: u8) -> anyhow::Result<bool> {
            self.events.push(format!("status {sample}"));
            Ok(self.playing)
        }
        fn stop_sample(&mut self, sample: u8) -> anyhow::Result<()> {
            self.events.push(format!("stop {sample}"));
            self.playing = false;
            Ok(())
        }
        fn refresh_volume(&mut self, _: ScriptActorId, sample: u8) -> anyhow::Result<()> {
            self.events.push(format!("volume {sample}"));
            Ok(())
        }
        fn present_line(&mut self, index: usize, text: &str) -> anyhow::Result<()> {
            self.events.push(format!("present {index} {text}"));
            Ok(())
        }
    }
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    let speaker = world.id(0).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.begin_player_line(speaker, "expired".into(), None, 21);
    dialogue.begin_line(speaker, "later".into(), Some(3), 42);
    let mut effects = Effects::default();
    dialogue.advance_records(1, false, &mut effects).unwrap();
    assert_eq!(
        effects.events,
        ["inhibit 0", "inhibit 0", "present 1 later"]
    );
    assert!(!dialogue.line_active(Some(0)));
    assert!(dialogue.line_active(Some(1)));
    assert_eq!(dialogue.presented_line(), Some((1, "later")));
    effects.events.clear();
    effects.playing = true;
    dialogue.begin_player_line(speaker, "silent".into(), None, 0);
    dialogue.advance_records(1, true, &mut effects).unwrap();
    assert_eq!(
        effects.events,
        [
            "inhibit 0",
            "inhibit 0",
            "status 3",
            "volume 3",
            "present 1 later"
        ]
    );
    assert!(
        dialogue.line_active(Some(0)),
        "speech mode does not expire an absent sample by countdown"
    );
    effects.playing = false;
    effects.events.clear();
    dialogue.begin_line(speaker, "unvisited".into(), Some(4), 1);
    dialogue.advance_records(1, true, &mut effects).unwrap();
    assert_eq!(effects.events, ["inhibit 0", "inhibit 0", "status 3"]);
    assert_eq!(dialogue.presented_line(), None);
    assert!(!dialogue.line_active(Some(1)));
    assert!(dialogue.line_active(Some(2)));

    let mut fresh = ScriptWorld::prepare(&cache(), 2).unwrap();
    let speaker = fresh.id(0).unwrap();
    let dialogue = fresh.globals_mut().dialogue_mut().unwrap();
    effects.events.clear();
    effects.playing = true;
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert_eq!(&effects.events[..2], ["status 0", "stop 0"]);
    assert_eq!(
        effects
            .events
            .iter()
            .filter(|event| *event == "status 0")
            .count(),
        20
    );
    assert_eq!(
        effects
            .events
            .iter()
            .filter(|event| event.starts_with("stop "))
            .count(),
        1
    );
    dialogue.begin_player_line(speaker, "held".into(), None, 100);
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(dialogue.waiting(), "held skip does not repeat cleanup");
    dialogue
        .advance(0, true, false, Some(1), &mut effects)
        .unwrap();
    dialogue
        .advance(0, true, true, Some(2), &mut effects)
        .unwrap();
    assert!(dialogue.waiting(), "only exact lockout one admits cleanup");
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(
        dialogue.waiting(),
        "ineligible press still consumes the edge"
    );
    dialogue
        .advance(0, true, false, Some(1), &mut effects)
        .unwrap();
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(!dialogue.waiting());

    let mut actors = cyrus_game::script::ScriptActors::default();
    actors.current = Some(ScriptWorld::prepare(&cache(), 3).unwrap());
    assert!(
        format!(
            "{:#}",
            actors
                .current
                .as_mut()
                .unwrap()
                .advance_dialogue(0, true, true, &mut effects)
                .unwrap_err()
        )
        .contains("lockout unavailable for skip edge")
    );
    actors
        .current
        .as_mut()
        .unwrap()
        .advance_dialogue(0, true, false, &mut effects)
        .unwrap();
    actors.begin_new_game();
    let world = actors.current.as_mut().unwrap();
    let speaker = world.id(0).unwrap();
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .begin_line(speaker, "speaker".into(), None, 100);
    assert_eq!(world.actors()[0].attributes()[117], 0);
    world
        .advance_dialogue(1, true, false, &mut effects)
        .unwrap();
    assert_eq!(world.actors()[0].attributes()[117], 1);
    assert_eq!(world.actors()[1].attributes()[117], 0);
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .clear_line(Some(1));
    world.globals_mut().dialogue_mut().unwrap().begin_line(
        ScriptActorId {
            generation: 2,
            index: 1,
        },
        "stale".into(),
        None,
        100,
    );
    assert!(
        format!(
            "{:#}",
            world
                .advance_dialogue(1, true, false, &mut effects)
                .unwrap_err()
        )
        .contains("expired world generation 2")
    );
    assert_eq!(world.actors()[1].attributes()[117], 0);
    assert!(world.dialogue_speaker_positions().unwrap().is_empty());
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(
        !world.globals().dialogue().unwrap().waiting(),
        "skip clears records without requiring a speaker pose"
    );
}

#[test]
fn topic_menu_owner_keeps_its_state_separate_from_dialogue_and_lockout() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.globals().menu_state(), 0);
    world.globals_mut().begin_topic_menu();
    assert_eq!(world.globals().menu_state(), 1);
    world.globals_mut().end_topic_menu();
    assert_eq!(world.globals().menu_state(), 0);
    assert_eq!(world.globals().lockout(), None);
    assert!(!world.globals().dialogue().unwrap().waiting());
}

#[test]
fn player_dialogue_task_uses_reserved_record_once_and_reset_bypasses_lookup() {
    use cyrus_game::script::{
        DialogueLine, DialogueSource, ScriptActorId, ScriptInputs, ScriptStage,
    };
    use std::cell::RefCell;
    #[derive(Debug, Default)]
    struct Source(
        RefCell<Vec<([u8; 4], ScriptActorId, u32)>>,
        std::cell::Cell<u32>,
    );
    impl DialogueSource for Source {
        fn player_line(
            &self,
            label: [u8; 4],
            player: ScriptActorId,
            width: u32,
        ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
            self.0.borrow_mut().push((label, player, width));
            if self.1.get() != 0 {
                self.1.set(self.1.get() - 1);
                return Ok(std::task::Poll::Pending);
            }
            Ok(std::task::Poll::Ready(Some(DialogueLine {
                text: "formatted line".into(),
                sample: Some(3),
                lifetime: 128,
            })))
        }
    }
    let mut scripts = wait_cache(vec![
        wait(
            4,
            "RTXp",
            CallKind::Task,
            vec![Value::Integer {
                value: i32::from_le_bytes(*b"TEST"),
            }],
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: "RTXp".into(),
        param_count: 1,
        kind: CallKind::Task,
    });
    let mut player = scripts.actors["LOGIC"].clone();
    player.instances = 0;
    player.instance_variables.clear();
    player.namespace = Some("cyrus".into());
    scripts.actors.insert("CYRUS".into(), player);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 9).unwrap();
    let speaker = world.id(0).unwrap();
    let player = world.id(2).unwrap();
    let source = Source::default();
    let inputs = ScriptInputs {
        dialogue_source: Some(&source),
        sound_reset: Some(false),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            speaker,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        *source.0.borrow(),
        [(*b"TEST", player, scripts.dialogue.max_text_width)]
    );
    let records = serde_json::to_value(world.globals().dialogue().unwrap()).unwrap();
    assert_eq!(
        records["records"][0]["owner"],
        serde_json::to_value(player).unwrap()
    );
    assert_eq!(records["records"][0]["text"], "formatted line");
    assert!(world.globals().dialogue().unwrap().line_active(Some(0)));
    world
        .advance(speaker, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(source.0.borrow().len(), 1);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .clear_line(Some(0));
    world
        .advance(speaker, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    world
        .advance(
            speaker,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.flags(), [7]);

    let mut pending_world = ScriptWorld::prepare(&scripts, 11).unwrap();
    let speaker = pending_world.id(0).unwrap();
    let pending_source = Source::default();
    pending_source.1.set(2);
    let pending_inputs = ScriptInputs {
        dialogue_source: Some(&pending_source),
        nominal_ticks: Some(17),
        ..inputs
    };
    for stage in [ScriptStage::Body(Thread::Main), ScriptStage::Tasks] {
        pending_world
            .advance(speaker, stage, pending_inputs, None, None, 20)
            .unwrap();
        assert!(!pending_world.globals().dialogue().unwrap().waiting());
        assert_eq!(
            pending_world.actors()[0].runtime().unwrap().active_tasks(),
            1
        );
    }
    pending_world
        .advance(speaker, ScriptStage::Tasks, pending_inputs, None, None, 20)
        .unwrap();
    assert!(
        pending_world
            .globals()
            .dialogue()
            .unwrap()
            .line_active(Some(0))
    );
    assert_eq!(pending_source.0.borrow().len(), 3);
    pending_world
        .advance(speaker, ScriptStage::Tasks, pending_inputs, None, None, 20)
        .unwrap();
    assert_eq!(
        pending_source.0.borrow().len(),
        3,
        "ready dialogue must not start again"
    );

    let mut reset_world = ScriptWorld::prepare(&scripts, 10).unwrap();
    let speaker = reset_world.id(0).unwrap();
    let inputs = ScriptInputs {
        sound_reset: Some(true),
        frame_scale: Some(256),
        ..Default::default()
    };
    reset_world
        .advance(
            speaker,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(reset_world.actors()[0].runtime().unwrap().active_tasks(), 1);
    reset_world
        .advance(speaker, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(reset_world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert!(!reset_world.globals().dialogue().unwrap().waiting());
}

#[test]
fn ordered_combat_taunts_share_rng_skip_ineligible_labels_and_fall_back_only_for_death() {
    use cyrus_game::random::EngineRandom;
    let mut scripts = cache();
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.combat_taunts = [
        (2, *b"H000"),
        (0, *b"A000"),
        (0, [0; 4]),
        (0, *b"B000"),
        (0, *b"C000"),
        (0, *b"D000"),
    ]
    .into_iter()
    .map(|(trigger, label)| CombatTaunt {
        trigger,
        label,
        animation: 0,
    })
    .collect();
    actor.attributes.insert("attr_num_taunts".into(), 6);
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut random = EngineRandom::default();
    let selected = world
        .select_ordered_combat_taunt(id, 0, &mut random)
        .unwrap()
        .unwrap();
    assert_eq!(selected.label, *b"C000");
    assert_eq!(random.state(), 662824084);
    assert!(
        world
            .select_ordered_combat_taunt(id, 10, &mut random)
            .unwrap()
            .is_none()
    );
    assert_eq!(random.state(), 662824084);
    assert_eq!(
        world
            .select_ordered_combat_taunt(id, 6, &mut random)
            .unwrap()
            .unwrap()
            .label,
        *b"H000"
    );
    assert_eq!(random.state(), 3295386429);
    assert!(
        world
            .select_ordered_combat_taunt(id, 1, &mut random)
            .unwrap_err()
            .to_string()
            .contains("requires timed selection")
    );
    assert_eq!(random.state(), 3295386429);
    let replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    assert!(
        replacement
            .select_ordered_combat_taunt(id, 0, &mut random)
            .is_err()
    );
    assert_eq!(random.state(), 3295386429);
}

#[test]
fn combat_taunt_presentation_owns_cache_dialogue_animation_and_active_result() {
    use cyrus_game::script::{CombatTauntPlayback, DialogueLine, ScriptActorId};
    #[derive(Default, Debug)]
    struct Playback {
        events: Vec<&'static str>,
        fail: Option<&'static str>,
        speech: bool,
        width: Option<u32>,
    }
    impl Playback {
        fn stage(&mut self, stage: &'static str) -> anyhow::Result<()> {
            self.events.push(stage);
            anyhow::ensure!(self.fail != Some(stage), "injected {stage}");
            Ok(())
        }
    }
    impl CombatTauntPlayback for Playback {
        fn prepare(
            &mut self,
            label: [u8; 4],
            speech: bool,
            width: Option<u32>,
        ) -> anyhow::Result<()> {
            assert_eq!(label, *b"TEST");
            self.speech = speech;
            self.width = width;
            self.stage("prepare")
        }
        fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
            self.stage("release")?;
            *retained = None;
            Ok(())
        }
        fn load(&mut self, _: [u8; 4]) -> anyhow::Result<usize> {
            self.stage("load")?;
            Ok(4)
        }
        fn present(
            &mut self,
            index: usize,
            _: bool,
            _: Option<u32>,
        ) -> anyhow::Result<DialogueLine> {
            assert_eq!(index, 4);
            self.stage("present")?;
            Ok(DialogueLine {
                text: "Voice".into(),
                sample: Some(3),
                lifetime: 210,
            })
        }
    }
    let fixture = |print_text, use_speech| {
        let mut scripts = cache();
        scripts.dialogue.print_text = print_text;
        scripts.dialogue.use_speech = use_speech;
        scripts.definition.attributes = (0..128).map(|index| format!("attr_{index}")).collect();
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert("attr_14".into(), 1);
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .combat_taunts
            .push(CombatTaunt {
                trigger: 0,
                label: *b"TEST",
                animation: 159,
            });
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert("attr_num_taunts".into(), 1);
        for instance in &mut scripts.instances {
            instance.object_type = 1;
        }
        scripts.definition.functions.push(Function {
            name: "beginCombat".into(),
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
            function_id: 1,
            function: "beginCombat".into(),
            target: Object::Me,
            kind: CallKind::Function,
            arguments: vec![],
        });
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
            "animation_groups":[
                {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]},
                {"group_index":159,"anim_id":2,"anim_type":0,"commands":[{"opcode":0,"frame_reference":42},{"opcode":2,"value":0}]}
            ]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        world
            .globals_mut()
            .dialogue_mut()
            .unwrap()
            .begin_player_line(id, "Prior".into(), Some(2), 999);
        (world, id)
    };
    let taunt = CombatTaunt {
        trigger: 0,
        label: *b"TEST",
        animation: 159,
    };
    let empty = CombatTaunt {
        trigger: 0,
        label: [0; 4],
        animation: 159,
    };
    let (mut world, id) = fixture(true, true);
    let mut playback = Playback::default();
    assert_eq!(
        world
            .present_combat_taunt(id, &empty, &mut playback, 0, true, &mut |_, _| Ok(()))
            .unwrap(),
        1
    );
    assert!(playback.events.is_empty());
    assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    assert_eq!(
        world
            .present_combat_taunt(id, &taunt, &mut playback, 0, true, &mut |_, _| Ok(()))
            .unwrap(),
        1
    );
    assert_eq!(playback.events, ["prepare", "release", "load", "present"]);
    assert!(playback.speech);
    assert_eq!(playback.width, Some(500));
    assert_eq!(world.animation_frame(id).unwrap(), Some(42));
    let state = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (state.taunt_cache, state.taunt_dialogue, state.taunt_active),
        (Some(4), Some(1), 1)
    );
    assert!(!world.globals().dialogue().unwrap().line_active(Some(0)));
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .clear_line(Some(1));
    assert_eq!(
        world
            .present_combat_taunt(id, &empty, &mut playback, 0, true, &mut |_, _| Ok(()))
            .unwrap(),
        0
    );
    assert_eq!(playback.events.len(), 4);

    for (print_text, speech) in [(false, true), (true, false), (false, false)] {
        let (mut world, id) = fixture(print_text, speech);
        let mut playback = Playback::default();
        assert_eq!(
            world
                .present_combat_taunt(id, &taunt, &mut playback, 0, false, &mut |_, _| Ok(()))
                .unwrap(),
            i32::from(print_text)
        );
        assert_eq!(playback.speech, speech);
        assert_eq!(playback.width, print_text.then_some(500));
        assert_eq!(
            world.globals().dialogue().unwrap().line_active(Some(0)),
            !print_text
        );
        assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    }
    for failure in ["prepare", "release", "load"] {
        let (mut world, id) = fixture(true, true);
        let mut playback = Playback {
            fail: Some(failure),
            ..Default::default()
        };
        assert!(
            world
                .present_combat_taunt(id, &taunt, &mut playback, 0, true, &mut |_, _| Ok(()))
                .is_err()
        );
        assert_eq!(
            world.globals().dialogue().unwrap().line_active(Some(0)),
            failure != "load"
        );
        assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    }
    let (mut full, full_id) = fixture(true, true);
    for _ in 1..20 {
        assert!(
            full.globals_mut()
                .dialogue_mut()
                .unwrap()
                .begin_line(full_id, "Occupied".into(), None, 50)
                .is_some()
        );
    }
    let mut full_playback = Playback::default();
    assert_eq!(
        full.present_combat_taunt(
            full_id,
            &taunt,
            &mut full_playback,
            0,
            false,
            &mut |_, _| Ok(())
        )
        .unwrap(),
        0
    );
    let state = full.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (state.taunt_cache, state.taunt_dialogue, state.taunt_active),
        (Some(4), None, 0)
    );
    assert_eq!(
        full_playback.events,
        ["prepare", "release", "load", "present"]
    );
    let mut playback = Playback::default();
    assert!(
        world
            .present_combat_taunt(
                ScriptActorId {
                    generation: 2,
                    ..id
                },
                &taunt,
                &mut playback,
                0,
                true,
                &mut |_, _| Ok(())
            )
            .is_err()
    );
    assert!(playback.events.is_empty());
    let (mut entry, id) = fixture(true, true);
    let mut playback = Playback::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    entry
        .begin_actor_combat(
            id,
            cyrus_game::script::CombatEntryInputs {
                weapon: cyrus_game::script::WeaponInputs {
                    selected_player: id,
                    item_system_ready: true,
                    shell_execution: false,
                },
                nominal_ticks: 0,
                taunt_animation_admitted: true,
                selected_player_death_override: false,
            },
            &mut random,
            &mut playback,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(playback.events, ["prepare", "release", "load", "present"]);
    assert_eq!(random.state(), 1103527590);
    assert_eq!(entry.animation_frame(id).unwrap(), Some(42));
    let control = entry.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.death_selector,
            control.living_state,
            control.weapon_drawn
        ),
        (6, 0, true)
    );
    assert_eq!(control.combat.as_ref().unwrap().taunt_active, 1);
    #[derive(Debug)]
    struct Source<'a>(std::cell::RefCell<&'a mut Playback>);
    impl cyrus_game::script::CombatTauntSource for Source<'_> {
        fn with_playback(
            &self,
            update: &mut dyn FnMut(&mut dyn CombatTauntPlayback) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            update(&mut **self.0.borrow_mut())
        }
    }
    let (mut native, id) = fixture(true, true);
    let mut playback = Playback::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    let source = Source(std::cell::RefCell::new(&mut playback));
    let inputs = cyrus_game::script::ScriptInputs {
        nominal_ticks: Some(0),
        combat_entry: Some(cyrus_game::script::CombatEntryInputs {
            weapon: cyrus_game::script::WeaponInputs {
                selected_player: id,
                item_system_ready: true,
                shell_execution: false,
            },
            nominal_ticks: 0,
            taunt_animation_admitted: true,
            selected_player_death_override: false,
        }),
        combat_taunts: Some(&source),
        ..Default::default()
    };
    native
        .advance(
            id,
            cyrus_game::script::ScriptStage::NativeHook,
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            0,
        )
        .unwrap();
    assert_eq!(random.state(), 1103527590);
    assert_eq!(native.animation_frame(id).unwrap(), Some(42));
    native
        .advance(
            id,
            cyrus_game::script::ScriptStage::NativeHook,
            inputs,
            None,
            None,
            0,
        )
        .unwrap();
    assert_eq!(
        source.0.borrow().events,
        ["prepare", "release", "load", "present"]
    );
    let (mut overridden, id) = fixture(true, true);
    let mut playback = Playback::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    overridden
        .begin_actor_combat(
            id,
            cyrus_game::script::CombatEntryInputs {
                weapon: cyrus_game::script::WeaponInputs {
                    selected_player: id,
                    item_system_ready: true,
                    shell_execution: false,
                },
                nominal_ticks: 0,
                taunt_animation_admitted: true,
                selected_player_death_override: true,
            },
            &mut random,
            &mut playback,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        overridden.actors()[id.index]
            .control_state()
            .unwrap()
            .death_selector,
        6
    );
    assert_eq!(overridden.animation_frame(id).unwrap(), Some(10));
    assert!(playback.events.is_empty());
    assert_eq!(
        random.state(),
        1,
        "selected death override suppresses taunt RNG only"
    );
}

#[test]
fn combat_entry_and_weapon_cleanup_preserve_lifecycle_motion_and_selected_effects() {
    use cyrus_game::script::{CombatEntryInputs, CombatTauntPlayback, DialogueLine, WeaponInputs};
    struct NoTaunt;
    impl CombatTauntPlayback for NoTaunt {
        fn prepare(&mut self, _: [u8; 4], _: bool, _: Option<u32>) -> anyhow::Result<()> {
            anyhow::bail!("unexpected taunt")
        }
        fn release(&mut self, _: &mut Option<usize>) -> anyhow::Result<()> {
            anyhow::bail!("unexpected taunt")
        }
        fn load(&mut self, _: [u8; 4]) -> anyhow::Result<usize> {
            anyhow::bail!("unexpected taunt")
        }
        fn present(&mut self, _: usize, _: bool, _: Option<u32>) -> anyhow::Result<DialogueLine> {
            anyhow::bail!("unexpected taunt")
        }
    }
    for has_draw_group in [false, true] {
        for authored_weapon in [0, 1] {
            for selected in [false, true] {
                let mut scripts = cache();
                scripts.definition.attributes = (0..128).map(|i| format!("attr_{i}")).collect();
                let actor = scripts.actors.get_mut("LOGIC").unwrap();
                actor.attributes.insert("attr_14".into(), 1);
                actor.attributes.insert("attr_23".into(), authored_weapon);
                for instance in &mut scripts.instances {
                    instance.object_type = 1;
                }
                scripts.content_id = scripts.content_identity().unwrap();
                let mut groups = vec![
                    serde_json::json!({"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}),
                ];
                if has_draw_group {
                    groups.push(
                        serde_json::json!({"group_index":91,"anim_id":2,"anim_type":0,
                        "commands":[{"opcode":4,"sound_param":-17,"volume_shift":0},
                        {"opcode":0,"frame_reference":91},{"opcode":2,"value":0}]}),
                    );
                }
                let data = cyrus_assets::animation::Animations {
                    version: 2,
                    world: scripts.world,
                    metadata: serde_json::json!({"script_content_id":scripts.content_id,
                        "actors":[{"script_name":"LOGIC","model_load_path":"animation_table","animation_groups":groups}]}),
                    sections: Default::default(),
                    models: Default::default(),
                    unsupported: Default::default(),
                };
                let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
                let id = world.id(0).unwrap();
                let player = world.id(if selected { 0 } else { 1 }).unwrap();
                world
                    .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
                    .unwrap();
                world.set_actor_surface(id, 7).unwrap();
                let inputs = CombatEntryInputs {
                    weapon: WeaponInputs {
                        selected_player: player,
                        item_system_ready: true,
                        shell_execution: false,
                    },
                    nominal_ticks: 0,
                    taunt_animation_admitted: true,
                    selected_player_death_override: false,
                };
                if has_draw_group && authored_weapon != 0 && selected {
                    let mut failed = ScriptWorld::prepare(&scripts, 7).unwrap();
                    failed
                        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
                        .unwrap();
                    let mut random = cyrus_game::random::EngineRandom::from_state(123);
                    assert!(
                        failed
                            .begin_actor_combat(
                                id,
                                inputs,
                                &mut random,
                                &mut NoTaunt,
                                &mut |_, _| anyhow::bail!("injected cue failure")
                            )
                            .is_err()
                    );
                    assert!(
                        failed.actors()[0]
                            .failure()
                            .unwrap()
                            .contains("injected cue failure")
                    );
                    assert!(
                        failed
                            .begin_actor_combat(
                                id,
                                inputs,
                                &mut random,
                                &mut NoTaunt,
                                &mut |_, _| Ok(())
                            )
                            .is_err(),
                        "failed combat entry must not become an already-six success"
                    );
                }
                let mut random = cyrus_game::random::EngineRandom::from_state(123);
                let mut cues = Vec::new();
                world
                    .begin_actor_combat(
                        id,
                        inputs,
                        &mut random,
                        &mut NoTaunt,
                        &mut |receiver, sound| {
                            cues.push((receiver, sound));
                            Ok(())
                        },
                    )
                    .unwrap();
                let drawing = has_draw_group && authored_weapon != 0;
                let control = world.actors()[0].control_state().unwrap();
                assert_eq!(
                    (
                        control.death_selector,
                        control.living_state,
                        control.weapon_drawn
                    ),
                    (6, if drawing { 20 } else { 0 }, !drawing)
                );
                assert_eq!(control.script_control, if selected { 0 } else { 2 });
                assert_eq!(control.combat.as_ref().unwrap().flags, 4);
                assert_eq!(
                    world.animation_frame(id).unwrap(),
                    Some(if drawing { 91 } else { 10 })
                );
                assert_eq!(random.state(), 123, "no taunt consumes no random draw");
                let count = cues.len();
                assert_eq!(count, usize::from(drawing));
                world
                    .begin_actor_combat(
                        id,
                        inputs,
                        &mut random,
                        &mut NoTaunt,
                        &mut |receiver, sound| {
                            cues.push((receiver, sound));
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(
                    cues.len(),
                    count,
                    "already in combat does not repeat entry cues"
                );
                world
                    .cleanup_actor_weapon(id, player, 0, &mut |_, _| Ok(()))
                    .unwrap();
                let control = world.actors()[0].control_state().unwrap();
                assert_eq!(
                    (
                        control.death_selector,
                        control.living_state,
                        control.motion_direction,
                        control.weapon_drawn
                    ),
                    (6, 0, 0, false)
                );
                assert_eq!(
                    control.combat.as_ref().unwrap().flags,
                    if drawing { 4 } else { 0 },
                    "already-clear drawn flag preserves combat flag"
                );
                assert_eq!(
                    world.animation_frame(id).unwrap(),
                    Some(if drawing && !selected { 91 } else { 10 })
                );
                assert_eq!(
                    world.globals_mut().take_candle_show_request(),
                    selected && !drawing
                );
                assert!(!world.globals_mut().take_candle_show_request());
                world
                    .cleanup_actor_weapon(id, player, 0, &mut |_, _| Ok(()))
                    .unwrap();
                assert!(
                    !world.globals_mut().take_candle_show_request(),
                    "already clear does not reveal candle"
                );
                let stale = cyrus_game::script::ScriptActorId {
                    generation: 6,
                    ..player
                };
                assert!(
                    world
                        .cleanup_actor_weapon(id, stale, 0, &mut |_, _| Ok(()))
                        .is_err()
                );
            }
        }
    }
}

#[test]
fn native_combat_entry_is_synchronous_resultless_and_idempotent_without_taunts() {
    use cyrus_game::script::{CombatEntryInputs, ScriptInputs, ScriptStage, WeaponInputs};
    for (expression, derived, selected_receiver) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (false, true, true),
    ] {
        let call = Call {
            function_id: 4,
            function: "beginCombat".into(),
            target: if selected_receiver {
                Object::Player
            } else {
                Object::Me
            },
            kind: CallKind::Function,
            arguments: vec![],
        };
        let operation = if expression {
            Operation::Assign {
                destination: Place::Flag { index: 0 },
                formula: Formula {
                    first: Value::Call {
                        call: Box::new(call.clone()),
                    },
                    rest: vec![],
                },
            }
        } else if selected_receiver {
            Operation::ObjectCall { call: call.clone() }
        } else {
            Operation::Call { call: call.clone() }
        };
        let mut scripts = wait_cache(vec![
            operation,
            if selected_receiver {
                Operation::ObjectCall { call }
            } else {
                Operation::Call { call }
            },
            set_flag(42),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.push(Function {
            name: "beginCombat".into(),
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.definition.attributes = (0..128).map(|i| format!("attr_{i}")).collect();
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert("attr_14".into(), 1);
        for instance in &mut scripts.instances {
            instance.object_type = 1;
        }
        if derived {
            add_standalone_receivers(&mut scripts);
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 19).unwrap();
        let id = world.id(0).unwrap();
        let player = world.id(1).unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id": scripts.content_id,
                "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        let admission = CombatEntryInputs {
            weapon: WeaponInputs {
                selected_player: player,
                item_system_ready: true,
                shell_execution: false,
            },
            nominal_ticks: 0,
            taunt_animation_admitted: false,
            selected_player_death_override: false,
        };
        let hud = cyrus_game::script::ScriptHud::default();
        if derived {
            let models = cyrus_collision::actor_models::ActorModelCollision::new(
                &cyrus_assets::collision::ActorCollisionModels {
                    object_count: 2,
                    collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
                    ..Default::default()
                },
            )
            .unwrap();
            world.prepare_model_resources(&data, &models).unwrap();
        }
        let inputs = ScriptInputs {
            combat_entry: (!derived).then_some(admission),
            hud: Some(&hud),
            shell_execution: Some(false),
            nominal_ticks: Some(0),
            ..Default::default()
        };
        let result = world.advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 100);
        let control = world.actors()[0].control_state().unwrap();
        if selected_receiver {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("selected-player death override unavailable"),
                "{error}"
            );
            assert_eq!(
                (
                    control.death_selector,
                    control.script_control,
                    control.weapon_drawn
                ),
                (0, 0, false)
            );
            assert_eq!(control.combat.as_ref().unwrap().flags, 0);
            assert_ne!(world.flags(), [42]);
            assert!(
                world
                    .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 100)
                    .is_err()
            );
            continue;
        }
        assert_eq!(
            (
                control.death_selector,
                control.script_control,
                control.weapon_drawn
            ),
            (6, 2, true),
            "derived={derived} expression={expression} result={result:?}"
        );
        assert_eq!(control.combat.as_ref().unwrap().flags, 4);
        assert_eq!(world.actors()[1].control_state().unwrap().death_selector, 0);
        if expression {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("numeric"), "{error}");
            assert_ne!(world.flags(), [42]);
            assert!(
                world
                    .advance(id, ScriptStage::Body(Thread::Main), inputs, None, None, 100)
                    .is_err()
            );
        } else {
            result.unwrap();
            assert_eq!(
                world.flags(),
                [42],
                "statement continues after the resultless effect"
            );
        }
    }
}

#[test]
fn automatic_combat_ground_dispatch_preserves_pose_without_ordinary_support() {
    use cyrus_game::script::{
        CombatEntryInputs, OrdinaryInputs, ScriptHud, ScriptInputs, WeaponInputs,
    };
    for shell_context in [Some(false), None] {
        let mut scripts = wait_cache(vec![
            Operation::Call {
                call: Call {
                    function_id: 4,
                    function: "beginCombat".into(),
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![],
                },
            },
            set_flag(42),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.push(Function {
            name: "beginCombat".into(),
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.definition.attributes = (0..128).map(|i| format!("attr_{i}")).collect();
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        for (index, value) in [(1, 4), (6, 64), (14, 1), (23, 7), (115, 64), (119, 4)] {
            actor.attributes.insert(format!("attr_{index}"), value);
        }
        add_standalone_receivers(&mut scripts);
        for instance in &mut scripts.instances {
            instance.object_type = 1;
            instance.active = 1;
            instance.map_position = [10, 20, 30];
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({
                "script_content_id": scripts.content_id,
                "actors": [{"script_name":"LOGIC", "model_load_path":"animation_table",
                    "ravc_count":0, "mpsz_bounds_primary":0, "raex":{"unknown_12":0},
                    "raan_entries":[{"file_path":"BODY.3D","frame_count":1}],
                    "animation_groups":([0,91].map(|group| serde_json::json!({
                        "group_index":group,"anim_id":1,"anim_type":0,
                        "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
                    }))) }],
                "mpsz_entries":[{"total_extent":[2,2,2],"pos_extent":[1,1,1],
                    "neg_extent":[1,1,1],"center_offset":[0,0,0]}]
            }),
            sections: Default::default(),
            models: BTreeMap::from([(
                "BODY".into(),
                cyrus_assets::animation::AnimationModel {
                    radius: Some(1.0),
                    base_positions: vec![],
                    base_normals: vec![],
                    faces: vec![],
                    corner_normals: vec![],
                    encoding: "animated_i16".into(),
                    positions: vec![vec![]],
                    normals: vec![vec![]],
                },
            )]),
            unsupported: Default::default(),
        };
        let models = cyrus_collision::actor_models::ActorModelCollision::new(
            &cyrus_assets::collision::ActorCollisionModels {
                object_count: 2,
                collision_flags: BTreeMap::from([(0, 0), (1, 0)]),
                ..Default::default()
            },
        )
        .unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        world.prepare_model_resources(&data, &models).unwrap();
        let player = world.id(2).unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        let hud = ScriptHud::default();
        world
            .update_ordinary(
                &data,
                [10, 30],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        hud: Some(&hud),
                        shell_execution: shell_context,
                        combat_entry: Some(CombatEntryInputs {
                            weapon: WeaponInputs {
                                selected_player: player,
                                item_system_ready: true,
                                shell_execution: false,
                            },
                            nominal_ticks: 0,
                            taunt_animation_admitted: false,
                            selected_player_death_override: false,
                        }),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        for index in 0..2 {
            let actor = &world.actors()[index];
            if shell_context.is_none() {
                assert!(
                    actor
                        .failure()
                        .unwrap()
                        .contains("combat ground weapon admission unavailable")
                );
                continue;
            }
            assert!(actor.failure().is_none());
            let control = actor.control_state().unwrap();
            assert_eq!(
                (
                    control.death_selector,
                    control.living_state,
                    control.combat.as_ref().unwrap().flags
                ),
                (6, 20, 4)
            );
            assert_eq!(
                world
                    .spatial(world.id(index).unwrap())
                    .unwrap()
                    .local
                    .position,
                [2560, 5120, 7680]
            );
            assert_eq!(world.actor_surface(world.id(index).unwrap()).unwrap(), 0);
            assert_eq!(actor.attributes()[6], 64);
        }
        assert_eq!(world.flags(), [42]);
    }
}

fn flag_snapshot_scripts() -> Scripts {
    let mut scripts = cache();
    scripts.definition.flags = [("BOOL", 0), ("FLIPFLOP", 0), ("COUNTER", 2), ("NUMBER", -3)]
        .into_iter()
        .enumerate()
        .map(|(index, (kind, initial))| Flag {
            name: format!("Flag{index}"),
            flag_type: kind.into(),
            initial_value: initial,
        })
        .collect();
    scripts.definition.functions.push(Function {
        name: "SetGlobalFlag".into(),
        param_count: 1,
        kind: CallKind::Function,
    });
    let operations = (0..4)
        .map(|index| Operation::Call {
            call: Call {
                function_id: 1,
                function: "SetGlobalFlag".into(),
                target: Object::Me,
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value: index }],
            },
        })
        .chain(std::iter::once(Operation::End { target: 0 }));
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = operations
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
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn global_flag_snapshot_preserves_native_latches_without_executing_scripts() {
    use cyrus_game::script::GlobalFlagsSnapshot;
    let scripts = flag_snapshot_scripts();
    let mut source = ScriptWorld::prepare(&scripts, 1).unwrap();
    source
        .step(source.id(0).unwrap(), Thread::Main, 20)
        .unwrap();
    assert_eq!(source.flags(), [1, 1, 1, -3]);
    let bytes = serde_json::to_vec(&source.snapshot_flags()).unwrap();
    let snapshot: GlobalFlagsSnapshot = serde_json::from_slice(&bytes).unwrap();
    let mut restored = ScriptWorld::prepare(&scripts, 99).unwrap();
    restored.restore_flags(snapshot).unwrap();
    assert_eq!(restored.flags(), [1, 1, 1, -3]);
    assert_eq!(restored.actors()[0].runtime().unwrap().variables(), [4]);
    assert_eq!(restored.actors()[1].runtime().unwrap().variables(), [9]);
    restored
        .step(restored.id(0).unwrap(), Thread::Main, 20)
        .unwrap();
    // A restored Bool latch prevents a second toggle; FlipFlop still toggles.
    assert_eq!(restored.flags(), [1, 0, 0, -3]);
    assert_eq!(source.flags(), [1, 1, 1, -3]);
}

#[test]
fn global_flag_snapshot_retains_raw_values_and_mixed_latches() {
    let scripts = flag_snapshot_scripts();
    let mut source = ScriptWorld::prepare(&scripts, 1).unwrap();
    for (index, value) in [i32::MIN, -17, i32::MIN, i32::MAX].into_iter().enumerate() {
        source
            .assign_global_flag(index, value, index % 2 == 0)
            .unwrap();
    }
    let snapshot =
        serde_json::from_value(serde_json::to_value(source.snapshot_flags()).unwrap()).unwrap();
    let mut restored = ScriptWorld::prepare(&scripts, 2).unwrap();
    restored.restore_flags(snapshot).unwrap();
    assert_eq!(restored.flags(), [i32::MIN, -17, i32::MIN, i32::MAX]);
    for index in 0..4 {
        assert_eq!(restored.global_flag_is_set(index).unwrap(), index % 2 == 0);
    }
    restored
        .step(restored.id(0).unwrap(), Thread::Main, 20)
        .unwrap();
    assert_eq!(restored.flags(), [i32::MIN, 0, i32::MAX, i32::MAX]);
    for index in 0..4 {
        assert!(restored.global_flag_is_set(index).unwrap());
    }
}

#[test]
fn global_flag_snapshot_rejects_incompatible_schema_before_any_write() {
    use serde_json::json;
    let scripts = flag_snapshot_scripts();
    let source = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = serde_json::to_value(source.snapshot_flags()).unwrap();
    let mut destination = ScriptWorld::prepare(&scripts, 2).unwrap();
    for index in 0..4 {
        destination
            .assign_global_flag(index, 40 + index as i32, true)
            .unwrap();
    }
    let before = serde_json::to_value(destination.snapshot_flags()).unwrap();
    let mut cases = vec![];
    let mut version = snapshot.clone();
    version["version"] = json!(2);
    cases.push(version);
    let mut short = snapshot.clone();
    short["flags"].as_array_mut().unwrap().pop();
    cases.push(short);
    let mut long = snapshot.clone();
    long["flags"]
        .as_array_mut()
        .unwrap()
        .push(snapshot["flags"][0].clone());
    cases.push(long);
    let mut reordered = snapshot.clone();
    reordered["flags"].as_array_mut().unwrap().swap(0, 3);
    cases.push(reordered);
    for (field, value) in [
        ("name", json!("Other")),
        ("flag_type", json!("BOOL")),
        ("initial_value", json!(30)),
    ] {
        let mut changed = snapshot.clone();
        changed["flags"][3]["definition"][field] = value;
        cases.push(changed);
    }
    for corrupt in cases {
        let error = destination
            .restore_flags(serde_json::from_value(corrupt).unwrap())
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("world 7 global flag restoration") && message.contains("snapshot"),
            "{message}"
        );
        assert_eq!(
            serde_json::to_value(destination.snapshot_flags()).unwrap(),
            before
        );
    }
}

#[test]
fn global_flag_snapshot_empty_schema_and_malformed_records_are_explicit() {
    use cyrus_game::script::GlobalFlagsSnapshot;
    use serde_json::json;
    let mut scripts = flag_snapshot_scripts();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let snapshot = serde_json::to_value(world.snapshot_flags()).unwrap();
    for (field, invalid) in [("value", json!(2147483648u64)), ("latched", json!(1))] {
        let mut corrupt = snapshot.clone();
        corrupt["flags"][0][field] = invalid;
        assert!(serde_json::from_value::<GlobalFlagsSnapshot>(corrupt).is_err());
    }
    let mut missing = snapshot.clone();
    missing["flags"][0]
        .as_object_mut()
        .unwrap()
        .remove("latched");
    assert!(serde_json::from_value::<GlobalFlagsSnapshot>(missing).is_err());
    scripts.definition.flags.clear();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut empty = ScriptWorld::prepare(&scripts, 2).unwrap();
    empty
        .restore_flags(
            serde_json::from_value(serde_json::to_value(empty.snapshot_flags()).unwrap()).unwrap(),
        )
        .unwrap();
    assert!(empty.flags().is_empty());
    assert!(
        empty
            .restore_flags(serde_json::from_value(snapshot).unwrap())
            .is_err()
    );
    assert!(empty.flags().is_empty());
}

#[test]
fn global_flag_snapshot_round_trips_the_installed_schema() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (global flag snapshots)");
        return;
    };
    let definition =
        rgpre::import::soup_def::try_load_soup_def(&install.root).expect("installed SOUP386.DEF");
    assert!(!definition.flags.is_empty());
    let mut scripts = flag_snapshot_scripts();
    scripts.definition.flags = definition
        .flags
        .into_iter()
        .map(|flag| Flag {
            name: flag.name,
            flag_type: flag.flag_type,
            initial_value: flag.initial_value,
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut source = ScriptWorld::prepare(&scripts, 1).unwrap();
    let values: Vec<_> = (0..scripts.definition.flags.len())
        .map(|index| i32::MIN + index as i32)
        .collect();
    for (index, value) in values.iter().enumerate() {
        source
            .assign_global_flag(index, *value, index % 2 == 0)
            .unwrap();
    }
    let snapshot =
        serde_json::from_slice(&serde_json::to_vec(&source.snapshot_flags()).unwrap()).unwrap();
    let mut restored = ScriptWorld::prepare(&scripts, 2).unwrap();
    restored.restore_flags(snapshot).unwrap();
    assert_eq!(restored.flags(), values);
    for index in 0..values.len() {
        assert_eq!(restored.global_flag_is_set(index).unwrap(), index % 2 == 0);
    }
}

#[test]
fn standalone_player_spatial_initialization_is_generation_bound_and_once_only() {
    let mut scripts = cache();
    add_standalone_receivers(&mut scripts);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    let alternate = world.id(3).unwrap();
    let camera = world.id(4).unwrap();
    assert!(world.spatial(player).is_err());
    assert!(
        world
            .initialize_player_spatial(camera, [1, 2, 3], [0; 3])
            .is_err()
    );
    assert!(
        world
            .initialize_player_spatial(player, [1, 2, 3], [0, 2048, 0])
            .is_err()
    );
    assert!(world.spatial(player).is_err());
    world
        .initialize_player_spatial(player, [101, -202, 303], [0, 668, 0])
        .unwrap();
    world
        .initialize_player_spatial(alternate, [404, -505, 606], [0, 1185, 0])
        .unwrap();
    let state = world.spatial(player).unwrap();
    assert_eq!(state.query_pose().unwrap().position, [101, -202, 303]);
    assert_eq!(state.local.angles, [0, 668, 0]);
    assert_eq!(state.previous_local_position, [101, -202, 303]);
    assert_eq!(state.angle_history, [0, 668, 0]);
    assert_eq!(state.water_rollback_position, None);
    assert!(
        world
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .is_err()
    );
    world.refresh_families().unwrap();
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [101, -202, 303]
    );
    assert_eq!(
        world.spatial(alternate).unwrap().local.position,
        [404, -505, 606]
    );
    let mut replacement = ScriptWorld::prepare(&scripts, 8).unwrap();
    replacement.prepare_spatial().unwrap();
    assert!(
        replacement
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .is_err()
    );
}

fn player_walking_fixture() -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_game::player::data::PlayerScene,
) {
    use cyrus_assets::animation::AnimationModel;
    use cyrus_game::script::ScriptActorSource;
    let mut scripts = cache();
    add_standalone_receivers(&mut scripts);
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    scripts
        .actors
        .get_mut("CYRUS")
        .unwrap()
        .attributes
        .insert("attr_1".into(), 15);
    scripts.definition.attributes.resize(203, String::new());
    for (index, name, value) in [
        (14, "attr_combat", 1),
        (119, "attr_normal_collision", 15),
        (200, "attr_forward_walk", 1),
        (201, "attr_forward_speed", 24),
        (202, "attr_step_type", 1),
    ] {
        scripts.definition.attributes[index] = name.into();
        scripts
            .actors
            .get_mut("CYRUS")
            .unwrap()
            .attributes
            .insert(name.into(), value);
    }
    let function_id = u16::try_from(scripts.definition.functions.len()).unwrap();
    scripts.definition.functions.push(Function {
        name: "SetMyAttr".into(),
        param_count: 2,
        kind: CallKind::Function,
    });
    scripts
        .actors
        .get_mut("CYRUS")
        .unwrap()
        .program
        .steps
        .get_mut(&0)
        .unwrap()
        .operation = Operation::Call {
        call: Call {
            function_id,
            function: "SetMyAttr".into(),
            target: Object::Me,
            kind: CallKind::Function,
            arguments: vec![Value::Integer { value: 200 }, Value::Integer { value: 2 }],
        },
    };
    let hook_id = u16::try_from(scripts.definition.functions.len()).unwrap();
    scripts.definition.functions.push(Function {
        name: "playermain".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("CYRUS").unwrap().native_hook = Some(Call {
        function_id: hook_id,
        function: "playermain".into(),
        target: Object::Me,
        kind: CallKind::Function,
        arguments: vec![],
    });
    for placement in &mut scripts.instances {
        placement.object_type = 1;
        placement.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (mut world, mut data, models) =
        static_model_fixture_session(scripts.clone(), None, [10, 20, 30], true);
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "CYRUS")
        .unwrap();
    *definition = serde_json::json!({"script_name":"CYRUS", "model_load_path":"animation_table",
        "attributes":{"attr_forward_walk":9,"attr_forward_speed":99,"attr_step_type":1},
        "ravc_count":0,"mpsz_bounds_primary":0,"raex":{"unknown_12":256},
        "raan_entries":[{"file_path":"PLAYER.3D","frame_count":1}],
        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
            {"group_index":20,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":4,"sound_param":9,"volume_shift":0},
                {"opcode":0,"frame_reference":0},{"opcode":1}]}]});
    data.metadata["mpsz_entries"] = serde_json::json!([{"total_extent":[24,48,24],
        "pos_extent":[12,24,12],"neg_extent":[12,24,12],"center_offset":[0,0,0],"flags":0}]);
    data.models.insert(
        "PLAYER".into(),
        AnimationModel {
            radius: Some(31.0),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![]],
            normals: vec![vec![]],
        },
    );
    world
        .initialize_player_spatial(player, [2560, -1536, 7680], [0; 3])
        .unwrap();
    assert!(world.begin_player_movement_update(player, 0).is_err());
    world
        .initialize_player_animation(player, &data, 0, &mut |_| Ok(()))
        .unwrap();
    world.prepare_player_model_resources(player, &data).unwrap();
    assert!(world.prepare_player_model_resources(player, &data).is_err());
    world
        .refresh_model_presentations(&data, &[map, player])
        .unwrap();
    assert_eq!(world.model_radius(player).unwrap(), Some(31.0));
    assert_eq!(
        world.live_collision_inputs(&[player, map]).unwrap().len(),
        1
    );
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 4;
    scene.definition = Some(
        cyrus_game::player::definition::PlayerDefinition::from_metadata(&data.metadata).unwrap(),
    );
    (scripts, world, data, scene)
}

#[test]
fn keyboard_walk_runs_through_ordinary_scheduler_and_projects_release() {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    use cyrus_assets::animation::Animations;
    use cyrus_game::{
        AppState,
        player::{
            Player, PlayerPlugin,
            input::{PlayerBindings, PlayerInput, PlayerToggles},
            placement::PlayerPose,
        },
        script::{ScriptActors, ScriptPlugin, ScriptView},
    };
    use cyrus_world::{animation::AnimationRequest, lifecycle::PreparedWorld, sound::SoundCue};

    let (scripts, mut world, mut data, mut scene) = player_walking_fixture();
    let alternate = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "GREMLIN")
        .unwrap();
    alternate["raan_entries"] = serde_json::json!([
        {"file_path":"PLAYER.3D","frame_count":1}
    ]);
    alternate["animation_groups"] = serde_json::json!([
        {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
            {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}
        ]}
    ]);
    scene.collision.model_visibility = Some(Default::default());
    world
        .prepare_model_admission(
            scene.collision.model_visibility.as_ref().unwrap(),
            scene.collision.placements.len(),
        )
        .unwrap();
    let player_id = world
        .id(world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, cyrus_game::script::ScriptActorSource::Player))
            .unwrap())
        .unwrap();
    data.metadata["markers"] =
        serde_json::json!([{"map_position":[10,-6,30],"position":[-10.0,6.0,30.0],"reserved":1}]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<cyrus_game::fidelity::Fidelity>()
        .add_message::<SoundCue>()
        .add_message::<cyrus_world::animation::AnimationTick>()
        .add_plugins((PlayerPlugin, ScriptPlugin));
    app.insert_resource(PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 46, 32]).unwrap());
    app.insert_resource(PlayerToggles {
        walk_mode: true,
        auto_defend: false,
    });
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(55),
    ));
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let scene_handle = app
        .world()
        .resource::<AssetServer>()
        .load::<cyrus_game::player::data::PlayerScene>("keyboard-test.player");
    app.world_mut()
        .resource_mut::<Assets<cyrus_game::player::data::PlayerScene>>()
        .insert(scene_handle.id(), scene)
        .unwrap();
    app.insert_resource(PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(world.world),
        scripts,
        animations: animations.clone(),
        player_path: "keyboard-test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: [2560, 0, 7680],
    });
    app.update();
    app.world_mut().resource_mut::<ScriptActors>().current = Some(world);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let position = |app: &App| {
        let actors = app.world().resource::<ScriptActors>();
        assert!(
            !matches!(
                app.world().resource::<cyrus_game::script::ScriptStatus>(),
                cyrus_game::script::ScriptStatus::Failed { .. }
            ),
            "{:?}",
            app.world().resource::<cyrus_game::script::ScriptStatus>()
        );
        let world = actors.current.as_ref().unwrap();
        assert!(
            world.actors()[player_id.index].failure().is_none(),
            "{:?}",
            world.actors()[player_id.index].failure()
        );
        world.spatial(player_id).unwrap().local.position
    };
    let initial = position(&app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);
    app.update();
    assert!(app.world().resource::<PlayerInput>().forward);
    let moving = position(&app);
    assert!(moving[2] > initial[2], "{initial:?} -> {moving:?}");
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap().position,
        moving.map(i64::from)
    );
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert!(cues.iter().any(|cue| matches!(cue, SoundCue::Animation { actor, sound: cyrus_world::animation::AnimationSound::Play { effect: 9, .. }, surface: Some(0), .. } if *actor == player)), "{cues:?}");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ArrowUp);
    for _ in 0..12 {
        app.update();
        position(&app);
    }
    assert!(!app.world().resource::<PlayerInput>().forward);
    let stopped = position(&app);
    app.update();
    assert_eq!(position(&app), stopped);
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap().position,
        stopped.map(i64::from)
    );
}

#[test]
fn selected_player_collision_uses_logical_pose_and_live_map_support() {
    use cyrus_collision::ground::SupportSource;
    use cyrus_game::script::{OrdinaryGroundQuery, ScriptActorSource};
    let (_, mut world, data, scene) = player_walking_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
        .unwrap();
    assert_eq!(
        world
            .ordinary_collision_shape(player)
            .unwrap()
            .positive_extent,
        [12, 24, 12]
    );
    world.begin_player_movement_update(player, 0).unwrap();
    let support = world
        .query_ordinary_ground(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        support.source(),
        SupportSource::Actor {
            actor: map.index,
            face: 0
        }
    );
    let fallback = world
        .query_ordinary_ground(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 1,
                sidedness: false,
            },
        )
        .unwrap()
        .unwrap();
    assert!(
        matches!(fallback, cyrus_collision::ground::SelectedSupport::Fallback {
        source: SupportSource::Actor { actor, face: 0 }, actor_height: -1536, ..
    } if actor == map.index)
    );
    let before = world.spatial(player).unwrap().local.position;
    let mut history = cyrus_collision::ground::SupportHistory::new(0, 0);
    assert!(
        world
            .query_player_forward_support(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                before,
                &mut history,
                false
            )
            .unwrap()
    );
    assert_eq!(world.spatial(player).unwrap().local.position, before);
    assert_eq!(world.actor_surface(player).unwrap(), 0);
    assert_eq!(history.ticks(), 12);
    let spheres = world.ordinary_overlap_spheres(player, &data).unwrap();
    assert!(!spheres.is_empty());
    let contact = cyrus_collision::collision_world::RuntimeBodyContact {
        source: SupportSource::Actor {
            actor: map.index,
            face: 0,
        },
        point: [0; 3],
        normal: [256, 0, 0],
        classification: 1,
    };
    world
        .resolve_ordinary_residual_contacts(player, &[contact], &scene.settings)
        .unwrap();
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [2816, -1536, 7680]
    );
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .body_response_flags,
        4
    );
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 4,
        sidedness: false,
    };
    let timing = cyrus_game::clock::ClockStep {
        frame_scale: 256,
        nominal_ticks: 0,
        fine_ticks: 0,
    };
    let mut cues = Vec::new();
    world.finish_player_movement_update(player).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    assert!(world.begin_player_movement_update(player, 0).is_err());
    assert!(
        world
            .advance_player_controller(
                player,
                query(),
                timing,
                cyrus_game::script::PlayerControllerInput {
                    held: &cyrus_game::player::input::PlayerInput {
                        forward: true,
                        ..Default::default()
                    },
                    walk_mode: true,
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |id, cue| {
                    cues.push((id, cue));
                    Ok(())
                }
            )
            .unwrap()
    );
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [2816, -1536, 7744]
    );
    assert!(
        world
            .advance_player_walking(player, query(), timing, true, &mut |_, _| {
                panic!("already accepted update requested animation")
            })
            .unwrap()
    );
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [2816, -1536, 7744]
    );
    let control = world.actors()[index].control_state().unwrap();
    assert_eq!(
        (
            control.motion_classification,
            control.motion_direction,
            control.movement_flags
        ),
        (6, 2, 0x30)
    );
    assert!(cues.iter().any(|(id, cue)| *id == player
        && matches!(
            cue,
            cyrus_game::script::ScriptSound::Animation {
                sound: cyrus_world::animation::AnimationSound::Play { .. },
                surface: Some(0),
                position: Some([2816, -1536, 7680])
            }
        )));
    world.finish_player_movement_update(player).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    assert!(
        world
            .advance_player_walking(player, query(), timing, true, &mut |_, _| panic!(
                "continuation requested animation"
            ))
            .unwrap()
    );
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [2816, -1536, 7829]
    );
    world.finish_player_movement_update(player).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    world
        .advance(
            player,
            cyrus_game::script::ScriptStage::Body(Thread::Main),
            cyrus_game::script::ScriptInputs {
                frame_scale: Some(timing.frame_scale),
                nominal_ticks: Some(timing.nominal_ticks),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[index].attributes()[200], 2);
    assert!(
        world
            .advance_player_walking(player, query(), timing, true, &mut |_, _| {
                panic!("continuation requested animation")
            })
            .unwrap()
    );
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [2816, -1536, 8085]
    );
    world.finish_player_movement_update(player).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    world.finish_player_movement_update(player).unwrap();
    assert!(world.finish_player_movement_update(player).is_err());
    world.begin_player_movement_update(player, 0).unwrap();
    assert!(
        world
            .advance_player_walking(player, query(), timing, true, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        [2816, -1536, 8213]
    );
    world.finish_player_movement_update(player).unwrap();
    assert!(world.ordinary_actors([10, 30]).unwrap().contains(&map));
    let held = cyrus_game::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    world
        .update_ordinary(
            &data,
            [10, 30],
            cyrus_game::script::OrdinaryInputs {
                scene: Some(&scene),
                scripts: cyrus_game::script::ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    shell_execution: Some(false),
                    player_controller: Some(cyrus_game::script::PlayerControllerInput {
                        held: &held,
                        walk_mode: true,
                    }),
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(
        world.actors()[index].failure().is_none(),
        "{:?}",
        world.actors()[index].failure()
    );
    assert_eq!(world.spatial(player).unwrap().local.position[2], 8383);
    let combat = world.actors()[index]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (combat.decision.attack_state, combat.action, combat.cooldown),
        (0, -1, 0)
    );
    assert_eq!(combat.retained_action_indices, [-1; 2]);
    let released = cyrus_game::player::input::PlayerInput::default();
    let mut released_positions = Vec::new();
    for _ in 0..8 {
        world
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_game::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        shell_execution: Some(false),
                        player_controller: Some(cyrus_game::script::PlayerControllerInput {
                            held: &released,
                            walk_mode: true,
                        }),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
        released_positions.push(world.spatial(player).unwrap().local.position[2]);
    }
    assert_eq!(released_positions[0], 8383 + 256);
    assert_eq!(
        released_positions[6], released_positions[7],
        "{released_positions:?}"
    );
    world.begin_player_movement_update(player, 0).unwrap();
    world.finish_player_movement_update(player).unwrap();
    let flags = world.flags().to_vec();
    let error = world
        .run_player_native_hook(player, &mut |_| anyhow::bail!("controller failure witness"))
        .unwrap_err();
    assert!(format!("{error:#}").contains("controller failure witness"));
    assert!(world.actors()[index].runtime().is_some());
    assert_eq!(world.flags(), flags);
    assert!(
        world
            .run_player_native_hook(player, &mut |_| panic!("failed native hook replayed"))
            .is_err()
    );
}

#[test]
fn restored_alternate_entity_survives_ordinary_binding_updates() {
    use bevy::prelude::*;
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptActorSource, ScriptActors, ScriptBinding, ScriptPlugin};
    use cyrus_world::{animation::AnimationRequest, lifecycle::PreparedWorld};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .add_message::<cyrus_world::animation::AnimationTick>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .insert_resource(State::new(cyrus_game::AppState::InGame))
        .add_plugins(ScriptPlugin);
    let mut data = cache();
    add_standalone_receivers(&mut data);
    let animations = app.world_mut().resource_mut::<Assets<Animations>>().add(Animations {
        version: 2, world: data.world,
        metadata: serde_json::json!({"actors": [], "markers": [], "walk_node_maps": [], "script_content_id": data.content_id}),
        sections: BTreeMap::new(), models: BTreeMap::new(), unsupported: BTreeMap::new(),
    });
    let world_id = data.world;
    let scripts = app.world_mut().resource_mut::<Assets<Scripts>>().add(data);
    app.insert_resource(PreparedWorld {
        requested_name: "TEST".into(),
        world: Some(world_id),
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    })
    .insert_resource(AnimationRequest(animations.clone()));
    let player = app
        .world_mut()
        .spawn((cyrus_game::player::Player, Transform::default()))
        .id();
    app.update();
    app.update();
    let current = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap();
    let index = current
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::AlternatePlayer))
        .unwrap();
    let actor = current.id(index).unwrap();
    let candidate = current.prepare_entity_binding(actor, animations).unwrap();
    let old: Vec<_> = app
        .world_mut()
        .query::<(Entity, &ScriptBinding)>()
        .iter(app.world())
        .filter_map(|(entity, binding)| (binding.actor == actor).then_some(entity))
        .collect();
    assert_eq!(old.len(), 1);
    app.world_mut().despawn(old[0]);
    let replacement = app.world_mut().spawn(Transform::default()).id();
    let mut queue = bevy::ecs::world::CommandQueue::default();
    candidate.queue(
        &mut Commands::new(&mut queue, app.world()),
        replacement,
        player,
    );
    queue.apply(app.world_mut());
    app.update();
    app.update();
    let retained: Vec<_> = app
        .world_mut()
        .query::<(Entity, &ScriptBinding)>()
        .iter(app.world())
        .filter_map(|(entity, binding)| (binding.actor == actor).then_some(entity))
        .collect();
    assert_eq!(retained, [replacement]);
    assert_eq!(
        app.world().get::<ChildOf>(replacement).unwrap().parent(),
        player
    );
}

#[test]
fn authored_light_off_survives_world_replacement_without_reenabling() {
    use cyrus_game::script::{
        GameplayGlobals, ScriptInputs, ScriptStage, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = random_scripts("LightOff", vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.instances.truncate(1);
    let placement = &mut scripts.instances[0];
    placement.object_type = 6;
    placement.light_intensity = 211;
    placement.light_radius = 900;
    placement.light_rgb = [-12, 34, 56];
    placement.map_position = [100, -200, 300];
    scripts.content_id = scripts.content_identity().unwrap();
    let (_, _, animations) = revisit_test_app(&scripts);
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &animations,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let lights = world.object_lights();
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].position, [100, -200, 300]);
    assert_eq!(lights[0].radius, 900);
    assert_eq!(lights[0].rgb, [-12, 34, 56]);
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap();
    assert!(world.object_lights().is_empty());
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    let snapshot = world.snapshot(&sources).unwrap();
    let snapshot = serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let replacement = ScriptWorld::from_snapshot(
        snapshot,
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: GameplayGlobals::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    assert!(replacement.object_lights().is_empty());
    assert_eq!(replacement.actors()[0].runtime().unwrap().variables(), [0]);
    let mut saved = serde_json::to_value(replacement.snapshot(&sources).unwrap()).unwrap();
    saved["actors"][0]["light"]["position"][0] = 101.into();
    assert!(
        ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 3,
                globals: GameplayGlobals::default(),
                samples: &BTreeMap::new()
            },
        )
        .is_err()
    );
}

#[test]
fn light_off_without_a_record_returns_zero_without_creating_a_light() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = random_scripts("LightOff", vec![], Object::Me);
    scripts.definition.functions[1].param_count = 0;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs::default(),
            None,
            None,
            20,
        )
        .unwrap();
    assert!(world.object_lights().is_empty());
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
}

#[test]
fn separate_light_cleanup_runs_with_tasks_disabled_and_uses_wrapping_byte_ticks() {
    use cyrus_game::script::{ScriptInputs, ScriptStage, WorldSnapshotSources};
    for (ticks, retained) in [(32, true), (33, false), (256, true)] {
        let mut scripts = random_scripts("LightOff", vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        scripts.updates.task_system = false;
        scripts.instances.truncate(1);
        scripts.instances[0].object_type = 6;
        scripts.instances[0].map_position = [600, 0, 600];
        scripts.content_id = scripts.content_identity().unwrap();
        let (_, _, animations) = revisit_test_app(&scripts);
        let collision = cyrus_assets::collision::CollisionScene::default();
        let sources = WorldSnapshotSources {
            player: None,
            scripts: &scripts,
            animations: &animations,
            collision: &collision,
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs::default(),
                None,
                None,
                20,
            )
            .unwrap();
        world
            .update_separate(
                [600, 600],
                ScriptInputs {
                    nominal_ticks: Some(ticks),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(world.actors()[0].failure().is_none());
        let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        assert_eq!(
            !saved["actors"][0]["light"].is_null(),
            retained,
            "ticks {ticks}"
        );
        assert!(world.object_lights().is_empty());
    }
}

#[test]
fn installed_authored_lights_reach_logical_render_projection_in_map_order() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (authored logical lights)");
        return;
    }
    let mut count = 0;
    for entry in std::fs::read_dir(root.join("maps")).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rgm"))
        {
            continue;
        }
        let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(&path).unwrap()).unwrap();
        let expected: Vec<_> = map
            .sections
            .iter()
            .filter_map(|section| match section {
                rgpre::import::rgm::RgmSection::MpobParsed(_, records) => Some(records),
                _ => None,
            })
            .flatten()
            .filter(|record| record.object_type == 6)
            .collect();
        let scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
        let world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let lights = world.object_lights();
        assert_eq!(
            lights.len(),
            expected.len(),
            "{} light census",
            path.display()
        );
        for (light, record) in lights.iter().zip(expected) {
            assert_eq!(
                light.radius,
                record.radius,
                "{} light radius",
                path.display()
            );
            assert_eq!(
                light.rgb,
                [record.red, record.green, record.blue],
                "{} light RGB",
                path.display()
            );
        }
        count += lights.len();
    }
    assert!(count > 0, "installed maps must exercise authored lights");
}

#[test]
fn player_looking_uses_signed_tolerance_and_quantized_position_bearing() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [0; 3];
    for (position, heading, tolerance, expected) in [
        ([0, 900, 100], 1024, 0, 1),
        ([0, -900, 100], 1023, 0, 0),
        ([0, 0, 100], 1023, 1, 1),
        ([100, 0, 100], 768, 0, 1),
        ([100, 0, 100], 770, 1, 0),
        ([0, 0, 100], 1024, -1, 0),
        ([i32::MIN, 0, 1], 1024, 0, 1),
        ([i32::MIN, 0, i32::MIN], 0, 0, 1),
    ] {
        let inputs = QueryInputs {
            player: Some(PlayerQuery { position, heading }),
            distance_override: None,
            fallback_position: None,
            linked_position: Some(None),
        };
        assert_eq!(
            actor
                .query("PlayerLooking", &[Argument::Integer(tolerance)], inputs)
                .unwrap(),
            Some(expected),
            "{position:?}/{heading}/{tolerance}"
        );
    }
}

#[test]
fn player_looking_preserves_attached_then_linked_position_priority() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [100, 0, 0];
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [0, 0, 100],
            heading: 1024,
        }),
        distance_override: None,
        fallback_position: None,
        linked_position: Some(Some([0; 3])),
    };
    let args = [Argument::Integer(0)];
    assert_eq!(
        actor.query("PlayerLooking", &args, inputs).unwrap(),
        Some(1)
    );
    inputs.linked_position = Some(None);
    assert_eq!(
        actor.query("PlayerLooking", &args, inputs).unwrap(),
        Some(0)
    );
    inputs.linked_position = None;
    assert!(actor.query("PlayerLooking", &args, inputs).is_err());
    actor.parent = Some(1);
    actor.effective = None;
    assert!(actor.query("PlayerLooking", &args, inputs).is_err());
    let mut effective = actor.local;
    effective.position = [0; 3];
    actor.effective = Some(effective);
    assert_eq!(
        actor.query("PlayerLooking", &args, inputs).unwrap(),
        Some(1)
    );
    inputs.player = None;
    assert!(actor.query("PlayerLooking", &args, inputs).is_err());
    assert!(actor.query("PlayerLooking", &[], inputs).is_err());
}

#[test]
fn player_looking_dispatch_returns_result_to_script_local() {
    use cyrus_game::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    for (heading, expected) in [(1024, 1), (1023, 0)] {
        let scripts = random_scripts(
            "PlayerLooking",
            vec![Value::Integer { value: 0 }],
            Object::Me,
        );
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let origin = world.spatial(id).unwrap().local.position;
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [origin[0], origin[1], origin[2].wrapping_add(100)],
                            heading,
                        }),
                        distance_override: None,
                        fallback_position: None,
                        linked_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
    }
}

#[test]
fn player_stand_reads_selected_player_support_without_changing_it() {
    use cyrus_game::script::ModelSupportOwners;
    for (support_index, receiver, expected) in [
        (Some(0), Object::Me, 1),
        (Some(1), Object::Me, 0),
        (None, Object::Me, 0),
        (Some(2), Object::Player, 1),
    ] {
        let mut scripts = random_scripts("PlayerStand", vec![], receiver);
        scripts.definition.functions[1].param_count = 0;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let player = world.id(2).unwrap();
        let support = ModelSupportOwners {
            actor: support_index.map(|index| world.id(index).unwrap()),
            static_placement: None,
        };
        world.set_model_support(player, support).unwrap();
        world.step(caller, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
        assert_eq!(world.model_support(player).unwrap(), support);
    }
}

#[test]
fn player_stand_rejects_missing_player_and_arguments() {
    for (arguments, player, expected) in [
        (vec![], false, "selected player unavailable"),
        (
            vec![Value::Integer { value: 0 }],
            true,
            "requires no arguments",
        ),
    ] {
        let mut scripts = random_scripts("PlayerStand", arguments, Object::Me);
        if !player {
            scripts.definition.functions[1].param_count = 0;
        }
        scripts.content_id = scripts.content_identity().unwrap();
        if player {
            add_standalone_receivers(&mut scripts);
        }
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        let error = world.step(id, Thread::Main, 20).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }
}

#[test]
fn player_stand_validates_explicit_selected_identity() {
    use cyrus_game::script::{ActivationInputs, ModelSupportOwners, ScriptInputs, ScriptStage};
    for (selected_index, stale, expected_error) in [
        (2, false, None),
        (2, true, Some("stale script actor generation")),
        (3, false, Some("selected-player switching unsupported")),
    ] {
        let mut scripts = random_scripts("PlayerStand", vec![], Object::Me);
        scripts.definition.functions[1].param_count = 0;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let mut selected = world.id(selected_index).unwrap();
        world
            .set_model_support(
                selected,
                ModelSupportOwners {
                    actor: Some(caller),
                    static_placement: None,
                },
            )
            .unwrap();
        if stale {
            selected.generation += 1;
        }
        let inputs = ActivationInputs {
            player: selected,
            actors: BTreeMap::new(),
            labels: BTreeMap::new(),
            lockout: 0,
            dialogue_open: false,
            lineup_result: 0,
            use_held: false,
            item_system_ready: false,
            shell_execution: false,
            render_disabled: false,
        };
        let result = world.advance(
            caller,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                activation: Some(&inputs),
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        );
        if let Some(expected) = expected_error {
            let error = result.unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        } else {
            result.unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [1]);
            assert_eq!(world.model_support(selected).unwrap().actor, Some(caller));
        }
    }
}

#[test]
fn player_arc_uses_receiver_heading_signed_offset_and_inclusive_tolerance() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    for (position, yaw, offset, tolerance, expected) in [
        ([0, 900, 100], 0, 0, 0, 1),
        ([100, 0, 0], 1536, 0, 0, 1),
        ([-100, 0, 0], 512, 0, 0, 1),
        ([0, 0, -100], 1024, 0, 0, 1),
        ([100, 0, 100], 1792, 0, 0, 1),
        ([0, 0, 100], 10, 10, 0, 1),
        ([0, 0, 100], 2047, 0, 1, 1),
        ([0, 0, 100], 2047, 0, 0, 0),
        ([0, 0, 100], 0, 0, -1, 0),
        ([0, 0, 100], 10, 0, 10, 1),
        ([0, 0, 100], 10, 0, 9, 0),
        ([0, 0, 100], 0, i32::MAX, 1, 1),
        ([0, 0, 100], 0, i32::MAX, 0, 0),
        ([0, 0, 100], 0, i32::MIN, 0, 1),
    ] {
        actor.local.position = position;
        actor.local.angles[1] = yaw;
        let inputs = QueryInputs {
            player: Some(PlayerQuery {
                position: [0, -900, 0],
                heading: 777,
            }),
            linked_position: Some(None),
            distance_override: None,
            fallback_position: None,
        };
        assert_eq!(
            actor
                .query(
                    "PlayerArc",
                    &[Argument::Integer(offset), Argument::Integer(tolerance)],
                    inputs
                )
                .unwrap(),
            Some(expected),
            "{position:?}/{yaw}/{offset}/{tolerance}"
        );
    }
}

#[test]
fn player_arc_distinguishes_linked_position_from_attached_heading() {
    use cyrus_game::script::{PlayerQuery, QueryInputs};
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    world.prepare_spatial().unwrap();
    let mut actor = world.spatial(world.id(0).unwrap()).unwrap().clone();
    actor.local.position = [100, 0, 0];
    actor.local.angles[1] = 0;
    let args = [Argument::Integer(0), Argument::Integer(0)];
    let mut inputs = QueryInputs {
        player: Some(PlayerQuery {
            position: [0; 3],
            heading: 777,
        }),
        linked_position: Some(None),
        distance_override: None,
        fallback_position: None,
    };
    assert_eq!(actor.query("PlayerArc", &args, inputs).unwrap(), Some(0));
    inputs.linked_position = Some(Some([0, 0, 100]));
    assert_eq!(actor.query("PlayerArc", &args, inputs).unwrap(), Some(1));
    actor.parent = Some(1);
    let mut effective = actor.local;
    effective.position = [0, 0, -100];
    effective.angles[1] = 1024;
    actor.effective = Some(effective);
    assert_eq!(actor.query("PlayerArc", &args, inputs).unwrap(), Some(1));
    actor.effective = None;
    assert!(actor.query("PlayerArc", &args, inputs).is_err());
    actor.parent = None;
    inputs.linked_position = None;
    assert!(actor.query("PlayerArc", &args, inputs).is_err());
    inputs.linked_position = Some(None);
    inputs.player = None;
    assert!(actor.query("PlayerArc", &args, inputs).is_err());
    assert!(actor.query("PlayerArc", &args[..1], inputs).is_err());
}

#[test]
fn player_arc_dispatch_returns_receiver_relative_result() {
    use cyrus_game::script::{PlayerQuery, QueryInputs, ScriptInputs, ScriptStage};
    for (offset_delta, expected) in [(0, 1), (1, 0)] {
        let mut scripts = random_scripts(
            "PlayerArc",
            vec![Value::Integer { value: 0 }, Value::Integer { value: 0 }],
            Object::Me,
        );
        scripts.definition.functions[1].param_count = 2;
        scripts.instances[0].angles[1] = offset_delta;
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let origin = world.spatial(id).unwrap().local.position;
        world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    player_query: Some(QueryInputs {
                        player: Some(PlayerQuery {
                            position: [origin[0], origin[1], origin[2].wrapping_sub(100)],
                            heading: 777,
                        }),
                        linked_position: None,
                        distance_override: None,
                        fallback_position: None,
                    }),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
    }
}

fn heading_property_scripts(reference: u16, object: Object) -> Scripts {
    let mut scripts = cache();
    scripts.definition.references = vec!["unsupported".into(); usize::from(reference) + 1];
    scripts.definition.references[usize::from(reference)] = "angle.vy".into();
    scripts.instances[0].angles[1] = 1536;
    let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
    program.strings = vec!["logic".into()];
    program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Local { index: 0 },
                    formula: Formula {
                        first: Value::Read {
                            place: Place::Property { object, reference },
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
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

#[test]
fn heading_property_reads_installed_name_with_full_reference_and_receiver() {
    for reference in [0, 38, 294] {
        for (object, expected) in [
            (Object::Me, 1536),
            (Object::Player, 0),
            (Object::Named { string: 0 }, 1536),
        ] {
            let mut scripts = heading_property_scripts(reference, object);
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            world.prepare_spatial().unwrap();
            let player = world.id(2).unwrap();
            world
                .initialize_player_spatial(player, [0; 3], [0; 3])
                .unwrap();
            let caller = world.id(0).unwrap();
            let before = world.spatial(caller).unwrap().local;
            world
                .advance(
                    caller,
                    cyrus_game::script::ScriptStage::Body(Thread::Main),
                    cyrus_game::script::ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.spatial(caller).unwrap().local.angles, before.angles);
            assert_eq!(
                world.spatial(caller).unwrap().local.position,
                before.position
            );
        }
    }
}

#[test]
fn heading_property_rejects_missing_owner_unknown_name_and_unrepresentable_writes() {
    for (spatial, name, write, expected) in [
        (false, "angle.vy", false, "spatial owner unavailable"),
        (true, "unknown", false, "unsupported"),
        (true, "angle.vy", true, "unsupported"),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name.into();
        if write {
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
                    first: Value::Integer { value: 4096 },
                    rest: vec![],
                },
            };
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        if spatial {
            world.prepare_spatial().unwrap();
        }
        let caller = world.id(0).unwrap();
        let error = world
            .advance(
                caller,
                cyrus_game::script::ScriptStage::Body(Thread::Main),
                cyrus_game::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
        if spatial {
            assert_eq!(world.spatial(caller).unwrap().local.angles[1], 1536);
        }
    }
}

#[test]
fn local_angle_properties_read_and_write_each_axis_without_refreshing_pose() {
    for (axis, name) in ["angle.vx", "angle.vy", "angle.vz"].into_iter().enumerate() {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name.into();
        scripts.instances[0].angles = [100, 1536, 300];
        let step = scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps
            .get_mut(&0)
            .unwrap();
        step.operation = Operation::Assign {
            destination: Place::Property {
                object: Object::Me,
                reference: 294,
            },
            formula: Formula {
                first: Value::Integer { value: 512 },
                rest: vec![],
            },
        };
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        let caller = world.id(0).unwrap();
        let before = world.spatial(caller).unwrap().local;
        world
            .advance(
                caller,
                cyrus_game::script::ScriptStage::Body(Thread::Main),
                cyrus_game::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        let after = world.spatial(caller).unwrap().local;
        let mut expected = before.angles;
        expected[axis] = 512;
        assert_eq!(after.angles, expected, "{name}");
        assert_eq!(after.position, before.position);
    }
}

#[test]
fn position_properties_read_local_coordinates_without_refreshing_pose() {
    for (axis, name) in ["pos.vx", "pos.vy", "pos.vz"].into_iter().enumerate() {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            let mut scripts = heading_property_scripts(294, object);
            scripts.definition.references[294] = name.into();
            scripts.instances[0].map_position = [-7, 11, -13];
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            world.prepare_spatial().unwrap();
            let caller = world.id(0).unwrap();
            let player = world.id(2).unwrap();
            world
                .initialize_player_spatial(player, [i32::MIN, -1, i32::MAX], [0; 3])
                .unwrap();
            let receiver = if selected { player } else { caller };
            let before = serde_json::to_value(world.spatial(receiver).unwrap()).unwrap();
            world
                .advance(
                    caller,
                    cyrus_game::script::ScriptStage::Body(Thread::Main),
                    cyrus_game::script::ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            let expected = if selected {
                [i32::MIN, -1, i32::MAX][axis]
            } else {
                [-7 * 256, 11 * 256, -13 * 256][axis]
            };
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(
                serde_json::to_value(world.spatial(receiver).unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn position_property_writes_change_only_the_selected_local_coordinate() {
    for (axis, name) in ["pos.vx", "pos.vy", "pos.vz"].into_iter().enumerate() {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            for value in [i32::MIN, -1, 0, 1, i32::MAX] {
                let mut scripts = heading_property_scripts(294, object.clone());
                scripts.definition.references[294] = name.into();
                let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
                let read = program.steps[&0].operation.clone();
                program.steps.insert(
                    0,
                    Step {
                        next: 1,
                        operation: Operation::Assign {
                            destination: Place::Property {
                                object: object.clone(),
                                reference: 294,
                            },
                            formula: Formula {
                                first: Value::Integer { value },
                                rest: vec![],
                            },
                        },
                    },
                );
                program.steps.insert(
                    1,
                    Step {
                        next: 2,
                        operation: read,
                    },
                );
                program.steps.insert(
                    2,
                    Step {
                        next: 3,
                        operation: Operation::End { target: 0 },
                    },
                );
                add_standalone_receivers(&mut scripts);
                let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
                world.prepare_spatial().unwrap();
                let caller = world.id(0).unwrap();
                let player = world.id(2).unwrap();
                world
                    .initialize_player_spatial(player, [123, -456, 789], [0; 3])
                    .unwrap();
                let receiver = if selected { player } else { caller };
                let other = if selected { caller } else { player };
                let mut expected = serde_json::to_value(world.spatial(receiver).unwrap()).unwrap();
                expected["local"]["position"][axis] = value.into();
                let other_before = serde_json::to_value(world.spatial(other).unwrap()).unwrap();
                world
                    .advance(
                        caller,
                        cyrus_game::script::ScriptStage::Body(Thread::Main),
                        cyrus_game::script::ScriptInputs {
                            frame_scale: Some(256),
                            nominal_ticks: Some(1),
                            ..Default::default()
                        },
                        None,
                        None,
                        20,
                    )
                    .unwrap();
                assert_eq!(world.actors()[0].runtime().unwrap().variables(), [value]);
                assert_eq!(
                    serde_json::to_value(world.spatial(receiver).unwrap()).unwrap(),
                    expected
                );
                assert_eq!(
                    serde_json::to_value(world.spatial(other).unwrap()).unwrap(),
                    other_before
                );
            }
        }
    }
}

#[test]
fn position_property_access_keeps_attached_effective_pose_until_family_refresh() {
    let mut scripts = heading_property_scripts(294, Object::Me);
    scripts.definition.references[294] = "pos.vx".into();
    let mut child = scripts.actors["LOGIC"].clone();
    child.namespace = Some("child".into());
    child.attributes.insert("attr_26".into(), 1);
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_25".into(), 1);
    scripts.actors.insert("CHILD".into(), child);
    scripts.instances[0].map_position = [100, 0, 0];
    scripts.instances[1].map_position = [103, 4, 0];
    scripts.instances[1].actor = "CHILD".into();
    scripts.instances[0].angles = [0; 3];
    let program = &mut scripts.actors.get_mut("CHILD").unwrap().program;
    program.steps.insert(
        1,
        Step {
            next: 2,
            operation: Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: 294,
                },
                formula: Formula {
                    first: Value::Integer { value: -512 },
                    rest: vec![],
                },
            },
        },
    );
    program.steps.insert(
        2,
        Step {
            next: 3,
            operation: Operation::End { target: 0 },
        },
    );
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let child = world.id(1).unwrap();
    assert_eq!(world.spatial(child).unwrap().local.position[0], 768);
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position[0],
        26368
    );
    let mut expected = serde_json::to_value(world.spatial(child).unwrap()).unwrap();
    expected["local"]["position"][0] = (-512).into();
    world
        .advance(
            child,
            cyrus_game::script::ScriptStage::Body(Thread::Main),
            cyrus_game::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().variables(), [768]);
    assert_eq!(
        serde_json::to_value(world.spatial(child).unwrap()).unwrap(),
        expected
    );
    world.refresh_families().unwrap();
    assert_eq!(
        world.spatial(child).unwrap().effective.unwrap().position[0],
        25088
    );
}

#[test]
fn position_property_rejects_unavailable_owner_and_unknown_reference() {
    for (name, prepare, expected) in [
        ("pos.vx", false, "spatial owner unavailable"),
        ("unknown", true, "unsupported"),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name.into();
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
                first: Value::Integer { value: i32::MIN },
                rest: vec![],
            },
        };
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        if prepare {
            world.prepare_spatial().unwrap();
        }
        let caller = world.id(0).unwrap();
        let before = prepare.then(|| serde_json::to_value(world.spatial(caller).unwrap()).unwrap());
        let error = world
            .advance(
                caller,
                cyrus_game::script::ScriptStage::Body(Thread::Main),
                cyrus_game::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        if let Some(before) = before {
            assert_eq!(
                serde_json::to_value(world.spatial(caller).unwrap()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn current_animation_property_reads_signed_group_without_advancing_playback() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (group, start, expected) in [(7, false, -1), (7, true, 7), (32768, true, -32768)] {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            let mut scripts = heading_property_scripts(294, object);
            scripts.definition.references[294] = "anim.current".into();
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let caller = world.id(0).unwrap();
            let receiver = world.id(if selected { 2 } else { 0 }).unwrap();
            let data = Animations {
                version: 2,
                world: scripts.world,
                metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
                    "script_name":world.actors()[receiver.index].name(),
                    "animation_groups":[{"group_index":group,"anim_id":1,"anim_type":0,
                        "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]}]
                }]}),
                sections: Default::default(),
                models: Default::default(),
                unsupported: Default::default(),
            };
            world.bind_animation(receiver, &data, group).unwrap();
            if start {
                world
                    .advance(
                        receiver,
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
            }
            let before = world.inspect(receiver.index, 1)["actors"][0]["animation"].clone();
            world
                .advance(
                    caller,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(
                world.inspect(receiver.index, 1)["actors"][0]["animation"],
                before
            );
        }
    }
}

#[test]
fn torch_property_wraps_writes_and_reads_the_shared_counter_without_other_changes() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (value, expected) in [
        (-1, 255),
        (0, 0),
        (1, 1),
        (255, 255),
        (256, 0),
        (511, 255),
        (i32::MIN, 0),
        (i32::MAX, 255),
    ] {
        for (object, selected) in [
            (Object::Me, false),
            (Object::Player, true),
            (Object::Named { string: 0 }, false),
        ] {
            let mut scripts = heading_property_scripts(294, object.clone());
            scripts.definition.references[294] = "torch_timer".into();
            let program = &mut scripts.actors.get_mut("LOGIC").unwrap().program;
            let read = program.steps[&0].operation.clone();
            program.steps.insert(
                0,
                Step {
                    next: 1,
                    operation: Operation::Assign {
                        destination: Place::Property {
                            object,
                            reference: 294,
                        },
                        formula: Formula {
                            first: Value::Integer { value },
                            rest: vec![],
                        },
                    },
                },
            );
            program.steps.insert(
                1,
                Step {
                    next: 2,
                    operation: read,
                },
            );
            program.steps.insert(
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            );
            add_standalone_receivers(&mut scripts);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let caller = world.id(0).unwrap();
            let receiver = if selected { 2 } else { 0 };
            let other = if selected { 0 } else { 2 };
            let mut expected_control =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            expected_control["torch_burning"] = expected.into();
            let other_before =
                serde_json::to_value(world.actors()[other].control_state().unwrap()).unwrap();
            world
                .advance(
                    caller,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [expected]);
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                expected_control
            );
            assert_eq!(
                serde_json::to_value(world.actors()[other].control_state().unwrap()).unwrap(),
                other_before
            );
        }
    }
}

#[test]
fn animation_and_torch_properties_reject_unprepared_owners_and_animation_writes() {
    for (name, write, separate, expected) in [
        ("anim.current", false, false, "animation owner unavailable"),
        ("anim.current", true, false, "write unsupported"),
        ("torch_timer", false, true, "control owner unavailable"),
        ("torch_timer", true, true, "control owner unavailable"),
    ] {
        let mut scripts = heading_property_scripts(294, Object::Me);
        scripts.definition.references[294] = name.into();
        if separate {
            scripts.instances[0].object_type = 2;
        }
        if write {
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
                    first: Value::Integer { value: 255 },
                    rest: vec![],
                },
            };
        }
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let error = world
            .advance(
                caller,
                cyrus_game::script::ScriptStage::Body(Thread::Main),
                cyrus_game::script::ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
    }
}

#[test]
fn animation_frame_waits_resume_from_the_correct_owner_without_advancing_playback() {
    use cyrus_assets::animation::Animations;
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (function, selected) in [("WaitAnimFrame", false), ("WaitPlayerAnimFrame", true)] {
        let mut scripts = wait_cache(vec![
            wait(
                1,
                function,
                CallKind::Task,
                vec![Value::Integer { value: 7 }, Value::Integer { value: 1 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions[1] = Function {
            name: function.into(),
            param_count: 2,
            kind: CallKind::Task,
        };
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let caller = world.id(0).unwrap();
        let receiver = world.id(if selected { 2 } else { 0 }).unwrap();
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
                "script_name":world.actors()[receiver.index].name(),
                "animation_groups":[{"group_index":7,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},{"opcode":2,"value":0}]}]
            }]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        world.bind_animation(receiver, &data, 7).unwrap();
        world
            .advance(
                receiver,
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
        let before = world.inspect(receiver.index, 1)["actors"][0]["animation"].clone();
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        world
            .advance(
                caller,
                ScriptStage::Tasks,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
        assert_eq!(
            world.inspect(receiver.index, 1)["actors"][0]["animation"],
            before
        );
        world
            .advance(
                receiver,
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
        world
            .advance(
                caller,
                ScriptStage::Tasks,
                ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                0,
            )
            .unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        world
            .advance(
                caller,
                ScriptStage::Body(Thread::Main),
                ScriptInputs {
                    nominal_ticks: Some(0),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.flags(), [7]);
    }
}

#[test]
fn animation_frame_wait_rejects_unprepared_owner_but_absent_selected_player_completes() {
    for (function, completes) in [("WaitAnimFrame", false), ("WaitPlayerAnimFrame", true)] {
        let mut scripts = wait_cache(vec![
            wait(
                1,
                function,
                CallKind::Task,
                vec![Value::Integer { value: 7 }, Value::Integer { value: 1 }],
            ),
            set_flag(7),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions[1] = Function {
            name: function.into(),
            param_count: 2,
            kind: CallKind::Task,
        };
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        let result = world.step(world.id(0).unwrap(), Thread::Main, 20);
        if completes {
            result.unwrap();
            world.step_tasks(world.id(0).unwrap(), Some(256)).unwrap();
            world.step(world.id(0).unwrap(), Thread::Main, 20).unwrap();
            assert_eq!(world.flags(), [7]);
        } else {
            assert!(format!("{:#}", result.unwrap_err()).contains("requires prepared playback"));
        }
    }
}

#[test]
fn actor_in_sight_is_blocked_by_candidate_models_and_gated_by_distance() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::script::OrdinaryObstacleQuery;
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
    }
}

#[test]
fn blocked_ahead_is_false_without_a_combat_record_and_fighting_group() {
    use cyrus_assets::collision::CollisionScene;
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::script::OrdinaryObstacleQuery;
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

fn tilted_surface_model(
    normal: [f32; 3],
    vertices: Vec<[f32; 3]>,
) -> cyrus_assets::collision::CollisionModel {
    use cyrus_assets::collision::{CollisionGeometry, CollisionModel, CollisionSubobject};
    CollisionModel {
        flags: 0,
        geometry: CollisionGeometry {
            version: Some(*b"v5.0"),
            radius: 1000.0,
            vertices,
            normals: vec![normal],
            faces: vec![vec![0, 1, 2, 3]],
            textures: None,
            subobjects: vec![CollisionSubobject {
                center: [0.0; 3],
                radius: 1000.0,
                extents: Some([1.0; 3]),
                faces: vec![0],
            }],
        },
    }
}

#[test]
fn step_hazard_is_a_steep_destination_surface_unless_both_points_are_floors() {
    use cyrus_assets::collision::{CollisionScene, StaticPlacement};
    use cyrus_collision::collision_world::StaticCollision;
    use cyrus_game::script::OrdinaryObstacleQuery;
    let model = |kind: &str| match kind {
        "floor" => tilted_surface_model(
            [0.0, -1.0, 0.0],
            vec![
                [-10.0, 0.0, -10.0],
                [10.0, 0.0, -10.0],
                [10.0, 0.0, 10.0],
                [-10.0, 0.0, 10.0],
            ],
        ),
        _ => tilted_surface_model(
            [-0.866, -0.5, 0.0],
            vec![
                [-5.0, 8.66, -10.0],
                [5.0, -8.66, -10.0],
                [5.0, -8.66, 10.0],
                [-5.0, 8.66, 10.0],
            ],
        ),
    };
    for (surface, attr_1, expected) in [
        (Some("slope"), 10, true),
        (Some("floor"), 10, false),
        (Some("slope"), 8, false),
        (None, 10, false),
    ] {
        let (mut world, data) = overlap_response_fixture_with(128, 0, 1024, attr_1);
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        world.snapshot_spatial(ids[0]).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let mut scene = ordinary_collision_scene(world.world, models);
        let current = world.spatial(ids[0]).unwrap().local.position;
        let destination = [current[0] + 594 * 256, current[1], current[2]];
        // The body shape is prepared with the first admission, so read its extent from an
        // empty scene before placing the surface.
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            ..Default::default()
        };
        scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
        world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [10, 30], &[ids[0], ids[1]])
            .unwrap();
        let centre = (destination[1] >> 8)
            + world
                .ordinary_collision_shape(ids[0])
                .unwrap()
                .positive_extent[1];
        scene.collision = CollisionScene {
            sphere_object_scale: Some(256),
            placements: surface
                .map(|_| StaticPlacement {
                    id: 1,
                    model: "SURFACE".into(),
                    position: [destination[0] >> 8, centre, destination[2] >> 8],
                    rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
                })
                .into_iter()
                .collect(),
            models: surface
                .map(|kind| BTreeMap::from([("SURFACE".into(), model(kind))]))
                .unwrap_or_default(),
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
        assert_eq!(
            world.step_hazard(ids[0], None, destination, query).unwrap(),
            expected,
            "surface {} attr_1 {attr_1}",
            surface.is_some()
        );
    }
}

#[test]
fn lifecycle_byte_queries_read_the_fresh_receiver_state() {
    for name in [
        "MyAIType",
        "MyAIMode",
        "IsOnEdge",
        "IsInAir",
        "IsCombatCapable",
    ] {
        assert_eq!(run_native_calls(&[(name, &[])]).unwrap(), 0, "{name}");
        let error = run_native_calls(&[(name, &[1])]).unwrap_err();
        assert!(format!("{error:#}").contains("no arguments"), "{name}");
    }
}

#[test]
fn health_strength_and_armor_queries_read_the_receiver() {
    assert_eq!(run_native_calls(&[("Health", &[])]).unwrap(), 0);
    assert_eq!(
        run_native_calls(&[("SetMyAttr", &[3, 40]), ("Health", &[])]).unwrap(),
        40
    );
    assert_eq!(
        run_native_calls(&[("SetMyAttr", &[3, 40]), ("ReplenishHealth", &[])]).unwrap(),
        1
    );
    assert_eq!(
        run_native_calls(&[
            ("SetMyAttr", &[3, 40]),
            ("ReplenishHealth", &[]),
            ("Health", &[])
        ])
        .unwrap(),
        100
    );
    assert_eq!(
        run_native_calls(&[("SetMyAttr", &[3, 100]), ("ReplenishHealth", &[])]).unwrap(),
        0
    );
    for name in ["Strength", "Armor"] {
        let error = run_native_calls(&[(name, &[])]).unwrap_err();
        assert!(
            format!("{error:#}").contains("receiver combat record unavailable"),
            "{error:#}"
        );
    }
}

#[test]
fn set_and_adjust_health_clamp_and_leave_death_and_gates_unavailable() {
    let health = |calls: &[(&str, &[i32])]| {
        let mut calls = calls.to_vec();
        calls.push(("Health", &[]));
        run_native_calls(&calls)
    };
    assert_eq!(health(&[("SetHealth", &[60])]).unwrap(), 60);
    assert_eq!(
        health(&[("SetHealth", &[60]), ("AdjustHealth", &[-20, 1])]).unwrap(),
        40
    );
    assert_eq!(health(&[("AdjustHealth", &[300, 0])]).unwrap(), 255);
    assert_eq!(health(&[("AdjustHealth", &[-5, 0])]).unwrap(), 0);
    for calls in [
        &[("AdjustHealth", &[-5, 1][..])][..],
        &[("SetHealth", &[0][..])][..],
    ] {
        let error = health(calls).unwrap_err();
        assert!(
            format!("{error:#}").contains("death sequence unavailable"),
            "{error:#}"
        );
    }
    // Attribute 120 ignores changes; attribute 97 needs the combat-record gate.
    assert_eq!(
        health(&[("SetMyAttr", &[120, 1]), ("SetHealth", &[60])]).unwrap(),
        0
    );
    let error = health(&[("SetMyAttr", &[97, 1]), ("SetHealth", &[60])]).unwrap_err();
    assert!(format!("{error:#}").contains("invulnerability gate"));
}

#[test]
fn ai_setters_master_slave_and_true_function_follow_their_bytes() {
    assert_eq!(
        run_native_calls(&[("SetAIType", &[300]), ("MyAIType", &[])]).unwrap(),
        44
    );
    assert_eq!(
        run_native_calls(&[("SetAIMode", &[7]), ("MyAIMode", &[])]).unwrap(),
        7
    );
    assert_eq!(run_native_calls(&[("IsMaster", &[])]).unwrap(), 0);
    assert_eq!(
        run_native_calls(&[("SetMyAttr", &[26, 3]), ("IsSlave", &[])]).unwrap(),
        1
    );
    assert_eq!(
        run_native_calls(&[("SetMyAttr", &[25, 3]), ("IsMaster", &[])]).unwrap(),
        1
    );
    assert_eq!(run_native_calls(&[("TrueFunction", &[0, 5])]).unwrap(), 1);
}

#[test]
fn player_moved_needs_a_selected_player_with_spatial_state() {
    let error = run_native_world_with(&[("PlayerMoved", &[])], true)
        .map(|_| ())
        .unwrap_err();
    assert!(format!("{error:#}").contains("selected player unavailable"));
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "PlayerMoved".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    let mut actor = scripts.actors.remove("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.namespace = Some("cyrus".into());
    actor.program.steps.get_mut(&0).unwrap().operation = Operation::Assign {
        destination: Place::Local { index: 0 },
        formula: Formula {
            first: Value::Call {
                call: Box::new(Call {
                    function_id: 1,
                    target: Object::Me,
                    kind: CallKind::Function,
                    function: "PlayerMoved".into(),
                    arguments: vec![],
                }),
            },
            rest: vec![],
        },
    };
    scripts.actors.insert("CYRUS".into(), actor);
    scripts.instances.clear();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(0).unwrap();
    let error = world
        .advance(
            player,
            cyrus_game::script::ScriptStage::OrdinaryMain,
            cyrus_game::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(3),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("spatial state unavailable"));
}

#[test]
fn enable_and_disable_natives_toggle_their_script_control_bit() {
    let bits = |calls: &[(&str, &[i32])]| {
        run_native_world(calls).unwrap().actors()[0]
            .control_state()
            .unwrap()
            .script_control
    };
    for (suffix, bit) in [("Script", 1), ("Tasks", 4), ("Object", 8), ("Combat", 16)] {
        let disable = format!("Disable{suffix}");
        let enable = format!("Enable{suffix}");
        assert_eq!(bits(&[(&disable, &[])]), bit, "{disable}");
        assert_eq!(bits(&[(&disable, &[]), (&enable, &[])]), 0, "{enable}");
    }
    assert_eq!(
        bits(&[
            ("DisableTasks", &[]),
            ("DisableCombat", &[]),
            ("EnableTasks", &[])
        ]),
        16
    );
}

#[test]
fn session_request_natives_raise_requests_for_the_host() {
    use cyrus_game::script::HostRequests;
    let mut world = run_native_world(&[
        ("LoadWorld", &[7, 11, -3]),
        ("WonGame", &[]),
        ("PushPlayer", &[1]),
        ("PopPlayer", &[1]),
    ])
    .unwrap();
    assert_eq!(
        world.globals_mut().take_host_requests(),
        HostRequests {
            load_world: Some([7, 11, -3]),
            won_game: true,
            push_player: true,
            pop_player: true,
        }
    );
    assert_eq!(
        world.globals_mut().take_host_requests(),
        HostRequests::default()
    );
    assert_eq!(run_native_calls(&[("IsWithinMap", &[1])]).unwrap(), 1);
    assert_eq!(run_native_calls(&[("Animal", &[5])]).unwrap(), 0);
    assert!(run_native_calls(&[("LoadWorld", &[1])]).is_err());
}

#[test]
fn dialog_and_menu_natives_write_their_mode_words() {
    let state = |calls: &[(&str, &[i32])]| {
        let world = run_native_world(calls).unwrap();
        (
            world.globals().dialogue_state(),
            world.globals().menu_state(),
        )
    };
    assert_eq!(state(&[("DialogBegin", &[])]), (Some(1), 0));
    assert_eq!(
        state(&[("DialogBegin", &[]), ("DialogEnd", &[])]),
        (Some(0), 0)
    );
    assert_eq!(state(&[("MenuBegin", &[])]), (None, 1));
    assert_eq!(state(&[("MenuBegin", &[]), ("MenuEnd", &[])]), (None, 0));
    assert!(run_native_calls(&[("DialogBegin", &[1])]).is_err());
}

#[test]
fn my_id_reads_the_runtime_identifier() {
    let world = run_native_world(&[("MyId", &[])]).unwrap();
    let actor = &world.actors()[0];
    let expected = i32::from(actor.runtime_identifier.unwrap());
    assert_eq!(actor.runtime().unwrap().variables()[0], expected);
}

#[test]
fn live_paced_pick_uses_receiver_geometry_and_branch_specific_damage() {
    use cyrus_game::script::decision::PacedDefenceInput;
    let input = PacedDefenceInput {
        distance: 50,
        random_draw: 0,
        pacing_sum: 129,
        facing_alike: true,
        target_in_combat_stance: true,
        attribute_58: 29,
    };
    for (damages, expected, pacing) in [
        ([Some(i32::MAX), Some(1)], Some(i32::MIN), 129),
        ([Some(4), Some(5)], Some(9), 129),
        ([Some(4), None], None, 129),
        ([Some(4), Some(5)], Some(9), 0),
    ] {
        let mut scripts = cache();
        scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
        scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
            ("attr_14".into(), 1),
            ("attr_62".into(), 129 - pacing),
            ("attr_71".into(), 1),
        ]);
        for instance in &mut scripts.instances {
            instance.object_type = 1;
        }
        scripts.instances[0].angles[1] = 1024;
        scripts.instances[1].map_position = [0, -20, 20];
        let motions = damages
            .into_iter()
            .enumerate()
            .map(|(index, damage)| {
                serde_json::from_value(serde_json::json!({
                    "section":index,"kind":1,"animation_group":97,
                    "min_range":if index == 0 {10} else {-999},
                    "max_range":if index == 0 {100} else {9999},
                    "first_collide_frame":7,"collide_duration":3,
                    "arc":if index == 0 {64} else {0},"damage":damage
                }))
                .unwrap()
            })
            .collect();
        scripts.combat_attacks = vec![
            cyrus_assets::script::combat::Attack {
                is_combo: false,
                motions: vec![],
                min_range: 0,
                max_range: 0,
            },
            cyrus_assets::script::combat::Attack {
                is_combo: true,
                motions,
                min_range: -999,
                max_range: 9999,
            },
        ];
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
                "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let own = world.id(0).unwrap();
        let target = world.id(1).unwrap();
        world
            .initialize_map_animation(own, &data, 1, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .spacing_ranges,
            [-999, 9999, 4500]
        );
        world.prepare_spatial().unwrap();
        let stale = cyrus_game::script::ScriptActorId {
            generation: 2,
            index: 1,
        };
        // Empty slots and out-of-range attacks must not query the stale target or missing bounds.
        assert_eq!(
            world
                .pick_combat_paced_defence(
                    own,
                    stale,
                    PacedDefenceInput {
                        distance: 10,
                        ..input
                    }
                )
                .unwrap(),
            0
        );
        assert!(world.pick_combat_paced_defence(own, stale, input).is_err());
        let (_, _, models) = static_model_fixture(cache());
        let scene = ordinary_collision_scene(world.world, models);
        for actor in [own, target] {
            world
                .prepare_ordinary_collision_shape(actor, &data, &scene)
                .unwrap();
        }
        // This branch never reads damage or the unused counter-attack field.
        let armed = world.pick_combat_paced_defence(
            own,
            target,
            PacedDefenceInput {
                facing_alike: false,
                ..input
            },
        );
        if pacing == 0 {
            assert!(format!("{:#}", armed.unwrap_err()).contains("invalid defence pacing divisor"));
        } else {
            assert_eq!(armed.unwrap(), 0);
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .decision
                .defence_counter,
            100
        );
        if pacing == 0 {
            continue;
        }
        let result = world.pick_combat_paced_defence(own, target, input);
        let Some(expected) = expected else {
            assert!(
                format!("{:#}", result.unwrap_err())
                    .contains("motion section 1: damage unavailable")
            );
            continue;
        };
        assert_eq!(result.unwrap(), if expected > 0 { 1 } else { 0 });
        assert!(world.pick_combat_paced_defence(own, stale, input).is_err());
    }
}

#[test]
fn timed_taunt_request_uses_live_period_shared_rng_and_synchronous_playback() {
    use cyrus_game::script::{CombatTauntPlayback, CombatTauntSource, DialogueLine, ScriptInputs};
    #[derive(Debug, Default)]
    struct Playback {
        events: Vec<&'static str>,
        fail: bool,
    }
    impl CombatTauntPlayback for Playback {
        fn prepare(&mut self, label: [u8; 4], _: bool, _: Option<u32>) -> anyhow::Result<()> {
            assert_eq!(label, *b"TEST");
            self.events.push("prepare");
            anyhow::ensure!(!self.fail, "injected preparation failure");
            Ok(())
        }
        fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
            self.events.push("release");
            *retained = None;
            Ok(())
        }
        fn load(&mut self, _: [u8; 4]) -> anyhow::Result<usize> {
            self.events.push("load");
            Ok(4)
        }
        fn present(&mut self, _: usize, _: bool, _: Option<u32>) -> anyhow::Result<DialogueLine> {
            self.events.push("present");
            Ok(DialogueLine {
                text: "Voice".into(),
                sample: None,
                lifetime: 210,
            })
        }
    }
    #[derive(Debug, Default)]
    struct Source(std::cell::RefCell<Playback>);
    impl CombatTauntSource for Source {
        fn with_playback(
            &self,
            update: &mut dyn FnMut(&mut dyn CombatTauntPlayback) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            update(&mut *self.0.borrow_mut())
        }
    }
    for fail in [false, true] {
        let mut scripts = cache();
        scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes.extend([
            ("attr_14".into(), 1),
            ("attr_87".into(), 124),
            ("attr_num_taunts".into(), 1),
        ]);
        actor.combat_taunts = vec![CombatTaunt {
            trigger: 9,
            label: *b"TEST",
            animation: 0,
        }];
        for instance in &mut scripts.instances {
            instance.object_type = 1;
        }
        scripts.instances[1].map_position = [1024, 0, 0];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
                "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "raan_entries":[{"frame_count":1,"file_path":"TEST.3D"}],
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        data.metadata["mpob_objects"] = serde_json::json!([
            {"script_name":"LOGIC"}, {"script_name":"LOGIC"}
        ]);
        data.models.insert(
            "TEST".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "static_3d".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        );
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        // Construction order, rather than runtime actor index, owns the allocation index.
        world
            .initialize_map_animation(world.id(1).unwrap(), &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        let id = world.id(0).unwrap();
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .taunt_period,
            5
        );
        let collision = cyrus_assets::collision::CollisionScene::default();
        let sources = cyrus_game::script::WorldSnapshotSources {
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let allocation = |world: &ScriptWorld, index: usize| {
            world.actors()[index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .allocation_index
        };
        assert_eq!((allocation(&world, 0), allocation(&world, 1)), (1, 0));
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        assert_eq!(saved["next_combat_index"], 2);
        let mut missing_counter = saved.clone();
        missing_counter
            .as_object_mut()
            .unwrap()
            .remove("next_combat_index");
        assert!(
            serde_json::from_value::<cyrus_game::script::WorldSnapshot>(missing_counter).is_err()
        );
        let mut continued = saved.clone();
        continued["actors"][1]["control"]["combat"] = serde_json::Value::Null;
        continued["actors"][1]["animation"] = serde_json::Value::Null;
        continued["next_combat_index"] = i32::MAX.into();
        let mut continued = ScriptWorld::from_snapshot(
            serde_json::from_value(continued).unwrap(),
            &sources,
            cyrus_game::script::WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        continued
            .initialize_map_animation(continued.id(1).unwrap(), &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(allocation(&continued, 1), i32::MAX);
        assert_eq!(
            serde_json::to_value(continued.snapshot(&sources).unwrap()).unwrap()["next_combat_index"],
            i32::MIN
        );

        saved["actors"][0]["control"]["combat"]["allocation_index"] = (-1).into();
        saved["actors"][0]["control"]["combat"]["action"] = 0.into();
        let mut missing = saved.clone();
        missing["actors"][0]["control"]["combat"]
            .as_object_mut()
            .unwrap()
            .remove("taunt_period");
        assert!(serde_json::from_value::<cyrus_game::script::WorldSnapshot>(missing).is_err());
        for dead in [false, true] {
            let mut blocked = saved.clone();
            if dead {
                blocked["actors"][0]["control"]["death_selector"] = 8.into();
            } else {
                blocked["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            let mut blocked = ScriptWorld::from_snapshot(
                serde_json::from_value(blocked).unwrap(),
                &sources,
                cyrus_game::script::WorldRestoreContext {
                    generation: 2,
                    globals: Default::default(),
                    samples: &BTreeMap::new(),
                },
            )
            .unwrap();
            let mut random = cyrus_game::random::EngineRandom::default();
            blocked
                .request_timed_combat_taunt(
                    blocked.id(0).unwrap(),
                    9,
                    1,
                    ScriptInputs::default(),
                    &mut random,
                    &mut |_, _| panic!("blocked sound"),
                )
                .unwrap();
            assert_eq!(
                random.state(),
                1,
                "dead or exact-distance boundary must not draw"
            );
        }
        // Attribute writes after construction must not replace the retained divisor.
        saved["actors"][0]["attributes"][87] = 129.into();
        world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            cyrus_game::script::WorldRestoreContext {
                generation: 2,
                globals: std::mem::take(world.globals_mut()),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(world.actors()[0].attributes()[87], 129);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .taunt_period,
            5
        );
        let mut endpoint = cyrus_game::random::EngineRandom::from_state(24884);
        let error = world
            .request_timed_combat_taunt(
                id,
                9,
                1,
                ScriptInputs::default(),
                &mut endpoint,
                &mut |_, _| panic!("unsupported endpoint sound"),
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("world 7 instance 0 timed combat taunt type 9"));
        assert!(format!("{error:#}").contains("one-past access unsupported"));
        assert_eq!(endpoint.state(), 2147445597);
        let source = Source::default();
        source.0.borrow_mut().fail = fail;
        let inputs = ScriptInputs {
            nominal_ticks: Some(0),
            combat_taunts: Some(&source),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::default();
        let initial = random.state();
        // Allocation index -1 is off-period; action and attack state zero must not admit it.
        world
            .request_timed_combat_taunt(id, 9, 0, inputs, &mut random, &mut |_, _| Ok(()))
            .unwrap();
        assert!(source.0.borrow().events.is_empty());
        let mut expected = cyrus_game::random::EngineRandom::from_state(initial);
        for _ in 0..15 {
            expected.draw();
        }
        assert_eq!(random.state(), expected.state());
        let result =
            world.request_timed_combat_taunt(id, 9, 1, inputs, &mut random, &mut |_, _| Ok(()));
        for _ in 0..15 {
            expected.draw();
        }
        assert_eq!(random.state(), expected.state());
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("injected preparation failure"));
            assert_eq!(combat.flags & 0x40, 0);
            assert_eq!(source.0.borrow().events, ["prepare"]);
        } else {
            result.unwrap();
            assert_eq!(combat.flags & 0x40, 0x40);
            assert_eq!(
                source.0.borrow().events,
                ["prepare", "release", "load", "present"]
            );
            assert_eq!(combat.taunt_cache, Some(4));
            // Type-nine retained flag rejects without RNG or playback services.
            world
                .request_timed_combat_taunt(
                    id,
                    9,
                    1,
                    ScriptInputs::default(),
                    &mut random,
                    &mut |_, _| panic!("rejected sound"),
                )
                .unwrap();
            assert_eq!(random.state(), expected.state());
        }
    }
}

#[test]
fn live_defence_executor_uses_following_motion_loop_count_and_synchronous_sound() {
    use cyrus_game::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    #[derive(Debug)]
    struct RejectTaunt;
    impl cyrus_game::script::CombatTauntSource for RejectTaunt {
        fn with_playback(
            &self,
            _: &mut dyn FnMut(
                &mut dyn cyrus_game::script::CombatTauntPlayback,
            ) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            anyhow::bail!("injected defence taunt failure")
        }
    }
    let mut scripts = cache();
    scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
    scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
        ("attr_14".into(), 1),
        ("attr_16".into(), 17),
        ("attr_70".into(), 1),
        ("attr_87".into(), 127),
        ("attr_num_taunts".into(), 1),
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().combat_taunts = vec![CombatTaunt {
        trigger: 1,
        label: *b"TEST",
        animation: 0,
    }];
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts.combat_attacks = vec![
        cyrus_assets::script::combat::Attack {
            is_combo: false,
            motions: vec![],
            min_range: 0,
            max_range: 0,
        },
        cyrus_assets::script::combat::Attack {
            is_combo: true,
            motions: (0..2)
                .map(|motion| {
                    serde_json::from_value(serde_json::json!({
                        "section":motion,"kind":motion,"animation_group":100+motion,
                        "min_range":0,"max_range":100,"first_collide_frame":0,"collide_duration":0
                    }))
                    .unwrap()
                })
                .collect(),
            min_range: 0,
            max_range: 100,
        },
    ];
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "mpob_objects":[{"script_name":"LOGIC"},{"script_name":"LOGIC"}],
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "raan_entries":[{"frame_count":1,"file_path":"TEST.3D"}],
                "animation_groups":[
                    {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":100,"anim_id":1,"anim_type":0,"commands":[
                        {"opcode":4,"sound_param":-17,"volume_shift":0},
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":101,"anim_id":1,"anim_type":0,"commands":[
                        {"opcode":4,"sound_param":-18,"volume_shift":0},
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]}),
        sections: Default::default(),
        models: BTreeMap::from([(
            "TEST".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "static_3d".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        )]),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let constructed = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(constructed.spacing_ranges, [10000, 30000, 20000]);
    assert_eq!(constructed.spacing_period, 129);
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for field in ["spacing_ranges", "spacing_period"] {
        let mut incomplete = saved.clone();
        incomplete["actors"][0]["control"]["combat"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<cyrus_game::script::WorldSnapshot>(incomplete).is_err());
    }
    saved["actors"][0]["control"]["combat"]["spacing_ranges"] = serde_json::json!([10, 90, 25]);
    saved["actors"][0]["control"]["combat"]["spacing_period"] = (-7).into();
    let combat = &mut saved["actors"][0]["control"]["combat"];
    combat["action"] = 0.into();
    combat["retained_action_indices"] = serde_json::json!([-1, 0]);
    combat["flags"] = 48.into();
    combat["decision"]["aggression"] = 50.into();
    combat["decision"]["move_selector"] = 2.into();
    for (
        state,
        motion,
        fail,
        taunt_failure,
        expected_motion,
        expected_aggression,
        expected_active,
    ) in [
        (0, 2, false, false, 0, 40, true),
        (0, 2, true, false, 0, 50, false),
        (1, 0, false, false, 1, 40, true),
        (1, 0, true, false, 1, 50, false),
        (1, 1, false, false, 1, 50, false),
        (0, 2, false, true, 0, 40, true),
    ] {
        let mut snapshot = saved.clone();
        snapshot["actors"][0]["control"]["combat"]["decision"]["attack_state"] = state.into();
        snapshot["actors"][0]["control"]["combat"]["decision"]["move_selector"] = motion.into();
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
        let restored = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(restored.spacing_ranges, [10, 90, 25]);
        assert_eq!(restored.spacing_period, -7);
        let id = world.id(0).unwrap();
        let mut cues = 0;
        let result = world.execute_combat_defence(
            id,
            0,
            if taunt_failure { 0 } else { 1 },
            ScriptInputs {
                nominal_ticks: Some(1),
                combat_taunts: Some(&RejectTaunt),
                ..Default::default()
            },
            &mut Default::default(),
            &mut |owner, _| {
                assert_eq!(owner, id);
                cues += 1;
                anyhow::ensure!(!fail, "injected defence sound failure");
                Ok(())
            },
        );
        if fail {
            assert!(
                format!("{:#}", result.unwrap_err()).contains("injected defence sound failure")
            );
        } else if taunt_failure {
            assert!(
                format!("{:#}", result.unwrap_err()).contains("injected defence taunt failure")
            );
        } else {
            assert_eq!(result.unwrap(), expected_active);
        }
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (combat.decision.move_selector, combat.decision.aggression),
            (expected_motion, expected_aggression)
        );
        assert_eq!(combat.flags, if fail || !expected_active { 48 } else { 1 });
        assert_eq!(cues, if motion == 1 { 0 } else { 1 });
        if !fail && expected_active {
            let snapshot = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
            assert_eq!(
                snapshot["actors"][0]["animation"]["playback"]["current"][1]["loops"],
                1
            );
        }
    }

    // Exercise the world starter against live receiver ownership, not caller-provided admission.
    for (kind, selected_player, action, cooldown, living, fail_movement, expected) in [
        (0, true, -1, 0, 0, false, true),
        (0, true, 0, 0, 0, false, false),
        (1, false, 5, 0, 0, false, true),
        (1, true, 5, 0, 0, false, false),
        (4, true, -1, 1, 0, false, false),
        (4, true, -1, 0, 20, false, false),
        (4, true, -1, 0, 0, true, false),
        (4, true, -1, 0, 0, false, true),
        (4, false, -1, 0, 0, false, true),
        (8, true, -1, 0, 0, false, true),
        (6, true, -1, 0, 0, false, true),
        (7, true, -1, 0, 0, false, false),
        (9, true, -1, 0, 0, false, false),
    ] {
        let mut snapshot = saved.clone();
        let control = &mut snapshot["actors"][0]["control"];
        control["living_state"] = living.into();
        let combat = &mut control["combat"];
        combat["action"] = action.into();
        combat["retained_action_indices"][1] = (-1).into();
        combat["cooldown"] = cooldown.into();
        let attack = combat["attack_indices"][0].clone();
        for slot in [1, 4, 6, 8] {
            combat["attack_indices"][slot] = attack.clone();
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
        let id = world.id(0).unwrap();
        let selected = if selected_player {
            id
        } else {
            world.id(1).unwrap()
        };
        let fail_sound = kind == 6;
        let mut moved = false;
        let mut cues = 0;
        let result = world.start_combat_defence(
            id,
            kind,
            cyrus_game::script::WeaponInputs {
                selected_player: selected,
                item_system_ready: true,
                shell_execution: false,
            },
            1,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut Default::default(),
            |world, owner, distance, mode| {
                assert_eq!(owner, id);
                assert_eq!((distance, mode), (17 << 8, 1));
                let combat = world.actors()[0]
                    .control_state()
                    .unwrap()
                    .combat
                    .as_ref()
                    .unwrap();
                assert_eq!((combat.action, combat.retained_action_indices[1]), (-1, -1));
                moved = true;
                anyhow::ensure!(!fail_movement, "injected starter movement failure");
                Ok(())
            },
            &mut |_, _| {
                cues += 1;
                anyhow::ensure!(!fail_sound, "injected starter sound failure");
                Ok(())
            },
        );
        if fail_movement {
            assert!(
                format!("{:#}", result.unwrap_err()).contains("injected starter movement failure")
            );
        } else if fail_sound {
            assert!(
                format!("{:#}", result.unwrap_err()).contains("injected starter sound failure")
            );
        } else {
            assert_eq!(result.unwrap(), expected, "kind {kind}");
        }
        assert_eq!(
            moved,
            kind == 4 && selected_player && cooldown == 0 && living == 0
        );
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (combat.action, combat.retained_action_indices[1]),
            if expected { (kind, kind) } else { (action, -1) }
        );
        assert_eq!(cues, usize::from(expected));
        assert_eq!(
            combat.decision.attack_state,
            i32::from(expected && !fail_sound)
        );
    }
    // The live selection owner must reload executor writes even after a sound error.
    for fail_sound in [false, true] {
        let mut snapshot = saved.clone();
        let combat = &mut snapshot["actors"][0]["control"]["combat"];
        combat["action"] = (-1).into();
        combat["attack_indices"][1] = combat["attack_indices"][0].clone();
        combat["decision"]["attack_state"] = 0.into();
        combat["decision"]["defence_counter"] = 1.into();
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
        let id = world.id(0).unwrap();
        let target = world.id(1).unwrap();
        assert_eq!(
            world.pick_combat_defence(id, target, 50, false).unwrap(),
            Some(0)
        );
        assert!(
            format!(
                "{:#}",
                world.pick_combat_defence(id, target, 50, true).unwrap_err()
            )
            .contains("counter-attack reference unavailable")
        );
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        let mut cues = 0;
        let result = world.select_combat_defence(
            id,
            target,
            cyrus_game::script::decision::DefenceSelectionInput {
                armed: true,
                lockout_clear: true,
                own_selected: 0,
                target_in_arc: false,
                distance: 0,
                own_window_far: 0,
                facing_alike: false,
                target_in_combat_stance: false,
                attribute_58: 0,
                pacing_sum: 0,
                defence_pacing: 0,
                target_selected: -1,
                target_frame: 0,
                target_near: 0,
                target_third: 0,
                own_selector: 0,
            },
            cyrus_game::script::WeaponInputs {
                selected_player: target,
                item_system_ready: true,
                shell_execution: false,
            },
            1,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut random,
            &mut |_, _, _, _| anyhow::bail!("unexpected movement"),
            &mut |_, _| {
                cues += 1;
                anyhow::ensure!(!fail_sound, "selection sound failure");
                Ok(())
            },
        );
        if fail_sound {
            assert!(format!("{:#}", result.unwrap_err()).contains("selection sound failure"));
        } else {
            result.unwrap();
        }
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!((combat.action, combat.retained_action_indices[1]), (1, 1));
        assert_eq!(combat.decision.move_selector, 0);
        assert_eq!(combat.decision.attack_state, i32::from(!fail_sound));
        assert_eq!(combat.decision.aggression, if fail_sound { 50 } else { 40 });
        assert_eq!(cues, 1);
        assert_eq!(random.state(), if fail_sound { 42 } else { 1974836613 });
    }
    let mut snapshot = saved.clone();
    snapshot["actors"][0]["control"]["combat"]["action"] = (-1).into();
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
    let id = world.id(0).unwrap();
    let target = world.id(1).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let error = world
        .select_combat_defence(
            id,
            target,
            cyrus_game::script::decision::DefenceSelectionInput {
                armed: true,
                lockout_clear: true,
                own_selected: -1,
                target_in_arc: true,
                distance: 50,
                own_window_far: 100,
                facing_alike: false,
                target_in_combat_stance: true,
                attribute_58: 29,
                pacing_sum: 100,
                defence_pacing: 3,
                target_selected: -1,
                target_frame: 0,
                target_near: 0,
                target_third: 0,
                own_selector: 0,
            },
            cyrus_game::script::WeaponInputs {
                selected_player: target,
                item_system_ready: true,
                shell_execution: false,
            },
            1,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut random,
            &mut |_, _, _, _| anyhow::bail!("unexpected movement"),
            &mut |_, _| anyhow::bail!("unexpected sound"),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("attack slot 0: arc unavailable"));
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(combat.decision.defence_counter, 100);
    assert_eq!(combat.flags, 49);
    assert_eq!(combat.action, -1);
    assert_eq!(random.state(), 3397979675);
    // Best starts synchronously before paced selection consumes the next shared draw.
    let mut snapshot = saved.clone();
    let combat = &mut snapshot["actors"][0]["control"]["combat"];
    combat["action"] = (-1).into();
    combat["decision"]["attack_state"] = 0.into();
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
    let id = world.id(0).unwrap();
    let target = world.id(1).unwrap();
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    for actor in [id, target] {
        world
            .prepare_ordinary_collision_shape(actor, &data, &scene)
            .unwrap();
    }
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let mut cues = 0;
    let error = world
        .select_combat_defence(
            id,
            target,
            cyrus_game::script::decision::DefenceSelectionInput {
                armed: true,
                lockout_clear: true,
                own_selected: -1,
                target_in_arc: true,
                distance: 50,
                own_window_far: 100,
                facing_alike: false,
                target_in_combat_stance: true,
                attribute_58: 29,
                pacing_sum: 100,
                defence_pacing: 1,
                target_selected: 0,
                target_frame: 1,
                target_near: 0,
                target_third: 0,
                own_selector: 0,
            },
            cyrus_game::script::WeaponInputs {
                selected_player: target,
                item_system_ready: true,
                shell_execution: false,
            },
            1,
            ScriptInputs {
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut random,
            &mut |_, _, _, _| anyhow::bail!("unexpected movement"),
            &mut |_, _| {
                cues += 1;
                Ok(())
            },
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("attack slot 0: arc unavailable"));
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!((combat.action, combat.retained_action_indices[1]), (0, 0));
    assert_eq!(combat.decision.move_selector, 0);
    assert_eq!(combat.decision.attack_state, 1);
    assert_eq!(combat.decision.aggression, 40);
    assert_eq!(combat.decision.defence_counter, 100);
    assert_eq!(combat.flags, 1);
    assert_eq!(cues, 1);
    assert_eq!(random.state(), 238077914);
}

#[test]
fn current_combat_counter_query_uses_retained_slot_motion_and_idle_gate() {
    use cyrus_game::script::{WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
    scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
        ("attr_14".into(), 1),
        ("attr_73".into(), 1),
        ("attr_75".into(), 2),
    ]);
    scripts.instances[0].object_type = 1;
    scripts.combat_attacks = [
        vec![],
        vec![Some(0), Some(i32::MIN), Some(1), None],
        vec![Some(99), Some(3)],
    ]
    .into_iter()
    .map(|references| cyrus_assets::script::combat::Attack {
        is_combo: true,
        min_range: 0,
        max_range: 100,
        motions: references
            .into_iter()
            .enumerate()
            .map(|(section, counter)| {
                serde_json::from_value(serde_json::json!({
                    "section":section,"kind":0,"animation_group":0,"min_range":0,"max_range":100,
                    "first_collide_frame":0,"collide_duration":0,"defense":counter
                }))
                .unwrap()
            })
            .collect(),
    })
    .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "mpob_objects":[{"script_name":"LOGIC"},{"script_name":"LOGIC"}],
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "raan_entries":[{"frame_count":1,"file_path":"TEST.3D"}],
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]}),
        sections: Default::default(),
        models: BTreeMap::from([(
            "TEST".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "static_3d".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        )]),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    assert_eq!(world.current_combat_counter_attack(id).unwrap(), 0);
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    world.prepare_spatial().unwrap();
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for (action, slot, motion, expected) in [
        (-1, -99, -88, Ok(0)),
        (9, -99, -88, Ok(0)),
        (0, 5, 1, Ok(2)),
        (7, 5, 1, Ok(2)),
        (0, 3, 2, Ok(0)),
        (0, 3, 0, Ok(-1)),
        (0, 3, 1, Ok(i32::MAX)),
        (
            0,
            3,
            3,
            Err("attack slot 3 motion 3: counter-attack reference unavailable"),
        ),
        (0, 3, 4, Err("attack slot 3 motion 4: unavailable")),
        (0, 4, 0, Err("attack slot 4: unpopulated")),
    ] {
        let mut snapshot = saved.clone();
        let combat = &mut snapshot["actors"][0]["control"]["combat"];
        combat["action"] = action.into();
        combat["retained_action_indices"] = serde_json::json!([-1, slot]);
        combat["decision"]["move_selector"] = motion.into();
        let restored = ScriptWorld::from_snapshot(
            serde_json::from_value(snapshot).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let own = restored.id(0).unwrap();
        let target = restored.id(1).unwrap();
        // Rejected windows never query active metadata; best never needs counters.
        assert_eq!(
            restored
                .pick_combat_defence(own, target, 200, true)
                .unwrap(),
            None
        );
        assert_eq!(
            restored
                .pick_combat_defence(own, target, 50, false)
                .unwrap(),
            Some(3)
        );
        let open = restored.pick_combat_defence(own, target, 50, true);
        match expected {
            Ok(counter) => assert_eq!(open.unwrap(), Some(if counter == -1 { 5 } else { 3 })),
            Err(_) => assert!(open.is_err()),
        }
        let result = restored.current_combat_counter_attack(restored.id(0).unwrap());
        match expected {
            Ok(expected) => assert_eq!(result.unwrap(), expected),
            Err(expected) => {
                let error = format!("{:#}", result.unwrap_err());
                assert!(error.contains(expected), "{error}");
                assert!(
                    error.contains("instance 0 current counter-attack"),
                    "{error}"
                );
            }
        }
        assert!(restored.current_combat_counter_attack(id).is_err());
    }
}

#[test]
fn ground_prevalidation_retains_shared_results_after_destination_and_receiver_changes() {
    use cyrus_collision::ground::TerrainGround;
    use cyrus_game::script::{OrdinaryGroundPrevalidation, OrdinaryGroundQuery};
    for bypass_first in [false, true] {
        let mut scripts = cache();
        for placement in &mut scripts.instances {
            placement.active = 1;
        }
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert("attr_1".into(), 2);
        let (mut world, data, models) =
            static_model_fixture_at(scripts, Some(20.0), [600, 2400, 600]);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, other])
            .unwrap();
        world.snapshot_spatial(id).unwrap();
        let mut scene = ordinary_collision_scene(world.world, models);
        scene.settings.post_collide_height = 5000;
        let heights = (0_usize..256 * 256)
            .map(|index| ((index % 256).min(16) * 1024) as i16)
            .collect();
        scene.terrain = Some(TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap());
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
            .unwrap();
        let result = world
            .query_ordinary_ground_prevalidation(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 1,
                    sidedness: false,
                },
                bypass_first,
            )
            .unwrap();
        assert_eq!(
            result,
            OrdinaryGroundPrevalidation {
                rollback: false,
                sliding: !bypass_first
            }
        );
        let models = scene.actor_collision.take();
        scene.terrain = None;
        assert_eq!(
            world
                .query_ordinary_ground_prevalidation(
                    other,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    },
                    false
                )
                .unwrap(),
            result
        );
        scene.actor_collision = models;
        scene.terrain =
            Some(TerrainGround::new(vec![2400; 256 * 256], vec![false; 256 * 256], None).unwrap());
        world
            .update_ordinary(
                &data,
                [600, 600],
                cyrus_game::script::OrdinaryInputs {
                    scripts: cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        for actor in world.actors() {
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
        }
        assert_eq!(
            world
                .query_ordinary_ground_prevalidation(
                    id,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 1,
                        sidedness: false,
                    },
                    false
                )
                .unwrap(),
            OrdinaryGroundPrevalidation::default()
        );
    }
}

#[test]
fn ground_prevalidation_does_not_turn_an_unsupported_first_query_into_success() {
    use cyrus_game::script::OrdinaryGroundQuery;
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_1".into(), 2);
    let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    world.snapshot_spatial(id).unwrap();
    let mut scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id])
        .unwrap();
    scene.actor_collision = None;
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 1,
        sidedness: false,
    };
    let error = world
        .query_ordinary_ground_prevalidation(id, query(), false)
        .unwrap_err();
    assert!(format!("{error:#}").contains("live geometry unavailable"));
    let error = world
        .query_ordinary_ground_prevalidation(id, query(), true)
        .unwrap_err();
    assert!(format!("{error:#}").contains("earlier unsupported query"));
}

#[test]
fn native_failure_preserves_written_flags_and_restores_runtime() {
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "PlayerLineUp".into(),
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: set_flag(73),
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: wait(1, "PlayerLineUp", CallKind::Function, vec![]),
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
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let error = world.step(id, Thread::Main, 20).unwrap_err();
    assert!(format!("{error:#}").contains("PlayerLineUp"));
    assert_eq!(world.flags(), [73]);
    assert!(world.actors()[0].runtime().unwrap().failure().is_some());
    assert!(world.step(id, Thread::Main, 20).is_err());
    assert_eq!(world.flags(), [73]);
}

#[test]
fn destination_support_restores_all_position_axes_on_success_miss_and_error() {
    use cyrus_game::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    for outcome in ["support", "miss", "error"] {
        let mut scripts = cache();
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert("attr_1".into(), 4);
        let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [10, 20, 30]);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, other])
            .unwrap();
        world.snapshot_spatial(id).unwrap();
        let saved = world.spatial(id).unwrap().local.position;
        let mut scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
            .unwrap();
        if outcome == "error" {
            scene.actor_collision = None;
        }
        let destination = if outcome == "miss" {
            [2000 * 256, 21 * 256, 2000 * 256]
        } else {
            [11 * 256, 21 * 256, 31 * 256]
        };
        let result = world.admit_ordinary_destination(
            id,
            destination,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 50,
                sidedness: false,
            },
            ObstacleEnvironment {
                selected_player: None,
                auto_grab: false,
                item_system_ready: true,
                shell_execution: false,
                ticks: 0,
            },
            &mut |_, _| Ok(()),
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            saved,
            "{outcome}"
        );
        match outcome {
            "support" => {
                assert!(result.unwrap());
                assert_eq!(world.model_support(id).unwrap().actor, Some(other));
            }
            "miss" => assert!(!result.unwrap()),
            _ => {
                assert!(format!("{:#}", result.unwrap_err()).contains("live geometry unavailable"))
            }
        }
    }
}

#[test]
fn destination_support_rejects_steep_ground_after_prevalidation_bypass() {
    use cyrus_collision::ground::{SelectedSupport, TerrainGround};
    use cyrus_game::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .insert("attr_1".into(), 2);
    let (mut world, data, models) = static_model_fixture_at(scripts, Some(20.0), [600, 2400, 600]);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    world.snapshot_spatial(id).unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 5000;
    let heights = (0_usize..256 * 256)
        .map(|index| ((index % 256).min(16) * 1024) as i16)
        .collect();
    scene.terrain = Some(TerrainGround::new(heights, vec![false; 256 * 256], None).unwrap());
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [600, 600], &[id])
        .unwrap();
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 5000,
        sidedness: false,
    };
    let selected = world.query_ordinary_ground(id, query()).unwrap().unwrap();
    assert!(
        matches!(selected, SelectedSupport::Primary { support, .. } if support.classification() == 2)
    );
    world
        .query_ordinary_ground_prevalidation(id, query(), true)
        .unwrap();
    assert!(
        !world
            .admit_ordinary_destination(
                id,
                saved,
                query(),
                ObstacleEnvironment {
                    selected_player: None,
                    auto_grab: false,
                    item_system_ready: true,
                    shell_execution: false,
                    ticks: 0
                },
                &mut |_, _| Ok(()),
            )
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, saved);
}

#[test]
fn destination_body_checks_models_without_moving_the_receiver() {
    use cyrus_game::script::OrdinaryObstacleQuery;
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    for (height, expected) in [(20, false), (100, true)] {
        assert_eq!(
            world
                .ordinary_destination_body_clear(
                    id,
                    [10 * 256, height * 256, 30 * 256],
                    OrdinaryObstacleQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        vertical_offset: 0,
                        respond: false
                    },
                )
                .unwrap(),
            expected
        );
        assert_eq!(world.spatial(id).unwrap().local.position, saved);
    }
}

#[test]
fn walk_forward_body_refusal_marks_step_without_running_later_ground_query() {
    use cyrus_game::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    assert!(
        !world
            .walk_forward_step_clear(
                id,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: u32::MAX,
                    sidedness: false
                },
                ObstacleEnvironment {
                    selected_player: None,
                    auto_grab: false,
                    item_system_ready: true,
                    shell_execution: false,
                    ticks: 0
                },
                &mut |_, _| Ok(()),
            )
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, saved);
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .collision_response_auxiliary[0],
        0x80
    );
    world.finish_ordinary_script_response(id, Some(1)).unwrap();
    assert_eq!(
        world.actors()[id.index]
            .control_state()
            .unwrap()
            .collision_response_auxiliary[0],
        0
    );
}

#[test]
fn directional_body_refusal_retains_markers_and_skips_support_without_translation() {
    use cyrus_game::script::{ObstacleEnvironment, OrdinaryGroundQuery, OrdinaryStepDirection};
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let saved = world.spatial(id).unwrap().local.position;
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    for (direction, markers) in [
        (OrdinaryStepDirection::Left, [0x20, 0]),
        (OrdinaryStepDirection::Right, [0x60, 0]),
        (OrdinaryStepDirection::Backward, [0x60, 1]),
    ] {
        assert!(
            !world
                .ordinary_step_clear(
                    id,
                    direction,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: u32::MAX,
                        sidedness: false
                    },
                    ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: true,
                        shell_execution: false,
                        ticks: 0
                    },
                    &mut |_, _| panic!("body refusal must not reach response sounds"),
                )
                .unwrap(),
            "{direction:?}"
        );
        assert_eq!(world.spatial(id).unwrap().local.position, saved);
        assert_eq!(
            world.actors()[id.index]
                .control_state()
                .unwrap()
                .collision_response_auxiliary,
            markers
        );
    }
}

#[test]
fn directional_early_results_precede_geometry_and_preserve_status_bytes() {
    use cyrus_game::script::{
        ObstacleEnvironment, OrdinaryGroundQuery, OrdinaryStepDirection, WorldRestoreContext,
        WorldSnapshotSources,
    };
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
        instance.map_position = [10, 20, 30];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, models) = static_model_fixture_at(scripts.clone(), Some(20.0), [10, 20, 30]);
    let scene = ordinary_collision_scene(world.world, models);
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for rope in [false, true] {
        for direction in [
            OrdinaryStepDirection::Left,
            OrdinaryStepDirection::Right,
            OrdinaryStepDirection::Backward,
        ] {
            let mut snapshot = saved.clone();
            snapshot["actors"][0]["control"]["rope_attached"] = rope.into();
            snapshot["actors"][0]["control"]["death_selector"] = 7.into();
            snapshot["actors"][0]["control"]["collision_response_auxiliary"] =
                serde_json::json!([0x95, 0xA2]);
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
            let id = world.id(0).unwrap();
            let other = world.id(1).unwrap();
            world
                .refresh_model_presentations(&data, &[id, other])
                .unwrap();
            let prepared = world
                .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
                .unwrap();
            // A mismatched scene must never be read by either early branch.
            let (_, _, unrelated_models) = static_model_fixture(cache());
            let unavailable = ordinary_collision_scene(999, unrelated_models);
            assert_eq!(
                world
                    .ordinary_step_clear(
                        id,
                        direction,
                        OrdinaryGroundQuery {
                            scene: &unavailable,
                            animations: &data,
                            prepared: &prepared,
                            distance: u32::MAX,
                            sidedness: false
                        },
                        ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: false,
                            shell_execution: false,
                            ticks: 0
                        },
                        &mut |_, _| panic!("early result reached sounds")
                    )
                    .unwrap(),
                !rope
            );
            assert_eq!(
                world.actors()[0]
                    .control_state()
                    .unwrap()
                    .collision_response_auxiliary,
                [0x95, 0xA2]
            );
        }
    }
}

#[test]
fn native_walk_forward_uses_live_body_admission_on_call_and_poll() {
    use cyrus_game::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, ScriptInputs, ScriptStage,
    };
    let mut scripts = cache();
    scripts.definition.functions.push(Function {
        name: "WalkForward".into(),
        param_count: 1,
        kind: CallKind::Task,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: wait(
                    1,
                    "WalkForward",
                    CallKind::Task,
                    vec![Value::Integer { value: 4 }],
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
    for query_owner in [0, 1] {
        let (mut world, data, models) =
            static_model_fixture_at(scripts.clone(), Some(20.0), [10, 20, 30]);
        let id = world.id(0).unwrap();
        let other = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, other])
            .unwrap();
        let saved = world.spatial(id).unwrap().local.position;
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(
                if query_owner == 0 { id } else { other },
                &scene,
                &data,
                [10, 30],
                &[id, other],
            )
            .unwrap();
        let hud = cyrus_game::script::ScriptHud::default();
        for stage in [ScriptStage::Body(Thread::Main), ScriptStage::Tasks] {
            let result = world.advance(
                id,
                stage,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    forward_step_clear: Some(true),
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: u32::MAX,
                            sidedness: false,
                        },
                        environment: ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: false,
                            shell_execution: false,
                            ticks: 1,
                        },
                    }),
                    ..Default::default()
                },
                None,
                None,
                30,
            );
            if query_owner == 1 {
                let error = result.unwrap_err();
                assert!(
                    format!("{error:#}")
                        .contains("collision query belongs to another actor or world"),
                    "{error:#}"
                );
                assert_eq!(world.spatial(id).unwrap().local.position, saved);
                assert_eq!(
                    world.actors()[id.index]
                        .control_state()
                        .unwrap()
                        .collision_response_auxiliary[0],
                    0
                );
                break;
            }
            result.unwrap();
            assert_eq!(world.spatial(id).unwrap().local.position, saved);
            assert_eq!(
                world.actors()[id.index]
                    .control_state()
                    .unwrap()
                    .collision_response_auxiliary[0],
                0x80
            );
            assert_eq!(
                world.actors()[id.index]
                    .control_state()
                    .unwrap()
                    .motion_direction,
                0
            );
            world.finish_ordinary_script_response(id, Some(1)).unwrap();
        }
        assert_eq!(
            world.actors()[id.index].runtime().unwrap().active_tasks(),
            0
        );
    }
}

fn facing_world(
    heading: u32,
    destination: [i64; 3],
    group: Option<u16>,
) -> (
    ScriptWorld,
    cyrus_game::script::ScriptActorId,
    cyrus_game::script::ScriptActorId,
) {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    let mut scripts = cache();
    scripts.instances[0].map_position = [0, 5, 0];
    scripts.instances[0].angles[1] = heading;
    scripts.instances[1].map_position = destination;
    for (attribute, value) in [
        (8, 4),
        (9, 5),
        (10, 6),
        (12, 2),
        (13, 3),
        (15, 16),
        (17, 8),
        (19, 7),
        (21, 32),
    ] {
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert(format!("attr_{attribute}"), value);
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let target = world.id(1).unwrap();
    if let Some(group) = group {
        let mut groups = vec![group, 14, 15, 16, 17, 96, 97];
        groups.sort_unstable();
        groups.dedup();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
                "actors":[{"script_name":"LOGIC","animation_groups":groups.into_iter().map(|group| serde_json::json!({
                    "group_index":group,"anim_id":group+1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":group},{"opcode":2,"value":0}]
                })).collect::<Vec<_>>()}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        world.bind_animation(id, &data, group).unwrap();
        world
            .advance(
                id,
                ScriptStage::Animation,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                20,
            )
            .unwrap();
    }
    (world, id, target)
}

#[test]
fn ordinary_facing_snaps_at_each_current_group_speed_boundary_and_preserves_y_and_heading() {
    use cyrus_game::script::OrdinarySlopeInputs;
    for (group, units) in [
        (None, 1),
        (Some(0), 1),
        (Some(18), 5),
        (Some(30), 5),
        (Some(142), 5),
        (Some(19), 6),
        (Some(143), 6),
        (Some(20), 3),
        (Some(21), 3),
        (Some(22), 4),
        (Some(23), 4),
        (Some(24), 7),
        (Some(25), 7),
        (Some(26), 7),
        (Some(27), 7),
        (Some(31), 8),
        (Some(92), 9),
        (Some(93), 9),
    ] {
        let (mut world, id, target) = facing_world(400, [units, -3, 0], group);
        let previous_frame = world.animation_frame(id).unwrap();
        let angle = world
            .face_ordinary_target(
                id,
                target,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0,
                },
                &mut |_, _| panic!("arrival must not request animation sounds"),
            )
            .unwrap();
        assert_eq!(angle, 0, "{group:?}");
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [units as i32 * 256, 1280, 0],
            "{group:?}"
        );
        assert_eq!(world.spatial(id).unwrap().local.angles[1], 400);
        assert_eq!(world.animation_frame(id).unwrap(), previous_frame);
    }
}

#[test]
fn ordinary_facing_returns_pre_turn_angle_and_uses_independent_weapon_rate_and_groups() {
    use cyrus_game::script::OrdinarySlopeInputs;
    for (heading, weapon, combat, expected_heading, expected_angle, expected_group) in [
        (1408, false, false, 1424, 128, 14),
        (1664, false, false, 1648, -128, 15),
        (1408, false, true, 1440, 128, 14),
        (1408, true, false, 1440, 128, 96),
        (1664, true, true, 1632, -128, 97),
        (1520, false, false, 1536, 0, 0),
    ] {
        let (mut world, id, target) = facing_world(heading, [10, -3, 0], Some(0));
        assert_eq!(
            world
                .face_ordinary_target(
                    id,
                    target,
                    OrdinarySlopeInputs {
                        selected_player: None,
                        holding_weapon: weapon,
                        combat_active: combat,
                        frame_scale: 256,
                        ticks: 0,
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            expected_angle
        );
        assert_eq!(world.spatial(id).unwrap().local.angles[1], expected_heading);
        assert_eq!(world.animation_frame(id).unwrap(), Some(expected_group));
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 1280, 0]);
    }
}

#[test]
fn ordinary_facing_handles_absent_animation_wrapping_scale_and_selected_player_rejection() {
    use cyrus_game::script::OrdinarySlopeInputs;
    let (mut world, id, target) = facing_world(1424, [2, -3, 0], None);
    assert_eq!(
        world
            .face_ordinary_target(
                id,
                target,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: 256,
                    ticks: 0,
                },
                &mut |_, _| panic!("absent animation requested sound")
            )
            .unwrap(),
        112
    );
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 1440);
    let before = world.spatial(id).unwrap().local;
    let error = world
        .face_ordinary_target(
            id,
            target,
            OrdinarySlopeInputs {
                selected_player: Some(id),
                holding_weapon: false,
                combat_active: false,
                frame_scale: 256,
                ticks: 0,
            },
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("selected-player facing camera unavailable"));
    assert_eq!(world.spatial(id).unwrap().local.position, before.position);
    assert_eq!(world.spatial(id).unwrap().local.angles, before.angles);
    let (mut world, id, target) = facing_world(400, [0, -3, 0], None);
    assert_eq!(
        world
            .face_ordinary_target(
                id,
                target,
                OrdinarySlopeInputs {
                    selected_player: None,
                    holding_weapon: false,
                    combat_active: false,
                    frame_scale: u32::MAX,
                    ticks: 0,
                },
                &mut |_, _| Ok(())
            )
            .unwrap(),
        -400
    );
    assert_eq!(world.spatial(id).unwrap().local.angles[1], 392);
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 1280, 0]);
}

#[test]
fn ordinary_facing_arrival_registers_cell_movement_until_the_deferred_pass() {
    use cyrus_game::script::OrdinarySlopeInputs;
    for excluded in [false, true] {
        let mut scripts = cache();
        scripts.instances[0].map_position = [512, 5, 512];
        scripts.instances[1].map_position = [511, -3, 512];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let id = world.id(0).unwrap();
        let target = world.id(1).unwrap();
        world.ordinary_actors([0, 0]).unwrap();
        for _ in 0..2 {
            assert_eq!(
                world
                    .face_ordinary_target(
                        id,
                        target,
                        OrdinarySlopeInputs {
                            selected_player: None,
                            holding_weapon: false,
                            combat_active: false,
                            frame_scale: 256,
                            ticks: 0,
                        },
                        &mut |_, _| Ok(())
                    )
                    .unwrap(),
                0
            );
        }
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [130816, 1280, 131072]
        );
        world.flush_map_updates([0; 3], excluded).unwrap();
        world.ordinary_actors([6000, 6000]).unwrap();
        assert_eq!(
            world
                .ordinary_actors([0, 0])
                .unwrap()
                .iter()
                .map(|id| id.index)
                .collect::<Vec<_>>(),
            if excluded { vec![1, 0] } else { vec![0, 1] }
        );
    }
}

fn forward_amount_world(
    initial_group: u16,
    suppressed: bool,
    animation_system: bool,
    edit_actor: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    movement_fixture(
        initial_group,
        suppressed,
        animation_system,
        |_| {},
        |saved| {
            edit_actor(&mut saved["actors"][0]);
        },
    )
    .0
}

fn movement_fixture(
    initial_group: u16,
    suppressed: bool,
    animation_system: bool,
    edit_scripts: impl FnOnce(&mut Scripts),
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    use cyrus_game::script::{
        ScriptInputs, ScriptStage, WorldRestoreContext, WorldSnapshotSources,
    };
    let mut scripts = cache();
    scripts.updates.animation_system = animation_system;
    for (attribute, value) in [
        (8, 8),
        (9, 12),
        (11, 1),
        (12, 4),
        (16, 6),
        (17, 10),
        (18, 6),
        (19, 3),
        (83, 4),
    ] {
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .attributes
            .insert(format!("attr_{attribute}"), value);
    }
    edit_scripts(&mut scripts);
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","animation_groups":([0,18,19,20,21,30,31,92,93,94,95,106,142].into_iter().map(|group| {
                let mut commands = vec![];
                if suppressed {commands.push(serde_json::json!({"opcode":5}));}
                commands.push(serde_json::json!({"opcode":0,"frame_reference":group}));
                commands.push(serde_json::json!({"opcode":2,"value":0}));
                serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":commands})
            }).collect::<Vec<_>>())}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world.bind_animation(id, &data, initial_group).unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let collision = Default::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    // This logical fixture retains playback but has no rendered placement frame.
    saved["actors"][0]["animation"]["frame"] = serde_json::Value::Null;
    saved["actors"][0]["animation"]["playback"]["default"] = 0.into();
    edit_saved(&mut saved);
    let world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: 2,
            globals: Default::default(),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    (world, scripts, data)
}

fn forward_amount_inputs() -> cyrus_game::script::OrdinaryForwardAmountInputs {
    cyrus_game::script::OrdinaryForwardAmountInputs {
        selected_player: None,
        holding_weapon: false,
        frame_scale: 256,
        ticks: 1,
        duration: 1,
        elapsed: 0,
    }
}

#[test]
fn forward_amount_advances_shared_counter_and_called_state_without_translation() {
    let mut world = forward_amount_world(18, false, true, |_| {});
    let id = world.id(0).unwrap();
    let before = world.spatial(id).unwrap().local;
    for (amount, counter) in [(512, 0x10), (682, 0x20), (1024, 0x30), (2048, 0x30)] {
        assert_eq!(
            world
                .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()))
                .unwrap(),
            amount
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.movement_flags, counter);
        assert_eq!(control.motion_direction, 2);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
        assert_eq!(world.spatial(id).unwrap().local.position, before.position);
        assert_eq!(world.spatial(id).unwrap().local.angles, before.angles);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn forward_amount_uses_live_combat_action_surface_torch_and_suppression() {
    use cyrus_game::script::CombatControlState;
    for (weapon, combat, action, surface, torch, group, amount) in [
        (false, false, -1, 0, 0, 18, 2048),
        (false, false, -1, 0, 1, 30, 2048),
        (false, false, 4, 2, 1, 142, 1024),
        (true, false, -1, 0, 0, 92, 2560),
        (false, true, 4, 0, 0, 106, 1536),
        (true, true, 4, 4, 0, 106, 768),
        (false, true, -1, 2, 0, 92, 1280),
    ] {
        for (suppressed, scale, multiplier) in
            [(false, 128, 1), (true, 128, 2), (false, u32::MAX, -1)]
        {
            let mut world = forward_amount_world(group, suppressed, true, |actor| {
                let control = &mut actor["control"];
                control["movement_flags"] = 0xF3.into();
                control["torch_burning"] = torch.into();
                let state = CombatControlState {
                    flags: if combat { 4 } else { 0 },
                    action,
                    ..Default::default()
                };
                control["combat"] = serde_json::to_value(state).unwrap();
            });
            let id = world.id(0).unwrap();
            world.set_actor_surface(id, surface).unwrap();
            let mut inputs = forward_amount_inputs();
            inputs.holding_weapon = weapon;
            inputs.frame_scale = scale;
            let expected = if multiplier == -1 {
                -amount / 256
            } else {
                amount / 2 * multiplier
            };
            assert_eq!(
                world
                    .ordinary_forward_amount(id, inputs, None, &mut |_, _| Ok(()))
                    .unwrap(),
                expected,
                "weapon {weapon} combat {combat} action {action} surface {surface} suppressed {suppressed} scale {scale}"
            );
            assert_eq!(
                world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
                group
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().movement_flags,
                0xF3
            );
        }
    }
}

#[test]
fn forward_amount_guards_precede_animation_and_geometry_and_keep_shared_state() {
    for (field, value) in [
        ("rope_attached", serde_json::json!(true)),
        ("sliding_state", 1.into()),
        ("falling_state", 1.into()),
        ("queued_motion", 1.into()),
        ("living_state", 15.into()),
    ] {
        let mut world = forward_amount_world(18, false, true, |actor| {
            actor["animation"] = serde_json::Value::Null;
            let control = &mut actor["control"];
            control[field] = value;
            control["movement_flags"] = 0xC3.into();
            control["motion_direction"] = 99.into();
        });
        let id = world.id(0).unwrap();
        assert_eq!(
            world
                .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| panic!(
                    "refused query reached sounds"
                ))
                .unwrap(),
            0
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.movement_flags, 0xC3);
        assert_eq!(control.motion_direction, 99);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
    }
}

#[test]
fn forward_amount_waits_for_current_animation_without_advancing_counter() {
    for (enabled, duration, elapsed, expected, counter, called) in [
        (true, 4, 0, 1, 0xC3, 0),
        (true, 1, 0, 0, 0xC3, 0),
        (true, 4, 3, 0, 0xC3, 0),
        (false, 4, 0, 512, 0xD3, 4),
    ] {
        let mut world = forward_amount_world(30, false, enabled, |actor| {
            actor["control"]["movement_flags"] = 0xC3.into();
            actor["control"]["motion_direction"] = 99.into();
        });
        let id = world.id(0).unwrap();
        let mut inputs = forward_amount_inputs();
        inputs.duration = duration;
        inputs.elapsed = elapsed;
        assert_eq!(
            world
                .ordinary_forward_amount(id, inputs, None, &mut |_, _| Ok(()))
                .unwrap(),
            expected
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_direction, 2);
        assert_eq!(control.movement_flags, counter);
        let state = world.inspect(0, 1);
        assert_eq!(state["globals"]["locomotion_called"], called);
        assert_eq!(state["actors"][0]["animation"]["current_group"], 30);
        assert_eq!(state["actors"][0]["animation"]["pending_group"], 18);
    }
}

#[test]
fn forward_amount_requires_live_step_geometry_only_below_saturated_counter() {
    for counter in [0xC3, 0xD3, 0xE3, 0xF3] {
        let mut world = forward_amount_world(18, false, true, |actor| {
            actor["attributes"][1] = 2.into();
            actor["control"]["movement_flags"] = counter.into();
            actor["control"]["motion_direction"] = 99.into();
        });
        let id = world.id(0).unwrap();
        let result =
            world.ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()));
        if counter == 0xF3 {
            assert_eq!(result.unwrap(), 2048);
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("world 7 instance 0 forward amount"),
                "{error}"
            );
            assert!(error.contains("step geometry unavailable"), "{error}");
            assert_eq!(
                world.actors()[0].control_state().unwrap().motion_direction,
                99
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().movement_flags,
                counter
            );
            assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 0);
        }
    }
}

#[test]
fn forward_amount_halves_deep_water_before_the_shared_startup_ramp() {
    let mut world = forward_amount_world(142, false, true, |_| {});
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, 2).unwrap();
    for expected in [256, 341, 512, 1024] {
        assert_eq!(
            world
                .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()))
                .unwrap(),
            expected
        );
    }
}

#[test]
fn synchronous_forward_resamples_the_counter_mutated_by_the_amount_query() {
    use cyrus_game::script::{OrdinaryMovement, ScriptHud, ScriptInputs};
    let mut world = forward_amount_world(18, false, true, |_| {});
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    assert_eq!(
        world
            .ordinary_forward_amount(id, forward_amount_inputs(), None, &mut |_, _| Ok(()))
            .unwrap(),
        512
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
    for (expected_position, expected_counter) in [([0, 0, 682], 0x20), ([0, 0, 1706], 0x30)] {
        assert_eq!(
            world
                .move_ordinary(
                    id,
                    OrdinaryMovement::Forward,
                    1,
                    0,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        forward_step_clear: Some(true),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            0
        );
        assert_eq!(world.spatial(id).unwrap().local.position, expected_position);
        assert_eq!(
            world.actors()[0].control_state().unwrap().movement_flags,
            expected_counter
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
    }
}

#[test]
fn synchronous_movement_retains_duration_and_animation_wait_results_without_tasks() {
    use cyrus_game::script::{OrdinaryMovement, ScriptHud, ScriptInputs};
    for (group, enabled, duration, elapsed, expected, position, counter) in [
        (18, true, 4, 0, 1, [0, 0, 512], 0x10),
        (18, true, 4, 3, 0, [0, 0, 512], 0x10),
        (30, true, 4, 0, 1, [0, 0, 0], 0),
        (30, true, 1, 0, 0, [0, 0, 0], 0),
        (30, false, 1, 0, 0, [0, 0, 512], 0x10),
    ] {
        let mut world = forward_amount_world(group, false, enabled, |_| {});
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert_eq!(
            world
                .move_ordinary(
                    id,
                    OrdinaryMovement::Forward,
                    duration,
                    elapsed,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        forward_step_clear: Some(true),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            expected
        );
        assert_eq!(world.spatial(id).unwrap().local.position, position);
        assert_eq!(
            world.actors()[0].control_state().unwrap().movement_flags,
            counter
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn synchronous_backward_and_combat_strafes_use_existing_real_movement() {
    use cyrus_game::script::{CombatControlState, OrdinaryMovement, ScriptHud, ScriptInputs};
    for (movement, group, position, direction, counter, called) in [
        (OrdinaryMovement::Backward, 19, [0, 0, -768], 7, 0x40, 8),
        (OrdinaryMovement::Left, 18, [-1024, 0, 0], 4, 0, 0),
        (OrdinaryMovement::Right, 18, [1024, 0, 0], 5, 0, 0),
    ] {
        let mut world = forward_amount_world(group, false, true, |actor| {
            if matches!(movement, OrdinaryMovement::Left | OrdinaryMovement::Right) {
                actor["control"]["combat"] = serde_json::to_value(CombatControlState {
                    flags: 4,
                    ..Default::default()
                })
                .unwrap();
            }
        });
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert_eq!(
            world
                .move_ordinary(
                    id,
                    movement,
                    1,
                    0,
                    ScriptInputs {
                        frame_scale: Some(256),
                        nominal_ticks: Some(1),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            0
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            position,
            "{movement:?}"
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_direction, direction);
        assert_eq!(control.movement_flags, counter);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], called);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn synchronous_walk_forward_uses_live_body_refusal_without_creating_a_task() {
    use cyrus_game::script::{
        LocomotionGeometry, ObstacleEnvironment, OrdinaryGroundQuery, OrdinaryMovement, ScriptHud,
        ScriptInputs,
    };
    let (mut world, data, models) = static_model_fixture_at(cache(), Some(20.0), [10, 20, 30]);
    let id = world.id(0).unwrap();
    let other = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[id, other])
        .unwrap();
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[id, other])
        .unwrap();
    let hud = ScriptHud::default();
    let before = world.spatial(id).unwrap().local.position;
    assert_eq!(
        world
            .move_ordinary(
                id,
                OrdinaryMovement::WalkForward,
                1,
                0,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    forward_step_clear: Some(true),
                    locomotion_geometry: Some(LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: u32::MAX,
                            sidedness: false
                        },
                        environment: ObstacleEnvironment {
                            selected_player: None,
                            auto_grab: false,
                            item_system_ready: true,
                            shell_execution: false,
                            ticks: 0
                        }
                    }),
                    ..Default::default()
                },
                &mut |_, _| panic!("body refusal reached sounds")
            )
            .unwrap(),
        0
    );
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .collision_response_auxiliary,
        [0x80, 0]
    );
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn synchronous_movement_rejects_unknown_timing_before_receiver_writes() {
    use cyrus_game::script::{OrdinaryMovement, ScriptInputs};
    for (scale, ticks) in [(None, Some(1)), (Some(256), None)] {
        let mut world = forward_amount_world(18, false, true, |_| {});
        let id = world.id(0).unwrap();
        let before = world.inspect(0, 1);
        let error = world
            .move_ordinary(
                id,
                OrdinaryMovement::Forward,
                1,
                0,
                ScriptInputs {
                    frame_scale: scale,
                    nominal_ticks: ticks,
                    ..Default::default()
                },
                &mut |_, _| panic!("invalid timing reached sounds"),
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(
            error.contains("world 7 instance 0 Forward synchronous movement"),
            "{error}"
        );
        assert!(
            error.contains("requires frame_scale and nominal_ticks"),
            "{error}"
        );
        assert_eq!(world.inspect(0, 1), before);
    }
}

#[test]
fn neighbor_spacing_uses_normalized_quantized_local_vectors_and_signed_half_turns() {
    for (target, receiver, neighbor, expected) in [
        ([0; 3], [256, 256000000, 256], [256, 256, 256], -256),
        ([0; 3], [255, 0, 256], [256, 0, 256], -256),
        ([0; 3], [-1, 0, 256], [0, 0, 256], -256),
        ([0; 3], [0, 0, 256], [0, 0, -256], 1024),
        ([0; 3], [0, 0, -256], [0, 0, 256], -1024),
        (
            [-8388608, 0, 0],
            [i32::MAX, 0, 0],
            [i32::MIN + 256, 0, 0],
            -1024,
        ),
        ([0; 3], [0; 3], [0; 3], 0),
    ] {
        let mut scripts = cache();
        scripts.instances[0].map_position = target;
        add_standalone_receivers(&mut scripts);
        let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
        world.prepare_spatial().unwrap();
        let target = world.id(0).unwrap();
        let id = world.id(2).unwrap();
        let other = world.id(3).unwrap();
        world
            .initialize_player_spatial(id, receiver, [0; 3])
            .unwrap();
        world
            .initialize_player_spatial(other, neighbor, [0; 3])
            .unwrap();
        let before = serde_json::to_value(world.spatial(id).unwrap()).unwrap();
        assert_eq!(
            world
                .ordinary_neighbor_separation(id, target, Some(other))
                .unwrap(),
            expected,
            "receiver {receiver:?}, neighbor {neighbor:?}"
        );
        assert_eq!(
            serde_json::to_value(world.spatial(id).unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn absent_spacing_neighbor_bypasses_spatial_queries_and_errors_name_all_receivers() {
    let world = ScriptWorld::prepare(&cache(), 7).unwrap();
    let id = world.id(0).unwrap();
    let target = world.id(1).unwrap();
    assert_eq!(
        world
            .ordinary_neighbor_separation(id, target, None)
            .unwrap(),
        1024
    );
    let error = format!(
        "{:#}",
        world
            .ordinary_neighbor_separation(id, target, Some(target))
            .unwrap_err()
    );
    assert!(
        error.contains("world 7 instance 0 neighbor 1 target 1 spacing separation"),
        "{error}"
    );
    assert!(error.contains("spatial"), "{error}");
}

fn spacing_fixture(
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    spacing_fixture_group(18, edit_saved)
}

fn spacing_fixture_group(
    initial_group: u16,
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    movement_fixture(
        initial_group,
        false,
        true,
        |scripts| {
            scripts.non_engaged_spacing = Some(128);
            for index in 2..5 {
                let mut instance = scripts.instances[0].clone();
                instance.object_id = index;
                scripts.instances.push(instance);
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 5;
            actor.instance_variables = vec![vec![4]; 5];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            scripts.instances[0].angles[1] = 1536;
            scripts.instances[1].map_position = [80, 0, 0];
        },
        |saved| {
            for index in [0, 2, 3] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_game::script::CombatControlState {
                        action: -1,
                        flags: 4,
                        attacker_selection_state: 1,
                        spacing_period: 5,
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            edit_saved(saved);
        },
    )
}

fn movement_snapshot(
    world: &ScriptWorld,
    scripts: &Scripts,
    data: &cyrus_assets::animation::Animations,
) -> serde_json::Value {
    let collision = Default::default();
    serde_json::to_value(
        world
            .snapshot(&cyrus_game::script::WorldSnapshotSources {
                player: None,
                scripts,
                animations: data,
                collision: &collision,
            })
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn live_spacing_neighbors_keep_first_unmeasured_strict_order_and_previous_primary() {
    for (positions, expected) in [
        ([100000, 200, 100], [Some(0), Some(2)]),
        ([100, 200, 200], [Some(2), Some(3)]),
        ([100, 65535, 65535], [Some(3), None]),
    ] {
        let (mut world, _, _) = spacing_fixture(|saved| {
            // Actor 4 is the receiver so actor 0 is an unmeasured first attacker.
            for (index, x) in [3, 2, 0].into_iter().zip(positions) {
                saved["spatial"]["actors"][index]["local"]["position"] =
                    serde_json::json!([x, 999999, 0]);
            }
            saved["spatial"]["actors"][4]["local"]["position"] = serde_json::json!([0, 0, 0]);
        });
        let ids = world
            .ordinary_actors([0, 0])
            .unwrap()
            .iter()
            .map(|id| id.index)
            .collect::<Vec<_>>();
        assert_eq!(ids, [4, 3, 2, 1, 0]);
        let pair = world
            .ordinary_spacing_neighbors(world.id(4).unwrap(), world.id(1).unwrap(), None)
            .unwrap()
            .unwrap();
        assert_eq!(
            [
                pair.primary.map(|id| id.index),
                pair.secondary.map(|id| id.index)
            ],
            expected
        );
    }
}

#[test]
fn live_spacing_neighbor_filters_short_circuit_death_and_use_linked_distance() {
    let (mut world, _, _) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
        saved["actors"][2]["control"]["death_selector"] = 8.into();
        saved["actors"][3]["control"]["combat"]["target"] = serde_json::Value::Null;
    });
    world.ordinary_actors([0, 0]).unwrap();
    assert_eq!(
        world
            .ordinary_spacing_neighbors(world.id(0).unwrap(), world.id(1).unwrap(), None)
            .unwrap(),
        None
    );
    let (mut world, _, _) = spacing_fixture(|saved| {
        saved["spatial"]["actors"][2]["local"]["position"] = serde_json::json!([100, 0, 0]);
        saved["spatial"]["actors"][2]["linked_position"] = serde_json::json!([70000, 0, 0]);
        saved["spatial"]["actors"][3]["local"]["position"] = serde_json::json!([200, 999999, 0]);
    });
    world.ordinary_actors([0, 0]).unwrap();
    let pair = world
        .ordinary_spacing_neighbors(world.id(0).unwrap(), world.id(1).unwrap(), None)
        .unwrap()
        .unwrap();
    assert_eq!(pair.primary, Some(world.id(3).unwrap()));
    assert_eq!(pair.secondary, None);
}

#[test]
fn live_neighbor_sidestep_translates_synchronously_and_ignores_completion() {
    use cyrus_game::script::{ScriptHud, ScriptInputs};
    for disabled in [false, true] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 25600]);
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][2]["local"]["position"] = serde_json::json!([256, 0, 25600]);
            saved["actors"][3]["control"]["combat"] = serde_json::Value::Null;
            if disabled {
                saved["actors"][0]["control"]["rope_attached"] = true.into();
            }
        });
        world.ordinary_actors([0, 0]).unwrap();
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        assert!(
            world
                .space_ordinary_neighbors(
                    id,
                    world.id(1).unwrap(),
                    ScriptInputs {
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    &mut |_, _| Ok(())
                )
                .unwrap()
        );
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            if disabled {
                [0, 0, 25600]
            } else {
                [1024, 0, 25600]
            }
        );
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn live_spacing_root_registers_idle_passes_once_and_keeps_rng_and_tasks_untouched() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = spacing_fixture(|_| {});
    world.ordinary_actors([0, 0]).unwrap();
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::default();
    for tick_counter in [0, 2, 5] {
        world
            .space_ordinary(
                id,
                OrdinarySpacingInput {
                    distance: 20480,
                    tick_counter,
                },
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([0])
        );
        assert_eq!(random.state(), 1);
        assert_eq!(world.spatial(id).unwrap().local.position, [0; 3]);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
    world.flush_map_updates([0; 3], true).unwrap();
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["map_updates"],
        serde_json::json!([])
    );
}

#[test]
fn live_spacing_root_preserves_action_turn_and_clock_admission_and_nonstance_draws() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let sample = OrdinarySpacingInput {
        distance: 20480,
        tick_counter: 0,
    };
    let (mut world, scripts, data) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["combat"]["action"] = 4.into();
    });
    let id = world.id(0).unwrap();
    let before = movement_snapshot(&world, &scripts, &data);
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .space_ordinary(
            id,
            sample,
            ScriptInputs::default(),
            &mut random,
            &mut |_, _| panic!("inactive action dispatched sound"),
        )
        .unwrap();
    assert_eq!(movement_snapshot(&world, &scripts, &data), before);
    for (stance, heading, expected_state) in [(1, 1408, 1), (0, 0, 2524885223)] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = stance.into();
            saved["actors"][0]["animation"] = serde_json::Value::Null;
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, heading, 0]);
        });
        let id = world.id(0).unwrap();
        let before = world.inspect(0, 1);
        let mut random = cyrus_game::random::EngineRandom::default();
        let error = world
            .space_ordinary(
                id,
                sample,
                ScriptInputs::default(),
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("frame_scale"));
        assert_eq!(world.inspect(0, 1), before);
        assert_eq!(random.state(), 1);
        let hud = ScriptHud::default();
        world
            .space_ordinary(
                id,
                sample,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(random.state(), expected_state);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn live_spacing_root_reports_signed_division_faults_before_rng_and_registration() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (period, tick_counter) in [(0, 0), (-1, i32::MIN)] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["spacing_period"] = period.into();
        });
        world.ordinary_actors([0, 0]).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::default();
        let error = world
            .space_ordinary(
                world.id(0).unwrap(),
                OrdinarySpacingInput {
                    distance: 20480,
                    tick_counter,
                },
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(
            error.contains("world 7 instance 0 fight spacing") && error.contains("division fault"),
            "{error}"
        );
        assert_eq!(random.state(), 1);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([])
        );
    }
}

#[test]
fn live_spacing_root_queries_amount_then_moves_only_when_midpoint_is_retained() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (step, expected) in [(10, [2560, 0, 0]), (40, [0; 3])] {
        let (mut world, scripts, data) = spacing_fixture_group(92, |saved| {
            saved["actors"][0]["attributes"][17] = step.into();
            saved["actors"][0]["control"]["movement_flags"] = 0x30.into();
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([25600, 0, 0]);
        });
        let id = world.id(0).unwrap();
        world.refresh_transform(id).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::default();
        world
            .space_ordinary(
                id,
                OrdinarySpacingInput {
                    distance: 25600,
                    tick_counter: 1,
                },
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    forward_step_clear: Some(true),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, expected);
        assert_eq!(world.inspect(0, 1)["globals"]["locomotion_called"], 4);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        assert_eq!(random.state(), 1);
        assert_eq!(
            movement_snapshot(&world, &scripts, &data)["map_updates"],
            serde_json::json!([0])
        );
    }
}

#[test]
fn live_spacing_root_registers_far_pass_without_available_walk_animation() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let (mut world, scripts, data) = spacing_fixture(|saved| {
        saved["actors"][0]["animation"] = serde_json::Value::Null;
    });
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::default();
    world
        .space_ordinary(
            world.id(0).unwrap(),
            OrdinarySpacingInput {
                distance: 100000,
                tick_counter: 1,
            },
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                hud: Some(&hud),
                shell_execution: Some(false),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        movement_snapshot(&world, &scripts, &data)["map_updates"],
        serde_json::json!([0])
    );
    assert_eq!(random.state(), 1);
}

#[test]
fn sight_uses_attribute_selected_attached_then_linked_then_local_positions() {
    use cyrus_game::script::{OrdinaryObstacleQuery, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
        instance.map_position = [0; 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, models) = static_model_fixture_at(scripts.clone(), None, [0; 3]);
    let scene = ordinary_collision_scene(world.world, models);
    let sources = WorldSnapshotSources {
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

#[test]
fn combat_decision_prefix_admission_precedes_target_and_geometry_queries() {
    for (selector, combat) in [(5, true), (8, true), (6, false)] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["death_selector"] = selector.into();
            if !combat {
                saved["actors"][0]["control"]["combat"] = serde_json::Value::Null;
            }
        });
        let before = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            world
                .prepare_ordinary_combat_decision(
                    world.id(0).unwrap(),
                    cyrus_game::script::ScriptActorId {
                        generation: 999,
                        index: 999
                    },
                    None
                )
                .unwrap(),
            None
        );
        assert_eq!(movement_snapshot(&world, &scripts, &data), before);
    }
}

#[test]
fn combat_decision_prefix_uses_real_target_validity_and_accepts_invalid_player_fallback() {
    for kind in [
        "missing",
        "valid",
        "self",
        "dead-killed",
        "dead-live",
        "no-combat",
    ] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["death_selector"] = 6.into();
            saved["actors"][0]["attributes"][121] = 1.into();
            saved["actors"][0]["control"]["combat"]["retained_target_position"] =
                serde_json::json!([7, 8, 9]);
            saved["actors"][1]["control"]["combat"] =
                serde_json::to_value(cyrus_game::script::CombatControlState::default()).unwrap();
            if kind == "missing" {
                saved["actors"][0]["control"]["combat"]["target"] = serde_json::Value::Null;
            }
            if kind == "self" {
                saved["actors"][1]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            if kind.starts_with("dead") {
                saved["actors"][1]["control"]["death_selector"] = 8.into();
                saved["actors"][1]["control"]["killed"] = (kind == "dead-killed").into();
            }
            if kind == "no-combat" {
                saved["actors"][1]["control"]["combat"] = serde_json::Value::Null;
            }
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([111, 222, 333]);
            saved["spatial"]["actors"][4]["local"]["position"] =
                serde_json::json!([-444, 555, -666]);
        });
        let own = world.id(0).unwrap();
        let player = world.id(4).unwrap();
        let sample = world
            .prepare_ordinary_combat_decision(own, player, None)
            .unwrap()
            .unwrap();
        let valid = matches!(kind, "valid" | "dead-live");
        assert_eq!(
            sample.target,
            world.id(if valid { 1 } else { 4 }).unwrap(),
            "{kind}"
        );
        assert!(sample.engaged);
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            saved["actors"][0]["control"]["combat"]["retained_target_position"],
            if valid {
                serde_json::json!([111, 222, 333])
            } else {
                serde_json::json!([-444, 555, -666])
            }
        );
        assert_eq!(
            saved["actors"][0]["control"]["combat"]["close_distance_passes"],
            1
        );
        assert_eq!(saved["map_updates"], serde_json::json!([]));
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    }
}

#[test]
fn combat_decision_prefix_samples_3d_query_distance_and_wraps_counter_before_engagement() {
    for (query_position, initial, expected) in [
        ([0, 24999, 0], 10, 11),
        ([15000, 20000, 0], 10, 0),
        ([0, 25001, 0], 10, 0),
        ([0, 1, 0], i32::MAX, i32::MIN),
    ] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            saved["actors"][0]["control"]["death_selector"] = 6.into();
            saved["actors"][0]["control"]["combat"]["close_distance_passes"] = initial.into();
            saved["actors"][0]["control"]["combat"]["retained_target_position"] =
                serde_json::json!([7, 8, 9]);
            saved["actors"][1]["control"]["combat"] =
                serde_json::to_value(cyrus_game::script::CombatControlState::default()).unwrap();
            saved["spatial"]["actors"][1]["linked_position"] = serde_json::json!(query_position);
        });
        let error = world
            .prepare_ordinary_combat_decision(world.id(0).unwrap(), world.id(4).unwrap(), None)
            .unwrap_err();
        assert!(format!("{error:#}").contains("combat decision sight geometry unavailable"));
        let saved = movement_snapshot(&world, &scripts, &data);
        assert_eq!(
            saved["actors"][0]["control"]["combat"]["close_distance_passes"],
            expected
        );
        assert_eq!(
            saved["actors"][0]["control"]["combat"]["retained_target_position"],
            serde_json::json!([7, 8, 9])
        );
    }
}

#[test]
fn combat_decision_prefix_retains_fallback_local_copy_when_sight_is_unavailable() {
    let (mut world, scripts, data) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["death_selector"] = 6.into();
        saved["actors"][0]["control"]["combat"]["target"] = serde_json::Value::Null;
        saved["spatial"]["actors"][4]["local"]["position"] = serde_json::json!([111, 222, 333]);
        saved["spatial"]["actors"][4]["linked_position"] = serde_json::json!([0, 25000, 0]);
    });
    let error = world
        .prepare_ordinary_combat_decision(world.id(0).unwrap(), world.id(4).unwrap(), None)
        .unwrap_err();
    assert!(format!("{error:#}").contains("world 7 instance 0 combat decision prefix"));
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][0]["control"]["combat"]["retained_target_position"],
        serde_json::json!([111, 222, 333])
    );
    assert_eq!(
        saved["actors"][0]["control"]["combat"]["close_distance_passes"],
        0
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .target,
        Some(world.id(4).unwrap())
    );
}

#[test]
fn combat_decision_prefix_rebinds_target_and_continues_counter_with_live_sight() {
    use cyrus_game::script::{OrdinaryObstacleQuery, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = 1;
        instance.map_position = [0; 3];
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let (world, data, models) = static_model_fixture_at(scripts.clone(), None, [0; 3]);
    let scene = ordinary_collision_scene(world.world, models);
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &scene.collision,
    };
    let mut snapshot = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for index in [0, 1] {
        snapshot["actors"][index]["control"]["combat"] =
            serde_json::to_value(cyrus_game::script::CombatControlState::default()).unwrap();
    }
    snapshot["actors"][0]["control"]["death_selector"] = 6.into();
    snapshot["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([1000, 0, 0]);
    for generation in [2, 3] {
        let mut world = ScriptWorld::from_snapshot(
            serde_json::from_value(snapshot).unwrap(),
            &sources,
            WorldRestoreContext {
                generation,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let ids = [world.id(0).unwrap(), world.id(1).unwrap()];
        let prepared = world
            .prepare_ordinary_collision_query(ids[0], &scene, &data, [0, 0], &ids)
            .unwrap();
        let sample = world
            .prepare_ordinary_combat_decision(
                ids[0],
                ids[1],
                Some(OrdinaryObstacleQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    vertical_offset: 0,
                    respond: false,
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            sample,
            cyrus_game::script::OrdinaryCombatSample {
                target: ids[1],
                distance: 1000,
                engaged: true
            }
        );
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(combat.retained_target_position, [1000, 0, 0]);
        assert_eq!(combat.close_distance_passes, generation as i32 - 1);
        snapshot = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    }
}

#[test]
fn live_attack_executor_uses_four_slots_and_orders_animation_before_combat_writes() {
    use cyrus_game::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    scripts.definition.attributes = (0..256).map(|index| format!("attr_{index}")).collect();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .extend([("attr_14".into(), 1), ("attr_79".into(), 1)]);
    for instance in &mut scripts.instances {
        instance.object_type = 1;
    }
    scripts.combat_defenses = vec![
        combat::Defense {
            animation_group: 199,
            min_range: None,
            max_range: 1,
            first_collide_frame: None,
            collide_duration: None,
            arc: None,
        },
        combat::Defense {
            animation_group: 100,
            min_range: None,
            max_range: 1,
            first_collide_frame: None,
            collide_duration: None,
            arc: None,
        },
    ];
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "mpob_objects":[{"script_name":"LOGIC"},{"script_name":"LOGIC"}],
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "raan_entries":[{"frame_count":1,"file_path":"TEST.3D"}],
                "animation_groups":[
                    {"group_index":0,"anim_id":1,"anim_type":0,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":100,"anim_id":1,"anim_type":0,"commands":[
                        {"opcode":4,"sound_param":-17,"volume_shift":0},
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]}),
        sections: Default::default(),
        models: BTreeMap::from([(
            "TEST".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "static_3d".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        )]),
        unsupported: Default::default(),
    };
    let collision = cyrus_assets::collision::CollisionScene::default();
    let sources = WorldSnapshotSources {
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &collision,
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    // Idle cleanup needs neither move metadata nor timing.
    assert!(
        !world
            .execute_combat_attack(id, -1, Default::default(), &mut |_, _| panic!("idle sound"))
            .unwrap()
    );
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    saved["actors"][0]["control"]["combat"]["retained_action_indices"] = serde_json::json!([0, -1]);
    saved["actors"][0]["control"]["combat"]["action"] = 9.into();
    for missing in ["slot", "record", "animation"] {
        let mut snapshot = saved.clone();
        if missing == "slot" {
            snapshot["actors"][0]["control"]["combat"]["retained_action_indices"][0] = 1.into();
        } else if missing == "record" {
            snapshot["actors"][0]["control"]["combat"]["defense_indices"][0] = 2.into();
        } else {
            snapshot["actors"][0]["animation"] = serde_json::Value::Null;
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
        let id = world.id(0).unwrap();
        let error = world
            .execute_combat_attack(
                id,
                0,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                &mut |_, _| panic!("missing metadata dispatched sound"),
            )
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(
            error.contains("world 7 instance 0 attack executor"),
            "{error}"
        );
        assert!(
            error.contains(match missing {
                "slot" => "defense slot 1: unpopulated",
                "record" => "defense slot 0 record 2: unavailable",
                _ => "receiver animation unavailable",
            }),
            "{error}"
        );
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (combat.decision.attack_state, combat.decision.aggression),
            (0, 50)
        );
        if missing == "animation" {
            assert!(
                !world
                    .execute_combat_attack(id, -1, Default::default(), &mut |_, _| panic!(
                        "cleanup sound"
                    ))
                    .unwrap()
            );
        }
    }
    for (state, current, ticks, fail, expected_state, expected_budget, expected_pending) in [
        (0, 0, Some(1), false, 2, 40, 0),
        (0, 0, Some(1), true, 0, 50, 0),
        (0, 0, None, false, 0, 50, 0),
        (0, 100, None, false, 0, 50, 0),
        (2, 100, None, false, 2, 50, 0),
        (2, 0, None, false, 0, 50, -1),
    ] {
        let mut snapshot = saved.clone();
        snapshot["actors"][0]["control"]["combat"]["decision"]["attack_state"] = state.into();
        // Group definitions have identical sequence structure except for the already consumed sound.
        snapshot["actors"][0]["animation"]["playback"]["current"][0] = current.into();
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
        let before = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        let id = world.id(0).unwrap();
        let mut sounds = 0;
        let result = world.execute_combat_attack(
            id,
            0,
            ScriptInputs {
                nominal_ticks: ticks,
                ..Default::default()
            },
            &mut |owner, _| {
                assert_eq!(owner, id);
                sounds += 1;
                anyhow::ensure!(!fail, "injected attack sound failure");
                Ok(())
            },
        );
        if fail || ticks.is_none() && state == 0 && current == 0 {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains(if fail {
                    "injected attack sound failure"
                } else {
                    "nominal ticks unavailable"
                }),
                "{error}"
            );
        } else {
            assert_eq!(
                result.unwrap(),
                expected_state != 0 && expected_pending != -1
            );
        }
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                combat.decision.attack_state,
                combat.decision.aggression,
                combat.retained_action_indices[0]
            ),
            (expected_state, expected_budget, expected_pending)
        );
        assert_eq!(
            combat.action,
            if state == 2 && current == 0 { -1 } else { 9 }
        );
        assert_eq!(
            sounds,
            usize::from(state == 0 && current == 0 && ticks.is_some())
        );
        let after = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        assert_eq!(after["actors"][0]["tasks"], before["actors"][0]["tasks"]);
        if expected_state == 2 && state == 0 {
            assert_eq!(
                after["actors"][0]["animation"]["playback"]["current"][0],
                100
            );
            assert_eq!(
                after["actors"][0]["animation"]["playback"]["current"][1]["loops"],
                1
            );
            assert!(after["actors"][0]["animation"]["playback"]["pending"].is_null());
        }
    }
}

#[test]
fn combat_coordination_reuses_attacker_state_and_preserves_the_entry_branch() {
    use cyrus_game::script::{OrdinaryCombatBranch, OrdinaryCombatCoordinationInput};
    for (
        engaged,
        state,
        budget,
        close,
        candidate,
        distance,
        expected_branch,
        expected_state,
        expected_counter,
        message,
    ) in [
        (
            true,
            0,
            50,
            11,
            false,
            20480,
            OrdinaryCombatBranch::Waiting,
            1,
            0,
            None,
        ),
        (
            true,
            0,
            50,
            10,
            true,
            20480,
            OrdinaryCombatBranch::Waiting,
            0,
            10,
            None,
        ),
        (
            true,
            0,
            50,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Waiting,
            0,
            11,
            Some(0xd003),
        ),
        (
            true,
            1,
            0,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Active,
            1,
            11,
            None,
        ),
        (
            true,
            1,
            -1,
            11,
            true,
            29999,
            OrdinaryCombatBranch::Active,
            0,
            0,
            Some(0xd002),
        ),
        (
            true,
            1,
            -1,
            11,
            true,
            30000,
            OrdinaryCombatBranch::Active,
            1,
            11,
            None,
        ),
        (
            false,
            1,
            50,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Disengaged,
            0,
            0,
            Some(0xd002),
        ),
        (
            false,
            0,
            50,
            11,
            true,
            20480,
            OrdinaryCombatBranch::Disengaged,
            0,
            11,
            None,
        ),
    ] {
        let (mut world, scripts, data) = spacing_fixture(|saved| {
            let own = &mut saved["actors"][0]["control"]["combat"];
            own["attacker_selection_state"] = state.into();
            own["close_distance_passes"] = close.into();
            own["decision"]["aggression"] = budget.into();
            saved["actors"][2]["control"]["combat"] = serde_json::Value::Null;
            let other = &mut saved["actors"][3]["control"]["combat"];
            other["attacker_selection_state"] = i32::from(candidate).into();
            other["spacing_ranges"] = serde_json::json!([1, 10000, 5000]);
            if !candidate {
                other["target"] = serde_json::Value::Null;
            }
            saved["actors"][3]["attributes"][110] = 1.into();
            saved["spatial"]["actors"][3]["local"]["position"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["position"] =
                serde_json::json!([0, distance, 0]);
        });
        world.ordinary_actors([0, 0]).unwrap();
        let id = world.id(0).unwrap();
        let other = world.id(3).unwrap();
        let player = world.id(1).unwrap();
        let before = movement_snapshot(&world, &scripts, &data);
        let mut random = cyrus_game::random::EngineRandom::default();
        let branch = world
            .coordinate_ordinary_combat(
                id,
                OrdinaryCombatCoordinationInput {
                    engaged,
                    selected_player: player,
                    frame_value: 0,
                },
                Default::default(),
                &mut random,
                &mut |_, _| panic!("empty taunt list dispatched sound"),
            )
            .unwrap();
        assert_eq!(branch, expected_branch);
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                combat.attacker_selection_state,
                combat.close_distance_passes
            ),
            (expected_state, expected_counter)
        );
        assert_eq!(combat.decision.aggression, budget);
        let received = world.consume_combat_message(other).unwrap();
        assert_eq!(
            received.map(|m| (m.sender, m.receiver, m.code)),
            message.map(|code| (id, Some(other), code))
        );
        assert!(world.consume_combat_message(other).unwrap().is_none());
        assert_eq!(random.state(), 1);
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(after["actors"][0]["tasks"], before["actors"][0]["tasks"]);
        assert_eq!(after["spatial"], before["spatial"]);
    }
}

#[test]
fn combat_coordinator_queries_preserve_head_order_cycling_and_distinct_filters() {
    let (mut world, _, _) = spacing_fixture(|saved| {
        // Legacy duplicate fields cannot override the live coordination owner on restore.
        saved["actors"][3]["control"]["combat"]["decision"]["near_state"] = 0.into();
        saved["actors"][3]["control"]["combat"]["decision"]["far_ticks"] = 999.into();
        // Waiting lookup admits a different nonnull target and does not query weapon state.
        saved["actors"][3]["control"]["combat"]["target"] = serde_json::json!([1, 4]);
        saved["actors"][3]["attributes"][110] = 1.into();
        saved["actors"][2]["attributes"][110] = 1.into();
        saved["actors"][0]["attributes"][110] = 1.into();
    });
    let own = world.id(0).unwrap();
    let player = world.id(1).unwrap();
    assert!(
        format!(
            "{:#}",
            world.ordinary_waiting_coordinator(player).unwrap_err()
        )
        .contains("traversal unavailable")
    );
    world.ordinary_actors([0, 0]).unwrap();
    assert_eq!(
        world.ordinary_waiting_coordinator(player).unwrap(),
        Some(world.id(3).unwrap())
    );
    assert_eq!(
        world
            .ordinary_active_coordinator(own, player, Default::default())
            .unwrap(),
        Some(world.id(2).unwrap())
    );
    assert_eq!(
        world
            .ordinary_active_coordinator(world.id(2).unwrap(), player, Default::default())
            .unwrap(),
        Some(own)
    );
    world
        .set_combat_coordination(world.id(3).unwrap(), 0)
        .unwrap();
    assert_eq!(
        world.ordinary_waiting_coordinator(player).unwrap(),
        Some(world.id(2).unwrap())
    );
    world
        .set_combat_coordination(world.id(2).unwrap(), 0)
        .unwrap();
    world.set_combat_coordination(own, 0).unwrap();
    assert_eq!(world.ordinary_waiting_coordinator(player).unwrap(), None);
    // Active lookup does not filter candidates by coordination state.
    assert_eq!(
        world
            .ordinary_active_coordinator(own, player, Default::default())
            .unwrap(),
        Some(world.id(2).unwrap())
    );
}

#[test]
fn coordination_keeps_queued_message_and_prior_state_when_taunt_service_fails() {
    use cyrus_game::script::OrdinaryCombatCoordinationInput;
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let mut third = scripts.instances[0].clone();
            third.object_id = 2;
            scripts.instances.push(third);
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 3;
            actor.instance_variables = vec![vec![4]; 3];
            actor.attributes.insert("attr_num_taunts".into(), 1);
            actor.combat_taunts = vec![CombatTaunt {
                trigger: 9,
                label: *b"TEST",
                animation: 0,
            }];
        },
        |saved| {
            for index in [0, 2] {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_game::script::CombatControlState {
                        flags: 4,
                        attacker_selection_state: 1,
                        close_distance_passes: 11,
                        taunt_period: 1,
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            saved["actors"][0]["control"]["combat"]["decision"]["aggression"] = (-1).into();
            saved["actors"][2]["attributes"][110] = 1.into();
        },
    );
    world.ordinary_actors([0, 0]).unwrap();
    let id = world.id(0).unwrap();
    let player = world.id(1).unwrap();
    let other = world.id(2).unwrap();
    let mut random = cyrus_game::random::EngineRandom::default();
    let error = world
        .coordinate_ordinary_combat(
            id,
            OrdinaryCombatCoordinationInput {
                engaged: true,
                selected_player: player,
                frame_value: 0,
            },
            Default::default(),
            &mut random,
            &mut |_, _| panic!("missing timing dispatched sound"),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("nominal ticks unavailable"),
        "{error:#}"
    );
    assert_eq!(
        world.consume_combat_message(other).unwrap().unwrap().code,
        0xd002
    );
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            combat.attacker_selection_state,
            combat.close_distance_passes
        ),
        (1, 11)
    );
    let mut expected = cyrus_game::random::EngineRandom::default();
    for _ in 0..15 {
        expected.draw();
    }
    assert_eq!(random.state(), expected.state());
}

fn counter_fixture(edit: impl FnOnce(&mut serde_json::Value)) -> ScriptWorld {
    counter_fixture_sound(false, edit)
}

fn counter_fixture_sound(sound: bool, edit: impl FnOnce(&mut serde_json::Value)) -> ScriptWorld {
    let (mut world, scripts, mut data) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let motion: combat::Motion = serde_json::from_value(serde_json::json!({
                "section":1,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                "first_collide_frame":2,"collide_duration":1,"first_defend_frame":5,"defense":1
            }))
            .unwrap();
            scripts.combat_attacks = vec![combat::Attack {
                is_combo: false,
                motions: vec![motion],
                min_range: 0,
                max_range: 100,
            }];
            scripts.combat_defenses = vec![combat::Defense {
                animation_group: 19,
                min_range: None,
                max_range: 1,
                first_collide_frame: Some(1),
                collide_duration: Some(10),
                arc: None,
            }];
        },
        |saved| {
            for index in 0..2 {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_game::script::CombatControlState {
                        action: if index == 0 { -1 } else { 2 },
                        flags: if index == 0 { 0xa1 } else { 0 },
                        attack_indices: [Some(0); 9],
                        defense_indices: [Some(0); 4],
                        retained_action_indices: [-1, 0],
                        decision: cyrus_game::script::CombatDecision {
                            attack_pacing: 3,
                            ..Default::default()
                        },
                        spacing_ranges: [0, 100, 50],
                        ..Default::default()
                    })
                    .unwrap();
            }
            saved["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            saved["actors"][0]["attributes"][110] = 1.into();
            saved["actors"][1]["animation"] = saved["actors"][0]["animation"].clone();
            saved["spatial"]["actors"][0]["local"]["angles"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["angles"] = serde_json::json!([0, 1024, 0]);
            edit(saved);
        },
    );
    if sound {
        let mut saved = movement_snapshot(&world, &scripts, &data);
        for actor in saved["actors"].as_array_mut().unwrap() {
            actor["animation"] = serde_json::Value::Null;
        }
        let groups = data.metadata["actors"][0]["animation_groups"]
            .as_array_mut()
            .unwrap();
        let group = groups
            .iter_mut()
            .find(|group| group["group_index"] == 19)
            .unwrap();
        group["commands"].as_array_mut().unwrap().insert(
            0,
            serde_json::json!({"opcode":4,"sound_param":17,"volume_shift":0}),
        );
        let mut fresh = ScriptWorld::prepare(&scripts, 1).unwrap();
        fresh.prepare_spatial().unwrap();
        let mut candidate = movement_snapshot(&fresh, &scripts, &data);
        candidate["generation"] = saved["generation"].clone();
        candidate["actors"] = saved["actors"].clone();
        candidate["spatial"] = saved["spatial"].clone();
        let collision = Default::default();
        world = ScriptWorld::from_snapshot(
            serde_json::from_value(candidate).unwrap(),
            &cyrus_game::script::WorldSnapshotSources {
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &collision,
            },
            cyrus_game::script::WorldRestoreContext {
                generation: 3,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        for index in 0..2 {
            let id = world.id(index).unwrap();
            world.bind_animation(id, &data, 18).unwrap();
            world
                .advance(
                    id,
                    cyrus_game::script::ScriptStage::Animation,
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(0),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    20,
                )
                .unwrap();
        }
    }
    world
}

#[test]
fn live_counter_starts_receiver_animation_then_marks_target_without_a_task() {
    use cyrus_game::script::decision::CounterAttack;
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let mut world = counter_fixture(|_| {});
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let mut random = cyrus_game::random::EngineRandom::default();
    assert_eq!(
        world
            .counter_ordinary_combat(
                id,
                OrdinarySpacingInput {
                    distance: 50,
                    tick_counter: 0
                },
                ScriptInputs {
                    hud: Some(&hud),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| panic!("synthetic group has no sound")
            )
            .unwrap(),
        CounterAttack::Attack { index: 0 }
    );
    let own = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    let target = world.actors()[1]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            own.flags,
            own.action,
            own.retained_action_indices[0],
            own.decision.attack_state,
            own.decision.aggression
        ),
        (0xa3, 9, 0, 2, 40)
    );
    assert_eq!(target.flags, 0x10);
    assert_eq!(
        world.inspect(0, 2)["actors"][0]["animation"]["current_group"],
        19
    );
    assert_eq!(random.state(), 1);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn live_counter_preserves_intent_pending_and_action_before_executor_failure() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let mut world = counter_fixture(|_| {});
    let id = world.id(0).unwrap();
    let hud = ScriptHud::default();
    let error = world
        .counter_ordinary_combat(
            id,
            OrdinarySpacingInput {
                distance: 50,
                tick_counter: 0,
            },
            ScriptInputs {
                hud: Some(&hud),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| panic!("missing timing reached sound"),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("nominal ticks unavailable"),
        "{error:#}"
    );
    let own = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            own.flags,
            own.retained_action_indices[0],
            own.action,
            own.decision.attack_state,
            own.decision.aggression
        ),
        (0xa3, 0, 9, 0, 50)
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .flags,
        0
    );
}

#[test]
fn live_counter_admission_bypasses_unavailable_timing_and_keeps_rng() {
    use cyrus_game::script::decision::CounterAttack;
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    let hud = ScriptHud::default();
    for gate in [
        "weapon",
        "window",
        "facing",
        "target_action",
        "own_action",
        "slots",
        "mark",
    ] {
        let mut world = counter_fixture(|saved| {
            saved["actors"][1]["animation"] = serde_json::Value::Null;
            match gate {
                "weapon" => {
                    saved["actors"][0]["attributes"][110] = 0.into();
                    saved["actors"][0]["control"]["combat"] = serde_json::Value::Null;
                }
                "window" => {
                    saved["actors"][1]["control"]["combat"]["spacing_ranges"] =
                        serde_json::json!([50, 100, 75])
                }
                "facing" => {
                    saved["spatial"]["actors"][1]["local"]["angles"] =
                        serde_json::json!([0, 768, 0])
                }
                "target_action" => saved["actors"][1]["control"]["combat"]["action"] = 9.into(),
                "own_action" => saved["actors"][0]["control"]["combat"]["action"] = 9.into(),
                "slots" => {
                    saved["actors"][0]["control"]["combat"]["defense_indices"] =
                        serde_json::json!([null, null, null, null])
                }
                "mark" => saved["actors"][1]["control"]["combat"]["flags"] = 16.into(),
                _ => unreachable!(),
            }
        });
        let id = world.id(0).unwrap();
        let mut random = cyrus_game::random::EngineRandom::default();
        assert_eq!(
            world
                .counter_ordinary_combat(
                    id,
                    OrdinarySpacingInput {
                        distance: 50,
                        tick_counter: 0
                    },
                    ScriptInputs {
                        hud: Some(&hud),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |_, _| panic!("guard reached sound")
                )
                .unwrap(),
            CounterAttack::Idle,
            "{gate}"
        );
        assert_eq!(random.state(), 1);
    }
}

#[test]
fn live_counter_uses_shared_rng_lazily_for_random_fit() {
    use cyrus_game::script::decision::CounterAttack;
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (seed, draws, outcome) in [
        (1, 1, CounterAttack::Idle),
        (0, 2, CounterAttack::Attack { index: 0 }),
    ] {
        let mut world = counter_fixture(|_| {});
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(seed);
        let mut expected = cyrus_game::random::EngineRandom::from_state(seed);
        for _ in 0..draws {
            expected.draw();
        }
        assert_eq!(
            world
                .counter_ordinary_combat(
                    id,
                    OrdinarySpacingInput {
                        distance: 50,
                        tick_counter: 2
                    },
                    ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(1),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            outcome
        );
        assert_eq!(random.state(), expected.state());
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags,
            if draws == 2 { 16 } else { 0 }
        );
    }
}

#[test]
fn live_counter_reads_command_cursor_and_runs_direct_sidestep_primitives() {
    use cyrus_game::script::decision::{CounterAttack, Sidestep};
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (slot, tick, step, position) in [
        (3, 0, Sidestep::Right, [1024, 0, 0]),
        (4, 1, Sidestep::Left, [-1024, 0, 0]),
        (0, 0, Sidestep::Back, [0, 0, -768]),
    ] {
        let mut world = counter_fixture(|saved| {
            // Cursor one differs from byte progress zero, making the near gap too short.
            saved["actors"][1]["animation"]["playback"]["current"][1]["command"] = 1.into();
            saved["actors"][0]["control"]["combat"]["flags"] =
                if slot == 0 { 0xa1 } else { 0xa5 }.into();
            saved["actors"][1]["control"]["combat"]["retained_action_indices"][1] = slot.into();
            if slot == 0 {
                saved["actors"][0]["animation"]["playback"]["current"][0] = 19.into();
            }
        });
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::default();
        assert_eq!(
            world
                .counter_ordinary_combat(
                    id,
                    OrdinarySpacingInput {
                        distance: 50,
                        tick_counter: tick
                    },
                    ScriptInputs {
                        hud: Some(&hud),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        shell_execution: Some(false),
                        blocked_ahead_30: Some(false),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |_, _| Ok(())
                )
                .unwrap(),
            CounterAttack::Sidestep(step)
        );
        assert_eq!(world.spatial(id).unwrap().local.position, position);
        assert_eq!(random.state(), 1);
        assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
        let own = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (own.flags, own.action, own.retained_action_indices[0]),
            (if slot == 0 { 0xa1 } else { 0xa5 }, -1, -1)
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags,
            0
        );
    }
}

#[test]
fn live_counter_sound_failure_retains_start_action_but_does_not_mark_target() {
    use cyrus_game::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs, ScriptSound};
    use cyrus_world::animation::AnimationSound;
    for fail in [false, true] {
        let mut world = counter_fixture_sound(true, |_| {});
        let id = world.id(0).unwrap();
        let hud = ScriptHud::default();
        let mut cues = 0;
        let result = world.counter_ordinary_combat(
            id,
            OrdinarySpacingInput {
                distance: 50,
                tick_counter: 0,
            },
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |actor, sound| {
                assert_eq!(actor, id);
                assert!(matches!(
                    sound,
                    ScriptSound::Animation {
                        sound: AnimationSound::Play {
                            effect: 17,
                            rate_offset_hz: 0
                        },
                        ..
                    }
                ));
                cues += 1;
                if fail {
                    anyhow::bail!("counter sound rejected");
                }
                Ok(())
            },
        );
        assert_eq!(cues, 1);
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("counter sound rejected"));
        } else {
            assert!(result.is_ok());
        }
        let own = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (own.flags, own.action, own.retained_action_indices[0]),
            (0xa3, 9, 0)
        );
        assert_eq!(
            (own.decision.attack_state, own.decision.aggression),
            if fail { (0, 50) } else { (2, 40) }
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags,
            if fail { 0 } else { 16 }
        );
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
            19
        );
    }
}

fn coordination_message_fixture(
    edit: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    movement_fixture(
        18,
        false,
        true,
        |scripts| {
            for index in 2..6 {
                let mut instance = scripts.instances[0].clone();
                instance.object_id = index;
                scripts.instances.push(instance);
            }
            let actor = scripts.actors.get_mut("LOGIC").unwrap();
            actor.instances = 6;
            actor.instance_variables = vec![vec![4]; 6];
            for instance in &mut scripts.instances {
                instance.active = 1;
            }
        },
        |saved| {
            for index in 0..6 {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_game::script::CombatControlState {
                        attacker_selection_state: 1,
                        close_distance_passes: 11,
                        decision: cyrus_game::script::CombatDecision {
                            aggression: 7,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][index]["control"]["combat"]["target"] = serde_json::json!([1, 5]);
            }
            saved["actors"][4]["control"]["death_selector"] = 8.into();
            edit(saved);
        },
    )
}

#[test]
fn coordination_messages_reset_only_living_exact_active_attackers_then_assign_budget() {
    for code in [0xd002, 0xd003] {
        let (mut world, scripts, data) = coordination_message_fixture(|saved| {
            saved["actors"][0]["control"]["combat"]["attacker_selection_state"] = 0.into();
            saved["actors"][1]["control"]["combat"]["attacker_selection_state"] = 2.into();
            saved["actors"][3]["control"]["combat"]["target"] = serde_json::json!([1, 0]);
        });
        world.ordinary_actors([0, 0]).unwrap();
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world.send_combat_message(sender, receiver, code).unwrap();
        let before = movement_snapshot(&world, &scripts, &data);
        world.process_combat_message(receiver, None).unwrap();
        let states: Vec<_> = world
            .actors()
            .iter()
            .map(|actor| {
                let c = actor.control_state().unwrap().combat.as_ref().unwrap();
                (
                    c.attacker_selection_state,
                    c.close_distance_passes,
                    c.decision.aggression,
                )
            })
            .collect();
        assert_eq!(
            states,
            if code == 0xd002 {
                vec![
                    (1, 0, 50),
                    (2, 11, 7),
                    (0, 0, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                ]
            } else {
                vec![
                    (0, 11, 7),
                    (1, 0, 50),
                    (0, 0, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                    (1, 11, 7),
                ]
            }
        );
        assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
        let after = movement_snapshot(&world, &scripts, &data);
        assert_eq!(before["spatial"], after["spatial"]);
        for index in 0..6 {
            assert_eq!(
                before["actors"][index]["tasks"],
                after["actors"][index]["tasks"]
            );
            assert_eq!(
                before["actors"][index]["control"]["combat"]["target"],
                after["actors"][index]["control"]["combat"]["target"]
            );
        }
    }
}

#[test]
fn coordination_messages_use_receiver_or_sender_target_and_skip_null_target_traversal() {
    for code in [0xd002, 0xd003] {
        let (mut world, _, _) = coordination_message_fixture(|saved| {
            saved["actors"][if code == 0xd002 { 0 } else { 1 }]["control"]["combat"]["target"] =
                serde_json::Value::Null;
        });
        let receiver = world.id(0).unwrap();
        let sender = world.id(1).unwrap();
        world.send_combat_message(sender, receiver, code).unwrap();
        // A null target must not require the unprepared ordinary list.
        world.process_combat_message(receiver, None).unwrap();
        for index in 0..6 {
            let c = world.actors()[index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap();
            assert_eq!(
                (
                    c.attacker_selection_state,
                    c.close_distance_passes,
                    c.decision.aggression
                ),
                if index == if code == 0xd002 { 0 } else { 1 } {
                    (1, 0, 50)
                } else {
                    (1, 11, 7)
                }
            );
        }
    }
    let (mut world, _, _) = coordination_message_fixture(|saved| {
        saved["actors"][1]["control"]["combat"]["target"] = serde_json::json!([1, 3]);
        saved["actors"][2]["control"]["combat"]["target"] = serde_json::json!([1, 3]);
    });
    world.ordinary_actors([0, 0]).unwrap();
    let receiver = world.id(0).unwrap();
    world
        .send_combat_message(world.id(1).unwrap(), receiver, 0xd003)
        .unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .attacker_selection_state,
        1
    );
    assert_eq!(
        world.actors()[2]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .attacker_selection_state,
        0
    );
}

#[test]
fn coordination_message_consumption_precedes_errors_and_processes_one_code_per_call() {
    let (mut world, _, _) = coordination_message_fixture(|_| {});
    let receiver = world.id(0).unwrap();
    let sender = world.id(1).unwrap();
    world.send_combat_message(sender, receiver, 0xd002).unwrap();
    let error = format!(
        "{:#}",
        world.process_combat_message(receiver, None).unwrap_err()
    );
    assert!(
        error.contains("world 7 instance 0 combat message"),
        "{error}"
    );
    assert!(error.contains("ordinary traversal unavailable"), "{error}");
    assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .decision
            .aggression,
        7
    );
    world.send_combat_message(sender, receiver, 0xd004).unwrap();
    world.send_combat_message(sender, receiver, 0xd003).unwrap();
    world.process_combat_message(receiver, None).unwrap();
    assert_eq!(
        world
            .consume_combat_message(receiver)
            .unwrap()
            .unwrap()
            .code,
        0xd003
    );
    for code in [0xd000, 0xd001, 0xd005] {
        world.send_combat_message(sender, receiver, code).unwrap();
        let error = format!(
            "{:#}",
            world.process_combat_message(receiver, None).unwrap_err()
        );
        assert!(error.contains("unsupported"), "{error}");
        assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
    }
}

#[test]
fn coordination_message_keeps_prior_resets_when_selected_death_state_is_unknown() {
    let (mut world, _, _) = coordination_message_fixture(|_| {});
    let order = world.ordinary_actors([0, 0]).unwrap();
    assert_eq!(
        order.iter().map(|id| id.index).collect::<Vec<_>>(),
        [5, 4, 3, 2, 1, 0]
    );
    let receiver = world.id(0).unwrap();
    let selected_player = world.id(2).unwrap();
    world
        .send_combat_message(world.id(1).unwrap(), receiver, 0xd002)
        .unwrap();
    let error = format!(
        "{:#}",
        world
            .process_combat_message(receiver, Some(selected_player))
            .unwrap_err()
    );
    assert!(
        error.contains("selected-player override unavailable"),
        "{error}"
    );
    let state = |index: usize| {
        let c = world.actors()[index]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        (
            c.attacker_selection_state,
            c.close_distance_passes,
            c.decision.aggression,
        )
    };
    assert_eq!(state(3), (0, 0, 7));
    assert_eq!(state(2), (1, 11, 7));
    assert_eq!(state(0), (1, 11, 7));
    assert_eq!(world.consume_combat_message(receiver).unwrap(), None);
}
