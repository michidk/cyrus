use super::*;
use cyrus_assets::effects::{EffectImage, Effects};
use cyrus_game::script::{
    ScriptActorSource, ScriptHud, ScriptInputs, ScriptStage, camera::CameraPose,
};

pub(super) fn effects() -> Effects {
    Effects {
        ambient_brightness: Some(128),
        capacity: 8,
        disabled: false,
        images: BTreeMap::from([(
            0x2500,
            EffectImage {
                name: "synthetic-water".into(),
                size: [1, 4],
                texture_scale: 256,
                delay_ms: 71,
                flags: 1,
                alpha: 151,
                frames: vec![vec![255; 16]; 4],
            },
        )]),
    }
}

#[test]
fn water_models_use_installed_depth_and_strict_prior_height_effect_threshold() {
    for (surface, group, depth, previous, expected, spawn) in [
        (1, 141, Some(-12), 0.0, 58.0, false),
        (2, 140, None, 74.9, 58.0, false),
        (2, 140, None, 75.0, 58.0, true),
        (2, 141, Some(-12), 46.0, 46.0, false),
        (2, 143, Some(40), 98.0, 98.0, false),
        (2, 144, None, 58.0, 58.0, false),
    ] {
        let (scripts, world, mut data, scene) = player_walking_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        data.metadata["actors"].as_array_mut().unwrap().iter_mut().find(|actor| actor["script_name"] == "CYRUS").unwrap()["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, group, |saved| {
                saved["actors"][index]["model"]["current"] = serde_json::Value::Null;
                saved["actors"][index]["model"]["position"] = serde_json::json!([
                    10.0_f32.to_bits(),
                    (previous as f32).to_bits(),
                    30.0_f32.to_bits()
                ]);
            });
        let player = world.id(index).unwrap();
        world
            .prepare_ordinary_collision_shape(player, &data, &scene)
            .unwrap();
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
        let mut scene = scene;
        scene.settings.swim_depth = depth;
        world
            .prepare_model_admission(&Default::default(), scene.collision.placements.len())
            .unwrap();
        world.set_actor_surface(player, surface).unwrap();
        world
            .initialize_camera_pose(CameraPose {
                position: [0; 3],
                angles: [0; 3],
            })
            .unwrap();
        world.prepare_environment_effects(&effects()).unwrap();
        let logical = world.spatial(player).unwrap().local.position;
        let hud = ScriptHud::default();
        let mut random = cyrus_game::random::EngineRandom::from_state(42);
        world
            .refresh_automatic_models(
                &data,
                logical,
                Some(&scene),
                ScriptInputs {
                    hud: Some(&hud),
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
        assert_eq!(
            world.model_position(player).unwrap(),
            Some([10.0, expected, 30.0])
        );
        assert_eq!(
            world.model_presentation(player).unwrap().unwrap().position,
            [10.0, expected, 30.0]
        );
        assert_eq!(world.spatial(player).unwrap().local.position, logical);
        assert_eq!(
            world.environmental_effects().unwrap().active().count(),
            usize::from(spawn)
        );
        if spawn {
            let effect = world
                .environmental_effects()
                .unwrap()
                .active()
                .next()
                .unwrap()
                .1;
            assert_eq!(effect.descriptor, 0x2500);
            // Logical Y -6, positive extent 24, helper offset 6, texture centering -2.
            assert_eq!(effect.position[1], 22);
        }
        assert_eq!(random.state(), 42);
    }
}

#[test]
fn selected_water_prepass_clears_direction_and_uses_null_source_death() {
    for surface in 1..=4 {
        let (scripts, world, mut data, mut scene) = player_walking_fixture_with(|scripts| {
            scripts.definition.attributes[105] = "skip_death".into();
            scripts
                .actors
                .get_mut("CYRUS")
                .unwrap()
                .attributes
                .insert("skip_death".into(), 1);
        });
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
                saved["actors"][index]["control"]["directional_state"] =
                    serde_json::json!([129, 2]);
            });
        let player = world.id(index).unwrap();
        world
            .prepare_ordinary_collision_shape(player, &data, &scene)
            .unwrap();
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
        world
            .prepare_model_admission(&Default::default(), scene.collision.placements.len())
            .unwrap();
        world.set_actor_surface(player, surface).unwrap();
        scene.terrain = Some(
            cyrus_collision::ground::TerrainGround::new(
                vec![0; 256 * 256],
                vec![false; 256 * 256],
                Some(vec![0; 256 * 256]),
            )
            .unwrap(),
        );
        scene.collision.texture_surfaces = Some(Default::default());
        world
            .initialize_camera_pose(CameraPose {
                position: [0; 3],
                angles: [0; 3],
            })
            .unwrap();
        world.prepare_environment_effects(&effects()).unwrap();
        // No interpreter entry establishes native source; the prepass must supply null.
        world
            .refresh_automatic_models(
                &data,
                [0; 3],
                Some(&scene),
                ScriptInputs {
                    hud: Some(&hud),
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
        assert_eq!(
            world.actors()[index]
                .control_state()
                .unwrap()
                .directional_state,
            [0; 2]
        );
        assert_eq!(
            world.actors()[index].attributes()[3],
            if matches!(surface, 2 | 4) { 0 } else { 50 }
        );
        assert_eq!(world.model_position(player).unwrap().unwrap()[1], 58.0);
        if matches!(surface, 2 | 4) {
            assert!(world.globals().selected_health().unwrap().hit_inhibited());
            assert_eq!(world.environmental_effects().unwrap().active().count(), 0);
        }
    }
}

#[test]
fn shoreline_models_sample_stored_map_units_and_double_partial_coverage() {
    for (surface, map_x, expected_y) in [
        (3, 58, -5.0_f32),
        (4, 128, 57.0),
        (3, 130, 58.0),
        (4, 10, -6.0),
    ] {
        let (scripts, world, mut data, mut scene) = player_walking_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, 0, |saved| {
                saved["spatial"]["actors"][index]["local"]["position"] =
                    serde_json::json!([map_x * 256 + 255, -1536, 7680]);
                saved["actors"][index]["model"]["current"] = serde_json::Value::Null;
                saved["actors"][index]["model"]["position"] =
                    serde_json::json!([0, expected_y.to_bits(), 0]);
            });
        let player = world.id(index).unwrap();
        world
            .prepare_ordinary_collision_shape(player, &data, &scene)
            .unwrap();
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
        world
            .prepare_model_admission(&Default::default(), scene.collision.placements.len())
            .unwrap();
        world.set_actor_surface(player, surface).unwrap();
        scene.terrain = Some(
            cyrus_collision::ground::TerrainGround::new(
                vec![0; 256 * 256],
                vec![false; 256 * 256],
                Some(vec![6; 256 * 256]),
            )
            .unwrap(),
        );
        scene.collision.texture_surfaces = Some(Default::default());
        world
            .refresh_automatic_models(
                &data,
                [0; 3],
                Some(&scene),
                ScriptInputs::default(),
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert!(
            world.actors()[index].failure().is_none(),
            "{:?}",
            world.actors()[index].failure()
        );
        assert_eq!(
            world.model_position(player).unwrap(),
            Some([map_x as f32, expected_y, 30.0])
        );
    }
}

#[test]
fn water_models_reject_missing_depth_mismatched_scene_and_unknown_prior_height() {
    for (group, unknown_previous, wrong_world, expected) in [
        (141, false, false, "swim_depth unavailable"),
        (141, false, true, "water model scene belongs to world"),
        (140, true, false, "prior coordinates unavailable"),
    ] {
        let (scripts, world, mut data, mut scene) = player_walking_fixture();
        let index = world
            .actors()
            .iter()
            .position(|actor| matches!(actor.source, ScriptActorSource::Player))
            .unwrap();
        data.metadata["actors"].as_array_mut().unwrap().iter_mut().find(|actor|actor["script_name"]=="CYRUS").unwrap()["animation_groups"].as_array_mut().unwrap().push(serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}));
        let mut world =
            replace_health_test_animation(world, &scripts, &mut data, index, group, |saved| {
                if unknown_previous {
                    saved["actors"][index]["model"]["current"] = serde_json::Value::Null;
                    saved["actors"][index]["model"]
                        .as_object_mut()
                        .unwrap()
                        .remove("position");
                }
            });
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
        world
            .prepare_model_admission(&Default::default(), scene.collision.placements.len())
            .unwrap();
        world.set_actor_surface(player, 2).unwrap();
        if wrong_world {
            scene.world += 1;
        }
        world
            .refresh_automatic_models(
                &data,
                [0; 3],
                Some(&scene),
                ScriptInputs::default(),
                None,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert!(
            world.actors()[index].failure().unwrap().contains(expected),
            "{:?}",
            world.actors()[index].failure()
        );
    }
}
