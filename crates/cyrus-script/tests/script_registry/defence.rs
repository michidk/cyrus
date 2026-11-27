use super::*;

#[test]
fn live_defence_executor_uses_following_motion_loop_count_and_synchronous_sound() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    #[derive(Debug)]
    struct RejectTaunt;
    impl cyrus_script::script::CombatTauntSource for RejectTaunt {
        fn with_playback(
            &self,
            _: &mut dyn FnMut(
                &mut dyn cyrus_script::script::CombatTauntPlayback,
            ) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            anyhow::bail!("injected defence taunt failure")
        }
    }
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
        (Attribute::Combat, 1),
        (Attribute::FightLungeSpeed, 17),
        (Attribute::AttackSingle, 1),
        (Attribute::Chatty, 127),
        (Attribute::NumTaunts, 1),
    ]);
    scripts.actors.get_mut("LOGIC").unwrap().combat_taunts = vec![CombatTaunt {
        trigger: TauntType::ATTACK,
        label: *b"TEST",
        animation: 0,
    }];
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
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
                        "section":motion,"kind":0,"animation_group":100+motion,
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
                "raex":{"v_vertex":-7,"v_size":100},
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
        identity_memo: None,
        effects: None,
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
    assert_eq!(constructed.contact_profile.vertex, -7);
    assert_eq!(constructed.contact_profile.radius, 25600);
    // The fixture's one melee attack contributes: near 0, far 100, midpoint 50.
    assert_eq!(constructed.spacing_ranges, [0, 100, 50]);
    assert_eq!(constructed.spacing_period, 129);
    let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
    for field in ["spacing_ranges", "spacing_period", "contact_profile"] {
        let mut incomplete = saved.clone();
        incomplete["actors"][0]["control"]["combat"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<cyrus_script::script::WorldSnapshot>(incomplete).is_err());
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
        assert_eq!(restored.contact_profile.vertex, -7);
        assert_eq!(restored.contact_profile.radius, 25600);
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
        assert_eq!(
            combat.flags.bits(),
            if fail || !expected_active { 48 } else { 1 }
        );
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
            cyrus_script::script::WeaponInputs {
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
            if expected && !fail_sound {
                ExecutorState::DEFENDING
            } else {
                ExecutorState::IDLE
            }
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
            cyrus_script::script::decision::DefenceSelectionInput {
                armed: true,
                lockout_clear: true,
                own_selected: 0,
                target_in_arc: false,
                distance: 0,
                own_window_far: 0,
                facing_alike: false,
                target_in_combat_stance: false,
                aggression: 0,
                pacing_sum: 0,
                defence_pacing: 0,
                target_selected: -1,
                target_frame: 0,
                target_near: 0,
                target_third: 0,
                own_selector: 0,
            },
            cyrus_script::script::WeaponInputs {
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
        assert_eq!(
            combat.decision.attack_state,
            if fail_sound {
                ExecutorState::IDLE
            } else {
                ExecutorState::DEFENDING
            }
        );
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
            cyrus_script::script::decision::DefenceSelectionInput {
                armed: true,
                lockout_clear: true,
                own_selected: -1,
                target_in_arc: true,
                distance: 50,
                own_window_far: 100,
                facing_alike: false,
                target_in_combat_stance: true,
                aggression: 29,
                pacing_sum: 100,
                defence_pacing: 3,
                target_selected: -1,
                target_frame: 0,
                target_near: 0,
                target_third: 0,
                own_selector: 0,
            },
            cyrus_script::script::WeaponInputs {
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
    assert_eq!(combat.flags.bits(), 49);
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
            cyrus_script::script::decision::DefenceSelectionInput {
                armed: true,
                lockout_clear: true,
                own_selected: -1,
                target_in_arc: true,
                distance: 50,
                own_window_far: 100,
                facing_alike: false,
                target_in_combat_stance: true,
                aggression: 29,
                pacing_sum: 100,
                defence_pacing: 1,
                target_selected: 0,
                target_frame: 1,
                target_near: 0,
                target_third: 0,
                own_selector: 0,
            },
            cyrus_script::script::WeaponInputs {
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
    assert_eq!(combat.decision.attack_state, ExecutorState::DEFENDING);
    assert_eq!(combat.decision.aggression, 40);
    assert_eq!(combat.decision.defence_counter, 100);
    assert_eq!(combat.flags.bits(), 1);
    assert_eq!(cues, 1);
    assert_eq!(random.state(), 238077914);
}

#[test]
fn ordinary_defence_forward_arc_uses_quantized_local_xyz_and_strict_heading() {
    use cyrus_script::script::decision::ordinary_defence_forward_arc;
    for (own, target, heading, expected) in [
        ([0; 3], [256, 0, 0], 1536, true),
        ([0; 3], [256, 0, 0], 1281, true),
        ([0; 3], [256, 0, 0], 1280, false),
        ([0; 3], [256, 0, 0], 1791, true),
        ([0; 3], [256, 0, 0], 1792, false),
        ([0; 3], [-256, 0, 0], 512, true),
        ([0; 3], [0, 0, 256], 2047, true),
        ([0; 3], [0, 0, -256], 1024, true),
        ([13, -29, 91], [0; 3], 768, true),
        ([13, -29, 91], [0; 3], 512, false),
    ] {
        assert_eq!(
            ordinary_defence_forward_arc(own, target, heading),
            expected,
            "{own:?} {target:?} {heading}"
        );
    }
}

fn run_ordinary_defence(
    world: &mut ScriptWorld,
    ticks: i32,
    random: &mut cyrus_game::random::EngineRandom,
) -> anyhow::Result<()> {
    use cyrus_script::script::{OrdinarySpacingInput, ScriptInputs, WeaponInputs};
    let id = world.id(0).unwrap();
    let selected_player = world.id(2).or_else(|_| world.id(1)).unwrap();
    let hud = cyrus_script::script::ScriptHud::default();
    world.select_ordinary_combat_defence(
        id,
        OrdinarySpacingInput {
            distance: 50,
            tick_counter: ticks,
        },
        WeaponInputs {
            selected_player,
            item_system_ready: true,
            shell_execution: false,
        },
        1,
        ScriptInputs {
            hud: Some(&hud),
            nominal_ticks: Some(1),
            ..Default::default()
        },
        random,
        &mut |_, _, _, _| anyhow::bail!("unexpected defence movement"),
        &mut |_, _| anyhow::bail!("unexpected defence sound"),
    )
}

#[test]
fn ordinary_defence_weapon_gate_precedes_unknown_shared_override() {
    for armed in [false, true] {
        let mut world = counter_fixture(|saved| {
            saved["actors"][0]["attributes"][110] = i32::from(armed).into();
            saved["actors"][0]["control"]["combat"] = serde_json::Value::Null;
        });
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        let result = run_ordinary_defence(&mut world, 0, &mut random);
        if armed {
            assert!(
                format!("{:#}", result.unwrap_err())
                    .contains("selected-player hit inhibition unavailable")
            );
        } else {
            result.unwrap();
        }
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn ordinary_defence_selected_branch_does_not_query_unused_target_or_timing() {
    for (action, selector, ticks) in [(7, 2, 0), (0, 0, 1)] {
        let mut world = counter_fixture_state(false, true, |saved| {
            let combat = &mut saved["actors"][0]["control"]["combat"];
            combat["action"] = action.into();
            combat["target"] = serde_json::Value::Null;
            combat["decision"]["move_selector"] = selector.into();
            combat["decision"]["defence_counter"] = 2.into();
            combat["decision"]["defence_pacing"] = 0.into();
        });
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        run_ordinary_defence(&mut world, ticks, &mut random).unwrap();
        assert_eq!(random.state(), 42);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .action,
            action
        );
    }
}

fn ordinary_defence_idle_fixture(edit: impl FnOnce(&mut serde_json::Value)) -> ScriptWorld {
    counter_fixture_state(false, true, |saved| {
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
        saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([0, 0, 256]);
        saved["actors"][0]["attributes"][58] = 126.into();
        saved["actors"][1]["attributes"][110] = 1.into();
        saved["actors"][0]["control"]["combat"]["flags"] = 0xa0.into();
        saved["actors"][0]["control"]["combat"]["decision"]["defence_pacing"] = 2.into();
        saved["actors"][0]["control"]["combat"]["decision"]["defence_counter"] = 99.into();
        edit(saved);
    })
}

#[test]
fn ordinary_defence_pacing_and_open_branches_skip_active_target_metadata() {
    for open in [false, true] {
        let mut world = ordinary_defence_idle_fixture(|saved| {
            saved["actors"][1]["animation"] = serde_json::Value::Null;
            if open {
                saved["actors"][1]["control"]["combat"]["action"] = 9.into();
                saved["actors"][1]["control"]["combat"]["retained_action_indices"][1] =
                    (-99).into();
                saved["actors"][1]["control"]["combat"]["decision"]["move_selector"] = (-99).into();
            } else {
                saved["actors"][1]["control"]["combat"] = serde_json::Value::Null;
            }
        });
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        run_ordinary_defence(&mut world, if open { 2 } else { 1 }, &mut random).unwrap();
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                combat.action,
                combat.flags.bits(),
                combat.decision.defence_counter
            ),
            (-1, 0xa0, 3)
        );
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn ordinary_defence_retains_counter_before_pacing_or_target_motion_error() {
    for pacing_error in [false, true] {
        let mut world = ordinary_defence_idle_fixture(|saved| {
            if pacing_error {
                saved["actors"][0]["control"]["combat"]["decision"]["defence_pacing"] = 0.into();
            } else {
                saved["actors"][1]["control"]["combat"]["decision"]["move_selector"] = (-1).into();
            }
        });
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        let error = format!(
            "{:#}",
            run_ordinary_defence(&mut world, 2, &mut random).unwrap_err()
        );
        assert!(
            error.contains(if pacing_error {
                "invalid pacing divisor"
            } else {
                "negative target motion selector"
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
            (
                combat.action,
                combat.flags.bits(),
                combat.decision.defence_counter
            ),
            (-1, 0xa0, 3)
        );
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn ordinary_defence_reads_retained_motion_and_command_cursor_without_displayed_frame() {
    let mut world = ordinary_defence_idle_fixture(|saved| {
        saved["actors"][1]["animation"]["frame"] = serde_json::Value::Null;
        saved["actors"][1]["control"]["combat"]["attack_indices"][2] = serde_json::Value::Null;
    });
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    run_ordinary_defence(&mut world, 2, &mut random).unwrap();
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            combat.action,
            combat.flags.bits(),
            combat.decision.defence_counter
        ),
        (-1, 0xa0, 3)
    );
    assert_eq!(random.state(), 42);
}

#[test]
fn ordinary_defence_collision_timing_raises_intent_before_live_reach_error() {
    for timing in [[-4, 1], [i32::MAX, 1]] {
        let mut world = counter_fixture_timing(false, true, timing, |saved| {
            saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
            saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([0, 0, 256]);
            saved["actors"][0]["attributes"][58] = 126.into();
            saved["actors"][1]["attributes"][110] = 1.into();
            saved["actors"][0]["control"]["combat"]["flags"] = 0xa0.into();
            saved["actors"][0]["control"]["combat"]["decision"]["defence_pacing"] = 2.into();
        });
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        let result = run_ordinary_defence(&mut world, 2, &mut random);
        if timing[0] == i32::MAX {
            // Timing admits intent, but every candidate exceeds the picker's timing bound.
            result.unwrap();
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("receiver bounds unavailable"), "{error}");
        }
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                combat.action,
                combat.flags.bits(),
                combat.decision.defence_counter
            ),
            (-1, 0xa1, 3)
        );
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn ordinary_defence_selected_starter_reuses_live_executor_without_target_queries() {
    let mut world = counter_fixture_state(false, true, |saved| {
        let combat = &mut saved["actors"][0]["control"]["combat"];
        combat["action"] = 0.into();
        combat["target"] = serde_json::Value::Null;
        combat["decision"]["move_selector"] = 0.into();
        combat["decision"]["defence_counter"] = 1.into();
    });
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    run_ordinary_defence(&mut world, 0, &mut random).unwrap();
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            combat.action,
            combat.retained_action_indices[1],
            combat.decision.attack_state
        ),
        (1, 1, ExecutorState::IDLE)
    );
    assert_eq!(random.state(), 42);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn ordinary_defence_inspection_retains_actual_signed_clock_phase_without_changing_state() {
    let mut world = counter_fixture_state(false, true, |saved| {
        let combat = &mut saved["actors"][0]["control"]["combat"];
        combat["action"] = 7.into();
        combat["allocation_index"] = 1.into();
        combat["decision"]["move_selector"] = 2.into();
        combat["decision"]["defence_counter"] = 2.into();
        combat["decision"]["defence_pacing"] = 2.into();
        combat["target"] = serde_json::Value::Null;
    });
    let before = world.inspect(0, 1)["actors"][0]["control"].clone();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    run_ordinary_defence(&mut world, -4, &mut random).unwrap();
    let inspected = world.inspect(0, 1);
    let phase = &inspected["actors"][0]["combat_pacing"];
    assert_eq!(phase["cached_bios_sample"], u32::MAX - 3);
    assert_eq!(phase["allocation_index"], 1);
    assert_eq!(phase["selection"]["pacing_sum"], -3);
    assert_eq!(phase["counter_remainder"], -1);
    assert_eq!(phase["pacing_remainder"], -1);
    assert_eq!(phase["selection"]["own_selected"], 7);
    assert_eq!(phase["random_before"], 42);
    assert_eq!(random.state(), 42);
    assert_eq!(world.inspect(0, 1)["actors"][0]["control"], before);
    assert_eq!(world.inspect(0, 1), inspected);
}

#[test]
fn ordinary_defence_observation_distinguishes_absolute_phase_and_invalid_remainder() {
    for (sample, allocation, counter, expected_sum, expected_remainder) in [
        (1110847, 12, 29, 1110859, Some(14)),
        (393315, 12, 29, 393327, Some(0)),
        (i32::MAX, 1, 29, i32::MIN, Some(-8)),
        (i32::MIN, 0, -1, i32::MIN, None),
    ] {
        let mut world = counter_fixture_state(false, true, |saved| {
            let combat = &mut saved["actors"][0]["control"]["combat"];
            combat["action"] = 7.into();
            combat["allocation_index"] = allocation.into();
            combat["decision"]["move_selector"] = 2.into();
            combat["decision"]["defence_counter"] = counter.into();
            combat["decision"]["defence_pacing"] = 0.into();
            combat["target"] = serde_json::Value::Null;
        });
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        let result = run_ordinary_defence(&mut world, sample, &mut random);
        assert_eq!(result.is_ok(), expected_remainder.is_some());
        let inspected = world.inspect(0, 1);
        let phase = &inspected["actors"][0]["combat_pacing"];
        assert_eq!(phase["cached_bios_sample"], sample as u32);
        assert_eq!(phase["selection"]["pacing_sum"], expected_sum);
        assert_eq!(
            phase["counter_remainder"],
            serde_json::json!(expected_remainder)
        );
        assert!(phase["pacing_remainder"].is_null());
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn ordinary_combat_entry_retains_the_consumed_sample_even_before_selection() {
    use cyrus_script::script::ScriptInputs;
    let mut world = counter_fixture_state(false, true, |saved| {
        saved["actors"][0]["control"]["combat"]["allocation_index"] = 12.into();
    });
    let id = world.id(0).unwrap();
    let player = world.id(1).unwrap();
    let mut random = cyrus_game::random::EngineRandom::from_state(42);
    let result = world.run_ordinary_combat_decision(
        id,
        player,
        ScriptInputs::default(),
        &mut random,
        &mut |_, _| Ok(()),
    );
    assert!(format!("{:#}", result.unwrap_err()).contains("BIOS-clock sample unavailable"));
    assert!(world.inspect(0, 1)["actors"][0]["combat_pacing"].is_null());
    // The diagnostic survives missing downstream services without requiring or
    // manufacturing those services merely to publish a sample.
    let _ = world.run_ordinary_combat_decision(
        id,
        player,
        ScriptInputs {
            combat_clock_sample: Some(1110847),
            ..Default::default()
        },
        &mut random,
        &mut |_, _| Ok(()),
    );
    let inspected = world.inspect(0, 1);
    let phase = &inspected["actors"][0]["combat_pacing"];
    assert_eq!(phase["cached_bios_sample"], 1110847);
    assert_eq!(phase["allocation_index"], 12);
    assert_eq!(phase["pacing_sum"], 1110859);
    assert!(phase["selection"].is_null());
    assert_eq!(world.inspect(0, 1), inspected);
}
