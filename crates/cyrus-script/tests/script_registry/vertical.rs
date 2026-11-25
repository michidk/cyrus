use super::*;

#[test]
fn ordinary_vertical_motion_retains_byte_timer_signed_limit_and_q8_origin() {
    use cyrus_script::script::{
        OrdinaryVerticalInputs, OrdinaryVerticalState, ordinary_falling_vertical,
        ordinary_queued_vertical,
    };
    let input = OrdinaryVerticalInputs {
        factor: 256,
        advanced_factor: 256,
        fine_ticks: 8,
        additional_ticks: 0,
        post_collide_height: 50,
        model_controls_vertical: false,
    };
    let state = OrdinaryVerticalState {
        y: 9000,
        origin_y: 10000,
        timer: 30,
        queued: JumpMotion::from_byte(129),
        falling: JumpMotion::NONE,
    };
    assert_eq!(
        ordinary_queued_vertical(state, 32, input),
        OrdinaryVerticalState {
            y: 1808,
            timer: 38,
            queued: JumpMotion::NONE,
            falling: JumpMotion::from_byte(129),
            ..state
        }
    );
    assert_eq!(
        ordinary_falling_vertical(state, 32, input),
        OrdinaryVerticalState {
            y: 1808,
            timer: 22,
            ..state
        }
    );
    assert_eq!(
        ordinary_falling_vertical(
            OrdinaryVerticalState { timer: 0, ..state },
            32,
            OrdinaryVerticalInputs {
                factor: 16,
                ..input
            }
        ),
        OrdinaryVerticalState {
            y: 14120,
            timer: 0,
            ..state
        }
    );
    assert_eq!(
        ordinary_falling_vertical(
            OrdinaryVerticalState {
                timer: 128,
                ..state
            },
            255,
            OrdinaryVerticalInputs {
                factor: 16,
                ..input
            }
        )
        .y,
        34344
    );
    assert_eq!(
        ordinary_falling_vertical(
            state,
            32,
            OrdinaryVerticalInputs {
                model_controls_vertical: true,
                ..input
            }
        ),
        state
    );
    assert_eq!(
        ordinary_queued_vertical(
            state,
            32,
            OrdinaryVerticalInputs {
                model_controls_vertical: true,
                ..input
            }
        ),
        state
    );
    assert_eq!(
        ordinary_queued_vertical(
            OrdinaryVerticalState {
                timer: 250,
                ..state
            },
            32,
            OrdinaryVerticalInputs {
                advanced_factor: 255,
                ..input
            }
        )
        .timer,
        2
    );
    assert_eq!(
        ordinary_queued_vertical(
            state,
            32,
            OrdinaryVerticalInputs {
                advanced_factor: 255,
                ..input
            }
        )
        .queued,
        JumpMotion::from_byte(129)
    );
    assert_eq!(
        ordinary_falling_vertical(OrdinaryVerticalState { timer: 1, ..state }, 32, input).timer,
        249
    );
    assert_eq!(
        ordinary_falling_vertical(
            OrdinaryVerticalState {
                timer: 0,
                y: i32::MAX,
                ..state
            },
            1,
            OrdinaryVerticalInputs {
                factor: 16,
                fine_ticks: 0,
                ..input
            }
        )
        .y,
        i32::MIN + 31
    );
}

#[test]
fn ordinary_vertical_writers_commit_logical_y_without_moving_model_record() {
    use cyrus_script::script::OrdinaryVerticalInputs;
    let mut scripts = cache();
    scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
    let (mut world, data, _) = static_model_fixture(scripts);
    let id = world.id(0).unwrap();
    world.refresh_model_presentations(&data, &[id]).unwrap();
    let before = world.spatial(id).unwrap().local.position;
    let record = world.model_presentation(id).unwrap().unwrap().pose.position;
    let input = OrdinaryVerticalInputs {
        factor: 16,
        advanced_factor: 32,
        fine_ticks: 8,
        additional_ticks: 0,
        post_collide_height: 50,
        model_controls_vertical: false,
    };
    world.apply_ordinary_falling_vertical(id, input).unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [before[0], before[1] + 5120, before[2]]
    );
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        record
    );
    world
        .apply_ordinary_queued_vertical(id, input, &data, 0, &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(
        world.spatial(id).unwrap().local.position,
        [before[0], -512, before[2]]
    );
    assert_eq!(
        world.actors()[id.index].control_state().unwrap().fall_timer,
        8
    );
    assert_eq!(
        world.model_presentation(id).unwrap().unwrap().pose.position,
        record
    );
}

#[test]
fn ordinary_vertical_factor_resolution_obeys_signed_timer_and_read_order() {
    use cyrus_script::script::{OrdinaryVerticalInputs, ordinary_vertical_factor};
    assert_eq!(ordinary_vertical_factor(0).unwrap(), 16);
    assert_eq!(ordinary_vertical_factor(3).unwrap(), 63);
    assert_eq!(ordinary_vertical_factor(14).unwrap(), 192);
    assert_eq!(ordinary_vertical_factor(28).unwrap(), 255);
    for index in 29..70 {
        assert_eq!(ordinary_vertical_factor(index).unwrap(), 256);
    }
    assert!(ordinary_vertical_factor(-1).is_err());
    assert!(ordinary_vertical_factor(70).is_err());
    let queued = OrdinaryVerticalInputs::for_queued(24, 8, 0, 50, false).unwrap();
    assert_eq!((queued.factor, queued.advanced_factor), (248, 256));
    let falling = OrdinaryVerticalInputs::for_falling(128, 8, 0, 50, false).unwrap();
    assert_eq!(falling.factor, 16);
    assert_eq!(
        OrdinaryVerticalInputs::for_falling(30, 8, 0, 50, false)
            .unwrap()
            .factor,
        256
    );
    assert!(OrdinaryVerticalInputs::for_queued(128, 8, 0, 50, false).is_err());
    assert!(OrdinaryVerticalInputs::for_queued(64, 8, 0, 50, false).is_err());
    assert!(OrdinaryVerticalInputs::for_queued(128, 8, 0, 50, true).is_ok());
    assert!(OrdinaryVerticalInputs::for_falling(70, 8, 0, 50, true).is_ok());
}
