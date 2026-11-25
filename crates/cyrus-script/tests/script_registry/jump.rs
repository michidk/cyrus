use super::*;

pub(super) fn script_jump_fixture(
    group: u16,
    edit_saved: impl FnOnce(&mut serde_json::Value),
) -> (ScriptWorld, Scripts, cyrus_assets::animation::Animations) {
    movement_fixture(
        group,
        false,
        true,
        |scripts| {
            scripts.definition.functions.push(Function {
                name: Native::Jump,
                param_count: 0,
                kind: CallKind::Function,
            });
            let function_id = scripts.definition.functions.len() as u16 - 1;
            scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
                (0, assign_call(1, function_id, Native::Jump, &[])),
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
        },
        |saved| {
            let control = &mut saved["actors"][0]["control"];
            control["stable_position_valid"] = true.into();
            control["fall_timer"] = 17.into();
            control["sliding_distance"] = 99.into();
            control["motion_vector"] = serde_json::json!([11, 22, 33]);
            edit_saved(saved);
        },
    )
}

#[test]
fn script_jump_completes_immediately_and_uses_its_distinct_group_rows() {
    use cyrus_script::script::{ScriptInputs, WorldSnapshotSources};
    for (group, torch, holding, selector, queued, request, heading_write) in [
        (0, 0, 0, 0, JumpMotion::STANDING, 32, false),
        (0, 1, 0, 0, JumpMotion::STANDING, 33, false),
        (0, 1, 1, 0, JumpMotion::STANDING, 111, false),
        (18, 0, 0, 0, JumpMotion::RUNNING, 34, false),
        (30, 0, 0, 0, JumpMotion::RUNNING, 35, false),
        (19, 0, 0, 0, JumpMotion::BACKWARD, 49, true),
        (23, 0, 0, 0, JumpMotion::BACKWARD, 50, true),
        (31, 0, 0, 0, JumpMotion::RUNNING, 34, true),
        (19, 1, 1, 1, JumpMotion::ROPE, 85, false),
    ] {
        let (mut world, scripts, data) = script_jump_fixture(group, |saved| {
            saved["actors"][0]["control"]["torch_burning"] = torch.into();
            saved["actors"][0]["control"]["jump_selector"] = selector.into();
            saved["actors"][0]["attributes"][110] = holding.into();
        });
        let id = world.id(0).unwrap();
        let origin = world.spatial(id).unwrap().local;
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
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
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables()[0],
            1,
            "Jump returns zero and the next instruction runs without waiting"
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.queued_motion, control.motion_direction),
            (queued, MotionDirection::FORWARD)
        );
        assert!(!control.stable_position_valid);
        assert_eq!(
            (
                control.fall_timer,
                control.sliding_distance,
                control.fall_origin_y
            ),
            (0, 0, origin.position[1])
        );
        assert_eq!(
            control.motion_vector,
            [11, if heading_write { origin.angles[1] } else { 22 }, 33]
        );
        let collision = Default::default();
        let saved = serde_json::to_value(
            world
                .snapshot(&WorldSnapshotSources {
                    identity_memo: None,
                    effects: None,
                    player: None,
                    scripts: &scripts,
                    animations: &data,
                    collision: &collision,
                })
                .unwrap(),
        )
        .unwrap();
        let playback = &saved["actors"][0]["animation"]["playback"];
        assert_eq!(
            playback["pending"][0]
                .as_u64()
                .or_else(|| playback["current"][0].as_u64()),
            Some(request),
            "{playback}",
        );
    }
}

#[test]
fn script_jump_refusals_return_zero_without_changing_jump_state() {
    use cyrus_script::script::ScriptInputs;
    for (group, field, value) in [
        (114, "queued_motion", 0),
        (116, "queued_motion", 0),
        (130, "queued_motion", 0),
        (131, "queued_motion", 0),
        (0, "queued_motion", 1),
        (0, "falling_state", 1),
        (0, "sliding_state", 1),
        (0, "living_state", 1),
    ] {
        let (mut world, _, _) = script_jump_fixture(group, |saved| {
            saved["actors"][0]["control"][field] = value.into();
        });
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
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
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
        let control = world.actors()[0].control_state().unwrap();
        assert!(control.stable_position_valid);
        assert_eq!(
            (
                control.fall_timer,
                control.sliding_distance,
                control.motion_direction
            ),
            (17, 99, MotionDirection::NONE)
        );
    }
    for surface in [2, 4] {
        let (mut world, _, _) = script_jump_fixture(0, |_| {});
        let id = world.id(0).unwrap();
        world
            .set_actor_surface(id, Surface::from_byte(surface))
            .unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
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
        assert!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .stable_position_valid
        );
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
    }
}

#[test]
fn script_jump_from_a_nonselected_hang_returns_without_initializing_motion() {
    use cyrus_script::script::ScriptInputs;
    let (mut world, _, _) = script_jump_fixture(0, |saved| {
        saved["actors"][0]["control"]["living_state"] = 15.into();
    });
    let id = world.id(0).unwrap();
    let before = world.inspect(0, 1);
    let mut expected = before["actors"][0]["control"].clone();
    expected["stable_position_valid"] = false.into();
    world
        .advance(
            id,
            ScriptStage::OrdinaryMain,
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
    assert!(
        world.actors()[0].failure().is_none(),
        "{:?}",
        world.actors()[0].failure()
    );
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
    let after = world.inspect(0, 1);
    assert_eq!(after["actors"][0]["control"], expected);
    assert_eq!(
        after["actors"][0]["animation"],
        before["actors"][0]["animation"]
    );
    assert_eq!(
        after["actors"][0]["spatial"],
        before["actors"][0]["spatial"]
    );
}

#[test]
fn selected_script_jump_group_nineteen_sets_cooldown_and_heading_write() {
    use cyrus_script::script::{
        ActivationInputs, PlayerCollisionSource, ScriptActorSource, ScriptInputs,
    };
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        let function_id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::Jump,
            param_count: 0,
            kind: CallKind::Function,
        });
        let cyrus = scripts.actors.get_mut("CYRUS").unwrap();
        cyrus
            .program
            .steps
            .insert(0, assign_call(1, function_id, Native::Jump, &[]));
    });
    let player_index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let _player = world.id(player_index).unwrap();
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let saved = movement_snapshot(&world, &scripts, &data);
    let mut saved = saved;
    saved["actors"][player_index]["animation"] = serde_json::Value::Null;
    world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        },
        cyrus_script::script::WorldRestoreContext {
            generation: world.generation() + 1,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let player = world.id(player_index).unwrap();
    world
        .bind_animation(player, &data, AnimationGroup::RUN_BACKWARD)
        .unwrap();
    world
        .advance(
            player,
            ScriptStage::Animation,
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
    world.set_actor_surface(player, Surface::UNKNOWN).unwrap();
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
    let before_heading = world.spatial(player).unwrap().local.angles[1];
    let actor_ids = world
        .actors()
        .iter()
        .enumerate()
        .map(|(index, _)| world.id(index).unwrap())
        .collect::<Vec<_>>();
    world
        .advance(
            player,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                activation: Some(&activation),
                nominal_ticks: Some(0),
                fine_ticks: Some(0),
                frame_scale: Some(256),
                player_collision: Some(PlayerCollisionSource {
                    scene: &scene,
                    animations: &data,
                    view: [10, 30],
                    actors: &actor_ids,
                }),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    let control = world.actors()[player_index].control_state().unwrap();
    assert_eq!(control.queued_motion, JumpMotion::BACKWARD);
    assert_eq!(control.motion_vector[1], before_heading);
    assert_eq!(
        serde_json::to_value(world.globals()).unwrap()["controller_prelude"]["conditional_ticks"],
        6
    );
}

#[test]
fn selected_script_jump_hang_release_clears_support_without_starting_jump_motion() {
    use cyrus_script::script::{
        ActivationInputs, PlayerCollisionSource, ScriptActorSource, ScriptInputs,
    };
    let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
        let function_id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::Jump,
            param_count: 0,
            kind: CallKind::Function,
        });
        let cyrus = scripts.actors.get_mut("CYRUS").unwrap();
        cyrus
            .program
            .steps
            .insert(0, assign_call(1, function_id, Native::Jump, &[]));
    });
    let player_index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let _player = world.id(player_index).unwrap();
    data.metadata["markers"] = serde_json::json!([]);
    data.metadata["walk_node_maps"] = serde_json::json!([]);
    let saved = movement_snapshot(&world, &scripts, &data);
    let mut saved = saved;
    saved["actors"][player_index]["animation"] = serde_json::Value::Null;
    saved["actors"][player_index]["control"]["living_state"] = 15.into();
    saved["actors"][player_index]["control"]["stable_position_valid"] = true.into();
    saved["actors"][player_index]["model"]["ledge_support"] =
        serde_json::json!([world.generation(), 0]);
    world = ScriptWorld::from_snapshot(
        serde_json::from_value(saved).unwrap(),
        &cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &Default::default(),
        },
        cyrus_script::script::WorldRestoreContext {
            generation: world.generation() + 1,
            globals: std::mem::take(world.globals_mut()),
            samples: &BTreeMap::new(),
        },
    )
    .unwrap();
    let player = world.id(player_index).unwrap();
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
            "group_index":56,"anim_id":1,"anim_type":0,
            "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]
        }));
    world
        .bind_animation(player, &data, AnimationGroup::PANIC)
        .unwrap();
    world
        .advance(
            player,
            ScriptStage::Animation,
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
    world.set_actor_surface(player, Surface::UNKNOWN).unwrap();
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
    let before_position = world.spatial(player).unwrap().local.position;
    let actor_ids = world
        .actors()
        .iter()
        .enumerate()
        .map(|(index, _)| world.id(index).unwrap())
        .collect::<Vec<_>>();
    world
        .advance(
            player,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                activation: Some(&activation),
                nominal_ticks: Some(0),
                fine_ticks: Some(0),
                frame_scale: Some(256),
                player_collision: Some(PlayerCollisionSource {
                    scene: &scene,
                    animations: &data,
                    view: [10, 30],
                    actors: &actor_ids,
                }),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    let control = world.actors()[player_index].control_state().unwrap();
    assert!(!control.stable_position_valid);
    assert_eq!(control.queued_motion, JumpMotion::NONE);
    assert_eq!(control.falling_state, JumpMotion::NONE);
    assert_eq!(control.living_state, LivingState::ORDINARY);
    assert_eq!(control.action_transition_counter, 6);
    assert_eq!(
        world.spatial(player).unwrap().local.position,
        before_position
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    assert!(saved["actors"][player_index]["model"]["ledge_support"].is_null());
    assert_eq!(
        world.inspect(player_index, 1)["actors"][0]["animation"]["current_group"],
        AnimationGroup::FALL.index()
    );
}

fn public_jump_classification_fixture(
    group: u16,
    sound: bool,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    let function_id = u16::try_from(scripts.definition.functions.len()).unwrap();
    scripts.definition.functions.push(Function {
        name: cyrus_soup::Native::Jump,
        param_count: 0,
        kind: CallKind::Function,
    });
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (0, assign_call(1, function_id, Native::Jump, &[])),
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
    let groups = [
        0, 18, 19, 23, 30, 31, 32, 33, 34, 35, 36, 37, 49, 50, 85, 111,
    ]
    .map(|index| {
        let mut commands = vec![];
        if sound && index == 32 {
            commands.push(serde_json::json!({"opcode":4,"sound_param":9,"volume_shift":0}));
        }
        commands.extend([
            serde_json::json!({"opcode":0,"frame_reference":0}),
            serde_json::json!({"opcode":2,"value":0}),
        ]);
        serde_json::json!({"group_index":index,"anim_id":1,"anim_type":0,"commands":commands})
    });
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table","animation_groups":groups}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, AnimationGroup::from_index(group))
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
    (world, data)
}

fn invoke_public_script_jump(world: &mut ScriptWorld, fail_sound: bool) -> anyhow::Result<()> {
    let id = world.id(0).unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(cyrus_soup::Thread::Main),
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                shell_execution: Some(false),
                ..Default::default()
            },
            Some(&mut |_, _| {
                anyhow::ensure!(!fail_sound, "public Jump sound rejected");
                Ok(())
            }),
            None,
            100,
        )
        .map(|_| ())
}

#[test]
fn ordinary_jump_publishes_classification_after_admission_and_retains_it_on_refusal() {
    use cyrus_script::script::ObstacleEnvironment;
    for (group, motion, queued) in [(0, 8, 1), (18, 8, 8), (18, 9, 9), (30, 9, 9)] {
        let (mut world, data) = public_jump_classification_fixture(group, false);
        let id = world.id(0).unwrap();
        let environment = ObstacleEnvironment {
            selected_player: None,
            auto_grab: false,
            item_system_ready: false,
            shell_execution: false,
            ticks: 0,
        };
        assert!(
            !world
                .begin_ordinary_jump(
                    id,
                    &data,
                    motion,
                    true,
                    environment,
                    None,
                    &mut |_, _| panic!("refusal emitted sound")
                )
                .unwrap()
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
        assert!(
            world
                .begin_ordinary_jump(id, &data, motion, false, environment, None, &mut |_, _| Ok(
                    ()
                ))
                .unwrap()
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.queued_motion.byte(), control.motion_classification),
            (queued, MotionDirection::FORWARD)
        );
        assert!(
            !world
                .begin_ordinary_jump(
                    id,
                    &data,
                    motion,
                    false,
                    environment,
                    None,
                    &mut |_, _| panic!("queued refusal emitted sound")
                )
                .unwrap()
        );
        invoke_public_script_jump(&mut world, true).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::FORWARD
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
    }
}

#[test]
fn script_jump_returns_before_landing_with_receiver_classification_two() {
    for (group, queued) in [(0, 1), (18, 8), (19, 3), (23, 3), (30, 8), (31, 8)] {
        let (mut world, _) = public_jump_classification_fixture(group, false);
        invoke_public_script_jump(&mut world, false).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.queued_motion.byte(), control.motion_classification),
            (queued, MotionDirection::FORWARD)
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
    }
    let (mut world, _) = public_jump_classification_fixture(0, true);
    let id = world.id(0).unwrap();
    world.set_actor_surface(id, Surface::DEEPWATER).unwrap();
    invoke_public_script_jump(&mut world, true).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 1);
    let control = world.actors()[0].control_state().unwrap();
    assert_eq!(
        (control.queued_motion.byte(), control.motion_classification),
        (0, MotionDirection::NONE)
    );
}

#[test]
fn normal_and_script_jump_initialization_failure_precedes_classification_write() {
    use cyrus_script::script::ObstacleEnvironment;
    for script in [false, true] {
        let (mut world, data) = public_jump_classification_fixture(0, true);
        let id = world.id(0).unwrap();
        let error = if script {
            invoke_public_script_jump(&mut world, true).unwrap_err()
        } else {
            world
                .begin_ordinary_jump(
                    id,
                    &data,
                    8,
                    false,
                    ObstacleEnvironment {
                        selected_player: None,
                        auto_grab: false,
                        item_system_ready: false,
                        shell_execution: false,
                        ticks: 0,
                    },
                    None,
                    &mut |_, _| anyhow::bail!("public Jump sound rejected"),
                )
                .unwrap_err()
        };
        assert!(
            format!("{error:#}").contains("public Jump sound rejected"),
            "{error:#}"
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            (control.queued_motion.byte(), control.motion_classification),
            (1, MotionDirection::NONE)
        );
        assert!(world.actors()[0].animation_failure().is_some());
        assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 4);
        assert!(invoke_public_script_jump(&mut world, false).is_err());
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .motion_classification,
            MotionDirection::NONE
        );
    }
}

fn queued_jump_lifetime_fixture(
    queued: u8,
    sound_group: Option<u16>,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
    }
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
    let function = scripts.definition.functions.len() as u16;
    scripts.definition.functions.push(Function {
        name: cyrus_soup::Native::Jump,
        param_count: 0,
        kind: CallKind::Function,
    });
    let reference = scripts.definition.references.len() as u16;
    scripts
        .definition
        .references
        .push(cyrus_soup::Property::JumpFlag);
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = BTreeMap::from([
        (0, assign_call(1, function, Native::Jump, &[])),
        (
            1,
            Step {
                next: 2,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference,
                    },
                    formula: Formula {
                        first: Value::Integer {
                            value: i32::from(queued),
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
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let groups = [0, 32, 39, 40, 44, 48, 56, 58, 65, 67].map(|group| {
        let mut commands = vec![];
        if sound_group == Some(group) {
            commands.push(serde_json::json!({"opcode":4,"sound_param":9,"volume_shift":0}));
        }
        commands.extend([
            serde_json::json!({"opcode":0,"frame_reference":group}),
            serde_json::json!({"opcode":1}),
        ]);
        serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":commands})
    });
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table","ravc_count":0,"animation_groups":groups}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, cyrus_soup::AnimationGroup::JUMP_START)
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            cyrus_script::script::ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            100,
        )
        .unwrap();
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
    invoke_public_script_jump(&mut world, false).unwrap();
    world.snapshot_spatial(id).unwrap();
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::FORWARD
    );
    (world, data)
}

fn queued_jump_lifetime_sample(world: &ScriptWorld) -> (u8, u8, u8, u8) {
    let control = world.actors()[0].control_state().unwrap();
    (
        control.fall_timer,
        control.queued_motion.byte(),
        control.falling_state.byte(),
        control.motion_classification.byte(),
    )
}

#[test]
fn public_jump_continuation_and_landing_retain_classification_two() {
    use cyrus_script::script::{
        ObstacleEnvironment, OrdinaryGroundQuery, OrdinaryLandingDecision, OrdinaryLandingSettings,
    };
    let (mut world, data) = queued_jump_lifetime_fixture(1, None);
    let id = world.id(0).unwrap();
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
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    for expected in [(8, 1, 0, 2), (16, 1, 0, 2), (24, 1, 0, 2), (32, 0, 1, 2)] {
        assert!(
            world
                .resolve_ordinary_queued_motion(id, query(), environment, 8, 0, &mut |_, _| Ok(()))
                .unwrap()
        );
        assert_eq!(queued_jump_lifetime_sample(&world), expected);
    }
    assert!(
        !world
            .resolve_ordinary_queued_motion(id, query(), environment, 8, 0, &mut |_, _| panic!(
                "unqueued poll emitted sound"
            ))
            .unwrap()
    );
    assert_eq!(queued_jump_lifetime_sample(&world), (32, 0, 1, 2));
    assert!(
        world
            .continue_ordinary_falling(id, &data, environment, 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(queued_jump_lifetime_sample(&world), (24, 0, 1, 2));
    assert_eq!(
        world
            .resolve_ordinary_landing_damage(
                id,
                &data,
                environment,
                OrdinaryLandingSettings {
                    death_height: 1000,
                    hurt_height: 1000,
                    hurt_zap: 10,
                },
                None,
                &mut |_, _| Ok(())
            )
            .unwrap(),
        OrdinaryLandingDecision::Ordinary
    );
    world
        .complete_ordinary_landing(id, &data, environment, None, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(queued_jump_lifetime_sample(&world), (24, 0, 0, 2));
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .motion_classification,
        MotionDirection::NONE
    );
}

#[test]
fn public_queued_request_failure_precedes_vertical_and_preserves_classification() {
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    let (mut world, data) = queued_jump_lifetime_fixture(1, Some(39));
    let id = world.id(0).unwrap();
    let (_, _, models) = static_model_fixture(cache());
    let scene = ordinary_collision_scene(world.world, models);
    let prepared = world
        .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
        .unwrap();
    let query = OrdinaryGroundQuery {
        scene: &scene,
        animations: &data,
        prepared: &prepared,
        distance: 50,
        sidedness: true,
    };
    let environment = ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    };
    let before = world.spatial(id).unwrap().local.position;
    let error = world
        .resolve_ordinary_queued_motion(id, query, environment, 30, 0, &mut |_, _| {
            anyhow::bail!("continuation sound rejected")
        })
        .unwrap_err();
    assert!(format!("{error:#}").contains("continuation sound rejected"));
    assert_eq!(queued_jump_lifetime_sample(&world), (0, 1, 0, 2));
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    assert!(world.actors()[0].animation_failure().is_some());
}

#[test]
fn public_script_high_bit_peak_clears_queued_only_after_transition_request() {
    use cyrus_script::script::{ObstacleEnvironment, OrdinaryGroundQuery};
    for fail in [false, true] {
        let (mut world, data) = queued_jump_lifetime_fixture(129, Some(58));
        let id = world.id(0).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [10, 30], &[])
            .unwrap();
        let query = OrdinaryGroundQuery {
            scene: &scene,
            animations: &data,
            prepared: &prepared,
            distance: 50,
            sidedness: true,
        };
        let environment = ObstacleEnvironment {
            selected_player: None,
            auto_grab: false,
            item_system_ready: false,
            shell_execution: false,
            ticks: 0,
        };
        let mut sounds = 0;
        let result =
            world.resolve_ordinary_queued_motion(id, query, environment, 30, 0, &mut |_, _| {
                sounds += 1;
                anyhow::ensure!(!fail, "peak sound rejected");
                Ok(())
            });
        assert_eq!(sounds, 1);
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("peak sound rejected"));
            assert_eq!(queued_jump_lifetime_sample(&world), (30, 129, 129, 2));
            assert!(
                world
                    .complete_ordinary_landing(id, &data, environment, None, &mut |_, _| Ok(()))
                    .is_err()
            );
            assert_eq!(queued_jump_lifetime_sample(&world), (30, 129, 129, 2));
        } else {
            assert!(result.unwrap());
            assert_eq!(queued_jump_lifetime_sample(&world), (30, 0, 129, 2));
            world
                .complete_ordinary_landing(id, &data, environment, None, &mut |_, _| Ok(()))
                .unwrap();
            assert_eq!(queued_jump_lifetime_sample(&world), (30, 0, 0, 2));
        }
    }
}

#[test]
fn ordinary_update_carries_backward_jump_before_scripts_at_retained_heading() {
    use cyrus_script::script::{
        OrdinaryInputs, ScriptHud, ScriptInputs, WorldRestoreContext, WorldSnapshotSources,
    };
    for (queued, suppressed) in [(3, false), (131, false), (3, true)] {
        let (mut world, scripts, data) = movement_fixture(
            49,
            suppressed,
            false,
            |scripts| {
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                    instance.active = 1;
                }
                // Ground/collision stages are deliberately separate from this displacement contract.
                scripts
                    .actors
                    .get_mut("LOGIC")
                    .unwrap()
                    .attributes
                    .extend([(Attribute::Collide, 4), (Attribute::JumpTime, 8)]);
                scripts.updates.fall_landing = Some(cyrus_assets::script::FallLanding {
                    death_height: 568,
                    hurt_height: 300,
                    hurt_zap: 10,
                    bounce_height: None,
                });
            },
            |saved| {
                saved["actors"][0]["control"]["queued_motion"] = queued.into();
                saved["actors"][0]["control"]["motion_vector"] = serde_json::json!([0, 512, 0]);
                saved["actors"][0]["control"]["movement_flags"] = 0.into();
            },
        );
        // Use the public ordinary loop without ground response. Its contextual admission error
        // follows the displacement and prevents this fixture from claiming collision coverage.
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["attributes"][1] = 0.into();
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
                generation: 3,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let id = world.id(0).unwrap();
        let before = world.spatial(id).unwrap().local;
        let hud = ScriptHud::default();
        world
            .update_ordinary(
                &data,
                [0, 0],
                OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(128),
                        hud: Some(&hud),
                        shell_execution: Some(false),
                        ..Default::default()
                    },
                    scene: None,
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        let expected = if suppressed {
            before.position
        } else {
            let heading = if queued == 3 {
                1536
            } else {
                wrap(before.angles[1].wrapping_add(1024))
            };
            cyrus_world::transform::translate_forward(before.position, (heading & 2047) as u16, 384)
                .unwrap()
        };
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            expected,
            "{:?}",
            world.actors()[0].failure()
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.motion_direction, MotionDirection::BACKWARD);
        assert_eq!(control.move_counter & 0xc0, 0x40);
        // Without collision attribute bit 0x04 nothing resolves the queued motion: the update
        // completes and the motion stays (preservation player.md, "Stable ordinary position").
        assert!(world.actors()[0].failure().is_none());
        assert_eq!(control.queued_motion, JumpMotion::from_byte(queued));
    }
}
