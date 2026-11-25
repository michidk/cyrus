use super::*;

#[test]
fn surface_class_queries_read_receivers_stored_surface_without_a_support_query() {
    let cases = [
        (
            Native::IsInDeepWater,
            (|surface| surface == 2 || surface == 4) as fn(u8) -> bool,
        ),
        (Native::IsInWater, |surface| (1..=4).contains(&surface)),
        (Native::IsInLava, |surface| surface == 5),
    ];
    for (name, hit) in cases {
        let mut scripts = cache();
        scripts.definition.functions.push(Function {
            name,
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
                        function: name,
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
            world
                .set_actor_surface(first, Surface::from_byte(surface))
                .unwrap();
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
                        cyrus_script::script::ScriptActorId {
                            generation: 2,
                            index: 0
                        },
                        Surface::from_byte(surface)
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
        name: Native::IsInDeepWater,
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
                    function: Native::IsInDeepWater,
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
fn ordinary_landing_decision_uses_strict_installed_thresholds_and_water() {
    use cyrus_script::script::{
        OrdinaryLandingDecision as D, OrdinaryLandingSettings, ordinary_landing_decision,
    };
    let settings = OrdinaryLandingSettings {
        death_height: 600,
        hurt_height: 300,
        hurt_zap: 10,
    };
    let decide = |distance, surface| ordinary_landing_decision(distance, 0, surface, settings);
    assert_eq!(decide(300 * 256, Surface::UNKNOWN), D::Ordinary);
    assert_eq!(
        decide(300 * 256 + 1, Surface::UNKNOWN),
        D::Hurt { health_change: -10 }
    );
    assert_eq!(
        decide(-(600 * 256), Surface::UNKNOWN),
        D::Hurt { health_change: -10 }
    );
    assert_eq!(decide(-(600 * 256 + 1), Surface::UNKNOWN), D::Fatal);
    assert_eq!(decide(300 * 256 + 1, Surface::GLOOP), D::HurtExcluded);
    assert_eq!(decide(300 * 256 + 1, Surface::LAVA), D::HurtExcluded);
    for surface in 1..=4 {
        assert_eq!(
            decide(600 * 256 + 1, Surface::from_byte(surface)),
            D::Ordinary
        );
    }
    assert_eq!(decide(600 * 256 + 1, Surface::LAVA), D::Fatal);
    assert_eq!(
        ordinary_landing_decision(i32::MIN, i32::MAX, Surface::UNKNOWN, settings),
        D::Ordinary
    );
}

#[test]
fn ordinary_landing_requests_keep_selected_player_weapon_state_separate() {
    use cyrus_script::script::ordinary_landing_request as request;
    assert_eq!(
        request(JumpMotion::NONE, 0, false, None, None).unwrap(),
        None
    );
    for (motion, dry, torch, armed) in [
        (JumpMotion::RUNNING, 67, 68, 130),
        (JumpMotion::RUNNING_LEFT_LEAD, 69, 70, 131),
    ] {
        assert_eq!(
            request(motion.with_arms_raised(true), 0, true, Some(false), None).unwrap(),
            Some(AnimationGroup::from_index(dry))
        );
        assert_eq!(
            request(motion, 1, false, Some(false), None).unwrap(),
            Some(AnimationGroup::from_index(torch))
        );
        assert_eq!(
            request(motion, 0, false, Some(true), None).unwrap(),
            Some(AnimationGroup::from_index(armed))
        );
        assert!(
            request(motion, 0, false, None, None)
                .unwrap_err()
                .to_string()
                .contains("selected-player")
        );
    }
    assert_eq!(
        request(JumpMotion::STANDING, 0, false, None, None).unwrap(),
        Some(AnimationGroup::FIRST_LAND_GROUP)
    );
    assert_eq!(
        request(JumpMotion::STANDING, 1, false, None, None).unwrap(),
        Some(AnimationGroup::LAND_TORCH)
    );
    assert_eq!(
        request(
            JumpMotion::STANDING,
            0,
            true,
            None,
            Some(AnimationGroup::FIGHT_LAND)
        )
        .unwrap(),
        Some(AnimationGroup::FIGHT_LAND)
    );
    assert_eq!(
        request(
            JumpMotion::STANDING,
            0,
            true,
            None,
            Some(AnimationGroup::FIGHT_FALL_ATTACK)
        )
        .unwrap(),
        None
    );
    assert_eq!(
        request(
            JumpMotion::STANDING,
            0,
            true,
            None,
            Some(AnimationGroup::FIGHT_LAND_ATTACK)
        )
        .unwrap(),
        None
    );
}

#[test]
fn ordinary_water_rollback_restores_retained_position_frame_and_flags() {
    use cyrus_script::script::{ScriptInputs, ScriptSound, ScriptStage};
    use cyrus_world::sound::AnimationSoundState;
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Weight] = 7;
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
    world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
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
    world.set_actor_surface(id, Surface::WATER).unwrap();
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
    assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 51);
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
    assert_eq!(world.actors()[0].attributes()[Attribute::Weight], 0);
    assert_eq!(world.actors()[0].control_state().unwrap().move_counter, 48);
    assert!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .contains(CollisionFlags::ROLLBACK)
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
fn ordinary_automatic_ground_response_retains_support_and_sustained_script_updates() {
    use cyrus_script::script::{OrdinaryInputs, ScriptInputs};
    let mut scripts = cache();
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::Collide] = 4;
    actor.attributes[Attribute::FallDamage] = 1;
    actor.attributes[Attribute::Weight] = 128;
    actor.attributes[Attribute::ReservedWeight] = 64;
    for placement in &mut scripts.instances {
        placement.active = 1;
    }
    let (mut world, data, models) = static_model_fixture(scripts);
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    world
        .refresh_model_presentations(&data, &[first, second])
        .unwrap();
    world.set_actor_surface(first, Surface::WOOD).unwrap();
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
        assert_eq!(world.actor_surface(first).unwrap(), Surface::UNKNOWN);
        let control = world.actors()[0].control_state().unwrap();
        assert!(control.stable_position_valid);
        assert_eq!(control.stable_local_position, [2560, 4608, 7680]);
        assert_eq!(world.actors()[0].attributes()[Attribute::Weight], 128);
    }
}

#[test]
fn nonplayer_support_queries_publish_metadata_without_resetting_player_ticks() {
    use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource};
    let (scripts, mut world, data, scene) = player_walking_fixture();
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    let player = world.id(index).unwrap();
    let floor = world.id(0).unwrap();
    let npc = world.id(1).unwrap();
    if world.ordinary_collision_shape(npc).is_err() {
        world
            .prepare_ordinary_collision_shape(npc, &data, &scene)
            .unwrap();
    }
    world.begin_player_movement_update(player, 0).unwrap();
    let empty = world
        .prepare_ordinary_collision_query(player, &scene, &data, [10, 30], &[])
        .unwrap();
    assert!(
        world
            .query_ordinary_ground(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &empty,
                    distance: 4,
                    sidedness: false
                }
            )
            .unwrap()
            .is_none()
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][index]["player_movement"]["support_ticks"],
        0
    );
    let prepared = world
        .prepare_ordinary_collision_query(npc, &scene, &data, [10, 30], &[npc, floor])
        .unwrap();
    assert!(
        world
            .query_ordinary_ground(
                npc,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 1000,
                    sidedness: false
                }
            )
            .unwrap()
            .is_some()
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][index]["player_movement"]["support_metadata"], 6,
        "nonplayer result must reach the active player's retained metadata"
    );
    assert_eq!(
        saved["actors"][index]["player_movement"]["support_ticks"], 0,
        "nonplayer query must not reset the player timer"
    );
    let npc_empty = world
        .prepare_ordinary_collision_query(npc, &scene, &data, [10, 30], &[])
        .unwrap();
    assert!(
        world
            .query_ordinary_ground(
                npc,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &npc_empty,
                    distance: 4,
                    sidedness: false,
                }
            )
            .unwrap()
            .is_none()
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][index]["player_movement"]["support_metadata"],
        6
    );
    assert_eq!(
        saved["actors"][index]["player_movement"]["support_ticks"],
        0
    );
    assert!(
        world
            .query_ordinary_ground(
                player,
                OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &empty,
                    distance: 4,
                    sidedness: false
                }
            )
            .unwrap()
            .is_none()
    );
    let saved = movement_snapshot(&world, &scripts, &data);
    assert_eq!(
        saved["actors"][index]["player_movement"]["support_ticks"],
        12
    );
}
