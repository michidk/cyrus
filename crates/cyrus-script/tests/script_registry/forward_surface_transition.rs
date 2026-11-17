//! The selected player's forward probes run the water and lava transition handler.
use super::*;
use cyrus_collision::ground::{SupportHistory, TerrainGround};
use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource, ScriptSound};

#[test]
fn forward_probe_retains_water_transition_after_rejected_owner_and_flushes_once() {
    let (_, mut world, mut data, mut scene) = player_walking_fixture();
    // Texture 25 on the terrain resolves to deep water; the terrain surface sits inside the probe.
    scene.collision.texture_surfaces = Some(BTreeMap::from([(
        (302 << 7) | 25,
        cyrus_assets::surface::Surface::DEEPWATER,
    )]));
    scene.terrain = Some(
        TerrainGround::new(
            vec![18; 256 * 256],
            vec![false; 256 * 256],
            Some(vec![25; 256 * 256]),
        )
        .unwrap(),
    );
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
    for group in [142, 143] {
        definition["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "group_index":group,"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
            }));
    }
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    world
        .place_selected_player([512 * 256, -1536, 512 * 256], 0)
        .unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [512, 512], &[player, map])
        .unwrap();
    let before = world.spatial(player).unwrap().local.position;
    let mut history = SupportHistory::new(0, 0);
    // The probe no longer refuses a water transition; it queues it for the caller's sinks.
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
            false,
        )
        .unwrap();
    assert_eq!(
        world.actor_surface(player).unwrap(),
        cyrus_assets::surface::Surface::DEEPWATER
    );
    // An idempotent selection is not retirement and must preserve the queued transition.
    assert_eq!(world.select_player(false).unwrap(), player);
    let retired = cyrus_script::script::ScriptActorId {
        generation: player.generation.wrapping_add(1),
        index: player.index,
    };
    let error = world
        .flush_surface_transitions(
            retired,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| panic!("retired owner must not publish a transition"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("stale"), "{error:#}");
    let error = world
        .flush_surface_transitions(
            map,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| panic!("nonplayer must not publish the selected player's transition"),
        )
        .expect_err("water transition queue requires its selected player");
    assert!(error.to_string().contains("selected player"), "{error:#}");
    let mut sounds = vec![];
    world
        .flush_surface_transitions(
            player,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, sound| {
                sounds.push(sound);
                Ok(())
            },
        )
        .unwrap();
    assert!(matches!(
        sounds[..],
        [ScriptSound::Positional {
            effect: 71,
            volume: 0,
            ..
        }]
    ));
    // The queue is drained: a second flush does nothing.
    world
        .flush_surface_transitions(
            player,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| panic!("transition run twice"),
        )
        .unwrap();
    // Another probe belongs to CYRUS, even if the public selector changes before dispatch.
    world
        .set_actor_surface(player, cyrus_assets::surface::Surface::SAND)
        .unwrap();
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
            false,
        )
        .unwrap();
    let alternate = world.select_player(true).unwrap();
    world
        .flush_surface_transitions(
            alternate,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| {
                panic!("replacement player inherited the retired selection's water transition")
            },
        )
        .unwrap();
    world.select_player(false).unwrap();
    world
        .flush_surface_transitions(
            player,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| panic!("retired selection's transition returned after selecting it again"),
        )
        .unwrap();
}

/// The support handler publishes a splash at the actor before the accepted walk translates it.
#[test]
fn walking_water_entry_keeps_query_time_sound_and_effect_positions() {
    let (_, mut world, mut data, mut scene) = player_walking_fixture();
    // Texture 25 on the terrain resolves to deep water; the terrain surface sits inside the probe.
    scene.collision.texture_surfaces = Some(BTreeMap::from([(
        (302 << 7) | 25,
        cyrus_assets::surface::Surface::DEEPWATER,
    )]));
    scene.terrain = Some(
        TerrainGround::new(
            vec![18; 256 * 256],
            vec![false; 256 * 256],
            Some(vec![25; 256 * 256]),
        )
        .unwrap(),
    );
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
    for group in [AnimationGroup::SWIM_FORWARD, AnimationGroup::SWIM_BACKWARD] {
        definition["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "group_index":group.index(),"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
            }));
    }
    let player = world.id(index).unwrap();
    let map = world.id(0).unwrap();
    world
        .place_selected_player([512 * 256, -1536, 512 * 256 + 240], 0)
        .unwrap();
    let prepared = world
        .prepare_ordinary_collision_query(player, &scene, &data, [512, 512], &[player, map])
        .unwrap();
    let before = world.spatial(player).unwrap().local.position;
    world
        .initialize_camera_pose(cyrus_script::script::camera::CameraPose {
            position: [before[0], 0, before[2] - 100 * 256],
            angles: [0; 3],
        })
        .unwrap();
    world
        .prepare_environment_effects(&super::water_model_tests::effects())
        .unwrap();
    world.begin_player_movement_update(player, 0).unwrap();
    let held = cyrus_script::player::input::PlayerInput {
        forward: true,
        ..Default::default()
    };
    let mut positions = vec![];
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
                nominal_ticks: 1,
                fine_ticks: 0,
            },
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
            &mut |owner, sound| {
                assert_eq!(owner, player);
                if let ScriptSound::Positional {
                    effect: 71,
                    position,
                    ..
                } = sound
                {
                    positions.push(position);
                }
                Ok(())
            },
        )
        .unwrap();
    let after = world.spatial(player).unwrap().local.position;
    assert_ne!(after, before, "fixture must accept walking displacement");
    assert_eq!(
        positions,
        [before],
        "entry splash belongs to the support query, before walking from {before:?} to {after:?}"
    );
    let effects: Vec<_> = world.environmental_effects().unwrap().active().collect();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].1.descriptor, 0x2500);
    // The camera is directly behind the probe: the splash starts eight map units toward it.
    // Start just below a map-unit boundary so the later walk changes the visible integer position.
    assert_eq!(effects[0].1.position[0], before[0] >> 8);
    assert_eq!(effects[0].1.position[2], (before[2] >> 8) - 8);
}

/// Deferred probes must retain each transition's surface instead of reading the final one twice.
#[test]
fn queued_water_entry_and_exit_keep_their_own_surface_selection() {
    use cyrus_assets::surface::Surface;
    let (_, mut world, mut data, mut scene) = player_walking_fixture();
    scene.terrain = Some(
        TerrainGround::new(
            vec![18; 256 * 256],
            vec![false; 256 * 256],
            Some(vec![25; 256 * 256]),
        )
        .unwrap(),
    );
    let definition = data.metadata["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|actor| actor["script_name"] == "CYRUS")
        .unwrap();
    for group in [
        AnimationGroup::SWIM_FORWARD,
        AnimationGroup::SWIM_BACKWARD,
        AnimationGroup::RUN,
        AnimationGroup::RUN_BACKWARD,
    ] {
        definition["animation_groups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "group_index":group.index(),"anim_id":1,"anim_type":0,
                "commands":[{"opcode":0,"frame_reference":0},{"opcode":1}]
            }));
    }
    let player = world.selected_player().unwrap();
    let map = world.id(0).unwrap();
    world
        .place_selected_player([512 * 256, -1536, 512 * 256], 0)
        .unwrap();
    let before = world.spatial(player).unwrap().local.position;
    let mut history = SupportHistory::new(0, 0);
    for surface in [Surface::DEEPWATER, Surface::SAND] {
        scene.collision.texture_surfaces = Some(BTreeMap::from([((302 << 7) | 25, surface)]));
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [512, 512], &[player, map])
            .unwrap();
        world
            .apply_ordinary_directional_motion(player, 2, 0)
            .unwrap();
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
                false,
            )
            .unwrap();
        assert_eq!(world.actor_surface(player).unwrap(), surface);
    }
    // Later motion must not change either probe's already selected forward animation.
    world
        .apply_ordinary_directional_motion(player, 3, 0)
        .unwrap();
    let mut effects = vec![];
    world
        .flush_surface_transitions(
            player,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |owner, sound| {
                assert_eq!(owner, player);
                if let ScriptSound::Positional { effect, .. } = sound {
                    effects.push(effect);
                }
                Ok(())
            },
        )
        .unwrap();
    // Preservation player.md: entering deep water selects 71, leaving it selects 70.
    assert_eq!(effects, [71, 70]);
    assert_eq!(
        world.inspect(player.index, 1)["actors"][0]["animation"]["current_group"],
        AnimationGroup::RUN.index()
    );
    assert_eq!(world.actor_surface(player).unwrap(), Surface::SAND);
    world
        .flush_surface_transitions(
            player,
            1,
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| panic!("transition published twice"),
        )
        .unwrap();
}

/// A sidestep onto a different surface queues the transition like a forward probe, and it needs
/// the shared engine random stream to flush it; without one the step fails and halts the world.
#[test]
fn sidestep_onto_a_new_surface_flushes_the_transition_with_the_shared_random_stream() {
    use cyrus_script::script::{LocomotionGeometry, ObstacleEnvironment, ScriptInputs};
    use cyrus_soup::Native;
    for (function, with_random) in [
        (Native::MoveLeft, true),
        (Native::MoveRight, true),
        (Native::MoveRight, false),
    ] {
        let (_, mut world, data, mut scene) = player_walking_fixture();
        scene.collision.texture_surfaces = Some(BTreeMap::from([(
            (302 << 7) | 25,
            cyrus_assets::surface::Surface::SAND,
        )]));
        scene.terrain = Some(
            TerrainGround::new(
                vec![18; 256 * 256],
                vec![false; 256 * 256],
                Some(vec![25; 256 * 256]),
            )
            .unwrap(),
        );
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let player = world.id(index).unwrap();
        let map = world.id(0).unwrap();
        world
            .place_selected_player([512 * 256, -1536, 512 * 256], 0)
            .unwrap();
        let prepared = world
            .prepare_ordinary_collision_query(player, &scene, &data, [512, 512], &[player, map])
            .unwrap();
        let hud = cyrus_script::script::ScriptHud::default();
        world.begin_player_movement_update(player, 0).unwrap();
        let inputs = ScriptInputs {
            hud: Some(&hud),
            shell_execution: Some(false),
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            locomotion_geometry: Some(LocomotionGeometry {
                ground: OrdinaryGroundQuery {
                    scene: &scene,
                    animations: &data,
                    prepared: &prepared,
                    distance: 4,
                    sidedness: false,
                },
                environment: ObstacleEnvironment {
                    selected_player: Some(player),
                    auto_grab: false,
                    item_system_ready: false,
                    shell_execution: false,
                    ticks: 1,
                },
            }),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::default();
        let result = world.run_player_native_hook(player, &mut |world| {
            world.step_player(
                player,
                function,
                inputs,
                &mut |_, _| Ok(()),
                with_random.then_some(&mut random),
            )
        });
        if with_random {
            result.unwrap_or_else(|error| panic!("{function}: {error:#}"));
        } else {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("surface transition random owner unavailable"),
                "{error}"
            );
        }
    }
}
