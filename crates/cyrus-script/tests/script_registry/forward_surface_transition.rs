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
