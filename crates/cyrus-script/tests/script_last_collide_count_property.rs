//! Receiver-owned collision-response age uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_assets::surface::Surface;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn last_collide_count_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts =
                counter_scripts(cyrus_soup::Property::LastCollideCount, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["last_collide_count"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .last_collide_count,
                0
            );
        }
    }
}

#[test]
fn last_collide_count_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(
        cyrus_soup::Property::LastCollideCount,
        Object::Me,
        99,
        false,
    );
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::LastCollideCount,
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
                "last_collide_count control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn response_fixture(seed: i32, marked: bool) -> ScriptWorld {
    let mut scripts = counter_scripts(
        cyrus_soup::Property::LastCollideCount,
        Object::Me,
        seed,
        true,
    );
    scripts
        .actors
        .get_mut("LOGIC")
        .unwrap()
        .program
        .steps
        .get_mut(&3)
        .unwrap()
        .operation = Operation::End { target: 1 };
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    advance(&mut world, 0).unwrap();
    let id = world.id(0).unwrap();
    if marked {
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
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .last_collide
                .bits()
                .wrapping_shr(16) as u8,
            1
        );
    }
    world
}

#[test]
fn ordinary_wrapper_consumes_property_age_after_real_water_response() {
    for (seed, ticks, expected) in [
        (0, 1, 1),
        (198, 1, 199),
        (199, 2, 201),
        (199, 100, 43),
        (200, 100, 200),
        (255, 1, 255),
        (199, 256, 199),
    ] {
        let mut world = response_fixture(seed, true);
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    nominal_ticks: Some(ticks),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                None,
                None,
                30,
            )
            .unwrap();
        let c = world.actors()[0].control_state().unwrap();
        assert_eq!(c.last_collide_count, expected);
        assert_eq!(c.last_collide.bits().wrapping_shr(16) as u8, 0);
        assert_eq!(c.last_collide.bits() as u8, 0);
        assert_eq!(c.last_collide.bits() & 0xff00_ff00, 0);
        assert_eq!(c.overlap_contact_identifier, 0);
        assert_eq!(c.actions, [0, 0]);
        advance(&mut world, 0).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [i32::from(expected)]
        );
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn zero_record_resets_even_saturated_age_and_no_ticks_failure_retains_marks() {
    let mut world = response_fixture(255, false);
    let id = world.id(0).unwrap();
    world.finish_ordinary_script_response(id, None).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    let mut world = response_fixture(199, true);
    let id = world.id(0).unwrap();
    let error = world.finish_ordinary_script_response(id, None).unwrap_err();
    assert!(error.to_string().contains("requires nominal_ticks"));
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide_count,
        199
    );
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits()
            .wrapping_shr(16) as u8,
        1
    );
    world.finish_ordinary_script_response(id, Some(2)).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [201]);
    assert_eq!(
        world.actors()[0]
            .control_state()
            .unwrap()
            .last_collide
            .bits()
            .wrapping_shr(16) as u8,
        0
    );
}
