//! Recovered rules for the ordinary support response and the jump routine in the ledge and
//! hanging living states: the ledge states start no fall, the bounce states take the ordinary
//! fall start, a nonselected actor hanging only clears its stable-position flag, and the selected
//! player's jump needs the ledge services' inputs.
use super::*;
use cyrus_script::script::{LivingState, ObstacleEnvironment};

fn environment(
    selected_player: Option<cyrus_script::script::ScriptActorId>,
) -> ObstacleEnvironment {
    ObstacleEnvironment {
        selected_player,
        auto_grab: false,
        item_system_ready: false,
        shell_execution: false,
        ticks: 0,
    }
}

#[test]
fn the_ledge_living_states_start_no_static_fall_and_the_bounce_states_start_the_ordinary_one() {
    for state in 10..=32u8 {
        let mut scripts = cache();
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::JumpHeight] = 32;
        let (mut world, data, _) = static_model_fixture(scripts);
        let id = world.id(0).unwrap();
        let y = world.spatial(id).unwrap().local.position[1];
        world.set_living_state_for_test(id, LivingState::from_byte(state));
        let started = world.begin_static_ordinary_fall(id, &data).unwrap();
        let control = world.actors()[id.index].control_state().unwrap();
        if (10..=19).contains(&state) {
            assert!(!started, "living state {state}");
            assert_eq!(
                control.falling_state,
                JumpMotion::NONE,
                "living state {state}"
            );
            assert_eq!(control.fall_timer, 0, "living state {state}");
        } else {
            assert!(started, "living state {state}");
            assert_eq!(
                (
                    control.fall_origin_y,
                    control.fall_timer,
                    control.falling_state
                ),
                (y.wrapping_add(8192), 30, JumpMotion::STANDING),
                "living state {state}"
            );
        }
    }
}

#[test]
fn a_nonselected_actor_hanging_only_clears_its_stable_position_flag_on_a_jump() {
    let (mut world, data, _) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    assert!(world.retain_ordinary_stable_position(id).unwrap());
    world.set_living_state_for_test(id, LivingState::HANGING);
    let position = world.spatial(id).unwrap().local.position;
    let started = world
        .begin_ordinary_jump(id, &data, 8, false, environment(None), None, &mut |_, _| {
            Ok(())
        })
        .unwrap();
    let control = world.actors()[id.index].control_state().unwrap();
    assert!(!started);
    assert!(!control.stable_position_valid);
    assert_eq!(control.living_state, LivingState::HANGING);
    assert_eq!(control.queued_motion, JumpMotion::NONE);
    assert_eq!(world.spatial(id).unwrap().local.position, position);
}

#[test]
fn the_selected_players_ordinary_jump_entry_names_the_missing_edge_inputs() {
    let (mut world, data, _) = static_model_fixture(cache());
    let id = world.id(0).unwrap();
    let error = world
        .begin_ordinary_jump(
            id,
            &data,
            8,
            false,
            environment(Some(id)),
            None,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("edge geometry and clock inputs"),
        "{error:#}"
    );
}

#[test]
fn script_jump_admission_blocks_only_the_four_listed_groups() {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    // The original's group block tests a range whose bounds cannot both hold, so only the listed
    // groups refuse (preservation: Script Jump admission); the fall and fight groups in between
    // admit the jump like any other group.
    for (group, admitted) in [
        (27, true),
        (32, true),
        (33, true),
        (34, true),
        (35, true),
        (111, true),
        (114, false),
        (116, false),
        (130, false),
        (131, false),
    ] {
        let (mut world, _, _) = script_jump_fixture(group, |_| {});
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
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
        let control = world.actors()[0].control_state().unwrap();
        assert_eq!(
            control.queued_motion != JumpMotion::NONE,
            admitted,
            "group {group}"
        );
    }
}
