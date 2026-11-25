//! Receiver-owned accepted directional word uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::JumpMotion;
use cyrus_script::script::{ScriptInputs, ScriptWorld};
use cyrus_soup::Attribute;
use cyrus_soup::program::*;
use std::collections::BTreeMap;

mod support;
use support::advance;
use support::counter_scripts;
use support::ordinary_collision_scene;

#[test]
fn action_routes_full_signed_words_and_preserves_other_motion_state() {
    for (value, expected) in [
        (0, 0),
        (-1, -1),
        (256, 256),
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::Action, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["actions"] = serde_json::json!({"pending": 0, "accepted": expected as u32});
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .actions
                    .accepted,
                0
            );
        }
    }
}

#[test]
fn action_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::Action, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::Action,
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
                "action control owner unavailable"
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
    let mut scripts = counter_scripts(cyrus_soup::Property::Action, Object::Me, seed, true);
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.instances[0].active = 1;
    scripts.instances[0].map_position = [600, 20, 600];
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.attributes[Attribute::Collide] = 4;
    actor.attributes[Attribute::FallHold] = 1;
    let timer_write = Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 295,
        },
        formula: Formula {
            first: Value::Integer { value: 36 },
            rest: vec![],
        },
    };
    let mut old = std::mem::take(&mut actor.program.steps);
    let pending_write = Operation::Assign {
        destination: Place::Property {
            object: Object::Me,
            reference: 296,
        },
        formula: Formula {
            first: Value::Integer { value: 512 },
            rest: vec![],
        },
    };
    actor.program.steps = BTreeMap::from([
        (0, old.remove(&0).unwrap()),
        (
            1,
            Step {
                next: 2,
                operation: timer_write,
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: pending_write,
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: old.remove(&1).unwrap().operation,
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: old.remove(&2).unwrap().operation,
            },
        ),
        (
            5,
            Step {
                next: 6,
                operation: Operation::End { target: 3 },
            },
        ),
    ]);
    scripts.definition.references.extend([
        cyrus_soup::Property::ActionTime,
        cyrus_soup::Property::ActionFlag,
    ]);
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
fn accepted_bits_are_consumed_before_pending_promotion_and_high_bits_merge() {
    for (seed, cleared, merged) in [
        (257, 256, 768),
        (-255, -256, -256),
        (i32::MIN + 1, i32::MIN, i32::MIN + 512),
    ] {
        let (mut world, data, scene) = automatic_fixture(seed);
        assert_eq!(
            world.actors()[0].control_state().unwrap().actions,
            [512, seed as u32]
        );
        for (ticks, pending, timer, accepted) in [(0, 512, 36, cleared), (1, 0, 0, merged)] {
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
            let c = world.actors()[0].control_state().unwrap();
            assert_eq!(c.actions, [pending, accepted as u32]);
            assert_eq!(c.jump_window, timer);
            assert_eq!(c.queued_motion, JumpMotion::NONE);
            advance(&mut world, 0).unwrap();
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [accepted]);
            assert_eq!(world.flags(), [73]);
        }
    }
}

#[test]
fn missing_scene_preserves_distinct_accepted_pending_words_and_blocks_continuation() {
    let (mut world, data, _) = automatic_fixture(257);
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
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.actions, [512, 257]);
    assert_eq!(c.jump_window, 36);
    assert!(advance(&mut world, 0).is_err());
    assert_eq!(world.flags(), [73]);
}
