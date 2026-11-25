//! Receiver-owned collision-response low byte uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::surface::Surface;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use std::collections::BTreeMap;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn last_collide_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::LastCollide, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["last_collide"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .last_collide
                    .bits() as u8,
                0
            );
        }
    }
}

#[test]
fn last_collide_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::LastCollide, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::LastCollide,
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
                "last_collide control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn response_fixture(value: i32, water_mark: bool) -> ScriptWorld {
    let mut scripts = counter_scripts(cyrus_soup::Property::LastCollide, Object::Me, value, true);
    scripts
        .definition
        .references
        .push(cyrus_soup::Property::LastCollideCount);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    let mut old = std::mem::take(&mut actor.program.steps);
    actor.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Assign {
                    destination: Place::Property {
                        object: Object::Me,
                        reference: 295,
                    },
                    formula: Formula {
                        first: Value::Integer { value: 37 },
                        rest: vec![],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: old.remove(&0).unwrap().operation,
            },
        ),
        (
            2,
            Step {
                next: 3,
                operation: old.remove(&1).unwrap().operation,
            },
        ),
        (
            3,
            Step {
                next: 4,
                operation: old.remove(&2).unwrap().operation,
            },
        ),
        (
            4,
            Step {
                next: 5,
                operation: Operation::End { target: 2 },
            },
        ),
    ]);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    if water_mark {
        world.set_actor_surface(id, Surface::UNKNOWN).unwrap();
        assert!(world.retain_ordinary_water_position(id).unwrap());
        let rollback = world
            .capture_ordinary_rollback(id, &mut |_, _| Ok(()))
            .unwrap();
        world.set_actor_surface(id, Surface::WATER).unwrap();
        assert!(
            world
                .rollback_ordinary_water(id, None, &rollback, &mut |_, _| Ok(()))
                .unwrap()
        );
    }
    advance(&mut world, 0).unwrap();
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.last_collide.bits() as u8, value as u8);
    assert_eq!(
        c.last_collide.bits().wrapping_shr(16) as u8,
        u8::from(water_mark)
    );
    assert_eq!(c.last_collide_count, 37);
    assert_eq!(c.last_collide.bits() & 0xff00_ff00, 0);
    world
}

#[test]
fn wrapper_distinguishes_low_byte_from_independent_transition_and_age() {
    for (value, marked, age) in [
        (0, false, 0),
        (128, false, 38),
        (255, false, 38),
        (0, true, 38),
        (128, true, 38),
    ] {
        let mut world = response_fixture(value, marked);
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap();
        let c = world.actors()[0].control_state().unwrap();
        assert_eq!(c.last_collide.bits() as u8, 0);
        assert_eq!(c.last_collide.bits().wrapping_shr(16) as u8, 0);
        assert_eq!(c.last_collide.bits() & 0xff00_ff00, 0);
        assert_eq!(c.last_collide_count, age);
        advance(&mut world, 0).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn missing_response_clock_retains_low_byte_and_independent_transition_until_retry() {
    let mut world = response_fixture(128, true);
    let id = world.id(0).unwrap();
    let error = world.finish_ordinary_script_response(id, None).unwrap_err();
    assert!(error.to_string().contains("requires nominal_ticks"));
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.last_collide.bits() as u8, 128);
    assert_eq!(c.last_collide.bits().wrapping_shr(16) as u8, 1);
    assert_eq!(c.last_collide_count, 37);
    world.finish_ordinary_script_response(id, Some(1)).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide_count,
        38
    );
}
