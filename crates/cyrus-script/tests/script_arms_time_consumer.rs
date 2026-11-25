//! Arms timer updates preserve directional actions and consume typed motion bytes.
use cyrus_assets::script::PlacementType;
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use cyrus_soup::{Attribute, Thread};

mod support;
use support::cache;

fn assign(destination: Place, first: Value) -> Operation {
    Operation::Assign {
        destination,
        formula: Formula {
            first,
            rest: vec![],
        },
    }
}
fn fixture(
    timer: i32,
    queued: i32,
    falling: i32,
    collide: u8,
) -> (ScriptWorld, cyrus_assets::animation::Animations) {
    let mut scripts = cache();
    scripts.instances.truncate(1);
    scripts.instances[0].object_type = PlacementType::MODEL;
    scripts.instances[0].active = 1;
    scripts.instances[0].map_position = [600, 20, 600];
    scripts.updates.animation_system = false;
    scripts.definition.references = vec![
        cyrus_soup::Property::ArmsTime,
        cyrus_soup::Property::JumpFlag,
        cyrus_soup::Property::FallFlag,
    ];
    scripts.definition.functions.push(Function {
        name: cyrus_soup::Native::SetAction,
        param_count: 1,
        kind: CallKind::Function,
    });
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.instances = 1;
    actor.instance_variables.truncate(1);
    actor.attributes[Attribute::Collide] = collide;
    actor.attributes[Attribute::FallHold] = 1;
    let place = |reference| Place::Property {
        object: Object::Me,
        reference,
    };
    let ops = vec![
        Operation::Call {
            call: Call {
                function_id: 1,
                function: cyrus_soup::Native::SetAction,
                target: Object::Me,
                kind: CallKind::Function,
                arguments: vec![Value::Integer { value: 1 }],
            },
        },
        assign(place(0), Value::Integer { value: timer }),
        assign(place(1), Value::Integer { value: queued }),
        assign(place(2), Value::Integer { value: falling }),
        assign(Place::Local { index: 0 }, Value::Read { place: place(0) }),
        assign(Place::Flag { index: 0 }, Value::Integer { value: 73 }),
        Operation::End { target: 4 },
    ];
    actor.program.steps = ops
        .into_iter()
        .enumerate()
        .map(|(i, operation)| {
            (
                i as u32,
                Step {
                    next: i as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
    scripts.content_id = scripts.content_identity().unwrap();
    let data = cyrus_assets::animation::Animations {
        version: 2,
        world: 7,
        metadata: serde_json::json!({"script_content_id":scripts.content_id,"actors":[{
            "script_name":"LOGIC","animation_groups":([0,18].map(|group|serde_json::json!({"group_index":group,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]})))
        }]}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let mut world = ScriptWorld::prepare(&scripts, 7).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    world
        .bind_animation(id, &data, cyrus_soup::AnimationGroup::RUN)
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Animation,
            ScriptInputs {
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            20,
        )
        .unwrap();
    world
        .advance(
            id,
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                animations: Some(cyrus_script::script::AnimationData(&data)),
                nominal_ticks: Some(0),
                frame_scale: Some(256),
                ..Default::default()
            },
            None,
            None,
            30,
        )
        .unwrap();
    assert_eq!(world.actors()[0].control_state().unwrap().actions, [3, 0]);
    assert_eq!(world.actors()[0].runtime().unwrap().variables(), [timer]);
    (world, data)
}

#[test]
fn raised_motion_bytes_drive_early_clear_and_wrapping_timer_before_later_failure() {
    for (timer, queued, falling, ticks, remaining, next_queued, next_falling) in [
        (0, 129, 128, 1, 0, 129, 128),
        (2, 0, 129, 0, 0, 0, 129),
        (2, 129, 0, 0, 0, 129, 0),
        (2, 129, 128, 1, 1, 1, 0),
        (3, 0, 129, 1, 2, 0, 1),
        (6, 129, 129, 2, 4, 1, 1),
        (3, 129, 129, 6, 0, 1, 1),
        (1, 129, 129, 255, 2, 1, 1),
        (255, 0, 129, 1, 0, 0, 129),
        (255, 129, 129, 128, 127, 1, 1),
        (128, 129, 129, 1, 127, 1, 1),
        (6, 129, 129, 258, 4, 1, 1),
    ] {
        let (mut world, data) = fixture(timer, queued, falling, 4);
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
                    scene: None,
                },
                &mut cyrus_game::random::EngineRandom::default(),
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        let actor = &world.actors()[0];
        assert!(
            actor
                .failure()
                .unwrap()
                .contains("ordinary collision scene unavailable"),
            "{:?}",
            actor.failure()
        );
        let c = actor.control_state().unwrap();
        assert_eq!(c.action_transition_counter, remaining);
        assert_eq!(c.queued_motion.byte(), next_queued);
        assert_eq!(c.falling_state.byte(), next_falling);
        assert_eq!(c.actions, [3, 0]);
        assert_eq!(actor.runtime().unwrap().variables(), [timer]);
        let id = world.id(0).unwrap();
        assert!(
            world
                .advance(
                    id,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs::default(),
                    None,
                    None,
                    30
                )
                .is_err()
        );
        assert_eq!(world.flags(), [73]);
    }
}

#[test]
fn high_bit_only_motion_clears_before_expiry_and_public_body_continues() {
    let (mut world, data) = fixture(2, 128, 128, 0);
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
    let actor = &world.actors()[0];
    assert!(actor.failure().is_none(), "{:?}", actor.failure());
    let c = actor.control_state().unwrap();
    assert_eq!(c.action_transition_counter, 1);
    assert_eq!(c.queued_motion.byte(), 0);
    assert_eq!(c.falling_state.byte(), 0);
    assert_eq!(c.actions, [3, 0]);
    assert_eq!(actor.runtime().unwrap().variables(), [1]);
    assert_eq!(world.flags(), [73]);
}

#[test]
fn missing_clock_input_refuses_before_timer_and_motion_changes() {
    let (mut world, data) = fixture(6, 129, 129, 4);
    let error = world
        .update_ordinary(
            &data,
            [600, 600],
            cyrus_script::script::OrdinaryInputs {
                scripts: ScriptInputs {
                    nominal_ticks: Some(2),
                    ..Default::default()
                },
                scene: None,
            },
            &mut cyrus_game::random::EngineRandom::default(),
            &mut |_, _| Ok(()),
            100,
        )
        .unwrap_err();
    assert!(error.to_string().contains("requires frame_scale"));
    let c = world.actors()[0].control_state().unwrap();
    assert_eq!(c.action_transition_counter, 6);
    assert_eq!(c.queued_motion.byte(), 129);
    assert_eq!(c.falling_state.byte(), 129);
    assert_eq!(c.actions, [3, 0]);
}
