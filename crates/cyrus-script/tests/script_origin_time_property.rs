//! Receiver-owned vertical-motion timer uses public property dispatch.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::ScriptWorld;
use cyrus_soup::Attribute;
use cyrus_soup::program::*;

mod support;
use support::advance;
use support::counter_scripts;

#[test]
fn origin_time_routes_low_bytes_and_preserves_other_motion_state() {
    for (value, expected) in [(0, 0), (48, 48), (207, 207), (-1, 255), (258, 2), (256, 0)] {
        for (object, receiver) in [(Object::Me, 1), (Object::Named { string: 0 }, 0)] {
            let scripts = counter_scripts(cyrus_soup::Property::OriginTime, object, value, true);
            let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
            let mut before =
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap();
            advance(&mut world, 1).unwrap();
            assert_eq!(world.actors()[1].runtime().unwrap().variables(), [expected]);
            assert_eq!(world.flags(), [73]);
            before["fall_timer"] = expected.into();
            assert_eq!(
                serde_json::to_value(world.actors()[receiver].control_state().unwrap()).unwrap(),
                before
            );
            assert_eq!(
                world.actors()[1 - receiver]
                    .control_state()
                    .unwrap()
                    .fall_timer,
                0
            );
        }
    }
}

#[test]
fn origin_time_reads_zero_and_rejects_absent_owners_before_continuation() {
    let scripts = counter_scripts(cyrus_soup::Property::OriginTime, Object::Me, 99, false);
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [0]);
    for write in [false, true] {
        for absent_receiver in [false, true] {
            let mut scripts = counter_scripts(
                cyrus_soup::Property::OriginTime,
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
                "origin_time control owner unavailable"
            }));
            assert_eq!(world.flags(), [12]);
            assert_eq!(world.actors()[0].runtime().unwrap().variables(), [4]);
            let failure = world.actors()[0].failure().unwrap().to_owned();
            assert!(advance(&mut world, 0).is_err());
            assert_eq!(world.actors()[0].failure(), Some(failure.as_str()));
        }
    }
}

fn falling_fixture(
    timer: i32,
    start_fall: bool,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = counter_scripts(cyrus_soup::Property::OriginTime, Object::Me, timer, true);
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.attributes[Attribute::JumpHeight] = 16;
    actor.program.steps.get_mut(&3).unwrap().operation = Operation::End { target: 1 };
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","model_load_path":"none"}]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    if start_fall {
        assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
    }
    advance(&mut world, 0).unwrap();
    (world, data)
}

fn environment() -> cyrus_script::script::ObstacleEnvironment {
    cyrus_script::script::ObstacleEnvironment {
        selected_player: None,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    }
}

#[test]
fn unsigned_property_controls_signed_falling_timer_and_real_tick_progress() {
    for (seed, expected_y, after) in [
        (30, 0, 22),
        (1, 3584, 249),
        (0, 2560, 0),
        (128, 2560, 128),
        (-1, 2560, 255),
    ] {
        let (mut world, data) = falling_fixture(seed, true);
        let id = world.id(0).unwrap();
        assert_eq!(
            world.actors()[0].runtime().unwrap().variables(),
            [seed & 255]
        );
        assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
        assert!(
            world
                .continue_ordinary_falling(id, &data, environment(), 8, 0, 50, &mut |_, _| panic!(
                    "static falling dispatched sound"
                ))
                .unwrap()
        );
        advance(&mut world, 0).unwrap();
        assert_eq!(world.actors()[0].runtime().unwrap().variables(), [after]);
        assert_eq!(
            world.spatial(id).unwrap().local.position,
            [0, expected_y, 0]
        );
        assert_eq!(world.actors()[1].control_state().unwrap().fall_timer, 0);
        assert_eq!(world.actors()[0].control_state().unwrap().extra_time, 0);
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn a_timer_write_does_not_start_falling_and_existing_fall_refuses_reinitialization() {
    let (mut world, data) = falling_fixture(7, false);
    let id = world.id(0).unwrap();
    assert!(
        !world
            .continue_ordinary_falling(id, &data, environment(), 8, 0, 50, &mut |_, _| Ok(()))
            .unwrap()
    );
    assert_eq!(world.spatial(id).unwrap().local.position, [0, 0, 0]);
    assert!(world.begin_static_ordinary_fall(id, &data).unwrap());
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [30]);
    let (mut world, data) = falling_fixture(7, true);
    let id = world.id(0).unwrap();
    assert!(!world.begin_static_ordinary_fall(id, &data).unwrap());
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [7]);
}

#[test]
fn unsupported_factor_is_rejected_by_the_consumer_without_timer_or_position_writes() {
    let (mut world, data) = falling_fixture(70, true);
    let id = world.id(0).unwrap();
    let before = world.spatial(id).unwrap().local.position;
    let error = world
        .continue_ordinary_falling(id, &data, environment(), 8, 0, 50, &mut |_, _| Ok(()))
        .unwrap_err();
    assert!(format!("{error:#}").contains("factor index 70"));
    assert_eq!(world.spatial(id).unwrap().local.position, before);
    advance(&mut world, 0).unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [70]);
    assert_eq!(world.flags(), [73]);
}
