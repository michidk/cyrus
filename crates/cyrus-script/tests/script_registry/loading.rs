use super::*;

#[test]
fn automatic_world_updates_wait_for_effects_then_advance_and_pause_in_menu() {
    for browser_identity in [false, true] {
        automatic_world_updates_with_prepared_identity(browser_identity, None);
    }
}

#[test]
fn loading_player_models_does_not_advance_the_simulation_clock() {
    automatic_world_updates_with_prepared_identity(false, Some(PreparationDelay::Models));
}

#[test]
fn loading_host_services_does_not_advance_the_simulation_clock() {
    automatic_world_updates_with_prepared_identity(false, Some(PreparationDelay::Host));
}

#[derive(Clone, Copy)]
enum PreparationDelay {
    Models,
    Host,
}

fn automatic_world_updates_with_prepared_identity(
    browser_identity: bool,
    delay: Option<PreparationDelay>,
) {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    // Bevy's prelude adds a `Function` trait when reflect functions are enabled.
    use cyrus_assets::animation::Animations;
    use cyrus_game::AppState;
    use cyrus_script::script::{ScriptActors, ScriptPlugin, ScriptView};
    use cyrus_soup::program::Function;
    use cyrus_world::{
        animation::{AnimationRequest, AnimationTick},
        lifecycle::PreparedWorld,
    };
    let mut scripts = cache();
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.active = 1;
    }
    scripts.definition.functions.push(Function {
        name: Native::PlayerFacing,
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
                    function: Native::PlayerFacing,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![Value::Integer { value: 0 }, Value::Integer { value: 0 }],
                }),
            },
            rest: vec![],
        },
    };
    scripts.definition.functions.push(Function {
        name: Native::PlayerDistance,
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
                            function: Native::PlayerDistance,
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
        role.attributes[Attribute::Combat] = 1;
        role.attributes[Attribute::Weapon] = 1;
        scripts.actors.insert(name.into(), role);
    }
    scripts.weapons.insert(
        1,
        weapon::Weapon {
            hand_model: "LOGIC".into(),
            hilt_model: "LOGIC".into(),
            length: 17,
        },
    );
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
            positions: vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]; 14],
            normals: vec![vec![]; 14],
        },
    );
    data.metadata["markers"] = serde_json::json!([
        {"map_position":[10,20,30],"position":[0,0,0],"reserved":1}
    ]);
    for (name, effect) in [("CYRUS", 9), ("GREMLIN", 10)] {
        data.metadata["actors"].as_array_mut().unwrap().push(serde_json::json!({
            "script_name":name,"model_load_path":"animation_table",
            "raex":{"grip0":0,"grip1":1,"scabbard0":0,"scabbard1":1,"v_vertex":0,"v_size":0},
            "raan_entries":[{"file_path":"LOGIC.3D","frame_count":14}],
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
        text_style: [0; 2].into(),
        player_text_style: [0; 2].into(),
        used_text_style: [0; 2].into(),
        torch_time: None,
        inventory_screen: Default::default(),
        island_map: Default::default(),
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    let mut app = App::new();
    app.insert_resource(hud);
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_asset::<cyrus_script::player::data::PlayerScene>()
        .add_message::<AnimationTick>()
        .add_message::<cyrus_world::sound::SoundCue>()
        .init_resource::<cyrus_script::player::placement::MarkerEntry>()
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
        world: if browser_identity { None } else { Some(7) },
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    let scene = cyrus_script::player::data::PlayerScene::decode(
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
        .load::<cyrus_script::player::data::PlayerScene>("test.player");
    app.world_mut()
        .resource_mut::<Assets<cyrus_script::player::data::PlayerScene>>()
        .insert(scene_handle.id(), scene)
        .unwrap();
    app.insert_resource(cyrus_script::player::data::PlayerSceneRequest(scene_handle));
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: [256000, 0, 256000],
    });
    let player = app
        .world_mut()
        .spawn((
            cyrus_script::player::Player,
            Transform::default(),
            cyrus_script::player::placement::PlayerPose {
                position: [256, 512, 768],
                heading: 0,
            },
        ))
        .id();
    app.world_mut()
        .spawn(cyrus_script::player::placement::PlayerPose {
            position: [999, 999, 999],
            heading: 1024,
        });
    if let Some(delay) = delay {
        let request = app
            .world()
            .resource::<cyrus_script::player::data::PlayerSceneRequest>()
            .0
            .clone();
        let scene = match delay {
            PreparationDelay::Models => Some(
                app.world_mut()
                    .resource_mut::<Assets<cyrus_script::player::data::PlayerScene>>()
                    .remove(request.id())
                    .unwrap(),
            ),
            PreparationDelay::Host => {
                app.insert_resource(cyrus_script::script::OrdinarySoundExecutor(|_, _| {}));
                None
            }
        };
        let _environment = bind_scheduler_environment(&mut app);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(40),
        ));
        for _ in 0..20 {
            app.update();
            assert_eq!(
                app.world()
                    .resource::<cyrus_script::clock::SimulationClock>()
                    .elapsed_frame_scale(),
                0,
                "loading models must not consume simulated time before the first ordinary update"
            );
            assert_eq!(
                app.world()
                    .resource::<ScriptActors>()
                    .current
                    .as_ref()
                    .unwrap()
                    .ordinary_updates(),
                0
            );
        }
        if let Some(scene) = scene {
            app.world_mut()
                .resource_mut::<Assets<cyrus_script::player::data::PlayerScene>>()
                .insert(request.id(), scene)
                .unwrap();
        }
        app.world_mut()
            .remove_resource::<cyrus_script::script::OrdinarySoundExecutor>();
        app.update();
        assert_eq!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .ordinary_updates(),
            1
        );
        assert_eq!(
            app.world()
                .resource::<cyrus_script::clock::SimulationClock>()
                .elapsed_frame_scale(),
            0
        );
        app.update();
        assert!(
            app.world()
                .resource::<cyrus_script::clock::SimulationClock>()
                .elapsed_frame_scale()
                > 0
        );
        return;
    }
    app.update();
    let snapshot = |app: &App| {
        assert!(
            !matches!(
                app.world().resource::<cyrus_script::script::ScriptStatus>(),
                cyrus_script::script::ScriptStatus::Failed { .. }
            ),
            "{:?}",
            app.world().resource::<cyrus_script::script::ScriptStatus>()
        );

        let actors = app.world().resource::<ScriptActors>();
        actors
            .current
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "script preparation failed: {:?}",
                    app.world().resource::<cyrus_script::script::ScriptStatus>()
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
    let pending = snapshot(&app);
    assert_eq!(pending["actors"][0]["animation"], serde_json::Value::Null);
    let pending_clock = serde_json::to_value(
        app.world()
            .resource::<cyrus_script::clock::SimulationClock>()
            .snapshot(),
    )
    .unwrap();
    let pending_idle = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .globals()
        .player_idle_counter();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(60),
    ));
    for _ in 0..3 {
        app.update();
        assert_eq!(
            snapshot(&app),
            pending,
            "pending effects must not run model or script work"
        );
        assert_eq!(
            serde_json::to_value(
                app.world()
                    .resource::<cyrus_script::clock::SimulationClock>()
                    .snapshot()
            )
            .unwrap(),
            pending_clock
        );
        assert_eq!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .globals()
                .player_idle_counter(),
            pending_idle
        );
        assert!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .environmental_effects()
                .is_none()
        );
    }
    let _environment = bind_scheduler_environment(&mut app);
    let source = app.world().resource::<AnimationRequest>().0.clone();
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&source)
        .unwrap()
        .world = 8;
    app.update();
    assert_eq!(
        snapshot(&app),
        pending,
        "mismatched loaded animation metadata must not admit work"
    );
    assert_eq!(
        serde_json::to_value(
            app.world()
                .resource::<cyrus_script::clock::SimulationClock>()
                .snapshot()
        )
        .unwrap(),
        pending_clock
    );
    assert!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .environmental_effects()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<Assets<Animations>>()
        .get_mut(&source)
        .unwrap()
        .world = 7;
    app.update();
    assert!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .environmental_effects()
            .is_some()
    );
    assert_eq!(
        app.world()
            .resource::<cyrus_script::clock::SimulationClock>()
            .step()
            .frame_scale,
        0,
        "first admitted frame must exclude resource preparation time"
    );
    app.insert_resource(bevy::time::TimeUpdateStrategy::Automatic);
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
    assert!(initial_roles["actors"][0]["weapon"].is_object());
    assert_eq!(initial_roles["actors"][1]["attributes"][23], 1);
    assert_eq!(
        initial_roles["actors"][0]["animation"]["frame"], 1,
        "{initial_roles}"
    );
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
    let alternate = world.actors()[3].control_state().unwrap();
    assert!(alternate.hidden);
    assert!(alternate.object_update_excluded);
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
                surface: Some(Surface::UNKNOWN),
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
        .get_mut::<cyrus_script::player::placement::PlayerPose>(player)
        .unwrap()
        .heading = 1024;
    assert_eq!(first["actors"][0]["failure"], serde_json::Value::Null);
    assert_eq!(first["actors"][0]["locals"], serde_json::json!([5]));
    assert!(first["actors"][0]["animation"]["frame"].is_number());
    hud_asset.inventory.get_mut(&1).unwrap().count = 0;
    let unowned_hud = cyrus_script::script::ScriptHud::default();
    unowned_hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    app.insert_resource(unowned_hud);
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let globals = actors.current.as_mut().unwrap().globals_mut();
        globals
            .prepare_player_idle(
                false,
                Some(AnimationGroup::WALK),
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
    assert_eq!(
        app.world()
            .resource::<cyrus_script::clock::SimulationClock>()
            .step()
            .frame_scale,
        512,
        "a one-second host stall must use the installed minimum-FPS bound without smoothing"
    );
    let logical_world = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap();
    assert!(
        !matches!(
            app.world().resource::<cyrus_script::script::ScriptStatus>(),
            cyrus_script::script::ScriptStatus::Failed { .. }
        ),
        "{:?}",
        app.world().resource::<cyrus_script::script::ScriptStatus>()
    );
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
            .get::<cyrus_script::player::placement::PlayerPose>(player)
            .unwrap()
            .heading,
        0,
        "presentation must recover the authoritative logical heading"
    );
    assert!(
        role_snapshot(&app)["actors"][0]["weapon"].is_null(),
        "losing weapon ownership must release its displayed attachment"
    );
    initial_roles["actors"][0]["attributes"][23] = serde_json::json!(0);
    initial_roles["actors"][0]["weapon"] = serde_json::Value::Null;
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
        .resource::<cyrus_script::clock::SimulationClock>()
        .step();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(60),
    ));
    for _ in 0..3 {
        app.update();
        assert_eq!(snapshot(&app), before_console);
        assert_eq!(
            app.world()
                .resource::<cyrus_script::clock::SimulationClock>()
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
            .resource::<cyrus_script::clock::SimulationClock>()
            .step()
            .frame_scale,
        0,
        "resume must exclude the paused host interval"
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
            .resource::<cyrus_script::clock::SimulationClock>()
            .step()
            .frame_scale,
        0,
        "replacement world must not inherit the retiring world delta"
    );
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
            .query::<&cyrus_script::script::ScriptBinding>()
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
            {
                let mut state = cyrus_assets::cheats::empty();
                // SYSTEM.INI seeds these slots at startup in every host.
                cyrus_assets::cheats::CheatDefaults {
                    task: true,
                    animation: true,
                    nodemarker: false,
                }
                .apply(&mut state)?;
                Ok(state)
            }
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
    // drno through the same resource: while it is on, the map actors' main stage does not run,
    // so their first local stops advancing; off again, it advances by one per update. The
    // selected player does not run in this app (no controller sample), so its exemption is not
    // covered here.
    let map_actor_locals = |app: &App| {
        let world = app
            .world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap();
        [0, 1].map(|index| {
            world.actors()[index].runtime().unwrap().variables()[0]
                .integer()
                .unwrap()
        })
    };
    // A manual one-second step guarantees an admitted simulation step for every update.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(1),
    ));
    for (value, advances) in [(0, true), (1, false), (0, true)] {
        app.world_mut()
            .resource_mut::<cyrus_game::cheats::Cheats>()
            .execute("drno", Some(value))
            .unwrap();
        let before = map_actor_locals(&app);
        app.update();
        assert!(
            !matches!(
                app.world().resource::<cyrus_script::script::ScriptStatus>(),
                cyrus_script::script::ScriptStatus::Failed { .. }
            ),
            "{:?}",
            app.world().resource::<cyrus_script::script::ScriptStatus>()
        );
        let expected = if advances {
            before.map(|local| local + 1)
        } else {
            before
        };
        assert_eq!(map_actor_locals(&app), expected, "drno {value}");
    }
    let response = |app: &App| {
        let actors = app.world().resource::<ScriptActors>();
        let world = actors.current.as_ref().unwrap();
        let actor = &world.actors()[0];
        assert!(actor.failure().is_none(), "{:?}", actor.failure());
        // The overlap writer can also mark another actor, so both map actors must be non-colliding.
        for index in [0, 1] {
            assert_eq!(
                world.actors()[index].attributes()[Attribute::Collide],
                0,
                "map actor {index}"
            );
        }
        let control = actor.control_state().unwrap();
        assert_eq!(control.falling_state, JumpMotion::NONE);
        assert_eq!(control.queued_motion, JumpMotion::NONE);
        assert_eq!(
            control.last_collide.bits() & !CollisionFlags::ROLLBACK.bits(),
            0
        );
        assert_eq!(control.overlap_contact_identifier, 0);
        (
            control.last_collide_count,
            control.last_collide.bits() >> 16,
            world.ordinary_updates(),
        )
    };
    for enabled in [0, 1, 0] {
        app.world_mut()
            .resource_mut::<cyrus_game::cheats::Cheats>()
            .execute("drno", Some(enabled))
            .unwrap();
        {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            let id = world.id(0).unwrap();
            world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
            assert!(world.retain_ordinary_water_position(id).unwrap());
            let rollback = world
                .capture_ordinary_rollback(id, &mut |_, _| Ok(()))
                .unwrap();
            world.set_actor_surface(id, Surface::WATER).unwrap();
            assert!(
                world
                    .rollback_ordinary_water(id, None, &rollback, &mut |_, _| Ok(()))
                    .unwrap()
            );
            world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
        }
        let before = response(&app);
        assert_eq!(before.1, 1);
        assert!(before.0 < 200);
        app.update();
        assert!(
            !matches!(
                app.world().resource::<cyrus_script::script::ScriptStatus>(),
                cyrus_script::script::ScriptStatus::Failed { .. }
            ),
            "{:?}",
            app.world().resource::<cyrus_script::script::ScriptStatus>()
        );
        let after = response(&app);
        assert_eq!(after.2, before.2 + 1);
        let ticks = app
            .world()
            .resource::<cyrus_script::clock::SimulationClock>()
            .step()
            .nominal_ticks;
        assert!((1..=199).contains(&ticks));
        if enabled == 1 {
            assert_eq!((after.0, after.1), (before.0, 1));
        } else {
            assert_eq!((after.0, after.1), (before.0.wrapping_add(ticks as u8), 0));
        }
    }
    for enabled in [0, 1] {
        app.world_mut()
            .resource_mut::<cyrus_game::cheats::Cheats>()
            .execute("drno", Some(0))
            .unwrap();
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
        let reference = u16::try_from(changed_scripts.definition.references.len()).unwrap();
        changed_scripts
            .definition
            .references
            .push(Property::TorchTimer);
        changed_scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .program
            .steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Assign {
                        destination: Place::Property {
                            object: Object::Me,
                            reference,
                        },
                        formula: Formula {
                            first: Value::Integer { value: 200 },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Yield,
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::Increment {
                        destination: Place::Local { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                3,
                Step {
                    next: 4,
                    operation: Operation::End { target: 2 },
                },
            ),
        ]);
        changed_scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::PanicGroup] =
            AnimationGroup::IDLE_TORCH.index() as u8;
        changed_scripts.clock.normal_frame_rate = 12;
        changed_scripts.clock.min_frame_rate = 12;
        changed_scripts.clock.max_frame_rate = 12;
        changed_scripts.clock.use_smooth_fps = false;
        changed_scripts.clock.use_smooth_divisor = false;
        let prefix_clock = changed_scripts.clock;
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
        changed_animations.metadata["script_content_id"] =
            changed_scripts.content_id.clone().into();
        let groups = changed_animations.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "LOGIC")
            .unwrap()["animation_groups"]
            .as_array_mut()
            .unwrap();
        groups.retain(|group| group["group_index"] != AnimationGroup::IDLE_TORCH.index());
        let mut torch_group = groups[0].clone();
        torch_group["group_index"] = AnimationGroup::IDLE_TORCH.index().into();
        groups.push(torch_group);
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
        app.insert_resource(cyrus_script::clock::SimulationClock::new(prefix_clock).unwrap());
        app.update();
        let initial_frame = {
            let mut actors = app.world_mut().resource_mut::<ScriptActors>();
            let world = actors.current.as_mut().unwrap();
            assert_eq!(world.ordinary_updates(), 1);
            let id = world.id(0).unwrap();
            assert_ne!(world.selected_player(), Some(id));
            assert!(
                world.actors()[0].failure().is_none(),
                "{:?}",
                world.actors()[0].failure()
            );
            assert_eq!(
                world.actors()[0].control_state().unwrap().torch_burning,
                200
            );
            world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
            world
                .select_map_animation_default(id, false, 0, &mut |_, _| Ok(()))
                .unwrap();
            world
                .animation_frame(id)
                .unwrap()
                .expect("automatically initialized frame")
        };
        app.world_mut()
            .resource_mut::<cyrus_game::cheats::Cheats>()
            .execute("drno", Some(enabled))
            .unwrap();
        for step in 0..8_u64 {
            let before_local = map_actor_locals(&app)[0];
            app.update();
            let actors = app.world().resource::<ScriptActors>();
            assert!(!matches!(
                app.world().resource::<cyrus_script::script::ScriptStatus>(),
                cyrus_script::script::ScriptStatus::Failed { .. }
            ));
            let world = actors.current.as_ref().unwrap();
            let id = world.id(0).unwrap();
            assert!(world.actors()[0].failure().is_none());
            assert_eq!(world.ordinary_updates(), step + 2);
            assert_eq!(
                app.world()
                    .resource::<cyrus_script::clock::SimulationClock>()
                    .step()
                    .nominal_ticks,
                1
            );
            let control = world.actors()[0].control_state().unwrap();
            assert_eq!(world.actor_surface(id).unwrap(), Surface::UNKNOWN);
            assert_eq!(control.living_state, LivingState::ORDINARY);
            assert_eq!(
                control.torch_burning,
                if enabled != 0 || step < 6 { 200 } else { 199 }
            );
            assert_eq!(
                world.animation_frame(id).unwrap(),
                Some(if enabled != 0 {
                    initial_frame
                } else {
                    10 + (initial_frame - 10 + step as u32) % 4
                })
            );
            assert_eq!(
                map_actor_locals(&app)[0],
                before_local + if enabled == 0 { 1 } else { 0 }
            );
        }
    }
    app.insert_resource(bevy::time::TimeUpdateStrategy::Automatic);
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
            .get::<cyrus_script::script::ScriptBinding>(player)
            .is_none()
    );
}
