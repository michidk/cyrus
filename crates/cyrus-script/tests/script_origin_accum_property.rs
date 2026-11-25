//! Receiver-owned accumulated displacement uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::{ScriptInputs, ScriptWorld};
use cyrus_soup::Attribute;
use cyrus_soup::program::*;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn origin_accum_routes_full_signed_words_and_preserves_other_motion_state() {
    for (value, expected) in [
        (0, 0),
        (-1, -1),
        (256, 256),
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
    ] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::OriginAccum, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["sliding_distance"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .sliding_distance,
                0
            );
        }
    }
}

#[test]
fn origin_accum_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::OriginAccum, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::OriginAccum,
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
                "origin_accum control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn displacement_fixture(
    seed: i32,
    divisor: u8,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = counter_scripts(cyrus_soup::Property::OriginAccum, Object::Me, seed, true);
    scripts
        .definition
        .references
        .push(cyrus_soup::Property::JumpFlag);
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.instances[0].active = 1;
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.attributes[Attribute::JumpTime] = divisor;
    actor.program.steps.get_mut(&0).unwrap().next = 4;
    actor.program.steps.insert(
        4,
        Step {
            next: 1,
            operation: Operation::Assign {
                destination: Place::Property {
                    object: Object::Me,
                    reference: 295,
                },
                formula: Formula {
                    first: Value::Integer { value: 8 },
                    rest: vec![],
                },
            },
        },
    );
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 1 };
    if actor.program.steps.contains_key(&4) {
        let mut steps = std::mem::take(&mut actor.program.steps);
        actor.program.steps = [0, 4, 1, 2, 3]
            .into_iter()
            .enumerate()
            .map(|(offset, old)| {
                let mut step = steps.remove(&old).unwrap();
                step.next = offset as u32 + 1;
                if let Operation::End { target } = &mut step.operation {
                    *target = 2;
                }
                (offset as u32, step)
            })
            .collect();
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{"script_name":"LOGIC","model_load_path":"none"}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    advance(&mut world, 0).unwrap();
    (world, data)
}

#[test]
fn real_automatic_displacement_consumes_seed_and_the_update_completes() {
    for (seed, scale, after, z) in [
        (0, 256, 1024, 1024),
        (98300, 256, 98304, 4),
        (98304, 256, 98304, 0),
        (-1024, 256, 0, 1024),
        (i32::MAX, 256, i32::MAX, 0),
        (i32::MIN, u32::MAX, 98304, 0),
    ] {
        let (mut world, data) = displacement_fixture(seed, 64);
        let id = world.id(0).unwrap();
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
        world
            .update_ordinary(
                &data,
                [0, 0],
                cyrus_script::script::OrdinaryInputs {
                    scene: None,
                    scripts: ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(scale),
                        ..Default::default()
                    },
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                20,
            )
            .unwrap();
        assert_eq!(
            world.actors()[0].control_state().unwrap().sliding_distance,
            after
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, z]);
        // The collision attribute has no bit 0x04, so the queued motion is never resolved and the
        // update completes (preservation player.md, "Stable ordinary position").
        assert!(world.actors()[0].failure().is_none());

        assert_eq!(world.actors()[0].control_state().unwrap().fall_timer, 0);
        advance(&mut world, 0).unwrap();
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn missing_jump_divisor_fails_before_accumulator_or_position_changes() {
    let (mut world, data) = displacement_fixture(17, 0);
    let id = world.id(0).unwrap();
    world
        .update_ordinary(
            &data,
            [0, 0],
            cyrus_script::script::OrdinaryInputs {
                scene: None,
                scripts: ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            20,
        )
        .unwrap();
    assert!(
        world.actors()[0]
            .failure()
            .unwrap()
            .contains("jump displacement needs attr_jump_time")
    );
    assert_eq!(
        world.actors()[0].control_state().unwrap().sliding_distance,
        17
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
    assert_eq!(world.flags(), [73]);
}
