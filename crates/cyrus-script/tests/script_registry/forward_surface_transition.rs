//! The selected player's forward probes run the water and lava transition handler.
use super::*;
use cyrus_collision::ground::{SupportHistory, TerrainGround};
use cyrus_script::script::{OrdinaryGroundQuery, ScriptActorSource, ScriptSound};

#[test]
fn forward_probe_into_deep_water_requests_the_swim_group_and_plays_the_splash() {
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
