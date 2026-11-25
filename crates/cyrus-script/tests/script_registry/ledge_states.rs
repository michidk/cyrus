use super::*;

#[test]
fn climbing_forward_queues_motion_without_leaving_the_climb() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
        WorldRestoreContext, WorldSnapshotSources,
    };
    let (scripts, mut world, mut data, scene) = player_walking_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "CYRUS")
        .unwrap();
    definition["animation_groups"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "group_index":86,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
        }));
    let mut playback =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", AnimationGroup::PANIC)
            .unwrap();
    let frame = playback
        .force_request_with_sounds(AnimationGroup::PULL_UP_LEDGE, 0, 0, &mut |_| Ok(()))
        .unwrap();
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let mut saved = movement_snapshot(&world, &scripts, &data);
    saved["actors"][index]["animation"] =
        serde_json::json!({"playback":playback.snapshot(),"frame":frame});
    saved["actors"][index]["control"]["living_state"] = 11.into();
    let globals = std::mem::take(world.globals_mut());
    let sources = WorldSnapshotSources {
        identity_memo: None,
        effects: None,
        player: None,
        scripts: &scripts,
        animations: &data,
        collision: &Default::default(),
    };
    let mut world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &sources,
        WorldRestoreContext {
            generation: world.generation() + 1,
            globals,
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    let before = world.inspect(index, 1)["actors"][0]["spatial"].clone();
    let hud = cyrus_script::script::ScriptHud::default();
    for forward in [true, false] {
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
            .unwrap();
        let held = cyrus_script::player::input::PlayerInput {
            forward,
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
                    hud: Some(&hud),
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
                            ticks: 0,
                        },
                    }),
                    player_controller: Some(PlayerControllerInput {
                        held: &held,
                        walk_mode: false,
                        auto_defend: false,
                        steer: None,
                        claim: None,
                        probes: None,
                    }),
                    shell_execution: Some(false),
                    fine_ticks: Some(0),
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        let inspected = world.inspect(index, 1);
        let actor = &inspected["actors"][0];
        assert_eq!(
            actor["animation"]["pending_group"],
            AnimationGroup::RUN.index(),
            "forward request survives release during the climb"
        );
        assert_eq!(
            actor["animation"]["current_group"],
            AnimationGroup::PULL_UP_LEDGE.index()
        );
        assert_eq!(actor["control"]["living_state"], 11);
        assert_eq!(
            actor["spatial"], before,
            "a queued movement animation must not translate the climbing actor"
        );
    }
}

#[test]
fn hanging_backward_releases_without_the_walk_key() {
    use cyrus_script::script::{
        OrdinaryGroundQuery, PlayerControllerInput, ScriptActorSource, ScriptInputs,
    };
    {
        let (scripts, world, mut data, scene) = player_walking_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let definition = data.metadata["actors"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|actor| actor["script_name"] == "CYRUS")
            .unwrap();
        definition["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":56,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
                saved["actors"][index]["control"]["living_state"] = 15.into();
            });
        let player = world.id(index).unwrap();
        let map = world.id(0).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
            .unwrap();
        let held = cyrus_script::player::input::PlayerInput {
            backward: true,
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
                    fine_ticks: Some(0),
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            world.actors()[index].control_state().unwrap().living_state,
            LivingState::ORDINARY
        );
        assert_eq!(
            world.inspect(index, 1)["actors"][0]["animation"]["current_group"],
            AnimationGroup::FALL.index()
        );
    }
}

pub(super) fn selected_hang_classification_fixture() -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    selected_hang_fixture_with_point_task(false)
}

pub(super) fn selected_hang_fixture_with_point_task(
    point_task: bool,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let (_, mut world, data, scene) = player_walking_fixture_with_animation(
        |scripts| {
            let function = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: cyrus_soup::Native::Jump,
                param_count: 0,
                kind: CallKind::Function,
            });
            let point = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: Native::MovePosition,
                param_count: 4,
                kind: CallKind::Task,
            });
            let queued = scripts.definition.references.len() as u16;
            scripts
                .definition
                .references
                .push(cyrus_soup::Property::JumpFlag);
            let living = scripts.definition.references.len() as u16;
            scripts
                .definition
                .references
                .push(cyrus_soup::Property::ExtraFlag);
            let property = |reference, value, next| Step {
                next,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference,
                    },
                    formula: Formula {
                        first: Value::Integer { value },
                        rest: vec![],
                    },
                },
            };
            scripts.actors.get_mut("CYRUS").unwrap().program.steps = BTreeMap::from([
                (0, assign_call(1, function, Native::Jump, &[])),
                (1, property(queued, 0, 2)),
                (2, property(living, 15, 3)),
                (
                    3,
                    Step {
                        next: 4,
                        operation: Operation::End { target: 4 },
                    },
                ),
                (
                    4,
                    if point_task {
                        Step {
                            next: 5,
                            operation: wait(
                                point,
                                Native::MovePosition,
                                CallKind::Task,
                                [0, 0, 100000, 0]
                                    .map(|value| Value::Integer { value })
                                    .to_vec(),
                            ),
                        }
                    } else {
                        assign_call(5, function, Native::Jump, &[])
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
        },
        |data| {
            let actor = data.metadata["actors"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|actor| actor["script_name"] == "CYRUS")
                .unwrap();
            for group in [32, 56] {
                let mut commands = vec![];
                if group == 56 {
                    commands.push(serde_json::json!({"opcode":4,"sound_param":9,"volume_shift":0}));
                }
                commands.extend([
                    serde_json::json!({"opcode":0,"frame_reference":0}),
                    serde_json::json!({"opcode":1}),
                ]);
                actor["animation_groups"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({
                        "group_index":group,"anim_id":1,"anim_type":0,"commands":commands
                    }));
            }
        },
    );
    let player = world.selected_player().unwrap();
    world.set_actor_surface(player, Surface::UNKNOWN).unwrap();
    advance_selected_hang_script(&mut world, &data, &scene, false).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.motion_classification,
            control.living_state.byte(),
            control.queued_motion.byte()
        ),
        (MotionDirection::FORWARD, 15, 0)
    );
    (world, data, scene)
}

pub(super) fn advance_selected_hang_script(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    fail: bool,
) -> anyhow::Result<()> {
    advance_selected_hang_stage(world, data, scene, fail, ScriptStage::Body(Thread::Main))
}

pub(super) fn advance_selected_hang_stage(
    world: &mut ScriptWorld,
    data: &cyrus_assets::animation::Animations,
    scene: &cyrus_script::player::data::PlayerScene,
    fail: bool,
    stage: ScriptStage,
) -> anyhow::Result<()> {
    use cyrus_script::script::{ActivationInputs, PlayerCollisionSource, ScriptInputs};
    let player = world
        .selected_player()
        .ok_or_else(|| anyhow::anyhow!("selected hang player absent"))?;
    let actors = [player, world.id(0)?];
    let activation = ActivationInputs {
        player,
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
    world
        .advance(
            player,
            stage,
            ScriptInputs {
                activation: Some(&activation),
                player_collision: Some(PlayerCollisionSource {
                    scene,
                    animations: data,
                    view: [10, 30],
                    actors: &actors,
                }),
                nominal_ticks: Some(0),
                fine_ticks: Some(0),
                frame_scale: Some(256),
                shell_execution: Some(false),
                ..Default::default()
            },
            Some(&mut |_, _| {
                anyhow::ensure!(!fail, "hang release sound rejected");
                Ok(())
            }),
            None,
            100,
        )
        .map(|_| ())
}

#[test]
fn selected_public_jump_hang_release_retains_nonzero_classification_and_continues() {
    let (mut world, data, scene) = selected_hang_classification_fixture();
    let player = world.selected_player().unwrap();
    let before = world.spatial(player).unwrap().local.position;
    advance_selected_hang_script(&mut world, &data, &scene, false).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.motion_classification,
            control.living_state.byte(),
            control.action_transition_counter,
            control.queued_motion.byte()
        ),
        (MotionDirection::FORWARD, 0, 6, 0)
    );
    assert_eq!(world.spatial(player).unwrap().local.position, before);
    advance_selected_hang_script(&mut world, &data, &scene, false).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.motion_classification,
            control.living_state.byte(),
            control.queued_motion.byte()
        ),
        (MotionDirection::FORWARD, 0, 1)
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
}

#[test]
fn selected_public_jump_hang_surface_refusal_retains_classification() {
    let (mut world, data, scene) = selected_hang_classification_fixture();
    let player = world.selected_player().unwrap();
    world.set_actor_surface(player, Surface::DEEPWATER).unwrap();
    advance_selected_hang_script(&mut world, &data, &scene, true).unwrap();
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.motion_classification,
            control.living_state.byte(),
            control.action_transition_counter,
            control.queued_motion.byte()
        ),
        (MotionDirection::FORWARD, 15, 0, 0)
    );
    world.set_actor_surface(player, Surface::UNKNOWN).unwrap();
    advance_selected_hang_script(&mut world, &data, &scene, false).unwrap();
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .living_state
            .byte(),
        0
    );
}

#[test]
fn selected_public_jump_hang_sound_failure_precedes_drop_completion() {
    let (mut world, data, scene) = selected_hang_classification_fixture();
    let player = world.selected_player().unwrap();
    let error = advance_selected_hang_script(&mut world, &data, &scene, true).unwrap_err();
    assert!(format!("{error:#}").contains("hang release sound rejected"));
    let control = world.actors()[player.index].control_state().unwrap();
    assert_eq!(
        (
            control.motion_classification,
            control.living_state.byte(),
            control.action_transition_counter,
            control.queued_motion.byte()
        ),
        (MotionDirection::FORWARD, 17, 0, 0)
    );
    assert!(world.actors()[player.index].animation_failure().is_some());
    assert!(advance_selected_hang_script(&mut world, &data, &scene, false).is_err());
    assert_eq!(
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
}
