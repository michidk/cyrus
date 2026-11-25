//! Receiver-owned pending-action timer uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::JumpMotion;
use cyrus_script::script::test_support::ScriptWorldTestExt;
use cyrus_script::script::{ScriptInputs, ScriptWorld};
use cyrus_soup::Attribute;
use cyrus_soup::program::*;

mod support;
use support::advance;
use support::counter_scripts;
use support::ordinary_collision_scene;

#[test]
fn action_time_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::ActionTime, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["jump_window"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .jump_window,
                0
            );
        }
    }
}

#[test]
fn action_time_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::ActionTime, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::ActionTime,
                if absent_receiver {
                    Object::Player
                } else {
                    Object::Me
                },
                48,
                write,
            );
            if !absent_receiver {
                scripts.instances[0].object_type = PlacementType::SCRIPT_ONLY;
            }
            scripts.content_id = scripts.content_identity().unwrap();
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let error = advance(&mut world, 0).unwrap_err();
            assert!(format!("{error:#}").contains(if absent_receiver {
                "Player property selected player unavailable"
            } else {
                "action_time control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn automatic_fixture(
    seed: i32,
) -> (
    ScriptWorld,
    cyrus_assets::animation::Animations,
    cyrus_script::player::data::PlayerScene,
) {
    let mut scripts = counter_scripts(cyrus_soup::Property::ActionTime, Object::Me, seed, true);
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.instances[0].active = 1;
    scripts.instances[0].map_position = [600, 20, 600];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.attributes[Attribute::Collide] = 4;
    actor.attributes[Attribute::FallHold] = 1;
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 1 };
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"mpob_objects":[{"model_id":-1}],"actors":[{
            "script_name":"LOGIC","model_load_path":"static","mpsz_bounds_primary":-1,
            "raex":{"unknown_12":0,"v_vertex":0,"v_size":0}}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let models = cyrus_collision::actor_models::ActorModelCollision::new(
        &cyrus_assets::collision::ActorCollisionModels {
            object_count: 1,
            models: std::collections::BTreeMap::from([(
                "STATIC".into(),
                cyrus_assets::collision::CollisionGeometry {
                    version: Some(*b"v5.0"),
                    radius: 42.0,
                    vertices: vec![[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
                    normals: vec![[0.0, -1.0, 0.0]],
                    faces: vec![vec![0, 1, 2]],
                    textures: Some(vec![None]),
                    subobjects: vec![cyrus_assets::collision::CollisionSubobject {
                        center: [0.0; 3],
                        radius: 42.0,
                        extents: Some([1.0; 3]),
                        faces: vec![0],
                    }],
                },
            )]),
            instances: std::collections::BTreeMap::from([(0, "STATIC".into())]),
            collision_flags: std::collections::BTreeMap::from([(0, 6)]),
            unsupported: Default::default(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    world.prepare_model_resources(&data, &models).unwrap();
    advance(&mut world, 0).unwrap();
    let scene = ordinary_collision_scene(7, models);
    (world, data, scene)
}

#[test]
fn production_late_window_consumes_property_seed_at_unsigned_and_wrapping_boundaries() {
    for (seed, ticks, expected) in [
        (35, 1, 36),
        (36, 1, 0),
        (255, 1, 0),
        (255, 2, 1),
        (128, 0, 0),
        (0, 256, 0),
        (36, 256, 36),
    ] {
        let (mut world, data, scene) = automatic_fixture(seed);
        let id = world.id(0).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [seed]);
        world
            .update_ordinary(
                &data,
                [600, 600],
                cyrus_script::script::OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(ticks),
                        frame_scale: Some(256),
                        fine_ticks: Some(0),
                        step_fps: Some(12),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[0].failure().is_none(),
            "{:?}",
            world.actors()[0].failure()
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().jump_window,
            expected
        );
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .action_transition_counter,
            0
        );
        assert_eq!(
            world.actors()[0].control_state().unwrap().queued_motion,
            JumpMotion::NONE
        );
        advance(&mut world, 0).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [i32::from(expected)]
        );
        assert_eq!(world.flags(), [73]);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [153600, 5120, 153600]
        );
    }
}

#[test]
fn missing_late_collision_input_keeps_timer_before_its_consumer_and_sticks_failure() {
    let (mut world, data, _) = automatic_fixture(36);
    world
        .update_ordinary(
            &data,
            [600, 600],
            cyrus_script::script::OrdinaryInputs {
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                scene: None,
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap();
    assert!(
        world.actors()[0]
            .failure()
            .unwrap()
            .contains("ordinary collision scene unavailable")
    );
    assert_eq!(world.actors()[0].control_state().unwrap().jump_window, 36);
    assert!(advance(&mut world, 0).is_err());
    assert_eq!(world.flags(), [73]);
}

#[test]
fn a_falling_byte_in_a_ledge_state_runs_the_late_stage_without_the_ground_response() {
    // The original skips the obstacle wrapper and the ground response in living states 10 to 19
    // (the ledge transition replaces the latter), so a nonselected actor keeps its falling byte.
    // A finished climb with no animation receiver releases its living state before this stage.
    for state in (10..=19u8)
        .filter(|&state| state != cyrus_script::script::LivingState::CLIMB_FINISHED.byte())
    {
        let (mut world, data, mut scene) = automatic_fixture(0);
        scene.settings.post_collide_height = 8;
        let id = world.id(0).unwrap();
        let y = world.spatial(id).unwrap().local.position[1];
        world.set_living_state_for_test(id, cyrus_script::script::LivingState::from_byte(state));
        world.set_falling_for_test(id, JumpMotion::STANDING, y);
        world
            .update_ordinary(
                &data,
                [600, 600],
                cyrus_script::script::OrdinaryInputs {
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        fine_ticks: Some(0),
                        step_fps: Some(12),
                        ..Default::default()
                    },
                    scene: Some(&scene),
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        assert!(
            world.actors()[0].failure().is_none(),
            "living state {state}: {:?}",
            world.actors()[0].failure()
        );
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(control.living_state.byte(), state);
        assert_eq!(
            control.falling_state,
            JumpMotion::STANDING,
            "living state {state}"
        );
    }
}
