use super::*;

pub(super) fn player_walking_fixture() -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    player_walking_fixture_with(|_| {})
}

/// The walking fixture with the caller's changes to the scripts before they are identified.
pub(super) fn player_walking_fixture_with(
    prepare: impl FnOnce(&mut Scripts),
) -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    player_walking_fixture_with_animation(prepare, |_| {})
}

pub(super) fn player_walking_fixture_with_animation(
    prepare: impl FnOnce(&mut Scripts),
    prepare_animation: impl FnOnce(&mut cyrus_assets::animation::Animations),
) -> (
    Scripts,
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    use cyrus_assets::animation::AnimationModel;
    use cyrus_script::script::ScriptActorSource;
    let mut scripts = cache();
    add_standalone_receivers(&mut scripts);
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::Collide] = 15;
    scripts.actors.get_mut("CYRUS").unwrap().attributes.extend([
        (Attribute::ForwardSpeed, 24),
        (Attribute::BackwardSpeed, 8),
        (Attribute::SideSpeed, 8),
        (Attribute::BackwardWalk, 8),
        (Attribute::Combat, 1),
        (Attribute::ReservedCollide, 15),
        (Attribute::ForwardWalk, 1),
        (Attribute::StepType, 1),
    ]);
    let function_id = u16::try_from(scripts.definition.functions.len()).unwrap();
    scripts.definition.functions.push(Function {
        name: Native::SetMyAttr,
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
            function: Native::SetMyAttr,
            target: Object::Me,
            kind: CallKind::Function,
            arguments: vec![
                Value::Integer {
                    value: Attribute::ForwardWalk.index() as i32,
                },
                Value::Integer { value: 2 },
            ],
        },
    };
    let hook_id = u16::try_from(scripts.definition.functions.len()).unwrap();
    scripts.definition.functions.push(Function {
        name: Native::PlayerMain,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("CYRUS").unwrap().native_hook = Some(Call {
        function_id: hook_id,
        function: Native::PlayerMain,
        target: Object::Me,
        kind: CallKind::Function,
        arguments: vec![],
    });
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.map_position = [10, 20, 30];
    }
    prepare(&mut scripts);
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
        "ravc_count":0,"mpsz_bounds_primary":0,"raex":{"unknown_12":256,"v_vertex":0,"v_size":0},
        "raan_entries":[{"file_path":"PLAYER.3D","frame_count":1}],
        "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
            {"group_index":18,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":19,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":22,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":24,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":49,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":149,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
            {"group_index":150,"anim_id":1,"anim_type":0,"commands":[
                {"opcode":0,"frame_reference":0},{"opcode":1}]},
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
    prepare_animation(&mut data);
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
        2
    );
    let mut scene = ordinary_collision_scene(world.world, models);
    scene.settings.post_collide_height = 4;
    scene.definition = Some(
        cyrus_script::player::definition::PlayerDefinition::from_metadata(&data.metadata).unwrap(),
    );
    (scripts, world, data, scene)
}

#[test]
fn keyboard_walk_runs_through_ordinary_scheduler_and_projects_release() {
    keyboard_walk_session(false);
}

#[test]
fn drno_keeps_the_selected_players_keyboard_and_script_updates_running() {
    keyboard_walk_session(true);
}

fn keyboard_walk_session(verify_drno: bool) {
    use bevy::{asset::AssetApp, prelude::*, state::app::StatesPlugin};
    use cyrus_assets::animation::Animations;
    use cyrus_game::AppState;
    use cyrus_script::{
        player::{
            Player, PlayerPlugin,
            input::{PlayerBindings, PlayerInput, PlayerToggles},
            placement::PlayerPose,
        },
        script::{ScriptActors, ScriptPlugin, ScriptView},
    };
    use cyrus_world::{animation::AnimationRequest, lifecycle::PreparedWorld, sound::SoundCue};

    let (scripts, mut world, mut data, mut scene) = player_walking_fixture_with(|scripts| {
        let program = &mut scripts.actors.get_mut("CYRUS").unwrap().program;
        program.steps.insert(
            1,
            Step {
                next: 2,
                operation: Operation::Increment {
                    destination: Place::Local { index: 0 },
                    delta: 1,
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
    });
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
            .position(|actor| {
                matches!(
                    actor.source,
                    cyrus_script::script::ScriptActorSource::Player
                )
            })
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
        .load::<cyrus_script::player::data::PlayerScene>("keyboard-test.player");
    app.world_mut()
        .resource_mut::<Assets<cyrus_script::player::data::PlayerScene>>()
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
    let _environment = bind_scheduler_environment(&mut app);
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
                app.world().resource::<cyrus_script::script::ScriptStatus>(),
                cyrus_script::script::ScriptStatus::Failed { .. }
            ),
            "{:?}",
            app.world().resource::<cyrus_script::script::ScriptStatus>()
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
    let actors = app.world().resource::<ScriptActors>();
    let world = actors.current.as_ref().unwrap();
    assert_eq!(
        world
            .model_presentation(player_id)
            .unwrap()
            .unwrap()
            .position,
        initial.map(|value| (value >> 8) as f32),
        "model preparation must capture the position before ordinary movement"
    );
    let alternate = world
        .actors()
        .iter()
        .position(|actor| {
            matches!(
                actor.source,
                cyrus_script::script::ScriptActorSource::AlternatePlayer
            )
        })
        .unwrap();
    assert!(
        world
            .model_presentation(world.id(alternate).unwrap())
            .unwrap()
            .is_none()
    );
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
    assert!(cues.iter().any(|cue| matches!(cue, SoundCue::Animation { actor, sound: cyrus_world::animation::AnimationSound::Play { effect: 9, .. }, surface: Some(Surface::UNKNOWN), .. } if *actor == player)), "{cues:?}");
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
    if verify_drno {
        struct EmptyDrnoStore;
        impl cyrus_game::cheats::CheatStore for EmptyDrnoStore {
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
        app.insert_resource(cyrus_game::cheats::Cheats::new(EmptyDrnoStore));
        let player_local = |app: &App| {
            let actors = app.world().resource::<ScriptActors>();
            let world = actors.current.as_ref().unwrap();
            assert_eq!(world.selected_player(), Some(player_id));
            assert_eq!(
                world.actors()[player_id.index].attributes()[Attribute::ForwardWalk],
                2
            );
            world.actors()[player_id.index]
                .runtime()
                .unwrap()
                .variables()[0]
        };
        for enabled in [0, 1, 0] {
            app.world_mut()
                .resource_mut::<cyrus_game::cheats::Cheats>()
                .execute("drno", Some(enabled))
                .unwrap();
            let before_local = player_local(&app).integer().unwrap();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ArrowUp);
            app.update();
            assert!(app.world().resource::<PlayerInput>().forward);
            // The selected player's main stage runs under every cheat state. Travel is not asserted:
            // how far a pass moves relative to the map actors' collision models is not established.
            assert_eq!(player_local(&app), before_local + 1, "drno {enabled}");
            let after_position = position(&app);
            assert_eq!(
                app.world().get::<PlayerPose>(player).unwrap().position,
                after_position.map(i64::from)
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::ArrowUp);
            for _ in 0..12 {
                app.update();
                position(&app);
            }
            let released = position(&app);
            app.update();
            assert_eq!(position(&app), released, "drno {enabled}");
        }
        return;
    }
    let stopped = position(&app);
    let heading = |app: &App| {
        let actors = app.world().resource::<ScriptActors>();
        let world = actors.current.as_ref().unwrap();
        assert!(world.actors()[player_id.index].failure().is_none());
        world.spatial(player_id).unwrap().local.angles
    };
    let before = heading(&app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowLeft);
    // This fixture turns one integer heading unit per nominal tick. A sub-tick
    // update truncates that amount to zero, so sample over a whole tick here.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(100),
    ));
    app.update();
    let turning = heading(&app);
    let gained = (turning[1] + 2048 - before[1]) % 2048;
    assert!((1..=48).contains(&gained), "{before:?} -> {turning:?}");
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(55),
    ));
    assert_eq!(position(&app), stopped);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ArrowLeft);
    app.update();
    assert_eq!(heading(&app)[1], turning[1]);
    for _ in 0..12 {
        app.update();
        position(&app);
    }
    {
        let mut scenes = app
            .world_mut()
            .resource_mut::<Assets<cyrus_script::player::data::PlayerScene>>();
        let mut scene = scenes.get_mut(&scene_handle).unwrap();
        scene.terrain = Some(
            cyrus_collision::ground::TerrainGround::new(
                vec![0; 256 * 256],
                vec![false; 256 * 256],
                Some(vec![0; 256 * 256]),
            )
            .unwrap(),
        );
        scene.collision.texture_surfaces = Some(Default::default());
    }
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        world
            .set_actor_surface(player_id, Surface::SCAPEWATER)
            .unwrap();
        world
            .initialize_camera_pose(cyrus_script::script::camera::CameraPose {
                position: [0; 3],
                angles: [0; 3],
            })
            .unwrap();
        world
            .prepare_environment_effects(&water_model_tests::effects())
            .unwrap();
    }
    let logical = position(&app);
    app.update();
    let actors = app.world().resource::<ScriptActors>();
    let world = actors.current.as_ref().unwrap();
    let failure = world.actors()[player_id.index].failure();
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(
        world.model_position(player_id).unwrap().unwrap()[1],
        (logical[1] >> 8).wrapping_add(64) as f32
    );
    assert_eq!(world.environmental_effects().unwrap().active().count(), 1);
}

#[test]
fn the_walk_key_with_left_sidesteps_the_player_without_turning() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        left: true,
        walk: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: true,
                    auto_defend: false,
                    steer: None,
                    claim: None,
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                lateral_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // Verified (preservation animation.md step ramp): with `attr_step_type` 1 the first sidestep
    // moves a quarter of `attr_side_speed`. Inferred: at that length the initialized quarter-turn
    // residual rounds away, so stored Z is unchanged. The sidestep preserves the heading.
    assert_eq!(
        (after.position[0] - before.position[0]).abs(),
        512,
        "{before:?} -> {after:?}"
    );
    assert_eq!(after.position[2], before.position[2]);
    assert_eq!(after.angles, before.angles);
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::LEFT
    );
}

#[test]
fn forward_with_walk_mode_off_runs_the_player_along_the_heading() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: None,
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // Running steps along the heading (zero: +Z): Z advances; X and the heading do not.
    assert!(
        after.position[2] > before.position[2],
        "{before:?} -> {after:?}"
    );
    assert_eq!(after.position[0], before.position[0]);
    assert_eq!(after.angles, before.angles);
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::FORWARD
    );
}

#[test]
fn a_mod_claim_replaces_the_original_locomotion_with_its_own_heading_and_speed() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: Some(cyrus_script::player::input::LocomotionClaim {
                        heading: 512,
                        travel: 512,
                        speed: 4.0,
                        gait: cyrus_script::player::input::Gait::Run,
                        jump: false,
                        tuning: Default::default(),
                    }),
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // The held forward key would run along +Z; the claim faces a quarter turn and moves
    // four map units (1024 in Q8) along that heading instead. The rotation table's residual at
    // a quarter turn may truncate the other axis by one Q8 unit (preservation player.md,
    // heading-based translation).
    assert_eq!(after.angles[1], 512, "{before:?} -> {after:?}");
    assert_eq!(
        (after.position[0] - before.position[0]).abs(),
        1024,
        "{before:?} -> {after:?}"
    );
    assert!(
        after.position[2].abs_diff(before.position[2]) <= 1,
        "{before:?} -> {after:?}"
    );
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::FORWARD
    );
}

#[test]
fn collision_probes_answer_in_request_order_while_a_mod_claims_locomotion() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let origin = world
        .spatial(player)
        .unwrap()
        .local
        .position
        .map(|q8| (q8 >> 8) as f32);
    let mut probes = cyrus_script::player::input::CollisionProbes::default();
    probes.requests = vec![
        // A long vertical segment through the player, and one far from any geometry.
        cyrus_script::player::input::Probe {
            start: [origin[0], origin[1] - 4000.0, origin[2]],
            end: [origin[0], origin[1] + 4000.0, origin[2]],
        },
        cyrus_script::player::input::Probe {
            start: [origin[0] + 1.0e6, 0.0, origin[2]],
            end: [origin[0] + 1.0e6, 1.0, origin[2]],
        },
    ];
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: Some(cyrus_script::player::input::LocomotionClaim {
                        heading: 512,
                        travel: 512,
                        speed: 4.0,
                        gait: cyrus_script::player::input::Gait::Run,
                        jump: false,
                        tuning: Default::default(),
                    }),
                    probes: Some(&probes),
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let answers = probes.take_answers().expect("the claim answers its probes");
    assert_eq!(answers.len(), 2, "one answer per request, in order");
    // The vertical segment meets the fixture's ground under the player, facing up (Y grows
    // downward in the original axes, so the normal's Y is negative).
    let hit = answers[0].expect("the vertical probe meets the ground");
    assert_eq!((hit.point[0], hit.point[2]), (origin[0], origin[2]));
    assert!(hit.normal[1] < -0.99, "{hit:?}");
    assert!(answers[1].is_none(), "nothing exists a million units away");
}

#[test]
fn a_mod_claim_steers_a_jump_in_the_air_and_scales_the_jump_only_on_the_ground() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    // Jump tuning scales the height on the ground; a change waits while the player is airborne.
    world
        .set_actor_attribute(
            (player).index,
            (cyrus_soup::Attribute::JumpHeight).index(),
            80,
        )
        .unwrap();
    let base = world.selected_jump_height().unwrap();
    assert_eq!(base, 80);
    let doubled = cyrus_script::player::input::PlayerTuning {
        jump_height: 2.0,
        jump_speed: 1.0,
    };
    world.set_player_tuning(doubled);
    assert_eq!(
        world.selected_jump_height(),
        Some((u16::from(base) * 2).min(255) as u8)
    );
    world.set_falling_for_test(player, cyrus_script::script::JumpMotion::STANDING, 0);
    world.set_player_tuning(Default::default());
    assert_eq!(
        world.selected_jump_height(),
        Some((u16::from(base) * 2).min(255) as u8),
        "a tuning change waits for the ground"
    );
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: Some(cyrus_script::player::input::LocomotionClaim {
                        heading: 700,
                        travel: 700,
                        speed: 4.0,
                        gait: cyrus_script::player::input::Gait::Run,
                        jump: false,
                        tuning: Default::default(),
                    }),
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // Airborne: the claim steers the heading of the jump and nothing else moves the player.
    assert_eq!(after.angles[1], 700, "{before:?} -> {after:?}");
    assert_eq!(after.position, before.position, "{before:?} -> {after:?}");
}

#[test]
fn the_walk_key_swims_forward_in_deep_water_instead_of_walking() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    world.set_actor_surface(player, Surface::DEEPWATER).unwrap();
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        walk: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: None,
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                locomotion_geometry: Some(cyrus_script::script::LocomotionGeometry {
                    ground: OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    environment: cyrus_script::script::ObstacleEnvironment {
                        selected_player: Some(player),
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 1,
                    },
                }),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    // The swim group is only requested on the first update, so the step itself comes later.
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::FORWARD
    );
}

#[test]
fn the_jump_key_from_the_backward_run_group_takes_the_backward_jump_row() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let step = |world: &mut ScriptWorld, held: &cyrus_script::player::input::PlayerInput| {
        let mut cues = Vec::new();
        world
            .advance_player_controller(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                cyrus_script::clock::ClockStep {
                    frame_scale: 256,
                    nominal_ticks: 0,
                    fine_ticks: 0,
                },
                ScriptInputs {
                    player_controller: Some(PlayerControllerInput {
                        held,
                        walk_mode: false,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    hud: Some(&hud),
                    fine_ticks: Some(0),
                    shell_execution: Some(false),
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    forward_step_clear: Some(true),
                    locomotion_geometry: Some(cyrus_script::script::LocomotionGeometry {
                        ground: OrdinaryGroundQuery {
                            scene: &scene,
                            animations: &data,
                            prepared: &prepared,
                            distance: 4,
                            sidedness: false,
                        },
                        environment: cyrus_script::script::ObstacleEnvironment {
                            selected_player: Some(player),
                            auto_grab: false,
                            item_system_ready: false,
                            shell_execution: false,
                            ticks: 1,
                        },
                    }),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |id, cue| {
                    cues.push((id, cue));
                    Ok(())
                },
            )
            .unwrap();
    };
    let backward = cyrus_script::player::input::PlayerInput {
        backward: true,
        ..Default::default()
    };
    step(&mut world, &backward);
    world
        .advance(
            player,
            cyrus_script::script::ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    let held = cyrus_script::player::input::PlayerInput {
        backward: true,
        jump: true,
        ..Default::default()
    };
    step(&mut world, &held);
    // Original: Down + Space from the backward run group jumps in `anim_jump_backward` (queued 3) and starts
    // the controller cooldown; the unarmed standing jump (`anim_jump_start`, queued 1) is wrong here.
    assert_eq!(
        world.actors()[index].control_state().unwrap().queued_motion,
        JumpMotion::BACKWARD
    );
}

#[test]
fn a_sliding_player_keeps_its_controller_running_but_takes_no_step() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
    let (_, mut world, data, scene) = player_walking_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    // The slope response's slide is the only writer of this state.
    world.set_actor_sliding(player, true).unwrap();
    let map = world.id(0).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
        .unwrap();
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: None,
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // Every movement helper refuses while sliding: the controller ran without an error.
    assert_eq!(after.position, before.position);
    assert_eq!(after.angles, before.angles);
}

#[test]
fn backward_with_walk_mode_off_runs_the_player_against_the_heading() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        backward: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: None,
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                forward_step_clear: Some(true),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // Running backward steps against the heading (zero: +Z): Z decreases; X and the heading do not.
    assert!(
        after.position[2] < before.position[2],
        "{before:?} -> {after:?}"
    );
    assert_eq!(after.position[0], before.position[0]);
    assert_eq!(after.angles, before.angles);
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::WALK_BACKWARD
    );
}

#[test]
fn backward_with_the_walk_key_walks_the_player_back_through_the_support_check() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
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
    let hud_asset = cyrus_assets::hud::HudAsset {
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
        inventory: Default::default(),
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
    };
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(&hud_asset, &cyrus_game::preferences::Preferences::default());
    world.begin_player_movement_update(player, 0).unwrap();
    let before = world.spatial(player).unwrap().local;
    let held = cyrus_script::player::input::PlayerInput {
        backward: true,
        walk: true,
        ..Default::default()
    };
    let mut cues = Vec::new();
    world
        .advance_player_controller(
            player,
            OrdinaryGroundQuery {
                scene: &scene,
                animations: &data,
                prepared: &prepared,
                distance: 4,
                sidedness: false,
            },
            cyrus_script::clock::ClockStep {
                frame_scale: 256,
                nominal_ticks: 0,
                fine_ticks: 0,
            },
            ScriptInputs {
                player_controller: Some(PlayerControllerInput {
                    held: &held,
                    walk_mode: false,
                    auto_defend: false,
                    steer: None,
                    claim: None,
                    probes: None,
                }),
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                locomotion_geometry: Some(cyrus_script::script::LocomotionGeometry {
                    ground: OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    environment: cyrus_script::script::ObstacleEnvironment {
                        selected_player: Some(player),
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 1,
                    },
                }),
                ..Default::default()
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |id, cue| {
                cues.push((id, cue));
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local;
    // Running backward steps against the heading (zero: +Z): Z decreases; X and the heading do not.
    assert!(
        after.position[2] < before.position[2],
        "{before:?} -> {after:?}"
    );
    assert_eq!(after.position[0], before.position[0]);
    assert_eq!(after.angles, before.angles);
    assert_eq!(
        world.actors()[index]
            .control_state()
            .unwrap()
            .motion_direction,
        MotionDirection::WALK_BACKWARD
    );
}

#[test]
fn scripted_walk_steps_move_the_selected_player_through_the_support_check() {
    use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource, ScriptInputs};
    // Walking forward advances Z along the heading (zero: +Z); walking backward reverses it.
    for (function, direction) in [(Native::WalkForward, 1), (Native::WalkBackward, -1)] {
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
        let hud = cyrus_script::script::ScriptHud::default();
        world.begin_player_movement_update(player, 0).unwrap();
        let before = world.spatial(player).unwrap().local.position;
        let inputs = ScriptInputs {
            hud: Some(&hud),
            shell_execution: Some(false),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            locomotion_geometry: Some(cyrus_script::script::LocomotionGeometry {
                ground: OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                environment: cyrus_script::script::ObstacleEnvironment {
                    selected_player: Some(player),
                    auto_grab: false,
                    item_system_ready: false,
                    shell_execution: false,
                    ticks: 1,
                },
            }),
            ..Default::default()
        };
        world
            .run_player_native_hook(player, &mut |world| {
                world.step_player(player, function, inputs, &mut |_, _| Ok(()), None)
            })
            .unwrap();
        let after = world.spatial(player).unwrap().local.position;
        assert_eq!(after[0], before[0], "{function}");
        assert!(
            (after[2] - before[2]) * direction > 0,
            "{function}: {before:?} -> {after:?}"
        );
    }
}

#[test]
fn selected_directional_refusal_marks_response_without_moving_the_player() {
    use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource, StepDirection};
    let (_, mut world, data, scene) = player_walking_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    let position = [2560, 5120, 7680];
    world.place_selected_player(position, 0).unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
        .unwrap();
    for (direction, expected) in [
        (StepDirection::Forward, 0x80),
        (StepDirection::Left, 0xa0),
        (StepDirection::Right, 0xe0),
        (StepDirection::Backward, 0x1e0),
    ] {
        assert!(
            !world
                .player_step_clear(
                    player,
                    OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 4,
                        sidedness: false,
                    },
                    direction
                )
                .unwrap(),
            "{direction:?}"
        );
        assert_eq!(world.spatial(player).unwrap().local.position, position);
        let control = world.actors()[index].control_state().unwrap();
        assert_eq!(control.last_collide.bits(), expected);
        assert!(cyrus_script::script::alignment::fallback_response(
            control.last_collide
        ));
    }
    world
        .finish_ordinary_script_response(player, Some(9))
        .unwrap();
    let control = world.actors()[index].control_state().unwrap();
    assert_eq!(control.last_collide_count, 9);
    assert!(control.last_collide.is_empty());
}

/// The `animation` cheat at zero turns the animation system off. The original's selected player
/// still walks with the walk key (the startup distance per update, the group frozen); native
/// used to stop the whole script world with "walking stage requires enabled animation system".
#[test]
fn walking_with_the_animation_system_off_still_advances_and_keeps_the_group() {
    use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource};
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
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 4,
        sidedness: false,
    };
    let timing = cyrus_script::clock::ClockStep {
        frame_scale: 256,
        nominal_ticks: 0,
        fine_ticks: 0,
    };
    // Apply the cheat's switch state the way the ordinary update carries it into the world.
    world
        .advance(
            player,
            cyrus_script::script::ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(timing.frame_scale),
                nominal_ticks: Some(timing.nominal_ticks),
                animation_running: Some(false),
                animation_system: Some(false),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    let frame = world.animation_frame(player).unwrap();
    let start = world.spatial(player).unwrap().local.position;
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut positions = Vec::new();
    for _ in 0..3 {
        world.begin_player_movement_update(player, 0).unwrap();
        world
            .advance_player_controller(
                player,
                query(),
                timing,
                cyrus_script::script::ScriptInputs {
                    player_controller: Some(cyrus_script::script::PlayerControllerInput {
                        held: &held,
                        walk_mode: true,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        positions.push(world.spatial(player).unwrap().local.position[2]);
        world.finish_player_movement_update(player).unwrap();
    }
    assert!(positions[0] > start[2], "{positions:?} from {start:?}");
    assert!(
        positions.windows(2).all(|pair| pair[1] > pair[0]),
        "{positions:?}"
    );
    assert_eq!(world.animation_frame(player).unwrap(), frame);
}

#[test]
fn selected_player_collision_uses_logical_pose_and_live_map_support() {
    use cyrus_collision::ground::SupportSource;
    use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource};
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
    // The response kind a PlayerOnScape reads: an actor face is not terrain.
    assert_eq!(
        world.actors()[index].control_state().unwrap().ground_kind,
        cyrus_script::script::GroundKind::FACE
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
    assert_eq!(world.actor_surface(player).unwrap(), Surface::UNKNOWN);
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
        classification: SurfaceClass::Wall,
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
            .last_collide
            .bits(),
        4
    );
    let query = || OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 4,
        sidedness: false,
    };
    let timing = cyrus_script::clock::ClockStep {
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
                cyrus_script::script::ScriptInputs {
                    player_controller: Some(cyrus_script::script::PlayerControllerInput {
                        held: &cyrus_script::player::input::PlayerInput {
                            forward: true,
                            ..Default::default()
                        },
                        walk_mode: true,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    ..Default::default()
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
            control.move_counter
        ),
        (
            MotionDirection::WALK_FORWARD,
            MotionDirection::FORWARD,
            0x30
        )
    );
    assert!(cues.iter().any(|(id, cue)| *id == player
        && matches!(
            cue,
            cyrus_script::script::ScriptSound::Animation {
                sound: cyrus_world::animation::AnimationSound::Play { .. },
                surface: Some(Surface::UNKNOWN),
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
            cyrus_script::script::ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(timing.frame_scale),
                nominal_ticks: Some(timing.nominal_ticks),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        world.actors()[index].attributes()[Attribute::ForwardWalk],
        2
    );
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
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    world
        .update_ordinary(
            &data,
            [10, 30],
            cyrus_script::script::OrdinaryInputs {
                scene: Some(&scene),
                scripts: cyrus_script::script::ScriptInputs {
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    shell_execution: Some(false),
                    player_controller: Some(cyrus_script::script::PlayerControllerInput {
                        held: &held,
                        walk_mode: true,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
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
        (ExecutorState::IDLE, -1, 0)
    );
    assert_eq!(combat.retained_action_indices, [-1; 2]);
    let released = cyrus_script::player::input::PlayerInput::default();
    let mut released_positions = Vec::new();
    for _ in 0..8 {
        world
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_script::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: cyrus_script::script::ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        shell_execution: Some(false),
                        player_controller: Some(cyrus_script::script::PlayerControllerInput {
                            held: &released,
                            walk_mode: true,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
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

fn held_walk_turn_in_states(states: &[LivingState], sliding: u8) {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptInputs, WorldRestoreContext,
        WorldSnapshotSources,
    };
    for &living in states {
        let (scripts, mut world, mut data, scene) = player_walking_fixture();
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        let player = world.selected_player().unwrap();
        let map = world.id(0).unwrap();
        let held = cyrus_script::player::input::PlayerInput {
            left: true,
            ..Default::default()
        };
        // Establish a real turn accumulator through the ordinary public scheduler.
        world
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_script::script::OrdinaryInputs {
                    scene: Some(&scene),
                    scripts: ScriptInputs {
                        nominal_ticks: Some(0),
                        fine_ticks: Some(0),
                        frame_scale: Some(256),
                        shell_execution: Some(false),
                        player_controller: Some(PlayerControllerInput {
                            held: &held,
                            walk_mode: false,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(world.actors()[player.index].failure().is_none());
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][player.index]["control"]["living_state"] = living.byte().into();
        // Parameterize sliding so the heading control cannot mask walking admission.
        saved["actors"][player.index]["control"]["sliding_state"] = sliding.into();
        let globals = std::mem::take(world.globals_mut());
        world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &WorldSnapshotSources {
                identity_memo: None,
                effects: None,
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &Default::default(),
            },
            WorldRestoreContext {
                generation: world.generation() + 1,
                globals,
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let player = world.id(player.index).unwrap();
        let map = world.id(map.index).unwrap();
        world.begin_player_movement_update(player, 0).unwrap();
        let before_position = world.spatial(player).unwrap().local.position;
        let before = world.spatial(player).unwrap().local.angles;
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
            .unwrap();
        let held = cyrus_script::player::input::PlayerInput {
            left: true,
            forward: true,
            walk: true,
            ..Default::default()
        };
        world
            .advance_player_controller(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                cyrus_script::clock::ClockStep {
                    frame_scale: 256,
                    nominal_ticks: 0,
                    fine_ticks: 0,
                },
                ScriptInputs {
                    player_controller: Some(PlayerControllerInput {
                        held: &held,
                        walk_mode: false,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    shell_execution: Some(false),
                    nominal_ticks: Some(0),
                    fine_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        world.finish_player_movement_update(player).unwrap();
        let after = world.spatial(player).unwrap().local.angles;
        if living.is_ledge() {
            assert_eq!(after[1], before[1], "ledge state {living} owns heading");
            assert_eq!(
                world.spatial(player).unwrap().local.position,
                before_position
            );
        } else {
            assert_ne!(after[1], before[1], "ordinary turning must remain live");
        }
    }
}

#[test]
fn held_walk_turn_preserves_ledge_heading_and_resumes_outside_ledge_states() {
    held_walk_turn_in_states(
        &[
            LivingState::ORDINARY,
            LivingState::TURNING_TO_HIGH_LEDGE,
            LivingState::HIGH_CLIMB,
            LivingState::TURNING_TO_LOW_LEDGE,
            LivingState::LOW_CLIMB,
            LivingState::CATCHING_HANG,
            LivingState::CLIMB_FROM_HANG,
            LivingState::DROPPING,
            LivingState::CLIMB_FINISHED,
            LivingState::ENTERING_LEDGE,
        ],
        1,
    );
}

#[test]
fn held_walk_forward_preserves_low_climb_without_stopping_controller() {
    held_walk_turn_in_states(&[LivingState::LOW_CLIMB], 0);
}

#[test]
fn armed_no_input_controller_samples_combat_proximity_once() {
    use cyrus_script::script::{OrdinaryGroundQuery, PlayerControllerInput, ScriptInputs};
    for (armed, seed, expected) in [
        (false, 744964398, 744964398),
        (true, 744964398, 1969681615),
        (true, 0, 12345),
    ] {
        let (scripts, mut world, mut data, mut scene) = player_walking_fixture_with(|scripts| {
            scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::HoldingWeapon] =
                u8::from(armed);
        });
        world = proximity_world_with_target(world, &scripts, &mut data);
        scene.settings.tap_time = Some(8);
        scene.settings.sheath_sword_delay = Some(12);
        scene.settings.defend_interval = Some(8);
        let player = world.selected_player().unwrap();
        let map = world.id(0).unwrap();
        let actors = [player, map];
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &actors)
            .unwrap();
        world.ordinary_actors([10, 30]).unwrap();
        world.begin_player_movement_update(player, 0).unwrap();
        let before = world.spatial(player).unwrap().local;
        let held = cyrus_script::player::input::PlayerInput::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(seed);
        world
            .advance_player_controller(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                cyrus_script::clock::ClockStep {
                    frame_scale: 202,
                    nominal_ticks: 0,
                    fine_ticks: 0,
                },
                ScriptInputs {
                    player_controller: Some(PlayerControllerInput {
                        held: &held,
                        walk_mode: false,
                        auto_defend: true,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    shell_execution: Some(false),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(random.state(), expected);
        let control = world.actors()[player.index].control_state().unwrap();
        assert_eq!(
            control.combat.as_ref().unwrap().target,
            if armed && seed == 0 { None } else { Some(map) }
        );
        let after = world.spatial(player).unwrap().local;
        assert_eq!(after.position, before.position);
        assert_eq!(after.angles, before.angles);
        assert_eq!(after.matrix, before.matrix);
    }
}

fn proximity_world_with_target(
    mut world: ScriptWorld,
    scripts: &Scripts,
    data: &mut cyrus_assets::animation::Animations,
) -> ScriptWorld {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let index = world.selected_player().unwrap().index;
    let mut saved = movement_snapshot(&world, scripts, data);
    saved["actors"][index]["control"]["combat"]["target"] =
        serde_json::json!([world.generation(), 0]);
    ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts,
            animations: data,
            collision: &Default::default(),
        },
        WorldRestoreContext {
            generation: world.generation() + 1,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap()
}
