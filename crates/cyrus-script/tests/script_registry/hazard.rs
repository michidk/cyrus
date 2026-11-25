use super::*;

#[test]
fn automatic_hazard_models_publish_displacement_before_selected_death() {
    use cyrus_script::script::{ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage};
    for (surface, queued, directions) in [
        (5, 0, [0, 0]),
        (11, 0, [0, 0]),
        (5, 1, [0, 0]),
        (11, 0, [1, 0]),
        (5, 0, [0, 2]),
        (11, 0, [128, 128]),
    ] {
        let (scripts, mut world, mut data, scene) = player_walking_fixture_with(|scripts| {
            scripts.actors.get_mut("CYRUS").unwrap().attributes[Attribute::PlayerType] = 1;
        });
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        if queued != 0 || directions != [0, 0] {
            world = replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
                saved["actors"][index]["control"]["queued_motion"] = queued.into();
                saved["actors"][index]["control"]["directional_state"] =
                    serde_json::json!(directions);
            });
        }
        let player = world.id(index).unwrap();
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
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        world
            .apply_selected_contact_health(
                player,
                50,
                ScriptInputs {
                    hud: Some(&hud),
                    ..Default::default()
                },
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        // The ordinary script wrapper retains the source used by later model health.
        world
            .advance(
                world.id(0).unwrap(),
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
        world
            .prepare_model_admission(&Default::default(), scene.collision.placements.len())
            .unwrap();
        world
            .set_actor_surface(player, Surface::from_byte(surface))
            .unwrap();
        let logical = world.spatial(player).unwrap().local.position;
        world
            .refresh_automatic_models(
                &data,
                logical,
                None,
                ScriptInputs {
                    hud: Some(&hud),
                    shell_execution: Some(false),
                    death_suppressed: Some(false),
                    nominal_ticks: Some(0),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut random),
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
        assert_eq!(world.spatial(player).unwrap().local.position, logical);
        assert_eq!(
            world.model_presentation(player).unwrap().unwrap().position[1],
            ((logical[1] >> 8) + 64) as f32
        );
        assert_eq!(
            world.model_position(player).unwrap().unwrap(),
            world.model_presentation(player).unwrap().unwrap().position
        );
        let guarded = queued != 0 || directions.iter().any(|value| value & 3 != 0);
        assert_eq!(
            world.actors()[index].attributes()[Attribute::Health],
            if guarded { 50 } else { 0 }
        );
        let health = world.globals().selected_health().unwrap();
        assert_eq!(health.hit_inhibited(), !guarded);
        assert_eq!(
            health.death().unwrap().mode(),
            if surface == 5 { 1 } else { 0 }
        );
    }
}
