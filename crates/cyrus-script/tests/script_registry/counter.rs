use super::*;

#[test]
fn current_combat_counter_query_uses_retained_slot_motion_and_idle_gate() {
    use cyrus_script::script::{WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
        (Attribute::Combat, 1),
        (Attribute::AttackThrust, 1),
        (Attribute::AttackLow, 2),
    ]);
    scripts.instances[0].object_type = PlacementType::MODEL;
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
        identity_memo: None,
        effects: None,
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

pub(super) fn counter_fixture(edit: impl FnOnce(&mut serde_json::Value)) -> ScriptWorld {
    counter_fixture_sound(false, edit)
}

fn counter_fixture_sound(sound: bool, edit: impl FnOnce(&mut serde_json::Value)) -> ScriptWorld {
    counter_fixture_state(sound, false, edit)
}

pub(super) fn counter_fixture_state(
    sound: bool,
    initialize_player: bool,
    edit: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    counter_fixture_timing(sound, initialize_player, [2, 1], edit)
}

pub(super) fn counter_fixture_timing(
    sound: bool,
    initialize_player: bool,
    timing: [i32; 2],
    edit: impl FnOnce(&mut serde_json::Value),
) -> ScriptWorld {
    let (mut world, scripts, mut data) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            if initialize_player {
                add_standalone_receivers(scripts);
            }
            let motion: combat::Motion = serde_json::from_value(serde_json::json!({
                "section":1,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                "first_collide_frame":timing[0],"collide_duration":timing[1],"first_defend_frame":5,"defense":1
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
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        action: if index == 0 { -1 } else { 2 },
                        flags: CombatFlags::from_bits(if index == 0 { 0xa1 } else { 0 }),
                        attack_indices: [Some(0); 9],
                        defense_indices: [Some(0); 4],
                        retained_action_indices: [-1, 0],
                        decision: cyrus_script::script::CombatDecision {
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
            .find(|group| group["group_index"] == AnimationGroup::RUN_BACKWARD.index())
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
            &cyrus_script::script::WorldSnapshotSources {
                identity_memo: None,
                effects: None,
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &collision,
            },
            cyrus_script::script::WorldRestoreContext {
                generation: 3,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        for index in 0..2 {
            let id = world.id(index).unwrap();
            world
                .bind_animation(id, &data, AnimationGroup::RUN)
                .unwrap();
            world
                .advance(
                    id,
                    cyrus_script::script::ScriptStage::Animation,
                    cyrus_script::script::ScriptInputs {
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
    if initialize_player {
        let mut player = data.metadata["actors"][0].clone();
        player["script_name"] = "CYRUS".into();
        data.metadata["actors"].as_array_mut().unwrap().push(player);
        world
            .initialize_player_animation(world.id(2).unwrap(), &data, 0, &mut |_| Ok(()))
            .unwrap();
    }
    world
}

#[test]
fn live_counter_starts_receiver_animation_then_marks_target_without_a_task() {
    use cyrus_script::script::decision::CounterAttack;
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
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
            own.flags.bits(),
            own.action,
            own.retained_action_indices[0],
            own.decision.attack_state,
            own.decision.aggression
        ),
        (0xa3, 9, 0, ExecutorState::ATTACKING, 40)
    );
    assert_eq!(target.flags.bits(), 0x10);
    assert_eq!(
        world.inspect(0, 2)["actors"][0]["animation"]["current_group"],
        AnimationGroup::RUN_BACKWARD.index()
    );
    assert_eq!(random.state(), 1);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn live_counter_preserves_intent_pending_and_action_before_executor_failure() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
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
            own.flags.bits(),
            own.retained_action_indices[0],
            own.action,
            own.decision.attack_state,
            own.decision.aggression
        ),
        (0xa3, 0, 9, ExecutorState::IDLE, 50)
    );
    assert_eq!(
        world.actors()[1]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .flags
            .bits(),
        0
    );
}

#[test]
fn live_counter_admission_bypasses_unavailable_timing_and_keeps_rng() {
    use cyrus_script::script::decision::CounterAttack;
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
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
    use cyrus_script::script::decision::CounterAttack;
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
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
                .flags
                .bits(),
            if draws == 2 { 16 } else { 0 }
        );
    }
}

#[test]
fn live_counter_reads_command_cursor_and_runs_direct_sidestep_primitives() {
    use cyrus_script::script::decision::{CounterAttack, Sidestep};
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs};
    for (slot, tick, step, position) in [
        (3, 0, Sidestep::Right, [256, 0, 0]),
        (4, 1, Sidestep::Left, [-256, 0, 0]),
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
            (own.flags.bits(), own.action, own.retained_action_indices[0]),
            (if slot == 0 { 0xa1 } else { 0xa5 }, -1, -1)
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            0
        );
    }
}

#[test]
fn live_counter_sound_failure_retains_start_action_but_does_not_mark_target() {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptHud, ScriptInputs, ScriptSound};
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
            (own.flags.bits(), own.action, own.retained_action_indices[0]),
            (0xa3, 9, 0)
        );
        assert_eq!(
            (own.decision.attack_state, own.decision.aggression),
            if fail {
                (ExecutorState::IDLE, 50)
            } else {
                (ExecutorState::ATTACKING, 40)
            }
        );
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            if fail { 0 } else { 16 }
        );
        assert_eq!(
            world.inspect(0, 1)["actors"][0]["animation"]["current_group"],
            AnimationGroup::RUN_BACKWARD.index()
        );
    }
}
