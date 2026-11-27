use super::*;

#[test]
fn live_paced_pick_uses_receiver_geometry_and_branch_specific_damage() {
    use cyrus_script::script::decision::PacedDefenceInput;
    let input = PacedDefenceInput {
        distance: 50,
        random_draw: 0,
        pacing_sum: 129,
        facing_alike: true,
        target_in_combat_stance: true,
        aggression: 29,
    };
    for (damages, expected, pacing) in [
        ([Some(i32::MAX), Some(1)], Some(i32::MIN), 129),
        ([Some(4), Some(5)], Some(9), 129),
        ([Some(4), None], None, 129),
        ([Some(4), Some(5)], Some(9), 0),
    ] {
        let mut scripts = cache();
        scripts.actors.get_mut("LOGIC").unwrap().attributes.extend([
            (Attribute::Combat, 1),
            (Attribute::AttackSkill, 129 - pacing),
            (Attribute::AttackDouble, 1),
        ]);
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
        }
        scripts.instances[0].angles[1] = 1024;
        scripts.instances[1].map_position = [0, -20, 20];
        let motions = damages
            .into_iter()
            .enumerate()
            .map(|(index, damage)| {
                serde_json::from_value(serde_json::json!({
                    "section":index,"kind":0,"animation_group":97,
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
        let stale = cyrus_script::script::ScriptActorId {
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
fn live_attack_executor_uses_four_slots_and_orders_animation_before_combat_writes() {
    use cyrus_script::script::{ScriptInputs, WorldRestoreContext, WorldSnapshotSources};
    let mut scripts = cache();
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .attributes
        .extend([(Attribute::Combat, 1), (Attribute::DefendLow, 1)]);
    for instance in &mut scripts.instances {
        instance.object_type = PlacementType::MODEL;
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
        identity_memo: None,
        effects: None,
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
            (ExecutorState::IDLE, 50)
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
        (0, 0, Some(1), false, ExecutorState::ATTACKING, 40, 0),
        (0, 0, Some(1), true, ExecutorState::IDLE, 50, 0),
        (0, 0, None, false, ExecutorState::IDLE, 50, 0),
        (0, 100, None, false, ExecutorState::IDLE, 50, 0),
        (2, 100, None, false, ExecutorState::ATTACKING, 50, 0),
        (2, 0, None, false, ExecutorState::IDLE, 50, -1),
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
                !expected_state.is_idle() && expected_pending != -1
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
        if expected_state == ExecutorState::ATTACKING && state == 0 {
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
fn armed_attack_retained_taunt_sample_suppresses_only_until_cache_cleared() {
    use cyrus_script::script::{
        CombatTauntPlayback, CombatTauntSource, DialogueLine, OrdinaryGroundQuery,
        PlayerControllerInput, ScriptActorSource, ScriptInputs, WorldRestoreContext,
        WorldSnapshotSources,
    };
    #[derive(Debug)]
    struct Source(std::cell::RefCell<(Option<u8>, Vec<&'static str>)>);
    impl CombatTauntPlayback for Source {
        fn remembered_sample(&self, index: usize) -> anyhow::Result<Option<u8>> {
            assert_eq!(index, 4);
            self.0.borrow_mut().1.push("sample");
            Ok(self.0.borrow().0)
        }
        fn prepare(&mut self, label: [u8; 4], _: bool, _: Option<u32>) -> anyhow::Result<()> {
            assert_eq!(label, *b"TEST");
            self.0.borrow_mut().1.push("prepare");
            Ok(())
        }
        fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
            assert_eq!(retained.take(), Some(4));
            self.0.borrow_mut().1.push("release");
            Ok(())
        }
        fn load(&mut self, _: [u8; 4]) -> anyhow::Result<usize> {
            self.0.borrow_mut().1.push("load");
            Ok(4)
        }
        fn present(&mut self, _: usize, _: bool, _: Option<u32>) -> anyhow::Result<DialogueLine> {
            self.0.borrow_mut().1.push("present");
            Ok(DialogueLine {
                text: "Voice".into(),
                sample: None,
                lifetime: 210,
            })
        }
    }
    #[derive(Debug)]
    struct Playback(std::cell::RefCell<Source>);
    impl CombatTauntSource for Playback {
        fn with_playback(
            &self,
            update: &mut dyn FnMut(&mut dyn CombatTauntPlayback) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            update(&mut *self.0.borrow_mut())
        }
    }
    for woman in [0, 1] {
        let (scripts, mut world, mut data, mut scene) = player_walking_fixture_with(|scripts| {
            scripts
                .actors
                .get_mut("LOGIC")
                .unwrap()
                .attributes
                .extend([(Attribute::Npc, 1), (Attribute::Woman, woman)]);
            let player = scripts.actors.get_mut("CYRUS").unwrap();
            player.attributes.extend([
                (Attribute::AttackSingle, 1),
                (Attribute::Chatty, 127),
                (Attribute::NumTaunts, 1),
            ]);
            player.combat_taunts = vec![CombatTaunt {
                trigger: if woman == 0 {
                    TauntType::UNARMED_MALE_OPPONENT
                } else {
                    TauntType::UNARMED_FEMALE_OPPONENT
                },
                label: *b"TEST",
                animation: 0,
            }];
            scripts.combat_attacks = vec![
                combat::Attack {
                    is_combo: false,
                    motions: vec![],
                    min_range: 0,
                    max_range: 0,
                },
                combat::Attack {
                    is_combo: false,
                    motions: vec![serde_json::from_value(serde_json::json!({
                    "section":0,"kind":0,"animation_group":0,"min_range":0,"max_range":10000,
                    "arc":4096,"arc_center":0,"first_collide_frame":0,"collide_duration":0
                })).unwrap()],
                    min_range: 0,
                    max_range: 10000,
                },
            ];
        });
        data.metadata["markers"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        data.metadata["walk_node_maps"] = serde_json::json!([]);
        scene.source_identity = Some("protected-actor-taunt-fixture".into());
        scene.settings.tap_time = Some(6);
        scene.settings.sheath_sword_delay = Some(100);
        scene.settings.defend_interval = Some(100);
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let sources = WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: Some(&scene),
            scripts: &scripts,
            animations: &data,
            collision: &scene.collision,
        };
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        saved["actors"][index]["control"]["weapon_drawn"] = true.into();
        saved["actors"][index]["control"]["combat"]["taunt_cache"] = 4.into();
        let globals = std::mem::take(world.globals_mut());
        let mut world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            WorldRestoreContext {
                generation: 2,
                globals,
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let player = world.id(index).unwrap();
        let map = world.id(0).unwrap();
        world.ordinary_actors([10, 30]).unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[player, map])
            .unwrap();
        let hud = cyrus_script::script::ScriptHud::default();
        hud.configure(&inventory_asset(&[]), &Default::default());
        let held = cyrus_script::player::input::PlayerInput {
            action: true,
            ..Default::default()
        };
        let playback = Playback(std::cell::RefCell::new(Source(std::cell::RefCell::new((
            Some(3),
            vec![],
        )))));
        let mut random = cyrus_game::random::EngineRandom::from_state(1);
        for remembered in [true, false] {
            playback.0.borrow().0.borrow_mut().0 = remembered.then_some(3);
            world.begin_player_movement_update(player, 0).unwrap();
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
                        player_controller: Some(PlayerControllerInput {
                            held: &held,
                            walk_mode: false,
                            auto_defend: false,
                            steer: None,
                            claim: None,
                            probes: None,
                        }),
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        shell_execution: Some(false),
                        combat_taunts: Some(&playback),
                        ..Default::default()
                    },
                    &mut random,
                    &mut |_, _| Ok(()),
                )
                .unwrap();
            world.finish_player_movement_update(player).unwrap();
            if remembered {
                assert_eq!(playback.0.borrow().0.borrow().1, ["sample"]);
                // The armed proximity probe draws; the remembered taunt does not.
                assert_eq!(random.state(), 1103527590);
            } else {
                assert_eq!(
                    playback.0.borrow().0.borrow().1,
                    ["sample", "sample", "prepare", "release", "load", "present"]
                );
                // The next proximity probe and fresh taunt selection each draw once.
                assert_eq!(random.state(), 662824084);
            }
        }
    }
}
