use super::*;

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
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes.extend([
            (Attribute::Collide, 1),
            (Attribute::Combat, capable),
            (Attribute::ReservedCollide, 7),
            (Attribute::AttackSingle, 54),
            (Attribute::AttackTriple, 56),
            (Attribute::AttackDisarm, 64),
            (Attribute::DefendLow, defenses[0]),
            (Attribute::DefendRight, defenses[1]),
            (Attribute::DefendLeft, defenses[2]),
            (Attribute::DefendHigh, defenses[3]),
        ]);
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
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
            (
                combat.target,
                combat.flags.bits(),
                combat.action,
                combat.cooldown
            ),
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
            Some(cyrus_script::script::decision::DefenceCandidate {
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
        world
            .maintain_map_combat(
                first,
                1,
                false,
                cyrus_script::script::ScriptInputs {
                    shell_execution: Some(true),
                    ..Default::default()
                },
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 1);
        world
            .maintain_map_combat(
                first,
                1,
                false,
                Default::default(),
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(world.actors()[0].attributes()[Attribute::Collide], 7);
        world
            .send_combat_message(second, first, CombatMessageCode::HIT)
            .unwrap();
        let error = world
            .maintain_map_combat(
                first,
                1,
                false,
                Default::default(),
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("hit message actor 1 combat unavailable"));
        assert_eq!(world.consume_combat_message(first).unwrap(), None);
        let stale = cyrus_script::script::ScriptActorId {
            generation: 2,
            index: 0,
        };
        assert!(
            world
                .send_combat_message(second, stale, CombatMessageCode::from(1))
                .is_err()
        );
        world
            .send_combat_message(stale, stale, CombatMessageCode::NONE)
            .unwrap();
    }
}

#[test]
fn combat_entry_and_weapon_cleanup_preserve_lifecycle_motion_and_selected_effects() {
    use cyrus_script::script::{
        CombatEntryInputs, CombatTauntPlayback, DialogueLine, WeaponInputs,
    };
    struct NoTaunt;
    impl CombatTauntPlayback for NoTaunt {
        fn remembered_sample(&self, _: usize) -> anyhow::Result<Option<u8>> {
            Ok(None)
        }
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
                let actor = scripts.actors.get_mut("LOGIC").unwrap();
                actor.attributes[Attribute::Combat] = 1;
                actor.attributes[Attribute::Weapon] = authored_weapon;
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
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
                world.set_actor_surface(id, Surface::WOOD).unwrap();
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
                        control.ai_type.byte(),
                        control.living_state,
                        control.weapon_drawn
                    ),
                    (
                        6,
                        if drawing {
                            LivingState::DRAWING_SWORD
                        } else {
                            LivingState::ORDINARY
                        },
                        !drawing
                    )
                );
                assert_eq!(control.script_control, if selected { 0 } else { 2 });
                assert_eq!(control.combat.as_ref().unwrap().flags.bits(), 4);
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
                        control.ai_type.byte(),
                        control.living_state,
                        control.motion_direction,
                        control.weapon_drawn
                    ),
                    (6, LivingState::ORDINARY, MotionDirection::NONE, false)
                );
                assert_eq!(
                    control.combat.as_ref().unwrap().flags.bits(),
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
                let stale = cyrus_script::script::ScriptActorId {
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
    use cyrus_script::script::{CombatEntryInputs, ScriptInputs, ScriptStage, WeaponInputs};
    for (expression, derived, selected_receiver) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (false, true, true),
    ] {
        let call = Call {
            function_id: 4,
            function: Native::BeginCombat,
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
            name: Native::BeginCombat,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Combat] = 1;
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
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
        let hud = cyrus_script::script::ScriptHud::default();
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
            frame_scale: Some(0),
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
                    control.ai_type.byte(),
                    control.script_control,
                    control.weapon_drawn
                ),
                (0, 0, false)
            );
            assert_eq!(control.combat.as_ref().unwrap().flags.bits(), 0);
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
                control.ai_type.byte(),
                control.script_control,
                control.weapon_drawn
            ),
            (6, 2, true),
            "derived={derived} expression={expression} result={result:?}"
        );
        assert_eq!(control.combat.as_ref().unwrap().flags.bits(), 4);
        assert_eq!(
            world.actors()[1].control_state().unwrap().ai_type,
            AiType::NONE
        );
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
    use cyrus_script::script::{
        CombatEntryInputs, OrdinaryInputs, ScriptHud, ScriptInputs, WeaponInputs,
    };
    for shell_context in [Some(false), None] {
        let mut scripts = wait_cache(vec![
            Operation::Call {
                call: Call {
                    function_id: 4,
                    function: Native::BeginCombat,
                    target: Object::Me,
                    kind: CallKind::Function,
                    arguments: vec![],
                },
            },
            set_flag(42),
            Operation::End { target: 0 },
        ]);
        scripts.definition.functions.push(Function {
            name: Native::BeginCombat,
            param_count: 0,
            kind: CallKind::Function,
        });
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        for (index, value) in [
            (Attribute::Collide, 4),
            (Attribute::Weight, 64),
            (Attribute::Combat, 1),
            (Attribute::Weapon, 7),
            (Attribute::ReservedWeight, 64),
            (Attribute::ReservedCollide, 4),
        ] {
            actor.attributes[index] = value;
        }
        add_standalone_receivers(&mut scripts);
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
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
                    "ravc_count":0, "mpsz_bounds_primary":0, "raex":{"unknown_12":0,"v_vertex":0,"v_size":0},
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
                    control.ai_type.byte(),
                    control.living_state,
                    control.combat.as_ref().unwrap().flags.bits()
                ),
                (6, LivingState::DRAWING_SWORD, 4)
            );
            assert_eq!(
                world
                    .spatial(world.id(index).unwrap())
                    .unwrap()
                    .local
                    .position,
                [2560, 5120, 7680]
            );
            assert_eq!(
                world.actor_surface(world.id(index).unwrap()).unwrap(),
                Surface::UNKNOWN
            );
            assert_eq!(actor.attributes()[Attribute::Weight], 64);
        }
        assert_eq!(world.flags(), [42]);
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
                    cyrus_script::script::ScriptActorId {
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
                serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
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
                serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
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
    let mut snapshot = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for index in [0, 1] {
        snapshot["actors"][index]["control"]["combat"] =
            serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
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
            cyrus_script::script::OrdinaryCombatSample {
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
fn ordinary_combat_root_preserves_waiting_branch_after_coordination_promotes_receiver() {
    use cyrus_script::script::ScriptInputs;
    let (mut world, _, _) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["death_selector"] = 6.into();
        saved["actors"][0]["attributes"][121] = 1.into();
        let own = &mut saved["actors"][0]["control"]["combat"];
        own["attacker_selection_state"] = 0.into();
        own["action"] = 7.into();
        for index in [2, 3] {
            saved["actors"][index]["control"]["combat"] = serde_json::Value::Null;
        }
        saved["actors"][1]["control"]["combat"] =
            serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
    });
    world.ordinary_actors([0, 0]).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let mut expected = cyrus_game::random::EngineRandom::from_state(42);
    expected.draw();
    world
        .run_ordinary_combat_decision(
            world.id(0).unwrap(),
            world.id(4).unwrap(),
            ScriptInputs {
                combat_clock_sample: Some(0x8000_0001),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| anyhow::bail!("unexpected combat sound"),
        )
        .unwrap();
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(combat.attacker_selection_state, 1);
    assert_eq!(combat.action, 7);
    assert_eq!(random.state(), expected.state());
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn ordinary_combat_root_retains_prefix_writes_before_spacing_service_failure() {
    let (mut world, _, _) = spacing_fixture(|saved| {
        saved["actors"][0]["control"]["death_selector"] = 6.into();
        saved["actors"][0]["attributes"][121] = 1.into();
        saved["actors"][1]["control"]["combat"] =
            serde_json::to_value(cyrus_script::script::CombatControlState::default()).unwrap();
        saved["actors"][0]["control"]["combat"]["decision"]["aggression"] = 50.into();
    });
    world.ordinary_actors([0, 0]).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let error = world
        .run_ordinary_combat_decision(
            world.id(0).unwrap(),
            world.id(4).unwrap(),
            cyrus_script::script::ScriptInputs {
                combat_clock_sample: Some(77),
                ..Default::default()
            },
            &mut random,
            &mut |_, _| anyhow::bail!("unexpected combat sound"),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("fight spacing frame_scale unavailable"),
        "{error:#}"
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .close_distance_passes,
        1
    );
    assert_eq!(random.state(), 42);
}
