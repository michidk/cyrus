use super::*;

#[test]
fn actor_model_point_uses_static_source_order_and_local_pose() {
    let (world, data, statics) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    assert_eq!(
        world
            .actor_model_point(id, &data, Some(&statics), 0)
            .unwrap(),
        [-2560, 5120, 2560]
    );
    assert!(
        world
            .actor_model_point(id, &data, Some(&statics), -1)
            .is_err()
    );
    let error = world
        .actor_model_point(id, &data, Some(&statics), 3)
        .unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("world 7 instance 0") && text.contains("vertex 3"),
        "{text}"
    );
    assert!(world.actor_model_point(id, &data, None, 0).is_err());
    assert!(
        world
            .actor_model_point(
                cyrus_script::script::ScriptActorId {
                    generation: 2,
                    index: 0
                },
                &data,
                Some(&statics),
                0
            )
            .is_err()
    );
}

#[test]
fn actor_model_point_reads_the_static_model_of_a_none_load_path() {
    let (world, mut data, statics) = static_model_fixture(cache());
    for actor in data.metadata["actors"].as_array_mut().unwrap() {
        actor["model_load_path"] = "none".into();
    }
    let id = world.id(0).unwrap();
    assert_eq!(
        world
            .actor_model_point(id, &data, Some(&statics), 0)
            .unwrap(),
        [-2560, 5120, 2560]
    );
}

#[test]
fn actor_model_point_uses_selected_player_frame_and_original_vertex_rounding() {
    let (_, world, mut data, _) = player_walking_fixture();
    let player = world.id(2).unwrap();
    data.models.get_mut("PLAYER").unwrap().positions[0] =
        vec![[0.001953125, 0.005859375, -0.009765625]];
    assert_eq!(
        world.actor_model_point(player, &data, None, 0).unwrap(),
        [2560, -1534, 7678]
    );
    assert!(world.actor_model_point(player, &data, None, -1).is_err());
    assert!(world.actor_model_point(player, &data, None, 1).is_err());
    data.metadata["script_content_id"] = "wrong".into();
    assert!(world.actor_model_point(player, &data, None, 0).is_err());
}

#[test]
fn dummy_manager_model_point_bypasses_frame_and_vertex_lookup() {
    let (mut scripts, _, mut data, _) = player_walking_fixture();
    scripts.updates.dummy_model_manager = true;
    scripts.content_id = scripts.content_identity().unwrap();
    data.metadata["script_content_id"] = scripts.content_id.clone().into();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.id(2).unwrap();
    world
        .initialize_player_spatial(player, [2560, -1536, 7680], [0; 3])
        .unwrap();
    data.models.clear();
    assert_eq!(
        world.actor_model_point(player, &data, None, -7).unwrap(),
        [2560, -1536, 7680]
    );
}

#[test]
fn sphere_contact_uses_current_motion_model_point_and_strict_radius() {
    for (distance, radius, expected) in [
        (0, 0, false),
        (0, 1, true),
        (99, 100, true),
        (100, 100, false),
        (101, 100, false),
        (1, -1, false),
    ] {
        let (world, _, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                let motion = serde_json::from_value(serde_json::json!({
                "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                "first_collide_frame":0,"collide_duration":0,"col_vertex":0,"col_sphere_size":radius
            })).unwrap();
                scripts.combat_attacks = vec![combat::Attack {
                    is_combo: false,
                    motions: vec![motion],
                    min_range: 0,
                    max_range: 100,
                }];
            },
            |saved| {
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
                saved["spatial"]["actors"][0]["local"]["position"] =
                    serde_json::json!([5120, 0, 5120]);
                saved["spatial"]["actors"][1]["local"]["position"] =
                    serde_json::json!([distance, 0, 0]);
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "static".into();
        let (_, _, statics) = static_model_fixture(cache());
        let id = world.id(0).unwrap();
        let candidate = world.id(1).unwrap();
        assert_eq!(
            world
                .combat_sphere_contact(id, candidate, &data, Some(&statics))
                .unwrap(),
            expected,
            "distance {distance} radius {radius}"
        );
        assert!(
            world
                .combat_sphere_contact(id, candidate, &data, None)
                .is_err(),
            "zero radius must still query geometry"
        );
    }
}

#[test]
fn contact_orientation_uses_current_motion_and_strict_arc_plus_center() {
    for (heading, arc, center, expected) in [
        (0, 0, 0, false),
        (0, 1, 0, true),
        (255, 256, 0, true),
        (256, 256, 0, false),
        (257, 256, 0, false),
        (256, 256, 1, true),
        (255, 256, -1, false),
        (2047, 1, 1, true),
        (0, i32::MAX, 1, false),
    ] {
        let (world, _, _) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                let motion = |arc, center| {
                    serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "first_collide_frame":0,"collide_duration":0,"arc":arc,"arc_center":center
                    }))
                    .unwrap()
                };
                scripts.combat_attacks = vec![combat::Attack {
                    is_combo: true,
                    motions: vec![motion(0, 0), motion(arc, center)],
                    min_range: 0,
                    max_range: 100,
                }];
            },
            |saved| {
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][0]["control"]["combat"]["decision"]["move_selector"] =
                    serde_json::json!(1);
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
                saved["spatial"]["actors"][0]["local"]["angles"] =
                    serde_json::json!([0, heading, 0]);
            },
        );
        assert_eq!(
            world
                .combat_contact_orientation(world.id(0).unwrap(), [0, 0, 256])
                .unwrap(),
            expected,
            "heading {heading} arc {arc} center {center}"
        );
    }
}

#[test]
fn range_contact_uses_first_motion_strict_ranges_and_candidate_radius() {
    for (z, y, radius, expected) in [
        (0, 0, 100, false),
        (1, 0, 100, true),
        (1, 0, 0, true),
        (99, 1000, 100, true),
        (100, 0, 1000, false),
        (99, 0, 99, false),
        (99, 0, 98, false),
        (99, 0, -1, false),
        (99, 1000, 0, false),
    ] {
        let (mut world, _, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.updates.dummy_model_manager = true;
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                }
                let motion = |minimum, maximum| {
                    serde_json::from_value(serde_json::json!({
                "section":0,"kind":0,"animation_group":18,"min_range":minimum,"max_range":maximum,
                "first_collide_frame":0,"collide_duration":0,"arc":2048,"arc_center":0,"elevation":0
            })).unwrap()
                };
                scripts.combat_attacks = vec![combat::Attack {
                    is_combo: true,
                    motions: vec![motion(0, 100), motion(-100, 10000)],
                    min_range: 0,
                    max_range: 10000,
                }];
            },
            |saved| {
                let combat = serde_json::to_value(cyrus_script::script::CombatControlState {
                    attack_indices: [Some(0); 9],
                    retained_action_indices: [-1, 0],
                    ..Default::default()
                })
                .unwrap();
                saved["actors"][0]["control"]["combat"] = combat.clone();
                saved["actors"][0]["control"]["combat"]["decision"]["move_selector"] = 1.into();
                saved["actors"][1]["control"]["combat"] = combat;
                saved["actors"][1]["control"]["combat"]["contact_profile"] =
                    serde_json::json!({"vertex":0,"radius":radius});
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
                saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([0, y, z]);
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
        if radius == 0 {
            data.metadata["actors"][0]["mpsz_bounds_primary"] = 0.into();
            data.metadata["actors"][0]["raex"] = serde_json::json!({"unknown_12":256});
            data.metadata["mpsz_entries"] = serde_json::json!([{
                "total_extent":[24,24,24],"pos_extent":[12,12,12],
                "neg_extent":[12,12,12],"center_offset":[0,0,0],"flags":0
            }]);
            let (_, _, models) = static_model_fixture(cache());
            let scene = ordinary_collision_scene(world.world, models);
            for actor in [world.id(0).unwrap(), world.id(1).unwrap()] {
                world
                    .prepare_ordinary_collision_shape(actor, &data, &scene)
                    .unwrap();
            }
        }
        assert_eq!(
            world
                .combat_range_contact(world.id(0).unwrap(), world.id(1).unwrap(), &data, None)
                .unwrap(),
            expected,
            "z {z} y {y} radius {radius}"
        );
    }
}

#[test]
fn range_contact_with_combat_radius_vhit_tests_the_retained_weapon_tip_against_the_shifted_sphere()
{
    let build = || {
        movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.updates.dummy_model_manager = true;
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                }
            },
            |saved| {
                let combat = serde_json::to_value(cyrus_script::script::CombatControlState {
                    attack_indices: [Some(0); 9],
                    retained_action_indices: [-1, 0],
                    ..Default::default()
                })
                .unwrap();
                saved["actors"][0]["control"]["combat"] = combat.clone();
                saved["actors"][1]["control"]["combat"] = combat;
                saved["actors"][1]["control"]["combat"]["contact_profile"] =
                    serde_json::json!({"vertex":0,"radius":100 << 8});
                saved["actors"][1]["attributes"][126] = 1.into();
                saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([0, 0, 0]);
                saved["spatial"]["actors"][1]["local"]["position"] =
                    serde_json::json!([0, 0, 50 << 8]);
            },
        )
    };
    let (world, scripts, mut data) = build();
    data.metadata["actors"][0]["model_load_path"] = "animation_table".into();
    data.models.insert(
        "WEAPON".into(),
        cyrus_assets::animation::AnimationModel {
            radius: Some(1.),
            base_positions: vec![],
            base_normals: vec![],
            faces: vec![],
            corner_normals: vec![],
            encoding: "animated_i16".into(),
            positions: vec![vec![]],
            normals: vec![vec![]],
        },
    );
    let point = world
        .actor_model_point(world.id(1).unwrap(), &data, None, 0)
        .unwrap()
        .map(|value| value >> 8);
    // The query shifts the point and radius right eight bits and includes equality.
    for (offset, expected) in [(100, true), (101, false), (-100, true), (-101, false)] {
        let tip = [point[0] + offset, point[1], point[2]];
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["actors"][0]["model"]["weapon"] = serde_json::json!({
            "item":1,"model":"WEAPON","position_bits":[0,0,0],
            "matrix_bits":[[1065353216_u32,0,0],[0,1065353216_u32,0],[0,0,1065353216_u32]],
            "retained_point":[0,0,0],"retained_angles":[0,0,0],
            "endpoints":[tip,tip]
        });
        let world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        assert_eq!(
            world
                .combat_range_contact(world.id(0).unwrap(), world.id(1).unwrap(), &data, None)
                .unwrap(),
            expected,
            "tip offset {offset}"
        );
    }
}

#[test]
fn attack_windows_use_command_cursor_current_motion_and_retained_mark() {
    for (start, duration, cursor, flags, expected) in [
        (1, 1, 0, 1, false),
        (0, 1, 0, 1, true),
        (0, 1, 1, 1, true),
        (0, 1, 1, 0x21, false),
        (0, 1, 0, 0x21, true),
        (0, 0, 0, 1, false),
        (0, -1, 0, 1, true),
        (i32::MAX, 1, 0, 1, true),
        (0, 1, 0, 0, false),
    ] {
        let (mut world, _, _) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.combat_attacks = vec![combat::Attack {
                    is_combo: true,
                    motions: [0, start]
                        .into_iter()
                        .map(|start| {
                            serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "first_collide_frame":start,"collide_duration":duration
                    })).unwrap()
                        })
                        .collect(),
                    min_range: 0,
                    max_range: 100,
                }];
            },
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        action: 0,
                        flags: CombatFlags::from_bits(flags),
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][0]["control"]["combat"]["decision"]["move_selector"] = 1.into();
                saved["actors"][0]["animation"]["playback"]["current"][1]["command"] =
                    cursor.into();
            },
        );
        let id = world.id(0).unwrap();
        assert_eq!(
            world.combat_attack_window(id).unwrap(),
            expected,
            "start {start} duration {duration} cursor {cursor} flags {flags}"
        );
        let result =
            world.maintain_map_combat(id, 1, false, Default::default(), None, &mut |_, _| Ok(()));
        if expected {
            assert!(
                format!("{:#}", result.unwrap_err())
                    .contains("attack candidate random owner unavailable")
            );
        } else {
            result.unwrap();
        }
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        assert_eq!(
            (combat.action, combat.flags.bits()),
            (0, if expected { flags | 0x20 } else { flags })
        );
    }
}

#[test]
fn reverse_attack_admission_returns_before_motion_metadata() {
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |_| {},
        |saved| {
            saved["actors"][0]["control"]["death_selector"] = 0.into();
            saved["actors"][0]["control"]["combat"] =
                serde_json::to_value(cyrus_script::script::CombatControlState {
                    action: 0,
                    flags: CombatFlags::from_bits(1),
                    ..Default::default()
                })
                .unwrap();
            saved["actors"][0]["animation"]["playback"]["reverse"] = true.into();
        },
    );
    let id = world.id(0).unwrap();
    assert!(!world.combat_attack_window(id).unwrap());
    world
        .maintain_map_combat(id, 1, false, Default::default(), None, &mut |_, _| Ok(()))
        .unwrap();
    let combat = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!((combat.action, combat.flags.bits()), (0, 1));
}

#[test]
fn defence_windows_use_current_motion_signed_boundaries_and_required_metadata() {
    for (start, duration, cursor, flags, expected) in [
        (1, Some(1), 0, 0, Some(false)),
        (1, None, 0, 0, Some(false)),
        (0, None, 0, 0, None),
        (0, Some(1), 0, 0, Some(true)),
        (0, Some(1), 1, 0x21, Some(false)),
        (0, Some(0), 0, 0, Some(false)),
        (0, Some(-1), 0, 1, Some(false)),
        (i32::MAX, Some(1), 0, 1, Some(false)),
        (0, Some(1), 0, 0x21, Some(true)),
    ] {
        let (world, _, _) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.combat_attacks = vec![combat::Attack {
                    is_combo: true,
                    motions: [0, start]
                        .into_iter()
                        .map(|start| {
                            serde_json::from_value(serde_json::json!({
                        "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                        "first_collide_frame":0,"collide_duration":0,
                        "first_defend_frame":start,"defend_duration":duration
                    })).unwrap()
                        })
                        .collect(),
                    min_range: 0,
                    max_range: 100,
                }];
            },
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        action: 0,
                        flags: CombatFlags::from_bits(flags),
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][0]["control"]["combat"]["decision"]["move_selector"] = 1.into();
                saved["actors"][0]["animation"]["playback"]["current"][1]["command"] =
                    cursor.into();
            },
        );
        let id = world.id(0).unwrap();
        let result = world.combat_defence_window(id);
        match expected {
            Some(expected) => assert_eq!(
                result.unwrap(),
                expected,
                "start {start} duration {duration:?} cursor {cursor} flags {flags}"
            ),
            None => assert!(
                format!("{:#}", result.unwrap_err()).contains("defend_duration unavailable")
            ),
        }
    }
}

#[test]
fn selected_player_is_a_live_candidate_and_its_own_model_is_excluded() {
    use cyrus_collision::candidates::{
        CandidateOwner, CandidateQuery, CollisionCandidates, ModelBounds, ModelInstance,
    };
    use cyrus_script::script::ScriptActorSource;
    let (_, world, _, _) = player_walking_fixture();
    let player = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(player).unwrap();
    let map = world.id(0).unwrap();
    let alternate = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::AlternatePlayer))
        .map(|index| world.id(index).unwrap());
    let mut order = vec![player, map];
    order.extend(alternate);
    let live = world.live_collision_inputs(&order).unwrap();
    assert_eq!(
        live.iter().map(|actor| actor.actor).collect::<Vec<_>>(),
        [player.index, map.index]
    );
    assert_eq!(live[0].collision_flags, 0);
    let model = live[0].model.unwrap();
    let query =
        CandidateQuery::new(model.position.map(|value| (value * 256.0) as i32), None).unwrap();
    let candidates = CollisionCandidates::build(&query, &[], &live, &[]).unwrap();
    assert!(
        candidates
            .actors
            .iter()
            .any(|candidate| candidate.owner == CandidateOwner::Actor(player.index))
    );
    let query = CandidateQuery::new(
        model.position.map(|value| (value * 256.0) as i32),
        Some(ModelBounds {
            instance: ModelInstance::Runtime(player.index),
            ..model
        }),
    )
    .unwrap();
    let candidates = CollisionCandidates::build(&query, &[], &live, &[]).unwrap();
    assert!(
        !candidates
            .actors
            .iter()
            .any(|candidate| candidate.owner == CandidateOwner::Actor(player.index))
    );
}

#[test]
fn hit_defence_retains_ordered_response_and_partial_writes() {
    use cyrus_game::random::EngineRandom;
    use cyrus_script::script::{CombatControlState, ScriptInputs, ScriptSound};
    for (distance, flags, selector, fail_sound, clock, expected) in [
        (0, 0x8a, 0, false, true, Some(true)),
        (0, 0x82, 0, false, true, Some(true)),
        (100, 0x8a, 0, false, true, Some(false)),
        (0, 0x88, 0, false, true, Some(false)),
        (0, 0x8a, 1, false, true, Some(false)),
        (0, 0x8a, 0, true, true, None),
        (0, 0x8a, 0, false, false, None),
    ] {
        let (mut world, scripts, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.combat_clank_effects = [Some(31), Some(32)];
                scripts.combat_attacks =
                    vec![combat::Attack {
                        is_combo: false,
                        motions: vec![serde_json::from_value(serde_json::json!({
                    "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                    "first_collide_frame":0,"collide_duration":1,"first_defend_frame":0,
                    "defend_duration":1,"defense":1,"col_vertex":0,"col_sphere_size":100
                })).unwrap()],
                        min_range: 0,
                        max_range: 100,
                    }];
            },
            |saved| {
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        action: 0,
                        flags: CombatFlags::from_bits(0xff),
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
                saved["actors"][1]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        flags: CombatFlags::from_bits(flags),
                        retained_action_indices: [selector, -1],
                        ..Default::default()
                    })
                    .unwrap();
                saved["spatial"]["actors"][0]["local"]["position"] =
                    serde_json::json!([5120, 0, 5120]);
                saved["spatial"]["actors"][1]["local"]["position"] =
                    serde_json::json!([distance, 0, 0]);
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "static".into();
        let (_, _, statics) = static_model_fixture(cache());
        let attacker = world.id(0).unwrap();
        let defender = world.id(1).unwrap();
        let mut random = EngineRandom::from_state(1);
        let mut sounds = vec![];
        let result = world.try_combat_hit_defence(
            defender,
            attacker,
            &data,
            Some(&statics),
            ScriptInputs {
                combat_clock_sample: clock.then_some(0),
                ..Default::default()
            },
            &mut random,
            &mut |actor, sound| {
                sounds.push((actor, sound));
                anyhow::ensure!(!fail_sound, "impact backend failed");
                Ok(())
            },
        );
        match expected {
            Some(value) => assert_eq!(result.unwrap(), value),
            None => assert!(result.is_err()),
        }
        let collision = cyrus_assets::collision::CollisionScene::default();
        let sources = cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        let hit = distance == 0 && flags & 2 != 0 && selector == 0;
        assert_eq!(saved["actors"][0]["animation"]["playback"]["reverse"], hit);
        let complete = expected == Some(true);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            if complete { 0xfc } else { 0xff }
        );
        let missed = distance == 100 && flags & 2 != 0 && selector == 0;
        assert_eq!(
            world.actors()[1]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            if complete {
                (flags & !3) ^ 8
            } else if missed {
                flags & !8
            } else {
                flags
            }
        );
        assert_eq!(sounds.len(), usize::from(hit));
        if hit {
            assert!(
                matches!(sounds[0],(actor,ScriptSound::Positional {effect:31,volume:0,rate_offset_hz:96,start_index:None,..}) if actor==defender)
            );
            assert_eq!(random.state(), 1103527590);
        } else {
            assert_eq!(random.state(), 1);
        }
        let message = world.consume_combat_message(attacker).unwrap();
        if hit && !fail_sound {
            let message = message.unwrap();
            assert_eq!(
                (message.sender, message.receiver, message.code),
                (defender, Some(attacker), CombatMessageCode::BLOCK)
            );
        } else {
            assert!(message.is_none());
        }
    }
}

#[test]
fn owned_face_contact_ignores_segment_miss_and_applies_damage_after_sound() {
    use cyrus_script::script::{
        CombatControlState, LocomotionGeometry, OrdinaryGroundQuery, ScriptInputs,
    };
    for (health, fail_sound, expected_health) in [(50, false, 43), (5, false, 0), (50, true, 50)] {
        let (tip_x, base_x, expected_cues) = (10, -10, 1);
        let (world, scripts, mut data) = movement_fixture(
            18,
            false,
            true,
            |scripts| {
                for instance in &mut scripts.instances {
                    instance.object_type = PlacementType::MODEL;
                }
                scripts.combat_clank_effects = [Some(31), None];
                scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::HitSound] = 31;
                scripts.combat_attacks =
                    vec![combat::Attack {
                        is_combo: false,
                        motions: vec![serde_json::from_value(serde_json::json!({
                    "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                    "first_collide_frame":0,"collide_duration":1,"col_vertex":0,"col_sphere_size":10000,"damage":7
                })).unwrap()],
                        min_range: 0,
                        max_range: 100,
                    }];
            },
            |saved| {
                saved["actors"][0]["control"]["death_selector"] = 0.into();
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(CombatControlState {
                        action: 0,
                        flags: CombatFlags::from_bits(1),
                        attack_indices: [Some(0); 9],
                        retained_action_indices: [-1, 0],
                        ..Default::default()
                    })
                    .unwrap();
            },
        );
        data.metadata["actors"][0]["model_load_path"] = "static".into();
        data.metadata["actors"][0]["raex"] =
            serde_json::json!({"unknown_12":0,"v_vertex":0,"v_size":0});
        data.metadata["actors"][0]["mpsz_bounds_primary"] = (-1).into();
        data.metadata["mpob_objects"] = serde_json::json!([{"model_id":-1},{"model_id":-1}]);
        data.models.insert(
            "WEAPON".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        );
        let mut saved = movement_snapshot(&world, &scripts, &data);
        saved["spatial"]["actors"][0]["local"]["position"] = serde_json::json!([262144, 0, 262144]);
        saved["actors"][0]["model"]["weapon"] = serde_json::json!({
            "item":1,"model":"WEAPON","position_bits":[0,0,0],
            "matrix_bits":[[1065353216_u32,0,0],[0,1065353216_u32,0],[0,0,1065353216_u32]],
            "retained_point":[0,0,0],"retained_angles":[0,0,0],
            "endpoints":[[1024+base_x,0,1024],[1024+tip_x,0,1024]]
        });
        saved["spatial"]["actors"][1]["local"]["position"] = serde_json::json!([262144, 0, 262144]);
        saved["actors"][1]["attributes"][3] = health.into();
        saved["actors"][1]["control"]["death_selector"] = 0.into();
        let mut world = restore_navigation_owner(saved, &scripts, &data).unwrap();
        let id = world.id(0).unwrap();
        let (_, _, models) = static_model_fixture(cache());
        world.prepare_model_resources(&data, &models).unwrap();
        let defender = world.id(1).unwrap();
        world
            .refresh_model_presentations(&data, &[id, defender])
            .unwrap();
        let scene = ordinary_collision_scene(world.world, models);
        let prepared = world
            .prepare_ordinary_collision_query(id, &scene, &data, [1024, 1024], &[defender])
            .unwrap();
        let mut random = cyrus_game::random::EngineRandom::default();
        let mut cues = Vec::new();
        let result = world.maintain_map_combat(
            id,
            1,
            false,
            ScriptInputs {
                locomotion_geometry: Some(LocomotionGeometry {
                    ground: OrdinaryGroundQuery {
                        scene: &scene,
                        animations: &data,
                        prepared: &prepared,
                        distance: 0,
                        sidedness: false,
                    },
                    environment: cyrus_script::script::ObstacleEnvironment {
                        selected_player: Some(id),
                        auto_grab: false,
                        item_system_ready: true,
                        shell_execution: false,
                        ticks: 1,
                    },
                }),
                ..Default::default()
            },
            Some(&mut random),
            &mut |actor, sound| {
                cues.push((actor, sound));
                anyhow::ensure!(!fail_sound, "impact backend failed");
                Ok(())
            },
        );
        assert_eq!(result.is_err(), fail_sound);
        assert_eq!(
            world.actors()[1].attributes()[Attribute::Health],
            expected_health
        );
        assert_eq!(
            world.actors()[1].control_state().unwrap().ai_type.byte(),
            if expected_health == 0 { 8 } else { 0 }
        );
        assert_eq!(cues.len(), expected_cues, "tip {tip_x} base {base_x}");
        assert_eq!(random.state(), 1103527590);
        for ((actor, cue), expected_pitch) in cues.into_iter().zip([96, 992]) {
            assert_eq!(actor, id);
            assert!(matches!(
                cue,
                cyrus_script::script::ScriptSound::Positional {
                    effect: 31,
                    volume: 0,
                    rate_offset_hz,
                    ..
                } if rate_offset_hz == expected_pitch
            ));
        }
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .flags
                .bits(),
            0x21
        );
    }
}

#[test]
fn player_collide_natives_read_the_selected_players_contact_lists() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let run = |function: Native, touched: Option<usize>| {
        let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name: function,
                param_count: 0,
                kind: CallKind::Function,
            });
            scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
                assign_call(1, id, function, &[]).operation,
                set_flag(7),
                Operation::End { target: 0 },
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
        });
        if let Some(actor) = touched {
            world.globals_mut().record_player_contact(actor);
        }
        let map = world.id(0).unwrap();
        world
            .advance(
                map,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        world.actors()[0].runtime().unwrap().variables()[0]
    };
    for function in [Native::PlayerCollide, Native::PlayerCollideStatic] {
        assert_eq!(run(function, None), 0, "{function} without contact");
        assert_eq!(
            run(function, Some(0)),
            1,
            "{function} touching the receiver"
        );
        assert_eq!(
            run(function, Some(1)),
            0,
            "{function} touching another actor"
        );
    }
}

#[test]
fn collide_player_weapon_is_false_unless_the_selected_player_swings_a_drawn_weapon() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let id = u16::try_from(scripts.definition.functions.len()).unwrap();
        scripts.definition.functions.push(Function {
            name: Native::CollidePlayerWeapon,
            param_count: 4,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            assign_call(1, id, Native::CollidePlayerWeapon, &[0, 0, 0, 10240]).operation,
            set_flag(7),
            Operation::End { target: 0 },
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
    });
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(1, 1)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    let map = world.id(0).unwrap();
    world
        .advance(
            map,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                hud: Some(&hud),
                shell_execution: Some(false),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            None,
            100,
        )
        .unwrap();
    assert_eq!(
        world.actors()[0].runtime().unwrap().variables()[0],
        0,
        "no drawn weapon, no swing"
    );
}

#[test]
fn selected_wall_contact_receipt_comes_from_admitted_primary_response_not_floor_or_other_actor() {
    use cyrus_collision::{collision_world::RuntimeBodyContact, ground::SupportSource};
    let mut scripts = attribute_cache(vec![Operation::End { target: 0 }]);
    for placement in &mut scripts.instances {
        placement.object_type = PlacementType::MODEL;
        placement.angles = [0, 344, 0];
    }
    add_standalone_receivers(&mut scripts);
    let falling = scripts.definition.references.len() as u16;
    scripts.definition.references.push(Property::FallFlag);
    scripts.actors.get_mut("CYRUS").unwrap().program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: falling,
                    },
                    formula: Formula {
                        first: Value::Integer { value: 0x81 },
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
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let player = world.selected_player().unwrap();
    world
        .initialize_player_spatial(player, [0; 3], [0, 344, 0])
        .unwrap();
    world
        .advance(
            player,
            ScriptStage::Body(Thread::Main),
            cyrus_script::script::ScriptInputs {
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
        world.actors()[player.index]
            .control_state()
            .unwrap()
            .falling_state,
        JumpMotion::from_byte(0x81)
    );
    let settings = cyrus_script::player::data::MovementSettings {
        standing_height: -2,
        post_collide_height: 50,
        slide_range: -60000,
        slide_speed: None,
        slide_hurt: None,
        tap_time: None,
        sheath_sword_delay: None,
        defend_interval: None,
        swim_depth: None,
        max_remap_objects: None,
        turn_speed: 12,
        turn_max_speed: 48,
        poly_push_units: 16,
        joystick: None,
    };
    let mut contact = RuntimeBodyContact {
        source: SupportSource::Model { model: 0, face: 0 },
        point: [0; 3],
        normal: [256, 0, 0],
        classification: SurfaceClass::Floor,
    };
    world
        .resolve_ordinary_primary_contacts(
            player,
            std::slice::from_ref(&contact),
            &settings,
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(world.selected_wall_contact_update(), None);
    contact.classification = SurfaceClass::Wall;
    let other = world.id(0).unwrap();
    world
        .resolve_ordinary_primary_contacts(
            other,
            std::slice::from_ref(&contact),
            &settings,
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(world.selected_wall_contact_update(), None);
    assert!(
        world
            .resolve_ordinary_primary_contacts(
                player,
                std::slice::from_ref(&contact),
                &settings,
                &mut |_| anyhow::bail!("rejected transition")
            )
            .is_err()
    );
    assert_eq!(
        world.selected_wall_contact_update(),
        None,
        "a failed response must not publish an accepted contact"
    );
    world
        .resolve_ordinary_primary_contacts(
            player,
            std::slice::from_ref(&contact),
            &settings,
            &mut |_| Ok(()),
        )
        .unwrap();
    assert_eq!(world.selected_wall_contact_update(), Some(0));
    assert_eq!(
        world.first_selected_wall_contact_after(0).unwrap(),
        None,
        "a capture armed after the response cannot relabel it as a new contact"
    );
}

#[cfg(feature = "original-save-import")]
#[test]
fn original_attack_selector_restores_independently_of_absent_defence_before_continuation() {
    let (mut world, _, _) =
        movement_fixture(
            18,
            false,
            true,
            |scripts| {
                scripts.combat_attacks =
                    vec![combat::Attack {
                        is_combo: true,
                        motions: vec![serde_json::from_value(serde_json::json!({
                    "section":0,"kind":0,"animation_group":18,"min_range":0,"max_range":100,
                    "first_collide_frame":0,"collide_duration":1
                })).unwrap()],
                        min_range: 0,
                        max_range: 100,
                    }];
            },
            |saved| {
                saved["actors"][0]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState {
                        attack_indices: [Some(0); 9],
                        ..Default::default()
                    })
                    .unwrap();
            },
        );
    let id = world.id(0).unwrap();
    let mut bytes = [0; 422];
    for (offset, value) in [
        (0, -1i32),
        (4, 41),
        (8, -1),
        (0x38, 0),
        (0x48, 0),
        (0x4c, -1),
        (0x14d, 1),
        (0x17d, -1),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x18d] = 1;
    let mut object = rgpre::import::savegame::SaveObject {
        object_id: 41,
        rope_id: 0,
        record_kind: 0,
        body: [0; 783],
        named_resource: None,
        attributes: None,
        opaque_41: None,
        opaque_20: None,
        animation_header: None,
        script_state: Some(bytes),
        attachment: None,
        vectors: [0; 48],
        opaque_60: None,
        private_variables: vec![],
        extension: vec![],
    };
    world.restore_original_combat(&[(id, &object)]).unwrap();
    assert!(world.combat_attack_window(id).unwrap());
    let continuation =
        world.maintain_map_combat(id, 0, false, Default::default(), None, &mut |_, _| Ok(()));
    assert!(
        format!("{:#}", continuation.unwrap_err())
            .contains("attack candidate random owner unavailable")
    );
    // Missing active attack state remains an error, never a guessed slot zero.
    object.script_state.as_mut().unwrap()[0x48..0x4c].copy_from_slice(&(-1i32).to_le_bytes());
    world.restore_original_combat(&[(id, &object)]).unwrap();
    assert!(world.combat_attack_window(id).is_err());
}
